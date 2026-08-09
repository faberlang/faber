//! Device-program construction + packaged payload codec (S1-6 vertical slice).
//!
//! This module is the faber-owned half of the S1-6 consumption seam (the
//! differentiable-GPU campaign, `docs/factory/gpu-training-lowering/`): it
//! carries one proven collection kernel from **Faber source** through the
//! **common device program** (the S1-1 `DeviceProgram` schema), the
//! **packaged FMIR image** (the S1-2 `device` section), and the **composite
//! host** (S1-4) that runs it on real Metal/CUDA sessions — all through the
//! ordinary `faber run --backend <metal|cuda>` command.
//!
//! # What this module owns
//!
//! - [`device_program_for_lowered`] — the device-program constructor: scans
//!   the lowered package MIR for `@ nucleum` compute kernels (functions whose
//!   `shader_stage` is [`MirKernelShaderStage::Compute`]) and composes the
//!   target-neutral [`DeviceProgram`] from typed facts only (A3): the ABI
//!   kernel signature, the shared plan pass
//!   ([`kernel_plan_for_function`] — the typed
//!   `MirCollectionOp → CollectionKernelPlan` bridge), the typed storage
//!   buffers, and the derived launch plan.
//! - [`wire_program_for_program`] / [`admit_device_program_section`] — the
//!   codec-v3 (S3-A4) canonical wire: the former serializes the complete
//!   typed program (kernels, launches, results, per-resource access +
//!   version) into the typed [`FmirDeviceProgramSection`] wire of the FMIR
//!   `device` section; the latter admits it fail-closed, gating on the
//!   `payload_version` check before any field-level interpretation. CUDA
//!   symbols and host input values are not program semantics — they never
//!   enter the canonical bytes. The S1-3 typed logical-entry → NVVM symbol
//!   mapping ([`CudaKernelIdentity`]) now rides the per-artifact symbols
//!   metadata and is consumed by [`descriptor_for_backend`] when it
//!   constructs the CUDA descriptor.
//! - [`descriptor_for_backend`] — maps a parsed run plan + a declared backend
//!   artifact blob onto the S1-4 host [`DeviceDescriptor`], and
//!   [`execute_device_route`] — the ordinary-command launch seam that
//!   constructs the composite host, executes the descriptor (load → allocate
//!   → copy-in → launch → sync → readback → release), and prints an A9-style
//!   receipt (selected hardware, module hash, launches, transfers, readbacks,
//!   output values).
//!
//! # Ownership boundaries (N2/N3)
//!
//! The shared schemas remain single-writer in radix: this module consumes
//! `radix-mir`'s `DeviceProgram` / plan-mapping and the S1-3 emitters; it
//! never forks the schema. The host descriptor surface is hosts' S1-4
//! `faber-host-macos-arm64`; faber constructs descriptors from typed facts,
//! never by parsing emitted MSL/PTX text (A3).

use faber::device::{DeviceBackend, DeviceSelection};
use faber_host_macos_arm64::composite_host::ProgramSession;
use faber_host_macos_arm64::device_descriptor::{
    DescriptorBuffer, DescriptorBufferVersion, DescriptorDataFlow as HostDescriptorDataFlow,
    DescriptorEndOfRunResult, DescriptorKernel, DescriptorLaunch, DescriptorResult,
    DeviceBufferInitialization, DeviceBufferLifetime, DeviceBufferRole, DeviceDataType,
    DeviceDescriptor, DeviceProgramLifetime as HostDeviceProgramLifetime,
};
use radix::diagnostics::Diagnostic;
use radix::hir::DefId;
use radix::lexer::Interner;
use radix::mir::{MirFunction, MirKernelShaderStage, ValidatedMir};
use radix::semantic::Type;
use radix::semantic::TypeTable;
use radix_mir::abi::{
    collection_op_contract, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceRole, MirKernelSignature,
};
use radix_mir::device::MirCompanionDerivativeKind;
use radix_mir::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit,
    ObservationCadence,
};
use radix_mir::device_program_plans::{
    is_transformer_recipe_op, kernel_plan_for_function,
    subchain_signature_for_emission_with_source,
    transformer_subchain_signature_for_emission_with_source,
};
use radix_mir::device_semantics::{
    DependencyEdge, DeviceSemantics, InitializationFact, InitializationPolicy,
    LosslessMirCompanionEntry, ObservationFact, SemanticValue, SemanticValueId,
    SemanticValueOrigin, ValueBinding, ValueGeneration,
};
use radix_mir::kernel_decomposition::decompose_kernel_function;
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_mir::layout::MirTensorStorageLayout;
use radix_mir::names::MirNames;
use radix_mir::{
    MirCallee, MirCollectionOp, MirConstant, MirFunctionId, MirIntrinsic, MirLocalId, MirOperand,
    MirPlace, MirPlaceBase, MirProjection, MirStatementKind, MirTempId, MirTerminatorKind, MirType,
    MirValueKind,
};
use radix_mir_fmir::schema::{
    WireCompanionDerivativeKind, WireCompanionRelation, WireCompanionSelectedInput,
    WireCompanionSelectedOutput,
};
use radix_mir_fmir::{
    DeviceArtifactFormat, DevicePayloadEncoding, DeviceTargetId, FmirDeviceArtifact,
    FmirDeviceArtifactsSection, FmirDeviceBackend, FmirDeviceInput, FmirDeviceProgramSection,
    FmirDeviceSection, FmirDeviceSelection, FmirDeviceSymbol, MaterializationStage,
    WireBarrierPhase, WireBarrierPoint, WireBinding, WireBufferIdentity, WireBufferLifetime,
    WireBufferRole, WireBufferVersion, WireCollectionKernelPlan, WireDependencyEdge,
    WireDeviceProgram, WireDeviceResource, WireDispatchSize, WireInitializationPolicy,
    WireKernelLaunchPlan, WireKernelUnit, WireLaunchUnit, WireMatMulPlan, WireMatMulSharedMemory,
    WireObservationCadence, WireObservationFact, WireOobPaddingPolicy, WireProgramLifetime,
    WireReduceOp, WireReductionPlan, WireResourceAccess, WireResultBuffer, WireSemanticValue,
    WireSemanticValueOrigin, WireSharedMemoryLayout, WireStorageLayout, WireTransposePlan,
    WireWorkgroupCount, WireWorkgroupSize,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

/// Stable fail-closed diagnostic for a device-program construction failure.
fn device_diag(context: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("device program {context}: {}", message.into()))
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")
}

/// NGAB1-U2: the typed diagnostic for a rejected cross-boundary value. The
/// versioned call-ABI gate (radix-mir `boundary_abi`) fails at compile time
/// with one typed class — wrong type, shape, lifetime, mutation of a
/// read-only resource, or observation of unlaunched work — carried on the
/// diagnostic so a fixture can assert rejection without a launch.
fn boundary_abi_diag(class: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("boundary call ABI: {}", message.into()))
        .with_arg("issue", "E_DEVICE_BOUNDARY_ABI")
        .with_arg("class", class)
}

mod prefill_run;
mod program;
mod run;
mod section;
mod training;
mod wire;

// Cross-cluster glue: siblings reach each other through these device-root
// bindings (the mir/ split carries the same seams via `use super::*`; the
// explicit lists here keep them without wildcard imports).
use program::function_has_shape_construction;
use section::{inputs_by_buffer_id, wire_buffer_name};

// External contract: package/mod.rs and package/mir/* consume these at the
// device root; the sibling seams (program → training, section → wire,
// run → section/wire/training) resolve through the same re-exports.
pub(crate) use program::device_program_for_lowered;

/// NGAB1-U1 host-partition wire: derive the typed host/device partition of a
/// lowered package — the host-side functions, the device kernels, the typed,
/// target-neutral [`radix_mir::HostPartition`] device program, and the
/// declared cross-boundary calls — from typed MIR facts only (NGAB0
/// §Partition/§Abi: identity/type/lifetime facts are never reconstructed from
/// emitted LLVM/MSL/PTX text or naming conventions).
///
/// `Ok(None)` when the package carries no device kernels (no device payload),
/// mirroring [`device_program_for_lowered`].
#[allow(dead_code, unused_imports)] // seam kept for the NGAB1 host-partition tests/consumers
pub(crate) fn host_partition_for_lowered(
    lowered: &radix::mir::LoweredMirUnit<'_>,
) -> Result<Option<radix_mir::HostPartition>, Vec<Diagnostic>> {
    use radix_mir::boundary_abi::BoundaryAbiClass;
    use radix_mir::host_partition::HostPartitionError;
    match radix_mir::host_partition::derive_host_partition(&lowered.validated, &lowered.interner) {
        Ok(partition) if partition.has_device() => Ok(Some(partition)),
        Ok(_) => Ok(None),
        Err(HostPartitionError::Boundary(error)) => Err(vec![boundary_abi_diag(
            error.class().spelling(),
            error.to_string(),
        )]),
        Err(error) => Err(vec![device_diag("host partition", error.to_string())]),
    }
}
// The Q1-default prefill device-run driver (GI3-5). The burgus Metal device
// run is the gated consumer step (env-gated integration test); the lib build
// keeps the seam for the route + tests.
#[allow(dead_code, unused_imports)] // seam kept for the gated device-run consumer
pub(crate) use prefill_run::run_prefill_device_route;
pub(crate) use run::execute_device_route;
pub(crate) use section::{artifact_for_backend, descriptor_for_backend};
pub(crate) use section::{device_section_for_program, DeviceSectionBuild};
pub(crate) use training::{training_plan_facts, tuple_return_locals, DEFAULT_TRAINING_STEPS};
pub(crate) use wire::{
    admit_device_program_section, admit_session_section, wire_program_for_program,
    DEVICE_RUN_PLAN_VERSION,
};

// Test surface: device_test.rs's `use super::*` resolves the constructor's
// training-plan facts, the run seam, and the wire-graph test cluster here.
#[cfg(test)]
#[allow(unused_imports)]
// the run/training types ride the device root even when a test does not name one directly.
pub(crate) use run::{
    declared_end_of_run_observations, declared_per_step_observations, device_repeat_count,
    device_step_count, execute_session_receipts, host_receipt_graph_lines,
    host_receipt_launch_order_line, step_run_report, EndOfRunObservationSet, StepRunReport,
};
#[cfg(test)]
#[allow(unused_imports)]
// WireGraphBuffer rides the cluster's re-export (tests name only the edge type).
pub(crate) use section::{
    observation_buffer_ids, wire_resource_graph, WireGraphBuffer, WireGraphEdge,
};
#[cfg(test)]
#[allow(unused_imports)]
// the training-plan types ride the device root even when a test does not name one directly.
pub(crate) use training::{admit_step_count, GradientLink, TrainableParam, TrainingPlanFacts};

#[cfg(test)]
#[path = "device_test.rs"]
mod tests;
