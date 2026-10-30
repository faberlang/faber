//! The single typed MIR → [`DeviceProgram`] materializer (DPSC-U2; extended
//! to the faber superset in DPSC-U4a: decomposition fallback, training
//! param/gradient aliasing, dependency-ordered launches, declared
//! observation cadence, and the effective step count).
//!
//! This module owns the construction spine.  Consumers may build views over
//! the returned program, but they do not rebuild its kernels, buffers,
//! launches, or semantic facts.

use super::host_wiring::{HostWiring, Slot, collect_host_wiring};
use super::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramError, DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit,
    LaunchId, LaunchUnit, ObservationCadence, ResultBuffer,
};
use crate::abi::{
    MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole,
    MirKernelSignature, collection_op_contract,
};
use crate::device::{MirCompanionMap, MirDeviceFunctionMetadataMap};
use crate::device_plan_compiler::LibraryOperation;
use crate::device_program_plans::{
    elementwise_subchain_signature_for_emission, explicit_output_subchain_signature,
    is_transformer_recipe_op, kernel_plan_for_function, scan_plan,
    subchain_has_runtime_shaped_buffer, subchain_signature_for_emission_with_source,
    transformer_subchain_signature_for_emission_with_source,
};
use crate::device_semantics::{
    DependencyEdge, DeviceSemantics, InitializationFact, InitializationPolicy,
    LosslessMirCompanionEntry, ObservationFact, SemanticValue, SemanticValueId,
    SemanticValueOrigin, ValueBinding, ValueGeneration,
};
use crate::kernel_decomposition::decompose_kernel_function;
use crate::kernel_plan::CollectionKernelPlan;
use crate::layout::MirTensorStorageLayout;
use crate::static_shape_fold::{ShapeFoldError, fold_validated_device_program};
use crate::{
    MirCallee, MirConstant, MirFunction, MirFunctionId, MirFunctionOrigin, MirFunctionVisibility,
    MirLocalId, MirOperand, MirParamMode, MirStatementKind, MirType, MirValidationContext,
    ValidatedMir,
};
use radix_hir::DefId;
use radix_lexer::Interner;
use radix_types::{IndexExpr, Type, TypeTable};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// A typed failure from the one device-program construction spine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceProgramBuildError {
    /// A selected kernel's storage-buffer ABI could not be synthesized.
    KernelAbi {
        /// The offending MIR function.
        function: MirFunctionId,
        /// The ABI diagnostic detail.
        detail: String,
    },
    /// A selected kernel's typed collection plan could not be resolved.
    KernelPlan {
        /// The offending MIR function.
        function: MirFunctionId,
        /// The plan diagnostic detail.
        detail: String,
    },
    /// An ABI role/access pair cannot be represented by the program schema.
    BufferRole {
        /// The offending MIR function.
        function: MirFunctionId,
        /// The ABI binding.
        binding: u32,
        /// The ABI role.
        role: String,
        /// The ABI access.
        access: String,
    },
    /// A companion relation names a device companion absent from MIR.
    MissingCompanion {
        /// The primal's stable definition identity.
        primal: u32,
        /// The missing companion's stable definition identity.
        companion: u32,
    },
    /// A multi-recipe body could not be decomposed at recipe boundaries.
    Decomposition {
        /// The offending MIR function.
        function: MirFunctionId,
        /// The decomposition diagnostic detail.
        detail: String,
    },
    /// The subchain type table could not be restored for decomposition.
    SubchainTypes {
        /// The typed table diagnostic.
        detail: String,
    },
    /// The carried producer/consumer dependency graph contains a cycle.
    LaunchOrder {
        /// The launch-order diagnostic detail.
        detail: String,
    },
    /// The shared static-shape fold rejected a selected device function.
    ShapeFold(ShapeFoldError),
    /// One buffer version received contradictory producer projections.
    ReducedProjection {
        /// The affected program buffer.
        buffer: BufferId,
        /// The first projection fact.
        first: String,
        /// The contradictory projection fact.
        second: String,
    },
    /// One buffer version received contradictory parameter shape vectors.
    BufferShape {
        /// The affected program buffer.
        buffer: BufferId,
        /// The first shape fact.
        first: String,
        /// The contradictory shape fact.
        second: String,
    },
    /// The composed program or semantic facts failed the shared schema.
    Program(DeviceProgramError),
}

impl fmt::Display for DeviceProgramBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KernelAbi { function, detail } => {
                write!(f, "kernel {} ABI synthesis failed: {detail}", function.0)
            }
            Self::KernelPlan { function, detail } => {
                write!(f, "kernel {} plan resolution failed: {detail}", function.0)
            }
            Self::BufferRole {
                function,
                binding,
                role,
                access,
            } => write!(
                f,
                "kernel {} binding {binding} has no coherent program buffer role ({role} {access})",
                function.0
            ),
            Self::MissingCompanion { primal, companion } => write!(
                f,
                "companion DefId({companion}) of device-resident primal DefId({primal}) is missing from the lowered MIR"
            ),
            Self::Decomposition { function, detail } => {
                write!(f, "kernel {} decomposition failed: {detail}", function.0)
            }
            Self::SubchainTypes { detail } => {
                write!(f, "subchain type table restoration failed: {detail}")
            }
            Self::LaunchOrder { detail } => write!(f, "launch order failed: {detail}"),
            Self::ShapeFold(error) => write!(f, "device shape fold failed: {error}"),
            Self::ReducedProjection {
                buffer,
                first,
                second,
            } => write!(
                f,
                "buffer {} carries conflicting reduced-resource projections ({first} vs {second})",
                buffer.0
            ),
            Self::BufferShape {
                buffer,
                first,
                second,
            } => write!(
                f,
                "buffer {} carries conflicting parameter shape vectors ({first} vs {second})",
                buffer.0
            ),
            Self::Program(error) => {
                write!(f, "device program construction failed validation: {error}")
            }
        }
    }
}

impl std::error::Error for DeviceProgramBuildError {}

/// One typed storage-buffer slot during construction.
#[derive(Debug, Clone)]
struct ResourceBuild {
    group: u32,
    binding: u32,
    access: MirKernelResourceAccess,
    buffer_id: BufferId,
    element_ty: MirType,
    element_count: u64,
    reduced_projection: Option<crate::abi::ReducedProjection>,
    /// The slot's declared parameter shape vector (`None` when the slot has
    /// none) — reconciled to one fact per buffer in pass 1.
    shape: Option<Vec<u64>>,
}

/// One kernel's build: signature/plan facts plus its resource slots (buffer
/// ids resolved by the unification pass).
#[derive(Debug, Clone)]
struct KernelBuild {
    function: MirFunctionId,
    entry: String,
    library_operation: LibraryOperation,
    plan: CollectionKernelPlan,
    launch: KernelLaunchPlan,
    resources: Vec<ResourceBuild>,
}

/// The aggregate identity and independent resource-axis facts for one buffer.
#[derive(Debug, Clone)]
struct ProgramBuffer {
    id: BufferId,
    /// The kernel source function that minted this buffer; a `SingleRun`
    /// program joins same-named slots only within one function.
    function: MirFunctionId,
    name: String,
    element_ty: MirType,
    element_count: u64,
    /// First carried parameter shape vector among the joining slots; the
    /// pass-1 reconcile owns contradiction detection.
    shape: Option<Vec<u64>>,
    role: BufferRole,
    written: bool,
    consumed: bool,
    readwrite: bool,
    /// A trainable parameter of a `RepeatingStep` training program (S5-U5):
    /// `PerProgram` persistent state with `HostProvided` init, updated in place
    /// at every step. The flag is the independent fact behind the lifetime
    /// and initialization axes — never derived from the access pattern.
    param: bool,
    /// A backward companion's gradient output slot (S5-U5): per-step scratch
    /// feeding the optimizer step — never an observation point and never
    /// persistent state, even when no step kernel consumes it.
    gradient: bool,
    origin: SemanticValueOrigin,
    /// The kernel slot that minted this buffer, when the host call sites give
    /// it a known value identity (NAMEJOIN NJ2): a later slot of ANOTHER
    /// kernel joins by VALUE when its host-value set equals this slot's.
    host_slot: Option<Slot>,
}

impl ProgramBuffer {
    /// The cross-kernel wiring key: same declared logical name and same
    /// shape. Callers additionally guard on [`unify_roles`] before joining,
    /// so a name/shape coincidence alone never aliases two unrelated values.
    /// Equal element counts are not equal shapes: when both sides carry a
    /// parameter shape vector the vectors must be equal, so a `[3, 8]` and a
    /// `[4, 6]` slot of the same name never share a buffer.
    ///
    /// ACROSS KERNELS a one-sided shape joins only when it cannot be
    /// inherited by a WRITER: a slot with no shape of its own must not take
    /// the joined buffer's shape for its output (the dispatcher sizes a
    /// fresh output from the buffer version's shape). A shapeless READER
    /// slot (it only consumes) and a buffer whose own slots are all readers
    /// (nothing writes into it) have nothing to inherit. Two shapeless slots
    /// (scalars) join. The lenient reading survives only for a join inside
    /// one function.
    fn matches(&self, name: &str, slot: &SlotFacts) -> bool {
        self.name == name && self.matches_value(slot)
    }

    /// The type/count/shape half of [`Self::matches`]: what a VALUE join
    /// (equal host-value sets) checks, with no name involved.
    fn matches_value(&self, slot: &SlotFacts) -> bool {
        self.element_ty == slot.element_ty
            && self.element_count == slot.element_count
            && match (self.shape.as_deref(), slot.shape) {
                (Some(existing), Some(next)) => existing == next,
                (None, None) => true,
                _ if !slot.cross_kernel => true,
                (Some(_), None) => slot.reads,
                (None, Some(_)) => !self.written,
            }
    }
}

/// The type, extent, shape and access facts of a joining slot, with whether
/// the join crosses kernels (see [`ProgramBuffer::matches`]).
#[derive(Clone, Copy)]
struct SlotFacts<'a> {
    element_ty: MirType,
    element_count: u64,
    shape: Option<&'a [u64]>,
    reads: bool,
    cross_kernel: bool,
}

/// Training-side facts intentionally stay MIR-owned.  Faber's package layer
/// can translate its package analysis into this small carrier without making
/// the compiler crate depend on product-private training types.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceTrainingFacts {
    /// The admitted training-plan step count (S5-U5b): the constructor
    /// returns it as the effective step count of the `RepeatingStep` program.
    pub step_count: u32,
    /// Additional validated MIR functions selected by training analysis.
    pub device_function_ids: Vec<MirFunctionId>,
    /// `(function, local)` origins that persist across repeating steps.
    pub persistent_buffers: BTreeSet<(MirFunctionId, MirLocalId)>,
    /// The trainable parameters of the package's training loop (S5-U5): each
    /// step-function param whose update output slot aliases the param buffer
    /// (the in-place update).
    pub trainable: Vec<DeviceTrainableParam>,
    /// The gradient-flow links (S5-U5): which companion result-tuple slot
    /// produces the gradient each step-function param consumes.
    pub gradients: Vec<DeviceGradientLink>,
}

/// One trainable parameter of a `RepeatingStep` training program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceTrainableParam {
    /// The optimizer-step function that updates the parameter.
    pub step_function: MirFunctionId,
    /// The parameter's position in the step function's signature.
    pub param_position: u32,
    /// The step function's full result-tuple slot carrying the update.
    pub update_output: u32,
}

/// One gradient-flow link of a training program (S5-U5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceGradientLink {
    /// The optimizer-step function that consumes the gradient.
    pub step_function: MirFunctionId,
    /// The gradient's position in the step function's signature.
    pub gradient_position: u32,
    /// The generated companion producing the gradient.
    pub companion: DefId,
    /// The companion's full result-tuple slot carrying the gradient.
    pub gradient_slot: u32,
}

/// The direct function a call edge names, when it names one: a `Function`
/// callee carries the id; a function-constant `Value` callee is the same
/// physical edge. Used by the host-launch scan, never by entry admission
/// policy itself.
fn direct_callee_function_id(callee: &MirCallee) -> Option<MirFunctionId> {
    match callee {
        MirCallee::Function(id)
        | MirCallee::Value(MirOperand::Constant(MirConstant::Function(id))) => Some(*id),
        MirCallee::Value(_) | MirCallee::Closure(_) | MirCallee::Definition { .. } => None,
    }
}

/// Explicit-facts admission for one compute-stage candidate (KCM-05).
///
/// Every entry uses the metadata carrier's launchability fact. Named source
/// functions are `Source` rows in that carrier, so an unmarked/private named
/// helper cannot reach ABI synthesis merely because it has a source identity.
/// The only private exception is a synthetic kernel closure with a remaining
/// host call edge: that physical host boundary is its launch fact. A function
/// with no carrier row never admits (fail-closed: an unclassifiable device
/// body cannot become a launch).
fn admits_device_entry(
    function: &MirFunction,
    metadata: &MirDeviceFunctionMetadataMap,
    host_launched: &BTreeSet<MirFunctionId>,
) -> bool {
    let Some(row) = metadata.get(&function.id) else {
        return false;
    };
    if row.origin == MirFunctionOrigin::SyntheticKernelClosure
        && row.visibility == MirFunctionVisibility::Private
    {
        return host_launched.contains(&function.id);
    }
    row.entry_role() == Some(crate::MirDeviceRole::Kernel)
}

/// Build the one target-neutral [`DeviceProgram`], its carried semantics, and
/// the effective declared step count.
///
/// `interner` is explicit because kernel entry identity is a typed MIR fact
/// that must be preserved at the public boundary.  `companions` carries the
/// lossless primal/companion relation.  `training` optionally contributes
/// additional device-resident MIR functions, persistent-buffer origins,
/// trainable-param/gradient aliasing facts, and the admitted step count; the
/// lifetime argument remains the caller's explicit program-lifetime fact.
/// `declared_steps` is the caller's declared count, returned unchanged when
/// there is no training program. The shared static-shape fold is applied once
/// to the complete selected device set before any ABI or plan derivation.
///
/// Selection is explicit-facts admission (KCM-05), never the compute stage
/// alone: named functions are admitted only by their carried public `Source`
/// row; a source-less synthetic kernel closure is admitted only through its
/// metadata carrier — public rows unconditionally, private rows only when a
/// host caller still launches them (the one region/launch law); any compute
/// function with no carrier row is never admitted (fail-closed — an
/// unclassifiable device body cannot become a launch).
///
/// **DPSC-U4a (the faber superset):** a multi-recipe body whose whole-function
/// ABI/plan synthesis fails decomposes at recipe boundaries into subchain
/// kernels (one kernel per recipe, transformer subchains through the P1
/// seam); buffer unification joins producing and consuming slots by the
/// carried `(function, local)` origin identity, the trainable-param update
/// outputs (in-place updates), and the companion gradient outputs; the launch
/// sequence follows the carried producer/consumer dependency graph — never
/// kernel declaration order; and the result rows carry the S5A-U1 declared
/// observation cadence.
///
/// # Errors
/// Returns a typed error when folding, ABI/plan/decomposition derivation,
/// companion lookup, buffer identity construction, launch ordering, or the
/// shared program-plus-semantics schema rejects the result.
///
/// # Panics
/// Panics on internal `expect` failures that presuppose a validated MIR
/// (selected function ids, registered resource buffers).
pub fn build_device_program(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    companions: &MirCompanionMap,
    training: Option<&DeviceTrainingFacts>,
    lifetime: DeviceProgramLifetime,
    declared_steps: u32,
    device_function_metadata: &MirDeviceFunctionMetadataMap,
) -> Result<(DeviceProgram, DeviceSemantics, u32), DeviceProgramBuildError> {
    let program = validated.program();
    // Host-launched synthetic closures: the physical launch fact is a
    // remaining direct call edge from a host-side (non-device-stage) caller.
    // Device-caller references belong to composition, not to a launch row.
    let host_launched: BTreeSet<MirFunctionId> = program
        .functions
        .iter()
        .filter(|caller| caller.shader_stage.is_none())
        .flat_map(|caller| {
            caller.blocks.iter().flat_map(|block| {
                block
                    .statements
                    .iter()
                    .filter_map(|statement| match &statement.kind {
                        MirStatementKind::Call { callee, .. } => direct_callee_function_id(callee),
                        _ => None,
                    })
            })
        })
        .collect();
    let mut selected_ids: BTreeSet<MirFunctionId> = program
        .functions
        .iter()
        .filter(|function| function.shader_stage == Some(crate::MirKernelShaderStage::Compute))
        .filter(|function| admits_device_entry(function, device_function_metadata, &host_launched))
        .map(|function| function.id)
        .collect();

    let mut companion_ids = Vec::new();
    for entry in companions.iter().filter(|entry| entry.device_resident) {
        let Some(function) = program
            .functions
            .iter()
            .find(|function| function.source == Some(entry.companion))
        else {
            return Err(DeviceProgramBuildError::MissingCompanion {
                primal: entry.primal.0,
                companion: entry.companion.0,
            });
        };
        selected_ids.insert(function.id);
        companion_ids.push(function.id);
    }
    if let Some(training) = training {
        selected_ids.extend(training.device_function_ids.iter().copied());
    }

    // This is the single fold site owned by the constructor.  All downstream
    // signature, plan, and launch derivation consumes this returned token.
    let folded = fold_validated_device_program(validated, &selected_ids)
        .map_err(DeviceProgramBuildError::ShapeFold)?;
    let folded_program = folded.program();

    let mut ordered_ids: Vec<MirFunctionId> = folded_program
        .functions
        .iter()
        .filter(|function| {
            function.shader_stage == Some(crate::MirKernelShaderStage::Compute)
                && selected_ids.contains(&function.id)
        })
        .map(|function| function.id)
        .collect();
    for &id in &companion_ids {
        if !ordered_ids.contains(&id) {
            ordered_ids.push(id);
        }
    }
    if let Some(training) = training {
        for id in &training.device_function_ids {
            if selected_ids.contains(id) && !ordered_ids.contains(id) {
                ordered_ids.push(*id);
            }
        }
    }

    // The effective declared step count (S5-U5b): the admitted training-plan
    // count when the program is a RepeatingStep training program; otherwise
    // the caller's declared value.  Computed before the empty guard so both
    // return sites report the same count.
    let effective_steps = training.map_or(declared_steps, |facts| facts.step_count);

    // A validated MIR with no device body has no payload.  Returning the empty
    // pair keeps the constructor total and lets host-partition remain a view.
    if ordered_ids.is_empty() {
        return Ok((
            DeviceProgram::new(lifetime),
            DeviceSemantics::default(),
            effective_steps,
        ));
    }

    // The forward set (the declared compute-stage kernels, not the companions
    // or the optimizer-step functions selected by training analysis): the
    // S5A-U1 per-step observation is a forward kernel's return output.
    let forward_ids: BTreeSet<MirFunctionId> = folded_program
        .functions
        .iter()
        .filter(|function| {
            function.shader_stage == Some(crate::MirKernelShaderStage::Compute)
                && selected_ids.contains(&function.id)
                && !companion_ids.contains(&function.id)
        })
        .map(|function| function.id)
        .collect();

    // (step function, param local) → update output slot for the trainable
    // params of the training loop — resolved once from the folded program so
    // the per-slot aliasing can key on locals.
    let mut param_updates: BTreeMap<(MirFunctionId, MirLocalId), u32> = BTreeMap::new();
    if let Some(training) = training {
        for param in &training.trainable {
            if let Some(function) = folded_program
                .functions
                .iter()
                .find(|function| function.id == param.step_function)
                && let Some(param_local) = function
                    .params
                    .get(param.param_position as usize)
                    .map(|param| param.local)
            {
                param_updates.insert((param.step_function, param_local), param.update_output);
            }
        }
    }
    // Host call-site value identity of the selected kernels (NAMEJOIN NJ2),
    // read from the validated program's host functions: the buffer-join
    // authority of a SingleRun program.
    let wiring = collect_host_wiring(program, &selected_ids);
    let mut builder = KernelBuilder::new(
        interner,
        folded_program,
        device_function_metadata,
        folded.validation(),
        companions,
        training,
        param_updates,
        wiring,
    );
    for function_id in &ordered_ids {
        let function = folded
            .function_by_id(*function_id)
            .expect("selected function ids came from the validated MIR");
        builder.build_function(function)?;
    }

    // S5A-U1: the declared per-step observation of a RepeatingStep training
    // program — the forward kernel's RETURN output (the loss the training
    // loop measures), identified from the training facts, never a shape scan:
    // the last-built forward kernel's first write slot in binding order.
    let per_step_observation: Option<BufferId> = training
        .is_some()
        .then(|| {
            builder
                .builds
                .iter()
                .rev()
                .find(|build| forward_ids.contains(&build.function))
                .and_then(|build| {
                    build
                        .resources
                        .iter()
                        .find(|resource| {
                            matches!(
                                resource.access,
                                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                            )
                        })
                        .map(|resource| resource.buffer_id)
                })
        })
        .flatten();

    // S5A-U1: the declared END-OF-RUN gradient set — the companion gradient
    // buffers the training plan's gradient-flow links feed into a train_step.
    // A frozen-input gradient no step consumes is per-step scratch — declared
    // never, observed never.
    let train_step_gradients: BTreeSet<BufferId> = training
        .map(|training| {
            training
                .gradients
                .iter()
                .filter_map(|link| {
                    builder
                        .gradient_buffers
                        .get(&(link.companion.0, link.gradient_slot))
                        .copied()
                })
                .collect()
        })
        .unwrap_or_default();

    // Council-8 CB-3: the reduced-resource projection is a fact of the BUFFER
    // VERSION, defined by its producing slot — reconciled to ONE fact per
    // (buffer, version); two distinct `Some` facts fail closed here.
    let mut projection_by_buffer: BTreeMap<BufferId, crate::abi::ReducedProjection> =
        BTreeMap::new();
    for build in &builder.builds {
        for resource in &build.resources {
            let Some(projection) = resource.reduced_projection else {
                continue;
            };
            if let Some(previous) = projection_by_buffer.get(&resource.buffer_id) {
                if *previous != projection {
                    return Err(DeviceProgramBuildError::ReducedProjection {
                        buffer: resource.buffer_id,
                        // The prior diagnostic rendered the first fact as an
                        // `Option`; keep that rendering byte-for-byte.
                        first: format!("Some({previous:?})"),
                        second: format!("{projection:?}"),
                    });
                }
            } else {
                projection_by_buffer.insert(resource.buffer_id, projection);
            }
        }
    }

    // Wire-shape widening: the parameter shape vector is a fact of the
    // BUFFER VERSION, defined by its producing slot — reconciled to ONE fact
    // per (buffer, version); two distinct `Some` facts fail closed, exactly
    // like the reduced projection above.
    let mut shape_by_buffer: BTreeMap<BufferId, Vec<u64>> = BTreeMap::new();
    for build in &builder.builds {
        for resource in &build.resources {
            let Some(shape) = &resource.shape else {
                continue;
            };
            if let Some(previous) = shape_by_buffer.get(&resource.buffer_id) {
                if previous.as_slice() != shape.as_slice() {
                    return Err(DeviceProgramBuildError::BufferShape {
                        buffer: resource.buffer_id,
                        // The prior diagnostic rendered the first fact as an
                        // `Option`; keep that rendering byte-for-byte.
                        first: format!("Some({previous:?})"),
                        second: format!("Some({shape:?})"),
                    });
                }
            } else {
                shape_by_buffer.insert(resource.buffer_id, shape.clone());
            }
        }
    }

    // Pass 2: materialize the program with the merged identity facts. Launch
    // ids are assigned in declaration order; the launch SEQUENCE is
    // re-ordered afterwards to follow the carried dependency graph.
    let training_program = training.is_some();
    let mut device_program = DeviceProgram::new(lifetime);
    for build in &builder.builds {
        let kernel_index = device_program.kernels.len();
        let device_resources = build
            .resources
            .iter()
            .map(|resource| {
                let buffer = builder
                    .buffers
                    .iter()
                    .find(|buffer| buffer.id == resource.buffer_id)
                    .expect("every resource buffer was registered");
                let identity = BufferIdentity {
                    id: buffer.id,
                    name: buffer.name.clone(),
                    role: buffer.role,
                    storage: MirTensorStorageLayout::DeviceHandle,
                    lifetime: unified_lifetime(buffer, training_program, per_step_observation),
                };
                let version = BufferVersion {
                    version: 1,
                    element_ty: resource.element_ty,
                    element_count: resource.element_count,
                    // Every slot carrying a projection/shape registered its
                    // buffer in the reconciliation passes above, so the
                    // reconciled maps are the sole authority here.
                    reduced_projection: projection_by_buffer.get(&resource.buffer_id).copied(),
                    shape: shape_by_buffer.get(&resource.buffer_id).cloned(),
                };
                DeviceResource {
                    buffer: identity,
                    version,
                    binding: Binding {
                        group: resource.group,
                        binding: resource.binding,
                    },
                    access: resource.access,
                }
            })
            .collect::<Vec<_>>();
        device_program.kernels.push(KernelUnit {
            function: build.function,
            entry: build.entry.clone(),
            library_operation: build.library_operation,
            plan: build.plan.clone(),
            resources: device_resources,
            launch: build.launch,
        });
        device_program.launches.push(LaunchUnit {
            id: LaunchId(u32::try_from(kernel_index + 1).unwrap_or(u32::MAX)),
            kernel_index,
        });
    }

    // F3: the launch sequence follows the carried producer/consumer
    // dependency graph, never kernel declaration order. A RepeatingStep
    // program's trainable-param edges are persistent-state facts (the step
    // writes the value the NEXT step reads) — excluded so the single-step
    // sequence stays acyclic.
    let param_buffer_ids: Vec<BufferId> = builder
        .buffers
        .iter()
        .filter(|buffer| buffer.param)
        .map(|buffer| buffer.id)
        .collect();
    device_program.launches =
        dependency_ordered_launches(&device_program, &param_buffer_ids, &builder.wiring)?;

    // S5A-U1: declare the observation-eligible results with their cadence —
    // the constructor's DECLARED fact from the training program's structure,
    // never derived from buffer shapes or names.
    device_program.results = declared_result_rows(
        &device_program,
        &builder.buffers,
        &projection_by_buffer,
        &shape_by_buffer,
        training_program,
        per_step_observation,
        &train_step_gradients,
    );

    // Pass 3: mint the carried semantic facts (F1–F6) and validate the
    // program together with them fail-closed.
    let mut semantics = semantic_facts(
        &device_program,
        &builder.buffers,
        companions,
        &param_buffer_ids,
        &builder.wiring,
    );
    semantics.observations = device_program
        .results
        .iter()
        .map(|result| ObservationFact {
            buffer: result.buffer.id,
            version: result.version.version,
            at_launch: result.produced_by,
        })
        .collect();

    device_program
        .validate_with_semantics(&semantics)
        .map_err(DeviceProgramBuildError::Program)?;

    Ok((device_program, semantics, effective_steps))
}

// ---------------------------------------------------------------------------
// Per-kernel construction (whole-function path + decomposition fallback)
// ---------------------------------------------------------------------------

/// One function's kernel construction state: the cross-kernel buffer
/// unification table plus the training aliasing facts.
struct KernelBuilder<'a> {
    interner: &'a Interner,
    /// The folded program: the scope kernel entry names are unique within.
    program: &'a crate::MirProgram,
    /// Explicit device facts indexed by concrete MIR function identity.
    device_function_metadata: &'a MirDeviceFunctionMetadataMap,
    validation: &'a crate::MirValidationContext<'a>,
    /// `(function, local)` origins that persist across repeating steps.
    persistent_locals: BTreeSet<(MirFunctionId, MirLocalId)>,
    /// (step function, param local) → update output slot for the trainable
    /// params of the training loop.
    param_updates: BTreeMap<(MirFunctionId, MirLocalId), u32>,
    /// (step function, param position) → (companion `DefId`, gradient slot)
    /// for the gradient-flow links.
    gradient_links: BTreeMap<(MirFunctionId, u32), (u32, u32)>,
    /// Every selected-input gradient slot of a device-resident companion.
    companion_gradient_slots: BTreeMap<u32, BTreeSet<u32>>,
    /// The companion gradient buffers minted so far, keyed by the producing
    /// (companion `DefId`, full result-tuple slot) — consulted while the
    /// `train_step` kernels are built (the backward → `train_step` edge).
    gradient_buffers: BTreeMap<(u32, u32), BufferId>,
    buffers: Vec<ProgramBuffer>,
    next_buffer_id: u32,
    builds: Vec<KernelBuild>,
    /// Whether same-named parameter slots of DIFFERENT kernels join one
    /// buffer. Training and device-resident programs wire kernels through
    /// shared names; a `SingleRun` program keys residency on host backings, so
    /// each kernel owns its slots and a name coincidence is not a join.
    ///
    /// KNOWN VIOLATION of the value-identity rule (register V-01 training
    /// programs, V-02 device-resident companion programs); not an exemption.
    cross_kernel_name_join: bool,
    /// Host call-site value identity of the selected kernels' slots.
    wiring: HostWiring,
}

impl<'a> KernelBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        interner: &'a Interner,
        program: &'a crate::MirProgram,
        device_function_metadata: &'a MirDeviceFunctionMetadataMap,
        validation: &'a crate::MirValidationContext<'a>,
        companions: &MirCompanionMap,
        training: Option<&DeviceTrainingFacts>,
        param_updates: BTreeMap<(MirFunctionId, MirLocalId), u32>,
        wiring: HostWiring,
    ) -> Self {
        let mut gradient_links = BTreeMap::new();
        let persistent_locals = training
            .map(|facts| facts.persistent_buffers.clone())
            .unwrap_or_default();
        if let Some(training) = training {
            for link in &training.gradients {
                gradient_links.insert(
                    (link.step_function, link.gradient_position),
                    (link.companion.0, link.gradient_slot),
                );
            }
        }
        // EVERY selected-input gradient slot of a device-resident companion
        // is a per-step scratch buffer — including a frozen input's gradient.
        let mut companion_gradient_slots: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for entry in companions.iter().filter(|entry| entry.device_resident) {
            for selected in &entry.selected_inputs {
                companion_gradient_slots
                    .entry(entry.companion.0)
                    .or_default()
                    .insert(selected.gradient_slot);
            }
        }
        Self {
            interner,
            program,
            device_function_metadata,
            validation,
            persistent_locals,
            param_updates,
            gradient_links,
            companion_gradient_slots,
            gradient_buffers: BTreeMap::new(),
            buffers: Vec::new(),
            next_buffer_id: 1,
            builds: Vec::new(),
            cross_kernel_name_join: training.is_some()
                || companions.iter().any(|entry| entry.device_resident),
            wiring,
        }
    }

    /// Build every kernel of one device-resident function: the whole-function
    /// path when the complete ABI signature and plan resolve, else the
    /// decomposition fallback (one kernel per recipe subchain).
    fn build_function(&mut self, function: &MirFunction) -> Result<(), DeviceProgramBuildError> {
        let base_entry =
            crate::host_partition::unique_kernel_entry_name(self.program, function, self.interner);
        // The source function's return-tuple field locals (in full-tuple
        // order) — the discriminator for the S5-U5 update-output aliasing.
        let return_tuple_locals = tuple_return_locals(function).unwrap_or_default();

        // Whole-function path: a single-recipe or elementwise-only body
        // carries ONE kernel from its full ABI signature. Every other body
        // (including the composed MLP) fails the plan pass and falls through
        // to the generic decomposition path below.
        let whole_signature =
            MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                function,
                self.validation,
                self.interner,
            )
            .map_err(|error| DeviceProgramBuildError::KernelAbi {
                function: function.id,
                detail: error.message,
            });
        // `Ok(None)` is the plan pass's elementwise-only verdict: the body
        // carries no recipe and is ONE Elementwise kernel over the full ABI —
        // the same reading the emitters' whole-function cross-check applies
        // (`carried_plan_agrees(Elementwise, None)`). Routing it through the
        // decomposition instead binds the read-order synthetic's params,
        // which disagree with the whole-function buffer indices the emitters
        // lay out (`v ⊙ v + x` would bind `v` at slot 0).
        if let Ok(signature) = &whole_signature
            && let Ok(plan) = kernel_plan_for_function(function, signature, self.validation)
        {
            let plan = match plan {
                Some(plan) => plan,
                None => match scan_plan(function, signature, self.validation).map_err(|detail| {
                    DeviceProgramBuildError::KernelAbi {
                        function: function.id,
                        detail,
                    }
                })? {
                    Some(plan) => plan,
                    None => lane_grid_plan(
                        function,
                        signature,
                        self.validation,
                        self.device_function_metadata
                            .get(&function.id)
                            .and_then(|metadata| metadata.lane_domain_local),
                    )
                    .map_err(|detail| DeviceProgramBuildError::KernelAbi {
                        function: function.id,
                        detail: format!(
                            "{detail}; {}",
                            lane_kernel_call_context(function, self.program, self.interner)
                        ),
                    })?
                    .or_else(|| single_lane_plan(function, self.validation))
                    .unwrap_or(CollectionKernelPlan::Elementwise),
                },
            };
            return self.emit_subchain(
                function,
                function,
                signature,
                plan,
                base_entry,
                None,
                &return_tuple_locals,
                &mut BTreeMap::new(),
                true,
            );
        }

        // Decomposition path (S5-U1/U5): split the body at recipe boundaries
        // into subchain kernels, each carrying exactly one recipe (or an
        // elementwise-only body), with the shared contract-shaped emission
        // signature for recipe subchains and the full ABI synthesis for
        // elementwise-only subchains.
        // A lane-form body is never decomposed: its tensors are indexed by the
        // lane coordinates, so the whole-function ABI refusal is the real
        // diagnostic and decomposition would only mask it.
        if function
            .params
            .iter()
            .any(|param| param.mode == MirParamMode::Lane)
            && let Err(error) = whole_signature
        {
            return Err(error);
        }
        let decomposition =
            decompose_kernel_function(function, self.validation).map_err(|error| {
                DeviceProgramBuildError::Decomposition {
                    function: function.id,
                    detail: error.message,
                }
            })?;
        // An empty body (no statements at all) decomposes to zero subchains:
        // it is trivially elementwise — no op exists to floor — and the
        // whole-function signature emits it as one Elementwise kernel. An
        // unplannable body never reaches here (the decomposition above
        // already failed with the typed plan diagnostic).
        if decomposition.subchains.is_empty() {
            return match whole_signature {
                Ok(signature) => self.emit_subchain(
                    function,
                    function,
                    &signature,
                    CollectionKernelPlan::Elementwise,
                    base_entry,
                    None,
                    &return_tuple_locals,
                    &mut BTreeMap::new(),
                    true,
                ),
                Err(error) => Err(error),
            };
        }
        let mut subchain_types = TypeTable::from_snapshot(self.validation.types.snapshot())
            .map_err(|error| DeviceProgramBuildError::SubchainTypes { detail: error })?;
        decomposition.intern_output_tuple_types(function, &mut subchain_types);
        let subchain_validation = self.validation.with_types(&subchain_types);

        // The decomposed path records each subchain output slot's buffer id
        // (subchain index, output-slot index) while the subchains are built,
        // so the D-6 fill can resolve the companion's FULL result-tuple field
        // producers after all subchains are emitted.
        let mut subchain_output_buffer_ids: BTreeMap<(usize, u32), BufferId> = BTreeMap::new();
        self.emit_decomposition_subchains(
            function,
            &decomposition,
            subchain_validation,
            base_entry,
            &return_tuple_locals,
            &mut subchain_output_buffer_ids,
        )?;

        self.record_subchain_gradient_buffers(
            function,
            &decomposition,
            &subchain_output_buffer_ids,
        );
        Ok(())
    }

    /// Emit one kernel per decomposition subchain (the S5-U1/U5 decomposition
    /// path): resolve each synthetic subchain's emission signature and hand it
    /// to the shared emission seam.
    fn emit_decomposition_subchains(
        &mut self,
        function: &MirFunction,
        decomposition: &crate::kernel_decomposition::KernelDecomposition,
        subchain_validation: crate::MirValidationContext<'_>,
        base_entry: String,
        return_tuple_locals: &[MirLocalId],
        subchain_output_buffer_ids: &mut BTreeMap<(usize, u32), BufferId>,
    ) -> Result<(), DeviceProgramBuildError> {
        for (subchain_index, subchain) in decomposition.subchains.iter().enumerate() {
            let synthetic = decomposition.subchain_function(
                function,
                subchain_index,
                subchain_validation.types.as_table(),
            );
            let contract =
                collection_op_contract(&synthetic, &subchain_validation).map_err(|error| {
                    DeviceProgramBuildError::KernelPlan {
                        function: function.id,
                        detail: error.message,
                    }
                })?;
            // The subchain that stores into the source's explicit output
            // parameter is the indexed-view kernel the source is: its output
            // IS that parameter, never a recipe result local.
            let explicit_signature = subchain
                .output_param
                .map(|output_param| {
                    explicit_output_subchain_signature(
                        &synthetic,
                        output_param,
                        &subchain_validation,
                        self.interner,
                    )
                })
                .transpose()
                .map_err(|error| DeviceProgramBuildError::KernelAbi {
                    function: function.id,
                    detail: error.message,
                })?;
            let signature = match (explicit_signature, &contract) {
                (Some(signature), _) => signature,
                (None, Some(contract)) => subchain_signature_for_emission_with_source(
                    &synthetic,
                    contract,
                    &subchain_validation,
                    &subchain.outputs,
                    Some(function),
                )
                .map_err(|error| DeviceProgramBuildError::KernelAbi {
                    function: function.id,
                    detail: error.message,
                })?,
                (None, None) => {
                    // A transformer-recipe subchain (`TensorSumAxis` /
                    // `TensorSoftmax` / `TensorLayerNorm` / the GI3-1
                    // inference recipes) has no `CollectionOpContract`
                    // variant — its signature dispatches through the P1 seam
                    // (the transformer-shaped recipe resources plus the
                    // subchain's data-flow inputs). Elementwise-only
                    // subchains take the full ABI synthesis on a literal-dim
                    // surface and the runtime-aware elementwise emission
                    // signature on a run-time-shaped one. The SOURCE
                    // function rides both seam calls so cross-kernel
                    // unification sees one shape per local.
                    if let Some(op) = transformer_recipe_op(&synthetic) {
                        transformer_subchain_signature_for_emission_with_source(
                            &synthetic,
                            op,
                            &subchain_validation,
                            &subchain.outputs,
                            Some(function),
                        )
                        .map_err(|error| {
                            DeviceProgramBuildError::KernelAbi {
                                function: function.id,
                                detail: error.message,
                            }
                        })?
                    } else {
                        // Elementwise-only subchain: a literal-dim surface
                        // takes the full-ABI synthesis. A run-time-shaped
                        // surface cannot (the ABI sizes every buffer with
                        // the literal-only device-view element count), so it
                        // takes the runtime-aware elementwise emission
                        // signature, which binds the
                        // `RUNTIME_SHAPED_ELEMENT_COUNT` marker instead.
                        // The SOURCE function rides both seam calls so
                        // cross-kernel unification sees one shape per local.
                        let mut signature = if subchain_has_runtime_shaped_buffer(
                            &synthetic,
                            &subchain.outputs,
                            &subchain_validation,
                        ) {
                            elementwise_subchain_signature_for_emission(
                                &synthetic,
                                &subchain_validation,
                                &subchain.outputs,
                            )
                            .map_err(|error| {
                                DeviceProgramBuildError::KernelAbi {
                                    function: function.id,
                                    detail: error.message,
                                }
                            })?
                        } else {
                            MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                                &synthetic,
                                &subchain_validation,
                                self.interner,
                            )
                            .map_err(|error| DeviceProgramBuildError::KernelAbi {
                                function: function.id,
                                detail: error.message,
                            })?
                        };
                        // A plain return carries no param identity, so the
                        // ABI leaves the output slot's source local unset —
                        // stamp the subchain's declared output local so the
                        // S5-U1 (function, local) join wires the join child's
                        // product to the consuming subchain's input.
                        if signature.output.source_local.is_none()
                            && let Some(&output_local) = subchain.outputs.first()
                        {
                            signature.output.source_local = Some(output_local);
                        }
                        signature
                    }
                }
            };
            let entry = if decomposition.subchains.len() == 1 {
                base_entry.clone()
            } else {
                format!("{base_entry}__{subchain_index}")
            };
            self.emit_subchain(
                function,
                &synthetic,
                &signature,
                subchain.plan.clone(),
                entry,
                Some(subchain_index),
                return_tuple_locals,
                subchain_output_buffer_ids,
                decomposition.subchains.len() == 1,
            )?;
        }
        Ok(())
    }

    /// D-6 gradient-slot → buffer mapping: the companion's gradient slots
    /// index the FULL result-tuple, which the LAST subchain's return-tuple
    /// Construct binds to locals. The `train_step` gradient reads alias the
    /// producing subchains' gradient buffers (the backward → `train_step`
    /// device-to-device edge).
    fn record_subchain_gradient_buffers(
        &mut self,
        function: &MirFunction,
        decomposition: &crate::kernel_decomposition::KernelDecomposition,
        subchain_output_buffer_ids: &BTreeMap<(usize, u32), BufferId>,
    ) {
        if let Some(source) = function.source
            && self.companion_gradient_slots.contains_key(&source.0)
            && let Some(fields) = decomposition.return_tuple_fields(function)
        {
            for (slot, field_local) in fields.iter().enumerate() {
                let slot = u32::try_from(slot).unwrap_or(u32::MAX);
                if !self.companion_gradient_slots[&source.0].contains(&slot) {
                    continue;
                }
                let Some((producer_index, output_position)) =
                    decomposition.producer_position(*field_local)
                else {
                    continue;
                };
                let output_position = u32::try_from(output_position).unwrap_or(u32::MAX);
                let Some(&buffer_id) =
                    subchain_output_buffer_ids.get(&(producer_index, output_position))
                else {
                    continue;
                };
                if let Some(entry) = self.buffers.iter_mut().find(|entry| entry.id == buffer_id) {
                    entry.gradient = true;
                }
                self.gradient_buffers.insert((source.0, slot), buffer_id);
            }
        }
    }

    /// Emit one kernel (a whole function or one decomposition subchain): the
    /// typed resource slots with buffer ids resolved by the unification pass
    /// (the S2-5 wiring, the param-update aliasing, and the gradient
    /// aliasing).
    #[allow(clippy::too_many_arguments)]
    fn emit_subchain(
        &mut self,
        kernel_source: &MirFunction,
        synthetic: &MirFunction,
        signature: &MirKernelSignature,
        plan: CollectionKernelPlan,
        entry: String,
        subchain_index: Option<usize>,
        return_tuple_locals: &[MirLocalId],
        subchain_output_buffer_ids: &mut BTreeMap<(usize, u32), BufferId>,
        whole_result: bool,
    ) -> Result<(), DeviceProgramBuildError> {
        let mut resources: Vec<ResourceBuild> = Vec::new();
        // The trainable param local → its registered buffer id, filled as the
        // signature's input slots are processed (inputs precede outputs in
        // the ABI), so the update output slots can alias it.
        let mut param_buffer_by_local: BTreeMap<MirLocalId, BufferId> = BTreeMap::new();
        for resource in signature
            .resources_by_binding()
            .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
        {
            let shape = crate::abi::kernel_param_shape(
                resource.source_local,
                kernel_source,
                self.validation,
            );
            let role =
                BufferRole::from_abi_role(resource.role, resource.access).ok_or_else(|| {
                    DeviceProgramBuildError::BufferRole {
                        function: kernel_source.id,
                        binding: resource.binding,
                        role: format!("{:?}", resource.role),
                        access: format!("{:?}", resource.access),
                    }
                })?;
            // A trainable parameter's read slot: the buffer is persistent
            // InOut state with HostProvided init — the program role is InOut
            // regardless of the slot's ABI role.
            let is_param_input = resource.role == MirKernelResourceRole::Input
                && resource.source_local.is_some_and(|local| {
                    self.param_updates.contains_key(&(kernel_source.id, local))
                        || self.persistent_locals.contains(&(kernel_source.id, local))
                });
            // The output-slot tuple index (the ABI's first output is element
            // 0, each extra output follows).
            let output_index = if resource.role == MirKernelResourceRole::Output {
                Some(output_slot_index(signature, resource))
            } else {
                None
            };
            // A whole-function kernel's (or sole subchain's) first output slot
            // is its declared result: its shape is the return type's literal dims (a fresh
            // output has no parameter to carry one). Only when the product
            // agrees with the slot's element count.
            let shape = shape.or_else(|| {
                // A whole-result output takes the source function's result
                // shape. A subchain output returns its own synthetic result,
                // so its shape is that synthetic return type's shape — the
                // dispatcher needs it to allocate the intermediate buffer.
                if resource.role != MirKernelResourceRole::Output {
                    return None;
                }
                let result_shape = if whole_result {
                    match output_index {
                        Some(index) => crate::abi::kernel_result_slot_shape(
                            kernel_source,
                            index,
                            self.validation,
                        ),
                        None => crate::abi::kernel_result_shape(kernel_source, self.validation),
                    }
                } else {
                    resource
                        .source_local
                        .and_then(|local| {
                            crate::abi::kernel_local_shape(synthetic, local, self.validation)
                        })
                        .or_else(|| crate::abi::kernel_result_shape(synthetic, self.validation))
                };
                result_shape.filter(|dims| {
                    dims.iter().try_fold(1u64, |acc, d| acc.checked_mul(*d))
                        == Some(resource.element_count)
                })
            });
            // An update output slot: the slot's tuple index decides which
            // parameter it updates. The alias applies ONLY when the output
            // slot IS the full result-tuple slot for that parameter's update
            // — an intermediate subchain's local output slot never carries a
            // full-tuple index and must never be written into the parameter
            // buffer.
            let alias_param = match output_index {
                Some(output_index) => {
                    let full_tuple_slot = resource.source_local.is_some_and(|local| {
                        return_tuple_locals.get(output_index as usize).copied() == Some(local)
                    });
                    if full_tuple_slot {
                        self.param_updates
                            .iter()
                            .find(|((_, _), update_output)| **update_output == output_index)
                            .and_then(|((_, param_local), _)| {
                                param_buffer_by_local.get(param_local).copied()
                            })
                    } else {
                        None
                    }
                }
                None => None,
            };
            // A gradient input slot of a train_step: the gradient buffer the
            // companion's result-tuple slot writes — the explicit backward →
            // train_step data-flow connection (the gradient identity rides
            // the companion relation, never a name).
            let gradient_alias = if resource.role == MirKernelResourceRole::Input {
                resource.source_local.and_then(|local| {
                    let position = kernel_source
                        .params
                        .iter()
                        .position(|param| param.local == local);
                    position.and_then(|position| {
                        self.gradient_links
                            .get(&(
                                kernel_source.id,
                                u32::try_from(position).unwrap_or(u32::MAX),
                            ))
                            .and_then(|(companion, slot)| {
                                self.gradient_buffers.get(&(*companion, *slot)).copied()
                            })
                    })
                })
            } else {
                None
            };
            let name = buffer_slot_name(kernel_source, self.interner, resource);
            // The slot's host-value identity key: a parameter slot by its
            // parameter position, a plain-return output slot by the call
            // destination. A decomposed kernel's intermediate slots have no
            // host call site and so no key.
            let host_slot = resource
                .source_local
                .and_then(|local| Slot::of_param_local(kernel_source, local))
                .or_else(|| {
                    (resource.role == MirKernelResourceRole::Output
                        && whole_result
                        && output_index == Some(0)
                        && signature.extra_outputs.is_empty())
                    .then(|| Slot::result(kernel_source.id))
                });
            let slot_facts = SlotFacts {
                element_ty: resource.element_ty,
                element_count: resource.element_count,
                shape: shape.as_deref(),
                reads: resource.access == MirKernelResourceAccess::Read,
                cross_kernel: false,
            };
            // The cross-kernel wiring: the same logical buffer appears at
            // this kernel too. The role guard ([`unify_roles`]) keeps a
            // name/shape coincidence from aliasing two unrelated values.
            let buffer_id = if let Some(alias) = alias_param {
                alias
            } else if let Some(alias) = gradient_alias {
                // The gradient input consumes the companion's gradient output
                // buffer — the buffer becomes an InOut per-step intermediate,
                // never an observation point.
                if let Some(entry) = self.buffers.iter_mut().find(|entry| entry.id == alias) {
                    // `merge_buffer_roles` with `InOut` is `InOut` for every
                    // previous role; assign the unified role directly.
                    entry.role = BufferRole::InOut;
                    entry.consumed = true;
                }
                alias
            } else if let Some(entry) = self.buffers.iter_mut().find(|entry| {
                // S5-U1 forward-save identity: when the slot carries a source
                // local, the (function, local) origin fact joins the
                // producing and consuming decomposition subchains even when
                // the role-based fallback names differ. The role guard keeps
                // the join honest exactly as it guards the name+shape wiring.
                let same_local_identity = resource.source_local.is_some_and(|local| {
                    matches!(
                        entry.origin,
                        SemanticValueOrigin::MirLocal {
                            function: origin_function,
                            local: origin_local,
                        } if origin_function == kernel_source.id && origin_local == local.0
                    )
                });
                same_local_identity && unify_roles(entry.role, role)
            }) {
                merge_slot_facts(entry, role, resource, shape.as_deref(), is_param_input);
                entry.id
            } else if let Some(entry) = self.buffers.iter_mut().find(|entry| {
                // Two joins reach here. The consumer/producer asymmetry is
                // shared: a consumer slot joins a buffer that already has a
                // writer; a producer slot joins only a buffer with NO writer
                // yet (the reverse-declared producer→consumer chain). Two
                // readers' slots never make a writer, two writers never share.
                let continuation = if resource.access == MirKernelResourceAccess::Read {
                    entry.written
                } else {
                    !entry.written && entry.consumed
                };
                // KNOWN VIOLATION of the value-identity rule (register V-01
                // training programs, V-02 device-resident companion
                // programs); not an exemption. A NAME join is legal only
                // inside one function (the same kernel's own slots) or for
                // those two program kinds.
                let by_name = (self.cross_kernel_name_join || entry.function == kernel_source.id)
                    && entry.matches(
                        &name,
                        &SlotFacts {
                            cross_kernel: entry.function != kernel_source.id,
                            ..slot_facts
                        },
                    )
                    && unify_roles(entry.role, role)
                    && (resource.access == MirKernelResourceAccess::Read || !entry.written);
                // The VALUE join (NAMEJOIN NJ2): the two slots hold the same
                // host value(s) at the host call sites, never the same
                // parameter name. Slots of one kernel never value-join (two
                // parameters fed one host value are that kernel's own).
                let by_value = continuation
                    && host_slot.is_some_and(|slot| {
                        entry.host_slot.is_some_and(|other| {
                            other.kernel != slot.kernel && self.wiring.same_host_values(slot, other)
                        })
                    })
                    && entry.matches_value(&SlotFacts {
                        cross_kernel: true,
                        ..slot_facts
                    })
                    && unify_roles(entry.role, role);
                by_name || by_value
            }) {
                merge_slot_facts(entry, role, resource, shape.as_deref(), is_param_input);
                entry.id
            } else {
                let id = BufferId(self.next_buffer_id);
                self.next_buffer_id = self.next_buffer_id.saturating_add(1);
                self.buffers.push(ProgramBuffer {
                    id,
                    function: kernel_source.id,
                    name,
                    element_ty: resource.element_ty,
                    element_count: resource.element_count,
                    shape: shape.clone(),
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
                    host_slot,
                });
                id
            };
            if is_param_input && let Some(local) = resource.source_local {
                param_buffer_by_local.insert(local, buffer_id);
            }
            // Record the companion's gradient output buffers as they are
            // minted (the train_step kernels built afterwards alias them);
            // a companion gradient slot is a per-step scratch value, never an
            // observation point. Decomposed companions mark their gradient
            // slots after ALL subchains from the decomposition's
            // return-tuple field bindings (D-6) — here only the produced
            // buffer id is recorded for that fill.
            if let (Some(source), Some(output_index)) = (kernel_source.source, output_index) {
                if subchain_index.is_none()
                    && self
                        .companion_gradient_slots
                        .get(&source.0)
                        .is_some_and(|slots| slots.contains(&output_index))
                {
                    if let Some(entry) = self.buffers.iter_mut().find(|entry| entry.id == buffer_id)
                    {
                        entry.gradient = true;
                    }
                    self.gradient_buffers
                        .insert((source.0, output_index), buffer_id);
                }
                if let Some(subchain_index) = subchain_index {
                    subchain_output_buffer_ids.insert((subchain_index, output_index), buffer_id);
                }
            }
            resources.push(ResourceBuild {
                group: resource.group,
                binding: resource.binding,
                access: resource.access,
                buffer_id,
                element_ty: resource.element_ty,
                element_count: resource.element_count,
                reduced_projection: resource.reduced_projection,
                shape,
            });
        }
        // The launch binds buffers in binding order (inputs first, output
        // last), matching the emitted kernel's buffer indices / param order.
        resources.sort_by_key(|resource| (resource.binding, resource.group));
        let launch =
            KernelLaunchPlan::from_signature_plan_and_function(signature, &plan, synthetic);
        self.builds.push(KernelBuild {
            function: kernel_source.id,
            entry,
            // This MIR→DeviceProgram path has no compiled library selection
            // carrier. Keep the no-library fact explicit rather than infer it
            // from the function name or body shape.
            library_operation: LibraryOperation::Generated,
            plan,
            launch,
            resources,
        });
        Ok(())
    }
}

/// The transformer recipe op of a subchain body, when it carries exactly one
/// (S6-P1 statement scan — a body mixing distinct transformer recipes fails
/// closed upstream at plan resolution).
fn transformer_recipe_op(function: &MirFunction) -> Option<crate::MirCollectionOp> {
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                continue;
            };
            let crate::MirIntrinsic::Collection(op) = call.intrinsic else {
                continue;
            };
            if is_transformer_recipe_op(op) {
                return Some(op);
            }
        }
    }
    None
}

/// The return-tuple element locals of a tuple-return function, in tuple order
/// (from the aggregate construction that produces the returned tuple).
fn tuple_return_locals(function: &MirFunction) -> Option<Vec<MirLocalId>> {
    let block = function.blocks.first()?;
    let crate::MirTerminatorKind::Return(Some(operand)) = &block.terminator.kind else {
        return None;
    };
    let returned = match operand {
        crate::MirOperand::Temp(temp) => crate::MirPlace::temp(*temp),
        crate::MirOperand::Place(place) if place.projections.is_empty() => place.clone(),
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
            .filter_map(|operand| match operand {
                crate::MirOperand::Place(place) if place.projections.is_empty() => {
                    match place.base {
                        crate::MirPlaceBase::Local(local) => Some(local),
                        crate::MirPlaceBase::Temp(_) => None,
                    }
                }
                _ => None,
            })
            .collect();
        if locals.is_empty() {
            return None;
        }
        return Some(locals);
    }
    None
}

/// The output-slot tuple index of a multi-output ABI output resource: the
/// first output (`signature.output`) is tuple element 0, each `extra_outputs`
/// element follows.
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

/// Whether a slot with program role `next` may join an existing buffer with
/// role `existing` under the cross-kernel wiring: the wiring is a data-flow
/// continuation, so two independent producers of the same named output never
/// unify.
fn unify_roles(existing: BufferRole, next: BufferRole) -> bool {
    matches!(
        (existing, next),
        (BufferRole::Input | BufferRole::Output, BufferRole::Input)
            | (BufferRole::InOut, _)
            | (_, BufferRole::InOut)
            | (BufferRole::Input, BufferRole::Output)
    )
}

/// Merge an ABI-derived role onto a buffer's program-level role under one id.
fn merge_buffer_roles(previous: BufferRole, next: BufferRole) -> BufferRole {
    match (previous, next) {
        (BufferRole::Input, BufferRole::Input) => BufferRole::Input,
        (BufferRole::Output, BufferRole::Output) => BufferRole::Output,
        _ => BufferRole::InOut,
    }
}

/// Merge a slot's facts onto an existing unified buffer under one id: the
/// role merge (a trainable-parameter slot forces `InOut`) and the independent
/// resource-state axes (`written` / `consumed` / `readwrite` / `param`).
/// Shared by the carried local-identity wiring (S5-U1) and the name+shape
/// wiring — the two joins must never drift apart.
fn merge_slot_facts(
    entry: &mut ProgramBuffer,
    role: BufferRole,
    resource: &MirKernelResource,
    shape: Option<&[u64]>,
    is_param_input: bool,
) {
    entry.role = if is_param_input {
        BufferRole::InOut
    } else {
        merge_buffer_roles(entry.role, role)
    };
    if entry.shape.is_none() {
        entry.shape = shape.map(<[u64]>::to_vec);
    }
    entry.written |= matches!(
        resource.access,
        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
    );
    entry.consumed |= resource.access == MirKernelResourceAccess::Read;
    entry.readwrite |= resource.access == MirKernelResourceAccess::ReadWrite;
    entry.param |= is_param_input;
}

/// The buffer name for a storage slot: the kernel source function's param /
/// local name when the slot names one, else the role-based fallback.
fn buffer_slot_name(
    function: &MirFunction,
    interner: &Interner,
    resource: &MirKernelResource,
) -> String {
    if let Some(local) = resource.source_local {
        if let Some(name) = function
            .params
            .iter()
            .find(|param| param.local == local)
            .and_then(|param| param.name)
            .map(|symbol| interner.resolve(symbol).to_owned())
        {
            return name;
        }
        if let Some(name) = function
            .locals
            .iter()
            .find(|local_info| local_info.id == local)
            .and_then(|local_info| local_info.name)
            .map(|symbol| interner.resolve(symbol).to_owned())
        {
            return name;
        }
    }
    match resource.role {
        MirKernelResourceRole::Input => format!("input_{}", resource.binding),
        MirKernelResourceRole::Output => format!("output_{}", resource.binding),
    }
}

/// The carried fact a buffer's semantic value identity is minted from (F1):
/// the MIR local the slot's value flows from when the slot names one (stable
/// under rename), a host-provided input for an anonymous read slot, or a
/// distinct synthetic label for an anonymous writer slot.
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

// ---------------------------------------------------------------------------
// Launch ordering (F3) + lifetime/results/semantics derivation
// ---------------------------------------------------------------------------

/// Order the launch sequence to follow the carried producer/consumer
/// dependency graph (F3) — never kernel declaration order. Producers always
/// precede their consumers; the launch ids are carried facts and do not
/// change, only their position in the execution sequence does.
///
/// A `RepeatingStep` program's trainable-parameter edges are EXCLUDED: the
/// parameter is persistent state — the step writes the value the NEXT step
/// reads — so the write→read edge within one step's sequence would form a
/// cycle.
///
/// A host loop's carried edges are EXCLUDED the same way: an
/// edge whose consumer the host calls only after the producer's NEXT
/// iteration (`fwd(w, mut y); upd(y, mut w)` — `fwd` reads the `w` that `upd`
/// wrote one iteration earlier) orders no launch within one iteration.
///
/// # Errors
/// Fail-closed when the dependency graph contains a cycle (a launch sequence
/// that follows it would never terminate).
fn dependency_ordered_launches(
    program: &DeviceProgram,
    excluded_buffers: &[BufferId],
    wiring: &HostWiring,
) -> Result<Vec<LaunchUnit>, DeviceProgramBuildError> {
    let edges = program
        .buffer_registry()
        .data_flow_pairs()
        .into_iter()
        .filter(|edge| !excluded_buffers.contains(&edge.buffer))
        .filter(|edge| !is_loop_carried_edge(program, wiring, edge))
        .collect::<Vec<_>>();
    let mut indegree: BTreeMap<LaunchId, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<LaunchId, Vec<LaunchId>> = BTreeMap::new();
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
        return Err(DeviceProgramBuildError::LaunchOrder {
            detail: "the carried producer/consumer dependency graph contains a cycle; the \
                     launch sequence cannot follow it"
                .to_owned(),
        });
    }
    let mut position: BTreeMap<LaunchId, usize> = BTreeMap::new();
    for (index, id) in ordered.iter().enumerate() {
        position.insert(*id, index);
    }
    let mut sorted = program.launches.clone();
    sorted.sort_by_key(|launch| position.get(&launch.id).copied().unwrap_or(usize::MAX));
    Ok(sorted)
}

/// Whether a data-flow edge is a host loop's carry: the host calls the
/// consumer's kernel only after the producer's kernel's next iteration (see
/// [`HostWiring::is_loop_carried`]).
fn is_loop_carried_edge(
    program: &DeviceProgram,
    wiring: &HostWiring,
    edge: &super::DataFlowPair,
) -> bool {
    let function_of = |launch: LaunchId| {
        program
            .launches
            .iter()
            .find(|unit| unit.id == launch)
            .and_then(|unit| program.kernels.get(unit.kernel_index))
            .map(|kernel| kernel.function)
    };
    match (function_of(edge.producer), function_of(edge.consumer)) {
        (Some(producer), Some(consumer)) => wiring.is_loop_carried(producer, consumer),
        _ => false,
    }
}

/// The independent allocation-lifetime axis (F5) of a unified buffer, decided
/// from its aggregate access facts and the trainable-parameter flag — never
/// from the role:
///
/// - a trainable parameter is persistent device-resident state (per-program);
/// - a buffer no kernel ever writes is host-provided persistent state
///   (per-program);
/// - an in-place `ReadWrite` slot is persistent writable state (an accumulator
///   / optimizer-state buffer, per-program);
/// - a kernel-written buffer another kernel consumes is a step-local
///   intermediate (per-step);
/// - a companion gradient slot is per-step scratch;
/// - a kernel-written final is read back at an observation point.
///
/// G2/G3 (U8 repair) + S5A-U1: a `RepeatingStep` training program's per-step
/// observation is ONLY the DECLARED loss buffer — every other
/// written-not-consumed final is a step-local final.
fn unified_lifetime(
    entry: &ProgramBuffer,
    training_program: bool,
    per_step_observation: Option<BufferId>,
) -> BufferLifetime {
    if entry.param {
        return BufferLifetime::PerProgram;
    }
    if entry.readwrite {
        return BufferLifetime::PerProgram;
    }
    if entry.gradient {
        return BufferLifetime::PerStep;
    }
    if !entry.written {
        BufferLifetime::PerProgram
    } else if entry.consumed {
        BufferLifetime::PerStep
    } else if Some(entry.id) == per_step_observation {
        BufferLifetime::ObservationPoint
    } else if training_program {
        BufferLifetime::PerStep
    } else {
        BufferLifetime::ObservationPoint
    }
}

/// The launch that produces a buffer's observed version: the LAST write in
/// execution order — the version the final readback observes.
fn producing_launch(program: &DeviceProgram, buffer: BufferId) -> Option<LaunchId> {
    let mut produced: Option<LaunchId> = None;
    for launch in &program.launches {
        let Some(kernel) = program.kernels.get(launch.kernel_index) else {
            continue;
        };
        if kernel.resources.iter().any(|resource| {
            resource.buffer.id == buffer
                && matches!(
                    resource.access,
                    MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                )
        }) {
            produced = Some(launch.id);
        }
    }
    produced
}

/// Declare the observation-eligible results of a materialized program with
/// their observation cadence (S5A-U1). The cadence is a DECLARED fact from
/// the training program's structure — never derived from buffer shapes or
/// names:
///
/// - a `RepeatingStep` training program declares the forward's return output
///   (the loss) `PerStep` and the final forward / final gradients / final
///   params `EndOfRun`;
/// - a `SingleRun` program declares every declared per-execution observation
///   point (an `ObservationPoint`-lifetime final) `PerStep`; per-step scratch
///   (a companion gradient slot) is never a result.
fn declared_result_rows(
    program: &DeviceProgram,
    buffers: &[ProgramBuffer],
    projection_by_buffer: &BTreeMap<BufferId, crate::abi::ReducedProjection>,
    shape_by_buffer: &BTreeMap<BufferId, Vec<u64>>,
    training_program: bool,
    per_step_observation: Option<BufferId>,
    train_step_gradients: &BTreeSet<BufferId>,
) -> Vec<ResultBuffer> {
    let mut rows = Vec::new();
    for buffer in buffers {
        let Some(produced_by) = producing_launch(program, buffer.id) else {
            continue;
        };
        let lifetime = unified_lifetime(buffer, training_program, per_step_observation);
        let cadence = if training_program {
            if Some(buffer.id) == per_step_observation {
                // The declared per-step observation — the loss.
                ObservationCadence::PerStep
            } else if buffer.param
                || train_step_gradients.contains(&buffer.id)
                || (buffer.role == BufferRole::Output && buffer.written && !buffer.consumed)
            {
                // The final forward / final gradients / final params.
                ObservationCadence::EndOfRun
            } else {
                continue;
            }
        } else if lifetime == BufferLifetime::ObservationPoint {
            ObservationCadence::PerStep
        } else {
            continue;
        };
        rows.push(ResultBuffer {
            buffer: BufferIdentity {
                id: buffer.id,
                name: buffer.name.clone(),
                role: buffer.role,
                storage: MirTensorStorageLayout::DeviceHandle,
                lifetime,
            },
            version: BufferVersion {
                version: 1,
                element_ty: buffer.element_ty,
                element_count: buffer.element_count,
                reduced_projection: projection_by_buffer.get(&buffer.id).copied(),
                shape: shape_by_buffer.get(&buffer.id).cloned(),
            },
            role: buffer.role,
            produced_by,
            cadence,
        });
    }
    rows
}

/// Mint the carried semantic facts (F1–F6) of a materialized program.
///
/// - values + bindings: one semantic value per unified buffer, minted from
///   the carried origin facts (F1);
/// - generations: every logical write in the ordered launch sequence is an
///   explicit new generation (F2);
/// - roots + dependencies: the carried graph the host schedules from (F3);
///   a trainable parameter's edges are persistent-state facts and a host loop's
///   carried edges are loop-carry facts — both excluded;
/// - relations: the lossless primal/companion rows from the carrier (F4);
/// - initializations: the independent axis facts (F5) — a trainable
///   parameter is `HostProvided` (copied in exactly once at session creation),
///   never zero-filled despite its `InOut` access.
fn semantic_facts(
    program: &DeviceProgram,
    buffers: &[ProgramBuffer],
    companions: &MirCompanionMap,
    param_buffer_ids: &[BufferId],
    wiring: &HostWiring,
) -> DeviceSemantics {
    let values = buffers
        .iter()
        .map(|buffer| SemanticValue {
            id: SemanticValueId(buffer.id.0),
            name: buffer.name.clone(),
            element_ty: buffer.element_ty,
            element_count: buffer.element_count,
            origin: buffer.origin.clone(),
        })
        .collect::<Vec<_>>();
    let bindings = buffers
        .iter()
        .map(|buffer| ValueBinding {
            value: SemanticValueId(buffer.id.0),
            buffer: buffer.id,
        })
        .collect::<Vec<_>>();

    // Generations in EXECUTION order: a later write is a new generation,
    // never another producer of the same one (F2).
    let mut generation_by_buffer: BTreeMap<BufferId, u32> = BTreeMap::new();
    let mut generations = Vec::new();
    for launch in &program.launches {
        let Some(kernel) = program.kernels.get(launch.kernel_index) else {
            continue;
        };
        for resource in &kernel.resources {
            if !matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            ) {
                continue;
            }
            let entry = generation_by_buffer.entry(resource.buffer.id).or_default();
            *entry = entry.saturating_add(1);
            generations.push(ValueGeneration {
                value: SemanticValueId(resource.buffer.id.0),
                generation: *entry,
                element_ty: resource.version.element_ty,
                element_count: resource.version.element_count,
                produced_by: launch.id,
            });
        }
    }

    // Persistent trainable-parameter state: the step's write feeds the NEXT
    // step's read — excluded from this step's dependency graph.
    let dependencies: Vec<DependencyEdge> = program
        .buffer_registry()
        .data_flow_pairs()
        .into_iter()
        .filter(|pair| !param_buffer_ids.contains(&pair.buffer))
        .filter(|pair| !is_loop_carried_edge(program, wiring, pair))
        .map(|pair| DependencyEdge {
            producer: pair.producer,
            consumer: pair.consumer,
            buffer: pair.buffer,
            version: pair.version,
        })
        .collect();
    let roots = program
        .launches
        .iter()
        .map(|launch| launch.id)
        .filter(|launch| !dependencies.iter().any(|edge| edge.consumer == *launch))
        .collect();
    let relations: Vec<LosslessMirCompanionEntry> = companions.iter().cloned().collect();
    let initializations = buffers
        .iter()
        .map(|buffer| {
            let policy = if buffer.param {
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

    DeviceSemantics {
        values,
        bindings,
        generations,
        roots,
        dependencies,
        relations,
        initializations,
        observations: Vec::new(),
    }
}

fn lane_kernel_call_context(
    function: &MirFunction,
    program: &crate::MirProgram,
    interner: &Interner,
) -> String {
    let kernel_name = function.name.map_or_else(
        || format!("function {}", function.id.0),
        |name| interner.resolve(name).to_owned(),
    );
    let targets_kernel = |callee: &MirCallee| match callee {
        MirCallee::Function(id)
        | MirCallee::Value(MirOperand::Constant(MirConstant::Function(id))) => *id == function.id,
        MirCallee::Definition { source, .. } => function.source == Some(*source),
        MirCallee::Closure(closure) => closure.function == function.id,
        MirCallee::Value(_) => false,
    };
    let mut call_sites = Vec::new();
    for caller in &program.functions {
        let caller_name = caller.name.map_or_else(
            || format!("function {}", caller.id.0),
            |name| interner.resolve(name).to_owned(),
        );
        for block in &caller.blocks {
            for statement in &block.statements {
                let MirStatementKind::Call { callee, .. } = &statement.kind else {
                    continue;
                };
                if targets_kernel(callee) {
                    call_sites.push(format!(
                        "`{kernel_name}` called from `{caller_name}` at bytes {}..{}",
                        statement.span.start, statement.span.end
                    ));
                }
            }
            if let crate::MirTerminatorKind::TryCall { callee, .. } = &block.terminator.kind
                && targets_kernel(callee)
            {
                call_sites.push(format!(
                    "`{kernel_name}` called from `{caller_name}` at bytes {}..{}",
                    block.terminator.span.start, block.terminator.span.end
                ));
            }
        }
    }
    if call_sites.is_empty() {
        format!("kernel `{kernel_name}`")
    } else {
        call_sites.join("; ")
    }
}

/// The lane-form plan of a function (gpu-reset C4-1), selected by structure:
/// a [`MirParamMode::Lane`] parameter plus a written tensor resource. Its
/// domain is the exact source local carried by lane-form lowering, resolved
/// through single-assignment parameter aliases to the matching ABI resource.
/// `Ok(None)` for a function with no Lane parameter; a Lane function with no
/// carried source, no written resource, or an unranked source fails closed.
fn lane_grid_plan(
    function: &MirFunction,
    signature: &MirKernelSignature,
    validation: &MirValidationContext<'_>,
    lane_domain_local: Option<MirLocalId>,
) -> Result<Option<CollectionKernelPlan>, String> {
    if !function
        .params
        .iter()
        .any(|param| param.mode == MirParamMode::Lane)
    {
        return Ok(None);
    }
    let resources: Vec<_> = signature
        .resources_by_binding()
        .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
        .collect();
    if !resources
        .iter()
        .any(|resource| resource.access != MirKernelResourceAccess::Read)
    {
        return Err("lane kernel has no written tensor resource".to_owned());
    }
    // C4-8: a whole-grid reduction region names its own source resource; it
    // is never "the first read buffer".
    if let Some((filum, source_local)) = crate::abi::grid_reduce_node(function) {
        let source_resource = resources
            .iter()
            .position(|resource| resource.source_local == Some(source_local))
            .ok_or_else(|| "grid reduce source tensor has no storage-buffer resource".to_owned())?;
        let rank = u8::try_from(filum.coordinates.len())
            .ok()
            .filter(|rank| *rank > 0)
            .ok_or_else(|| "grid reduce source tensor has no static rank".to_owned())?;
        let partial_layout = crate::abi::grid_reduce_partial_layout(filum, validation)
            .map_err(|error| error.message)?;
        return Ok(Some(CollectionKernelPlan::GridReduce {
            source_resource,
            rank,
            group_shape: filum.group.as_ref().map(|group| group.dimensions.clone()),
            workgroup: crate::kernel_plan::GRID_REDUCE_WORKGROUP,
            partials_cap: crate::kernel_plan::GRID_REDUCE_PARTIALS_CAP,
            partial_layout,
        }));
    }
    let lane_domain_local = lane_domain_local
        .map(|local| crate::abi::ParameterAliases::new(function).resolve(local))
        .ok_or_else(|| "lane kernel has no carried lane-domain source identity".to_owned())?;
    let Some((source_resource, source)) = resources
        .iter()
        .enumerate()
        .find(|(_, resource)| resource.source_local == Some(lane_domain_local))
    else {
        return Err("lane-domain source has no storage-buffer resource".to_owned());
    };
    let rank = source
        .source_local
        .and_then(|local| function.params.iter().find(|param| param.local == local))
        .and_then(|param| tensor_param_rank(param.ty, validation))
        .and_then(|rank| u8::try_from(rank).ok())
        .filter(|rank| *rank > 0)
        .ok_or_else(|| "lane kernel source tensor has no static rank".to_owned())?;
    Ok(Some(CollectionKernelPlan::LaneGrid {
        source_resource,
        rank,
    }))
}

/// The single-lane plan of a function (gpu-reset A2 rung 3): a void compute
/// function with no lane loop, no recipe (the caller reaches this only when
/// the plan pass found none) and one `mut` tensor, which is sequential and so
/// runs as exactly one lane.
fn single_lane_plan(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Option<CollectionKernelPlan> {
    MirKernelSignature::is_single_lane_form(function, validation)
        .then_some(CollectionKernelPlan::SingleLane)
}

/// The axis count of a tensor parameter's declared shape, whatever its
/// dimensions are: a runtime-extent axis (`_`, a `size` generic) still counts,
/// which `kernel_param_shape` (literal dimensions only) cannot answer.
fn tensor_param_rank(ty: MirType, validation: &MirValidationContext<'_>) -> Option<usize> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(_, shape) => match validation.types.get_index(*shape) {
            IndexExpr::Literal(_) => Some(1),
            IndexExpr::Tuple(dims) => Some(dims.len()),
            _ => None,
        },
        Type::Alias(_, target) => tensor_param_rank(MirType::semantic(*target), validation),
        _ => None,
    }
}
