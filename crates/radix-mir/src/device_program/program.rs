use super::error::DeviceProgramError;
use super::helpers::{
    ShapeRecord, record_identity, record_shape, registry_buffer_mut, registry_version_mut,
    result_is_produced, role_permits_access,
};
use super::registry::{BufferRegistry, RegistryBuffer};
use super::types::{
    Binding, BufferIdentity, BufferLifetime, BufferRole, DeviceProgram, DeviceProgramLifetime,
    KernelUnit, LaunchId, ObservationCadence,
};
use crate::abi::MirKernelResourceAccess;
use crate::device_semantics::DeviceSemantics;

impl DeviceProgram {
    /// A new, empty device program with the given lifetime regime.
    #[must_use]
    pub fn new(lifetime: DeviceProgramLifetime) -> Self {
        Self {
            kernels: Vec::new(),
            launches: Vec::new(),
            lifetime,
            results: Vec::new(),
        }
    }

    /// Derive the program's buffer registry: every distinct [`BufferId`] with
    /// its identity facts, version chain, and producer/consumer launch edges.
    ///
    /// The registry is a view over the program — it is not stored in the
    /// program, so the frozen four-field shape stays intact. Callers should
    /// [`validate`](DeviceProgram::validate) first so launch references are in
    /// range; an unvalidated program still derives (out-of-range launches are
    /// skipped).
    #[must_use]
    pub fn buffer_registry(&self) -> BufferRegistry {
        let mut buffers: Vec<RegistryBuffer> = Vec::new();

        for launch in &self.launches {
            let Some(kernel) = self.kernels.get(launch.kernel_index) else {
                continue;
            };
            for resource in &kernel.resources {
                let entry = registry_buffer_mut(&mut buffers, &resource.buffer);
                let version = registry_version_mut(&mut entry.versions, &resource.version);
                match resource.access {
                    MirKernelResourceAccess::Read => version.consumed_by.push(launch.id),
                    MirKernelResourceAccess::Write => version.produced_by.push(launch.id),
                    MirKernelResourceAccess::ReadWrite => {
                        version.consumed_by.push(launch.id);
                        version.produced_by.push(launch.id);
                    }
                }
            }
        }

        // Results contribute the observed versions to the chain.
        for result in &self.results {
            let entry = registry_buffer_mut(&mut buffers, &result.buffer);
            registry_version_mut(&mut entry.versions, &result.version);
        }

        BufferRegistry { buffers }
    }

    /// Validate the device program's consistency rules.
    ///
    /// Checks, in order:
    /// 1. the program declares at least one kernel and one launch;
    /// 2. per kernel: a non-empty entry name, a valid
    ///    [`CollectionKernelPlan`], unique (group, binding) slots, version
    ///    numbers ≥ 1, and role/access consistency (inputs are read-only;
    ///    outputs are written; in-out buffers may be read or written);
    /// 3. the launch sequence: every kernel reference is in range and launch
    ///    ids are unique;
    /// 4. results: version ≥ 1, role output or in-out, the observation role
    ///    agrees with the buffer's identity role, and the producing
    ///    launch exists;
    /// 5. cross-reference consistency: every reference to the same
    ///    [`BufferId`] carries the same identity facts (name, role, storage,
    ///    lifetime), and every reference to the same `(BufferId, version)`
    ///    carries the same shape — a shape change must be a **new version**
    ///    (no in-place reinterpretation);
    /// 6. every result is actually produced by its named launch.
    ///
    /// # Errors
    /// Returns the first [`DeviceProgramError`] the program violates.
    pub fn validate(&self) -> Result<(), DeviceProgramError> {
        if self.kernels.is_empty() {
            return Err(DeviceProgramError::EmptyKernels);
        }
        if self.launches.is_empty() {
            return Err(DeviceProgramError::EmptyLaunches);
        }

        validate_kernel_resources(&self.kernels)?;

        let mut seen_launches: Vec<LaunchId> = Vec::new();
        for launch in &self.launches {
            if launch.kernel_index >= self.kernels.len() {
                return Err(DeviceProgramError::LaunchOutOfRange {
                    launch: launch.id,
                    kernel_index: launch.kernel_index,
                    kernel_count: self.kernels.len(),
                });
            }
            if seen_launches.contains(&launch.id) {
                return Err(DeviceProgramError::RepeatedLaunchId { launch: launch.id });
            }
            seen_launches.push(launch.id);
        }

        for (result_index, result) in self.results.iter().enumerate() {
            if result.version.version == 0 {
                return Err(DeviceProgramError::ZeroVersion {
                    buffer: result.buffer.id,
                    version: result.version.version,
                });
            }
            if !matches!(result.role, BufferRole::Output | BufferRole::InOut) {
                return Err(DeviceProgramError::ResultRole {
                    result: result_index,
                    role: result.role,
                });
            }
            // The observation role must agree with the buffer's identity role:
            // a result declaring `InOut` on an `Output` buffer (or vice versa)
            // is a contradictory schema fact — both roles are individually
            // valid, so only the cross-check catches it.
            if result.role != result.buffer.role {
                return Err(DeviceProgramError::ResultBufferRoleMismatch {
                    result: result_index,
                    role: result.role,
                    buffer_role: result.buffer.role,
                });
            }
            // The observation cadence must be consistent with the buffer's
            // lifetime class (S5A-U1): a PerStep readback is valid only for an
            // ObservationPoint buffer (read back every step, read-then-release);
            // an EndOfRun readback is valid only for PerStep or PerProgram
            // buffers (kept live through the final step — never a read-only
            // host input and never per-step state read both ways).
            match result.cadence {
                ObservationCadence::PerStep
                    if result.buffer.lifetime != BufferLifetime::ObservationPoint =>
                {
                    return Err(DeviceProgramError::ResultCadenceMismatch {
                        result: result_index,
                        buffer: result.buffer.id,
                        cadence: result.cadence,
                        lifetime: result.buffer.lifetime,
                    });
                }
                ObservationCadence::EndOfRun
                    if !matches!(
                        result.buffer.lifetime,
                        BufferLifetime::PerStep | BufferLifetime::PerProgram
                    ) =>
                {
                    return Err(DeviceProgramError::ResultCadenceMismatch {
                        result: result_index,
                        buffer: result.buffer.id,
                        cadence: result.cadence,
                        lifetime: result.buffer.lifetime,
                    });
                }
                _ => {}
            }
            if !self
                .launches
                .iter()
                .any(|launch| launch.id == result.produced_by)
            {
                return Err(DeviceProgramError::AbsentProducingLaunch {
                    result: result_index,
                    produced_by: result.produced_by,
                });
            }
        }

        validate_recorded_identities(self)?;

        Ok(())
    }

    /// Validate the frozen program together with its carried semantic facts
    /// (Stage 3R F1–F7).
    ///
    /// Runs the program rules ([`DeviceProgram::validate`]) and the semantic
    /// rules ([`DeviceSemantics::validate`]), then the **F6 coupling** that
    /// makes results declare observation points only:
    ///
    /// - every [`ResultBuffer`] must be backed by an explicit
    ///   [`ObservationFact`](crate::device_semantics::ObservationFact) for the
    ///   same buffer, content version, and producing launch — a writable
    ///   intermediate exposed as a result without being declared observed is
    ///   rejected (`ResultWithoutObservationFact`);
    /// - every observation fact must be backed by a declared result row —
    ///   a readback the host would never be told to perform is rejected
    ///   (`UndeclaredObservationFact`).
    ///
    /// The axes stay **independent** (F5): initialization, lifetime, access,
    /// observation, and generation each ride their own carried facts; this
    /// cross-check is the one declared coupling — that a result IS an
    /// explicit observation point (F6).
    ///
    /// Finally the compiler derives the **prepared submission regions**
    /// ([`DeviceSemantics::derive_prepared_regions`]) from the carried
    /// dependency graph and validates the partition fail-closed — every
    /// launch in exactly one region, no region crossing an observation
    /// boundary, every region path-connected (contract §Prepared submission
    /// regions rules 1–4; NGAB0-R1: one host call → one prepared submission
    /// region).
    ///
    /// # Errors
    /// Returns the first [`DeviceProgramError`] the program or its carried
    /// facts violate.
    pub fn validate_with_semantics(
        &self,
        semantics: &DeviceSemantics,
    ) -> Result<(), DeviceProgramError> {
        self.validate()?;
        let launches: Vec<LaunchId> = self.launches.iter().map(|launch| launch.id).collect();
        semantics.validate(&launches)?;
        for (result_index, result) in self.results.iter().enumerate() {
            let declared = semantics.observations.iter().any(|observation| {
                observation.buffer == result.buffer.id
                    && observation.version == result.version.version
                    && observation.at_launch == result.produced_by
            });
            if !declared {
                return Err(DeviceProgramError::ResultWithoutObservationFact {
                    result: result_index,
                    buffer: result.buffer.id,
                    version: result.version.version,
                });
            }
        }
        for observation in &semantics.observations {
            let backed = self.results.iter().any(|result| {
                result.buffer.id == observation.buffer
                    && result.version.version == observation.version
                    && result.produced_by == observation.at_launch
            });
            if !backed {
                return Err(DeviceProgramError::UndeclaredObservationFact {
                    buffer: observation.buffer,
                    version: observation.version,
                    at_launch: observation.at_launch,
                });
            }
        }
        // DDCP1-U2 / contract rule 3: a consumer binds the identity and
        // generation produced by its producer edge — inherited unambiguously
        // from the dependency graph (the value's buffer binding, the
        // producer's carried generation, and the program's version chain must
        // agree on exactly one generation).
        self.validate_edge_bindings(semantics)?;
        // DDCP1-U3 / contract §Resource axes: the five axes are separate —
        // a first read before initialization fails closed, a use outside the
        // declared allocation lifetime fails closed, and per-output bounds/
        // index domains never under-fill under the shared dispatch extent.
        self.validate_resource_axes(semantics)?;
        // DDCP1-U5 / contract §Prepared submission regions: the compiler
        // derives the compiler-visible prepared submission regions from the
        // carried dependency graph (one host call → one region; NGAB0-R1)
        // and validates the partition fail-closed — every launch assigned to
        // exactly one region, no region crossing an observation boundary,
        // every region path-connected through the carried edges.
        let regions = semantics.derive_prepared_regions(&launches);
        semantics.validate_regions(&launches, &regions)?;
        Ok(())
    }

    /// DDCP1-U3 / contract §Resource axes — independent-axis validation.
    ///
    /// - **Initialization axis**: a buffer read (read or read-write) by any
    ///   kernel must have a declared initialization fact (zero, host-provided,
    ///   or kernel-initialized) — a first read before initialization fails
    ///   closed. Initialization stays a separate axis from access, lifetime,
    ///   mutation, and observation: no other fact implies or replaces it.
    /// - **Allocation-lifetime axis**: a buffer declaring the read-back
    ///   lifetime class (`ObservationPoint`) must actually be declared
    ///   observed (a result row; the F6 coupling then requires its
    ///   observation fact) — a use outside the declared lifetime fails
    ///   closed. Per-step/per-program classes ride their own declarations.
    /// - **Per-output domains** (`crate::device_program_plans::
    ///   validate_per_output_domains`): every output has its own bounds/
    ///   index domain; no shared dispatch extent under-fills an output.
    fn validate_resource_axes(
        &self,
        semantics: &DeviceSemantics,
    ) -> Result<(), DeviceProgramError> {
        for (kernel_index, kernel) in self.kernels.iter().enumerate() {
            for resource in &kernel.resources {
                let reading = matches!(
                    resource.access,
                    MirKernelResourceAccess::Read | MirKernelResourceAccess::ReadWrite
                );
                if !reading {
                    continue;
                }
                let initialized = semantics
                    .initializations
                    .iter()
                    .any(|fact| fact.buffer == resource.buffer.id);
                if !initialized {
                    let launch = self
                        .launches
                        .iter()
                        .find(|launch| launch.kernel_index == kernel_index)
                        .map_or(LaunchId(0), |launch| launch.id);
                    return Err(DeviceProgramError::UninitializedBufferRead {
                        buffer: resource.buffer.id,
                        launch,
                    });
                }
            }
        }
        for kernel in &self.kernels {
            for resource in &kernel.resources {
                if resource.buffer.lifetime != BufferLifetime::ObservationPoint {
                    continue;
                }
                let observed = self
                    .results
                    .iter()
                    .any(|result| result.buffer.id == resource.buffer.id);
                if !observed {
                    return Err(DeviceProgramError::ObservationPointBufferNotObserved {
                        buffer: resource.buffer.id,
                    });
                }
            }
        }
        crate::device_program_plans::validate_per_output_domains(self)
    }

    /// DDCP1-U2 / contract rule 3 — consumer binding cross-check.
    ///
    /// For every carried dependency edge the consumer's logical value is
    /// inherited unambiguously: the edge's buffer binds exactly one value,
    /// the producer launch actually wrote the flowing buffer version (program
    /// version chain), and exactly one of the producer's carried generations
    /// for that value matches the flowing version's shape. Zero matches is a
    /// missing producer (or a shape contradiction), more than one is an
    /// ambiguous binding — both fail closed.
    fn validate_edge_bindings(
        &self,
        semantics: &DeviceSemantics,
    ) -> Result<(), DeviceProgramError> {
        let registry = self.buffer_registry();
        for edge in &semantics.dependencies {
            let Some(binding) = semantics
                .bindings
                .iter()
                .find(|binding| binding.buffer == edge.buffer)
            else {
                return Err(DeviceProgramError::UnboundEdgeBuffer {
                    buffer: edge.buffer,
                });
            };
            let value = binding.value;

            let Some(entry) = registry.buffer(edge.buffer) else {
                return Err(DeviceProgramError::AbsentProducer {
                    producer: edge.producer,
                    value,
                });
            };
            let Some(version) = entry
                .versions
                .iter()
                .find(|version| version.version.version == edge.version)
            else {
                return Err(DeviceProgramError::AbsentProducer {
                    producer: edge.producer,
                    value,
                });
            };
            if !version.produced_by.contains(&edge.producer) {
                return Err(DeviceProgramError::AbsentProducer {
                    producer: edge.producer,
                    value,
                });
            }

            let matching: Vec<u32> = semantics
                .generations
                .iter()
                .filter(|generation| {
                    generation.value == value
                        && generation.produced_by == edge.producer
                        && generation.element_ty == version.version.element_ty
                        && generation.element_count == version.version.element_count
                })
                .map(|generation| generation.generation)
                .collect();
            match matching.len() {
                0 => {
                    let producer_writes_value = semantics.generations.iter().any(|generation| {
                        generation.value == value && generation.produced_by == edge.producer
                    });
                    if producer_writes_value {
                        return Err(DeviceProgramError::EdgeGenerationShapeMismatch {
                            value,
                            producer: edge.producer,
                            version: edge.version,
                        });
                    }
                    return Err(DeviceProgramError::AbsentProducer {
                        producer: edge.producer,
                        value,
                    });
                }
                1 => {
                    // DDCP1-U4 / contract rule 5 (stale): in a companion-
                    // bearing program, a consumer binding an OLDER generation
                    // while the producer carries a newer one consumes
                    // superseded primal state. A forward-only program
                    // declares no companions and needs none — the staleness
                    // rule applies only where companion outputs are actually
                    // referenced (i.e., where relations are carried).
                    if !semantics.relations.is_empty() {
                        let bound = matching[0];
                        let latest = semantics
                            .generations
                            .iter()
                            .filter(|generation| {
                                generation.value == value && generation.produced_by == edge.producer
                            })
                            .map(|generation| generation.generation)
                            .max()
                            .unwrap_or(bound);
                        if bound < latest {
                            return Err(DeviceProgramError::StaleCompanionFact {
                                value,
                                bound_generation: bound,
                                latest_generation: latest,
                            });
                        }
                    }
                }
                _ => {
                    return Err(DeviceProgramError::AmbiguousEdgeGeneration {
                        value,
                        producer: edge.producer,
                        version: edge.version,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Per-kernel consistency rules: a non-empty entry name, a valid plan,
/// unique `(group, binding)` slots, version numbers ≥ 1, and role/access
/// agreement.
fn validate_kernel_resources(kernels: &[KernelUnit]) -> Result<(), DeviceProgramError> {
    for (kernel_index, kernel) in kernels.iter().enumerate() {
        if kernel.entry.trim().is_empty() {
            return Err(DeviceProgramError::EmptyEntryName {
                kernel: kernel_index,
            });
        }
        kernel
            .plan
            .validate()
            .map_err(|error| DeviceProgramError::Plan { error })?;
        let mut seen_bindings: Vec<Binding> = Vec::new();
        for resource in &kernel.resources {
            if resource.version.version == 0 {
                return Err(DeviceProgramError::ZeroVersion {
                    buffer: resource.buffer.id,
                    version: resource.version.version,
                });
            }
            if seen_bindings.contains(&resource.binding) {
                return Err(DeviceProgramError::RepeatedBinding {
                    kernel: kernel_index,
                    group: resource.binding.group,
                    binding: resource.binding.binding,
                });
            }
            seen_bindings.push(resource.binding);
            if !role_permits_access(resource.buffer.role, resource.access) {
                return Err(DeviceProgramError::RoleAccessMismatch {
                    buffer: resource.buffer.id,
                    role: resource.buffer.role,
                    access: resource.access,
                });
            }
        }
    }
    Ok(())
}

/// Cross-reference consistency: every reference to the same [`BufferId`]
/// carries the same identity facts and the same shape for a given version,
/// and every result is actually produced by its named launch.
fn validate_recorded_identities(program: &DeviceProgram) -> Result<(), DeviceProgramError> {
    let mut first_identities: Vec<BufferIdentity> = Vec::new();
    let mut first_shapes: Vec<ShapeRecord> = Vec::new();
    for kernel in &program.kernels {
        for resource in &kernel.resources {
            record_identity(&mut first_identities, &resource.buffer)?;
            record_shape(&mut first_shapes, resource.buffer.id, &resource.version)?;
        }
    }
    for (result_index, result) in program.results.iter().enumerate() {
        record_identity(&mut first_identities, &result.buffer)?;
        record_shape(&mut first_shapes, result.buffer.id, &result.version)?;
        if !result_is_produced(program, result) {
            return Err(DeviceProgramError::ResultNotProduced {
                result: result_index,
                produced_by: result.produced_by,
                buffer: result.buffer.id,
                version: result.version.version,
            });
        }
    }
    Ok(())
}
