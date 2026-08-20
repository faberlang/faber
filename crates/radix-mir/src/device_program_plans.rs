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
    collection_op_contract, CollectionOpContract, MirAbiError, MirKernelResource,
    MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature,
    MAX_STORAGE_BUFFERS_PER_KERNEL,
};
use crate::device_program::{
    Binding, BufferRole, DeviceProgram, DeviceProgramError, DeviceResource, KernelLaunchPlan,
    KernelUnit,
};
use crate::kernel_plan::{CollectionKernelPlan, MatMulShapes, SharedMemoryLayout};
use crate::layout::MirTensorStorageLayout;
use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirKernelDispatchSize, MirKernelWorkgroupCount,
    MirLayoutKind, MirLocalId, MirOperand, MirPlaceBase, MirRuntimeCall, MirStatementKind, MirType,
    MirUnOp, MirValidationContext, MirWorkgroupSize,
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
    // A kernel whose storage-buffer binding count exceeds the per-kernel
    // maximum cannot be emitted as ONE kernel (Metal's `[[buffer(N)]]`
    // attribute allows bindings 0..=30). The whole-function plan is refused
    // with a typed diagnostic so the callers (faber, both emitters) fall
    // through to the decomposition path, which splits the body into
    // budget-fitting kernels — never a silent Elementwise floor.
    let buffer_count = signature
        .resources()
        .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
        .count();
    if buffer_count > MAX_STORAGE_BUFFERS_PER_KERNEL {
        return Err(PlanMappingError::mapping(format!(
            "kernel storage-buffer count {buffer_count} exceeds the \
             {MAX_STORAGE_BUFFERS_PER_KERNEL}-buffer binding maximum; \
             the decomposition must split the body"
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
    let matmul_shapes = kernel_matmul_shapes(function, validation)?;
    let mut recipe: Option<CollectionKernelPlan> = None;
    for op in ops {
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

fn adopt_recipe(
    recipe: &mut Option<CollectionKernelPlan>,
    op: MirCollectionOp,
    plan: CollectionKernelPlan,
) -> Result<(), PlanMappingError> {
    if let Some(existing) = recipe.as_ref() {
        if existing != &plan {
            return Err(PlanMappingError::mapping(format!(
                "kernel body mixes collection ops with conflicting recipes \
                 ({op:?} resolves to {plan:?}, but the body already carries {existing:?})"
            )));
        }
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
        None => matches!(carried, CollectionKernelPlan::Elementwise),
        Some(derived) => derived == carried,
    }
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
    let mut expected: FxHashMap<(u32, u32), &MirKernelResource> = FxHashMap::default();
    for resource in signature
        .resources()
        .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
    {
        expected.insert((resource.group, resource.binding), resource);
    }
    if carried.len() != expected.len() {
        return false;
    }
    for resource in carried {
        let key = (resource.binding.group, resource.binding.binding);
        let Some(signature_resource) = expected.remove(&key) else {
            return false;
        };
        let derived = BufferRole::from_abi_role(signature_resource.role, signature_resource.access);
        let compatible = match derived {
            Some(derived_role) => {
                derived_role == resource.buffer.role || resource.buffer.role == BufferRole::InOut
            }
            None => false,
        };
        if !compatible {
            return false;
        }
        if signature_resource.element_ty != resource.version.element_ty
            || signature_resource.element_count != resource.version.element_count
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
            || (signature_resource.reduced_projection.is_some()
                && signature_resource.reduced_projection != resource.version.reduced_projection)
        {
            return false;
        }
    }
    true
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
    /// SSM conv1d recipe (R-PACK-01A).
    SsmConv1d,
    /// SSM linear-recurrence scan recipe (R-PACK-01A).
    SsmScan,
}

impl RecipeKind {
    fn from_plan(plan: &CollectionKernelPlan) -> Self {
        match plan {
            CollectionKernelPlan::Elementwise => Self::Elementwise,
            CollectionKernelPlan::TiledMatMul(_) => Self::TiledMatMul,
            CollectionKernelPlan::TreeReduction(_) => Self::TreeReduction,
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
            CollectionKernelPlan::SsmConv1d(_) => Self::SsmConv1d,
            CollectionKernelPlan::SsmScan(_) => Self::SsmScan,
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

/// Target-neutral layout family for a library body bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindLayout {
    /// A flat one-dimensional body.
    Flat,
    /// Contiguous row-major storage.
    RowMajor,
    /// A non-contiguous view described by the carried strides.
    Strided,
    /// RoPE consecutive-pair layout.
    RopeConsecutivePair { rotated_width: u64 },
    /// RoPE rotate-half layout.
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

/// One optional library selection and its launch bind facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySelection {
    /// Index into [`DeviceProgram`]`::kernels`.
    pub kernel_index: usize,
    /// The logical body selected by the executor. `None` retains the
    /// generated-kernel fallback for a recipe not yet owned by library v0.
    pub library_entry: Option<String>,
    /// Shape/layout/grid facts supplied to the selected body.
    pub bind: BindDescriptor,
}

/// M0-frozen additive executor-plan extension for library calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorPlanExtension {
    /// Extension schema version.
    pub abi_version: u16,
    /// Ordered selection/bind records, one per declared kernel.
    pub selections: Vec<LibrarySelection>,
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
                LibrarySelection {
                    kernel_index,
                    library_entry: library_entry_for_kernel(kernel, recipe),
                    bind: bind_descriptor_for_kernel(kernel),
                }
            })
            .collect(),
    }
}

fn bind_dimensions(plan: &CollectionKernelPlan, output_extent: u64) -> (Vec<u64>, BindLayout) {
    match plan {
        CollectionKernelPlan::RmsNormalization(plan) => {
            if plan.width > 0 && output_extent % plan.width == 0 {
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
            let rows = if plan.width > 0 && output_extent % plan.width == 0 {
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
        CollectionKernelPlan::Transpose(plan) => (vec![plan.m, plan.n], BindLayout::RowMajor),
        CollectionKernelPlan::TreeReduction(plan) => (vec![plan.length], BindLayout::Flat),
        CollectionKernelPlan::Elementwise
        | CollectionKernelPlan::AxisReduction(_)
        | CollectionKernelPlan::RowSoftmax(_)
        | CollectionKernelPlan::LayerNormalization(_)
        | CollectionKernelPlan::Gather(_)
        | CollectionKernelPlan::SsmConv1d(_)
        | CollectionKernelPlan::SsmScan(_) => (vec![output_extent.max(1)], BindLayout::Flat),
    }
}

fn library_entry_for_kernel(kernel: &KernelUnit, recipe: RecipeKind) -> Option<String> {
    let entry = match recipe {
        RecipeKind::RmsNormalization => Some("rms"),
        RecipeKind::Rope => Some("rope"),
        RecipeKind::RowSoftmax | RecipeKind::CausalMaskedSoftmax => Some("softmax"),
        RecipeKind::Elementwise if kernel.entry.starts_with("swiglu") => Some("swiglu"),
        RecipeKind::Elementwise
            if kernel.entry.contains("residual") || kernel.entry.starts_with("sum_o_") =>
        {
            Some("residual")
        }
        _ => None,
    }?;
    Some(entry.to_owned())
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
        let dispatched = dispatched_lanes(&kernel.launch);
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

/// The total lanes the shared dispatch launches: workgroup count ×
/// workgroup size across all three axes.
fn dispatched_lanes(launch: &KernelLaunchPlan) -> u64 {
    launch.workgroup_count.x
        * u64::from(launch.workgroup.x)
        * launch.workgroup_count.y
        * u64::from(launch.workgroup.y)
        * launch.workgroup_count.z
        * u64::from(launch.workgroup.z)
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
        | CollectionKernelPlan::SsmConv1d(_)
        | CollectionKernelPlan::SsmScan(_) => 0,
        CollectionKernelPlan::TreeReduction(plan) => {
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
        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
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
            let reduce_op = if is_recipe_op_scan(function, MirCollectionOp::TensorMean) {
                MirCollectionOp::TensorMean
            } else {
                MirCollectionOp::Sum
            };
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
            dispatch_x,
            dispatch_y,
        } => {
            let shapes = kernel_matmul_shapes(function, validation)?.ok_or_else(|| {
                PlanMappingError::mapping("matmul subchain has no operand shapes")
            })?;
            let left_ty = recipe_operand_ty(function, MirCollectionOp::TensorMatMul)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            signature.input = storage_input(0, element_ty, shapes.m.saturating_mul(shapes.k));
            signature.extra_inputs.push(storage_input(
                1,
                element_ty,
                shapes.k.saturating_mul(shapes.n),
            ));
            signature.output = storage_output(2, element_ty, shapes.m.saturating_mul(shapes.n));
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
/// device-view element count (the flattened input dims) and the output
/// buffer at the flattened output dims (input dims for the same-shape
/// softmax/LN recipes; input dims minus the reduced axis for
/// `TensorSumAxis`), under the generic 1D contract the ABI applies to these
/// kernels. [`CollectionKernelPlan::from_transformer_op`]'s element-count
/// cross-checks then pass for the subchain exactly as they do for a
/// whole-function kernel.
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
    // binding 2. The ids' literal values were statically validated at the
    // source surface — the count is a typed fact, never inferred.
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

/// The id vector's device element type (GI3-1): the ids are a
/// `lista<numerus>` whose literal values were statically validated as
/// non-negative at the source surface, so the device buffer binds at the
/// kernel `numerus<u32>` index layout (the ABI's index surface) — resolved
/// read-only via [`radix_types::TypeTable::find_sized_numeric`] (the "GPU
/// builtin registry interns numerus<u32>" precedent). A non-numerus ids
/// element, or a table without the canonical index layout, stays fail-closed
/// with a typed diagnostic.
fn gather_ids_element_ty(
    ids_ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, PlanMappingError> {
    let Type::Array(element) = validation.types.get(ids_ty.semantic_id()) else {
        return Err(PlanMappingError::mapping(
            "row-gather ids operand must be a lista<numerus>",
        ));
    };
    let (Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)) =
        validation.types.get(*element)
    else {
        return Err(PlanMappingError::mapping(
            "row-gather ids operand must be a lista<numerus>",
        ));
    };
    validation
        .types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
        .map(MirType::semantic)
        .ok_or_else(|| {
            PlanMappingError::mapping(
                "row-gather ids buffer requires the numerus<u32> kernel index layout \
                 in the type table",
            )
        })
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
    if let Some(first) = operands.first() {
        signature.input.source_local = Some(*first);
        bound.push(*first);
    }
    // Matmul binds its right operand as the contract-shaped signature's first
    // extra input (binding 1).
    if let Some(second) = operands.get(1) {
        if let Some(right) = signature.extra_inputs.first_mut() {
            right.source_local = Some(*second);
        }
        bound.push(*second);
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

/// The emission signature for a decomposition subchain kernel carrying a
/// S6-P1 transformer recipe (S6-P1 seam): the transformer-shaped recipe
/// resources plus the subchain's data-flow inputs, with the recipe
/// operand's source local patched so the emitters' input-buffer wiring
/// resolves. Mirrors [`subchain_signature_for_emission`] for the
/// transformer recipes (no `CollectionOpContract` variant exists, so the
/// base signature comes from [`transformer_shape_signature`]).
///
/// The transformer recipes bind exactly one recipe operand (the tensor the
/// recipe reads); the const operands (axis / epsilon) and the optional
/// affine pair (gamma / beta) are NOT recipe operands — the affine tensors
/// are data-flow inputs when they are subchain params, and the consts are
/// plan facts, never buffers.
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
    let mut next_binding = 1u32
        + u32::try_from(signature.extra_inputs.len()).map_err(|_| {
            PlanMappingError::mapping("subchain signature has too many extra inputs")
        })?;
    for param in &function.params {
        if bound.contains(&param.local) {
            continue;
        }
        let (element_ty, count) = data_flow_element_ty_and_count(param.ty, validation)?;
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
        let (element_ty, count) = data_flow_element_ty_and_count(local_ty, validation)?;
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
        let operand_ty = crate::local_ty::operand_local_ty(source, operand)?;
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
/// semantic identity. Non-local destinations (temps, or no destination)
/// yield `None` and the Synthetic origin fallback stays honest.
fn recipe_destination_local_for(function: &MirFunction, op: MirCollectionOp) -> Option<MirLocalId> {
    let (_, destination) = first_collection_call(function, |found| found == op)?;
    match destination?.base {
        MirPlaceBase::Local(local) => Some(local),
        MirPlaceBase::Temp(_) => None,
    }
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
    call.args
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

/// Whether the body's op-set scan carries the given recipe op (the
/// reduction-operator disambiguation for [`contract_shape_signature`]).
fn is_recipe_op_scan(function: &MirFunction, op: MirCollectionOp) -> bool {
    first_collection_call(function, |found| found == op).is_some()
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
    let mut recipe_op: Option<MirCollectionOp> = None;
    for op in ops {
        if is_recipe_op(op) {
            if let Some(existing) = recipe_op {
                if existing != op {
                    return Err(PlanMappingError::mapping(format!(
                        "subchain op set mixes distinct recipes ({existing:?} and {op:?}); \
                         the decomposition must split them into separate kernels"
                    )));
                }
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
    let shapes = if recipe_op == MirCollectionOp::TensorMatMul {
        kernel_matmul_shapes(function, validation)?
    } else {
        None
    };
    plan_for_collection_op(&signature, &contract, recipe_op, shapes)
}

/// Matmul operand shapes `(M, K, N)` resolved from the `TensorMatMul`
/// statement's rank-2 tensor operands — the same literal dims the ABI
/// contract resolver reads (the contract stops at the tile grid; the recipe
/// needs the exact dims for the K-tile loop count, the output guard, and the
/// OOB tile tails). `None` when the function carries no matmul statement.
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
    let left_ty = crate::local_ty::operand_local_ty(function, &call.args[0])
        .ok_or_else(|| PlanMappingError::mapping("matmul left operand has no local type"))?;
    let right_ty = crate::local_ty::operand_local_ty(function, &call.args[1])
        .ok_or_else(|| PlanMappingError::mapping("matmul right operand has no local type"))?;
    let left_dims = crate::tensor_type::tensor_literal_dims(left_ty, validation)
        .map_err(PlanMappingError::mapping)?;
    let right_dims = crate::tensor_type::tensor_literal_dims(right_ty, validation)
        .map_err(PlanMappingError::mapping)?;
    if left_dims.len() != 2 || right_dims.len() != 2 {
        return Err(PlanMappingError::mapping(
            "matmul requires rank-2 tensor operands",
        ));
    }
    if left_dims[1] != right_dims[0] {
        return Err(PlanMappingError::mapping(
            "matmul inner dimensions must match",
        ));
    }
    Ok(Some(MatMulShapes {
        m: left_dims[0],
        k: left_dims[1],
        n: right_dims[1],
    }))
}

#[cfg(test)]
#[path = "device_program_plans_test.rs"]
mod tests;
