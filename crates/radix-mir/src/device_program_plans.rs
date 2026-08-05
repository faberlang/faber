//! Typed device-program plan mapping (council TB5) and launch descriptors.
//!
//! This module is the **typed `MirCollectionOp → CollectionKernelPlan`
//! mapping** the frozen contract names (Stage 0 delivery spec §2 N1.6, S0-A
//! architecture record §6.5): the typed bridge that turns a collection
//! intrinsic plus the resolved dispatch contract into the closed recipe set
//! ([`CollectionKernelPlan`]), and the backend-neutral **launch descriptor**
//! derived from a [`DeviceProgram`]'s kernel units.
//!
//! The recipe itself is produced by the shared plan pass
//! [`CollectionKernelPlan::from_contract`] (in `kernel_plan.rs`, the
//! single implementation every backend lowers from); this module owns the
//! mapping surface the device-program pipeline consumes:
//!
//! - [`plan_for_collection_op`] — the direct `MirCollectionOp →
//!   CollectionKernelPlan` mapping (fail-closed: an op with no recipe under
//!   the resolved contract errors with a typed [`PlanMappingError`], never a
//!   silent [`CollectionKernelPlan::Elementwise`] floor);
//! - [`kernel_plan_for_function`] — re-derives the plan from a kernel
//!   function's typed facts (full statement scan + ABI contract + operand
//!   shapes), used by the emitters to cross-check a device program's carried
//!   plan against the MIR it was built from (A3: typed facts only, never
//!   text). The scan covers ALL statements: a multi-op body is either
//!   verified against the carried plan or rejected with a typed diagnostic —
//!   never guessed from the first matching statement;
//! - [`carried_plan_agrees`] — the agreement predicate for that cross-check;
//! - [`KernelLaunchDescriptor`] + [`launch_descriptors`] — the typed
//!   per-kernel launch facts hosts consume (entry, recipe kind,
//!   workgroup/dispatch/workgroup-count, typed buffer slots, shared-memory
//!   footprint). Every field is a program fact — never inferred from emitted
//!   MSL/LLVM/PTX text (A3).
//!
//! # A3 boundary
//!
//! Nothing in this module reads emitted text, parses strings, or reflects on
//! backend bindings. The mapping consumes the typed device-program and MIR
//! surfaces only; the emitters (and, later, the device-program materializer)
//! drive their output from these typed facts.

use crate::abi::{
    collection_op_contract, CollectionOpContract, MirAbiError, MirKernelResource,
    MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature,
};
use crate::device_program::{Binding, BufferRole, DeviceProgram, DeviceResource, KernelUnit};
use crate::kernel_plan::{CollectionKernelPlan, MatMulShapes, SharedMemoryLayout};
use crate::layout::{MirScalarLayout, MirTensorStorageLayout};
use crate::{
    MirCollectionOp, MirFunction, MirIntrinsic, MirKernelDispatchSize, MirKernelWorkgroupCount,
    MirLayoutKind, MirLocalId, MirOperand, MirPlaceBase, MirRuntimeCall, MirStatementKind,
    MirType, MirValidationContext, MirWorkgroupSize,
};
use radix_types::{IndexExpr, Type, TypeId};
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
/// transform (e.g. `TensorSoftmax`): **never a silent `Elementwise`.**
///
/// `TensorTranspose` is a recipe op (S5-U1): a transpose-bearing body
/// resolves the rank-2 [`CollectionKernelPlan::Transpose`] recipe. A body
/// whose complete op set MIXES distinct recipes cannot carry one plan and
/// fails here with a typed diagnostic — the decomposition pass
/// ([`crate::kernel_decomposition::decompose_kernel_function`]) is the path
/// that splits such bodies into single-recipe subchain kernels.
pub fn kernel_plan_for_function(
    function: &MirFunction,
    signature: &MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionKernelPlan>, PlanMappingError> {
    // Scan ALL statements: the kernel body's plan is decided by its complete
    // collection-op set, never by the first matching statement (N3.2).
    let ops = collection_ops_for_function(function);
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
        // Recipe ops resolve through the shared closed-set predicate
        // ([`is_recipe_op`]) — the same boundary the decomposition slice
        // uses; a second distinct recipe must agree exactly (never guessed).
        if is_recipe_op(op) {
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
            if let Some(existing) = &recipe {
                if existing != &plan {
                    return Err(PlanMappingError::mapping(format!(
                        "kernel body mixes collection ops with conflicting recipes \
                         ({op:?} resolves to {plan:?}, but the body already carries {existing:?})"
                    )));
                }
            }
            recipe = Some(plan);
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
/// intermediate (S2-5) carries the program role InOut on slots whose own
/// ABI is (Output, Write) or (Input, Read) — the program role is a
/// cross-kernel identity fact (N2.6 data-flow identity), not a per-slot
/// derivation. InOut accepts any coherent ABI pair; incoherent pairs (e.g.
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
        let compatible =
            derived == Some(resource.buffer.role) || (resource.buffer.role == BufferRole::InOut);
        if !compatible {
            return false;
        }
        if signature_resource.element_ty != resource.version.element_ty
            || signature_resource.element_count != resource.version.element_count
        {
            return false;
        }
    }
    true
}

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
}

impl RecipeKind {
    fn from_plan(plan: &CollectionKernelPlan) -> Self {
        match plan {
            CollectionKernelPlan::Elementwise => Self::Elementwise,
            CollectionKernelPlan::TiledMatMul(_) => Self::TiledMatMul,
            CollectionKernelPlan::TreeReduction(_) => Self::TreeReduction,
            CollectionKernelPlan::Transpose(_) => Self::Transpose,
        }
    }
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
}

impl KernelLaunchDescriptor {
    /// Build the launch descriptor for one kernel unit.
    #[must_use]
    pub fn from_kernel_unit(kernel: &KernelUnit, kernel_index: usize) -> Self {
        Self {
            kernel_index,
            entry: kernel.entry.clone(),
            recipe: RecipeKind::from_plan(&kernel.plan),
            workgroup: kernel.launch.workgroup,
            dispatch: kernel.launch.dispatch_size,
            workgroup_count: kernel.launch.workgroup_count,
            slots: kernel.resources.iter().map(launch_buffer_slot).collect(),
            shared_memory_bytes: plan_shared_memory_bytes(&kernel.plan),
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

fn launch_buffer_slot(resource: &DeviceResource) -> LaunchBufferSlot {
    LaunchBufferSlot {
        binding: resource.binding,
        element_ty: resource.version.element_ty,
        element_count: resource.version.element_count,
        role: resource.buffer.role,
        storage: resource.buffer.storage,
    }
}

/// The recipe's shared-memory footprint in bytes (the host sizes the
/// workgroup-memory budget from this; 0 for elementwise kernels).
fn plan_shared_memory_bytes(plan: &CollectionKernelPlan) -> u64 {
    match plan {
        CollectionKernelPlan::Elementwise | CollectionKernelPlan::Transpose(_) => 0,
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

/// The distinct collection ops in a kernel body's statement scan, in scan
/// order (the same scan `abi::collection_op_contract` runs; the plan pass
/// needs the FULL op set — never the first match only (N3.2) — to decide
/// recipe presence and to reject unplannable ops with a typed diagnostic).
///
/// A collection op whose result is never read is DEAD (S3-A5: the companion
/// recomputes the forward's mean without using it) — it is not part of the
/// kernel's semantics and must not drive the plan (a dead reduction must not
/// force a reduction contract on an elementwise kernel).
///
/// The dead filter resolves reads against `read_context` — the function the
/// op set is scanned from when that is the live context, or the SOURCE
/// function when the scan runs over a decomposition subchain (the
/// subchain's produced local is read by a LATER subchain, invisible inside
/// the subchain slice itself).
fn collection_ops_with_read_context(
    function: &MirFunction,
    read_context: &MirFunction,
) -> Vec<MirCollectionOp> {
    let mut ops: Vec<MirCollectionOp> = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            if let MirStatementKind::RuntimeCall {
                destination,
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(op),
                        ..
                    },
                ..
            } = &statement.kind
            {
                if destination
                    .as_ref()
                    .is_some_and(|dest| !crate::read_analysis::place_is_read(read_context, dest))
                {
                    continue;
                }
                if !ops.contains(op) {
                    ops.push(*op);
                }
            }
        }
    }
    ops
}

fn collection_ops_for_function(function: &MirFunction) -> Vec<MirCollectionOp> {
    collection_ops_with_read_context(function, function)
}


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
/// its dispatch contract (S5-U1: reduction, matmul, and the new transpose).
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
    )
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
/// ([`tensor_element_ty`] / [`tensor_literal_dims`]); a scalar data-flow
/// param (S5-W-A1: the companion's scalar upstream seed) binds as a
/// 1-element input at the scalar's own type — mirroring how the ABI scalar
/// kernel path treats `F32`/`I32`/`U32` (`abi::is_kernel_element_layout`).
/// A scalar outside that surface (e.g. f64) stays fail-closed with the
/// ABI's own typed diagnostic; non-scalar, non-tensor layouts stay
/// fail-closed with a typed diagnostic.
fn data_flow_element_ty_and_count(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(MirType, u64), PlanMappingError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if is_kernel_element_layout(scalar) => Ok((ty, 1)),
        MirLayoutKind::Scalar(scalar) => Err(PlanMappingError::mapping(format!(
            "kernel scalar layout {}",
            scalar.spelling()
        ))),
        MirLayoutKind::DeviceView(_) => {
            let element_ty = tensor_element_ty(ty, validation)?;
            let count = tensor_literal_dims(ty, validation)?
                .into_iter()
                .fold(1u64, u64::saturating_mul);
            Ok((element_ty, count))
        }
        _ => Err(PlanMappingError::mapping(
            "data-flow input requires a tensor or kernel-scalar type",
        )),
    }
}

/// Whether a scalar layout is a kernel buffer element — the ABI's
/// `is_kernel_element_layout` surface (the only scalar layouts a kernel
/// storage buffer can carry).
fn is_kernel_element_layout(scalar: MirScalarLayout) -> bool {
    matches!(
        scalar,
        MirScalarLayout::F32 | MirScalarLayout::I32 | MirScalarLayout::U32
    )
}

/// The local type of a recipe-op statement's first operand (the tensor the
/// reduction/transpose recipe reads) — resolved from the subchain body's
/// typed statement scan.
fn recipe_operand_ty(
    function: &MirFunction,
    op: MirCollectionOp,
) -> Result<MirType, PlanMappingError> {
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(statement_op),
                        args,
                        ..
                    },
                ..
            } = &statement.kind
            else {
                continue;
            };
            if *statement_op != op {
                continue;
            }
            return args
                .first()
                .and_then(|arg| operand_local_ty(function, arg))
                .map_or_else(
                    || {
                        Err(PlanMappingError::mapping(format!(
                            "{op:?} recipe operand has no local type"
                        )))
                    },
                    Ok,
                );
        }
    }
    Err(PlanMappingError::mapping(format!(
        "recipe op {op:?} statement not found in the subchain body"
    )))
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
            signature.output.source_local = recipe_destination_local(function);
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
            signature.output.source_local = recipe_destination_local(function);
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
            signature.output.source_local = recipe_destination_local(function);
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
    }
    Ok(signature)
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
        signature.extra_inputs.push(storage_input_with_local(
            next_binding,
            element_ty,
            count,
            param.local,
        ));
        next_binding = next_binding.saturating_add(1);
    }
    // The primary output (the contract-shaped recipe output) binds after the
    // inputs; each ADDITIONAL consumed output binds its own storage-buffer
    // output after it (D-2b-1).
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
    Ok(signature)
}

/// The local a recipe statement produces (its destination place's base
/// local), if the destination is a plain local (A2-2b).
///
/// The forward-save value a companion subchain reads is produced by an
/// EARLIER subchain of the same decomposed function, and decomposition
/// subchains share the source function's local tables — so recording the
/// produced local on the contract output gives the producer a
/// `MirLocal` origin the consumer (same function, same local) joins by
/// semantic identity. Non-local destinations (temps, or no destination)
/// yield `None` and the Synthetic origin fallback stays honest.
fn recipe_destination_local(function: &MirFunction) -> Option<MirLocalId> {
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall {
                destination,
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(_),
                        ..
                    },
                ..
            } = &statement.kind
            else {
                continue;
            };
            let Some(place) = destination else {
                return None;
            };
            let MirPlaceBase::Local(local) = place.base else {
                return None;
            };
            return Some(local);
        }
    }
    None
}

/// The recipe statement's operand locals, in operand order (the read inputs
/// the contract-shaped signature binds at 0..).
fn recipe_operand_locals(function: &MirFunction) -> Result<Vec<MirLocalId>, PlanMappingError> {
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(_),
                        args,
                        ..
                    },
                ..
            } = &statement.kind
            else {
                continue;
            };
            return args
                .iter()
                .map(|arg| {
                    crate::read_analysis::operand_local(arg).ok_or_else(|| {
                        PlanMappingError::mapping("recipe operand must reference a tensor local")
                    })
                })
                .collect();
        }
    }
    Err(PlanMappingError::mapping(
        "subchain body has no collection-op statement",
    ))
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

/// The MIR type of a local in the subchain function (params, then declared
/// locals) — the shared params-then-locals lookup the signature seam uses
/// (mirrors `abi::operand_local_ty` and the decomposition's local-type
/// resolution).
fn local_ty(function: &MirFunction, local: MirLocalId) -> Option<MirType> {
    function
        .params
        .iter()
        .find(|param| param.local == local)
        .map(|param| param.ty)
        .or_else(|| {
            function
                .locals
                .iter()
                .find(|entry| entry.id == local)
                .map(|entry| entry.ty)
        })
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
    local_ty(function, local).ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "subchain output local {local:?} has no type in the subchain body"
        ))
    })
}

/// Whether the body's op-set scan carries the given recipe op (the
/// reduction-operator disambiguation for [`contract_shape_signature`]).
fn is_recipe_op_scan(function: &MirFunction, op: MirCollectionOp) -> bool {
    for block in &function.blocks {
        for statement in &block.statements {
            if let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(statement_op),
                        ..
                    },
                ..
            } = &statement.kind
            {
                if *statement_op == op {
                    return true;
                }
            }
        }
    }
    false
}

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
    let ops = collection_ops_with_read_context(function, read_context);
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
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                        args,
                        ..
                    },
                ..
            } = &statement.kind
            else {
                continue;
            };
            if args.len() != 2 {
                return Err(PlanMappingError::mapping(
                    "matmul requires exactly two tensor operands",
                ));
            }
            let left_ty = operand_local_ty(function, &args[0]).ok_or_else(|| {
                PlanMappingError::mapping("matmul left operand has no local type")
            })?;
            let right_ty = operand_local_ty(function, &args[1]).ok_or_else(|| {
                PlanMappingError::mapping("matmul right operand has no local type")
            })?;
            let left_dims = tensor_literal_dims(left_ty, validation)?;
            let right_dims = tensor_literal_dims(right_ty, validation)?;
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
            return Ok(Some(MatMulShapes {
                m: left_dims[0],
                k: left_dims[1],
                n: right_dims[1],
            }));
        }
    }
    Ok(None)
}

/// Resolve a runtime-call operand to the MIR type of the local it references
/// (via [`local_ty`]) — mirrors `abi::operand_local_ty` for the plan-mapping
/// matmul shape scan.
fn operand_local_ty(function: &MirFunction, operand: &MirOperand) -> Option<MirType> {
    let MirOperand::Place(place) = operand else {
        return None;
    };
    let MirPlaceBase::Local(local) = place.base else {
        return None;
    };
    local_ty(function, local)
}

/// Literal shape dims of a tensor-typed MIR type (a flat literal is a rank-1
/// shape; a tuple is the dim list) — mirrors `abi::tensor_literal_dims`.
fn tensor_literal_dims(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<u64>, PlanMappingError> {
    let Type::Tensor(_, shape) = validation.types.get(ty.semantic_id()) else {
        return Err(PlanMappingError::mapping(
            "kernel collection op requires tensor types",
        ));
    };
    match validation.types.get_index(*shape) {
        IndexExpr::Literal(count) => Ok(vec![*count]),
        IndexExpr::Tuple(dims) => {
            let mut out = Vec::with_capacity(dims.len());
            for &dim in dims {
                match validation.types.get_index(dim) {
                    IndexExpr::Literal(count) => out.push(*count),
                    _ => {
                        return Err(PlanMappingError::mapping(
                            "kernel collection op requires literal dimensions",
                        ))
                    }
                }
            }
            Ok(out)
        }
        _ => Err(PlanMappingError::mapping(
            "kernel collection op requires static shape",
        )),
    }
}

#[cfg(test)]
#[path = "device_program_plans_test.rs"]
mod tests;
