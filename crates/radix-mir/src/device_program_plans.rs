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
//!   CollectionKernelPlan` mapping (elementwise ops map to the no-recipe
//!   [`CollectionKernelPlan::Elementwise`]);
//! - [`kernel_plan_for_function`] — re-derives the plan from a kernel
//!   function's typed facts (statement scan + ABI contract + operand shapes),
//!   used by the emitters to cross-check a device program's carried plan
//!   against the MIR it was built from (A3: typed facts only, never text);
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

use crate::abi::{collection_op_contract, CollectionOpContract, MirAbiError, MirKernelSignature};
use crate::device_program::{Binding, BufferRole, DeviceProgram, DeviceResource, KernelUnit};
use crate::kernel_plan::{CollectionKernelPlan, MatMulShapes, SharedMemoryLayout};
use crate::layout::MirTensorStorageLayout;
use crate::{
    MirCollectionOp, MirFunction, MirIntrinsic, MirKernelDispatchSize, MirKernelWorkgroupCount,
    MirOperand, MirPlaceBase, MirRuntimeCall, MirStatementKind, MirType, MirValidationContext,
    MirWorkgroupSize,
};
use radix_types::{IndexExpr, Type};
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
    fn mapping(shape: impl Into<String>) -> Self {
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
/// ([`CollectionKernelPlan::from_contract`]). Elementwise kernels map to the
/// no-recipe [`CollectionKernelPlan::Elementwise`] — the fallback the shared
/// plan pass documents for kernels without a recipe.
///
/// The caller must already hold the resolved [`CollectionOpContract`] (the
/// sibling of the plan pass; `abi::collection_op_contract`) and, for a
/// `TensorMatMul` op, the exact operand shapes `(M, K, N)`; the contract
/// stops at the tile grid, the recipe needs the exact dims.
#[must_use]
pub fn plan_for_collection_op(
    signature: &MirKernelSignature,
    contract: &CollectionOpContract,
    op: MirCollectionOp,
    matmul_shapes: Option<MatMulShapes>,
) -> CollectionKernelPlan {
    CollectionKernelPlan::from_contract(signature, contract, op, matmul_shapes)
        .unwrap_or(CollectionKernelPlan::Elementwise)
}

/// Re-derive the collection-op plan from a kernel function's typed facts.
///
/// Runs the same typed inputs the shared plan pass consumes — the statement
/// scan's collection op, the ABI dispatch contract, and (for matmul) the
/// operand dims resolved from the operands' tensor types — and returns the
/// plan the function carries, or `None` for elementwise kernels (no recipe).
///
/// The emitters use this to cross-check a [`DeviceProgram`] kernel's **carried**
/// plan against the MIR function it was built from: a carried recipe that
/// disagrees with the function's typed op facts fails closed (A3) instead of
/// emitting a body that contradicts the declared plan.
///
/// # Errors
/// Returns [`PlanMappingError`] when the contract resolver cannot resolve the
/// operand shapes (rank, static dims, inner-dimension agreement).
pub fn kernel_plan_for_function(
    function: &MirFunction,
    signature: &MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionKernelPlan>, PlanMappingError> {
    let Some(contract) = collection_op_contract(function, validation)? else {
        return Ok(None);
    };
    let Some(op) = collection_op_for_function(function) else {
        return Ok(None);
    };
    let matmul_shapes = match op {
        MirCollectionOp::TensorMatMul => kernel_matmul_shapes(function, validation)?,
        _ => None,
    };
    Ok(CollectionKernelPlan::from_contract(
        signature,
        &contract,
        op,
        matmul_shapes,
    ))
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
}

impl RecipeKind {
    fn from_plan(plan: &CollectionKernelPlan) -> Self {
        match plan {
            CollectionKernelPlan::Elementwise => Self::Elementwise,
            CollectionKernelPlan::TiledMatMul(_) => Self::TiledMatMul,
            CollectionKernelPlan::TreeReduction(_) => Self::TreeReduction,
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
        CollectionKernelPlan::Elementwise => 0,
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

/// The first collection op in a kernel body's statement scan (the same scan
/// `abi::collection_op_contract` runs; the plan pass needs the op to
/// distinguish `Sum` from `TensorMean` and to recognize matmul).
fn collection_op_for_function(function: &MirFunction) -> Option<MirCollectionOp> {
    for block in &function.blocks {
        for statement in &block.statements {
            if let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(op),
                        ..
                    },
                ..
            } = &statement.kind
            {
                return Some(*op);
            }
        }
    }
    None
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
/// (params first, then locals) — mirrors `abi::operand_local_ty` for the
/// plan-mapping matmul shape scan.
fn operand_local_ty(function: &MirFunction, operand: &MirOperand) -> Option<MirType> {
    let MirOperand::Place(place) = operand else {
        return None;
    };
    let MirPlaceBase::Local(local) = place.base else {
        return None;
    };
    function
        .params
        .iter()
        .find(|param| param.local == local)
        .map(|param| param.ty)
        .or_else(|| {
            function
                .locals
                .iter()
                .find(|local_entry| local_entry.id == local)
                .map(|local_entry| local_entry.ty)
        })
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
