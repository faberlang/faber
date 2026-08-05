use super::error::DeviceProgramError;
use super::helpers::{
    record_identity, record_shape, registry_buffer_mut, registry_version_mut, result_is_produced,
    role_permits_access,
};
use super::registry::{BufferRegistry, RegistryBuffer};
use super::types::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, DeviceProgram,
    DeviceProgramLifetime, LaunchId, ObservationCadence,
};
use crate::abi::MirKernelResourceAccess;
use crate::device_semantics::DeviceSemantics;
use crate::MirType;

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

        for (kernel_index, kernel) in self.kernels.iter().enumerate() {
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
                    return Err(DeviceProgramError::DuplicateBinding {
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
                return Err(DeviceProgramError::DuplicateLaunchId { launch: launch.id });
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
                return Err(DeviceProgramError::UnknownProducingLaunch {
                    result: result_index,
                    produced_by: result.produced_by,
                });
            }
        }

        let mut first_identities: Vec<BufferIdentity> = Vec::new();
        let mut first_shapes: Vec<(BufferId, u32, MirType, u64)> = Vec::new();
        for kernel in &self.kernels {
            for resource in &kernel.resources {
                record_identity(&mut first_identities, &resource.buffer)?;
                record_shape(&mut first_shapes, resource.buffer.id, &resource.version)?;
            }
        }
        for (result_index, result) in self.results.iter().enumerate() {
            record_identity(&mut first_identities, &result.buffer)?;
            record_shape(&mut first_shapes, result.buffer.id, &result.version)?;
            if !result_is_produced(self, result) {
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
        Ok(())
    }
}
