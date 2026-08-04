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
    DescriptorKernel, DescriptorLaunch, DescriptorResult, DeviceBufferInitialization,
    DeviceBufferLifetime, DeviceBufferRole, DeviceDataType, DeviceDescriptor,
    DeviceProgramLifetime as HostDeviceProgramLifetime,
};
use radix::diagnostics::Diagnostic;
use radix::hir::DefId;
use radix::lexer::Interner;
use radix::mir::{MirFunction, MirKernelShaderStage, ValidatedMir};
use radix::semantic::Type;
use radix_mir::abi::{
    collection_op_contract, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceRole, MirKernelSignature,
};
use radix_mir::device::MirCompanionDerivativeKind;
use radix_mir::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit,
};
use radix_mir::device_program_plans::{
    kernel_plan_for_function, subchain_signature_for_emission,
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
    MirPlace, MirPlaceBase, MirProjection, MirStatementKind, MirTempId, MirTerminatorKind,
    MirType, MirValueKind,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use radix_mir_fmir::schema::{
    WireCompanionDerivativeKind, WireCompanionRelation, WireCompanionSelectedInput,
    WireCompanionSelectedOutput,
};
use radix_mir_fmir::{
    FmirDeviceArtifact, FmirDeviceArtifactsSection, FmirDeviceBackend, FmirDeviceInput,
    FmirDeviceProgramSection, FmirDeviceSection, FmirDeviceSelection, FmirDeviceSymbol,
    WireBarrierPhase, WireBarrierPoint, WireBinding, WireBufferIdentity, WireBufferLifetime,
    WireBufferRole, WireBufferVersion, WireCollectionKernelPlan, WireDependencyEdge,
    WireDeviceProgram, WireDeviceResource, WireDispatchSize, WireInitializationPolicy,
    WireKernelLaunchPlan, WireKernelUnit, WireLaunchUnit, WireMatMulPlan, WireMatMulSharedMemory,
    WireObservationFact, WireOobPaddingPolicy, WireProgramLifetime, WireReduceOp,
    WireReductionPlan, WireResourceAccess, WireResultBuffer, WireSemanticValue,
    WireSemanticValueOrigin, WireSharedMemoryLayout, WireStorageLayout, WireTransposePlan,
    WireWorkgroupCount, WireWorkgroupSize,
};

/// Stable fail-closed diagnostic for a device-program construction failure.
fn device_diag(context: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("device program {context}: {}", message.into()))
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")
}

// ---------------------------------------------------------------------------
// Device-program constructor
// ---------------------------------------------------------------------------

/// The logical entry name for a kernel function.
///
/// The target-neutral entry is the Faber function name when the function
/// carries one (the S1-3 emitters name the Metal kernel entry after it; the
/// CUDA PTX symbol is separate and carried via [`CudaKernelIdentity`]).
fn kernel_entry_name(function: &MirFunction, interner: &Interner) -> String {
    function
        .name
        .map(|symbol| interner.resolve(symbol))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("kernel{}", function.id.0))
}

/// Whether a device-resident function's body CONSTRUCTS tensor shapes
/// (`magnitudines()` shape lists and `crea` fills over a vacua seed) — the
/// exact surface the S5-U4 static-shape fold
/// ([`radix_mir::static_shape_fold::fold_static_shapes`]) rewrites to
/// constant dims + scalar-broadcast elementwise fills. A library-backed
/// `train_step` body (the gradus `train_step_2x2`/`train_step_4x4` surface)
/// is the first consumer: without the fold the shape ops carry no kernel
/// plan and the constructor fails the decomposition closed.
fn function_has_shape_construction(function: &MirFunction) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .any(|statement| {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return false;
            };
            matches!(
                call.intrinsic,
                MirIntrinsic::Collection(
                    MirCollectionOp::TensorShape | MirCollectionOp::TensorCreate
                )
            )
        })
}

/// The param-name for a buffer slot, when the kernel function names the
/// source local (diagnostics + the `[device] inputs` manifest keys).
///
/// `source` is the ORIGINAL device-resident function the slot's value comes
/// from: for a decomposition subchain the synthetic function's params carry
/// no names, so the name resolves through the source function's params and
/// locals — never from the subchain slice (S5-U1b/U5).
fn buffer_slot_name(
    source: &MirFunction,
    interner: &Interner,
    resource: &MirKernelResource,
) -> String {
    if let Some(local) = resource.source_local {
        if let Some(name) = source
            .params
            .iter()
            .find(|param| param.local == local)
            .and_then(|param| param.name)
            .and_then(|symbol| safe_interner_name(interner, symbol))
        {
            return name;
        }
        if let Some(name) = source
            .locals
            .iter()
            .find(|entry| entry.id == local)
            .and_then(|entry| entry.name)
            .and_then(|symbol| safe_interner_name(interner, symbol))
        {
            return name;
        }
    }
    match resource.role {
        radix_mir::abi::MirKernelResourceRole::Input => {
            format!("input_{}", resource.binding)
        }
        radix_mir::abi::MirKernelResourceRole::Output => {
            format!("output_{}", resource.binding)
        }
    }
}

/// Resolve a symbol name safely: an uninterned synthetic symbol (the
/// reverse-AD residual/upstream placeholders of a generated companion)
/// falls back to `None` instead of indexing past the interner's string
/// table — the caller then uses the role-based fallback name (S3-A2).
fn safe_interner_name(interner: &Interner, symbol: radix::lexer::Symbol) -> Option<String> {
    if (symbol.0 as usize) < interner.strings().len() {
        Some(interner.resolve(symbol).to_owned())
    } else {
        None
    }
}

/// One unified buffer's program-level identity facts plus the carried
/// semantic-value fact it is minted from (F1).
///
/// The S2-5 cross-kernel wiring still unifies one kernel's output with
/// another kernel's input by the declared logical identity (name + shape) —
/// that wiring is the only carried fact connecting independent kernels — but
/// the unification is now guarded by [`unify_roles`]: two independent
/// producers of the same named output never alias (F1). The buffer's
/// semantic VALUE identity is minted from the carried MIR fact of the slot
/// that first references it (a MIR local, a host input, or a distinct
/// synthetic label) — never from the buffer id, binding position, or
/// declaration coincidence.
///
/// The resource-state axes (F5) ride here as **independent facts** gathered
/// from the slot access pattern across the whole program: `written` and
/// `consumed` feed the allocation-lifetime and initialization decisions in
/// pass 2 — no axis is derived from the role or from another axis.
struct ProgramBuffer {
    id: BufferId,
    name: String,
    element_ty: MirType,
    element_count: u64,
    role: BufferRole,
    /// Any kernel slot writes the buffer (Write/ReadWrite) — the independent
    /// fact behind the lifetime and initialization axes.
    written: bool,
    /// Any kernel slot reads the buffer (Read) — the independent fact that
    /// distinguishes a step-local intermediate from a final observation.
    consumed: bool,
    /// Any kernel slot accesses the buffer in place (ReadWrite).
    readwrite: bool,
    /// A trainable parameter of a RepeatingStep training program (S5-U5):
    /// PerProgram persistent state with HostProvided init, copied in exactly
    /// once at session creation (the step's once-init contract). The flag is
    /// the independent fact behind the lifetime and initialization axes — the
    /// access pattern alone would decide these differently (a readwrite
    /// intermediate is ZeroFill, a written+consumed buffer is PerStep).
    param: bool,
    /// A backward companion's gradient output slot (S5-U5): a per-step
    /// scratch value feeding the optimizer step — never an observation point
    /// and never persistent state, even when no step kernel consumes it (a
    /// frozen-input gradient like `gx`).
    gradient: bool,
    /// The carried MIR/value fact the buffer's semantic value identity is
    /// minted from (F1).
    origin: SemanticValueOrigin,
}

impl ProgramBuffer {
    /// The S2-5 wiring key: same declared logical name and same shape. The
    /// caller additionally guards on [`unify_roles`] before unifying, so a
    /// name/shape coincidence alone never aliases two unrelated values.
    fn matches(&self, name: &str, element_ty: MirType, element_count: u64) -> bool {
        self.name == name && self.element_ty == element_ty && self.element_count == element_count
    }
}

/// Whether a slot with program role `next` may join an existing buffer with
/// role `existing` under the S2-5 wiring (same logical name + shape).
///
/// The wiring is a data-flow continuation — a shared input, a
/// producer→consumer intermediate, or an in-place accumulation slot. Two
/// independent producers of the same named output NEVER unify (F1: two
/// unrelated same-name/same-shape values are distinct, never aliased).
fn unify_roles(existing: BufferRole, next: BufferRole) -> bool {
    match (existing, next) {
        (BufferRole::Input, BufferRole::Input) => true,
        (BufferRole::InOut, _) | (_, BufferRole::InOut) => true,
        (BufferRole::Output, BufferRole::Input) | (BufferRole::Input, BufferRole::Output) => true,
        (BufferRole::Output, BufferRole::Output) => false,
    }
}

/// The carried fact a buffer's semantic value identity is minted from (F1):
/// the MIR local the slot's value flows from when the slot names one (stable
/// under rename — the identity never follows the diagnostic name), a
/// host-provided input for an anonymous read slot, or a distinct synthetic
/// label for an anonymous writer slot (a tuple-output gradient).
fn value_origin(
    function: &MirFunction,
    resource: &MirKernelResource,
    buffer_id: BufferId,
) -> SemanticValueOrigin {
    if let Some(local) = resource.source_local {
        SemanticValueOrigin::MirLocal {
            function: function.id,
            local: local.0,
        }
    } else if resource.role == MirKernelResourceRole::Input {
        SemanticValueOrigin::HostInput
    } else {
        SemanticValueOrigin::Synthetic {
            label: format!("buffer-{}", buffer_id.0),
        }
    }
}

/// Merge an ABI-derived role onto a buffer's program-level role under one id.
fn merge_buffer_roles(previous: BufferRole, next: BufferRole) -> BufferRole {
    match (previous, next) {
        (BufferRole::Input, BufferRole::Input) => BufferRole::Input,
        (BufferRole::Output, BufferRole::Output) => BufferRole::Output,
        _ => BufferRole::InOut,
    }
}

/// Order the launch sequence to follow the carried producer/consumer
/// dependency graph (F3) — never kernel declaration order. Producers always
/// precede their consumers; the launch ids are carried facts and do not
/// change, only their position in the execution sequence does.
///
/// A RepeatingStep program's trainable-parameter edges are EXCLUDED: the
/// parameter is persistent state — the step writes the value the NEXT step
/// reads — so the write→read edge within one step's sequence would form a
/// cycle. The parameter's current value is the once-init'd (or previous-step)
/// state, not a producer inside this step.
///
/// # Errors
/// Fail-closed when the dependency graph contains a cycle (a launch sequence
/// that follows it would never terminate).
fn dependency_ordered_launches(
    program: &DeviceProgram,
    excluded_buffers: &[BufferId],
) -> Result<Vec<LaunchUnit>, Vec<Diagnostic>> {
    let edges = program
        .buffer_registry()
        .data_flow_pairs()
        .into_iter()
        .filter(|edge| !excluded_buffers.contains(&edge.buffer))
        .collect::<Vec<_>>();
    let mut indegree: HashMap<LaunchId, usize> = HashMap::new();
    let mut dependents: HashMap<LaunchId, Vec<LaunchId>> = HashMap::new();
    for launch in &program.launches {
        indegree.insert(launch.id, 0);
    }
    for edge in &edges {
        if edge.producer == edge.consumer {
            continue;
        }
        *indegree.entry(edge.consumer).or_insert(0) += 1;
        dependents
            .entry(edge.producer)
            .or_default()
            .push(edge.consumer);
    }
    // Kahn's algorithm seeded in declaration order: independent launches keep
    // their declaration order, and any launch that depends on another is
    // emitted only after every producer has run.
    let mut ready: VecDeque<LaunchId> = program
        .launches
        .iter()
        .filter(|launch| indegree.get(&launch.id).copied().unwrap_or(0) == 0)
        .map(|launch| launch.id)
        .collect();
    let mut ordered: Vec<LaunchId> = Vec::with_capacity(program.launches.len());
    while let Some(id) = ready.pop_front() {
        ordered.push(id);
        if let Some(consumers) = dependents.get(&id) {
            for &consumer in consumers {
                if let Some(count) = indegree.get_mut(&consumer) {
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(consumer);
                    }
                }
            }
        }
    }
    if ordered.len() != program.launches.len() {
        return Err(vec![device_diag(
            "launch order",
            "the carried producer/consumer dependency graph contains a cycle; the launch sequence cannot follow it",
        )]);
    }
    // Reorder the launch units by their position in the carried topological
    // order (launch ids are carried facts; only their sequence position
    // changes).
    let mut position: HashMap<LaunchId, usize> = HashMap::new();
    for (index, id) in ordered.iter().enumerate() {
        position.insert(*id, index);
    }
    let mut sorted: Vec<LaunchUnit> = program.launches.to_vec();
    sorted.sort_by_key(|launch| position.get(&launch.id).copied().unwrap_or(usize::MAX));
    Ok(sorted)
}

/// Mint the carried semantic facts (F1–F6) of a materialized program, in
/// execution order.
///
/// - values + bindings: one semantic value per unified buffer, minted from
///   the carried origin facts (F1);
/// - generations: every logical write in the ordered launch sequence is an
///   explicit new generation; a read consumes the current generation (F2);
/// - roots + dependencies: the carried graph the host schedules from (F3);
///   a trainable parameter's edges are persistent-state facts — the write
///   feeds the NEXT step, never a producer inside this step — so they are
///   excluded (the same exclusion the launch ordering applies);
/// - relations: the lossless primal/companion rows from the carrier (F4);
/// - initializations + observations: the independent axis facts (F5/F6). A
///   trainable parameter is HostProvided (copied in exactly once at session
///   creation), never zero-filled despite its InOut access.
fn semantic_facts(
    program: &DeviceProgram,
    buffers: &[ProgramBuffer],
    companions: &radix_mir::device::MirCompanionMap,
) -> DeviceSemantics {
    let values: Vec<SemanticValue> = buffers
        .iter()
        .map(|buffer| SemanticValue {
            id: SemanticValueId(buffer.id.0),
            name: buffer.name.clone(),
            element_ty: buffer.element_ty,
            element_count: buffer.element_count,
            origin: buffer.origin.clone(),
        })
        .collect();
    let bindings: Vec<ValueBinding> = buffers
        .iter()
        .map(|buffer| ValueBinding {
            value: SemanticValueId(buffer.id.0),
            buffer: buffer.id,
        })
        .collect();

    // Generations in EXECUTION order: a later write is a new generation,
    // never another producer of the same one (F2). A read-only buffer's
    // reads consume its initial (host-provided) state generation 1.
    let mut produced: HashMap<BufferId, u32> =
        buffers.iter().map(|buffer| (buffer.id, 0)).collect();
    let mut generations: Vec<ValueGeneration> = Vec::new();
    for launch in &program.launches {
        let Some(kernel) = program.kernels.get(launch.kernel_index) else {
            continue;
        };
        for resource in &kernel.resources {
            let buffer = resource.buffer.id;
            match resource.access {
                MirKernelResourceAccess::Read => {}
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite => {
                    let next = produced.get(&buffer).copied().unwrap_or(0) + 1;
                    produced.insert(buffer, next);
                    generations.push(ValueGeneration {
                        value: SemanticValueId(buffer.0),
                        generation: next,
                        element_ty: resource.version.element_ty,
                        element_count: resource.version.element_count,
                        produced_by: launch.id,
                    });
                }
            }
        }
    }

    let dependencies: Vec<DependencyEdge> = program
        .buffer_registry()
        .data_flow_pairs()
        .into_iter()
        // Persistent trainable-parameter state: the step's write feeds the
        // NEXT step's read — excluded from this step's dependency graph.
        .filter(|pair| !buffers.iter().any(|buffer| buffer.param && buffer.id == pair.buffer))
        .map(|pair| DependencyEdge {
            producer: pair.producer,
            consumer: pair.consumer,
            buffer: pair.buffer,
            version: pair.version,
        })
        .collect();
    let roots: Vec<LaunchId> = program
        .launches
        .iter()
        .filter(|launch| !dependencies.iter().any(|edge| edge.consumer == launch.id))
        .map(|launch| launch.id)
        .collect();
    let relations: Vec<LosslessMirCompanionEntry> = companions.iter().cloned().collect();
    let initializations: Vec<InitializationFact> = buffers
        .iter()
        .map(|buffer| {
            let policy = if buffer.param {
                // S5-U5 / param-identity.md: a trainable parameter's storage
                // is HostProvided — copied in exactly once at session
                // creation (the RepeatingStep once-init contract). Its InOut
                // access alone would decide ZeroFill; the param identity is
                // the independent fact.
                InitializationPolicy::HostProvided
            } else if buffer.readwrite {
                InitializationPolicy::ZeroFill
            } else if !buffer.written {
                InitializationPolicy::HostProvided
            } else {
                InitializationPolicy::KernelInitialized
            };
            InitializationFact {
                buffer: buffer.id,
                policy,
            }
        })
        .collect();
    let observations: Vec<ObservationFact> = program
        .results
        .iter()
        .map(|result| ObservationFact {
            buffer: result.buffer.id,
            version: result.version.version,
            at_launch: result.produced_by,
        })
        .collect();

    DeviceSemantics {
        values,
        bindings,
        generations,
        roots,
        dependencies,
        relations,
        initializations,
        observations,
    }
}

// ---------------------------------------------------------------------------
// Training-plan analysis (S5-U5): RepeatingStep + trainable-param identity
// ---------------------------------------------------------------------------

/// The default declared step count of a RepeatingStep device program (S5-U5):
/// the device section carries `RepeatingStep` + a declared step count; the
/// U5 default is 100 steps, and the constructor's step-count admission
/// validates it against the source loop bound when the entry loop carries a
/// constant bound. The manifest `[device] steps` channel (S5-U5b) overrides
/// the default, and the effective count rides the wire's
/// `RepeatingStep(count)` lifetime variant so an image-loaded route recovers
/// it.
pub(crate) const DEFAULT_TRAINING_STEPS: u32 = 100;

/// One trainable parameter of a RepeatingStep training program (S5-U5 +
/// `stage-5-param-identity.md`): the entry-loop local that is BOTH
///
/// - (a) a selected input of the device-resident backward companion
///   (backward selection — the gradient-to-primal identity), AND
/// - (b) the read source of the train_step update, whose updated tuple
///   element the loop re-binds into the same local (step write).
///
/// The materializer projects it to a PerProgram `InOut` buffer with
/// `HostProvided` init, and the train_step kernel's output slot carrying the
/// update aliases the SAME buffer (in-place update at the device level —
/// the next step's read source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrainableParam {
    /// The training-step MIR function that updates this parameter.
    pub(crate) step_function: MirFunctionId,
    /// Parameter position in the train_step signature (the arg position in
    /// the entry loop's step call).
    pub(crate) param_position: u32,
    /// The train_step result-tuple output slot carrying the updated value
    /// (0-based; the slot aliases the parameter's buffer).
    pub(crate) update_output: u32,
    /// The entry-loop local the parameter lives in (semantic identity).
    pub(crate) entry_local: MirLocalId,
}

/// The faber-owned training-plan facts of a package (S5-U5): the repeating
/// step count, the trainable-param identity rows, and the gradient-flow
/// links. Derived from the entry loop's value graph — never from name/shape
/// coincidence (param-identity.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrainingPlanFacts {
    /// The effective declared step count of the RepeatingStep program.
    pub(crate) step_count: u32,
    /// The source loop bound when the entry loop's iteration count is a
    /// constant (the authority the declared count must agree with).
    pub(crate) source_loop_bound: Option<u32>,
    /// The trainable-param identity rows.
    pub(crate) trainable: Vec<TrainableParam>,
    /// The gradient-flow links: which companion result-tuple slot carries
    /// each trainable param's gradient and which train_step input position
    /// consumes it (the backward → train_step data-flow).
    pub(crate) gradients: Vec<GradientLink>,
}

/// One gradient-flow link (S5-U5): the companion result-tuple slot carrying a
/// trainable parameter's gradient (the `MirCompanionSelectedInput`'s
/// `gradient_slot`), and the train_step input position that consumes it. This
/// is what makes the backward → train_step data-flow explicit — the gradient
/// buffer written by the companion is the gradient buffer read by the step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GradientLink {
    /// The training-step MIR function consuming the gradient.
    pub(crate) step_function: MirFunctionId,
    /// The train_step input position (param position) holding the gradient.
    pub(crate) gradient_position: u32,
    /// The companion function producing the gradient.
    pub(crate) companion: DefId,
    /// The companion result-tuple slot carrying the gradient.
    pub(crate) gradient_slot: u32,
}

/// Fail-closed step-count admission (S5-U5 done-when): a declared repeating
/// step count must agree with the source loop bound when the entry loop
/// carries a constant bound — a mismatch fails construction closed (the
/// image's step count and the source loop's iteration count are the same
/// contract). When the source bound is absent the declared count applies.
///
/// # Errors
/// Fail-closed when both a declared count and a constant source bound exist
/// and disagree.
pub(crate) fn admit_step_count(
    declared: u32,
    source: Option<u32>,
) -> Result<u32, Vec<Diagnostic>> {
    if let Some(source) = source {
        if declared != source {
            return Err(vec![device_diag(
                "step count",
                format!(
                    "declared repeating step count {declared} contradicts the source training loop bound {source}; the device image's step count and the entry loop's iteration count must agree"
                ),
            )]);
        }
    }
    Ok(declared)
}

/// The package entry function (the lowered `incipit`), when the program has
/// one.
fn entry_function<'a>(
    validated: &'a ValidatedMir<'_>,
    interner: &'a Interner,
) -> Option<&'a MirFunction> {
    MirNames::new(validated.program(), validated.validation().types, interner).entry_function()
}

/// The local a plain place operand references, if any.
fn operand_local(operand: &MirOperand) -> Option<MirLocalId> {
    let MirOperand::Place(place) = operand else {
        return None;
    };
    let MirPlaceBase::Local(local) = place.base else {
        return None;
    };
    Some(local)
}

/// The constant loop bound of the entry function's training loop, when the
/// loop's iteration count is a literal: the largest positive integer constant
/// assigned to a local that a branch condition compares (the
/// `itera ab 0‥steps` bound when `steps` is a constant). Tracks one
/// local→local copy hop (the lowered `steps` copy). `None` when the loop
/// bound is not a compile-time constant.
fn source_loop_bound(entry: &MirFunction) -> Option<u32> {
    /// A local's initialization: a literal integer or a copy of another
    /// local (the lowered `steps` copy). First assignment wins (the prelude
    /// dominates the loop body's re-assignments).
    #[derive(Clone, Copy)]
    enum LocalDef {
        Const(i64),
        Copy(MirLocalId),
    }
    let mut defs: BTreeMap<MirLocalId, LocalDef> = BTreeMap::new();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() || defs.contains_key(&local) {
                continue;
            }
            match &value.kind {
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(n))) => {
                    defs.insert(local, LocalDef::Const(*n));
                }
                MirValueKind::Operand(MirOperand::Place(MirPlace {
                    base: MirPlaceBase::Local(other),
                    projections,
                })) if projections.is_empty() => {
                    defs.insert(local, LocalDef::Copy(*other));
                }
                _ => {}
            }
        }
    }
    fn resolve(local: MirLocalId, defs: &BTreeMap<MirLocalId, LocalDef>) -> Option<i64> {
        let mut seen: Vec<MirLocalId> = Vec::new();
        let mut current = local;
        loop {
            if seen.contains(&current) {
                return None;
            }
            seen.push(current);
            match defs.get(&current)? {
                LocalDef::Const(n) => return Some(*n),
                LocalDef::Copy(next) => current = *next,
            }
        }
    }

    // The branch conditions that compare two locals (the loop guard).
    let mut compared: Vec<MirLocalId> = Vec::new();
    for block in &entry.blocks {
        let MirTerminatorKind::Branch { condition, .. } = &block.terminator.kind else {
            continue;
        };
        let temp = match condition {
            MirOperand::Temp(temp) => *temp,
            MirOperand::Place(MirPlace {
                base: MirPlaceBase::Temp(temp),
                projections,
            }) if projections.is_empty() => *temp,
            _ => continue,
        };
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            if place.base != MirPlaceBase::Temp(temp) {
                continue;
            }
            let MirValueKind::Binary { lhs, rhs, .. } = &value.kind else {
                continue;
            };
            for operand in [lhs, rhs] {
                if let Some(local) = operand_local(operand) {
                    compared.push(local);
                }
            }
        }
    }
    compared
        .iter()
        .filter_map(|local| resolve(*local, &defs).filter(|n| *n > 0))
        .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
        .max()
}

/// The entry-loop's tuple trace: how tuple-return call results are unpacked
/// into locals (the lowered `iuncta` pattern-match).
#[derive(Debug, Clone, Default)]
struct TupleTrace {
    /// Local → the call-destination temp it was plain-copied from
    /// (`tuple ← Temp(t)`).
    plain_copy: BTreeMap<MirLocalId, MirTempId>,
    /// Local → the (call-destination temp, element index) it was extracted
    /// from (`element ← tuple[i]`, including through a plain copy hop).
    extract: BTreeMap<MirLocalId, (MirTempId, u32)>,
}

/// Build the tuple trace of one function's statements.
fn build_tuple_trace(entry: &MirFunction) -> TupleTrace {
    let mut trace = TupleTrace::default();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let (base_temp, index) = match &value.kind {
                MirValueKind::Operand(MirOperand::Temp(temp)) => (*temp, None),
                MirValueKind::Operand(MirOperand::Place(inner)) => {
                    let temp = match inner.base {
                        MirPlaceBase::Temp(temp) => temp,
                        // One hop through a plain copy: `Local(l)[i]` where
                        // `l` holds the call's tuple.
                        MirPlaceBase::Local(other) => {
                            let Some(temp) = trace.plain_copy.get(&other).copied() else {
                                continue;
                            };
                            temp
                        }
                    };
                    (temp, place_tuple_index(inner))
                }
                _ => continue,
            };
            match index {
                None => {
                    trace.plain_copy.insert(local, base_temp);
                }
                Some(index) => {
                    trace.extract.insert(local, (base_temp, index));
                }
            }
        }
    }
    trace
}

/// The tuple element index of a place's projections, when they are exactly
/// one constant index.
fn place_tuple_index(place: &MirPlace) -> Option<u32> {
    if place.projections.len() != 1 {
        return None;
    }
    let MirProjection::Index(MirOperand::Constant(MirConstant::Int(n))) = &place.projections[0]
    else {
        return None;
    };
    u32::try_from(*n).ok()
}

/// The tuple element an assign value extracts from a call-destination temp:
/// `Temp(t)[i]`, `Local(l)[i]` (where `l` was plain-copied from the call
/// temp), or a re-binding of an already-extracted local (`Place(Local(l))`
/// where `l ← tuple[i]`).
fn tuple_index_of(kind: &MirValueKind, trace: &TupleTrace) -> Option<(MirTempId, u32)> {
    let MirValueKind::Operand(MirOperand::Place(place)) = kind else {
        return None;
    };
    match place.base {
        MirPlaceBase::Temp(temp) => Some((temp, place_tuple_index(place)?)),
        MirPlaceBase::Local(local) => {
            if place.projections.is_empty() {
                // Re-binding a previously extracted element.
                return trace.extract.get(&local).copied();
            }
            // `Local(l)[i]`: `l` holds the call's tuple (plain copy).
            let temp = *trace.plain_copy.get(&local)?;
            Some((temp, place_tuple_index(place)?))
        }
    }
}

/// The entry-loop calls to device-resident backward companions: the
/// entry-loop local at each companion-selected primal parameter position,
/// and — through the tuple trace — the entry-loop local each companion
/// result-tuple slot's value is extracted into.
struct CompanionCalls {
    /// Selected-input locals: the entry-loop local passed at a selected
    /// primal parameter position.
    selected_locals: BTreeSet<MirLocalId>,
    /// (companion, result-tuple slot) → the entry-loop local holding that
    /// slot's value (the gradient extraction).
    tuple_locals: BTreeMap<(u32, u32), MirLocalId>,
    /// (entry-loop local) → (companion, gradient slot) for the selected
    /// inputs, keyed by the local passed at the primal parameter position.
    selected_gradient_slots: BTreeMap<MirLocalId, (u32, u32)>,
}

fn companion_calls(
    entry: &MirFunction,
    companions: &radix_mir::device::MirCompanionMap,
) -> CompanionCalls {
    let trace = build_tuple_trace(entry);
    eprintln!("CCDBG plain={:?} extract={:?}", trace.plain_copy, trace.extract);
    let mut selected_locals: BTreeSet<MirLocalId> = BTreeSet::new();
    let mut selected_gradient_slots: BTreeMap<MirLocalId, (u32, u32)> = BTreeMap::new();
    let mut tuple_locals: BTreeMap<(u32, u32), MirLocalId> = BTreeMap::new();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Call {
                destination,
                callee,
                args,
            } = &statement.kind
            else {
                continue;
            };
            let MirCallee::Definition { source, .. } = callee else {
                continue;
            };
            let Some(companion) = companions
                .iter()
                .find(|candidate| candidate.device_resident && candidate.companion == *source)
            else {
                continue;
            };
            for selected_input in &companion.selected_inputs {
                if let Some(arg) = args.get(selected_input.position as usize) {
                    if let Some(local) = operand_local(arg) {
                        selected_locals.insert(local);
                        selected_gradient_slots
                            .insert(local, (source.0, selected_input.gradient_slot));
                    }
                }
            }
            let Some(destination) = destination else {
                continue;
            };
            let MirPlaceBase::Temp(temp) = destination.base else {
                continue;
            };
            if !destination.projections.is_empty() {
                continue;
            }
            // The gradient extractions from this companion call's result.
            for (local, (call_temp, index)) in &trace.extract {
                if *call_temp == temp {
                    tuple_locals.insert((source.0, *index), *local);
                }
            }
        }
    }
    CompanionCalls {
        selected_locals,
        tuple_locals,
        selected_gradient_slots,
    }
}

/// Whether a program function is a tuple-return function (the `iuncta` /
/// result-tuple shape a training step carries its updated parameters in).
fn is_tuple_return(function: &MirFunction, validated: &ValidatedMir<'_>) -> bool {
    matches!(
        validated.validation().types.get(function.return_ty.semantic_id()),
        Type::Tuple(elements) if !elements.is_empty()
    )
}

/// One entry-loop call to a package-local training-step function: the callee
/// function id, the destination temp holding the result tuple, and the
/// entry-loop locals at each argument position.
struct StepCall {
    function: MirFunctionId,
    destination: MirTempId,
    args: Vec<MirLocalId>,
}

/// Derive the training-plan facts from the entry loop (S5-U5).
///
/// The identity rule (`stage-5-param-identity.md`): a trainable parameter is
/// an entry-loop local that (a) is a selected input of a device-resident
/// backward companion AND (b) is the read source of a train_step call whose
/// updated result-tuple element the loop re-binds into the same local. The
/// intersection of the two facts decides the trainable-param rows — never a
/// name/shape coincidence. `None` when the package has no training loop (no
/// device-resident companion called from the entry, or no step functions).
///
/// # Errors
/// Fail-closed when a training loop's step function is not a package-local
/// MIR function (e.g. a provider/library-backed `train_step`), when the
/// train_step result tuple cannot be traced, or when a trainable param's
/// write path does not read the param it claims to update.
pub(crate) fn training_plan_facts(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    companions: &radix_mir::device::MirCompanionMap,
    declared_steps: Option<u32>,
) -> Result<Option<TrainingPlanFacts>, Vec<Diagnostic>> {
    let program = validated.program();
    let Some(entry) = entry_function(validated, interner) else {
        return Ok(None);
    };
    // (a) backward selection: the entry-loop locals passed to a device-
    // resident companion at its selected-input positions.
    let companion_calls = companion_calls(entry, companions);
    if companion_calls.selected_locals.is_empty() {
        return Ok(None);
    }

    // Step calls: direct calls to package-local functions (Definition callee
    // whose source is a program function) that are neither compute kernels
    // nor companions and carry a tuple return — the optimizer step shape.
    let mut step_calls: Vec<StepCall> = Vec::new();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Call {
                destination,
                callee,
                args,
            } = &statement.kind
            else {
                continue;
            };
            let MirCallee::Definition { source, .. } = callee else {
                continue;
            };
            let Some(step_function) = program
                .functions
                .iter()
                .find(|function| function.source == Some(*source))
            else {
                continue;
            };
            if step_function.shader_stage == Some(MirKernelShaderStage::Compute) {
                continue;
            }
            if companions
                .iter()
                .any(|companion| companion.companion == *source)
            {
                continue;
            }
            if !is_tuple_return(step_function, validated) {
                continue;
            }
            let Some(destination) = destination else {
                continue;
            };
            let MirPlaceBase::Temp(temp) = destination.base else {
                continue;
            };
            if !destination.projections.is_empty() {
                continue;
            }
            step_calls.push(StepCall {
                function: step_function.id,
                destination: temp,
                args: args.iter().filter_map(operand_local).collect(),
            });
        }
    }
    let step_call_dests: BTreeMap<MirTempId, MirFunctionId> = step_calls
        .iter()
        .map(|call| (call.destination, call.function))
        .collect();

    // The loop's re-bindings: which result-tuple index re-binds which
    // entry-loop local (the def-use chain through the extraction temp).
    // key: (step function, tuple index) → entry local.
    let trace = build_tuple_trace(entry);
    let mut tuple_to_entry: BTreeMap<(MirFunctionId, u32), MirLocalId> = BTreeMap::new();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let Some(index) = tuple_index_of(&value.kind, &trace) else {
                continue;
            };
            if let Some(step_function) = step_call_dests.get(&index.0) {
                tuple_to_entry.insert((*step_function, index.1), local);
            }
        }
    }
    // The identity rule (b): a trainable parameter is the PRE-UPDATE
    // entry-loop local — a local the loop re-binds FROM the step result AND
    // passes INTO a step call. Extraction intermediates (the tuple element
    // copied into a fresh local before the re-bind) are not parameters.
    let step_arg_locals: BTreeSet<MirLocalId> = step_calls
        .iter()
        .flat_map(|call| call.args.iter().copied())
        .collect();
    tuple_to_entry.retain(|_, local| step_arg_locals.contains(local));

    // Trainable params: step-call args whose entry-local is BOTH companion-
    // selected (a) AND re-bound from a step result tuple element (b).
    let mut trainable: Vec<TrainableParam> = Vec::new();
    for call in &step_calls {
        for (position, entry_local) in call.args.iter().enumerate() {
            if !companion_calls.selected_locals.contains(entry_local) {
                continue;
            }
            let Some(update_output) = tuple_to_entry
                .iter()
                .find(|((function, _), local)| {
                    *function == call.function && **local == *entry_local
                })
                .map(|((_, index), _)| *index)
            else {
                continue;
            };
            trainable.push(TrainableParam {
                step_function: call.function,
                param_position: u32::try_from(position).unwrap_or(u32::MAX),
                update_output,
                entry_local: *entry_local,
            });
        }
    }
    if trainable.is_empty() {
        return Ok(None);
    }
    trainable.sort_by_key(|param| (param.step_function.0, param.param_position));
    trainable.dedup();

    // Gradient-flow links: for each trainable param, the companion
    // result-tuple slot carrying its gradient (the selected input's
    // `gradient_slot`), and the train_step input position consuming the
    // extracted gradient local. The backward → train_step edge is explicit
    // only through this graph connection — never a name/shape coincidence.
    let mut gradients: Vec<GradientLink> = Vec::new();
    for param in &trainable {
        let Some((companion, gradient_slot)) =
            companion_calls.selected_gradient_slots.get(&param.entry_local).copied()
        else {
            continue;
        };
        let Some(gradient_local) = companion_calls
            .tuple_locals
            .get(&(companion, gradient_slot))
            .copied()
        else {
            continue;
        };
        for call in step_calls
            .iter()
            .filter(|call| call.function == param.step_function)
        {
            if let Some((position, _)) = call
                .args
                .iter()
                .enumerate()
                .find(|(_, local)| **local == gradient_local)
            {
                gradients.push(GradientLink {
                    step_function: param.step_function,
                    gradient_position: u32::try_from(position).unwrap_or(u32::MAX),
                    companion: DefId(companion),
                    gradient_slot,
                });
                break;
            }
        }
    }
    gradients.sort_by_key(|link| (link.step_function.0, link.gradient_position));
    gradients.dedup();

    // Fail-closed step-write verification (param-identity.md): every trainable
    // param's update output must be a local whose defining statement reads
    // the param local it claims to update (param' = param − lr·grad). A
    // train_step that writes a value without reading the param cannot
    // establish in-place identity.
    for param in &trainable {
        let step_function = program
            .functions
            .iter()
            .find(|function| function.id == param.step_function)
            .ok_or_else(|| {
                vec![device_diag(
                    "training step",
                    format!(
                        "train_step function DefId({}) is missing from the lowered MIR",
                        param.step_function.0
                    ),
                )]
            })?;
        let param_local = step_function
            .params
            .get(param.param_position as usize)
            .map(|param| param.local)
            .ok_or_else(|| {
                vec![device_diag(
                    "training step",
                    format!(
                        "train_step DefId({}) has no parameter at position {}",
                        param.step_function.0, param.param_position
                    ),
                )]
            })?;
        let update_local = tuple_return_locals(step_function)
            .and_then(|locals| locals.get(param.update_output as usize).copied())
            .ok_or_else(|| {
                vec![device_diag(
                    "training step",
                    format!(
                        "train_step DefId({}) result tuple has no element at slot {}",
                        param.step_function.0, param.update_output
                    ),
                )]
            })?;
        if !statement_defining_reads(step_function, update_local, param_local) {
            return Err(vec![device_diag(
                "param identity",
                format!(
                    "train_step DefId({}) writes update local l{} at tuple slot {} without reading the parameter local l{} it claims to update; in-place param identity requires the write path to read the pre-update parameter (param' = param − lr·grad)",
                    param.step_function.0,
                    update_local.0,
                    param.update_output,
                    param_local.0
                ),
            )]);
        }
    }

    let source_loop_bound = source_loop_bound(entry);
    let step_count = admit_step_count(declared_steps.unwrap_or(DEFAULT_TRAINING_STEPS), source_loop_bound)?;
    Ok(Some(TrainingPlanFacts {
        step_count,
        source_loop_bound,
        trainable,
        gradients,
    }))
}

/// The return-tuple element locals of a tuple-return function, in tuple
/// order (from the aggregate construction that produces the returned tuple).
fn tuple_return_locals(function: &MirFunction) -> Option<Vec<MirLocalId>> {
    let block = function.blocks.first()?;
    let MirTerminatorKind::Return(Some(operand)) = &block.terminator.kind else {
        return None;
    };
    // The returned value: a plain temp or a plain local place.
    let returned = match operand {
        MirOperand::Temp(temp) => MirPlace::temp(*temp),
        MirOperand::Place(place) if place.projections.is_empty() => place.clone(),
        _ => return None,
    };
    for statement in &block.statements {
        let MirStatementKind::Construct {
            destination,
            aggregate,
        } = &statement.kind
        else {
            continue;
        };
        if destination != &returned {
            continue;
        }
        let locals: Vec<MirLocalId> = aggregate
            .fields
            .operands()
            .iter()
            .filter_map(|operand| operand_local(operand))
            .collect();
        if locals.is_empty() {
            return None;
        }
        return Some(locals);
    }
    None
}

/// Whether the statement that defines `destination_local` reads `source`
/// (the step-write verification: the update local's defining statement reads
/// the pre-update parameter). Traces one temp hop: the defining copy
/// (`nw ← t`) is traced to the producing runtime call (`t ← w − gw`), which
/// reads the parameter.
fn statement_defining_reads(
    function: &MirFunction,
    destination_local: MirLocalId,
    source: MirLocalId,
) -> bool {
    for block in &function.blocks {
        for statement in &block.statements {
            let writes = match &statement.kind {
                MirStatementKind::Assign { place, .. }
                | MirStatementKind::Construct { destination: place, .. } => {
                    matches!(
                        place.base,
                        MirPlaceBase::Local(local) if local == destination_local
                    ) && place.projections.is_empty()
                }
                MirStatementKind::RuntimeCall { destination, .. }
                | MirStatementKind::Call { destination, .. } => destination
                    .as_ref()
                    .is_some_and(|place| {
                        matches!(
                            place.base,
                            MirPlaceBase::Local(local) if local == destination_local
                        ) && place.projections.is_empty()
                    }),
            };
            if !writes {
                continue;
            }
            if statement_reads_local(statement, source) {
                return true;
            }
            // One temp hop: the defining value is a temp whose producing
            // statement reads `source`.
            if let Some(temp) = statement_value_temp(statement) {
                if function
                    .blocks
                    .iter()
                    .flat_map(|block| block.statements.iter())
                    .any(|producer| {
                        statement_dest_temp(producer) == Some(temp)
                            && statement_reads_local(producer, source)
                    })
                {
                    return true;
                }
            }
            return false;
        }
    }
    false
}

/// The temp a statement's assigned value is a plain copy of, if any.
fn statement_value_temp(statement: &radix_mir::MirStatement) -> Option<MirTempId> {
    let MirStatementKind::Assign { value, .. } = &statement.kind else {
        return None;
    };
    match &value.kind {
        MirValueKind::Operand(MirOperand::Temp(temp)) => Some(*temp),
        MirValueKind::Operand(MirOperand::Place(place))
            if place.projections.is_empty() && matches!(place.base, MirPlaceBase::Temp(_)) =>
        {
            let MirPlaceBase::Temp(temp) = place.base else {
                return None;
            };
            Some(temp)
        }
        _ => None,
    }
}

/// The temp a statement's destination names, if any.
fn statement_dest_temp(statement: &radix_mir::MirStatement) -> Option<MirTempId> {
    let destination = match &statement.kind {
        MirStatementKind::Assign { place, .. }
        | MirStatementKind::Construct { destination: place, .. } => Some(place),
        MirStatementKind::RuntimeCall { destination, .. }
        | MirStatementKind::Call { destination, .. } => destination.as_ref(),
    };
    destination.and_then(|place| match place.base {
        MirPlaceBase::Temp(temp) if place.projections.is_empty() => Some(temp),
        _ => None,
    })
}

/// Whether a statement reads the given local in any operand/value position.
fn statement_reads_local(statement: &radix_mir::MirStatement, local: MirLocalId) -> bool {
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => match &value.kind {
            MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
                operand_local(operand) == Some(local)
            }
            MirValueKind::Binary { lhs, rhs, .. } => {
                operand_local(lhs) == Some(local) || operand_local(rhs) == Some(local)
            }
            _ => false,
        },
        MirStatementKind::RuntimeCall { call, .. } => call
            .args
            .iter()
            .any(|operand| operand_local(operand) == Some(local)),
        MirStatementKind::Call { args, .. } => {
            args.iter().any(|operand| operand_local(operand) == Some(local))
        }
        MirStatementKind::Construct { aggregate, .. } => aggregate
            .fields
            .operands()
            .iter()
            .any(|operand| operand_local(operand) == Some(local)),
    }
}

/// Construct the common device program for a lowered package.
///
/// Scans the validated package MIR for `@ nucleum` compute kernels
/// (`shader_stage == Compute`) and composes one ordered [`DeviceProgram`]
/// whose kernel units carry the typed plan, typed storage-buffer resources,
/// and derived launch plans, plus the frozen [`DeviceSemantics`] that rides
/// alongside it (Stage 3R F1–F7). Every field is a program fact from the ABI
/// signature and the shared plan pass — never inferred from emitted text
/// (A3). A package with no compute kernels yields `None` (no device payload).
///
/// **Faithful materialization (U4):**
/// - semantic VALUE identity (F1) is minted from carried MIR facts —
///   [`SemanticValueOrigin::MirLocal`] from the slot's source local (stable
///   under rename), `HostInput` for anonymous host-provided reads, or a
///   distinct `Synthetic` label for anonymous writer slots. Two unrelated
///   same-name/same-shape values never alias (the unification guard
///   [`unify_roles`] rejects a second independent producer of a named
///   output);
/// - every logical write advances an explicit value generation (F2) in the
///   ordered launch sequence;
/// - the launch SEQUENCE follows the carried producer/consumer dependency
///   graph (F3), never kernel declaration order;
/// - results name declared observation points only (F6): the program's
///   Output-role buffers. Writable intermediates and persistent state are
///   never results merely because they are writable;
/// - the lossless primal/companion relation (F4) rides from the owned
///   [`radix_mir::device::MirCompanionMap`] carrier into the semantics.
///
/// **Companion path (S3-A2, THE SPINE):** the constructor also selects the
/// generated companions of device-resident primals through the owned
/// [`radix_mir::device::MirCompanionMap`] carrier — NOT only
/// `shader_stage == Compute`. Each carried companion's tuple gradient return
/// lowers through the multi-output ABI (S3-A1) into distinct output
/// resources (N gradient outputs bind to N distinct slots), its kernel is
/// ordered AFTER the forward kernels, and its buffers join the same S2-5
/// unification (the companion reads the primal's device-resident buffers by
/// identity). Placement is decided here (A5): a companion of a
/// device-resident primal joins the same `DeviceProgram`; generated AIR
/// stays pure (the purity ledger is untouched).
///
/// # Errors
/// Fail-closed [`Diagnostic`]s when a kernel's ABI signature or plan cannot
/// be derived, a storage buffer has no coherent program role, a carried
/// companion is missing from the lowered MIR, the launch sequence cannot
/// follow the carried dependency graph, or the resulting program + carried
/// semantics fail [`DeviceProgram::validate_with_semantics`].
pub(crate) fn device_program_for_lowered(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    companions: &radix_mir::device::MirCompanionMap,
    declared_steps: u32,
) -> Result<Option<(DeviceProgram, DeviceSemantics, u32)>, Vec<Diagnostic>> {
    let program = validated.program();
    let training =
        training_plan_facts(validated, interner, companions, Some(declared_steps))?;

    let kernel_functions: Vec<&MirFunction> = program
        .functions
        .iter()
        .filter(|function| function.shader_stage == Some(MirKernelShaderStage::Compute))
        .collect();
    if kernel_functions.is_empty() {
        return Ok(None);
    }

    /// One kernel's build: signature/plan facts plus its resource slots
    /// (buffer ids resolved by the unification pass).
    struct KernelBuild {
        function: MirFunctionId,
        entry: String,
        plan: CollectionKernelPlan,
        launch: KernelLaunchPlan,
        resources: Vec<ResourceBuild>,
    }
    /// One storage-buffer slot: binding facts + the unified buffer id.
    struct ResourceBuild {
        group: u32,
        binding: u32,
        access: MirKernelResourceAccess,
        buffer_id: BufferId,
        element_ty: MirType,
        element_count: u64,
    }

    // Pass 1: derive signatures/plans and unify cross-kernel buffer identity.
    // One kernel builder serves the forward kernels, the S3-A2 companion
    // kernels, and the S5-U5 train_step kernels — every kernel participates
    // in the same S2-5 unification, so a companion reads its primal's
    // device-resident buffers by identity and a train_step updates the
    // parameter buffers in place.
    let mut buffers: Vec<ProgramBuffer> = Vec::new();
    let mut next_buffer_id = 1u32;
    let mut builds: Vec<KernelBuild> = Vec::new();
    // (step function, param local) → (param position, update output slot)
    // for the trainable params of the package's training loop.
    let mut param_updates: BTreeMap<(MirFunctionId, MirLocalId), (u32, u32)> = BTreeMap::new();
    if let Some(training) = &training {
        for param in &training.trainable {
            if let Some(function) = program
                .functions
                .iter()
                .find(|function| function.id == param.step_function)
            {
                if let Some(param_local) = function
                    .params
                    .get(param.param_position as usize)
                    .map(|param| param.local)
                {
                    param_updates.insert((param.step_function, param_local), (param.param_position, param.update_output));
                }
            }
        }
    }
    // Gradient-flow links (S5-U5): the train_step input position consuming
    // each trainable param's gradient, and the companion result-tuple slot
    // producing it. `gradient_buffers` is filled while the companion kernels
    // are built (the companion precedes the train_step) and consulted while
    // the train_step kernels are built — the backward → train_step edge.
    let mut gradient_links: BTreeMap<(MirFunctionId, u32), (u32, u32)> = BTreeMap::new();
    let mut companion_gradient_slots: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    if let Some(training) = &training {
        for link in &training.gradients {
            gradient_links.insert((link.step_function, link.gradient_position), (link.companion.0, link.gradient_slot));
        }
    }
    // EVERY selected-input gradient slot of a device-resident companion is a
    // per-step scratch buffer — including a frozen input's gradient (no step
    // consumes it, but it is still a gradient, never an observation point).
    for entry in companions.iter() {
        if !entry.device_resident {
            continue;
        }
        for selected in &entry.selected_inputs {
            companion_gradient_slots
                .entry(entry.companion.0)
                .or_default()
                .insert(selected.gradient_slot);
        }
    }
    let mut gradient_buffers: BTreeMap<(u32, u32), BufferId> = BTreeMap::new();
    let mut build_kernel = |function: &MirFunction| -> Result<(), Vec<Diagnostic>> {
        // The entry name: the function's logical entry (the source name),
        // with a `__N` suffix when a multi-recipe body decomposes into
        // several subchain kernels (distinct Metal entries / CUDA logical
        // entries per kernel).
        //
        // S5-U4 static-shape fold: a device-resident body that CONSTRUCTS
        // shapes (`magnitudines()` / `crea`) folds to constant dims +
        // scalar-broadcast fills BEFORE any plan/signature derivation —
        // the shape ops carry no kernel plan and would otherwise fail the
        // decomposition closed. The first consumer is the library-backed
        // `train_step` body (gradus `train_step_2x2`/`train_step_4x4`) the
        // S5-U5 training-plan path materializes; the fold is a pure rewrite
        // (same function id, params, and semantic types), so bodies without
        // shape construction pass through unchanged.
        let folded: Option<MirFunction>;
        let kernel_source: &MirFunction = if function_has_shape_construction(function) {
            let outcome =
                radix_mir::static_shape_fold::fold_static_shapes(function, validated.validation())
                    .map_err(|error| vec![device_diag("shape fold", error.message)])?;
            folded = Some(outcome.function);
            folded.as_ref().ok_or_else(|| {
                vec![device_diag("shape fold", "folded function missing after fold admission")]
            })?
        } else {
            function
        };
        let base_entry = kernel_entry_name(kernel_source, interner);
        let mut emit_subchain = |synthetic: &MirFunction,
                                 signature: &MirKernelSignature,
                                 plan: CollectionKernelPlan,
                                 entry: String,
                                 param_updates: &BTreeMap<(MirFunctionId, MirLocalId), (u32, u32)>|
         -> Result<(), Vec<Diagnostic>> {
            let mut resources: Vec<ResourceBuild> = Vec::new();
            // The trainable param local → its registered buffer id, filled
            // as the signature's input slots are processed (inputs precede
            // outputs in the ABI), so the update output slots can alias it.
            let mut param_buffer_by_local: BTreeMap<MirLocalId, BufferId> = BTreeMap::new();
            for resource in signature
                .resources()
                .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
            {
                let role = BufferRole::from_abi_role(resource.role, resource.access).ok_or_else(|| {
                    vec![device_diag(
                        "buffer role",
                        format!(
                            "storage buffer binding {} has no coherent program role ({:?} {:?})",
                            resource.binding, resource.role, resource.access
                        ),
                    )]
                })?;
                // A trainable parameter's read slot: the buffer is persistent
                // InOut state with HostProvided init (param-identity.md) —
                // the program role is InOut regardless of the slot's ABI role.
                let is_param_input = resource.role == MirKernelResourceRole::Input
                    && resource.source_local.is_some_and(|local| {
                        param_updates.contains_key(&(kernel_source.id, local))
                    });
                // The output-slot tuple index (the ABI's first output is
                // element 0, each extra output follows).
                let output_index = if resource.role == MirKernelResourceRole::Output {
                    Some(output_slot_index(signature, resource))
                } else {
                    None
                };
                // An update output slot: the slot's tuple index decides which
                // parameter it updates (the loop re-binds that parameter's
                // entry-local from this tuple element).
                let alias_param = match output_index {
                    Some(output_index) => param_updates
                        .iter()
                        .find(|((_, _), (_, update_output))| *update_output == output_index)
                        .and_then(|((_, param_local), _)| {
                            param_buffer_by_local.get(param_local).copied()
                        }),
                    None => None,
                };
                // A gradient input slot of a train_step: the gradient buffer
                // the companion's result-tuple slot writes — the explicit
                // backward → train_step data-flow connection (the gradient
                // identity rides the companion relation, never a name).
                let gradient_alias = if resource.role == MirKernelResourceRole::Input {
                    resource.source_local.and_then(|local| {
                        let position = kernel_source
                            .params
                            .iter()
                            .position(|param| param.local == local);
                        position.and_then(|position| {
                            gradient_links
                                .get(&(
                                    kernel_source.id,
                                    u32::try_from(position).unwrap_or(u32::MAX),
                                ))
                                .and_then(|(companion, slot)| {
                                    gradient_buffers.get(&(*companion, *slot)).copied()
                                })
                        })
                    })
                } else {
                    None
                };
                let name = buffer_slot_name(kernel_source, interner, resource);
                // S2-5 wiring: the same logical buffer appears at this kernel
                // too. The role guard ([`unify_roles`]) keeps a name/shape
                // coincidence from aliasing two unrelated values — a second
                // independent producer of a named output mints a distinct buffer
                // and a distinct semantic value (F1).
                let buffer_id = if let Some(alias) = alias_param {
                    alias
                } else if let Some(alias) = gradient_alias {
                    // The gradient input consumes the companion's gradient
                    // output buffer — the buffer becomes an InOut per-step
                    // intermediate (written by the companion, read by the
                    // step), never an observation point.
                    if let Some(entry) = buffers.iter_mut().find(|entry| entry.id == alias) {
                        entry.role = merge_buffer_roles(entry.role, BufferRole::InOut);
                        entry.consumed = true;
                    }
                    alias
                } else if let Some(entry) = buffers.iter_mut().find(|entry| {
                    entry.matches(&name, resource.element_ty, resource.element_count)
                        && unify_roles(entry.role, role)
                }) {
                    // A trainable parameter's program role is InOut regardless
                    // of the merged slot roles (the param identity is a
                    // carried fact, never derived from one slot's ABI role).
                    entry.role = if is_param_input {
                        BufferRole::InOut
                    } else {
                        merge_buffer_roles(entry.role, role)
                    };
                    entry.written |= matches!(
                        resource.access,
                        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                    );
                    entry.consumed |= resource.access == MirKernelResourceAccess::Read;
                    entry.readwrite |= resource.access == MirKernelResourceAccess::ReadWrite;
                    // The param identity is a carried fact: whichever kernel
                    // first registers the buffer, a trainable-parameter slot
                    // makes it persistent HostProvided state.
                    entry.param |= is_param_input;
                    entry.id
                } else {
                    let id = BufferId(next_buffer_id);
                    next_buffer_id += 1;
                    buffers.push(ProgramBuffer {
                        id,
                        name,
                        element_ty: resource.element_ty,
                        element_count: resource.element_count,
                        role: if is_param_input {
                            BufferRole::InOut
                        } else {
                            role
                        },
                        written: matches!(
                            resource.access,
                            MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                        ),
                        consumed: resource.access == MirKernelResourceAccess::Read,
                        readwrite: resource.access == MirKernelResourceAccess::ReadWrite,
                        param: is_param_input,
                        gradient: false,
                        origin: value_origin(kernel_source, resource, id),
                    });
                    id
                };
                if is_param_input {
                    if let Some(local) = resource.source_local {
                        param_buffer_by_local.insert(local, buffer_id);
                    }
                }
                // Record the companion's gradient output buffers as they are
                // minted (the train_step kernels built afterwards alias them);
                // a companion gradient slot is a per-step scratch value,
                // never an observation point.
                if let (Some(source), Some(output_index)) =
                    (kernel_source.source, output_index)
                {
                    if companion_gradient_slots
                        .get(&source.0)
                        .is_some_and(|slots| slots.contains(&output_index))
                    {
                        if let Some(entry) = buffers.iter_mut().find(|entry| entry.id == buffer_id)
                        {
                            entry.gradient = true;
                        }
                        gradient_buffers.insert((source.0, output_index), buffer_id);
                    }
                }
                resources.push(ResourceBuild {
                    group: resource.group,
                    binding: resource.binding,
                    access: resource.access,
                    buffer_id,
                    element_ty: resource.element_ty,
                    element_count: resource.element_count,
                });
            }
            // The launch binds buffers in binding order (inputs first, output
            // last), matching the emitted kernel's buffer indices / param
            // order.
            resources.sort_by_key(|resource| (resource.binding, resource.group));
            builds.push(KernelBuild {
                function: kernel_source.id,
                entry,
                plan,
                launch: KernelLaunchPlan::from_signature_and_function(signature, synthetic),
                resources,
            });
            Ok(())
        };

        // Whole-function path: a single-recipe or elementwise-only body
        // carries ONE kernel from its full ABI signature — the S2-4/S3-A1
        // behavior unchanged. A multi-recipe body (the S5-U1 decomposition
        // contract) falls through to the subchain path.
        let whole_signature =
            MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                kernel_source,
                validated.validation(),
                interner,
            );
        if let Ok(whole_signature) = &whole_signature {
            if let Ok(Some(plan)) = kernel_plan_for_function(
                kernel_source,
                whole_signature,
                validated.validation(),
            ) {
                return emit_subchain(
                    kernel_source,
                    whole_signature,
                    plan,
                    base_entry,
                    &param_updates,
                );
            }
        }

        // Decomposition path (S5-U1/U5): split the body at recipe boundaries
        // into subchain kernels, each carrying exactly one recipe (or an
        // elementwise-only body), with the shared contract-shaped emission
        // signature for recipe subchains and the full ABI synthesis for
        // elementwise-only subchains.
        let decomposition = decompose_kernel_function(kernel_source, validated.validation())
            .map_err(|error| vec![device_diag("decomposition", error.message)])?;
        for (subchain_index, subchain) in decomposition.subchains.iter().enumerate() {
            let synthetic = decomposition.subchain_function(kernel_source, subchain_index);
            let contract = collection_op_contract(&synthetic, validated.validation())
                .map_err(|error| vec![device_diag("plan", error.message)])?;
            let signature = match &contract {
                Some(contract) => subchain_signature_for_emission(
                    &synthetic,
                    contract,
                    validated.validation(),
                )
                .map_err(|error| vec![device_diag("signature", error.message)])?,
                None => MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                    &synthetic,
                    validated.validation(),
                    interner,
                )
                .map_err(|error| vec![device_diag("signature", error.message)])?,
            };
            let entry = if decomposition.subchains.len() == 1 {
                base_entry.clone()
            } else {
                format!("{base_entry}__{subchain_index}")
            };
            emit_subchain(&synthetic, &signature, subchain.plan.clone(), entry, &param_updates)?;
        }
        Ok(())
    };

    for function in kernel_functions {
        build_kernel(function)?;
    }

    // S3-A2 companion path: the generated companions of device-resident
    // primals, ordered AFTER the forward kernels. Each companion's tuple
    // gradient return lowers through the multi-output ABI into distinct
    // output resources; a carried companion missing from the lowered MIR
    // fails construction closed (relation facts are the typed routing
    // surface — never a name heuristic).
    for entry in companions.iter() {
        if !entry.device_resident {
            continue;
        }
        let companion = validated
            .program()
            .functions
            .iter()
            .find(|function| function.source == Some(entry.companion))
            .ok_or_else(|| {
                vec![device_diag(
                    "companion",
                    format!(
                        "companion DefId({}) of device-resident primal DefId({}) is missing from the lowered MIR",
                        entry.companion.0, entry.primal.0
                    ),
                )]
            })?;
        build_kernel(companion)?;
    }

    // S5-U5 train_step path: the optimizer-step functions the entry loop
    // calls, materialized after the forward + companion kernels. Their
    // trainable parameters are PerProgram InOut HostProvided buffers; their
    // update output slots alias those parameter buffers (in-place updates).
    if let Some(training) = &training {
        for function in program.functions.iter() {
            if training
                .trainable
                .iter()
                .any(|param| param.step_function == function.id)
            {
                build_kernel(function)?;
            }
        }
    }

    // Pass 2: materialize the program with the merged identity facts. Every
    // reference to a unified id carries the same name/role/lifetime so the
    // schema's cross-reference consistency checks pass. The launch ids are
    // assigned in declaration order; the launch SEQUENCE is re-ordered
    // afterwards to follow the carried dependency graph (F3) — declaration
    // order is never an execution authority.
    let lifetime = if training.is_some() {
        DeviceProgramLifetime::RepeatingStep
    } else {
        DeviceProgramLifetime::SingleRun
    };
    let mut program = DeviceProgram::new(lifetime);
    for build in builds {
        let kernel_index = program.kernels.len();
        let mut resources: Vec<DeviceResource> = Vec::with_capacity(build.resources.len());
        for slot in build.resources {
            let entry = buffers
                .iter()
                .find(|entry| entry.id == slot.buffer_id)
                .expect("every slot buffer was registered by the unification pass");
            let identity = BufferIdentity {
                id: entry.id,
                name: entry.name.clone(),
                role: entry.role,
                storage: MirTensorStorageLayout::DeviceHandle,
                // Independent allocation-lifetime axis (F5): decided from the
                // buffer's aggregate access facts (written / consumed) and the
                // trainable-parameter flag, never derived from the role. The
                // host receives these facts through the payload; it never
                // re-derives a lifetime from slot role alone.
                lifetime: unified_lifetime(entry),
            };
            resources.push(DeviceResource {
                buffer: identity,
                version: BufferVersion {
                    version: 1,
                    element_ty: slot.element_ty,
                    element_count: slot.element_count,
                },
                binding: Binding {
                    group: slot.group,
                    binding: slot.binding,
                },
                access: slot.access,
            });
        }

        let launch_id = LaunchId(u32::try_from(program.launches.len()).unwrap_or(u32::MAX) + 1);
        program.kernels.push(KernelUnit {
            function: build.function,
            entry: build.entry.clone(),
            plan: build.plan,
            resources: resources.clone(),
            launch: build.launch,
        });
        program.launches.push(LaunchUnit {
            id: launch_id,
            kernel_index,
        });
        for resource in resources {
            // F6: a result names a DECLARED observation point — only the
            // program's Output-role buffers that are final observations
            // (ObservationPoint lifetime). Writable intermediates (InOut),
            // per-step data-flow buffers (PerStep), and persistent parameter
            // state are never results merely because they are writable.
            if resource.buffer.role == BufferRole::Output
                && resource.buffer.lifetime == BufferLifetime::ObservationPoint
            {
                program
                    .results
                    .push(radix_mir::device_program::ResultBuffer {
                        buffer: resource.buffer.clone(),
                        version: resource.version.clone(),
                        role: resource.buffer.role,
                        produced_by: launch_id,
                    });
            }
        }
    }

    // The launch sequence follows the carried dependency graph (F3), never
    // kernel declaration order. A RepeatingStep program's trainable-param
    // edges are persistent-state facts (the step writes the value the NEXT
    // step reads) — they are excluded so the single-step sequence stays
    // acyclic.
    let param_buffer_ids: Vec<BufferId> = buffers
        .iter()
        .filter(|buffer| buffer.param)
        .map(|buffer| buffer.id)
        .collect();
    program.launches = dependency_ordered_launches(&program, &param_buffer_ids)?;

    // Pass 3: mint the carried semantic facts (F1–F6) and validate the
    // program together with them fail-closed.
    let semantics = semantic_facts(&program, &buffers, companions);
    program
        .validate_with_semantics(&semantics)
        .map_err(|error| {
            vec![device_diag(
                "validation",
                format!("constructed device program is inconsistent: {error}"),
            )]
        })?;
    // The effective declared step count (S5-U5b): the admitted training-plan
    // count when the program is a RepeatingStep training program; otherwise
    // the caller's declared value (ignored — the wire only carries the count
    // for RepeatingStep).
    let effective_steps = training
        .as_ref()
        .map(|facts| facts.step_count)
        .unwrap_or(declared_steps);
    Ok(Some((program, semantics, effective_steps)))
}

/// The output-slot tuple index of a multi-output ABI output resource: the
/// first output (`signature.output`) is tuple element 0, each
/// `extra_outputs` element follows.
fn output_slot_index(signature: &MirKernelSignature, output: &MirKernelResource) -> u32 {
    if signature.output.binding == output.binding {
        return 0;
    }
    for (index, extra) in signature.extra_outputs.iter().enumerate() {
        if extra.binding == output.binding {
            return u32::try_from(index + 1).unwrap_or(u32::MAX);
        }
    }
    u32::MAX
}

// ---------------------------------------------------------------------------
// Typed device-program wire (S3-A4 / N3.3, the operator-authorized seam break)
// ---------------------------------------------------------------------------

/// Wire/codec version. Bump only with a documented representation change
/// (the wire is the identity substrate of the same-package hash).
///
/// **v2 → v3 (S3-A4, the faber-owned codec bump in lockstep with the FMIR
/// 4 → 5 ratchet):** the thinned `DeviceRunPlan` slot list is REPLACED by
/// the serialized COMPLETE program — the typed
/// [`FmirDeviceProgramSection`] wire (kernels, launches, results,
/// per-resource access + version). CUDA symbols and host input values are
/// no longer part of the canonical program bytes: they ride the per-artifact
/// symbol mapping and the declared-inputs section. Admission is fail-closed:
/// [`admit_device_program_section`] checks the wire version before any
/// field-level interpretation, so an old wire payload fails with the
/// structured `payload_version` diagnostic.
///
/// **v3 → v4 (Stage 3R U1, the F1–F7 frozen-contract clean break, S1):**
/// the wire gains the carried semantic-value identities (F1), explicit
/// value generations per slot (F2), roots + producer/consumer dependencies
/// (F3), and the independent initialization / observation axes (F5/F6).
/// Bumped in lockstep with [`radix_mir_fmir::WIRE_DEVICE_PROGRAM_VERSION`];
/// the radix decode boundary rejects any other value before field-level
/// interpretation (S8/F7).
///
/// **v4 → v5 (S5-U5b, the declared-step-count clean break):** the
/// `RepeatingStep` lifetime variant carries the declared training step
/// count (a count the wire admits fail-closed at decode — zero is
/// rejected). Bumped in lockstep with
/// [`radix_mir_fmir::WIRE_DEVICE_PROGRAM_VERSION`].
const DEVICE_RUN_PLAN_VERSION: u32 = 5;

/// Fail-closed wire admission (S3-A4): the wire version is read FIRST,
/// before any field-level interpretation, so an old (or unknown) codec
/// version fails with the structured `payload_version` diagnostic — never a
/// silent default and never a generic field-level error that hides the
/// version gate. After the version gate, the S5-U5b declared step count is
/// admitted: a `RepeatingStep` program must declare at least one step (a
/// count the route could never drive fails closed here and at the radix
/// decode boundary).
///
/// # Errors
/// Fail-closed when the wire carries an unsupported version, or a
/// `RepeatingStep` program declares a zero step count.
pub(crate) fn admit_device_program_section(
    section: &FmirDeviceProgramSection,
) -> Result<(), Vec<Diagnostic>> {
    if section.v != DEVICE_RUN_PLAN_VERSION {
        return Err(vec![Diagnostic::error(format!(
            "device section wire version {} is not supported (expected {})",
            section.v, DEVICE_RUN_PLAN_VERSION
        ))
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")
        .with_arg("payload_version", section.v.to_string())]);
    }
    // S5-U5b: the declared RepeatingStep count is admitted fail-closed at
    // the faber boundary too — a zero count is a contradiction, never a
    // silently-defaulted step loop.
    if let WireProgramLifetime::RepeatingStep(declared) = section.program.lifetime {
        if declared == 0 {
            return Err(vec![Diagnostic::error(
                "device image declares a RepeatingStep program with step count 0; a training program must declare at least one step",
            )]);
        }
    }
    Ok(())
}

/// Build the typed complete-program wire from a constructed device program
/// and its carried semantic facts (S3-A4). Every program fact the constructor
/// materialized is carried field-for-field: kernels (function id, entry,
/// plan, typed resources with access + version, launch), the ordered launch
/// sequence, the program lifetime, and the explicit result buffers — plus
/// the frozen semantic contract (Stage 3R F1–F7): the semantic-value table
/// (F1), explicit value generations per slot (F2), carried roots +
/// producer/consumer dependencies (F3), the lossless primal/companion
/// relation (F4), per-buffer initialization policies and explicit
/// observation facts on every result row (F5/F6). CUDA symbols and host
/// input values are NOT program semantics — they never enter the canonical
/// bytes. A RepeatingStep program carries its declared training step count
/// (S5-U5b) in the `RepeatingStep(count)` lifetime variant.
#[must_use]
fn wire_program_for_program(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    repeating_steps: u32,
) -> WireDeviceProgram {
    WireDeviceProgram {
        kernels: program
            .kernels
            .iter()
            .enumerate()
            .map(|(kernel_index, kernel)| WireKernelUnit {
                function: kernel.function.0,
                entry: kernel.entry.clone(),
                plan: wire_plan(&kernel.plan),
                resources: kernel
                    .resources
                    .iter()
                    .map(|resource| WireDeviceResource {
                        buffer: wire_buffer_identity(&resource.buffer),
                        version: wire_buffer_version(&resource.version),
                        binding: WireBinding {
                            group: resource.binding.group,
                            binding: resource.binding.binding,
                        },
                        access: wire_access(resource.access),
                        // Explicit value generation (F2): the slot's carried
                        // generation fact — a write/read-write slot carries
                        // the generation its launch produces; a read slot
                        // carries the generation it consumes. Never a
                        // universal `1`.
                        generation: wire_slot_generation(
                            program,
                            semantics,
                            kernel_index,
                            resource,
                        ),
                        // Independent initialization axis (F5): how the
                        // buffer's storage is brought to its first defined
                        // state, decided from access facts, never from role.
                        initialization: wire_initialization_policy(semantics, resource.buffer.id),
                    })
                    .collect(),
                launch: WireKernelLaunchPlan {
                    workgroup: WireWorkgroupSize {
                        x: kernel.launch.workgroup.x,
                        y: kernel.launch.workgroup.y,
                        z: kernel.launch.workgroup.z,
                    },
                    dispatch_size: WireDispatchSize {
                        x: kernel.launch.dispatch_size.x,
                        y: kernel.launch.dispatch_size.y,
                        z: kernel.launch.dispatch_size.z,
                    },
                    workgroup_count: WireWorkgroupCount {
                        x: kernel.launch.workgroup_count.x,
                        y: kernel.launch.workgroup_count.y,
                        z: kernel.launch.workgroup_count.z,
                    },
                },
            })
            .collect(),
        launches: program
            .launches
            .iter()
            .map(|launch| WireLaunchUnit {
                id: launch.id.0,
                kernel_index: u32::try_from(launch.kernel_index).unwrap_or(u32::MAX),
            })
            .collect(),
        lifetime: match program.lifetime {
            DeviceProgramLifetime::SingleRun => WireProgramLifetime::SingleRun,
            DeviceProgramLifetime::RepeatingStep => {
                WireProgramLifetime::RepeatingStep(repeating_steps)
            }
        },
        results: program
            .results
            .iter()
            .map(|result| WireResultBuffer {
                buffer: wire_buffer_identity(&result.buffer),
                version: wire_buffer_version(&result.version),
                role: wire_role(result.role),
                produced_by: result.produced_by.0,
                // Explicit observation fact (F5/F6): the result row IS a
                // declared observation point at the producing launch's
                // completion boundary.
                observation: WireObservationFact {
                    at_launch: result.produced_by.0,
                },
            })
            .collect(),
        // The carried semantic-value table (F1): every value minted from its
        // carried MIR/value origin.
        semantic_values: semantics
            .values
            .iter()
            .map(|value| WireSemanticValue {
                id: value.id.0,
                name: value.name.clone(),
                origin: wire_origin(&value.origin),
            })
            .collect(),
        // Declared legal execution roots (F3): the launches no dependency
        // edge feeds — the minimal set a host may start from.
        roots: semantics.roots.iter().map(|root| root.0).collect(),
        // Carried producer/consumer dependency edges (F3): hosts schedule the
        // validated graph from these facts, never from kernel declaration
        // order.
        dependencies: semantics
            .dependencies
            .iter()
            .map(|edge| WireDependencyEdge {
                producer: edge.producer.0,
                consumer: edge.consumer.0,
                buffer: edge.buffer.0,
                version: edge.version,
            })
            .collect(),
        // Lossless primal/companion relation rows (F4) projected from the
        // carried carrier.
        relations: semantics.relations.iter().map(wire_relation).collect(),
    }
}

/// The explicit value generation (F2) a kernel slot carries on the wire,
/// projected from the carried generation facts (the semantic
/// [`ValueGeneration`] list) — never reconstructed from declaration order or
/// a universal `1`. A write/read-write slot carries the generation its
/// launch produces; a read slot carries the generation its launch consumes
/// (the latest produced before it, or the buffer's initial state).
fn wire_slot_generation(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    kernel_index: usize,
    resource: &DeviceResource,
) -> u32 {
    let value = semantics
        .bindings
        .iter()
        .find(|binding| binding.buffer == resource.buffer.id)
        .map(|binding| binding.value);
    let Some(value) = value else {
        return 1;
    };
    let Some(launch) = program
        .launches
        .iter()
        .find(|launch| launch.kernel_index == kernel_index)
    else {
        return 1;
    };
    match resource.access {
        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite => semantics
            .generations
            .iter()
            .find(|generation| generation.value == value && generation.produced_by == launch.id)
            .map(|generation| generation.generation)
            .unwrap_or(1),
        MirKernelResourceAccess::Read => {
            let position = launch_position(program, launch.id);
            semantics
                .generations
                .iter()
                .filter(|generation| {
                    generation.value == value
                        && launch_position(program, generation.produced_by) < position
                })
                .map(|generation| generation.generation)
                .max()
                .unwrap_or(1)
        }
    }
}

/// The position of a launch in the ordered execution sequence.
fn launch_position(program: &DeviceProgram, launch: LaunchId) -> usize {
    program
        .launches
        .iter()
        .position(|candidate| candidate.id == launch)
        .unwrap_or(usize::MAX)
}

/// Map a carried [`SemanticValueOrigin`] onto its typed wire mirror (F1).
fn wire_origin(origin: &SemanticValueOrigin) -> WireSemanticValueOrigin {
    match origin {
        SemanticValueOrigin::MirLocal { function, local } => WireSemanticValueOrigin::MirLocal {
            function: function.0,
            local: *local,
        },
        SemanticValueOrigin::HostInput => WireSemanticValueOrigin::HostInput,
        SemanticValueOrigin::Synthetic { label } => WireSemanticValueOrigin::Synthetic {
            label: label.clone(),
        },
    }
}

/// Map a carried lossless companion-relation row onto its typed wire mirror
/// (F4): the gradient-to-primal identity survives onto the serialized
/// package.
fn wire_relation(entry: &LosslessMirCompanionEntry) -> WireCompanionRelation {
    WireCompanionRelation {
        primal: entry.primal.0,
        companion: entry.companion.0,
        derivative: match entry.derivative {
            MirCompanionDerivativeKind::ReverseModeVjp => {
                WireCompanionDerivativeKind::ReverseModeVjp
            }
        },
        device_resident: entry.device_resident,
        selected_inputs: entry
            .selected_inputs
            .iter()
            .map(|selected| WireCompanionSelectedInput {
                param: selected.param.0,
                position: selected.position,
                ty: selected.ty.0,
                gradient_slot: selected.gradient_slot,
            })
            .collect(),
        selected_outputs: entry
            .selected_outputs
            .iter()
            .map(|selected| WireCompanionSelectedOutput {
                position: selected.position,
                ty: selected.ty.0,
                upstream_gradient_ty: selected.upstream_gradient_ty.0,
            })
            .collect(),
    }
}

/// The independent initialization axis (F5) of a buffer, projected onto the
/// wire from the carried facts: how its storage is brought to its first
/// defined state — decided from the buffer's aggregate access facts, never
/// from its role or lifetime. A read-only host-provided buffer is uploaded
/// (HostProvided); a buffer any slot writes in place is zero-filled at
/// allocation (ZeroFill — its first generation is defined at allocation); a
/// kernel-written buffer is fully defined by the kernel before any read
/// (KernelInitialized).
fn wire_initialization_policy(
    semantics: &DeviceSemantics,
    buffer: BufferId,
) -> WireInitializationPolicy {
    semantics
        .initializations
        .iter()
        .find(|fact| fact.buffer == buffer)
        .map(|fact| match fact.policy {
            InitializationPolicy::ZeroFill => WireInitializationPolicy::ZeroFill,
            InitializationPolicy::HostProvided => WireInitializationPolicy::HostProvided,
            InitializationPolicy::KernelInitialized => WireInitializationPolicy::KernelInitialized,
        })
        .unwrap_or(WireInitializationPolicy::KernelInitialized)
}

fn wire_buffer_identity(identity: &BufferIdentity) -> WireBufferIdentity {
    WireBufferIdentity {
        id: identity.id.0,
        name: identity.name.clone(),
        role: wire_role(identity.role),
        storage: match identity.storage {
            MirTensorStorageLayout::DeviceHandle => WireStorageLayout::DeviceHandle,
            MirTensorStorageLayout::HostOwned => WireStorageLayout::HostOwned,
        },
        lifetime: match identity.lifetime {
            BufferLifetime::PerProgram => WireBufferLifetime::PerProgram,
            BufferLifetime::PerStep => WireBufferLifetime::PerStep,
            BufferLifetime::ObservationPoint => WireBufferLifetime::ObservationPoint,
        },
        // The stable semantic value identity (F1): every buffer reference
        // carries the value it holds (first projection — one value per
        // buffer, minted above).
        semantic_value: identity.id.0,
    }
}

fn wire_buffer_version(version: &BufferVersion) -> WireBufferVersion {
    WireBufferVersion {
        version: version.version,
        element_ty: data_type_spelling(version.element_ty),
        element_count: version.element_count,
    }
}

fn wire_role(role: BufferRole) -> WireBufferRole {
    match role {
        BufferRole::Input => WireBufferRole::Input,
        BufferRole::Output => WireBufferRole::Output,
        BufferRole::InOut => WireBufferRole::InOut,
    }
}

fn wire_access(access: MirKernelResourceAccess) -> WireResourceAccess {
    match access {
        MirKernelResourceAccess::Read => WireResourceAccess::Read,
        MirKernelResourceAccess::Write => WireResourceAccess::Write,
        MirKernelResourceAccess::ReadWrite => WireResourceAccess::ReadWrite,
    }
}

/// Map a radix [`CollectionKernelPlan`] onto its typed wire mirror (the
/// complete plan is a program fact — never dropped on the wire).
fn wire_plan(plan: &CollectionKernelPlan) -> WireCollectionKernelPlan {
    match plan {
        CollectionKernelPlan::Elementwise => WireCollectionKernelPlan::Elementwise,
        CollectionKernelPlan::TiledMatMul(matmul) => {
            WireCollectionKernelPlan::TiledMatMul(WireMatMulPlan {
                m: matmul.m,
                k: matmul.k,
                n: matmul.n,
                tile: matmul.tile,
                workgroup_x: matmul.workgroup_x,
                workgroup_y: matmul.workgroup_y,
                shared_memory: WireMatMulSharedMemory {
                    shared_a: wire_shared_layout(&matmul.shared_memory.shared_a),
                    shared_b: wire_shared_layout(&matmul.shared_memory.shared_b),
                },
                barriers: matmul
                    .barriers
                    .iter()
                    .map(|barrier| WireBarrierPoint {
                        after: match barrier.after {
                            radix_mir::kernel_plan::BarrierPhase::SharedMemoryLoad => {
                                WireBarrierPhase::SharedMemoryLoad
                            }
                            radix_mir::kernel_plan::BarrierPhase::ReductionStep => {
                                WireBarrierPhase::ReductionStep
                            }
                            radix_mir::kernel_plan::BarrierPhase::InnerProductStep => {
                                WireBarrierPhase::InnerProductStep
                            }
                        },
                    })
                    .collect(),
                oob_padding: wire_oob_padding(matmul.oob_padding),
            })
        }
        CollectionKernelPlan::TreeReduction(reduction) => {
            WireCollectionKernelPlan::TreeReduction(WireReductionPlan {
                op: match reduction.op {
                    radix_mir::kernel_plan::ReduceOp::Sum => WireReduceOp::Sum,
                    radix_mir::kernel_plan::ReduceOp::Mean => WireReduceOp::Mean,
                },
                length: reduction.length,
                workgroup_x: reduction.workgroup_x,
                partials: reduction.partials,
                shared_memory: wire_shared_layout(&reduction.shared_memory),
                barriers: reduction
                    .barriers
                    .iter()
                    .map(|barrier| WireBarrierPoint {
                        after: match barrier.after {
                            radix_mir::kernel_plan::BarrierPhase::SharedMemoryLoad => {
                                WireBarrierPhase::SharedMemoryLoad
                            }
                            radix_mir::kernel_plan::BarrierPhase::ReductionStep => {
                                WireBarrierPhase::ReductionStep
                            }
                            radix_mir::kernel_plan::BarrierPhase::InnerProductStep => {
                                WireBarrierPhase::InnerProductStep
                            }
                        },
                    })
                    .collect(),
                oob_padding: wire_oob_padding(reduction.oob_padding),
            })
        }
        // S5-U1: the rank-2 transpose recipe is a complete program fact —
        // never dropped on the wire (the mirror must carry it).
        CollectionKernelPlan::Transpose(transpose) => {
            WireCollectionKernelPlan::Transpose(WireTransposePlan {
                m: transpose.m,
                n: transpose.n,
                workgroup_x: transpose.workgroup_x,
                dispatch_x: transpose.dispatch_x,
            })
        }
    }
}

fn wire_shared_layout(
    layout: &radix_mir::kernel_plan::SharedMemoryLayout,
) -> WireSharedMemoryLayout {
    WireSharedMemoryLayout {
        element_byte_width: u32::try_from(layout.element_byte_width).unwrap_or(u32::MAX),
        slot_count: layout.slot_count,
        buffer_name: layout.buffer_name.clone(),
    }
}

fn wire_oob_padding(padding: radix_mir::kernel_plan::OobPaddingPolicy) -> WireOobPaddingPolicy {
    match padding {
        radix_mir::kernel_plan::OobPaddingPolicy::ZeroFill => WireOobPaddingPolicy::ZeroFill,
    }
}

// ---------------------------------------------------------------------------
// FMIR device-section assembly
// ---------------------------------------------------------------------------

/// Assemble the FMIR `device` section for a constructed device program.
///
/// Emits both backend artifacts through the S1-3 emitters (Metal MSL always;
/// CUDA PTX through the admitted clang NVPTX compiler when present — a
/// machine without the build-time compiler carries no CUDA artifact and
/// `--backend cuda` fails closed at run time as a missing declared
/// artifact), builds the canonical run-plan payload (with the host input
/// values), and records the selection + runtime requirements.
///
/// # Errors
/// Fail-closed when artifact emission fails (a carried plan or binding that
/// contradicts the typed function facts fails closed, A3).
pub(crate) fn device_section_for_program(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    selection: DeviceSelection,
    inputs: &BTreeMap<String, Vec<f32>>,
    ptx_target: &str,
    repeating_steps: u32,
) -> Result<FmirDeviceSection, Vec<Diagnostic>> {
    // S5-U5c: the emitters re-derive each kernel's body from the validated
    // MIR, so the shape-folded bodies the constructor planned must reach
    // them too — a kernel's function id still references the ORIGINAL
    // (unfolded) body in `validated`. Build a folded validated token: the
    // SAME fold the constructor applied, applied to the program's
    // kernel-referenced functions in a cloned program (a shape-bearing
    // kernel the constructor admitted folds here with identical inputs;
    // unfoldable shapes were already rejected closed there).
    let mut emitter_program = validated.program().clone();
    let kernel_ids: BTreeSet<MirFunctionId> =
        program.kernels.iter().map(|kernel| kernel.function).collect();
    for function in &mut emitter_program.functions {
        if !kernel_ids.contains(&function.id) || !function_has_shape_construction(function) {
            continue;
        }
        let outcome = radix_mir::static_shape_fold::fold_static_shapes(
            function,
            validated.validation(),
        )
        .map_err(|error| vec![device_diag("shape fold", error.message)])?;
        *function = outcome.function;
    }
    let emitter_context = validated.validation().clone();
    let emitter_validated =
        ValidatedMir::new(emitter_program, emitter_context).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| device_diag("shape fold", error.message))
                .collect::<Vec<_>>()
        })?;

    let metal_artifact =
        radix_mir_metal::emit_metal_device_artifact(program, &emitter_validated, interner)
            .map_err(|error| vec![device_diag("metal artifact", error.to_string())])?;
    // S3-A5 (Metal lane): the CUDA artifact emission is best-effort — an
    // emitter op the CUDA lane does not support yet (the companion's
    // elementwise surface lands in S3-A7) leaves the image Metal-only, and a
    // later `--backend cuda` request fails closed as a missing declared
    // artifact (the same seam the PTX-compile-unavailable path uses). The
    // Metal artifact is the S3-A5 proof surface.
    let cuda_artifact = match radix_mir_llvm::emit_cuda_device_artifact(
        program,
        &emitter_validated,
        interner,
    ) {
            Ok(artifact) => Some(artifact),
            Err(error) => {
                eprintln!(
                    "faber: CUDA artifact not emitted (S3-A7 emitter surface): {}",
                    error
                );
                None
            }
        };

    // The CUDA logical-entry → symbol mapping rides the artifact as
    // per-artifact metadata (N3.3): it is an artifact fact, not a program
    // semantic, so it never enters the canonical program bytes.
    let cuda_symbols = cuda_artifact
        .as_ref()
        .map(|artifact| {
            artifact
                .kernels
                .iter()
                .map(|identity| FmirDeviceSymbol {
                    entry: identity.entry.clone(),
                    symbol: identity.symbol.clone(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: metal_artifact.source,
        hash: metal_artifact.hash,
        symbols: Vec::new(),
    }];
    if let Some(cuda_artifact) = &cuda_artifact {
        match radix_mir_llvm::compile_nvvm_to_ptx(&cuda_artifact.source, ptx_target) {
            Ok(ptx) => {
                // The packaged CUDA artifact is PTX (N1.3 §3.1); its provenance
                // hash covers the PTX blob, not the NVVM source.
                let ptx_hash = radix_mir_fmir::fnv1a64_blob_hash(ptx.as_bytes());
                artifact.push(FmirDeviceArtifact {
                    backend: FmirDeviceBackend::Cuda,
                    blob: ptx,
                    hash: ptx_hash,
                    symbols: cuda_symbols,
                });
            }
            Err(error) => {
                // Build-time PTX compile unavailable (clang NVPTX missing): the
                // image carries the Metal artifact only and a later `--backend
                // cuda` request fails closed as a missing declared artifact.
                eprintln!(
                    "faber: CUDA PTX artifact not emitted (build-time clang NVPTX unavailable): {error}"
                );
            }
        }
    }

    let wire = wire_program_for_program(program, semantics, repeating_steps);
    let declared_inputs = inputs
        .iter()
        .map(|(name, values)| FmirDeviceInput {
            name: name.clone(),
            values: values.clone(),
        })
        .collect();

    let mut runtime_requirements = Vec::new();
    for artifact_entry in &artifact {
        match artifact_entry.backend {
            FmirDeviceBackend::Metal => runtime_requirements.push("device:metal".to_owned()),
            FmirDeviceBackend::Cuda => runtime_requirements.push("device:cuda".to_owned()),
        }
    }
    runtime_requirements.sort();
    runtime_requirements.dedup();

    Ok(FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: DEVICE_RUN_PLAN_VERSION,
            program: wire,
        },
        selection: match selection {
            DeviceSelection::Auto => FmirDeviceSelection::Auto,
            DeviceSelection::Metal => FmirDeviceSelection::Metal,
            DeviceSelection::Cuda => FmirDeviceSelection::Cuda,
        },
        artifacts: FmirDeviceArtifactsSection { artifact },
        declared_inputs,
        runtime_requirements,
    })
}

/// The independent allocation-lifetime axis (F5) of a unified buffer,
/// decided from its aggregate access facts and the trainable-parameter flag —
/// never from the role:
///
/// - a trainable parameter is persistent device-resident state (per-program);
/// - a buffer no kernel ever writes is host-provided persistent state
///   (per-program);
/// - a kernel-written buffer another kernel consumes is a step-local
///   intermediate (per-step);
/// - a kernel-written final is read back at an observation point.
fn unified_lifetime(entry: &ProgramBuffer) -> BufferLifetime {
    // S5-U5 (param-identity.md): a trainable parameter accumulates updates
    // across steps in ONE buffer — allocated once (per-program), initialized
    // HostProvided at session creation, updated in place at every step.
    if entry.param {
        return BufferLifetime::PerProgram;
    }
    // G4 (F5): an in-place ReadWrite slot is PERSISTENT writable state — an
    // accumulation/optimizer-state buffer — not a per-step intermediate.
    // Its storage is allocated once (per-program), zero-filled at allocation
    // (the readwrite→ZeroFill initialization axis), and updated in place at
    // every generation: the accumulator/optimizer-state lifecycle G4
    // requires. No axis is derived from the role; the access facts decide.
    if entry.readwrite {
        return BufferLifetime::PerProgram;
    }
    // A companion gradient slot is per-step scratch (S5-U5): it feeds the
    // optimizer step within one step and recycles at the step boundary — even
    // a frozen-input gradient with no step consumer is never an observation.
    if entry.gradient {
        return BufferLifetime::PerStep;
    }
    if !entry.written {
        BufferLifetime::PerProgram
    } else if entry.consumed {
        BufferLifetime::PerStep
    } else {
        BufferLifetime::ObservationPoint
    }
}

fn data_type_spelling(_ty: MirType) -> String {
    // The S1-1 device-program schema pins f32 as the campaign dtype (the
    // numeric policy admits only f32 and integer dtypes; the vertical-slice
    // kernel surface is f32). The host descriptor's typed element type is
    // always F32 for the Stage 1 slice; a future dtype widens this mapping
    // with the schema that carries it — never inferred from emitted text.
    "f32".to_owned()
}

// ---------------------------------------------------------------------------
// Descriptor construction + ordinary-command execution seam
// ---------------------------------------------------------------------------

/// The declared backend artifact for a resolved backend, from the image's
/// artifacts section.
pub(crate) fn artifact_for_backend(
    artifacts: &[FmirDeviceArtifact],
    backend: DeviceBackend,
) -> Option<&FmirDeviceArtifact> {
    artifacts.iter().find(|artifact| match backend {
        DeviceBackend::Metal => artifact.backend == FmirDeviceBackend::Metal,
        DeviceBackend::Cuda => artifact.backend == FmirDeviceBackend::Cuda,
    })
}

/// Build the typed host descriptor from the image's WIRE + backend artifact
/// blob (S3-A4: the host consumes the wire — the descriptor is derived
/// exclusively from the carried program facts, never from a thinned slot
/// list).
///
/// The S1-3 typed logical-entry → NVVM-symbol mapping is **consumed here**
/// from the artifact's per-artifact metadata: the CUDA descriptor's kernel
/// entry is the emitted PTX `.entry` symbol, never the logical entry; Metal
/// launches by the logical entry (the emitted MSL kernel name). Slots are
/// carried in binding order so the composite host binds buffers in the
/// emitted kernel's buffer/param order. The wire's typed lifetimes and
/// program regime are mapped onto the host descriptor's
/// [`DeviceBufferLifetime`]/[`DeviceProgramLifetime`] — the host consumes
/// the carried facts; it never re-derives a lifetime from slot role (S2-4).
///
/// # Errors
/// Fail-closed when a carried element-type spelling is outside the campaign
/// dtype surface (never a silent default), or when a result record does not
/// match a writable, observation-point resource of its producing launch.
pub(crate) fn descriptor_for_backend(
    device: &FmirDeviceSection,
    backend: DeviceBackend,
    blob: &[u8],
) -> Result<DeviceDescriptor, Vec<Diagnostic>> {
    let wire = &device.device_program.program;
    validate_wire_results(wire)?;
    let mut kernels = Vec::with_capacity(wire.kernels.len());
    let mut buffer_versions = Vec::new();
    for kernel in &wire.kernels {
        let entry = match backend {
            DeviceBackend::Cuda => device
                .artifacts
                .artifact
                .iter()
                .find(|artifact| artifact.backend == FmirDeviceBackend::Cuda)
                .and_then(|artifact| {
                    artifact
                        .symbols
                        .iter()
                        .find(|identity| identity.entry == kernel.entry)
                        .map(|identity| identity.symbol.clone())
                })
                .unwrap_or_else(|| kernel.entry.clone()),
            DeviceBackend::Metal => kernel.entry.clone(),
        };
        let mut buffers = Vec::with_capacity(kernel.resources.len());
        for resource in &kernel.resources {
            let element_ty = wire_element_ty_to_host(&resource.version.element_ty)?;
            add_descriptor_buffer_version(
                &mut buffer_versions,
                resource.buffer.id,
                resource.version.version,
                element_ty,
                resource.version.element_count,
            )?;
            buffers.push(DescriptorBuffer {
                buffer_id: resource.buffer.id,
                buffer_name: resource.buffer.name.clone(),
                // F1: the wire's carried stable semantic value identity —
                // the host consumes it; it never derives identity from
                // names, shapes, binding positions, or declaration order.
                semantic_value: resource.buffer.semantic_value,
                role: wire_role_to_host(resource.buffer.role),
                lifetime: wire_lifetime_to_host(resource.buffer.lifetime),
                // F5 (G4): the wire's carried initialization axis is
                // projected verbatim — the host honors it (zero-fill
                // persistent accumulation state at allocation); it never
                // re-derives initialization from role or lifetime.
                initialization: wire_initialization_to_host(resource.initialization),
                binding: resource.binding.binding,
                element_ty,
                element_count: resource.version.element_count,
                // R2: the host consumes the wire's carried content version —
                // it never re-derives or hardcodes `1` for the A10 graph.
                version: resource.version.version,
            });
        }
        kernels.push(DescriptorKernel {
            entry,
            buffers,
            grid: [
                u32::try_from(kernel.launch.workgroup_count.x).unwrap_or(u32::MAX),
                u32::try_from(kernel.launch.workgroup_count.y).unwrap_or(u32::MAX),
                u32::try_from(kernel.launch.workgroup_count.z).unwrap_or(u32::MAX),
            ],
            block: [
                kernel.launch.workgroup.x,
                kernel.launch.workgroup.y,
                kernel.launch.workgroup.z,
            ],
        });
    }
    for result in &wire.results {
        let element_ty = wire_element_ty_to_host(&result.version.element_ty)?;
        add_descriptor_buffer_version(
            &mut buffer_versions,
            result.buffer.id,
            result.version.version,
            element_ty,
            result.version.element_count,
        )?;
    }

    let descriptor = DeviceDescriptor {
        backend,
        module_image: blob.to_vec(),
        kernels,
        launches: wire
            .launches
            .iter()
            .map(|launch| DescriptorLaunch {
                id: launch.id,
                kernel_index: launch.kernel_index,
            })
            .collect(),
        buffer_versions,
        program_lifetime: match wire.lifetime {
            WireProgramLifetime::SingleRun => HostDeviceProgramLifetime::SingleRun,
            WireProgramLifetime::RepeatingStep(_) => HostDeviceProgramLifetime::RepeatingStep,
        },
        // R2/F3: the host consumes the WIRE'S CARRIED dependency edges
        // (real versions, producer/consumer per buffer version) verbatim —
        // the A10 graph is never re-derived from launch order or access
        // facts. The wire's `dependencies` are the materializer's frozen
        // producer/consumer facts (F3).
        data_flow: wire
            .dependencies
            .iter()
            .map(|edge| HostDescriptorDataFlow {
                buffer_id: edge.buffer,
                version: edge.version,
                producer: edge.producer,
                consumer: edge.consumer,
            })
            .collect(),
        // F3: the declared legal execution roots — the launches the graph may
        // start from, carried verbatim.
        roots: wire.roots.clone(),
        // F6: the declared observation points — the explicit result rows the
        // host reads back, projected from the wire's observation facts.
        results: wire
            .results
            .iter()
            .map(|result| DescriptorResult {
                buffer_id: result.buffer.id,
                version: result.version.version,
                produced_by: result.produced_by,
                at_launch: result.observation.at_launch,
            })
            .collect(),
    };

    descriptor
        .validate()
        .map_err(|error| vec![super::host_factory::host_error_diagnostic(&error)])?;
    Ok(descriptor)
}

/// Validate each wire result against the resource facts of its producing
/// launch before projecting the program into a host descriptor. The host
/// descriptor has no result surface, so a result-only or contradictory record
/// would otherwise be able to add metadata without proving a real producer.
/// Result rows are the authoritative readback set, but the host can only read
/// back `ObservationPoint` buffers and its receipt is keyed by buffer id; an
/// unsupported lifetime or repeated id therefore fails before host creation.
fn validate_wire_results(wire: &WireDeviceProgram) -> Result<(), Vec<Diagnostic>> {
    let mut result_buffers = BTreeMap::new();
    for (result_index, result) in wire.results.iter().enumerate() {
        if !matches!(result.role, WireBufferRole::Output | WireBufferRole::InOut) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} has invalid observation role {}",
                    result.role.spelling()
                ),
            )]);
        }
        if result.role != result.buffer.role {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} has observation role {} but buffer {} is {}",
                    result.role.spelling(),
                    result.buffer.id,
                    result.buffer.role.spelling()
                ),
            )]);
        }

        let Some(launch) = wire
            .launches
            .iter()
            .find(|launch| launch.id == result.produced_by)
        else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names unknown producing launch {}",
                    result.produced_by
                ),
            )]);
        };
        let Some(kernel) = wire.kernels.get(launch.kernel_index as usize) else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names producing launch {}, whose kernel index {} is unknown",
                    result.produced_by, launch.kernel_index
                ),
            )]);
        };

        let Some(resource) = kernel
            .resources
            .iter()
            .find(|resource| resource.buffer.id == result.buffer.id)
        else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names buffer {} version {} from producing launch {}, but that launch has no matching resource",
                    result.buffer.id, result.version.version, result.produced_by
                ),
            )]);
        };
        if resource.buffer != result.buffer {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} has identity facts that contradict its producing launch {}",
                    result.buffer.id, result.produced_by
                ),
            )]);
        }

        if resource.version.version != result.version.version {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} declares version {}, but producing launch {} uses version {}",
                    result.buffer.id,
                    result.version.version,
                    result.produced_by,
                    resource.version.version
                ),
            )]);
        }
        if resource.version.element_ty != result.version.element_ty
            || resource.version.element_count != result.version.element_count
        {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} version {} carries shape {}[{}], but producing launch {} carries {}[{}]",
                    result.buffer.id,
                    result.version.version,
                    result.version.element_ty,
                    result.version.element_count,
                    result.produced_by,
                    resource.version.element_ty,
                    resource.version.element_count
                ),
            )]);
        }
        // F6 (Stage 3R): the result row's explicit observation fact must
        // name the producing launch's completion boundary. A result whose
        // observation contradicts its producer is a writable intermediate
        // exposed without an explicit observation fact — rejected before
        // host construction (the same rule the radix decode boundary runs).
        if result.observation.at_launch != result.produced_by {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names producing launch {}, but its explicit observation fact names launch {}; a result is a declared observation point at its producing launch",
                    result.produced_by, result.observation.at_launch
                ),
            )]);
        }
        if resource.buffer.lifetime != WireBufferLifetime::ObservationPoint {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names buffer {} with lifetime {}; only observation-point results are supported by the host readback contract",
                    result.buffer.id,
                    resource.buffer.lifetime.spelling()
                ),
            )]);
        }
        if !matches!(
            resource.access,
            WireResourceAccess::Write | WireResourceAccess::ReadWrite
        ) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names launch {} as producer, but its matching resource is read-only",
                    result.produced_by
                ),
            )]);
        }
        if let Some(first_index) = result_buffers.insert(result.buffer.id, result_index) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} repeats observation buffer {} already named by result {first_index}; result buffers must be unique in the host receipt",
                    result.buffer.id
                ),
            )]);
        }
    }
    Ok(())
}

fn add_descriptor_buffer_version(
    versions: &mut Vec<DescriptorBufferVersion>,
    buffer_id: u32,
    version: u32,
    element_ty: DeviceDataType,
    element_count: u64,
) -> Result<(), Vec<Diagnostic>> {
    if let Some(existing) = versions
        .iter()
        .find(|existing| existing.buffer_id == buffer_id && existing.version == version)
    {
        if existing.element_ty != element_ty || existing.element_count != element_count {
            return Err(vec![device_diag(
                "buffer version",
                format!("buffer {buffer_id} version {version} carries conflicting shape facts"),
            )]);
        }
        return Ok(());
    }

    versions.push(DescriptorBufferVersion {
        buffer_id,
        version,
        element_ty,
        element_count,
    });
    Ok(())
}

fn wire_role_to_host(role: WireBufferRole) -> DeviceBufferRole {
    match role {
        WireBufferRole::Input => DeviceBufferRole::Input,
        WireBufferRole::Output => DeviceBufferRole::Output,
        WireBufferRole::InOut => DeviceBufferRole::InOut,
    }
}

/// Map the wire's typed lifetime onto the host descriptor's typed lifetime.
/// The wire is the typed section (deny_unknown_fields admission), so the
/// mapping is a total function over the three-class enum (N3.4).
fn wire_lifetime_to_host(lifetime: WireBufferLifetime) -> DeviceBufferLifetime {
    match lifetime {
        WireBufferLifetime::PerProgram => DeviceBufferLifetime::PerProgram,
        WireBufferLifetime::PerStep => DeviceBufferLifetime::PerStep,
        WireBufferLifetime::ObservationPoint => DeviceBufferLifetime::ObservationPoint,
    }
}

/// Map the wire's typed initialization policy (F5) onto the host descriptor's
/// typed initialization axis. Total over the three-class enum; the host
/// honors it at allocation (zero-fill persistent state), never re-deriving it
/// from role or lifetime.
fn wire_initialization_to_host(
    initialization: WireInitializationPolicy,
) -> DeviceBufferInitialization {
    match initialization {
        WireInitializationPolicy::ZeroFill => DeviceBufferInitialization::ZeroFill,
        WireInitializationPolicy::HostProvided => DeviceBufferInitialization::HostProvided,
        WireInitializationPolicy::KernelInitialized => {
            DeviceBufferInitialization::KernelInitialized
        }
    }
}

/// Map the wire's element-type spelling onto the host's typed element type.
/// The campaign dtype surface pins f32 (the S1-1 schema); an unknown spelling
/// fails closed — never a silent default and never an unreachable arm.
fn wire_element_ty_to_host(spelling: &str) -> Result<DeviceDataType, Vec<Diagnostic>> {
    match spelling {
        "f32" => Ok(DeviceDataType::F32),
        other => Err(vec![device_diag(
            "element type",
            format!("device program element type `{other}` is outside the campaign dtype surface"),
        )]),
    }
}

/// Map the wire's named declared inputs onto buffer ids (via the wire's
/// input-buffer identities).
///
/// The map covers BOTH the program's read-only input buffers AND a
/// RepeatingStep program's trainable parameters — InOut buffers with
/// `HostProvided` initialization (the once-init param values). The host
/// consumes the map through [`ProgramSession::init_params`] for
/// `RepeatingStep` sessions and through per-execution copy-in for
/// `SingleRun` sessions; the extra entries are inert in either mode's copy
/// loop (the host copies only declared Input slots per execution).
fn inputs_by_buffer_id(device: &FmirDeviceSection) -> BTreeMap<u32, Vec<f32>> {
    let mut by_name: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for input in &device.declared_inputs {
        by_name.insert(input.name.clone(), input.values.clone());
    }
    let mut by_id: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
    for kernel in &device.device_program.program.kernels {
        for resource in &kernel.resources {
            let is_input = resource.buffer.role == WireBufferRole::Input;
            let is_host_provided_param = resource.buffer.role == WireBufferRole::InOut
                && resource.initialization == WireInitializationPolicy::HostProvided;
            if (is_input || is_host_provided_param)
                && !by_id.contains_key(&resource.buffer.id)
            {
                if let Some(values) = by_name.get(&resource.buffer.name) {
                    by_id.insert(resource.buffer.id, values.clone());
                }
            }
        }
    }
    by_id
}

/// The explicit result buffer ids the run reads back (S2-4).
///
/// Result rows are the authoritative readback set. `validate_wire_results`
/// proves that each row names a unique `ObservationPoint` resource before this
/// function is used, so no valid result can disappear through a role/lifetime
/// re-derivation. Test-only since U5: the host projects readbacks from the
/// descriptor's carried observation facts, so the route no longer selects
/// outputs itself.
#[cfg(test)]
fn observation_buffer_ids(device: &FmirDeviceSection) -> Vec<u32> {
    let mut ids = Vec::new();
    for result in &device.device_program.program.results {
        if !ids.contains(&result.buffer.id) {
            ids.push(result.buffer.id);
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// Wire-derived A10 resource graph (S3-A4)
// ---------------------------------------------------------------------------

/// One wire-derived A10 graph buffer (identity + content version).
#[cfg(test)]
struct WireGraphBuffer {
    id: u32,
    name: String,
    version: u32,
    element_count: u64,
}

/// One wire-derived inter-kernel data-flow edge.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WireGraphEdge {
    buffer_id: u32,
    version: u32,
    producer: u32,
    consumer: u32,
}

/// Derive the A10 resource graph from the wire's COMPLETE facts (N3.3): the
/// per-buffer identity + content version, and the producer/consumer edges
/// from the carried ordered access + launches. This is the same derivation
/// as the radix-mir `BufferRegistry` over the program — the host consumes
/// the carried facts instead of re-deriving topology from launch order or a
/// slot-role string (no coincidence-based first-writer rule, no hardcoded
/// version). Test-only since U5: the descriptor consumes the wire's CARRIED
/// `dependencies` verbatim.
#[cfg(test)]
fn wire_resource_graph(device: &FmirDeviceSection) -> (Vec<WireGraphBuffer>, Vec<WireGraphEdge>) {
    let wire = &device.device_program.program;
    let mut buffers: Vec<WireGraphBuffer> = Vec::new();
    let mut producers: Vec<(u32, u32, u32)> = Vec::new();
    let mut consumers: Vec<(u32, u32, u32)> = Vec::new();
    for launch in &wire.launches {
        let Some(kernel) = wire.kernels.get(launch.kernel_index as usize) else {
            continue;
        };
        for resource in &kernel.resources {
            let id = resource.buffer.id;
            if !buffers
                .iter()
                .any(|buffer| buffer.id == id && buffer.version == resource.version.version)
            {
                buffers.push(WireGraphBuffer {
                    id,
                    name: resource.buffer.name.clone(),
                    version: resource.version.version,
                    element_count: resource.version.element_count,
                });
            }
            match resource.access {
                WireResourceAccess::Read => {
                    consumers.push((id, resource.version.version, launch.id));
                }
                WireResourceAccess::Write => {
                    producers.push((id, resource.version.version, launch.id));
                }
                WireResourceAccess::ReadWrite => {
                    consumers.push((id, resource.version.version, launch.id));
                    producers.push((id, resource.version.version, launch.id));
                }
            }
        }
    }
    // Results contribute the observed versions to the chain.
    for result in &wire.results {
        if !buffers
            .iter()
            .any(|buffer| buffer.id == result.buffer.id && buffer.version == result.version.version)
        {
            buffers.push(WireGraphBuffer {
                id: result.buffer.id,
                name: result.buffer.name.clone(),
                version: result.version.version,
                element_count: result.version.element_count,
            });
        }
    }
    // Data-flow edges (mirrors `BufferRegistry::data_flow_pairs`): every
    // producer/consumer launch pair per (buffer, version), excluding
    // self-edges.
    let mut edges: Vec<WireGraphEdge> = Vec::new();
    for (buffer_id, version, producer) in &producers {
        for (consumer_id, consumer_version, consumer) in &consumers {
            if consumer_id == buffer_id && consumer_version == version && consumer != producer {
                edges.push(WireGraphEdge {
                    buffer_id: *buffer_id,
                    version: *version,
                    producer: *producer,
                    consumer: *consumer,
                });
            }
        }
    }
    (buffers, edges)
}

/// The wire's logical name for a buffer id (diagnostics).
fn wire_buffer_name(device: &FmirDeviceSection, id: u32) -> String {
    device
        .device_program
        .program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .find(|resource| resource.buffer.id == id)
        .map(|resource| resource.buffer.name.clone())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

/// The ordered step-run result of a program session (S5-U5): the per-step
/// observed values (the loss trace) and the convergence verdict.
pub(crate) struct StepRunReport {
    /// How many ordered launches / training steps executed.
    pub(crate) step_count: u32,
    /// The per-execution observed readbacks, in execution order (the loss
    /// trace for a RepeatingStep run).
    pub(crate) loss_trace: Vec<BTreeMap<u32, Vec<f32>>>,
    /// The first observed value of the first execution (initial loss).
    pub(crate) initial_loss: Option<f32>,
    /// The first observed value of the last execution (final loss).
    pub(crate) final_loss: Option<f32>,
    /// Whether the training run converged: `final_loss < 0.1 * initial_loss`
    /// (the Stage 5 gate) — or, when the initial loss is not positive, the
    /// final loss strictly decreased.
    pub(crate) converged: bool,
}

/// The first observed value of one execution's readbacks: the first element
/// of the first observed buffer (deterministic BTreeMap order by buffer id).
fn first_observed(outputs: &BTreeMap<u32, Vec<f32>>) -> Option<f32> {
    outputs
        .iter()
        .next()
        .and_then(|(_, values)| values.first())
        .copied()
}

/// Reduce an ordered execution receipt list to the step-run report (S5-U5):
/// the loss trace (every observed readback per execution), the initial/final
/// loss, and the convergence verdict. Pure — the route prints it; the tests
/// assert on it.
#[must_use]
pub(crate) fn step_run_report(receipts: &[faber_host_macos_arm64::composite_host::DeviceExecutionReceipt]) -> StepRunReport {
    let loss_trace: Vec<BTreeMap<u32, Vec<f32>>> = receipts
        .iter()
        .map(|receipt| receipt.outputs.clone())
        .collect();
    let initial_loss = loss_trace.first().and_then(first_observed);
    let final_loss = loss_trace.last().and_then(first_observed);
    let converged = match (initial_loss, final_loss) {
        (Some(initial), Some(last)) if initial > 0.0 => last < 0.1 * initial,
        (Some(initial), Some(last)) => last < initial,
        _ => false,
    };
    StepRunReport {
        step_count: u32::try_from(receipts.len()).unwrap_or(u32::MAX),
        loss_trace,
        initial_loss,
        final_loss,
        converged,
    }
}

/// Execute a program session under its declared lifetime (S5-U5): a
/// `RepeatingStep` session once-inits its HostProvided params at session
/// creation (`init_params` — copied in exactly once, never re-copied on
/// later steps) and executes `steps` training steps, reading back the
/// declared observations per step (the loss trace). A `SingleRun` session
/// executes its ordered launches (the S2-8 repeat surface). Returns every
/// execution receipt in order.
///
/// # Errors
/// Fail-closed host diagnostics (a `RepeatingStep` session refuses
/// `execute`; a `SingleRun` session refuses `init_params`/`execute_step`).
fn execute_session_receipts(
    session: &mut ProgramSession,
    descriptor: &DeviceDescriptor,
    inputs: &BTreeMap<u32, Vec<f32>>,
    steps: u32,
) -> Result<Vec<faber_host_macos_arm64::composite_host::DeviceExecutionReceipt>, Vec<Diagnostic>> {
    match descriptor.program_lifetime {
        HostDeviceProgramLifetime::RepeatingStep => {
            session
                .init_params(inputs)
                .map_err(|error| vec![super::host_factory::host_error_diagnostic(&error)])?;
            let mut receipts = Vec::with_capacity(steps as usize);
            for _ in 0..steps {
                receipts.push(
                    session
                        .execute_step()
                        .map_err(|error| vec![super::host_factory::host_error_diagnostic(&error)])?,
                );
            }
            Ok(receipts)
        }
        HostDeviceProgramLifetime::SingleRun => {
            let repeat_count = device_repeat_count()?;
            let mut receipts = Vec::with_capacity(repeat_count);
            for _ in 0..repeat_count {
                receipts.push(
                    session
                        .execute(inputs)
                        .map_err(|error| vec![super::host_factory::host_error_diagnostic(&error)])?,
                );
            }
            Ok(receipts)
        }
    }
}

/// The `FABER_DEVICE_STEPS` env-var override (S5-U5b): when set, the value
/// must agree with the image's **declared** RepeatingStep step count
/// (recovered from the wire) — a contradiction fails closed, never a silent
/// override. When absent, the image's declared count is the authority; the
/// env var is never the sole authority for an image-loaded route.
fn device_step_count(declared: u32) -> Result<u32, Vec<Diagnostic>> {
    match std::env::var("FABER_DEVICE_STEPS") {
        Ok(value) => {
            let parsed = value.parse::<u32>().map_err(|error| {
                vec![Diagnostic::error(format!(
                    "FABER_DEVICE_STEPS must be a non-negative integer, got `{value}`: {error}"
                ))]
            })?;
            if parsed != declared {
                return Err(vec![Diagnostic::error(format!(
                    "FABER_DEVICE_STEPS={parsed} contradicts the image's declared RepeatingStep step count {declared}; the route's override must agree with the device image"
                ))]);
            }
            Ok(parsed)
        }
        Err(std::env::VarError::NotPresent) => Ok(declared),
        Err(error) => Err(vec![Diagnostic::error(format!(
            "FABER_DEVICE_STEPS could not be read: {error}"
        ))]),
    }
}

/// Execute a device-bearing FMIR image's device route through the composite
/// host and print the A9/A10 receipt (S2-8) or the training report (S5-U5).
///
/// The ordinary-command launch seam (S1-6): constructs the composite host
/// under the one host-construction policy, builds the typed descriptor from
/// the image's canonical payload + declared artifact blob, executes the
/// full lifecycle (load → allocate → copy-in → launch → sync → readback →
/// release), and prints the A9 observed events (selected hardware, module
/// hash, allocations, launches, syncs, transfers, readbacks, releases), the
/// A10 declared logical resource graph (buffer identities, roles, lifetimes,
/// versions, data-flow edges), and the repeated-execution leak proof.
///
/// A `RepeatingStep` program (S5-U5, the training-loop route) once-inits its
/// HostProvided params at session creation, executes the image's DECLARED
/// step count (recovered from the wire, S5-U5b — `FABER_DEVICE_STEPS`, when
/// set, must agree) on ONE session, prints the per-step loss trace, and runs
/// the convergence check. `FABER_DEVICE_REPEAT` (default 1) runs a
/// `SingleRun` program's ordered launch sequence N times before teardown —
/// the S2-8 leak-proof surface.
///
/// # Errors
/// Fail-closed diagnostics; never a silent CPU fallback.
pub(crate) fn execute_device_route(
    device: &FmirDeviceSection,
    backend: DeviceBackend,
    source_hashes: &[String],
) -> Result<(), Vec<Diagnostic>> {
    // Fail-closed wire admission (S3-A4): the typed-section wire version is
    // gated before any field-level interpretation (old v2 payloads fail
    // closed with the structured `payload_version` diagnostic).
    admit_device_program_section(&device.device_program)?;
    let artifact = artifact_for_backend(&device.artifacts.artifact, backend)
        .ok_or_else(|| vec![super::host_factory::missing_backend_artifact(backend)])?;
    // A9 discovery receipt: selected device + declared artifact hash.
    let discovery = super::host_factory::discovery_receipt(backend, &device.artifacts.artifact)
        .ok_or_else(|| vec![super::host_factory::missing_backend_artifact(backend)])?;
    discovery.print();
    // The host consumes the WIRE: the descriptor is derived exclusively from
    // the carried program facts (never a thinned slot list).
    let descriptor = descriptor_for_backend(device, backend, artifact.blob.as_bytes())?;
    // Fail-before-launch: the descriptor is validated by the composite host
    // before any kernel is dispatched.
    let selection = match backend {
        DeviceBackend::Metal => DeviceSelection::Metal,
        DeviceBackend::Cuda => DeviceSelection::Cuda,
    };
    let mut host = super::host_factory::construct_composite_host(selection, true)
        .map_err(|diagnostic| vec![diagnostic])?;
    let inputs = inputs_by_buffer_id(device);
    // The explicit observation facts, already validated by descriptor
    // construction, are the sole authority for host readback selection (F6):
    // the host projects results from the descriptor's carried result rows.

    // A10 identity over the COMPLETE program (S3-A4): the canonical bytes of
    // the typed wire (semantics-only — CUDA symbols and declared inputs are
    // absent by construction), hashed with the source identities. Both image
    // routes carry the identical wire, so the identity is route-independent.
    let source_refs = source_hashes.iter().map(String::as_str).collect::<Vec<_>>();
    let canonical = radix_mir_fmir::canonical_program_bytes(&device.device_program.program);
    let identity = radix_mir_fmir::device_identity_hash(&source_refs, &canonical);
    println!("device: identity {identity} (A10, complete program)");

    // The step count a RepeatingStep route drives (S5-U5b): the image's
    // DECLARED count is recovered from the wire — the route never falls back
    // to an env-var default. `FABER_DEVICE_STEPS`, when set, must agree
    // (fail-closed). SingleRun routes keep the S2-8 repeat surface.
    let steps = match device.device_program.program.lifetime {
        WireProgramLifetime::RepeatingStep(declared) => device_step_count(declared)?,
        WireProgramLifetime::SingleRun => DEFAULT_TRAINING_STEPS,
    };

    let mut session = super::host_factory::create_program_session(&mut host, &descriptor)
        .map_err(|diagnostic| vec![diagnostic])?;
    let receipts = execute_session_receipts(&mut session, &descriptor, &inputs, steps)?;
    let receipt = receipts.last().ok_or_else(|| {
        vec![Diagnostic::error(
            "device route executed zero iterations (FABER_DEVICE_STEPS / FABER_DEVICE_REPEAT must be >= 1)",
        )]
    })?;
    session
        .teardown()
        .map_err(|error| vec![super::host_factory::host_error_diagnostic(&error)])?;

    // A9 observed lifecycle events of the last execution (R9): real
    // synchronization operations, the exact readback count, and the
    // completion boundary the receipt states.
    println!(
        "device: module hash fnv64:{:016x} semantic graph hash fnv64:{:016x} launches {} syncs {} transfers {} readbacks {} releases {} allocated {}",
        receipt.module_hash,
        receipt.semantic_graph_hash,
        receipt.launches,
        receipt.syncs,
        receipt.transfers,
        receipt.readbacks,
        receipt.releases,
        receipt.allocated_buffers.len()
    );
    println!("device: {}", receipt.completion_boundary.spelling());
    println!("{}", host_receipt_launch_order_line(&descriptor));

    // A10 declared logical resource graph: render the host's receipt facts
    // verbatim. Faber must not print a duplicate graph derived from the wire
    // after execution, because the host receipt is the observable seam.
    for line in host_receipt_graph_lines(&receipt.resource_graph, &receipt.data_flow_edges) {
        println!("{line}");
    }

    for (buffer_id, values) in &receipt.outputs {
        let name = wire_buffer_name(device, *buffer_id);
        println!(
            "device: output buffer {} `{}` = [{}]",
            buffer_id,
            name,
            values
                .iter()
                .map(|value| format!("{value}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // S5-U5 training report: a RepeatingStep run prints the per-step loss
    // trace and the convergence verdict (the done-when surface).
    if descriptor.program_lifetime == HostDeviceProgramLifetime::RepeatingStep {
        let report = step_run_report(&receipts);
        print_training_report(&report);
    }

    // Repeated-execution leak proof (S2-8 done-when): after N runs + teardown
    // the live handle count is 0 and the driver counters are at baseline. On
    // real drivers the counters surface reports all-zero by design (the leak
    // evidence is the handle-registry live count); the fake drivers track
    // cumulative loads/releases so tests prove the cache policy at the driver
    // boundary.
    let live = host
        .device()
        .map(|runtime| runtime.live_handle_count())
        .unwrap_or(0);
    let counters = host.device().map(|runtime| runtime.driver_counters());
    match counters {
        Some(counters) => println!(
            "device: leak proof: {} run(s) then teardown -> live_handle_count()={live}, driver counters at baseline (module loads {} releases {} buffer allocs {} releases {})",
            receipts.len(),
            counters.module_loads,
            counters.module_releases,
            counters.buffer_allocs,
            counters.buffer_releases
        ),
        None => println!(
            "device: leak proof: {} run(s) then teardown -> live_handle_count()={live}, no device session after teardown",
            receipts.len()
        ),
    }
    Ok(())
}

/// Print the S5-U5 training report: the per-step loss trace and the
/// convergence verdict.
fn print_training_report(report: &StepRunReport) {
    println!(
        "device: training: {} step(s) on ONE session; per-step observation (loss) trace:",
        report.step_count
    );
    for (index, observed) in report.loss_trace.iter().enumerate() {
        let values = observed
            .values()
            .map(|values| {
                values
                    .iter()
                    .map(|value| format!("{value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect::<Vec<_>>()
            .join(" | ");
        println!("device:   step {index}: [{values}]");
    }
    match (report.initial_loss, report.final_loss) {
        (Some(initial), Some(last)) => println!(
            "device: training: initial loss {initial}, final loss {last}, converged: {} (final < 0.1 * initial)",
            report.converged
        ),
        _ => println!(
            "device: training: no loss observation read back; convergence not checked"
        ),
    }
}

/// Render the exact ordered launch records that the host will execute.
///
/// The descriptor's launch sequence, not the kernel declaration order or the
/// aggregate receipt count, is the observable program order. A kernel index
/// may therefore repeat or appear out of declaration order.
fn host_receipt_launch_order_line(descriptor: &DeviceDescriptor) -> String {
    let launches = descriptor
        .launches
        .iter()
        .enumerate()
        .map(|(position, launch)| {
            let backend_entry = descriptor
                .kernels
                .get(launch.kernel_index as usize)
                .map(|kernel| kernel.entry.as_str())
                .unwrap_or("<invalid>");
            format!(
                "#{} id={} kernel_index={} backend_entry=`{}`",
                position, launch.id, launch.kernel_index, backend_entry
            )
        })
        .collect::<Vec<_>>();
    format!("device: launch order: [{}]", launches.join(", "))
}

fn host_receipt_graph_lines(
    resource_graph: &[faber_host_macos_arm64::composite_host::ReceiptBuffer],
    data_flow_edges: &[faber_host_macos_arm64::composite_host::DataFlowEdge],
) -> Vec<String> {
    let mut lines = vec!["device: declared resource graph (A10, host receipt):".to_owned()];
    for buffer in resource_graph {
        lines.push(format!(
            "device:   buffer {} `{}` {} {} version {} ({}[{}])",
            buffer.id,
            buffer.name,
            buffer.role.spelling(),
            buffer.lifetime.spelling(),
            buffer.version,
            buffer.element_ty.spelling(),
            buffer.element_count
        ));
    }
    if data_flow_edges.is_empty() {
        lines.push("device:   data-flow edges: none".to_owned());
    } else {
        for edge in data_flow_edges {
            lines.push(format!(
                "device:   data-flow {} -> {} via buffer {} version {}",
                edge.producer, edge.consumer, edge.buffer_id, edge.version
            ));
        }
    }
    lines
}

/// The `FABER_DEVICE_REPEAT` env-var hook for the S2-8 repeated-execution
/// leak proof: how many times to run the ordered launch sequence on one
/// session before teardown. Defaults to 1; a non-numeric value fails closed
/// (never a silent fallback to 1).
fn device_repeat_count() -> Result<usize, Vec<Diagnostic>> {
    match std::env::var("FABER_DEVICE_REPEAT") {
        Ok(value) => value.parse::<usize>().map_err(|error| {
            vec![Diagnostic::error(format!(
                "FABER_DEVICE_REPEAT must be a non-negative integer, got `{value}`: {error}"
            ))]
        }),
        Err(std::env::VarError::NotPresent) => Ok(1),
        Err(error) => Err(vec![Diagnostic::error(format!(
            "FABER_DEVICE_REPEAT could not be read: {error}"
        ))]),
    }
}

#[cfg(test)]
#[path = "device_test.rs"]
mod tests;
