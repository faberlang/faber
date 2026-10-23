//! Typed device-program plan mapping and launch descriptors.
//!
//! This module is the fail-closed `MirCollectionOp → CollectionKernelPlan`
//! bridge: it maps a collection intrinsic plus the resolved dispatch contract
//! onto the closed recipe set, re-derives that plan from a kernel function's
//! typed facts, and builds backend-neutral launch descriptors from a
//! [`DeviceProgram`].
//!
//! Nothing here reads emitted text. An op with no recipe is a typed
//! [`PlanMappingError`], never a silent [`CollectionKernelPlan::Elementwise`]
//! floor. Mixed-recipe bodies belong to
//! [`crate::kernel_decomposition::decompose_kernel_function`].

use crate::abi::{
    CollectionOpContract, DEFAULT_STORAGE_BUFFER_LIMIT, MatMulDispatch, MirAbiError,
    MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole,
    MirKernelSignature, PackedStorageLayout, StorageBufferLimitRecord, collection_op_contract,
    device_view_element_count, device_view_element_count_or_runtime, storage_buffer_limit_error,
};
use crate::device_plan_compiler::LibraryOperation;
use crate::device_program::{
    Binding, BufferRole, DeviceProgram, DeviceProgramError, DeviceResource, KernelUnit,
};
use crate::kernel_plan::{
    BatchedAttentionKind, CollectionKernelPlan, MatMulShapes, SharedMemoryLayout,
};
use crate::layout::MirTensorStorageLayout;
use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirIntrinsic, MirKernelDispatchSize,
    MirKernelWorkgroupCount, MirLayoutKind, MirLocalId, MirOperand, MirPlaceBase, MirRuntimeCall,
    MirStatementKind, MirType, MirUnOp, MirValidationContext, MirValueKind, MirWorkgroupSize,
};
use radix_types::{NumericWidth, Primitive, Type, TypeId};
use rustc_hash::FxHashMap;
use std::fmt;

/// A typed plan-mapping failure (contract resolution, operand shapes, or a
/// recipe mismatch). Carries a stable message; the emitters map it onto their
/// own fail-closed diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMappingError {
    /// Human-readable failure detail.
    pub message: String,
}

impl PlanMappingError {
    pub(crate) fn mapping(shape: impl Into<String>) -> Self {
        Self {
            message: shape.into(),
        }
    }
}

impl fmt::Display for PlanMappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PlanMappingError {}

impl From<MirAbiError> for PlanMappingError {
    fn from(error: MirAbiError) -> Self {
        Self::mapping(error.message)
    }
}

// ---------------------------------------------------------------------------
// PLAN MAPPING
// ---------------------------------------------------------------------------

/// The typed `MirCollectionOp → CollectionKernelPlan` mapping (council TB5).
///
/// Maps a collection intrinsic plus the resolved dispatch contract onto the
/// closed recipe set through the shared plan pass
/// ([`CollectionKernelPlan::from_contract`]).
///
/// **Fail-closed (N3.2):** an op that has no recipe under the resolved
/// contract is a [`PlanMappingError`], never a silent
/// [`CollectionKernelPlan::Elementwise`] floor. Elementwise kernels are
/// decided by the function-level scan ([`kernel_plan_for_function`]) from
/// the body's complete op set; this op-level mapping admits recipe ops only
/// — an elementwise op under a recipe contract is a dispatch/op mismatch and
/// fails closed here too.
///
/// The caller must already hold the resolved [`CollectionOpContract`] (the
/// sibling of the plan pass; `abi::collection_op_contract`) and, for a
/// `TensorMatMul` op, the exact operand shapes `(M, K, N)`; the contract
/// stops at the tile grid, the recipe needs the exact dims.
///
/// # Errors
/// Returns [`PlanMappingError`] when the op has no recipe under the resolved
/// contract (unplannable collection op, or an elementwise op under a recipe
/// contract).
pub fn plan_for_collection_op(
    signature: &MirKernelSignature,
    contract: &CollectionOpContract,
    op: MirCollectionOp,
    matmul_shapes: Option<MatMulShapes>,
) -> Result<CollectionKernelPlan, PlanMappingError> {
    CollectionKernelPlan::from_contract(signature, contract, op, matmul_shapes).map_or_else(
        || {
            Err(PlanMappingError::mapping(format!(
                "collection op {op:?} has no kernel plan under the resolved {contract:?} contract \
                     (no recipe; elementwise kernels are decided by the function-level scan, \
                     never by a silent Elementwise fallback)"
            )))
        },
        Ok,
    )
}

/// Re-derive the collection-op plan from a kernel function's typed facts.
///
/// Runs the same typed inputs the shared plan pass consumes — the **full**
/// statement scan's collection-op set (never the first matching statement),
/// the ABI dispatch contract, and (for matmul) the operand dims resolved
/// from the operands' tensor types — and returns the plan the function
/// carries, or `None` for a kernel whose op set is elementwise-only (no
/// recipe).
///
/// The emitters use this to cross-check a [`DeviceProgram`] kernel's **carried**
/// plan against the MIR function it was built from: a carried recipe that
/// disagrees with the function's typed op facts fails closed (A3) instead of
/// emitting a body that contradicts the declared plan.
///
/// # Errors
/// Returns [`PlanMappingError`] when the contract resolver cannot resolve the
/// operand shapes (rank, static dims, inner-dimension agreement), when a
/// recipe op has no resolved contract or no plan under it, when a body mixes
/// collection ops with conflicting recipes, or when the body carries a
/// collection op that is neither recipe-plannable nor a known elementwise
/// transform: **never a silent `Elementwise`.**
///
/// `TensorTranspose` is a recipe op (S5-U1); the S6-P1 transformer recipes
/// (`TensorSumAxis` / `TensorSoftmax` / `TensorLayerNorm`) resolve through
/// [`CollectionKernelPlan::from_transformer_op`] (no `CollectionOpContract`
/// variant exists for them). A body whose complete op set MIXES distinct
/// recipes cannot carry one plan and fails here with a typed diagnostic —
/// the decomposition pass
/// ([`crate::kernel_decomposition::decompose_kernel_function`]) is the path
/// that splits such bodies into single-recipe subchain kernels.
pub fn kernel_plan_for_function(
    function: &MirFunction,
    signature: &MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionKernelPlan>, PlanMappingError> {
    kernel_plan_for_function_with_limit(
        function,
        signature,
        validation,
        DEFAULT_STORAGE_BUFFER_LIMIT,
    )
}

/// Re-derive a kernel plan against an explicit lane-owned storage-buffer
/// capacity. The default wrapper keeps the conservative shared minimum.
///
/// # Errors
/// Returns [`PlanMappingError`] when the signature exceeds the selected lane
/// limit or its typed recipe facts do not resolve.
pub fn kernel_plan_for_function_with_limit(
    function: &MirFunction,
    signature: &MirKernelSignature,
    validation: &MirValidationContext<'_>,
    limit: StorageBufferLimitRecord,
) -> Result<Option<CollectionKernelPlan>, PlanMappingError> {
    for statement in function.blocks.iter().flat_map(|block| &block.statements) {
        if let MirStatementKind::RuntimeCall { call, .. } = &statement.kind
            && call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::TensorExpand)
            && crate::tensor_expansion::TensorExpansion::for_call(function, call, validation)
                .is_none()
        {
            return Err(PlanMappingError::mapping(
                "tensor expansion has no exact compatible static tensor witness",
            ));
        }
    }
    // A kernel whose storage-buffer binding count exceeds the per-lane
    // `StorageBufferLimitRecord` capacity cannot be emitted as ONE kernel
    // (the shared default is the minimum across admitted lanes). The whole-function plan is refused
    // with a typed diagnostic so the callers (faber, both emitters) fall
    // through to the decomposition path, which splits the body into
    // budget-fitting kernels — never a silent Elementwise floor.
    let buffer_count = signature
        .resources()
        .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
        .count();
    if !limit.admits(buffer_count) {
        return Err(PlanMappingError::mapping(
            storage_buffer_limit_error(buffer_count, limit).message,
        ));
    }
    // A lane-form kernel (it carries the compiler-internal `Lane` builtin) is
    // one launch grid of independent scalar bodies; its `abs`/`maximum`/`sqrt`
    // collection-op statements are scalar math, not tensor recipes, so the
    // recipe scan does not apply. The caller derives the `LaneGrid` plan.
    if function
        .params
        .iter()
        .any(|param| param.mode == crate::MirParamMode::Lane)
    {
        return Ok(None);
    }
    // SFR-5 declared-recipe scan: a `FilumSum` declaration is the author-
    // stated recipe. The collection-op statement scan never sees it (the
    // declaration is not a collection op), so it resolves ahead of the op
    // scan; the source length `K` comes from the kernel signature's source
    // buffer — never guessed from the body. A second declared or recognized
    // recipe in the same body fails closed (one kernel, one recipe).
    //
    // The declared recipe resolves BEFORE the single-lane classification:
    // `FilumReduce` is not a `MirCollectionOp`, so `is_single_lane_form`
    // cannot see it, and a sequential single-lane early return here would
    // swallow the recipe (SFR-5).
    let filum_declarations = filum_sum_declarations(function);
    if !filum_declarations.is_empty() {
        if filum_declarations.len() > 1 {
            return Err(PlanMappingError::mapping(
                "kernel body carries more than one filum summa declaration (one recipe per kernel)",
            ));
        }
        let (filum, source_local) = &filum_declarations[0];
        let length = signature
            .resources()
            .find(|resource| resource.source_local == Some(*source_local))
            .map(|resource| resource.element_count)
            .ok_or_else(|| {
                PlanMappingError::mapping(format!(
                    "filum summa source local {source_local:?} has no storage-buffer resource in the kernel signature"
                ))
            })?;
        let plan = CollectionKernelPlan::from_filum_summa(filum, length).ok_or_else(|| {
            PlanMappingError::mapping(format!(
                "filum summa declaration has no kernel plan under its typed facts \
                 (lane width {}, source length {length})",
                filum.lane_width
            ))
        })?;
        for op in crate::collection_ops::collection_ops_for_function(function) {
            if is_contract_recipe_op(op) || is_transformer_recipe_op(op) {
                return Err(PlanMappingError::mapping(format!(
                    "filum summa kernel body mixes a second recipe ({op:?}) with the declared distributed sum"
                )));
            }
        }
        return Ok(Some(plan));
    }
    // A single-lane kernel (no lane loop, one `mut` tensor) is a sequential
    // body: its collection-op statements are scalar math too. The caller
    // selects the `SingleLane` plan.
    if MirKernelSignature::is_single_lane_form(function, validation) {
        return Ok(None);
    }
    // Live-recipe-statement census: the op scan below is deduplicated by op
    // kind, so two LIVE statements of the SAME recipe op would adopt ONE
    // plan by plan-equality — silently folding a multi-recipe body into a
    // single kernel whose matmul shapes read only the first statement. Fail
    // closed instead: one kernel, one recipe statement. Distinct-op mixes
    // stay with the scan below, whose `adopt_recipe` owns the pinned
    // "conflicting recipes" diagnostic.
    let stability_max_dead = crate::abi::stability_max_is_dead(function);
    let live_recipes = live_recipe_statements(function, stability_max_dead);
    if live_recipes.len() > 1 && live_recipes.iter().all(|op| *op == live_recipes[0]) {
        return Err(PlanMappingError::mapping(format!(
            "kernel body carries {} live {:?} recipe statements \
             (one recipe statement per kernel)",
            live_recipes.len(),
            live_recipes[0],
        )));
    }
    // Scan ALL statements: the kernel body's plan is decided by its complete
    // collection-op set, never by the first matching statement (N3.2).
    let ops = crate::collection_ops::collection_ops_for_function(function);
    if ops.is_empty() {
        return Ok(None);
    }
    // The ABI dispatch contract (reduction / matmul), when the body carries a
    // recipe op; elementwise-only bodies resolve no contract. Matmul shapes
    // are resolved once from the body's matmul statement scan.
    let contract = collection_op_contract(function, validation)?;
    // Run-time extents (`MatMulDispatch::Runtime`) carry no literal shapes:
    // the plan is built from the signature alone and the dims are read at
    // launch.
    let matmul_shapes = if matches!(
        contract,
        Some(CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime
        })
    ) {
        None
    } else {
        kernel_matmul_shapes(function, validation)?
    };
    resolve_recipe_for_ops(
        ops,
        signature,
        function,
        validation,
        contract,
        matmul_shapes,
        stability_max_dead,
    )
}

fn resolve_recipe_for_ops(
    ops: Vec<MirCollectionOp>,
    signature: &MirKernelSignature,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    contract: Option<CollectionOpContract>,
    matmul_shapes: Option<MatMulShapes>,
    stability_max_dead: bool,
) -> Result<Option<CollectionKernelPlan>, PlanMappingError> {
    let mut recipe: Option<CollectionKernelPlan> = None;
    for op in ops {
        // PGC-R5: a DEAD stability-max reduction is not a recipe — the
        // same dead skip the contract resolver applies (the `_stability_max`
        // idiom of the causal-softmax entries; the emitters' dead skips are
        // the precedent).
        if stability_max_dead
            && matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin)
        {
            continue;
        }
        if is_transformer_recipe_op(op) {
            // S6-P1: the transformer recipes have no `CollectionOpContract`
            // variant; the plan pass resolves them from the function's typed
            // facts (axis/epsilon from the trailing const operands, dims
            // from the operand tensor types) and cross-checks the signature's
            // element counts. A second distinct recipe must agree exactly
            // (never guessed).
            let plan =
                CollectionKernelPlan::from_transformer_op(signature, function, validation, op)
                    .ok_or_else(|| {
                        PlanMappingError::mapping(format!(
                            "collection op {op:?} has no kernel plan under its typed facts \
                             (axis/epsilon from the const operands, shapes from the tensor types)"
                        ))
                    })?;
            adopt_recipe(&mut recipe, op, plan)?;
        } else if is_contract_recipe_op(op) {
            // Contract recipes resolve through the shared closed-set
            // predicate — the same boundary the decomposition slice uses; a
            // second distinct recipe must agree exactly (never guessed).
            let Some(contract) = &contract else {
                return Err(PlanMappingError::mapping(format!(
                    "collection op {op:?} has a recipe but no resolved dispatch contract"
                )));
            };
            let shapes = if op == MirCollectionOp::TensorMatMul {
                matmul_shapes
            } else {
                None
            };
            let plan = CollectionKernelPlan::from_contract(signature, contract, op, shapes)
                .ok_or_else(|| {
                    PlanMappingError::mapping(format!(
                        "collection op {op:?} has no kernel plan under the resolved {contract:?} contract"
                    ))
                })?;
            adopt_recipe(&mut recipe, op, plan)?;
        } else if is_elementwise_collection_op(op) {
            // Elementwise transforms contribute no recipe; the function-level
            // scan decides them elementwise.
        } else {
            return Err(PlanMappingError::mapping(format!(
                "collection op {op:?} has no kernel plan \
                 (no recipe and not an elementwise transform)"
            )));
        }
    }
    Ok(recipe)
}

/// The body's LIVE recipe statements in scan order (the F2 census) —
/// statement level, never deduplicated by op kind the way the op scan is.
/// A recipe statement whose destination is never read is dead (the same
/// dead filter `collection_ops_with_read_context` applies), and a PGC-R5
/// dead stability-max `TensorMax`/`TensorMin` is not a recipe (the same
/// skip the op scan applies).
fn live_recipe_statements(
    function: &MirFunction,
    stability_max_dead: bool,
) -> Vec<MirCollectionOp> {
    let mut recipes = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            let MirStatementKind::RuntimeCall { destination, .. } = &statement.kind else {
                continue;
            };
            if stability_max_dead
                && matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin)
            {
                continue;
            }
            if !is_recipe_op(op) {
                continue;
            }
            if destination
                .as_ref()
                .is_some_and(|dest| !crate::read_analysis::place_is_read(function, dest))
            {
                continue;
            }
            recipes.push(op);
        }
    }
    recipes
}

/// The kernel body's `FilumSum` declarations (SFR-5): each entry is the
/// declaration facts plus the source local its lone tensor argument reads.
/// The caller admits exactly one per kernel (one recipe per kernel).
fn filum_sum_declarations(function: &MirFunction) -> Vec<(crate::MirFilumReduceOp, MirLocalId)> {
    let mut declarations = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::FilumReduce(filum),
                        args,
                        ..
                    },
                ..
            } = &statement.kind
            else {
                continue;
            };
            if filum.form != crate::MirFilumForm::Declared {
                continue;
            }
            let Some(MirOperand::Place(place)) = args.first() else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let MirPlaceBase::Local(source_local) = place.base else {
                continue;
            };
            declarations.push((filum.clone(), source_local));
        }
    }
    declarations
}

fn adopt_recipe(
    recipe: &mut Option<CollectionKernelPlan>,
    op: MirCollectionOp,
    plan: CollectionKernelPlan,
) -> Result<(), PlanMappingError> {
    if let Some(existing) = recipe.as_ref()
        && existing != &plan
    {
        return Err(PlanMappingError::mapping(format!(
            "kernel body mixes collection ops with conflicting recipes \
                 ({op:?} resolves to {plan:?}, but the body already carries {existing:?})"
        )));
    }
    *recipe = Some(plan);
    Ok(())
}

// ---------------------------------------------------------------------------
// CARRIED-FACT AGREEMENT
// ---------------------------------------------------------------------------

/// Whether a device program's carried plan agrees with the plan re-derived
/// from the kernel function's typed facts.
///
/// `derived == None` (no recipe on the function) agrees only with the
/// no-recipe [`CollectionKernelPlan::Elementwise`] carried plan; a carried
/// recipe must equal the derived plan exactly.
#[must_use]
pub fn carried_plan_agrees(
    carried: &CollectionKernelPlan,
    derived: Option<&CollectionKernelPlan>,
) -> bool {
    match derived {
        // A lane-form function derives no collection plan; the constructor
        // selects `LaneGrid` from its Lane parameter (`lane_grid_plan`).
        None => matches!(
            carried,
            CollectionKernelPlan::Elementwise
                | CollectionKernelPlan::LaneGrid { .. }
                | CollectionKernelPlan::SingleLane
                | CollectionKernelPlan::GridReduce { .. }
        ),
        Some(derived) => derived == carried,
    }
}

fn carried_resources_agree_impl(
    carried: &[DeviceResource],
    signature: &MirKernelSignature,
    allow_extra: bool,
) -> bool {
    let mut expected: FxHashMap<(u32, u32), &MirKernelResource> = FxHashMap::default();
    for resource in signature
        .resources()
        .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
    {
        expected.insert((resource.group, resource.binding), resource);
    }
    for resource in carried {
        let key = (resource.binding.group, resource.binding.binding);
        let Some(signature_resource) = expected.remove(&key) else {
            if !allow_extra {
                return false;
            }
            // A binding outside the carrier signature is a host-dispatch
            // fact of the library body.
            continue;
        };
        if !carried_slot_agrees(resource, signature_resource) {
            return false;
        }
    }
    expected.is_empty()
}

/// Whether a device program's carried resource list agrees with the
/// signature re-derived from the kernel function's typed facts.
///
/// The ABI role/access pair must be compatible with the carried program
/// role. Exact agreement holds for per-kernel roles; a **unified**
/// intermediate (S2-5) carries the program role `InOut` on slots whose own
/// ABI is (Output, Write) or (Input, Read) — the program role is a
/// cross-kernel identity fact (N2.6 data-flow identity), not a per-slot
/// derivation. `InOut` accepts any coherent ABI pair; incoherent pairs (e.g.
/// writing an input) still fail closed because `from_abi_role` yields
/// `None` for them.
#[must_use]
pub fn carried_resources_agree(carried: &[DeviceResource], signature: &MirKernelSignature) -> bool {
    carried_resources_agree_impl(carried, signature, false)
}

/// Whether a library-dispatch kernel's carried resource list agrees with the
/// carrier signature re-derived from the kernel function's typed facts.
///
/// A fused library dispatch (M2 library v0) carries the COMPLETE
/// host-dispatch resource set on the one launch — e.g. a `QkvProjection`
/// launch binds the K/V weights and outputs alongside the carrier ABI slots
/// for the host library dispatcher. Every signature slot must agree under
/// the same per-slot law as [`carried_resources_agree`]; carried resources
/// on bindings outside the signature are host-dispatch facts of the library
/// body, not kernel ABI slots, and no signature slot may be missing.
#[must_use]
pub fn carried_resources_agree_for_library_dispatch(
    carried: &[DeviceResource],
    signature: &MirKernelSignature,
) -> bool {
    carried_resources_agree_impl(carried, signature, true)
}

/// One carried slot against one signature slot — the shared per-slot law of
/// both agreement predicates (role/access coherence plus exact typed facts).
fn carried_slot_agrees(carried: &DeviceResource, signature: &MirKernelResource) -> bool {
    let Some(derived_role) = BufferRole::from_abi_role(signature.role, signature.access) else {
        return false;
    };
    (derived_role == carried.buffer.role || carried.buffer.role == BufferRole::InOut)
        && signature.element_ty == carried.version.element_ty
        && signature.element_count == carried.version.element_count
        // The carried reduced-resource projection (council-8 CB-3) must
        // agree with the version's — a stale or dropped projection is a
        // carried-fact disagreement, never silently tolerated. A
        // signature that DERIVES a projection (the kernel's own body
        // contains the producing SumAxis) must carry the same fact as
        // the version. A signature that derives NO projection is a
        // cross-function consumer (the producing SumAxis lives in
        // another kernel's source); it cannot reconstruct the producer's
        // fact and trusts the carried version — the mapping is carried,
        // never re-derived by a consumer.
        && (signature.reduced_projection.is_none()
            || signature.reduced_projection == carried.version.reduced_projection)
}

/// Whether a kernel unit dispatches a library body whose launch may carry
/// host-dispatch resources beyond the carrier signature (the same
/// library-entry derivation the executor plan uses; a derived library entry
/// means the host dispatcher owns the body).
#[must_use]
pub fn dispatches_library_body(kernel: &KernelUnit) -> bool {
    library_entry_for_kernel(kernel, RecipeKind::from_plan(&kernel.plan)).is_some()
}

// ---------------------------------------------------------------------------
// LAUNCH DESCRIPTORS
// ---------------------------------------------------------------------------

/// The recipe family of a kernel, as a typed tag.
///
/// Backends and hosts switch on this tag to select launch semantics (grid
/// coverage, shared-memory presence); it is derived from the typed plan,
/// never from emitted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeKind {
    /// No collection-op recipe (elementwise kernel).
    Elementwise,
    /// Tiled-matmul recipe.
    TiledMatMul,
    /// Tree-reduction recipe.
    TreeReduction,
    /// Rank-2 transpose recipe (S5-U1).
    Transpose,
    /// Axis-reduction recipe (S6-P1): row-wise reduction over the declared
    /// axis (the transformer `TensorSumAxis` op).
    AxisReduction,
    /// Row-wise softmax recipe (S6-P1).
    RowSoftmax,
    /// Layer-normalization recipe (S6-P1).
    LayerNormalization,
    /// Row-gather recipe (GI3-1).
    Gather,
    /// Root-mean-square-normalization recipe (GI3-1).
    RmsNormalization,
    /// Rotary-position-embedding recipe (GI3-1).
    Rope,
    /// Causal masked-row softmax recipe (GI3-1).
    CausalMaskedSoftmax,
    /// Packed quantized GEMM recipe (R-PACK-01A).
    QuantizedMatMul,
    /// Grouped packed GEMM recipe (R-PACK-01A).
    GroupedMatMul,
    /// M3 head-batched attention recipe.
    BatchedAttention,
    /// SSM conv1d recipe (R-PACK-01A).
    SsmConv1d,
    /// SSM linear-recurrence scan recipe (R-PACK-01A).
    SsmScan,
    /// Lane-form kernel (gpu-reset C4): one thread per lane of the domain.
    LaneGrid,
    /// Single-lane kernel (gpu-reset A2 rung 3): one thread.
    SingleLane,
    /// Whole-grid reduction (gpu-reset C4-8): two launches through a
    /// dispatcher-owned partials scratch.
    GridReduce,
}

impl RecipeKind {
    fn from_plan(plan: &CollectionKernelPlan) -> Self {
        match plan {
            CollectionKernelPlan::Elementwise => Self::Elementwise,
            CollectionKernelPlan::TiledMatMul(_) => Self::TiledMatMul,
            // SFR-5: the declared distributed sum's floor IS the tree recipe —
            // a backend with no native collective arm selects the tree-reduction
            // launch semantics here; native arms (SFR-6/7) match the plan variant
            // before consulting this tag.
            CollectionKernelPlan::TreeReduction(_) | CollectionKernelPlan::DistributedSum(_) => {
                Self::TreeReduction
            }
            CollectionKernelPlan::Transpose(_) => Self::Transpose,
            CollectionKernelPlan::AxisReduction(_) => Self::AxisReduction,
            CollectionKernelPlan::RowSoftmax(_) => Self::RowSoftmax,
            CollectionKernelPlan::LayerNormalization(_) => Self::LayerNormalization,
            CollectionKernelPlan::Gather(_) => Self::Gather,
            CollectionKernelPlan::RmsNormalization(_) => Self::RmsNormalization,
            CollectionKernelPlan::Rope(_) => Self::Rope,
            CollectionKernelPlan::CausalMaskedSoftmax(_) => Self::CausalMaskedSoftmax,
            CollectionKernelPlan::QuantizedMatMul(_) => Self::QuantizedMatMul,
            CollectionKernelPlan::GroupedMatMul(_) => Self::GroupedMatMul,
            CollectionKernelPlan::BatchedAttention(_) => Self::BatchedAttention,
            CollectionKernelPlan::SsmConv1d(_) => Self::SsmConv1d,
            CollectionKernelPlan::SsmScan(_) => Self::SsmScan,
            CollectionKernelPlan::LaneGrid { .. } => Self::LaneGrid,
            CollectionKernelPlan::SingleLane => Self::SingleLane,
            CollectionKernelPlan::GridReduce { .. } => Self::GridReduce,
        }
    }
}

/// Version of the additive executor-plan bind extension frozen by M0.
///
/// This extension carries library selection facts beside the existing
/// `DeviceProgram` shape. It is intentionally not a WIRE-v8 schema change:
/// callers that do not consume the extension continue to use the generated
/// kernel entry and its existing resource/launch facts.
pub const EXECUTOR_PLAN_BIND_ABI_VERSION: u16 = 1;

/// Uniform name for the packed GGML type id (M5-U2 / M6-U1 seam).
pub const PACKED_LAYOUT_GGML_TYPE_ID_UNIFORM: &str = "packed_layout_ggml_type_id";
/// Uniform name for packed block bytes.
pub const PACKED_BLOCK_BYTES_UNIFORM: &str = "packed_block_bytes";
/// Uniform name for packed block element count.
pub const PACKED_BLOCK_ELEMENTS_UNIFORM: &str = "packed_block_elements";
/// Uniform name for the native packed-region byte length.
pub const PACKED_BYTES_UNIFORM: &str = "packed_bytes";
/// Uniform name for decode GEMV selection (`1` when THIS step's row count M is 1).
pub const DECODE_GEMV_UNIFORM: &str = "decode_gemv";
/// Library entry for a packed GEMM body (M ≠ 1).
pub const QUANTIZED_GEMM_ENTRY: &str = "quantized_gemm";
/// Library entry for a packed GEMV body (M = 1 decode).
pub const QUANTIZED_GEMV_ENTRY: &str = "quantized_gemv";

/// CTO ruling `f083c057`: batched is the numeric baseline for decode
/// projections; M=1 GEMV selection stays off until PB-3 root-causes the
/// numeric divergence and proves identity. The GEMV bodies remain in the
/// tree; re-enabling is this one flag.
pub const DECODE_GEMV_SELECTION_ENABLED: bool = false;

/// Target-neutral layout family for a library body bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindLayout {
    /// A flat one-dimensional body.
    Flat,
    /// Contiguous row-major storage.
    RowMajor,
    /// A non-contiguous view described by the carried strides.
    Strided,
    /// `RoPE` consecutive-pair layout.
    RopeConsecutivePair { rotated_width: u64 },
    /// `RoPE` rotate-half layout.
    RopeRotateHalf { rotated_width: u64 },
}

/// Dimensions, strides, layout, and grid bound to one library body.
///
/// The descriptor is a typed executor-plan fact. Backends may lower it to
/// uniforms or native launch metadata, but they must not infer it from an
/// emitted kernel body or from a physical buffer length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindDescriptor {
    /// Logical dimensions in outer-to-inner order.
    pub dims: Vec<u64>,
    /// Physical element strides in the same order as [`Self::dims`].
    pub strides: Vec<u64>,
    /// Logical interpretation of the dimensions and strides.
    pub layout: BindLayout,
    /// Backend-neutral workgroup grid.
    pub grid: MirKernelWorkgroupCount,
}

impl BindDescriptor {
    fn row_major(dims: Vec<u64>, grid: MirKernelWorkgroupCount, layout: BindLayout) -> Self {
        let mut strides = vec![1u64; dims.len()];
        for axis in (0..dims.len()).rev().skip(1) {
            strides[axis] = strides[axis + 1].saturating_mul(dims[axis + 1]);
        }
        Self {
            dims,
            strides,
            layout,
            grid,
        }
    }

    /// Whether this descriptor has structurally coherent rank and extents.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        !self.dims.is_empty()
            && self.dims.len() == self.strides.len()
            && self.dims.iter().all(|dim| *dim > 0)
            && self.strides.iter().all(|stride| *stride > 0)
            && self.grid.x > 0
            && self.grid.y > 0
            && self.grid.z > 0
    }
}

/// Versioned scalar types accepted by library-call uniforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UniformType {
    /// IEEE-754 single precision value.
    F32,
    /// IEEE-754 half precision value, carried as its bit pattern.
    F16,
    /// Unsigned 32-bit integer.
    U32,
    /// Signed 32-bit integer.
    I32,
}

/// One constant or runtime uniform supplied to a library body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniformBinding {
    /// Stable logical uniform name.
    pub name: String,
    /// Uniform scalar type.
    pub ty: UniformType,
    /// Raw little-endian bits (4 bytes for all current uniform types).
    pub bits: u32,
}

/// One offset/stride fact for a bound view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetBinding {
    /// Logical view name (for example `q`, `k`, `v`, or `output`).
    pub name: String,
    /// Element offset from the base allocation.
    pub offset_elements: u64,
    /// Physical element strides in logical axis order.
    pub strides: Vec<u64>,
}

/// One typed buffer bind supplied to a library body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryBind {
    /// Stable logical view name.
    pub name: String,
    /// Device-program binding slot.
    pub binding: Binding,
    /// Logical dimensions of the view.
    pub dims: Vec<u64>,
    /// Physical element strides of the view.
    pub strides: Vec<u64>,
    /// Base element offset in the allocation.
    pub offset_elements: u64,
}

/// Optional scratch/workspace allocation required by a library body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceBinding {
    /// Required byte size.
    pub bytes: u64,
    /// Required byte alignment.
    pub alignment: u32,
}

/// A backend capability required by a selected library body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendRequirement {
    /// Backend identity, such as `metal`.
    pub backend: String,
    /// Capability or feature name.
    pub feature: String,
}

/// Provenance for a plan compiler selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanProvenance {
    /// Name of the pass that made the selection.
    pub pass: String,
    /// Revision of that pass's typed recognition rules.
    pub revision: u32,
    /// Canonical graph node identities consumed by the selection.
    pub source_nodes: Vec<u32>,
}

/// The logical signature selected independently of emitted kernel syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalSignature {
    /// Stable library operation name.
    pub operation: String,
    /// Logical input view names.
    pub inputs: Vec<String>,
    /// Logical output view names.
    pub outputs: Vec<String>,
}

/// One optional library selection and its launch bind facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySelection {
    /// Index into [`DeviceProgram`]`::kernels` or the compiled schedule.
    pub kernel_index: usize,
    /// The logical body selected by the executor. `None` retains the
    /// generated-kernel fallback for a recipe not yet owned by library v0.
    pub library_entry: Option<String>,
    /// The logical signature of the selected body.
    pub logical_signature: LogicalSignature,
    /// Primary shape/layout/grid facts supplied to the selected body.
    pub bind: BindDescriptor,
    /// All named view binds. `bind` is retained as the primary launch view.
    pub binds: Vec<LibraryBind>,
    /// Constant/runtime scalar uniforms.
    pub uniforms: Vec<UniformBinding>,
    /// Explicit base offsets and physical strides.
    pub offsets: Vec<OffsetBinding>,
    /// Optional scratch allocation.
    pub workspace: Option<WorkspaceBinding>,
    /// Backend requirements that must be admitted before dispatch.
    pub backend_requirements: Vec<BackendRequirement>,
    /// Compiler provenance for audit and cache identity.
    pub provenance: PlanProvenance,
}

/// M0-frozen additive executor-plan section for library calls. This section is
/// versioned independently of WIRE v8; an unknown version fails closed before
/// any selection field is interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorPlanExtension {
    /// Section schema version.
    pub abi_version: u16,
    /// Ordered selection/bind records, one per declared kernel.
    pub selections: Vec<LibrarySelection>,
}

impl ExecutorPlanExtension {
    /// Validate the section against the program kernel count before a consumer
    /// interprets any library selection.
    ///
    /// # Errors
    /// Returns a `String` detail when the section disagrees with the kernel count.
    pub fn validate_for_kernel_count(&self, kernel_count: usize) -> Result<(), String> {
        if self.abi_version != EXECUTOR_PLAN_BIND_ABI_VERSION {
            return Err(format!(
                "executor-plan section version {} is not supported (expected {})",
                self.abi_version, EXECUTOR_PLAN_BIND_ABI_VERSION
            ));
        }
        if self.selections.len() != kernel_count {
            return Err(format!(
                "executor-plan selection count {} does not match kernel count {}",
                self.selections.len(),
                kernel_count
            ));
        }
        for (position, selection) in self.selections.iter().enumerate() {
            if selection.kernel_index != position {
                return Err(format!(
                    "executor-plan kernel indices are not the ordered unique range at position {position}"
                ));
            }
            if !selection.bind.is_well_formed() {
                return Err(format!(
                    "executor-plan selection {position} has an invalid bind descriptor"
                ));
            }
            if selection.logical_signature.operation.is_empty()
                || selection.provenance.pass.is_empty()
            {
                return Err(format!(
                    "executor-plan selection {position} has incomplete signature/provenance"
                ));
            }
        }
        Ok(())
    }
}

/// Version of the executor-plan section used when launch bindings carry
/// runtime state. Static plans deliberately retain [`EXECUTOR_PLAN_BIND_ABI_VERSION`]
/// and the v1 shape above; dynamic plans use this version and the additive
/// allocation/view/binding facts below.
pub const EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION: u16 = 2;

/// Version of the additive speculative-verification record section.
///
/// The runtime-binding v2 payload remains wire-compatible for existing
/// consumers. Verification records are a new section at the next version so
/// an older consumer cannot default-fill `L`, `k`, generation, or result
/// fields and accidentally admit a different invocation.
pub const EXECUTOR_PLAN_VERIFICATION_ABI_VERSION: u16 = EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION + 1;

/// The layout of the ordered candidate-logits result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerificationLogitsLayout {
    /// Consecutive rows, each containing the complete vocabulary in order.
    RowMajor,
    /// A layout that is not the ordered row-major result contract.
    ColumnMajor,
}

impl VerificationLogitsLayout {
    /// Stable spelling used by diagnostics and identity canonicalization.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::RowMajor => "row_major",
            Self::ColumnMajor => "column_major",
        }
    }
}

/// Static model/output facts for one verification plan.
///
/// `prefix_before`, `query_rows`, generation, candidate ids, and transaction
/// identity deliberately do not appear here. They are invocation facts and
/// must not change the compiled-plan identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationPlanShape {
    /// The model's stable logical dimensions, excluding invocation extents.
    pub model_shape: Vec<u64>,
    /// Vocabulary width of each ordered logits row.
    pub vocab: u64,
    /// Result layout admitted by the plan.
    pub logits_layout: VerificationLogitsLayout,
}

impl VerificationPlanShape {
    /// Validate the static shape facts before they enter a plan identity.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when a shape fact is inadmissible.
    pub fn validate(&self) -> Result<(), VerificationAdmissionError> {
        if self.model_shape.is_empty() || self.model_shape.contains(&0) {
            return Err(VerificationAdmissionError::InvalidPlanShape {
                detail: "model shape must have a positive rank and dimensions".to_owned(),
            });
        }
        if self.vocab == 0 {
            return Err(VerificationAdmissionError::InvalidPlanShape {
                detail: "vocabulary width must be positive".to_owned(),
            });
        }
        Ok(())
    }

    /// Return the stable identity of this static plan shape.
    #[must_use]
    pub fn static_identity(&self) -> VerificationPlanIdentity {
        let mut bytes = Vec::new();
        push_verification_identity_bytes(&mut bytes, b"radix-mir:verification-plan:v1");
        push_verification_identity_u64(
            &mut bytes,
            u64::from(EXECUTOR_PLAN_VERIFICATION_ABI_VERSION),
        );
        push_verification_identity_u64(
            &mut bytes,
            u64::try_from(self.model_shape.len()).unwrap_or(u64::MAX),
        );
        for dimension in &self.model_shape {
            push_verification_identity_u64(&mut bytes, *dimension);
        }
        push_verification_identity_u64(&mut bytes, self.vocab);
        push_verification_identity_bytes(&mut bytes, self.logits_layout.spelling().as_bytes());
        VerificationPlanIdentity(format!("sha256:{}", handroll::sha256_hex(bytes)))
    }
}

/// Stable identity for static verification-plan facts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VerificationPlanIdentity(String);

impl VerificationPlanIdentity {
    /// Canonical `sha256:<64-hex>` identity spelling.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.0
    }
}

/// Target-neutral admission record for one speculative verification block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationBlockAdmission {
    /// Version of this additive verification record section.
    pub abi_version: u16,
    /// Committed prefix length `L` before this invocation.
    pub prefix_before: u64,
    /// Explicit candidate/query row count `k`; never inferred from a sequence.
    pub query_rows: u64,
    /// Candidate token ids, in the order represented by the result rows.
    pub candidate_ids: Vec<i64>,
    /// Generation of the committed cache state being read.
    pub committed_generation: Option<u64>,
    /// Identity of the transaction that owns the uncommitted candidate rows.
    pub transaction_id: Option<u64>,
    /// Static model and result-shape facts.
    pub plan: VerificationPlanShape,
}

/// Target-neutral result record returned for an admitted verification block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationBlockResult {
    /// Transaction that produced these candidate rows and logits.
    pub transaction_id: Option<u64>,
    /// Generation of the committed cache state used for the forward pass.
    pub committed_generation: Option<u64>,
    /// Ordered logits shape, expected to be exactly `[k, vocab]`.
    pub logits_shape: Vec<u64>,
    /// Physical/logical layout of the logits rows.
    pub logits_layout: VerificationLogitsLayout,
    /// Number of candidate rows accepted by the caller, `0 ≤ r ≤ k`.
    pub accepted_length: i64,
}

/// Focused fail-closed errors at the speculative-verification seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationAdmissionError {
    /// A static model/output shape is incomplete or malformed.
    InvalidPlanShape { detail: String },
    /// A prefix of zero would masquerade as fresh prefill.
    FreshPrefillMasquerade,
    /// The explicit candidate/query row count is zero.
    InvalidQueryRows,
    /// Candidate ids do not have the explicitly declared row count.
    CandidateCountMismatch { declared: u64, actual: usize },
    /// A candidate id is negative or outside the declared vocabulary.
    InvalidCandidateId { index: usize, candidate_id: i64 },
    /// The committed cache generation was omitted.
    MissingGeneration,
    /// The transaction identity was omitted.
    MissingTransactionIdentity,
    /// A result belongs to a different committed cache generation.
    StaleGeneration { expected: u64, actual: u64 },
    /// A result belongs to a different verification transaction.
    TransactionIdentityMismatch { expected: u64, actual: u64 },
    /// The result shape is not the ordered `[k, vocab]` shape, including a
    /// candidate-id/logit-row count mismatch.
    LogitsShapeSkew {
        expected: Vec<u64>,
        actual: Vec<u64>,
    },
    /// The result layout does not match the admitted plan layout.
    LogitsLayoutSkew {
        expected: VerificationLogitsLayout,
        actual: VerificationLogitsLayout,
    },
    /// The caller supplied an impossible accepted length.
    InvalidAcceptedLength { accepted: i64, maximum: u64 },
    /// A section consumer received a stale or unknown record version.
    UnsupportedVersion { actual: u16, expected: u16 },
}

impl fmt::Display for VerificationAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlanShape { detail } => {
                write!(formatter, "invalid verification plan shape: {detail}")
            }
            Self::FreshPrefillMasquerade => {
                write!(formatter, "verification prefix_before must be nonzero")
            }
            Self::InvalidQueryRows => {
                write!(formatter, "verification query_rows k must be nonzero")
            }
            Self::CandidateCountMismatch { declared, actual } => write!(
                formatter,
                "verification candidate count {actual} does not match explicit k={declared}"
            ),
            Self::InvalidCandidateId {
                index,
                candidate_id,
            } => write!(
                formatter,
                "verification candidate id {candidate_id} at index {index} is outside the admitted vocabulary"
            ),
            Self::MissingGeneration => {
                write!(formatter, "verification committed generation is missing")
            }
            Self::MissingTransactionIdentity => {
                write!(formatter, "verification transaction identity is missing")
            }
            Self::StaleGeneration { expected, actual } => write!(
                formatter,
                "verification result generation {actual} is stale; expected {expected}"
            ),
            Self::TransactionIdentityMismatch { expected, actual } => write!(
                formatter,
                "verification result transaction {actual} does not match admitted transaction {expected}"
            ),
            Self::LogitsShapeSkew { expected, actual } => write!(
                formatter,
                "verification logits shape {actual:?} does not match ordered {expected:?}"
            ),
            Self::LogitsLayoutSkew { expected, actual } => write!(
                formatter,
                "verification logits layout {} does not match {}",
                actual.spelling(),
                expected.spelling()
            ),
            Self::InvalidAcceptedLength { accepted, maximum } => write!(
                formatter,
                "verification accepted length r={accepted} is outside 0..={maximum}"
            ),
            Self::UnsupportedVersion { actual, expected } => write!(
                formatter,
                "unsupported verification record version {actual}; expected {expected}"
            ),
        }
    }
}

impl std::error::Error for VerificationAdmissionError {}

impl VerificationBlockAdmission {
    /// Validate and admit the invocation facts without inferring `k` from any
    /// sequence or candidate vector.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when the invocation facts are inconsistent.
    pub fn admit(self) -> Result<Self, VerificationAdmissionError> {
        self.validate()?;
        Ok(self)
    }

    /// Validate this invocation record.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when an invocation fact is inadmissible.
    pub fn validate(&self) -> Result<(), VerificationAdmissionError> {
        admit_executor_plan_verification_version(self.abi_version)?;
        self.plan.validate()?;
        if self.prefix_before == 0 {
            return Err(VerificationAdmissionError::FreshPrefillMasquerade);
        }
        if self.query_rows == 0 {
            return Err(VerificationAdmissionError::InvalidQueryRows);
        }
        if self.candidate_ids.len() != usize::try_from(self.query_rows).unwrap_or(usize::MAX) {
            return Err(VerificationAdmissionError::CandidateCountMismatch {
                declared: self.query_rows,
                actual: self.candidate_ids.len(),
            });
        }
        for (index, candidate_id) in self.candidate_ids.iter().copied().enumerate() {
            let in_vocab = u64::try_from(candidate_id)
                .ok()
                .is_some_and(|candidate_id| candidate_id < self.plan.vocab);
            if !in_vocab {
                return Err(VerificationAdmissionError::InvalidCandidateId {
                    index,
                    candidate_id,
                });
            }
        }
        if self.committed_generation.is_none() {
            return Err(VerificationAdmissionError::MissingGeneration);
        }
        if self.transaction_id.is_none() {
            return Err(VerificationAdmissionError::MissingTransactionIdentity);
        }
        Ok(())
    }

    /// The static identity excludes `L`, `k`, generation, candidates, and the
    /// transaction so equivalent model shapes share one compiled plan.
    #[must_use]
    pub fn static_plan_identity(&self) -> VerificationPlanIdentity {
        self.plan.static_identity()
    }

    /// Admit a result against this invocation's generation, transaction, row
    /// shape, layout, and accepted-length contract.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when the result violates the admitted contract.
    pub fn admit_result(
        &self,
        result: VerificationBlockResult,
    ) -> Result<VerificationBlockResult, VerificationAdmissionError> {
        self.validate()?;
        result.validate_against(self)?;
        Ok(result)
    }
}

impl VerificationBlockResult {
    /// Validate this result against its admitted verification block.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when the result does not match its admission.
    pub fn validate_against(
        &self,
        admission: &VerificationBlockAdmission,
    ) -> Result<(), VerificationAdmissionError> {
        let expected_generation = admission
            .committed_generation
            .ok_or(VerificationAdmissionError::MissingGeneration)?;
        let actual_generation = self
            .committed_generation
            .ok_or(VerificationAdmissionError::MissingGeneration)?;
        if actual_generation != expected_generation {
            return Err(VerificationAdmissionError::StaleGeneration {
                expected: expected_generation,
                actual: actual_generation,
            });
        }
        let expected_transaction = admission
            .transaction_id
            .ok_or(VerificationAdmissionError::MissingTransactionIdentity)?;
        let actual_transaction = self
            .transaction_id
            .ok_or(VerificationAdmissionError::MissingTransactionIdentity)?;
        if actual_transaction != expected_transaction {
            return Err(VerificationAdmissionError::TransactionIdentityMismatch {
                expected: expected_transaction,
                actual: actual_transaction,
            });
        }
        let expected_shape = vec![admission.query_rows, admission.plan.vocab];
        if self.logits_shape != expected_shape {
            return Err(VerificationAdmissionError::LogitsShapeSkew {
                expected: expected_shape,
                actual: self.logits_shape.clone(),
            });
        }
        if self.logits_layout != admission.plan.logits_layout {
            return Err(VerificationAdmissionError::LogitsLayoutSkew {
                expected: admission.plan.logits_layout,
                actual: self.logits_layout,
            });
        }
        if self.accepted_length < 0
            || u64::try_from(self.accepted_length)
                .map_or(true, |accepted| accepted > admission.query_rows)
        {
            return Err(VerificationAdmissionError::InvalidAcceptedLength {
                accepted: self.accepted_length,
                maximum: admission.query_rows,
            });
        }
        Ok(())
    }
}

/// Admit a verification section version before interpreting its records.
///
/// # Errors
/// Returns `VerificationAdmissionError` when the version is unsupported.
pub fn admit_executor_plan_verification_version(
    version: u16,
) -> Result<(), VerificationAdmissionError> {
    if version != EXECUTOR_PLAN_VERIFICATION_ABI_VERSION {
        return Err(VerificationAdmissionError::UnsupportedVersion {
            actual: version,
            expected: EXECUTOR_PLAN_VERIFICATION_ABI_VERSION,
        });
    }
    Ok(())
}

fn push_verification_identity_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    output.extend_from_slice(bytes);
}

fn push_verification_identity_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

/// The scalar data type of an allocation or typed source.
///
/// This is an alias of the existing uniform type vocabulary so the producer
/// and the v1 constant surface cannot disagree about byte widths. The KV
/// campaign's integrated route uses `F32`; `F16` remains an explicit type,
/// not an implicit narrowing.
pub type AllocationDType = UniformType;
/// Short alias used by consumers that call the field simply `dtype`.
pub type DType = AllocationDType;

impl UniformType {
    /// Number of bytes occupied by one scalar of this type.
    #[must_use]
    pub const fn byte_width(self) -> u64 {
        match self {
            Self::F16 => 2,
            Self::F32 | Self::U32 | Self::I32 => 4,
        }
    }
}

/// The typed invocation-state fields admitted as runtime binding sources.
///
/// These are tags, not string sentinels or magic uniform names. Every field is
/// carried as a `u32` by the invocation-state buffer and can therefore be
/// shared by append and attention launches without changing static plan
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeSource {
    /// Absolute write position / current prefix position.
    Position,
    /// Prefix length after the current append.
    ValidLenAfter,
    /// Number of query rows in this invocation.
    QueryRows,
    /// Monotonic sequence/reset epoch.
    SequenceEpoch,
}

impl RuntimeSource {
    /// The scalar type carried by every invocation-state field.
    #[must_use]
    pub const fn ty(self) -> UniformType {
        UniformType::U32
    }

    /// Stable field spelling for diagnostics and wire mirrors.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::ValidLenAfter => "valid_len_after",
            Self::QueryRows => "query_rows",
            Self::SequenceEpoch => "sequence_epoch",
        }
    }
}

/// A tagged source for a v2 launch binding.
///
/// The enum tag is the source contract: a constant is a typed bit pattern and
/// a runtime value is one of the closed invocation-state fields. A consumer
/// must never reinterpret a v1 constant as a runtime cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutorPlanSource {
    /// A compile-time scalar value.
    Constant { ty: UniformType, bits: u32 },
    /// A value read from the typed invocation-state buffer.
    Runtime(RuntimeSource),
}

impl ExecutorPlanSource {
    /// Construct a typed constant source.
    #[must_use]
    pub const fn constant(ty: UniformType, bits: u32) -> Self {
        Self::Constant { ty, bits }
    }

    /// Construct a typed runtime source.
    #[must_use]
    pub const fn runtime(source: RuntimeSource) -> Self {
        Self::Runtime(source)
    }

    /// The scalar type consumed by this source.
    #[must_use]
    pub const fn ty(self) -> UniformType {
        match self {
            Self::Constant { ty, .. } => ty,
            Self::Runtime(source) => source.ty(),
        }
    }

    /// Whether this source is dynamic.
    #[must_use]
    pub const fn is_runtime(self) -> bool {
        matches!(self, Self::Runtime(_))
    }
}

/// Compatibility spelling for the shared field table.
pub type TypedSource = ExecutorPlanSource;
/// Compatibility spelling for consumers that name this a plan source.
pub type PlanSource = ExecutorPlanSource;

/// The independent initialization fact of an allocation.
pub use crate::device_semantics::InitializationPolicy;

/// Typed fixed-capacity storage facts for one allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationFact {
    /// Stable program-level buffer identity.
    pub buffer: crate::device_program::BufferIdentity,
    /// Element data type of the allocation.
    pub dtype: AllocationDType,
    /// Physical allocation capacity in bytes, distinct from every view span.
    pub capacity_bytes: u64,
    /// Allocation lifetime, repeated explicitly for the v2 ABI contract.
    pub lifetime: crate::device_program::BufferLifetime,
    /// How the allocation reaches its first defined state.
    pub initialization: InitializationPolicy,
}

/// A bounded logical view over an allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewFact {
    /// Allocation identity shared by this view and any sibling views.
    pub allocation: crate::device_program::BufferId,
    /// Logical dimensions in outer-to-inner order.
    pub logical_dims: Vec<u64>,
    /// Element strides in the same order as [`Self::logical_dims`].
    pub strides: Vec<u64>,
    /// Static base element offset in the allocation.
    pub static_base: u64,
    /// Maximum byte span from the static base, not allocation capacity.
    pub maximum_span: u64,
}

/// Compatibility spelling for the shared field table.
pub type AllocationView = ViewFact;
/// Compatibility spelling for consumers that call this a view fact.
pub type View = ViewFact;

/// One v2 launch binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchBinding {
    /// Kernel selection receiving this binding.
    pub kernel_index: usize,
    /// Allocation handle/identity bound by the launch.
    pub handle: crate::device_program::BufferId,
    /// Target-neutral binding index.
    pub binding: crate::device_program::Binding,
    /// Byte offset from the allocation base.
    pub byte_offset: u64,
    /// Byte span exposed to this launch.
    pub view_span: u64,
    /// Typed constant or invocation-state source for the binding.
    pub runtime_source: ExecutorPlanSource,
}

impl LaunchBinding {
    /// Construct a launch binding while keeping the field table explicit.
    #[must_use]
    pub const fn new(
        kernel_index: usize,
        handle: crate::device_program::BufferId,
        binding: crate::device_program::Binding,
        byte_offset: u64,
        view_span: u64,
        runtime_source: ExecutorPlanSource,
    ) -> Self {
        Self {
            kernel_index,
            handle,
            binding,
            byte_offset,
            view_span,
            runtime_source,
        }
    }
}

/// All dynamic facts supplied to the v2 producer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutorPlanV2Facts {
    /// Fixed-capacity physical allocations.
    pub allocations: Vec<AllocationFact>,
    /// Logical bounded views over those allocations.
    pub views: Vec<ViewFact>,
    /// Per-kernel launch bindings.
    pub launch_bindings: Vec<LaunchBinding>,
}

/// Compatibility spelling for the producer input.
pub type RuntimeBindingFacts = ExecutorPlanV2Facts;

/// A typed v2 executor-plan section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorPlanExtensionV2 {
    /// Section schema version.
    pub abi_version: u16,
    /// The same ordered library/fallback selections as the static plan.
    pub selections: Vec<LibrarySelection>,
    /// Allocation facts shared by all views and launches.
    pub allocations: Vec<AllocationFact>,
    /// Bounded logical views over those allocations.
    pub views: Vec<ViewFact>,
    /// Launch bindings carrying byte ranges and tagged sources.
    pub launch_bindings: Vec<LaunchBinding>,
}

/// Compatibility spelling for the v2 payload.
pub type ExecutorPlanV2 = ExecutorPlanExtensionV2;

/// Typed v2 producer failure. Validation happens before the static selection
/// projection so malformed or overflowing spans cannot reach emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorPlanV2Error {
    /// An allocation identity appeared more than once.
    DuplicateAllocation {
        buffer: crate::device_program::BufferId,
    },
    /// An allocation's repeated identity/capacity facts are malformed.
    InvalidAllocation {
        buffer: crate::device_program::BufferId,
        detail: String,
    },
    /// A view refers to no allocation or has malformed/overflowing span facts.
    InvalidView {
        allocation: crate::device_program::BufferId,
        detail: String,
    },
    /// A binding points outside the ordered kernel/resource facts.
    InvalidLaunchBinding { kernel_index: usize, detail: String },
    /// Two bindings claim the same kernel slot.
    DuplicateLaunchBinding {
        kernel_index: usize,
        group: u32,
        binding: u32,
    },
    /// A v2 section without any dynamic source is an accidental static plan.
    MissingRuntimeSource,
    /// A v2 section has a stale or skewed selection count/order.
    SelectionSkew { detail: String },
}

impl fmt::Display for ExecutorPlanV2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateAllocation { buffer } => {
                write!(f, "executor-plan v2 repeats allocation {}", buffer.0)
            }
            Self::InvalidAllocation { buffer, detail } => {
                write!(
                    f,
                    "executor-plan v2 allocation {} is invalid: {detail}",
                    buffer.0
                )
            }
            Self::InvalidView { allocation, detail } => {
                write!(
                    f,
                    "executor-plan v2 view for allocation {} is invalid: {detail}",
                    allocation.0
                )
            }
            Self::InvalidLaunchBinding {
                kernel_index,
                detail,
            } => write!(
                f,
                "executor-plan v2 binding for kernel {kernel_index} is invalid: {detail}"
            ),
            Self::DuplicateLaunchBinding {
                kernel_index,
                group,
                binding,
            } => write!(
                f,
                "executor-plan v2 repeats kernel {kernel_index} binding ({group}, {binding})"
            ),
            Self::MissingRuntimeSource => {
                write!(f, "executor-plan v2 carries no tagged runtime source")
            }
            Self::SelectionSkew { detail } => {
                write!(f, "executor-plan v2 selection skew: {detail}")
            }
        }
    }
}

impl std::error::Error for ExecutorPlanV2Error {}

impl ExecutorPlanV2Facts {
    /// Validate dynamic facts against the typed device program before any
    /// executor-plan selection is projected.
    ///
    /// # Errors
    /// Returns `ExecutorPlanV2Error` when a dynamic fact contradicts the program.
    pub fn validate_for_program(&self, program: &DeviceProgram) -> Result<(), ExecutorPlanV2Error> {
        let allocations = collect_allocation_facts(&self.allocations)?;

        validate_view_facts(&self.views, &allocations)?;

        let mut seen_bindings = std::collections::BTreeSet::new();
        let mut has_runtime_source = false;
        for launch_binding in &self.launch_bindings {
            let key = (
                launch_binding.kernel_index,
                launch_binding.binding.group,
                launch_binding.binding.binding,
            );
            if !seen_bindings.insert(key) {
                return Err(ExecutorPlanV2Error::DuplicateLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    group: launch_binding.binding.group,
                    binding: launch_binding.binding.binding,
                });
            }
            let Some(kernel) = program.kernels.get(launch_binding.kernel_index) else {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "kernel index is outside the program".to_owned(),
                });
            };
            let Some(resource) = kernel
                .resources
                .iter()
                .find(|resource| resource.binding == launch_binding.binding)
            else {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: format!(
                        "binding ({}, {}) is not declared by the kernel",
                        launch_binding.binding.group, launch_binding.binding.binding
                    ),
                });
            };
            if resource.buffer.id != launch_binding.handle {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: format!(
                        "handle {} disagrees with declared resource handle {}",
                        launch_binding.handle.0, resource.buffer.id.0
                    ),
                });
            }
            let Some(allocation) = allocations.get(&launch_binding.handle) else {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "binding refers to an unknown allocation".to_owned(),
                });
            };
            if launch_binding.view_span == 0 {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "view span must be nonzero".to_owned(),
                });
            }
            let Some(end) = launch_binding
                .byte_offset
                .checked_add(launch_binding.view_span)
            else {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "byte offset plus view span overflows".to_owned(),
                });
            };
            if end > allocation.capacity_bytes {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: format!(
                        "binding end {end} exceeds allocation capacity {}",
                        allocation.capacity_bytes
                    ),
                });
            }
            if view_base_bytes_for_binding(
                &self.views,
                allocation,
                launch_binding.byte_offset,
                launch_binding.view_span,
            )
            .is_none()
            {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "byte range does not match a bounded allocation view".to_owned(),
                });
            }
            has_runtime_source |= launch_binding.runtime_source.is_runtime();
        }
        if !has_runtime_source {
            return Err(ExecutorPlanV2Error::MissingRuntimeSource);
        }
        Ok(())
    }
}

fn collect_allocation_facts(
    allocations: &[AllocationFact],
) -> Result<
    std::collections::BTreeMap<crate::device_program::BufferId, &AllocationFact>,
    ExecutorPlanV2Error,
> {
    let mut by_id = std::collections::BTreeMap::new();
    for allocation in allocations {
        let id = allocation.buffer.id;
        if by_id.insert(id, allocation).is_some() {
            return Err(ExecutorPlanV2Error::DuplicateAllocation { buffer: id });
        }
        if allocation.capacity_bytes == 0 {
            return Err(ExecutorPlanV2Error::InvalidAllocation {
                buffer: id,
                detail: "capacity bytes must be nonzero".to_owned(),
            });
        }
        if allocation.lifetime != allocation.buffer.lifetime {
            return Err(ExecutorPlanV2Error::InvalidAllocation {
                buffer: id,
                detail: "lifetime disagrees with buffer identity".to_owned(),
            });
        }
    }
    Ok(by_id)
}

fn validate_view_facts(
    views: &[ViewFact],
    allocations: &std::collections::BTreeMap<crate::device_program::BufferId, &AllocationFact>,
) -> Result<(), ExecutorPlanV2Error> {
    for view in views {
        let Some(allocation) = allocations.get(&view.allocation) else {
            return Err(ExecutorPlanV2Error::InvalidView {
                allocation: view.allocation,
                detail: "view refers to an unknown allocation".to_owned(),
            });
        };
        validate_view_span(view, allocation)?;
    }
    Ok(())
}

fn validate_view_span(
    view: &ViewFact,
    allocation: &AllocationFact,
) -> Result<(), ExecutorPlanV2Error> {
    if view.logical_dims.is_empty() || view.logical_dims.len() != view.strides.len() {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "logical dimensions and strides must have the same nonzero rank".to_owned(),
        });
    }
    if view
        .logical_dims
        .iter()
        .chain(view.strides.iter())
        .any(|value| *value == 0)
    {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "logical dimensions and strides must be positive".to_owned(),
        });
    }
    if view.maximum_span == 0 {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "maximum span must be nonzero".to_owned(),
        });
    }
    let Some(required_elements) = view_required_elements(view) else {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "logical span arithmetic overflows".to_owned(),
        });
    };
    let required_span = required_elements
        .checked_mul(allocation.dtype.byte_width())
        .ok_or_else(|| ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "byte span arithmetic overflows".to_owned(),
        })?;
    if view.maximum_span < required_span {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: format!(
                "maximum span {} is smaller than required span {required_span}",
                view.maximum_span
            ),
        });
    }
    let base_bytes = view
        .static_base
        .checked_mul(allocation.dtype.byte_width())
        .ok_or_else(|| ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "static base byte arithmetic overflows".to_owned(),
        })?;
    let end = base_bytes.checked_add(view.maximum_span).ok_or_else(|| {
        ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "view end arithmetic overflows".to_owned(),
        }
    })?;
    if end > allocation.capacity_bytes {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: format!(
                "view end {end} exceeds allocation capacity {}",
                allocation.capacity_bytes
            ),
        });
    }
    Ok(())
}

fn view_base_bytes_for_binding(
    views: &[ViewFact],
    allocation: &AllocationFact,
    byte_offset: u64,
    view_span: u64,
) -> Option<u64> {
    let dtype_width = allocation.dtype.byte_width();
    views.iter().find_map(|view| {
        if view.allocation != allocation.buffer.id || view.maximum_span < view_span {
            return None;
        }
        let required_span = view_required_elements(view)?.checked_mul(dtype_width)?;
        if view_span < required_span {
            return None;
        }
        let base_bytes = view.static_base.checked_mul(dtype_width)?;
        (base_bytes == byte_offset).then_some(base_bytes)
    })
}

/// The bounded element count a view's logical dims/strides address; `None`
/// when the logical span arithmetic overflows.
fn view_required_elements(view: &ViewFact) -> Option<u64> {
    let max_element =
        view.logical_dims
            .iter()
            .zip(&view.strides)
            .try_fold(0u64, |acc, (dim, stride)| {
                dim.checked_sub(1)
                    .and_then(|extent| extent.checked_mul(*stride))
                    .and_then(|extent| acc.checked_add(extent))
            })?;
    max_element.checked_add(1)
}

impl ExecutorPlanExtensionV2 {
    /// Validate version, ordered selections, and binding ownership before a
    /// consumer interprets any v2 field.
    ///
    /// # Errors
    /// Returns a `String` detail when a v2 field disagrees with the kernel count.
    pub fn validate_for_kernel_count(&self, kernel_count: usize) -> Result<(), String> {
        if self.abi_version != EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION {
            return Err(format!(
                "executor-plan section version {} is not supported (expected {})",
                self.abi_version, EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION
            ));
        }
        if self.selections.len() != kernel_count {
            return Err(format!(
                "executor-plan selection count {} does not match kernel count {}",
                self.selections.len(),
                kernel_count
            ));
        }
        for (position, selection) in self.selections.iter().enumerate() {
            if selection.kernel_index != position || !selection.bind.is_well_formed() {
                return Err(format!(
                    "executor-plan v2 selection {position} is not an ordered well-formed selection"
                ));
            }
        }
        for binding in &self.launch_bindings {
            if binding.kernel_index >= kernel_count {
                return Err(format!(
                    "executor-plan v2 binding references kernel {} but there are {} kernels",
                    binding.kernel_index, kernel_count
                ));
            }
        }
        if !self
            .launch_bindings
            .iter()
            .any(|binding| binding.runtime_source.is_runtime())
        {
            return Err("executor-plan v2 carries no tagged runtime source".to_owned());
        }
        Ok(())
    }
}

/// Produce a v2 executor plan after validating every allocation, view, and
/// launch binding against the typed program. The v1 [`executor_plan_extension`]
/// producer remains the static compatibility path and is not changed.
///
/// # Errors
/// Returns `ExecutorPlanV2Error` when any allocation, view, or launch binding
/// fails validation against the typed program.
pub fn executor_plan_extension_v2(
    program: &DeviceProgram,
    facts: ExecutorPlanV2Facts,
) -> Result<ExecutorPlanExtensionV2, ExecutorPlanV2Error> {
    facts.validate_for_program(program)?;
    let static_plan = executor_plan_extension(program);
    let extension = ExecutorPlanExtensionV2 {
        abi_version: EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION,
        selections: static_plan.selections,
        allocations: facts.allocations,
        views: facts.views,
        launch_bindings: facts.launch_bindings,
    };
    extension
        .validate_for_kernel_count(program.kernels.len())
        .map_err(|detail| ExecutorPlanV2Error::SelectionSkew { detail })?;
    Ok(extension)
}

/// One typed buffer slot in a launch descriptor.
///
/// A program fact snapshot of a [`DeviceResource`]: the binding indices, the
/// versioned shape, and the slot access. Hosts bind buffers from these typed
/// facts — never by parsing emitted MSL/LLVM/PTX (A3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchBufferSlot {
    /// Target-neutral binding indices.
    pub binding: Binding,
    /// Element type of the bound shape.
    pub element_ty: MirType,
    /// Element count of the bound shape.
    pub element_count: u64,
    /// Program-level buffer role (input / output / in-out).
    pub role: BufferRole,
    /// Where the storage lives (`DeviceHandle` / `HostOwned`).
    pub storage: MirTensorStorageLayout,
}

/// Backend-neutral typed launch descriptor for one device-program kernel.
///
/// The host-facing launch facts (S1-4 consumes them): the logical entry name,
/// the recipe family, the workgroup/dispatch/workgroup-count plan, the typed
/// buffer slots, and the shared-memory footprint. Every field is a program
/// fact; nothing is inferred from emitted text (A3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelLaunchDescriptor {
    /// Index into [`DeviceProgram`]`::kernels`.
    pub kernel_index: usize,
    /// Target-neutral logical entry name (backends map it to their syntax).
    pub entry: String,
    /// M2 library-v0 bind descriptor. The descriptor is carried separately
    /// from the entry so one body can serve multiple concrete shapes.
    pub bind: BindDescriptor,
    /// Recipe family tag.
    pub recipe: RecipeKind,
    /// Threads per workgroup.
    pub workgroup: MirWorkgroupSize,
    /// Dispatch extent in workgroups.
    pub dispatch: MirKernelDispatchSize,
    /// Derived workgroup count the host launches.
    pub workgroup_count: MirKernelWorkgroupCount,
    /// Typed buffer slots, in the kernel's typed resource order.
    pub slots: Vec<LaunchBufferSlot>,
    /// Shared-memory footprint in bytes (0 for elementwise kernels).
    pub shared_memory_bytes: u64,
    /// Per-output bounds/index domains (DDCP1-U3, contract §Per-output
    /// bounds): each Write/ReadWrite slot's own extent. Heterogeneous
    /// outputs each carry their own derived domain — hosts dispatch per
    /// output, never under one guessed shared extent.
    pub output_domains: Vec<PerOutputDomain>,
}

impl KernelLaunchDescriptor {
    /// Build the launch descriptor for one kernel unit.
    #[must_use]
    pub fn from_kernel_unit(kernel: &KernelUnit, kernel_index: usize) -> Self {
        Self {
            kernel_index,
            entry: kernel.entry.clone(),
            bind: bind_descriptor_for_kernel(kernel),
            recipe: RecipeKind::from_plan(&kernel.plan),
            workgroup: kernel.launch.workgroup,
            dispatch: kernel.launch.dispatch_size,
            workgroup_count: kernel.launch.workgroup_count,
            slots: kernel.resources.iter().map(launch_buffer_slot).collect(),
            shared_memory_bytes: plan_shared_memory_bytes(&kernel.plan),
            output_domains: per_output_domains(kernel),
        }
    }
}

/// Derive the typed launch descriptors for every kernel of a device program,
/// in program order.
#[must_use]
pub fn launch_descriptors(program: &DeviceProgram) -> Vec<KernelLaunchDescriptor> {
    program
        .kernels
        .iter()
        .enumerate()
        .map(|(index, kernel)| KernelLaunchDescriptor::from_kernel_unit(kernel, index))
        .collect()
}

/// Derive the M2 library-v0 bind facts for one carried kernel.
///
/// This is the only planner-side projection needed by the additive executor
/// extension. It preserves the existing `KernelUnit` and `DeviceProgram`
/// schema: generated kernels continue to carry their original entry, resources,
/// and launch plan, while library consumers receive the same typed dimensions,
/// strides, layout, and grid as a sidecar fact.
#[must_use]
pub fn bind_descriptor_for_kernel(kernel: &KernelUnit) -> BindDescriptor {
    let output_extent = kernel
        .resources
        .iter()
        .find(|resource| {
            matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            )
        })
        .map_or_else(
            || {
                kernel
                    .resources
                    .first()
                    .map_or(1, |resource| resource.version.element_count)
            },
            |resource| resource.version.element_count,
        );
    let (dims, layout) = bind_dimensions(&kernel.plan, output_extent);
    BindDescriptor::row_major(dims, kernel.launch.workgroup_count, layout)
}

/// Build the additive M0 executor-plan section without changing the existing
/// device-program or FMIR wire shape.
#[must_use]
pub fn executor_plan_extension(program: &DeviceProgram) -> ExecutorPlanExtension {
    ExecutorPlanExtension {
        abi_version: EXECUTOR_PLAN_BIND_ABI_VERSION,
        selections: program
            .kernels
            .iter()
            .enumerate()
            .map(|(kernel_index, kernel)| {
                let recipe = RecipeKind::from_plan(&kernel.plan);
                let bind = bind_descriptor_for_kernel(kernel);
                LibrarySelection {
                    kernel_index,
                    library_entry: library_entry_for_kernel(kernel, recipe),
                    logical_signature: logical_signature_for_kernel(kernel, recipe),
                    binds: library_binds_for_kernel(kernel, &bind),
                    uniforms: packed_decode_uniforms_for_kernel(kernel),
                    offsets: offsets_for_kernel(kernel, &bind),
                    workspace: None,
                    backend_requirements: packed_backend_requirements_for_kernel(kernel),
                    provenance: PlanProvenance {
                        pass: "device-program-plan".to_owned(),
                        revision: u32::from(EXECUTOR_PLAN_BIND_ABI_VERSION),
                        source_nodes: vec![kernel.function.0],
                    },
                    bind,
                }
            })
            .collect(),
    }
}

fn bind_dimensions(plan: &CollectionKernelPlan, output_extent: u64) -> (Vec<u64>, BindLayout) {
    match plan {
        CollectionKernelPlan::RmsNormalization(plan) => {
            if plan.width > 0 && output_extent.is_multiple_of(plan.width) {
                (
                    vec![output_extent / plan.width, plan.width],
                    BindLayout::RowMajor,
                )
            } else {
                (vec![output_extent], BindLayout::Flat)
            }
        }
        CollectionKernelPlan::Rope(plan) => {
            let layout = if plan.rotate_half {
                BindLayout::RopeRotateHalf {
                    rotated_width: plan.dim,
                }
            } else {
                BindLayout::RopeConsecutivePair {
                    rotated_width: plan.dim,
                }
            };
            let rows = if plan.width > 0 && output_extent.is_multiple_of(plan.width) {
                output_extent / plan.width
            } else {
                plan.rows.max(1)
            };
            (vec![rows, plan.width.max(1)], layout)
        }
        CollectionKernelPlan::CausalMaskedSoftmax(plan) => {
            (vec![plan.rows, plan.cols], BindLayout::RowMajor)
        }
        CollectionKernelPlan::TiledMatMul(plan) => (vec![plan.m, plan.n], BindLayout::RowMajor),
        CollectionKernelPlan::QuantizedMatMul(plan) => (vec![plan.m, plan.n], BindLayout::RowMajor),
        CollectionKernelPlan::GroupedMatMul(plan) => {
            (vec![plan.groups, plan.m, plan.n], BindLayout::RowMajor)
        }
        CollectionKernelPlan::BatchedAttention(plan) => {
            let dims = match plan.kind {
                BatchedAttentionKind::ProjectionQ => {
                    vec![plan.kv_heads, plan.q_per_kv, plan.seq, plan.head_dim]
                }
                BatchedAttentionKind::ProjectionK | BatchedAttentionKind::ProjectionV => {
                    vec![plan.kv_heads, plan.seq, plan.head_dim]
                }
                BatchedAttentionKind::Scores | BatchedAttentionKind::Softmax => {
                    vec![plan.kv_heads, plan.q_per_kv, plan.seq, plan.seq]
                }
                BatchedAttentionKind::ContextSequenceMajor => vec![plan.seq, plan.hidden],
            };
            (dims, BindLayout::Strided)
        }
        CollectionKernelPlan::Transpose(plan) => (vec![plan.m, plan.n], BindLayout::RowMajor),
        CollectionKernelPlan::TreeReduction(plan) => (vec![plan.length], BindLayout::Flat),
        CollectionKernelPlan::Elementwise
        | CollectionKernelPlan::AxisReduction(_)
        | CollectionKernelPlan::RowSoftmax(_)
        | CollectionKernelPlan::LayerNormalization(_)
        | CollectionKernelPlan::Gather(_)
        | CollectionKernelPlan::SsmConv1d(_)
        | CollectionKernelPlan::SsmScan(_)
        // SFR-5: the declared distributed sum's broadcast result is a flat
        // partial-slot write — same bind shape as the tree floor.
        | CollectionKernelPlan::LaneGrid { .. }
        | CollectionKernelPlan::SingleLane
        | CollectionKernelPlan::GridReduce { .. }
        | CollectionKernelPlan::DistributedSum(_) => (vec![output_extent.max(1)], BindLayout::Flat),
    }
}

fn logical_signature_for_kernel(kernel: &KernelUnit, recipe: RecipeKind) -> LogicalSignature {
    let inputs = kernel
        .resources
        .iter()
        .filter(|resource| {
            matches!(
                resource.access,
                MirKernelResourceAccess::Read | MirKernelResourceAccess::ReadWrite
            )
        })
        .enumerate()
        .map(|(index, _)| format!("input{index}"))
        .collect();
    let outputs = kernel
        .resources
        .iter()
        .filter(|resource| {
            matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            )
        })
        .enumerate()
        .map(|(index, _)| format!("output{index}"))
        .collect();
    LogicalSignature {
        operation: format!("{recipe:?}"),
        inputs,
        outputs,
    }
}

fn library_binds_for_kernel(kernel: &KernelUnit, bind: &BindDescriptor) -> Vec<LibraryBind> {
    let packed = packed_region_for_plan(&kernel.plan);
    kernel
        .resources
        .iter()
        .enumerate()
        .map(|(index, resource)| {
            let (dims, strides) = if is_packed_weight_slot(kernel, resource) {
                if let Some(packed) = packed.as_ref() {
                    (vec![packed.packed_bytes], vec![1])
                } else {
                    (bind.dims.clone(), bind.strides.clone())
                }
            } else {
                (bind.dims.clone(), bind.strides.clone())
            };
            LibraryBind {
                name: format!("buffer{index}"),
                binding: resource.binding,
                dims,
                strides,
                offset_elements: 0,
            }
        })
        .collect()
}

fn offsets_for_kernel(kernel: &KernelUnit, bind: &BindDescriptor) -> Vec<OffsetBinding> {
    kernel
        .resources
        .iter()
        .enumerate()
        .map(|(index, resource)| OffsetBinding {
            name: format!("buffer{index}"),
            offset_elements: 0,
            strides: if is_packed_weight_slot(kernel, resource) {
                vec![1]
            } else {
                bind.strides.clone()
            },
        })
        .collect()
}

fn library_entry_for_kernel(kernel: &KernelUnit, recipe: RecipeKind) -> Option<String> {
    if let Some(packed) = packed_region_for_plan(&kernel.plan) {
        return Some(quantized_compute_entry(packed.decode_extent).to_owned());
    }
    let entry = match recipe {
        RecipeKind::RmsNormalization => Some("rms"),
        RecipeKind::Rope => Some("rope"),
        RecipeKind::RowSoftmax | RecipeKind::CausalMaskedSoftmax => Some("softmax"),
        RecipeKind::Elementwise => match kernel.library_operation {
            LibraryOperation::SwiGlu => Some("swiglu"),
            LibraryOperation::ResidualRmsNorm => Some("residual"),
            _ => None,
        },
        _ => None,
    }?;
    Some(entry.to_owned())
}

fn quantized_compute_entry(decode_extent: u64) -> &'static str {
    if DECODE_GEMV_SELECTION_ENABLED && decode_extent == 1 {
        QUANTIZED_GEMV_ENTRY
    } else {
        QUANTIZED_GEMM_ENTRY
    }
}

struct PackedRegionFacts {
    layout: PackedStorageLayout,
    packed_bytes: u64,
    decode_extent: u64,
}

fn packed_region_for_plan(plan: &CollectionKernelPlan) -> Option<PackedRegionFacts> {
    match plan {
        CollectionKernelPlan::QuantizedMatMul(plan) => Some(PackedRegionFacts {
            layout: plan.layout,
            packed_bytes: plan
                .layout
                .packed_bytes_for_elements(plan.k.saturating_mul(plan.n))?,
            decode_extent: plan.m,
        }),
        CollectionKernelPlan::BatchedAttention(plan) => {
            let layout = plan.layout?;
            let n = match plan.kind {
                BatchedAttentionKind::ProjectionQ => plan.hidden,
                BatchedAttentionKind::ProjectionK | BatchedAttentionKind::ProjectionV => {
                    plan.kv_heads.saturating_mul(plan.head_dim)
                }
                _ => return None,
            };
            Some(PackedRegionFacts {
                layout,
                packed_bytes: layout.packed_bytes_for_elements(plan.hidden.saturating_mul(n))?,
                decode_extent: plan.seq,
            })
        }
        _ => None,
    }
}

fn is_packed_weight_slot(kernel: &KernelUnit, resource: &DeviceResource) -> bool {
    packed_region_for_plan(&kernel.plan).is_some() && resource.binding.binding == 1
}

fn packed_decode_uniforms_for_kernel(kernel: &KernelUnit) -> Vec<UniformBinding> {
    let Some(packed) = packed_region_for_plan(&kernel.plan) else {
        return Vec::new();
    };
    let packed_bytes = u32::try_from(packed.packed_bytes).unwrap_or(u32::MAX);
    vec![
        UniformBinding {
            name: PACKED_LAYOUT_GGML_TYPE_ID_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: packed.layout.ggml_type_id(),
        },
        UniformBinding {
            name: PACKED_BLOCK_BYTES_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: packed.layout.block_bytes(),
        },
        UniformBinding {
            name: PACKED_BLOCK_ELEMENTS_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: packed.layout.block_elements(),
        },
        UniformBinding {
            name: PACKED_BYTES_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: packed_bytes,
        },
        UniformBinding {
            name: DECODE_GEMV_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: u32::from(DECODE_GEMV_SELECTION_ENABLED && packed.decode_extent == 1),
        },
    ]
}

fn packed_backend_requirements_for_kernel(kernel: &KernelUnit) -> Vec<BackendRequirement> {
    let Some(packed) = packed_region_for_plan(&kernel.plan) else {
        return Vec::new();
    };
    vec![BackendRequirement {
        backend: "metal".to_owned(),
        feature: quantized_compute_entry(packed.decode_extent).to_owned(),
    }]
}

fn launch_buffer_slot(resource: &DeviceResource) -> LaunchBufferSlot {
    LaunchBufferSlot {
        binding: resource.binding,
        element_ty: resource.version.element_ty,
        element_count: resource.version.element_count,
        role: resource.buffer.role,
        storage: resource.buffer.storage,
    }
}

/// The per-output bounds/index domain of one kernel output slot
/// (DDCP1-U3, contract §Per-output bounds rule 1): every output has its
/// OWN extent — never inferred from the shared dispatch and never taken
/// from another output's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerOutputDomain {
    /// The output slot's target-neutral binding indices.
    pub binding: Binding,
    /// The output's own element extent.
    pub element_count: u64,
}

/// Derive each kernel output's own bounds/index domain (DDCP1-U3).
///
/// Output slots are the kernel's `Write` and `ReadWrite` resources — each one
/// gets its own extent from its bound version's element count. The derivation
/// is a pure function over carried program facts (A3: never text).
#[must_use]
pub fn per_output_domains(kernel: &KernelUnit) -> Vec<PerOutputDomain> {
    kernel
        .resources
        .iter()
        .filter(|resource| {
            matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            )
        })
        // A runtime-shaped output has no static extent; the dispatcher checks
        // the actual argument against the lane domain at launch (E_INVALID_ARGS).
        .filter(|resource| {
            resource.version.element_count != crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT
        })
        .map(|resource| PerOutputDomain {
            binding: resource.binding,
            element_count: resource.version.element_count,
        })
        .collect()
}

/// Validate per-output bounds/index domains fail-closed (DDCP1-U3, contract
/// §Per-output bounds rules 1–3).
///
/// Every kernel output must be covered by the kernel's dispatched lanes —
/// no shared dispatch extent may under-fill a heterogeneous or single
/// output. A single dispatch extent is legal only where the member outputs
/// provably share one domain (identical extents); heterogeneous outputs are
/// legal only because each output carries its own derived domain (the
/// launch descriptor exposes [`PerOutputDomain`] per output).
///
/// # Errors
/// Returns [`DeviceProgramError::OutputExtentUnderfilled`] when an output's
/// own domain exceeds the dispatched lanes.
pub fn validate_per_output_domains(program: &DeviceProgram) -> Result<(), DeviceProgramError> {
    for (kernel_index, kernel) in program.kernels.iter().enumerate() {
        // A single-lane kernel's one thread runs the whole body and addresses
        // every output element itself: its coverage is the body, not lanes.
        if kernel.plan == CollectionKernelPlan::SingleLane {
            continue;
        }
        let dispatched = dispatched_lanes(kernel);
        for domain in per_output_domains(kernel) {
            if domain.element_count > dispatched {
                return Err(DeviceProgramError::OutputExtentUnderfilled {
                    kernel: kernel_index,
                    group: domain.binding.group,
                    binding: domain.binding.binding,
                    element_count: domain.element_count,
                    dispatched,
                });
            }
        }
    }
    Ok(())
}

/// The output coverage of the shared dispatch: workgroup count ×
/// workgroup size across all three axes — one lane per element for the
/// per-element launches. The PGC-R5 row-reduction family is the one
/// collective 1D exception: one `ROW_REDUCTION_WORKGROUP_X`-lane workgroup
/// owns one FULL row (the `RowReduction` contract's law), so the write
/// coverage is workgroups × row width, not lanes — the per-thread
/// under-coverage oracle does not apply (a workgroup writes a whole row,
/// not one element; the tiled-matmul over-cover class is the precedent).
/// Keyed on the contract-minted launch shape for the three row-reduction
/// plans; an under-rowed launch still under-fills and fails closed.
fn dispatched_lanes(kernel: &KernelUnit) -> u64 {
    let launch = &kernel.launch;
    let lanes = launch.workgroup_count.x
        * u64::from(launch.workgroup.x)
        * launch.workgroup_count.y
        * u64::from(launch.workgroup.y)
        * launch.workgroup_count.z
        * u64::from(launch.workgroup.z);
    if launch.workgroup.y == 1
        && launch.workgroup.z == 1
        && launch.workgroup.x == crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X
    {
        let workgroups = launch.workgroup_count.x;
        match &kernel.plan {
            CollectionKernelPlan::RmsNormalization(plan) => {
                return workgroups.saturating_mul(plan.width);
            }
            CollectionKernelPlan::CausalMaskedSoftmax(plan) => {
                return workgroups.saturating_mul(plan.cols);
            }
            // The additive-mask softmax idiom (a `RowSoftmax` plan under
            // the row launch): the row width is the output extent
            // partitioned by the row workgroups — coverage is the row
            // partition itself (an indivisible extent keeps the lane law
            // and fails closed).
            CollectionKernelPlan::RowSoftmax(_) => {
                if let Some(output) = kernel.resources.iter().rev().find(|resource| {
                    matches!(
                        resource.access,
                        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                    )
                }) {
                    let extent = output.version.element_count;
                    if workgroups > 0 && extent % workgroups == 0 {
                        return extent;
                    }
                }
            }
            _ => {}
        }
    }
    lanes
}

/// The recipe's shared-memory footprint in bytes (the host sizes the
/// workgroup-memory budget from this; 0 for elementwise kernels).
fn plan_shared_memory_bytes(plan: &CollectionKernelPlan) -> u64 {
    match plan {
        CollectionKernelPlan::Elementwise
        | CollectionKernelPlan::Transpose(_)
        // S6-P1: the transformer recipes declare no shared-memory layout —
        // each row is one thread's serial pass (max → exp → row sum →
        // divide), so the launch descriptor reports a zero workgroup-memory
        // footprint.
        | CollectionKernelPlan::AxisReduction(_)
        | CollectionKernelPlan::RowSoftmax(_)
        | CollectionKernelPlan::LayerNormalization(_)
        // GI3-1: the inference recipes are likewise row-wise serial passes
        // with no shared-memory layout.
        | CollectionKernelPlan::Gather(_)
        | CollectionKernelPlan::RmsNormalization(_)
        | CollectionKernelPlan::Rope(_)
        | CollectionKernelPlan::CausalMaskedSoftmax(_)
        | CollectionKernelPlan::QuantizedMatMul(_)
        | CollectionKernelPlan::GroupedMatMul(_)
        | CollectionKernelPlan::BatchedAttention(_)
        | CollectionKernelPlan::SsmConv1d(_)
        | CollectionKernelPlan::SsmScan(_)
        | CollectionKernelPlan::LaneGrid { .. }
        | CollectionKernelPlan::SingleLane => 0,
        // The workgroup tree: one `value_bytes + ...` partial per lane.
        CollectionKernelPlan::GridReduce {
            workgroup,
            partial_layout,
            ..
        } => u64::from(*workgroup) * u64::from(partial_layout.element_bytes()),
        CollectionKernelPlan::TreeReduction(plan) => {
            shared_memory_layout_bytes(&plan.shared_memory)
        }
        // SFR-5: the declared distributed sum carries the same tree floor
        // shared-memory layout — one slot per workgroup lane.
        CollectionKernelPlan::DistributedSum(plan) => {
            shared_memory_layout_bytes(&plan.shared_memory)
        }
        CollectionKernelPlan::TiledMatMul(plan) => {
            shared_memory_layout_bytes(&plan.shared_memory.shared_a)
                + shared_memory_layout_bytes(&plan.shared_memory.shared_b)
        }
    }
}

fn shared_memory_layout_bytes(layout: &SharedMemoryLayout) -> u64 {
    u64::from(layout.slot_count)
        .saturating_mul(u64::try_from(layout.element_byte_width).unwrap_or(u64::MAX))
}

// ---------------------------------------------------------------------------
// RECIPE CLASSIFICATION
// ---------------------------------------------------------------------------

/// Whether a collection op is a pointwise tensor transform the generic
/// elementwise kernel body covers (no recipe). This is the closed set that
/// decides the no-recipe plan: an op outside it has no kernel plan and fails
/// closed with a typed diagnostic (N3.2 — never a silent `Elementwise`).
///
/// The set is grounded in the emitters' elementwise arms and the reverse-AD
/// op surface the companion materializer consumes (elementwise
/// mul/div/fill): every member is a uniform pointwise transform.
pub(crate) fn is_elementwise_collection_op(op: MirCollectionOp) -> bool {
    matches!(
        op,
        MirCollectionOp::TensorAdd
            | MirCollectionOp::TensorSub
            | MirCollectionOp::TensorMul
            | MirCollectionOp::TensorDiv
            | MirCollectionOp::TensorNeg
            | MirCollectionOp::TensorFill
            // Explicit static expansion is a reindexed tensor expression.
            // Fusion remains a typed barrier until its planner represents reindexing.
            | MirCollectionOp::TensorExpand
            | MirCollectionOp::TensorRelu
            | MirCollectionOp::TensorGelu
    )
}

/// Whether a collection op is a recipe-bearing op — the closed recipe-op
/// set the plan pass resolves a single [`CollectionKernelPlan`] for under
/// its dispatch contract (S5-U1: reduction, matmul, and the new transpose;
/// S6-P1: the transformer recipes `TensorSumAxis` / `TensorSoftmax` /
/// `TensorLayerNorm`).
///
/// This is the shared boundary predicate for the single-plan scan
/// ([`kernel_plan_for_function`]) and the decomposition slice
/// ([`crate::kernel_decomposition::decompose_kernel_function`]): every op
/// outside it is either an elementwise transform
/// ([`is_elementwise_collection_op`]) or unplannable (a typed diagnostic).
#[must_use]
pub fn is_recipe_op(op: MirCollectionOp) -> bool {
    matches!(
        op,
        MirCollectionOp::Sum
            | MirCollectionOp::TensorMean
            | MirCollectionOp::TensorMax
            | MirCollectionOp::TensorMin
            | MirCollectionOp::TensorMatMul
            | MirCollectionOp::TensorTranspose
            | MirCollectionOp::TensorSumAxis
            | MirCollectionOp::TensorSoftmax
            | MirCollectionOp::TensorLayerNorm
            // GI3-1: the pinned-row inference recipes are recipe boundaries
            // like the transformer ops (admission/resolution is GI3-1).
            | MirCollectionOp::TensorRmsNorm
            | MirCollectionOp::TensorRope
            | MirCollectionOp::TensorRopeHalf
            | MirCollectionOp::TensorGather
            | MirCollectionOp::TensorCausalMaskedSoftmax
            | MirCollectionOp::TensorQuantizedMatMul
            | MirCollectionOp::TensorGroupedMatMul
            | MirCollectionOp::TensorBatchedAttention
            | MirCollectionOp::TensorSsmConv1d
            | MirCollectionOp::TensorSsmScan
    )
}

/// Whether a collection op is one of the S6-P1 transformer recipes — the
/// recipe ops resolved from the function's typed facts by
/// [`CollectionKernelPlan::from_transformer_op`] (no `CollectionOpContract`
/// variant exists for them, so the plan reads the trailing const operands
/// and the operand tensor dims directly).
#[must_use]
pub fn is_transformer_recipe_op(op: MirCollectionOp) -> bool {
    matches!(
        op,
        MirCollectionOp::TensorSumAxis
            | MirCollectionOp::TensorSoftmax
            | MirCollectionOp::TensorLayerNorm
            // GI3-1: the inference recipes follow the same
            // `from_transformer_op`-sibling admission path (no
            // `CollectionOpContract` variant exists for them either).
            | MirCollectionOp::TensorRmsNorm
            | MirCollectionOp::TensorRope
            | MirCollectionOp::TensorRopeHalf
            | MirCollectionOp::TensorGather
            | MirCollectionOp::TensorCausalMaskedSoftmax
    )
}

/// Whether a collection op is a contract-shaped recipe (reduction, matmul,
/// or transpose). Transformer recipes have no `CollectionOpContract` variant
/// and resolve through [`CollectionKernelPlan::from_transformer_op`].
fn is_contract_recipe_op(op: MirCollectionOp) -> bool {
    is_recipe_op(op) && !is_transformer_recipe_op(op)
}

/// Whether a tensor-level unary assign (`MirValueKind::Unary` on a
/// tensor-typed operand) is admitted in the device elementwise surface —
/// the MIR-level admission seam declared by `S6-P1` for the emitter units
/// (`S6-P2`/`S6-P3`) to dispatch tensor-unary emission from.
///
/// `Exp` and `Sqrt` are the `S6-P1` transformer additions (the softmax exp
/// and the `LayerNorm` sqrt/rsqrt lanes); the pre-existing elementwise tensor
/// unaries (`Tanh` / `Relu` / `Gelu`) stay admitted alongside. A unary
/// outside this set on a tensor operand has no elementwise arm and stays
/// fail-closed at the emitters — never a silent fallback.
#[must_use]
pub fn is_admitted_tensor_unary(op: MirUnOp) -> bool {
    matches!(
        op,
        MirUnOp::Tanh | MirUnOp::Relu | MirUnOp::Gelu | MirUnOp::Exp | MirUnOp::Sqrt
            // GI3-1 amendment: the device K/V f16 rounding
            // unary (the comparator's `kv_f16_round` register rounding).
            | MirUnOp::F16Round
    )
}

// ---------------------------------------------------------------------------
// SUBCHAIN SIGNATURES
// ---------------------------------------------------------------------------

/// First collection runtime call that satisfies `visit`, in statement-scan
/// order. Shared by the recipe-operand, destination, and shape scans so each
/// site names the op it wants instead of re-walking the body.
fn find_collection_call<'a, T>(
    function: &'a MirFunction,
    mut visit: impl FnMut(MirCollectionOp, &'a MirRuntimeCall, Option<&'a crate::MirPlace>) -> Option<T>,
) -> Option<T> {
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
                continue;
            };
            if let Some(found) = visit(op, call, destination.as_ref()) {
                return Some(found);
            }
        }
    }
    None
}

fn first_collection_call(
    function: &MirFunction,
    pred: impl Fn(MirCollectionOp) -> bool,
) -> Option<(&MirRuntimeCall, Option<&crate::MirPlace>)> {
    find_collection_call(function, |op, call, dest| pred(op).then_some((call, dest)))
}

/// The element type of a tensor-typed MIR type — the buffer element type the
/// contract-shaped subchain signature needs (mirrors the ABI's
/// `buffer_element_type` for the decomposition seam; typed facts only).
fn tensor_element_ty(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, PlanMappingError> {
    let Type::Tensor(element, _) = validation.types.get(ty.semantic_id()) else {
        return Err(PlanMappingError::mapping(
            "recipe operand requires a tensor type",
        ));
    };
    Ok(MirType::semantic(*element))
}

/// The buffer element type and element count for a data-flow input bound by
/// [`subchain_signature_for_emission`] — a subchain param beyond the recipe
/// operands (a local read from an earlier subchain).
///
/// Tensors bind at their device-view element type and literal dim product
/// ([`tensor_element_ty`] / [`crate::tensor_type::tensor_literal_dims`]); a scalar data-flow
/// param (S5-W-A1: the companion's scalar upstream seed) binds as a
/// 1-element input at the scalar's own type — mirroring how the ABI scalar
/// kernel path treats `F32`/`I32`/`U32`
/// ([`crate::tensor_type::is_kernel_element_layout`]).
///
/// **Fail-closed f64 scalars (S5A-U2):** an f64-class scalar data-flow value
/// is outside the kernel scalar surface and stays fail-closed with the ABI's
/// typed `kernel scalar layout f64` diagnostic — there is no graph-wide
/// f64→f32 narrowing, because graph shape proves nothing about an individual
/// f64 scalar's value (the Stage 5 CTO correction, S5A-U2). The MLP's
/// scalar-return lane is f32 end-to-end (the fixture returns f32 and its
/// upstream seed is f32), so it never enters this arm. Non-scalar,
/// non-tensor layouts stay fail-closed with a typed diagnostic.
pub(crate) fn data_flow_element_ty_and_count(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(MirType, u64), PlanMappingError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar) => {
            Ok((ty, 1))
        }
        MirLayoutKind::Scalar(scalar) => Err(PlanMappingError::mapping(format!(
            "kernel scalar layout {}",
            scalar.spelling()
        ))),
        MirLayoutKind::DeviceView(_) => {
            let element_ty = tensor_element_ty(ty, validation)?;
            let count = crate::tensor_type::tensor_literal_dims(ty, validation)
                .map_err(PlanMappingError::mapping)?
                .into_iter()
                .fold(1u64, u64::saturating_mul);
            Ok((element_ty, count))
        }
        _ => Err(PlanMappingError::mapping(
            "data-flow input requires a tensor or kernel-scalar type",
        )),
    }
}

/// Resolve a data-flow buffer's kernel element type and extent, preserving a
/// run-time-shaped tensor as the launch-time marker while keeping scalar and
/// unsupported-layout handling identical to the literal-only reader.
pub(crate) fn data_flow_element_ty_and_count_or_runtime(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(MirType, u64), PlanMappingError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar) => {
            Ok((ty, 1))
        }
        MirLayoutKind::Scalar(scalar) => Err(PlanMappingError::mapping(format!(
            "kernel scalar layout {}",
            scalar.spelling()
        ))),
        MirLayoutKind::DeviceView(_) => {
            let element_ty = tensor_element_ty(ty, validation)?;
            let dims = crate::tensor_type::tensor_dims(ty, validation)
                .map_err(PlanMappingError::mapping)?;
            let count = match crate::tensor_type::dims_product_checked(&dims) {
                Ok(count) => count,
                Err(crate::tensor_type::NON_LITERAL_DIM_MESSAGE) => {
                    crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT
                }
                Err(message) => return Err(PlanMappingError::mapping(message)),
            };
            Ok((element_ty, count))
        }
        _ => Err(PlanMappingError::mapping(
            "data-flow input requires a tensor or kernel-scalar type",
        )),
    }
}

/// The local type of a recipe-op statement's first operand (the tensor the
/// reduction/transpose recipe reads) — resolved from the subchain body's
/// typed statement scan.
fn recipe_operand_ty(
    function: &MirFunction,
    op: MirCollectionOp,
) -> Result<MirType, PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |found| found == op) else {
        return Err(PlanMappingError::mapping(format!(
            "recipe op {op:?} statement not found in the subchain body"
        )));
    };
    call.args
        .first()
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
        .ok_or_else(|| {
            PlanMappingError::mapping(format!("{op:?} recipe operand has no local type"))
        })
}

fn storage_input(binding: u32, element_ty: MirType, count: u64) -> MirKernelResource {
    MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty,
        element_count: count,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    }
}

fn storage_output(binding: u32, element_ty: MirType, count: u64) -> MirKernelResource {
    MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Output,
        access: MirKernelResourceAccess::Write,
        element_ty,
        element_count: count,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    }
}

/// Build the contract-shaped storage-buffer signature a recipe subchain
/// carries (S5-U1 decomposition driver seam).
///
/// The full ABI synthesis (`MirKernelSignature::storage_buffer_kernel`)
/// cannot build a signature for an arbitrary subchain slice — a rectangular
/// transpose kernel whose `[M,N] → [N,M]` return is not semantically equal
/// to its param, or a matmul subchain whose operands are locals produced by
/// an earlier subchain — so the seam reconstructs exactly the contract facts
/// the plan pass requires: the recipe operands as read inputs at their
/// device-view element counts and the output buffer sized from the resolved
/// dispatch contract (reduction partials / matmul M·N / transpose M·N),
/// with the corrected workgroup/dispatch [`CollectionOpContract`] applies.
/// [`CollectionKernelPlan::from_contract`]'s signature-matching checks then
/// pass for the subchain exactly as they do for a whole-function kernel.
///
/// Extra data-flow inputs (locals read from earlier subchains) are NOT part
/// of this signature: they are a device-program buffer concern, not a plan
/// concern — the plan is decided by the recipe op and its operands alone.
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand types cannot be
/// resolved from the subchain body.
pub fn contract_shape_signature(
    function: &MirFunction,
    contract: &CollectionOpContract,
    validation: &MirValidationContext<'_>,
) -> Result<MirKernelSignature, PlanMappingError> {
    let mut signature = MirKernelSignature {
        function: function.id,
        input: storage_input(0, MirType::semantic(TypeId(0)), 0),
        extra_inputs: Vec::new(),
        output: storage_output(1, MirType::semantic(TypeId(0)), 0),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 0, y: 1, z: 1 },
    };
    match *contract {
        CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        } => {
            let mut reduce_op = MirCollectionOp::Sum;
            find_collection_call(function, |op, _, _| {
                match op {
                    MirCollectionOp::TensorMean => reduce_op = MirCollectionOp::TensorMean,
                    MirCollectionOp::TensorMax if reduce_op != MirCollectionOp::TensorMean => {
                        reduce_op = MirCollectionOp::TensorMax;
                    }
                    MirCollectionOp::TensorMin
                        if !matches!(
                            reduce_op,
                            MirCollectionOp::TensorMean | MirCollectionOp::TensorMax
                        ) =>
                    {
                        reduce_op = MirCollectionOp::TensorMin;
                    }
                    _ => {}
                }
                None::<()>
            });
            let operand_ty = recipe_operand_ty(function, reduce_op)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            signature.input = storage_input(0, element_ty, length);
            signature.output = storage_output(1, element_ty, partials);
            signature.output.source_local = recipe_destination_local_for(function, reduce_op);
            signature.workgroup = MirWorkgroupSize {
                x: workgroup_x,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: length,
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::MatMul {
            dispatch:
                MatMulDispatch::Literal {
                    dispatch_x,
                    dispatch_y,
                },
        } => {
            let shapes = kernel_matmul_shapes(function, validation)?.ok_or_else(|| {
                PlanMappingError::mapping("matmul subchain has no operand shapes")
            })?;
            let (left_ty, right_ty) = matmul_operand_tys(function)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            // U1 head-axis: the signature describes the buffers the launch
            // binds — the operands' FULL tensor extents (`batch·M·K` left,
            // `right_batch·K·N` right, `batch·M·N` output), never the bare
            // rank-2 dims. `from_contract` re-derives the batch counts
            // from exactly these extents.
            signature.input = storage_input(
                0,
                element_ty,
                device_view_element_count(left_ty, validation)?,
            );
            signature.extra_inputs.push(storage_input(
                1,
                element_ty,
                device_view_element_count(right_ty, validation)?,
            ));
            signature.output = storage_output(
                2,
                element_ty,
                signature.input.element_count / shapes.k * shapes.n,
            );
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorMatMul);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::MATMUL_TILE,
                y: crate::kernel_plan::MATMUL_TILE,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime,
        } => {
            // Run-time extents (design note item 4): operand and output
            // buffers are runtime-shaped; M, K and N travel as
            // `TensorAxisExtent` resources appended after the output. The
            // grid comes from the call-site shapes at launch, so
            // `dispatch_size` is left untouched.
            let (left_ty, right_ty) = matmul_operand_tys(function)?;
            let return_ty = matmul_return_ty(function)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            let left_dims = crate::tensor_type::tensor_dims(left_ty, validation)
                .map_err(PlanMappingError::mapping)?;
            let right_dims = crate::tensor_type::tensor_dims(right_ty, validation)
                .map_err(PlanMappingError::mapping)?;
            // Output is the full output tensor extent read from the DECLARED
            // result: every left axis but the last, times the right's last
            // axis — `[B, M, N]` for a batched left, `[M, N]` (or `[M]`) for a
            // rank-2 left. A run-time axis anywhere in that product binds at
            // launch, so the buffer takes the runtime marker; a zero literal
            // axis or an overflowing literal product is refused instead of
            // storing a saturated count. The contract resolver enforces the
            // same ranks and prefix law (rank-2 left, or the batched
            // rank-2..4 form).
            signature.input = storage_input(
                0,
                element_ty,
                device_view_element_count_or_runtime(left_ty, validation)?,
            );
            signature.extra_inputs.push(storage_input(
                1,
                element_ty,
                device_view_element_count_or_runtime(right_ty, validation)?,
            ));
            signature.output = storage_output(
                2,
                element_ty,
                device_view_element_count_or_runtime(return_ty, validation)?,
            );
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorMatMul);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::MATMUL_TILE,
                y: crate::kernel_plan::MATMUL_TILE,
                z: 1,
            };
            crate::abi::append_matmul_runtime_extent_resources(
                &mut signature,
                &left_dims,
                &right_dims,
                validation,
            )?;
        }
        CollectionOpContract::Transpose { m, n } => {
            let operand_ty = recipe_operand_ty(function, MirCollectionOp::TensorTranspose)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            signature.input = storage_input(0, element_ty, m.saturating_mul(n));
            signature.output = storage_output(1, element_ty, m.saturating_mul(n));
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorTranspose);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: m.saturating_mul(n),
                y: 1,
                z: 1,
            };
        }
        // U3 head-axis: the signature describes the buffers the launch
        // binds — the batched operand's FULL `batch·M·N` extent, never the
        // bare per-batch dims. `from_contract` re-derives the batch count
        // from exactly this extent.
        CollectionOpContract::BatchedTranspose { batch, m, n } => {
            let operand_ty = recipe_operand_ty(function, MirCollectionOp::TensorTranspose)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            let count = batch.saturating_mul(m).saturating_mul(n);
            signature.input = storage_input(0, element_ty, count);
            signature.output = storage_output(1, element_ty, count);
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorTranspose);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: count,
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::QuantizedMatMul {
            dispatch_x,
            dispatch_y,
            m,
            k,
            n,
            ..
        } => {
            let left_ty = recipe_operand_ty(function, MirCollectionOp::TensorQuantizedMatMul)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            signature.input = storage_input(0, element_ty, m.saturating_mul(k));
            signature
                .extra_inputs
                .push(storage_input(1, element_ty, k.saturating_mul(n)));
            signature.output = storage_output(2, element_ty, m.saturating_mul(n));
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorQuantizedMatMul);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::MATMUL_TILE,
                y: crate::kernel_plan::MATMUL_TILE,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        CollectionOpContract::GroupedMatMul {
            dispatch_x,
            dispatch_y,
            m,
            k,
            n,
            groups,
            ..
        } => {
            let left_ty = recipe_operand_ty(function, MirCollectionOp::TensorGroupedMatMul)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            signature.input = storage_input(0, element_ty, m.saturating_mul(k));
            signature.extra_inputs.push(storage_input(
                1,
                element_ty,
                groups.saturating_mul(k).saturating_mul(n),
            ));
            signature.output = storage_output(2, element_ty, m.saturating_mul(n));
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorGroupedMatMul);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::MATMUL_TILE,
                y: crate::kernel_plan::MATMUL_TILE,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        // PGC-R5 (verdict 91fe3d59): the row-reduction family. Resources
        // are the SAME facts the transformer seam builds (input at the
        // flattened dims, RMSNorm's gamma as the typed extra input with the
        // output at binding 2, output at the flattened dims); only the
        // launch geometry is the row-reduction contract (one workgroup per
        // row). A mixed subchain (additional consumed outputs) keeps the
        // generic 1D dispatch — a rows grid would under-cover the extra
        // output writes (the R6 P1 union-dispatch law).
        CollectionOpContract::RowReduction { rows, width } => {
            let op = find_collection_call(function, |op, _, _| {
                (op == MirCollectionOp::TensorRmsNorm).then_some(op)
            })
            .unwrap_or(MirCollectionOp::TensorCausalMaskedSoftmax);
            let operand_ty = recipe_operand_ty(function, op)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            let extent = rows.saturating_mul(width);
            signature.input = storage_input(0, element_ty, extent);
            signature.output = storage_output(1, element_ty, extent);
            signature.output.source_local = recipe_destination_local_for(function, op);
            if op == MirCollectionOp::TensorRmsNorm {
                let gamma_local = rms_norm_gamma_local(function).ok_or_else(|| {
                    PlanMappingError::mapping(
                        "TensorRmsNorm gamma peer has no local in the subchain body",
                    )
                })?;
                let gamma_ty = function_local_ty(function, gamma_local)?;
                let (gamma_element_ty, gamma_count) =
                    data_flow_element_ty_and_count(gamma_ty, validation)?;
                signature.extra_inputs.push(storage_input_with_local(
                    1,
                    gamma_element_ty,
                    gamma_count,
                    gamma_local,
                ));
                signature.output.binding = 2;
            }
            let lanes = u64::from(crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            // 1D dispatch law: threads (rows workgroups × lanes).
            signature.dispatch_size = MirKernelDispatchSize {
                x: rows.saturating_mul(lanes),
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::BatchedAttention {
            kv_heads,
            seq,
            head_dim,
            output_elements,
            ..
        } => {
            let op = MirCollectionOp::TensorBatchedAttention;
            // Whole functions carry ABI-only output/dispatch params, while a
            // decomposed recipe subchain retains only the tensor locals read
            // by the batched call. Resolve the call operands directly so both
            // forms preserve every projection peer (cos/sin/bias included).
            let operands = recipe_operand_locals(function)?;
            let first = operands.first().ok_or_else(|| {
                PlanMappingError::mapping(format!(
                    "batched attention function {} has no tensor operands",
                    function.id.0
                ))
            })?;
            let (element_ty, _) =
                data_flow_element_ty_and_count(function_local_ty(function, *first)?, validation)?;
            for (binding, local) in operands.iter().copied().enumerate() {
                let ty = function_local_ty(function, local)?;
                let (operand_ty, extent) = data_flow_element_ty_and_count(ty, validation)?;
                if binding == 0 {
                    signature.input = storage_input_with_local(0, operand_ty, extent, local);
                } else {
                    let mut extra = storage_input_with_local(
                        u32::try_from(binding).unwrap_or(u32::MAX),
                        operand_ty,
                        extent,
                        local,
                    );
                    // Persistent K/V views are written in-shader at
                    // write_position. Mark them ReadWrite so Metal binds
                    // `device` rather than `device const`.
                    if let Ok(dims) = crate::tensor_type::tensor_literal_dims(ty, validation)
                        && dims.len() == 3
                        && dims[0] == kv_heads
                        && dims[2] == head_dim
                        && dims[1] != seq
                    {
                        extra.access = MirKernelResourceAccess::ReadWrite;
                    }
                    signature.extra_inputs.push(extra);
                }
            }
            signature.output = storage_output(
                u32::try_from(operands.len()).unwrap_or(u32::MAX),
                element_ty,
                output_elements,
            );
            signature.output.source_local = recipe_destination_local_for(function, op);
            signature.workgroup = MirWorkgroupSize { x: 64, y: 1, z: 1 };
            signature.dispatch_size = MirKernelDispatchSize {
                x: output_elements,
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::SsmConv1d {
            length, channels, ..
        } => {
            let operand_ty = recipe_operand_ty(function, MirCollectionOp::TensorSsmConv1d)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            let extent = length.saturating_mul(channels);
            signature.input = storage_input(0, element_ty, extent);
            signature.output = storage_output(1, element_ty, extent);
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorSsmConv1d);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: extent,
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::SsmScan {
            length, state_dim, ..
        } => {
            let operand_ty = recipe_operand_ty(function, MirCollectionOp::TensorSsmScan)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            let extent = length.saturating_mul(state_dim);
            signature.input = storage_input(0, element_ty, extent);
            signature.output = storage_output(1, element_ty, extent);
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorSsmScan);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: extent,
                y: 1,
                z: 1,
            };
        }
    }
    Ok(signature)
}

/// Build the transformer-shaped storage-buffer signature a transformer
/// recipe subchain carries (S6-P1 seam, the sibling of
/// [`contract_shape_signature`]).
///
/// The transformer recipes have no `CollectionOpContract` variant, so the
/// plan pass cannot resolve one; the seam reconstructs exactly the facts the
/// plan resolution requires: the recipe operand as the read input at its
/// device-view element count (the flattened input dims), any typed affine or
/// index peer as an extra read input, and the output buffer at the flattened
/// output dims (input dims for the same-shape softmax/LN recipes; input dims
/// minus the reduced axis for `TensorSumAxis`), under the generic 1D contract
/// the ABI applies to these kernels. [`CollectionKernelPlan::from_transformer_op`]'s
/// element-count cross-checks then pass for the subchain exactly as they do
/// for a whole-function kernel.
///
/// # Errors
/// Returns [`PlanMappingError`] when the transformer recipe's typed shape
/// facts (dims/axis/const constraints) or the operand tensor type cannot be
/// resolved from the subchain body.
pub fn transformer_shape_signature(
    function: &MirFunction,
    op: MirCollectionOp,
    validation: &MirValidationContext<'_>,
) -> Result<MirKernelSignature, PlanMappingError> {
    let (dims, axis) = crate::kernel_plan::transformer_recipe_shape(function, validation, op)
        .ok_or_else(|| {
            PlanMappingError::mapping(format!(
                "transformer recipe {op:?} has no resolvable shape facts \
                 (axis/epsilon from the const operands, dims from the tensor types)"
            ))
        })?;
    let input_count = dims.iter().copied().fold(1u64, u64::saturating_mul);
    let output_count = if op == MirCollectionOp::TensorSumAxis {
        dims.iter()
            .enumerate()
            .filter(|(index, _)| *index as u64 != axis)
            .fold(1u64, |acc, (_, &dim)| acc.saturating_mul(dim))
    } else if op == MirCollectionOp::TensorGather {
        // GI3-1: the row-gather result is shape-transforming `[V, D] →
        // [T, D]`; the output count comes from the statement's return-type
        // dims (the CCI-1 source contract carries the concrete `[T, D]`
        // fact), never inferred from text (A3).
        gather_return_count(function, validation)?
    } else {
        input_count
    };
    let operand_local = transformer_operand_local(function, op).ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "transformer recipe {op:?} operand has no local in the subchain body"
        ))
    })?;
    let operand_ty = crate::local_ty::local_ty(function, operand_local).ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "transformer recipe {op:?} operand has no local type"
        ))
    })?;
    let element_ty = tensor_element_ty(operand_ty, validation)?;
    let mut signature = MirKernelSignature {
        function: function.id,
        input: storage_input(0, element_ty, input_count),
        extra_inputs: Vec::new(),
        output: storage_output(1, element_ty, output_count),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        // The generic 1D contract the ABI applies to kernels without a
        // collection-op contract (the transformer recipes have none); the
        // emitter units (S6-P2/P3) own the row-wise thread mapping under it.
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: output_count,
            y: 1,
            z: 1,
        },
    };
    signature.input.source_local = Some(operand_local);
    signature.output.source_local = recipe_destination_local_for(function, op);
    // RMSNorm's affine gamma is a kernel resource, not a carried plan field.
    // Bind the typed rank-1 peer here so both the plan/emitter lookup and the
    // decomposition seam see the same stable input ordering: x, gamma, out.
    if op == MirCollectionOp::TensorRmsNorm {
        bind_rms_norm_affine_resources(&mut signature, function, validation)?;
    }
    // RoPE's interleaved angle table is a caller-owned kernel resource. Bind
    // the exact fourth MIR operand; the planner and emitters must never scan
    // unrelated tensor parameters to discover a table.
    if op.is_rope() {
        let table_local = rope_table_local(function, op).ok_or_else(|| {
            PlanMappingError::mapping(
                "TensorRope table operand has no local in the transformer signature",
            )
        })?;
        let table_ty = function_local_ty(function, table_local)?;
        let (table_element_ty, table_count) = data_flow_element_ty_and_count(table_ty, validation)?;
        signature.extra_inputs.push(storage_input_with_local(
            1,
            table_element_ty,
            table_count,
            table_local,
        ));
        signature.output.binding = 2;
    }
    // Council-8 CB-3: the axis-reduction producer stamps its
    // producer-defined reduced-resource projection on the reduced OUTPUT
    // resource — the mapping a keep-dims consumer uses to address the
    // reduced buffer. The fact is carried (plan → resource → wire version),
    // never reconstructed from element count by a consumer.
    if op == MirCollectionOp::TensorSumAxis {
        signature.output.reduced_projection =
            crate::kernel_plan::axis_reduction_projection(&dims, axis);
    }
    // GI3-1: the row-gather recipe binds its id vector as the signature's
    // first extra input (binding 1, the kernel-index element type) at the
    // id count resolved from the return-type dims; the output moves to
    // binding 2. The CCI-1 contract validated the ids' literal values only
    // — the count is a typed fact, never inferred — and the emitted body
    // bounds-checks the runtime token against the table row count (SEC-G1).
    if op == MirCollectionOp::TensorGather {
        let ids_local = gather_ids_local(function).ok_or_else(|| {
            PlanMappingError::mapping(
                "row-gather recipe ids operand has no local in the subchain body",
            )
        })?;
        let ids_ty = crate::local_ty::local_ty(function, ids_local).ok_or_else(|| {
            PlanMappingError::mapping("row-gather recipe ids operand has no local type")
        })?;
        let id_count = gather_id_count(function, validation)?;
        signature.extra_inputs.push(storage_input(
            1,
            gather_ids_element_ty(ids_ty, validation)?,
            id_count,
        ));
        signature.output.binding = 2;
    }
    Ok(signature)
}

fn bind_rms_norm_affine_resources(
    signature: &mut MirKernelSignature,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<(), PlanMappingError> {
    let epsilon_local = rms_norm_epsilon_local(function);
    if let Some(epsilon_local) = epsilon_local {
        let epsilon_ty = function_local_ty(function, epsilon_local)?;
        let (epsilon_element_ty, epsilon_count) =
            data_flow_element_ty_and_count(epsilon_ty, validation)?;
        signature.extra_inputs.push(storage_input_with_local(
            1,
            epsilon_element_ty,
            epsilon_count,
            epsilon_local,
        ));
    }
    let gamma_local = rms_norm_gamma_local(function).ok_or_else(|| {
        PlanMappingError::mapping(
            "TensorRmsNorm gamma peer has no local in the transformer signature",
        )
    })?;
    let gamma_ty = function_local_ty(function, gamma_local)?;
    let (gamma_element_ty, gamma_count) = data_flow_element_ty_and_count(gamma_ty, validation)?;
    let gamma_binding = if epsilon_local.is_some() { 2 } else { 1 };
    signature.extra_inputs.push(storage_input_with_local(
        gamma_binding,
        gamma_element_ty,
        gamma_count,
        gamma_local,
    ));
    signature.output.binding = gamma_binding + 1;
    Ok(())
}

/// The row-gather return element count (GI3-1): the product of the gather
/// statement's return-type literal dims (`[T, D]`).
fn gather_return_count(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<u64, PlanMappingError> {
    let dims = gather_statement_return_dims(function, validation)?;
    Ok(dims.iter().copied().fold(1u64, u64::saturating_mul))
}

/// The literal dims of the gather statement's return type.
fn gather_statement_return_dims(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<u64>, PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |op| op == MirCollectionOp::TensorGather)
    else {
        return Err(PlanMappingError::mapping(
            "row-gather statement not found in the subchain body",
        ));
    };
    crate::tensor_type::tensor_literal_dims(call.return_ty, validation)
        .map_err(PlanMappingError::mapping)
}

/// The row-gather id-vector length (GI3-1): the return dims' first element
/// (`[T, D]`), the same typed fact the plan resolution cross-checks.
fn gather_id_count(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<u64, PlanMappingError> {
    let dims = gather_statement_return_dims(function, validation)?;
    dims.first()
        .copied()
        .ok_or_else(|| PlanMappingError::mapping("row-gather return shape has no row dimension"))
}

/// The id vector's device element type (GI3-1, K7c): the ids are a rank-1
/// `tensor<u32, [n]>`, whose element is already the kernel `numerus<u32>`
/// index layout (the ABI's index surface). The ids' values are runtime
/// tokens; the emitted body bounds-checks each against the table row count
/// (SEC-G1). Any other ids operand (a `lista`, another element type) stays
/// fail-closed with a typed diagnostic.
fn gather_ids_element_ty(
    ids_ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, PlanMappingError> {
    let Type::Tensor(element, _) = validation.types.get(ids_ty.semantic_id()) else {
        return Err(PlanMappingError::mapping(
            "row-gather ids operand must be a tensor<u32, [n]>",
        ));
    };
    if !matches!(
        validation.types.get(*element),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
    ) {
        return Err(PlanMappingError::mapping(
            "row-gather ids operand must be a tensor<u32, [n]>",
        ));
    }
    Ok(MirType::semantic(*element))
}

/// The ids operand local of the gather statement (the second argument — the
/// first is the table).
fn gather_ids_local(function: &MirFunction) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |op| op == MirCollectionOp::TensorGather)?;
    call.args
        .get(1)
        .and_then(crate::read_analysis::operand_local)
}

/// The recipe operand local of a transformer recipe statement — the first
/// TENSOR arg (const operands like axis/epsilon are plan facts, never
/// buffers). `None` when the op statement is absent or carries no tensor
/// operand.
fn transformer_operand_local(function: &MirFunction, op: MirCollectionOp) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |found| found == op)?;
    call.args
        .iter()
        .find_map(crate::read_analysis::operand_local)
}

/// The `RMSNorm` affine gamma peer is the fourth runtime-call argument after
/// the tensor operand and its two const plan facts. It is data, not a plan
/// field, so the source/signature seam binds its local as a read resource.
fn rms_norm_gamma_local(function: &MirFunction) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |op| op == MirCollectionOp::TensorRmsNorm)?;
    call.args
        .get(3)
        .and_then(crate::read_analysis::operand_local)
}

/// The explicit `RMSNorm` epsilon operand when it is a kernel parameter. A
/// literal epsilon is specialized into the recipe plan and has no resource.
fn rms_norm_epsilon_local(function: &MirFunction) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |op| op == MirCollectionOp::TensorRmsNorm)?;
    match call.args.get(2)? {
        MirOperand::Place(place) if place.projections.is_empty() => match place.base {
            MirPlaceBase::Local(local) => Some(local),
            MirPlaceBase::Temp(_) => None,
        },
        _ => None,
    }
}

/// The explicit caller-provided interleaved `RoPE` table operand (the fourth
/// runtime-call argument after `[x, pos, dim]`).
fn rope_table_local(function: &MirFunction, op: MirCollectionOp) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |found| found == op)?;
    call.args
        .get(3)
        .and_then(crate::read_analysis::operand_local)
}

/// The emission signature for a decomposition subchain kernel (S5-U1b
/// emitter seam): the contract-shaped recipe resources plus the subchain's
/// data-flow inputs as read inputs, with the recipe operands' source locals
/// patched so the emitters' input-buffer wiring resolves.
///
/// The plan pass resolves a subchain's plan from the contract-shaped
/// signature alone ([`contract_shape_signature`] — extra data-flow inputs
/// are a buffer concern, not a plan concern). The emitters additionally need
/// every read input bound: the recipe operands at the contract bindings
/// (input at 0, matmul's right operand at 1), then each remaining data-flow
/// input (a local read from an earlier subchain, e.g. the elementwise
/// operand folded into a reduction subchain's load) as a read input at the
/// next binding, and the contract output buffer last.
///
/// Data-flow inputs bind after the recipe operands: tensor-typed params at
/// their device-view element type and literal dim product, and scalar-typed
/// params (S5-W-A1: the companion's scalar upstream seed, e.g. the MLP
/// backward's f32 seed flowing into a Fill/Div) as 1-element inputs at the
/// scalar's own type — mirroring the ABI kernel scalar surface
/// (`F32`/`I32`/`U32`). An f64-class scalar stays fail-closed exactly as
/// the ABI rejects it; other non-scalar, non-tensor layouts stay fail-closed
/// with a typed diagnostic.
///
/// D-2b-1: `outputs` is the subchain's consumed-output set (D-2a), the
/// locals the synthetic exposes as kernel outputs. The first element is the
/// primary (the contract-shaped [`MirKernelSignature::output`]); each
/// ADDITIONAL output (`outputs[1..]`, the consumed elementwise destinations
/// a later subchain reads) binds as an [`MirKernelSignature::extra_output`]
/// after the primary, sized from its local's type exactly like the input
/// data-flow resolution (device-view element type + literal dim product).
/// `outputs.len() == 1` keeps the existing single-output shape.
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand's tensor type, a
/// data-flow input's tensor type, a scalar data-flow input's kernel layout,
/// or an additional output local's type cannot be resolved from the subchain
/// body.
pub fn subchain_signature_for_emission(
    function: &MirFunction,
    contract: &CollectionOpContract,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
) -> Result<MirKernelSignature, PlanMappingError> {
    subchain_signature_for_emission_with_source(function, contract, validation, outputs, None)
}

/// The source-aware form of [`subchain_signature_for_emission`] (widened
/// d8518267 seam repair): `source` is the ORIGINAL function the subchain was
/// decomposed from. The data-flow count resolution uses it to bind a
/// `TensorSumAxis`-produced input at the REDUCED count — the same count the
/// producing subchain's seam output carries — so the emitters' and the faber
/// constructor's cross-kernel unification see one shape per local
/// (`carried_resources_agree`). `None` keeps the type-derived count (whole-
/// function and test call sites).
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand's tensor type, a
/// data-flow input's tensor type, a scalar data-flow input's kernel layout,
/// or an additional output local's type cannot be resolved from the subchain
/// body (same failure modes as [`subchain_signature_for_emission`]).
pub fn subchain_signature_for_emission_with_source(
    function: &MirFunction,
    contract: &CollectionOpContract,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
    source: Option<&MirFunction>,
) -> Result<MirKernelSignature, PlanMappingError> {
    let mut signature = contract_shape_signature(function, contract, validation)?;
    let operands = recipe_operand_locals(function)?;
    let mut bound: Vec<MirLocalId> = Vec::new();
    for (binding, local) in operands.iter().copied().enumerate() {
        if binding == 0 {
            signature.input.source_local = Some(local);
        } else if let Some(extra) = signature.extra_inputs.get_mut(binding - 1) {
            extra.source_local = Some(local);
        }
        bound.push(local);
    }
    finish_subchain_emission_signature(
        function,
        validation,
        outputs,
        &mut signature,
        &bound,
        source,
    )?;
    Ok(signature)
}

/// Whether a subchain's data-flow surface carries a run-time-shaped buffer:
/// any non-vacuum param type or any outputs local type whose tensor shape has
/// a run-time extent ([`crate::tensor_type::Dim::Runtime`]). Callers use this
/// to pick the runtime-aware [`elementwise_subchain_signature_for_emission`]
/// over the literal-only ABI synthesis, whose buffer element counts cannot
/// carry a run-time extent. A param/local type that is not a tensor (e.g. a
/// kernel scalar) has no shape axes and never reads as runtime-shaped here —
/// the signature builders stay the fail-closed authority on such types.
pub fn subchain_has_runtime_shaped_buffer(
    function: &MirFunction,
    outputs: &[MirLocalId],
    validation: &MirValidationContext<'_>,
) -> bool {
    fn shape_has_runtime_dim(ty: MirType, validation: &MirValidationContext<'_>) -> bool {
        crate::tensor_type::tensor_dims(ty, validation).is_ok_and(|dims| {
            dims.iter()
                .any(|dim| matches!(dim, crate::tensor_type::Dim::Runtime))
        })
    }
    function.params.iter().any(|param| {
        !crate::abi::is_vacuum(param.ty, validation) && shape_has_runtime_dim(param.ty, validation)
    }) || outputs.iter().any(|&local| {
        function_local_ty(function, local).is_ok_and(|ty| shape_has_runtime_dim(ty, validation))
    })
}

/// The emission signature for a decomposed ELEMENTWISE-ONLY subchain: every
/// non-vacuum param binds as a read storage-buffer input in declaration order
/// (the first at binding 0, the rest as extra inputs at the following
/// bindings), then the outputs bind — the primary as the signature output,
/// each additional consumed output as an extra output — all sized by
/// [`data_flow_element_ty_and_count_or_runtime`] so a run-time-shaped tensor
/// binds at the [`crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT`] marker instead
/// of failing the literal-dimension law. A run-time-shaped surface appends one
/// count-one `u32` [`MirKernelResourceKind::RuntimeExtent`] channel after the
/// outputs: the host binds the written tensor's whole element count per call
/// and the emitters' out-of-bounds guard reads it, so the generic launch
/// geometry's rounded-up last workgroup cannot index past the last element.
///
/// Why not [`MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry`]:
/// the whole-function ABI synthesis sizes every buffer with the literal-only
/// `device_view_element_count`, so an elementwise subchain carrying a
/// run-time-shaped intermediate is refused before emission. This builder is
/// the runtime-aware path: the launch geometry (workgroup and dispatch) is a
/// per-call fact for a runtime-shaped kernel — one thread-block of one lane,
/// like the `MatMulDispatch::Runtime` signature — never a literal extent product.
///
/// # Errors
/// Returns [`PlanMappingError`] when `outputs` is empty, or a param/local
/// type is neither a tensor nor a kernel scalar (the same fail-closed law the
/// literal data-flow reader applies).
pub fn elementwise_subchain_signature_for_emission(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
) -> Result<MirKernelSignature, PlanMappingError> {
    if outputs.is_empty() {
        return Err(PlanMappingError::mapping(
            "elementwise subchain signature requires at least one output local",
        ));
    }
    let mut signature = MirKernelSignature {
        function: function.id,
        input: storage_input(0, MirType::semantic(TypeId(0)), 0),
        extra_inputs: Vec::new(),
        output: storage_output(0, MirType::semantic(TypeId(0)), 0),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: crate::abi::called_builtins_for_function(function),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
    };
    let mut next_binding = 0u32;
    let mut bound_primary_input = false;
    let mut runtime_shaped = false;
    for param in &function.params {
        if crate::abi::is_vacuum(param.ty, validation) {
            continue;
        }
        let (element_ty, count) = data_flow_element_ty_and_count_or_runtime(param.ty, validation)?;
        runtime_shaped |= count == crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
        let resource = storage_input_with_local(next_binding, element_ty, count, param.local);
        if bound_primary_input {
            signature.extra_inputs.push(resource);
        } else {
            signature.input = resource;
            bound_primary_input = true;
        }
        next_binding = next_binding.saturating_add(1);
    }
    for (index, &local) in outputs.iter().enumerate() {
        let local_ty = function_local_ty(function, local)?;
        let (element_ty, count) = data_flow_element_ty_and_count_or_runtime(local_ty, validation)?;
        runtime_shaped |= count == crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
        let resource = storage_output_with_local(next_binding, element_ty, count, local);
        if index == 0 {
            signature.output = resource;
        } else {
            signature.extra_outputs.push(resource);
        }
        next_binding = next_binding.saturating_add(1);
    }
    // The launch geometry of a run-time-shaped surface is a per-call fact, and
    // the generic one-dimensional rule rounds that call's element count up to
    // whole `GENERIC_1D_WORKGROUP_X` workgroups: a non-aligned tail such as
    // `[3, 7]` (21 elements) dispatches 128 invocations. The emitted body must
    // therefore guard on the per-call count, not on the placeholder one-lane
    // dispatch and not on nothing. Append the count channel — the same
    // `RuntimeExtent` binding a scalar-shaped kernel carries, which the host
    // populates with the written tensor's whole element count before dispatch
    // — so every invocation past the last element returns before its first
    // indexed load or store. The channel is count-typed (`numerus<u32>`) and
    // only appended when that width resolves; a table without it leaves the
    // surface unbounded, which the Metal emitter refuses at emission.
    if runtime_shaped
        && let Some(extent_ty) = validation
            .types
            .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
    {
        signature.extra_inputs.push(MirKernelResource {
            group: 0,
            binding: next_binding,
            kind: MirKernelResourceKind::RuntimeExtent,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty: MirType::semantic(extent_ty),
            element_count: 1,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        });
    }
    Ok(signature)
}

/// Append the runtime-extent count channel (U2/L7) to a run-time-shaped
/// elementwise emission signature.
///
/// [`elementwise_subchain_signature_for_emission`] sizes a run-time-shaped
/// buffer at the [`crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT`] marker and
/// keeps the one-lane placeholder dispatch, because the real extent is a
/// per-call launch fact. An emitter whose OOB guard compares the dispatch
/// index against a static limit therefore has no honest limit to use: the
/// placeholder `dispatch_size.x == 1` would cull every invocation past the
/// first element and leave the rest of a multi-element output unwritten.
/// The count channel is the fix — the host binds the written output's whole
/// element count at this slot before dispatch and the guard reads it.
///
/// Returns `true` when the resource was appended. A hand-built type table
/// without `numerus<u32>` (unit fixtures) keeps the signature untouched; the
/// production registry interns it for every kernel program.
pub fn append_runtime_extent_count_channel(
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> bool {
    if !signature
        .resources()
        .any(MirKernelResource::is_runtime_shaped)
    {
        return false;
    }
    let Some(extent_ty) = validation
        .types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
    else {
        return false;
    };
    let binding = signature
        .resources()
        .map(|resource| resource.binding)
        .max()
        .map_or(0, |binding| binding.saturating_add(1));
    signature.extra_inputs.push(runtime_extent_count_resource(
        binding,
        MirType::semantic(extent_ty),
    ));
    true
}

/// The runtime-extent count resource a run-time-shaped emission signature
/// carries: a count-one `u32` read input the host populates with the written
/// output's whole element count before dispatch (the same channel the
/// whole-function ABI appends for a scalar-shaped kernel).
fn runtime_extent_count_resource(binding: u32, element_ty: MirType) -> MirKernelResource {
    MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::RuntimeExtent,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty,
        element_count: 1,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    }
}

/// The emission signature for a decomposition subchain kernel carrying a
/// S6-P1 transformer recipe (S6-P1 seam): the transformer-shaped recipe
/// resources plus the subchain's data-flow inputs, with the recipe
/// operand's source local patched so the emitters' input-buffer wiring
/// resolves. Mirrors [`subchain_signature_for_emission`] for the
/// transformer recipes (no `CollectionOpContract` variant exists, so the
/// base signature comes from [`transformer_shape_signature`]).
///
/// The transformer recipes bind exactly one recipe operand (the tensor the
/// recipe reads). Const operands (axis / epsilon) remain plan facts, never
/// buffers. `RMSNorm`'s mandatory gamma peer is a typed extra read resource;
/// other affine peers remain data-flow inputs when their recipe admits them.
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand's tensor type, a
/// data-flow input's tensor type, a scalar data-flow input's kernel layout,
/// or an additional output local's type cannot be resolved from the subchain
/// body.
pub fn transformer_subchain_signature_for_emission(
    function: &MirFunction,
    op: MirCollectionOp,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
) -> Result<MirKernelSignature, PlanMappingError> {
    transformer_subchain_signature_for_emission_with_source(function, op, validation, outputs, None)
}

/// The source-aware form of [`transformer_subchain_signature_for_emission`]
/// (widened d8518267 seam repair): `source` is the ORIGINAL function the
/// subchain was decomposed from. The data-flow count resolution uses it to
/// bind a `TensorSumAxis`-produced input at the REDUCED count — the same
/// count this seam sizes the producing subchain's primary output at — so
/// the emitters' and the faber constructor's cross-kernel unification see
/// one shape per local (`carried_resources_agree`). `None` keeps the
/// type-derived count (whole-function and test call sites).
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand's tensor type, a
/// data-flow input's tensor type, a scalar data-flow input's kernel layout,
/// or an additional output local's type cannot be resolved from the subchain
/// body (same failure modes as [`transformer_subchain_signature_for_emission`]).
pub fn transformer_subchain_signature_for_emission_with_source(
    function: &MirFunction,
    op: MirCollectionOp,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
    source: Option<&MirFunction>,
) -> Result<MirKernelSignature, PlanMappingError> {
    let mut signature = transformer_shape_signature(function, op, validation)?;
    let mut bound: Vec<MirLocalId> = Vec::new();
    if let Some(operand) = transformer_operand_local(function, op) {
        signature.input.source_local = Some(operand);
        bound.push(operand);
    }
    // GI3-1: the row-gather recipe binds its id vector as the
    // transformer-shaped signature's first extra input (binding 1) — the
    // same binding [`transformer_shape_signature`] synthesized, so the
    // data-flow pass never re-binds the ids as a separate input.
    if let Some(ids_local) = gather_ids_local(function) {
        if let Some(extra) = signature.extra_inputs.first_mut() {
            extra.source_local = Some(ids_local);
        }
        bound.push(ids_local);
    }
    // RMSNorm's gamma peer is likewise already a typed resource in the
    // transformer-shaped signature. Mark its local bound so the generic
    // data-flow tail does not append a duplicate gamma buffer after it.
    if let Some(gamma_local) = rms_norm_gamma_local(function) {
        bound.push(gamma_local);
    }
    // RoPE's table resource is already part of the recipe-shaped signature;
    // mark its explicit call operand bound so generic data-flow discovery does
    // not append a duplicate input.
    if op.is_rope()
        && let Some(table_local) = rope_table_local(function, op)
    {
        bound.push(table_local);
    }
    finish_subchain_emission_signature(
        function,
        validation,
        outputs,
        &mut signature,
        &bound,
        source,
    )?;
    // The transformer dispatch must cover EVERY output's extent — the union
    // (max) over the recipe output and the folded elementwise tail's
    // additional consumed outputs (the keep-dims SumAxis shape: a REDUCED
    // `[2]` recipe output with full-shape `[2,8]` tail outputs must dispatch
    // 16 threads, never the reduced 2 — a reduced-only dispatch leaves 14
    // elements of each tail output unwritten). The recipe arm writes its
    // reduced output at the flat index and the tuple tail guards each write
    // by its own extent, so the union dispatch never overruns the smaller
    // recipe buffer (R6 P1, mirroring the elementwise multi-output union).
    apply_transformer_union_dispatch(&mut signature);
    // PGC-R5 (verdict 91fe3d59): the row-reduction family hands its two
    // ops to the contract geometry — one workgroup per row — whenever the
    // admission predicate resolves and the subchain has no additional
    // consumed outputs (a mixed subchain keeps the union 1D dispatch; the
    // emitter's per-element body covers the tail writes).
    if matches!(
        op,
        MirCollectionOp::TensorRmsNorm | MirCollectionOp::TensorCausalMaskedSoftmax
    ) && signature.extra_outputs.is_empty()
    {
        let admission = if op == MirCollectionOp::TensorRmsNorm {
            crate::kernel_plan::rmsnorm_row_reduction_admission
        } else {
            crate::kernel_plan::causal_softmax_row_reduction_admission
        };
        if let Some((dims, _)) =
            crate::kernel_plan::transformer_recipe_shape(function, validation, op)
            && dims.len() == 2
            && admission(dims[0], dims[1]).is_some()
        {
            let lanes = u64::from(crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            // 1D dispatch law: threads (rows workgroups × lanes).
            signature.dispatch_size = MirKernelDispatchSize {
                x: dims[0].saturating_mul(lanes),
                y: 1,
                z: 1,
            };
        }
    }
    Ok(signature)
}

/// Set a transformer subchain signature's dispatch to the union of its
/// output extents: the recipe output plus every additional consumed output
/// (the R6 P1 per-output-guard contract the elementwise multi-output ABI
/// already applies). Same-shape recipes (softmax / layer-norm) keep their
/// full-shape dispatch; the keep-dims `SumAxis` tail lifts it to the full
/// elementwise tail extent.
fn apply_transformer_union_dispatch(signature: &mut MirKernelSignature) {
    let union = signature
        .output_resources()
        .fold(0u64, |acc, output| acc.max(output.element_count));
    signature.dispatch_size.x = union;
}

/// The shared tail of the subchain emission signatures: the data-flow
/// inputs beyond the bound recipe operands, the primary output binding, and
/// the additional consumed outputs (D-2b-1).
///
/// `source` is the ORIGINAL function the subchain was decomposed from (the
/// widened d8518267 seam repair): the data-flow count resolution consults it
/// so a `TensorSumAxis`-produced input binds at the REDUCED count — the same
/// count the producing subchain's seam output carries — instead of the
/// keep-dims local type's element product (the AIR `LayerNorm` VJP types
/// `SumAxis` results at the operand shape). `None` keeps the type-derived
/// count (whole-function and test call sites).
fn finish_subchain_emission_signature(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
    signature: &mut MirKernelSignature,
    bound: &[MirLocalId],
    source: Option<&MirFunction>,
) -> Result<(), PlanMappingError> {
    // Data-flow inputs: params beyond the recipe operands — locals read from
    // earlier subchains. They bind after the recipe inputs, before the
    // output, mirroring the whole-function ABI's inputs-then-output layout.
    // Run-time matmul extent resources (`TensorAxisExtent`) bind after the
    // output, so they do not take a data-flow/output slot.
    let operand_inputs = signature
        .extra_inputs
        .iter()
        .filter(|resource| {
            !matches!(
                resource.kind,
                MirKernelResourceKind::TensorAxisExtent { .. }
            )
        })
        .count();
    let mut next_binding = 1u32
        + u32::try_from(operand_inputs).map_err(|_| {
            PlanMappingError::mapping("subchain signature has too many extra inputs")
        })?;
    for param in &function.params {
        if bound.contains(&param.local) {
            continue;
        }
        // The recipe's own destination param (the mut-out surface,
        // `out ⇇ a.matmul(b)`) is the output buffer the contract shape
        // already carries. Binding it AGAIN as a data-flow input
        // double-binds the buffer, shifts the output binding past what the
        // descriptor records, and the plan pass' operand-count law
        // (`signature_matches_matmul`'s single extra input) refuses the
        // signature — so the mut-out whole-function seam fails closed. The
        // param stays the output resource.
        if signature.output.source_local == Some(param.local) {
            continue;
        }
        let (element_ty, count) = data_flow_element_ty_and_count_or_runtime(param.ty, validation)?;
        // S6-P1 seam repair (widened d8518267): a data-flow input that is a
        // `TensorSumAxis` result of an EARLIER subchain binds at the REDUCED
        // count the producer's seam output carries — the keep-dims local
        // type would over-size the buffer to the operand shape and the faber
        // (function, local) unification would see two shapes for one local
        // (VersionShapeConflict). The producing SumAxis statement lives in an
        // earlier subchain, so it is scanned from the SOURCE function.
        // Council-8 CB-3: the producer-defined reduced-resource projection
        // rides the consumer input too — the keep-dims read is driven by
        // the carried fact, never by count-equality inference.
        let projection = source
            .and_then(|source| sum_axis_destination_projection(source, param.local, validation));
        let count = source
            .and_then(|source| sum_axis_destination_count(source, param.local, validation))
            .unwrap_or(count);
        let mut input = storage_input_with_local(next_binding, element_ty, count, param.local);
        input.reduced_projection = projection;
        signature.extra_inputs.push(input);
        next_binding = next_binding.saturating_add(1);
    }
    // The primary output (the recipe output) binds after the inputs; each
    // ADDITIONAL consumed output binds its own storage-buffer output after
    // it (D-2b-1).
    signature.output.binding = next_binding;
    next_binding = next_binding.saturating_add(1);
    for &output in outputs.iter().skip(1) {
        let local_ty = function_local_ty(function, output)?;
        let (element_ty, count) = data_flow_element_ty_and_count_or_runtime(local_ty, validation)?;
        signature.extra_outputs.push(storage_output_with_local(
            next_binding,
            element_ty,
            count,
            output,
        ));
        next_binding = next_binding.saturating_add(1);
    }
    Ok(())
}

/// The reduced element count of a `TensorSumAxis` destination local in the
/// source function — the flattening of the operand dims minus the reduced
/// axis, exactly the count [`transformer_shape_signature`] sizes the
/// reduction output at (SumAxis-specific: the CCI collection-op variants
/// `TensorRmsNorm` / `TensorRope` / `TensorGather` have no `SumAxis` reduction
/// semantics and are never matched). `None` when the local is not a
/// `TensorSumAxis` destination — a data-flow input with no reduction
/// semantics keeps its type-derived count.
fn sum_axis_destination_count(
    source: &MirFunction,
    local: MirLocalId,
    validation: &MirValidationContext<'_>,
) -> Option<u64> {
    let (dims, axis) = sum_axis_destination_shape(source, local, validation)?;
    Some(
        dims.iter()
            .enumerate()
            .filter(|(index, _)| *index as u64 != axis)
            .fold(1u64, |acc, (_, &dim)| acc.saturating_mul(dim)),
    )
}

/// The producer-defined reduced-resource projection of a `TensorSumAxis`
/// destination local in the source function (council-8 CB-3): `axis_extent`
/// (`dims[axis]`) + `inner_stride` (the dim product after the axis). This is
/// the fact a keep-dims consumer carries on its input resource so the
/// reduced read is never reconstructed from element count — which is
/// ambiguous for equal dims (a `[4,4]` axis-0 vs axis-1 reduction both
/// produce 4 elements but map differently). `None` when the local is not a
/// `TensorSumAxis` destination.
fn sum_axis_destination_projection(
    source: &MirFunction,
    local: MirLocalId,
    validation: &MirValidationContext<'_>,
) -> Option<crate::abi::ReducedProjection> {
    let (dims, axis) = sum_axis_destination_shape(source, local, validation)?;
    crate::kernel_plan::axis_reduction_projection(&dims, axis)
}

/// The operand dims + reduction axis of a `TensorSumAxis` destination local
/// in the source function — the shared scan behind the reduced count and the
/// reduced-resource projection (council-8 CB-3). `None` when the local is
/// not a `TensorSumAxis` destination with resolvable shape facts.
fn sum_axis_destination_shape(
    source: &MirFunction,
    local: MirLocalId,
    validation: &MirValidationContext<'_>,
) -> Option<(Vec<u64>, u64)> {
    find_collection_call(source, |op, call, destination| {
        if op != MirCollectionOp::TensorSumAxis {
            return None;
        }
        let place = destination?;
        let MirPlaceBase::Local(dest_local) = place.base else {
            return None;
        };
        if dest_local != local {
            return None;
        }
        let operand = call.args.first()?;
        let operand_ty = crate::local_ty::operand_ty(source, operand)?;
        let dims = crate::tensor_type::tensor_literal_dims(operand_ty, validation).ok()?;
        let axis = match call.args.get(1)? {
            MirOperand::Constant(MirConstant::Int(axis)) => u64::try_from(*axis).ok()?,
            MirOperand::Constant(MirConstant::UInt(axis)) => *axis,
            _ => return None,
        };
        Some((dims, axis))
    })
}

/// The local a recipe statement produces (its destination place's base
/// local), if the destination is a plain local (A2-2b) — matched to the
/// RECIPE op specifically (D-2b verification, hand-5's hypothesis): a
/// subchain slice can fold elementwise collection statements BEFORE the
/// recipe, and the primary output `source_local` must bind the RECIPE result,
/// never an elementwise destination — a misbound `[2]`-count reduction
/// output would carry a `[2,8]`-shaped source local and conflate with the
/// elementwise buffer. The recipe ops carry no `CollectionOpContract`
/// variant (transformer) or a contract the caller knows the op of
/// (reduction/matmul/transpose), so the op is threaded in; the new CCI
/// collection-op variants (`TensorRmsNorm` / `TensorRope` / `TensorGather`)
/// are never matched here unless they are the declared recipe op.
///
/// The forward-save value a companion subchain reads is produced by an
/// EARLIER subchain of the same decomposed function, and decomposition
/// subchains share the source function's local tables — so recording the
/// produced local on the contract output gives the producer a
/// `MirLocal` origin the consumer (same function, same local) joins by
/// semantic identity. A source-shaped recipe may instead write a result
/// temp that the IMMEDIATELY-following assignment forwards to an unprojected
/// local (the real lowering's assign-then-use shape) — the forwarded local
/// is the recipe's data-flow identity. Destinations without such a local
/// identity (a bare temp with no forward, a projected place, or no
/// destination) yield `None` and the Synthetic origin fallback stays honest.
fn recipe_destination_local_for(function: &MirFunction, op: MirCollectionOp) -> Option<MirLocalId> {
    let (block_index, statement_index, destination) = 'found: {
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (statement_index, statement) in block.statements.iter().enumerate() {
                if crate::collection_ops::statement_collection_op(statement) != Some(op) {
                    continue;
                }
                let MirStatementKind::RuntimeCall { destination, .. } = &statement.kind else {
                    continue;
                };
                break 'found (block_index, statement_index, destination.as_ref());
            }
        }
        return None;
    };
    let place = destination?;
    if !place.projections.is_empty() {
        return None;
    }
    let (mut identity, cursor) = match place.base {
        MirPlaceBase::Local(local) => (local, statement_index),
        MirPlaceBase::Temp(temp) => {
            // The assign-then-use forward: the next statement copies the
            // recipe's result temp into the declared stage local.
            let next = function
                .blocks
                .get(block_index)?
                .statements
                .get(statement_index + 1)?;
            let MirStatementKind::Assign { place, value } = &next.kind else {
                return None;
            };
            let MirPlaceBase::Local(local) = place.base else {
                return None;
            };
            if !place.projections.is_empty() {
                return None;
            }
            let MirValueKind::Operand(MirOperand::Temp(forwarded_temp)) = &value.kind else {
                return None;
            };
            if *forwarded_temp != temp {
                return None;
            }
            (local, statement_index + 1)
        }
    };
    // The mut-out tail: one plain copy of the identity into a declared
    // param (projected or not) is the recipe's host-visible output buffer
    // (`out ⇇ a.matmul(b)` forwards to the param directly;
    // `let total ← a.sum(); out[0] ← total` forwards through the stage
    // local). The param — not the intermediate local — is what the ABI
    // output resource binds, so the emitters' output-write suppression laws
    // key on it.
    if let Some(next) = function
        .blocks
        .get(block_index)
        .and_then(|block| block.statements.get(cursor + 1))
    {
        let MirStatementKind::Assign { place, value } = &next.kind else {
            return Some(identity);
        };
        let MirValueKind::Operand(MirOperand::Place(source)) = &value.kind else {
            return Some(identity);
        };
        if !source.projections.is_empty() {
            return Some(identity);
        }
        let MirPlaceBase::Local(source_local) = source.base else {
            return Some(identity);
        };
        if source_local != identity {
            return Some(identity);
        }
        if let MirPlaceBase::Local(param_local) = place.base
            && function
                .params
                .iter()
                .any(|param| param.local == param_local)
        {
            identity = param_local;
        }
    }
    Some(identity)
}

/// The recipe statement's operand locals, in operand order (the read inputs
/// the contract-shaped signature binds at 0..).
fn recipe_operand_locals(function: &MirFunction) -> Result<Vec<MirLocalId>, PlanMappingError> {
    // Bind the contract recipe's operands, not the first collection statement.
    // Decomposition folds a leading elementwise op into the adjacent recipe
    // subchain; those prefix operands are data-flow inputs, not the recipe.
    let Some((call, _)) = first_collection_call(function, is_contract_recipe_op) else {
        return Err(PlanMappingError::mapping(
            "subchain body has no recipe statement",
        ));
    };
    let operand_end = if matches!(
        call.intrinsic,
        MirIntrinsic::Collection(MirCollectionOp::TensorBatchedAttention)
    ) {
        let axes_base = call.args.len().saturating_sub(5);
        let kind_index = axes_base.saturating_sub(1);
        let kind = match call.args.get(kind_index) {
            Some(MirOperand::Constant(MirConstant::Int(value))) => *value,
            Some(MirOperand::Constant(MirConstant::UInt(value))) => {
                i64::try_from(*value).unwrap_or(-1)
            }
            _ => -1,
        };
        // Projection layout is the const immediately before the kind, while
        // scores carry a scale const there. Fused CausalAttention reuses the
        // context kind with three tensor operands plus a scale const before
        // the kind so WIRE v8 stays unchanged.
        if matches!(kind, 0 | 1 | 2 | 3 | 6 | 7)
            || (kind == 5
                && matches!(
                    call.args.get(kind_index.saturating_sub(1)),
                    Some(MirOperand::Constant(MirConstant::Float(_)))
                ))
        {
            kind_index.saturating_sub(1)
        } else {
            kind_index
        }
    } else {
        call.args.len()
    };
    call.args
        .get(..operand_end)
        .unwrap_or_default()
        .iter()
        .map(|arg| {
            crate::read_analysis::operand_local(arg).ok_or_else(|| {
                PlanMappingError::mapping("recipe operand must reference a tensor local")
            })
        })
        .collect()
}

/// A read input resource carrying a source local (the data-flow inputs the
/// subchain emission signature binds after the recipe operands).
fn storage_input_with_local(
    binding: u32,
    element_ty: MirType,
    count: u64,
    local: MirLocalId,
) -> MirKernelResource {
    let mut resource = storage_input(binding, element_ty, count);
    resource.source_local = Some(local);
    resource
}

/// A write output resource carrying a source local (the subchain's
/// ADDITIONAL consumed outputs the emission signature binds after the
/// primary output — D-2b-1).
fn storage_output_with_local(
    binding: u32,
    element_ty: MirType,
    count: u64,
    local: MirLocalId,
) -> MirKernelResource {
    let mut resource = storage_output(binding, element_ty, count);
    resource.source_local = Some(local);
    resource
}

/// The MIR type of a local in the subchain function — the additional-output
/// sizing source for the emission signature seam.
///
/// # Errors
/// Returns [`PlanMappingError`] when the local is neither a param nor a
/// declared local of the subchain body.
fn function_local_ty(
    function: &MirFunction,
    local: MirLocalId,
) -> Result<MirType, PlanMappingError> {
    crate::local_ty::local_ty(function, local).ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "subchain output local {local:?} has no type in the subchain body"
        ))
    })
}

// ---------------------------------------------------------------------------
// SUBCHAIN PLAN RESOLUTION
// ---------------------------------------------------------------------------

/// Resolve the single plan a subchain kernel carries (S5-U1 decomposition
/// driver seam).
///
/// Consumes the same typed facts the whole-function plan pass consumes —
/// the complete (dead-filtered) op set, the dispatch contract, and the
/// contract-shaped signature ([`contract_shape_signature`]) — so the
/// subchain's plan is decided exactly as a whole-function kernel's is, and
/// a carried plan can be A3 cross-checked against it later. The S3-A5 dead
/// filter resolves reads against `read_context` (the SOURCE function when
/// the plan is resolved for a decomposition subchain: the subchain's
/// produced local is read by a later subchain, which the slice itself does
/// not contain).
///
/// Fail-closed: an unplannable op in the subchain (neither recipe nor
/// elementwise), a recipe op without a resolvable contract or plan, or a
/// subchain whose op set mixes distinct recipes is a typed
/// [`PlanMappingError`] — never a silent `Elementwise`.
///
/// # Errors
/// Returns [`PlanMappingError`] on any of the fail-closed conditions above.
pub fn resolve_subchain_plan(
    function: &MirFunction,
    read_context: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<CollectionKernelPlan, PlanMappingError> {
    let ops = crate::collection_ops::collection_ops_with_read_context(function, read_context);
    let stability_max_dead = crate::abi::stability_max_is_dead(function);
    let mut recipe_op: Option<MirCollectionOp> = None;
    for op in ops {
        // PGC-R5: a DEAD stability-max reduction is not a recipe — the
        // same dead skip the contract resolver applies (a subchain whose
        // only recipe is a dead stability max keeps the elementwise lane;
        // the emitters' dead skips are the precedent).
        if stability_max_dead
            && matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin)
        {
            continue;
        }
        if is_recipe_op(op) {
            if let Some(existing) = recipe_op
                && existing != op
            {
                return Err(PlanMappingError::mapping(format!(
                    "subchain op set mixes distinct recipes ({existing:?} and {op:?}); \
                         the decomposition must split them into separate kernels"
                )));
            }
            recipe_op = Some(op);
        } else if !is_elementwise_collection_op(op) {
            return Err(PlanMappingError::mapping(format!(
                "collection op {op:?} has no kernel plan \
                 (no recipe and not an elementwise transform)"
            )));
        }
    }
    let Some(recipe_op) = recipe_op else {
        return Ok(CollectionKernelPlan::Elementwise);
    };
    if is_transformer_recipe_op(recipe_op) {
        // S6-P1: the transformer recipes have no `CollectionOpContract`
        // variant; the plan resolves from the transformer-shaped signature
        // (the same typed facts the plan pass consumes) — the single source
        // of recipe truth.
        let signature = transformer_shape_signature(function, recipe_op, validation)?;
        return CollectionKernelPlan::from_transformer_op(
            &signature, function, validation, recipe_op,
        )
        .ok_or_else(|| {
            PlanMappingError::mapping(format!(
                "collection op {recipe_op:?} has no kernel plan under its typed facts \
                 (axis/epsilon from the const operands, shapes from the tensor types)"
            ))
        });
    }
    let contract = collection_op_contract(function, validation)?.ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "collection op {recipe_op:?} has a recipe but no resolved dispatch contract"
        ))
    })?;
    let signature = contract_shape_signature(function, &contract, validation)?;
    let runtime_matmul = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Runtime,
    };
    let shapes = if recipe_op == MirCollectionOp::TensorMatMul && contract != runtime_matmul {
        kernel_matmul_shapes(function, validation)?
    } else {
        None
    };
    plan_for_collection_op(&signature, &contract, recipe_op, shapes)
}

/// Matmul operand shapes `(M, K, N)` resolved from the `TensorMatMul`
/// statement's rank-2 left and rank-1/rank-2 right tensor operands — the same
/// literal dims the ABI contract resolver reads. The plan also carries each
/// operand's scalar layout so dense emitters can widen BF16 at load time.
/// `None` when the function carries no matmul statement.
fn kernel_matmul_shapes(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Option<MatMulShapes>, PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |op| op == MirCollectionOp::TensorMatMul)
    else {
        return Ok(None);
    };
    if call.args.len() != 2 {
        return Err(PlanMappingError::mapping(
            "matmul requires exactly two tensor operands",
        ));
    }
    let left_ty = crate::local_ty::operand_ty(function, &call.args[0])
        .ok_or_else(|| PlanMappingError::mapping("matmul left operand has no local type"))?;
    let right_ty = crate::local_ty::operand_ty(function, &call.args[1])
        .ok_or_else(|| PlanMappingError::mapping("matmul right operand has no local type"))?;
    let left_all =
        crate::tensor_type::tensor_dims(left_ty, validation).map_err(PlanMappingError::mapping)?;
    let right_all =
        crate::tensor_type::tensor_dims(right_ty, validation).map_err(PlanMappingError::mapping)?;
    if left_all
        .iter()
        .chain(&right_all)
        .any(|dim| matches!(dim, crate::tensor_type::Dim::Runtime))
    {
        // The contract resolver accepts rank-2 run-time extents
        // (`MatMulDispatch::Runtime`); `MatMulShapes` carries literal dims
        // only, so no plan is built from them until the plan arm lands.
        return Err(PlanMappingError::mapping(
            "matmul with run-time extents has no kernel plan yet",
        ));
    }
    let left_dims = crate::tensor_type::tensor_literal_dims(left_ty, validation)
        .map_err(PlanMappingError::mapping)?;
    let right_dims = crate::tensor_type::tensor_literal_dims(right_ty, validation)
        .map_err(PlanMappingError::mapping)?;
    // U1 head-axis: a rank-3/4 receiver with a rank-2+ right operand is the
    // batched contraction; the argument's leading batch dims must be a
    // prefix of the receiver's (the GQA share). The plan's batch counts
    // derive from the signature's resource extents in `from_contract`;
    // this resolver pins the dims law.
    let batched_rank = left_dims.len() >= 3
        && left_dims.len() <= 4
        && right_dims.len() >= 2
        && right_dims.len() <= left_dims.len();
    if batched_rank {
        for axis in 0..right_dims.len() - 2 {
            if left_dims[axis] != right_dims[axis] {
                return Err(PlanMappingError::mapping(
                    "batched matmul requires the right operand's leading batch dims to be a prefix of the left operand's batch dims",
                ));
            }
        }
        if left_dims[left_dims.len() - 1] != right_dims[right_dims.len() - 2] {
            return Err(PlanMappingError::mapping(
                "matmul inner dimensions must match",
            ));
        }
    } else if left_dims.len() != 2 || !(1..=2).contains(&right_dims.len()) {
        return Err(PlanMappingError::mapping(
            "matmul requires a rank-2 left tensor and rank-1 or rank-2 right tensor",
        ));
    } else if left_dims[1] != right_dims[0] {
        return Err(PlanMappingError::mapping(
            "matmul inner dimensions must match",
        ));
    }
    let left_layout = validation
        .layouts
        .layout_for_type(left_ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    let right_layout = validation
        .layouts
        .layout_for_type(right_ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    let (MirLayoutKind::DeviceView(left_view), MirLayoutKind::DeviceView(right_view)) =
        (&left_layout.kind, &right_layout.kind)
    else {
        return Err(PlanMappingError::mapping(
            "matmul operands require device-view layouts",
        ));
    };
    let (Some(left_element), Some(right_element)) =
        (left_view.element.scalar(), right_view.element.scalar())
    else {
        return Err(PlanMappingError::mapping(
            "matmul operands cannot be vector-element device views",
        ));
    };
    // Batched trailing dims: the last two axes are the contraction's
    // [M, K] / [K, N]; rank-2 keeps `right_dims.get(1)` (the promoted
    // rank-1 RHS carries N = 1).
    let (m_index, k_index) = if left_dims.len() >= 3 {
        (left_dims.len() - 2, left_dims.len() - 1)
    } else {
        (0, 1)
    };
    let n = if right_dims.len() >= 2 {
        right_dims[right_dims.len() - 1]
    } else {
        1
    };
    Ok(Some(MatMulShapes {
        m: left_dims[m_index],
        k: left_dims[k_index],
        n,
        left_layout: left_element,
        right_layout: right_element,
    }))
}

/// U1 head-axis: the left and right operand types of the body's
/// `TensorMatMul` statement — the same typed statement scan
/// [`kernel_matmul_shapes`] resolves dims from, factored so
/// [`contract_shape_signature`] can bind the operands' FULL tensor extents
/// (`batch·M·K` / `right_batch·K·N`) as the resource element counts.
fn matmul_operand_tys(function: &MirFunction) -> Result<(MirType, MirType), PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |op| op == MirCollectionOp::TensorMatMul)
    else {
        return Err(PlanMappingError::mapping(
            "matmul subchain has no TensorMatMul statement",
        ));
    };
    let left_ty = crate::local_ty::operand_ty(function, &call.args[0])
        .ok_or_else(|| PlanMappingError::mapping("matmul left operand has no local type"))?;
    let right_ty = crate::local_ty::operand_ty(function, &call.args[1])
        .ok_or_else(|| PlanMappingError::mapping("matmul right operand has no local type"))?;
    Ok((left_ty, right_ty))
}

/// The declared result type of the body's `TensorMatMul` statement — the
/// `MirRuntimeCall.return_ty` the checked dims reader sizes the output buffer
/// from, so a zero literal axis or an overflowing literal product in the
/// result is a refusal rather than a saturated count.
fn matmul_return_ty(function: &MirFunction) -> Result<MirType, PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |op| op == MirCollectionOp::TensorMatMul)
    else {
        return Err(PlanMappingError::mapping(
            "matmul subchain has no TensorMatMul statement",
        ));
    };
    Ok(call.return_ty)
}

#[cfg(test)]
#[path = "device_program_plans_test.rs"]
pub(crate) mod tests;
