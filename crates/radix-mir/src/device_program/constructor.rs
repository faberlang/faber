//! The single typed MIR → [`DeviceProgram`] materializer (DPSC-U2).
//!
//! This module owns the construction spine.  Consumers may build views over
//! the returned program, but they do not rebuild its kernels, buffers,
//! launches, or semantic facts.

use super::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramError, DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit,
    LaunchId, LaunchUnit, ObservationCadence, ResultBuffer,
};
use crate::abi::{MirKernelResourceAccess, MirKernelResourceKind, MirKernelSignature};
use crate::device::MirCompanionMap;
use crate::device_program_plans::kernel_plan_for_function;
use crate::device_semantics::{
    DependencyEdge, DeviceSemantics, InitializationFact, InitializationPolicy,
    LosslessMirCompanionEntry, ObservationFact, SemanticValue, SemanticValueId,
    SemanticValueOrigin, ValueBinding, ValueGeneration,
};
use crate::kernel_plan::CollectionKernelPlan;
use crate::layout::MirTensorStorageLayout;
use crate::static_shape_fold::{fold_validated_device_program, ShapeFoldError};
use crate::{MirFunction, MirFunctionId, MirLocalId, ValidatedMir};
use radix_lexer::Interner;
use std::collections::{BTreeMap, BTreeSet};
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
    element_ty: crate::MirType,
    reduced_projection: Option<crate::abi::ReducedProjection>,
}

/// The aggregate identity and independent resource-axis facts for one buffer.
#[derive(Debug, Clone)]
struct ProgramBuffer {
    id: BufferId,
    name: String,
    element_ty: crate::MirType,
    element_count: u64,
    role: BufferRole,
    written: bool,
    consumed: bool,
    readwrite: bool,
    persistent: bool,
    origin: SemanticValueOrigin,
}

impl ProgramBuffer {
    fn matches(&self, name: &str, element_ty: crate::MirType, element_count: u64) -> bool {
        self.name == name && self.element_ty == element_ty && self.element_count == element_count
    }
}

/// Build the one target-neutral [`DeviceProgram`] and its carried semantics.
///
/// `interner` is explicit because kernel entry identity is a typed MIR fact
/// that must be preserved at the public boundary.  `companions` carries the
/// lossless primal/companion relation.  `training` optionally contributes
/// additional device-resident MIR functions and persistent-buffer origins;
/// the lifetime argument remains the caller's explicit program-lifetime fact.
/// The shared static-shape fold is applied once to the complete selected
/// device set before any ABI or plan derivation.
///
/// # Errors
/// Returns a typed error when folding, ABI/plan derivation, companion lookup,
/// buffer identity construction, or the shared program-plus-semantics schema
/// rejects the result.
pub fn build_device_program(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    companions: &MirCompanionMap,
    training: Option<&DeviceTrainingFacts>,
    lifetime: DeviceProgramLifetime,
) -> Result<(DeviceProgram, DeviceSemantics), DeviceProgramBuildError> {
    let program = validated.program();
    let mut selected_ids: BTreeSet<MirFunctionId> = program
        .functions
        .iter()
        .filter(|function| function.shader_stage == Some(crate::MirKernelShaderStage::Compute))
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
    for id in companion_ids {
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

    // A validated MIR with no device body has no payload.  Returning the empty
    // pair keeps the constructor total and lets host-partition remain a view.
    if ordered_ids.is_empty() {
        return Ok((DeviceProgram::new(lifetime), DeviceSemantics::default()));
    }

    let mut buffers = Vec::new();
    let mut next_buffer_id = 1u32;
    let mut builds = Vec::with_capacity(ordered_ids.len());

    for function_id in ordered_ids {
        let function = folded_program
            .function_by_id(function_id)
            .expect("selected function ids came from the validated MIR");
        let signature = MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            function,
            folded.validation(),
            interner,
        )
        .map_err(|error| DeviceProgramBuildError::KernelAbi {
            function: function.id,
            detail: error.message.clone(),
        })?;
        let plan = kernel_plan_for_function(function, &signature, folded.validation())
            .map_err(|error| DeviceProgramBuildError::KernelPlan {
                function: function.id,
                detail: error.message.clone(),
            })?
            .unwrap_or(CollectionKernelPlan::Elementwise);
        let mut resources = Vec::new();

        for resource in signature
            .resources()
            .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
        {
            let role =
                BufferRole::from_abi_role(resource.role, resource.access).ok_or_else(|| {
                    DeviceProgramBuildError::BufferRole {
                        function: function.id,
                        binding: resource.binding,
                        role: format!("{:?}", resource.role),
                        access: format!("{:?}", resource.access),
                    }
                })?;
            let name = buffer_slot_name(function, interner, resource);
            let persistent = training.is_some_and(|training| {
                resource.source_local.is_some_and(|local| {
                    training.persistent_buffers.contains(&(function.id, local))
                })
            });
            let buffer_id = find_or_insert_buffer(
                &mut buffers,
                &mut next_buffer_id,
                function,
                resource,
                name,
                role,
                persistent,
            );
            resources.push(ResourceBuild {
                group: resource.group,
                binding: resource.binding,
                access: resource.access,
                buffer_id,
                element_ty: resource.element_ty,
                reduced_projection: resource.reduced_projection,
            });
        }

        builds.push((
            function.id,
            kernel_entry_name(function, interner),
            plan,
            KernelLaunchPlan::from_signature_and_function(&signature, function),
            resources,
        ));
    }

    let mut projection_by_buffer: BTreeMap<BufferId, Option<crate::abi::ReducedProjection>> =
        BTreeMap::new();
    for (_, _, _, _, resources) in &builds {
        for resource in resources {
            let Some(projection) = resource.reduced_projection else {
                continue;
            };
            if let Some(previous) = projection_by_buffer.get(&resource.buffer_id) {
                if *previous != Some(projection) {
                    return Err(DeviceProgramBuildError::ReducedProjection {
                        buffer: resource.buffer_id,
                        first: format!("{previous:?}"),
                        second: format!("{projection:?}"),
                    });
                }
            } else {
                projection_by_buffer.insert(resource.buffer_id, Some(projection));
            }
        }
    }

    let mut device_program = DeviceProgram::new(lifetime);
    for (function, entry, plan, launch, resources) in builds {
        let kernel_index = device_program.kernels.len();
        let device_resources = resources
            .iter()
            .map(|resource| {
                let buffer = buffers
                    .iter()
                    .find(|buffer| buffer.id == resource.buffer_id)
                    .expect("every resource buffer was registered");
                let identity = BufferIdentity {
                    id: buffer.id,
                    name: buffer.name.clone(),
                    role: buffer.role,
                    storage: MirTensorStorageLayout::DeviceHandle,
                    lifetime: buffer_lifetime(buffer, lifetime),
                };
                let version = BufferVersion {
                    version: 1,
                    element_ty: resource.element_ty,
                    element_count: buffer.element_count,
                    reduced_projection: projection_by_buffer
                        .get(&resource.buffer_id)
                        .copied()
                        .flatten()
                        .or(resource.reduced_projection),
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
            function,
            entry,
            plan,
            resources: device_resources,
            launch,
        });
        device_program.launches.push(LaunchUnit {
            id: LaunchId(u32::try_from(kernel_index + 1).unwrap_or(u32::MAX)),
            kernel_index,
        });
    }

    let dependencies = derive_dependencies(&device_program);
    let mut semantics = derive_semantics(&device_program, &buffers, companions, &dependencies);
    device_program.results = derive_results(&device_program, &buffers, lifetime);
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
    Ok((device_program, semantics))
}

/// Training-side facts intentionally stay MIR-owned.  Faber's package layer
/// can translate its package analysis into this small carrier without making
/// the compiler crate depend on product-private training types.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceTrainingFacts {
    /// Additional validated MIR functions selected by training analysis.
    pub device_function_ids: Vec<MirFunctionId>,
    /// `(function, local)` origins that persist across repeating steps.
    pub persistent_buffers: BTreeSet<(MirFunctionId, MirLocalId)>,
}

fn find_or_insert_buffer(
    buffers: &mut Vec<ProgramBuffer>,
    next_buffer_id: &mut u32,
    function: &MirFunction,
    resource: &crate::abi::MirKernelResource,
    name: String,
    role: BufferRole,
    persistent: bool,
) -> BufferId {
    // A read continues an existing logical buffer.  A second independent
    // writer never aliases merely because its fallback name and shape match.
    if let Some(entry) = buffers.iter_mut().find(|entry| {
        entry.matches(&name, resource.element_ty, resource.element_count)
            && compatible_roles(entry.role, role)
            && (resource.access == MirKernelResourceAccess::Read || !entry.written)
    }) {
        merge_buffer_facts(entry, role, resource, persistent);
        return entry.id;
    }

    let id = BufferId(*next_buffer_id);
    *next_buffer_id = next_buffer_id.saturating_add(1);
    let origin = resource.source_local.map_or_else(
        || {
            if resource.role == crate::abi::MirKernelResourceRole::Input {
                SemanticValueOrigin::HostInput
            } else {
                SemanticValueOrigin::Synthetic {
                    label: format!("buffer-{}", id.0),
                }
            }
        },
        |local| SemanticValueOrigin::MirLocal {
            function: function.id,
            local: local.0,
        },
    );
    buffers.push(ProgramBuffer {
        id,
        name,
        element_ty: resource.element_ty,
        element_count: resource.element_count,
        role: if persistent { BufferRole::InOut } else { role },
        written: matches!(
            resource.access,
            MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
        ),
        consumed: resource.access == MirKernelResourceAccess::Read,
        readwrite: resource.access == MirKernelResourceAccess::ReadWrite,
        persistent,
        origin,
    });
    id
}

fn compatible_roles(existing: BufferRole, next: BufferRole) -> bool {
    matches!(
        (existing, next),
        (BufferRole::Input | BufferRole::Output, BufferRole::Input)
            | (BufferRole::InOut, _)
            | (_, BufferRole::InOut)
            | (BufferRole::Input, BufferRole::Output)
    )
}

fn merge_buffer_facts(
    entry: &mut ProgramBuffer,
    role: BufferRole,
    resource: &crate::abi::MirKernelResource,
    persistent: bool,
) {
    entry.role = if persistent {
        BufferRole::InOut
    } else {
        match (entry.role, role) {
            (BufferRole::Input, BufferRole::Input) => BufferRole::Input,
            (BufferRole::Output, BufferRole::Output) => BufferRole::Output,
            _ => BufferRole::InOut,
        }
    };
    entry.written |= matches!(
        resource.access,
        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
    );
    entry.consumed |= resource.access == MirKernelResourceAccess::Read;
    entry.readwrite |= resource.access == MirKernelResourceAccess::ReadWrite;
    entry.persistent |= persistent;
}

fn buffer_slot_name(
    function: &MirFunction,
    interner: &Interner,
    resource: &crate::abi::MirKernelResource,
) -> String {
    if let Some(name) = &resource.source_name {
        return name.clone();
    }
    if let Some(local) = resource.source_local {
        if let Some(name) = function
            .params
            .iter()
            .find(|param| param.local == local)
            .and_then(|param| param.name)
        {
            return interner.resolve(name).to_owned();
        }
        if let Some(name) = function
            .locals
            .iter()
            .find(|local_info| local_info.id == local)
            .and_then(|local_info| local_info.name)
        {
            return interner.resolve(name).to_owned();
        }
    }
    match resource.role {
        crate::abi::MirKernelResourceRole::Input => format!("input_{}", resource.binding),
        crate::abi::MirKernelResourceRole::Output => format!("output_{}", resource.binding),
    }
}

fn kernel_entry_name(function: &MirFunction, interner: &Interner) -> String {
    crate::host_partition::kernel_entry_name(function, interner)
}

fn buffer_lifetime(buffer: &ProgramBuffer, lifetime: DeviceProgramLifetime) -> BufferLifetime {
    if buffer.persistent || !buffer.written || buffer.readwrite {
        return BufferLifetime::PerProgram;
    }
    if buffer.consumed {
        return BufferLifetime::PerStep;
    }
    match lifetime {
        DeviceProgramLifetime::SingleRun => BufferLifetime::PerProgram,
        DeviceProgramLifetime::RepeatingStep => BufferLifetime::ObservationPoint,
    }
}

fn derive_dependencies(program: &DeviceProgram) -> Vec<DependencyEdge> {
    let mut latest_writer: BTreeMap<BufferId, LaunchId> = BTreeMap::new();
    let mut dependencies = Vec::new();
    for launch in &program.launches {
        let Some(kernel) = program.kernels.get(launch.kernel_index) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        for resource in &kernel.resources {
            let reading = matches!(
                resource.access,
                MirKernelResourceAccess::Read | MirKernelResourceAccess::ReadWrite
            );
            if reading {
                if let Some(producer) = latest_writer.get(&resource.buffer.id).copied() {
                    if seen.insert((producer, resource.buffer.id)) {
                        dependencies.push(DependencyEdge {
                            producer,
                            consumer: launch.id,
                            buffer: resource.buffer.id,
                            version: resource.version.version,
                        });
                    }
                }
            }
            if matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            ) {
                latest_writer.insert(resource.buffer.id, launch.id);
            }
        }
    }
    dependencies
}

fn derive_semantics(
    program: &DeviceProgram,
    buffers: &[ProgramBuffer],
    companions: &MirCompanionMap,
    dependencies: &[DependencyEdge],
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

    let mut generation_by_buffer: BTreeMap<BufferId, u32> = BTreeMap::new();
    let mut generations = Vec::new();
    let mut initializations = Vec::new();
    for launch in &program.launches {
        let Some(kernel) = program.kernels.get(launch.kernel_index) else {
            continue;
        };
        for resource in &kernel.resources {
            let entry = generation_by_buffer.entry(resource.buffer.id).or_default();
            if !initializations
                .iter()
                .any(|fact: &InitializationFact| fact.buffer == resource.buffer.id)
            {
                let policy = match resource.access {
                    MirKernelResourceAccess::Read => InitializationPolicy::HostProvided,
                    MirKernelResourceAccess::Write => InitializationPolicy::KernelInitialized,
                    MirKernelResourceAccess::ReadWrite => InitializationPolicy::ZeroFill,
                };
                initializations.push(InitializationFact {
                    buffer: resource.buffer.id,
                    policy,
                });
            }
            if matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            ) {
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
    }
    let launches = program
        .launches
        .iter()
        .map(|launch| launch.id)
        .collect::<Vec<_>>();
    let roots = launches
        .iter()
        .copied()
        .filter(|launch| !dependencies.iter().any(|edge| edge.consumer == *launch))
        .collect();
    let relations: Vec<LosslessMirCompanionEntry> = companions.iter().cloned().collect();

    DeviceSemantics {
        values,
        bindings,
        generations,
        roots,
        dependencies: dependencies.to_vec(),
        relations,
        initializations,
        observations: Vec::new(),
    }
}

fn derive_results(
    program: &DeviceProgram,
    buffers: &[ProgramBuffer],
    lifetime: DeviceProgramLifetime,
) -> Vec<ResultBuffer> {
    let mut results = Vec::new();
    for buffer in buffers {
        if !buffer.written || buffer.consumed || buffer.role == BufferRole::Input {
            continue;
        }
        let Some((launch, resource)) = program
            .launches
            .iter()
            .filter_map(|launch| {
                let kernel = program.kernels.get(launch.kernel_index)?;
                kernel
                    .resources
                    .iter()
                    .find(|resource| {
                        resource.buffer.id == buffer.id
                            && matches!(
                                resource.access,
                                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                            )
                    })
                    .map(|resource| (launch.id, resource))
            })
            .next_back()
        else {
            continue;
        };
        let identity = BufferIdentity {
            id: buffer.id,
            name: buffer.name.clone(),
            role: buffer.role,
            storage: MirTensorStorageLayout::DeviceHandle,
            lifetime: buffer_lifetime(buffer, lifetime),
        };
        let cadence = match lifetime {
            DeviceProgramLifetime::SingleRun => ObservationCadence::EndOfRun,
            DeviceProgramLifetime::RepeatingStep => ObservationCadence::PerStep,
        };
        results.push(ResultBuffer {
            buffer: identity,
            version: resource.version.clone(),
            role: buffer.role,
            produced_by: launch,
            cadence,
        });
    }
    results
}
