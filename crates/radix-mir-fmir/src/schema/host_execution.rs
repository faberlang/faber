//! Radix-owned v1 physical execution and invocation packets.
//!
//! This module is the only producer of the compact JSON wire contract. It
//! intentionally carries physical execution facts only. Hosts receives the
//! encoded bytes through its independent decode-only mirror; it does not
//! depend on this crate or recreate these packet semantics.

use std::{collections::BTreeMap, error::Error, fmt};

use serde::Serialize;

use super::sha256_digest;

/// Static physical execution packet schema name.
pub const HOST_EXECUTION_SCHEMA: &str = "faber.host-execution";

/// Static physical execution packet schema version.
pub const HOST_EXECUTION_WIRE_VERSION: u32 = 1;

/// Dynamic physical invocation packet schema name.
pub const HOST_INVOCATION_SCHEMA: &str = "faber.host-invocation";

/// Dynamic physical invocation packet schema version.
pub const HOST_INVOCATION_WIRE_VERSION: u32 = 1;

/// Version-one static physical execution packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostExecutionEnvelopeV1 {
    pub schema: String,
    pub version: u32,
    pub program: WireHostExecutionProgramV1,
}

/// Static physical execution facts for one selected target artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostExecutionProgramV1 {
    pub target: WireHostTargetV1,
    pub artifact: WireHostArtifactV1,
    pub resources: Vec<WireHostResourceV1>,
    pub kernels: Vec<WireHostKernelV1>,
    pub launches: Vec<WireHostLaunchV1>,
    pub dependencies: Vec<WireHostDependencyV1>,
    pub observations: Vec<WireHostObservationV1>,
}

/// The physical target selected by Radix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostTargetV1 {
    pub backend: String,
    pub required_features: Vec<String>,
}

/// The selected physical artifact's identity and byte shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostArtifactV1 {
    pub format: String,
    pub abi_version: u32,
    pub byte_length: u64,
    pub sha256: String,
}

/// A physical allocation declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostResourceV1 {
    pub id: u32,
    pub byte_capacity: u64,
    pub byte_alignment: u32,
    pub lifetime: WireHostResourceLifetimeV1,
    pub initialization: WireHostInitializationV1,
}

/// Physical allocation lifetime, without product-session vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireHostResourceLifetimeV1 {
    PreparedExecution,
    Invocation,
}

/// Physical first-definition requirement for an allocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireHostInitializationV1 {
    Zeroed,
    Supplied { input_slot: u32 },
    DeviceWritten,
}

/// A final backend entry and its physical binding slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostKernelV1 {
    pub id: u32,
    pub entry: String,
    pub block: [u32; 3],
    pub slots: Vec<WireHostKernelSlotV1>,
}

/// One physical binding location in a final backend entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostKernelSlotV1 {
    pub id: u32,
    pub group: u32,
    pub binding: u32,
    pub access: WireHostAccessV1,
}

/// Physical access granted to a bound resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireHostAccessV1 {
    Read,
    Write,
    ReadWrite,
}

/// One launch in admitted execution order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostLaunchV1 {
    pub id: u32,
    pub kernel: u32,
    pub grid: [u32; 3],
    pub bindings: Vec<WireHostLaunchBindingV1>,
}

/// The complete physical resource binding for one kernel slot at one launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostLaunchBindingV1 {
    pub slot: u32,
    pub resource: u32,
    pub window: WireHostBindingWindowV1,
}

/// A static byte range or a bounded per-invocation byte-range choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireHostBindingWindowV1 {
    Exact {
        byte_offset: u64,
        byte_length: u64,
    },
    Invocation {
        window_slot: u32,
        maximum_byte_offset: u64,
        maximum_byte_length: u64,
        byte_alignment: u32,
    },
}

/// A physical launch-order dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostDependencyV1 {
    pub before: u32,
    pub after: u32,
}

/// A declared physical readback opportunity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostObservationV1 {
    pub id: u32,
    pub resource: u32,
    pub after_launch: u32,
    pub window: WireHostBindingWindowV1,
}

/// Version-one dynamic physical invocation packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostInvocationEnvelopeV1 {
    pub schema: String,
    pub version: u32,
    pub invocation: WireHostInvocationV1,
}

/// Physical values selected for one invocation of an execution packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostInvocationV1 {
    pub execution_sha256: String,
    pub correlation_id: String,
    pub windows: Vec<WireHostInvocationWindowV1>,
    pub requested_observations: Vec<u32>,
}

/// One exact physical byte range for a declared dynamic window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireHostInvocationWindowV1 {
    pub window_slot: u32,
    pub byte_offset: u64,
    pub byte_length: u64,
}

/// A producer-side rejection of an invalid physical packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostExecutionWireError {
    field: &'static str,
    detail: String,
}

impl HostExecutionWireError {
    fn invalid(field: &'static str, detail: impl Into<String>) -> Self {
        Self {
            field,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for HostExecutionWireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid host execution wire {}: {}",
            self.field, self.detail
        )
    }
}

impl Error for HostExecutionWireError {}

/// Validates and encodes an exact compact v1 static execution packet.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn encode_host_execution_v1(
    envelope: &WireHostExecutionEnvelopeV1,
) -> Result<Vec<u8>, HostExecutionWireError> {
    validate_execution(envelope)?;
    encode_compact(envelope)
}

/// Validates and encodes an exact compact v1 dynamic invocation packet.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn encode_host_invocation_v1(
    envelope: &WireHostInvocationEnvelopeV1,
) -> Result<Vec<u8>, HostExecutionWireError> {
    validate_invocation(envelope)?;
    encode_compact(envelope)
}

fn encode_compact<T: Serialize>(value: &T) -> Result<Vec<u8>, HostExecutionWireError> {
    serde_json::to_vec(value)
        .map_err(|error| HostExecutionWireError::invalid("packet", error.to_string()))
}

fn validate_execution(
    envelope: &WireHostExecutionEnvelopeV1,
) -> Result<(), HostExecutionWireError> {
    if envelope.schema != HOST_EXECUTION_SCHEMA {
        return Err(HostExecutionWireError::invalid(
            "schema",
            "must be faber.host-execution",
        ));
    }
    if envelope.version != HOST_EXECUTION_WIRE_VERSION {
        return Err(HostExecutionWireError::invalid("version", "must be 1"));
    }

    let program = &envelope.program;
    require_nonempty("program.target.backend", &program.target.backend)?;
    require_strict_strings(
        "program.target.required_features",
        &program.target.required_features,
    )?;
    require_nonempty("program.artifact.format", &program.artifact.format)?;
    require_nonzero("program.artifact.abi_version", program.artifact.abi_version)?;
    require_nonzero("program.artifact.byte_length", program.artifact.byte_length)?;
    require_sha256("program.artifact.sha256", &program.artifact.sha256)?;

    validate_resources(&program.resources)?;
    validate_kernels(&program.kernels)?;
    let (mut dynamic_windows, mut next_dynamic_slot) = validate_launches(program)?;
    validate_dependencies(&program.launches, &program.dependencies)?;
    validate_observations(program, &mut dynamic_windows, &mut next_dynamic_slot)?;
    Ok(())
}

fn validate_resources(resources: &[WireHostResourceV1]) -> Result<(), HostExecutionWireError> {
    let mut next_input_slot = 1;
    for (index, resource) in resources.iter().enumerate() {
        require_canonical_id("program.resources", resource.id, index)?;
        require_nonzero("program.resources.byte_capacity", resource.byte_capacity)?;
        require_nonzero("program.resources.byte_alignment", resource.byte_alignment)?;
        if let WireHostInitializationV1::Supplied { input_slot } = &resource.initialization {
            if *input_slot != next_input_slot {
                return Err(HostExecutionWireError::invalid(
                    "program.resources.initialization.input_slot",
                    "must be assigned in resource order starting at one",
                ));
            }
            next_input_slot = next_input_slot.checked_add(1).ok_or_else(|| {
                HostExecutionWireError::invalid(
                    "program.resources.initialization.input_slot",
                    "input-slot assignment overflows u32",
                )
            })?;
        }
    }
    Ok(())
}

fn validate_kernels(kernels: &[WireHostKernelV1]) -> Result<(), HostExecutionWireError> {
    for (index, kernel) in kernels.iter().enumerate() {
        require_canonical_id("program.kernels", kernel.id, index)?;
        require_nonempty("program.kernels.entry", &kernel.entry)?;
        validate_dimensions("program.kernels.block", kernel.block)?;

        let mut previous_location = None;
        for (slot_index, slot) in kernel.slots.iter().enumerate() {
            require_canonical_id("program.kernels.slots", slot.id, slot_index)?;
            let location = (slot.group, slot.binding);
            if previous_location.is_some_and(|previous| previous >= location) {
                return Err(HostExecutionWireError::invalid(
                    "program.kernels.slots",
                    "must be in strict (group, binding) order",
                ));
            }
            previous_location = Some(location);
        }
    }
    Ok(())
}

fn validate_launches(
    program: &WireHostExecutionProgramV1,
) -> Result<(BTreeMap<u32, DynamicWindow>, u32), HostExecutionWireError> {
    let mut dynamic_windows = BTreeMap::new();
    let mut next_dynamic_slot = 1;

    for (index, launch) in program.launches.iter().enumerate() {
        require_canonical_id("program.launches", launch.id, index)?;
        validate_dimensions("program.launches.grid", launch.grid)?;
        let kernel = resource_by_id(&program.kernels, launch.kernel, |kernel| kernel.id)
            .ok_or_else(|| {
                HostExecutionWireError::invalid(
                    "program.launches.kernel",
                    "must reference a declared kernel",
                )
            })?;
        if launch.bindings.len() != kernel.slots.len() {
            return Err(HostExecutionWireError::invalid(
                "program.launches.bindings",
                "must bind every kernel slot exactly once",
            ));
        }
        for (binding_index, binding) in launch.bindings.iter().enumerate() {
            let expected_slot = kernel.slots[binding_index].id;
            if binding.slot != expected_slot {
                return Err(HostExecutionWireError::invalid(
                    "program.launches.bindings.slot",
                    "must follow the referenced kernel slot order",
                ));
            }
            let resource =
                resource_by_id(&program.resources, binding.resource, |resource| resource.id)
                    .ok_or_else(|| {
                        HostExecutionWireError::invalid(
                            "program.launches.bindings.resource",
                            "must reference a declared resource",
                        )
                    })?;
            validate_window(
                "program.launches.bindings.window",
                &binding.window,
                resource,
                &mut dynamic_windows,
                &mut next_dynamic_slot,
            )?;
        }
    }
    Ok((dynamic_windows, next_dynamic_slot))
}

fn validate_dependencies(
    launches: &[WireHostLaunchV1],
    dependencies: &[WireHostDependencyV1],
) -> Result<(), HostExecutionWireError> {
    let mut previous = None;
    let mut successors = vec![Vec::new(); launches.len()];
    let mut inbound = vec![0_usize; launches.len()];

    for dependency in dependencies {
        let pair = (dependency.before, dependency.after);
        if previous.is_some_and(|prior| prior >= pair) {
            return Err(HostExecutionWireError::invalid(
                "program.dependencies",
                "must be in strict (before, after) order",
            ));
        }
        previous = Some(pair);
        if dependency.before == dependency.after {
            return Err(HostExecutionWireError::invalid(
                "program.dependencies",
                "cannot depend on the same launch",
            ));
        }
        let before = launch_index(launches, dependency.before)?;
        let after = launch_index(launches, dependency.after)?;
        successors[before].push(after);
        inbound[after] += 1;
    }

    let mut ready = (0..launches.len())
        .filter(|&index| inbound[index] == 0)
        .collect::<Vec<_>>();
    let mut admitted = 0;
    while let Some(index) = ready.pop() {
        admitted += 1;
        for &successor in &successors[index] {
            inbound[successor] -= 1;
            if inbound[successor] == 0 {
                ready.push(successor);
            }
        }
    }
    if admitted != launches.len() {
        return Err(HostExecutionWireError::invalid(
            "program.dependencies",
            "must be acyclic",
        ));
    }
    Ok(())
}

fn validate_observations(
    program: &WireHostExecutionProgramV1,
    dynamic_windows: &mut BTreeMap<u32, DynamicWindow>,
    next_dynamic_slot: &mut u32,
) -> Result<(), HostExecutionWireError> {
    let mut previous_key = None;

    for (index, observation) in program.observations.iter().enumerate() {
        require_canonical_id("program.observations", observation.id, index)?;
        let resource = resource_by_id(&program.resources, observation.resource, |resource| {
            resource.id
        })
        .ok_or_else(|| {
            HostExecutionWireError::invalid(
                "program.observations.resource",
                "must reference a declared resource",
            )
        })?;
        launch_index(&program.launches, observation.after_launch)?;
        let key = observation_key(observation);
        if previous_key.is_some_and(|previous| previous >= key) {
            return Err(HostExecutionWireError::invalid(
                "program.observations",
                "must be in canonical physical order",
            ));
        }
        previous_key = Some(key);
        validate_window(
            "program.observations.window",
            &observation.window,
            resource,
            dynamic_windows,
            next_dynamic_slot,
        )?;
    }
    Ok(())
}

fn validate_window(
    field: &'static str,
    window: &WireHostBindingWindowV1,
    resource: &WireHostResourceV1,
    dynamic_windows: &mut BTreeMap<u32, DynamicWindow>,
    next_dynamic_slot: &mut u32,
) -> Result<(), HostExecutionWireError> {
    match window {
        WireHostBindingWindowV1::Exact {
            byte_offset,
            byte_length,
        } => {
            require_nonzero(field, *byte_length)?;
            require_range(field, *byte_offset, *byte_length, resource.byte_capacity)?;
            if byte_offset % u64::from(resource.byte_alignment) != 0 {
                return Err(HostExecutionWireError::invalid(
                    field,
                    "exact byte offset must satisfy the resource alignment",
                ));
            }
        }
        WireHostBindingWindowV1::Invocation {
            window_slot,
            maximum_byte_offset,
            maximum_byte_length,
            byte_alignment,
        } => {
            require_nonzero(field, *window_slot)?;
            require_nonzero(field, *maximum_byte_length)?;
            require_nonzero(field, *byte_alignment)?;
            require_range(
                field,
                *maximum_byte_offset,
                *maximum_byte_length,
                resource.byte_capacity,
            )?;
            if maximum_byte_offset % u64::from(*byte_alignment) != 0 {
                return Err(HostExecutionWireError::invalid(
                    field,
                    "maximum byte offset must satisfy the declared alignment",
                ));
            }
            admit_dynamic_window(
                field,
                *window_slot,
                DynamicWindow {
                    maximum_byte_offset: *maximum_byte_offset,
                    maximum_byte_length: *maximum_byte_length,
                    byte_alignment: *byte_alignment,
                },
                dynamic_windows,
                next_dynamic_slot,
            )?;
        }
    }
    Ok(())
}

/// Admit one invocation window against the per-artifact dynamic-slot map.
///
/// A slot is assigned on first use in physical order (`next_dynamic_slot`)
/// and every later reuse must repeat the exact physical envelope; a slot may
/// not be silently redefined at a new launch.
fn admit_dynamic_window(
    field: &'static str,
    window_slot: u32,
    declared: DynamicWindow,
    dynamic_windows: &mut BTreeMap<u32, DynamicWindow>,
    next_dynamic_slot: &mut u32,
) -> Result<(), HostExecutionWireError> {
    match dynamic_windows.get(&window_slot) {
        Some(previous) if previous != &declared => Err(HostExecutionWireError::invalid(
            field,
            "reused dynamic window slots must retain one physical envelope",
        )),
        Some(_) => Ok(()),
        None if window_slot == *next_dynamic_slot => {
            dynamic_windows.insert(window_slot, declared);
            *next_dynamic_slot = next_dynamic_slot.checked_add(1).ok_or_else(|| {
                HostExecutionWireError::invalid(
                    field,
                    "dynamic window-slot assignment overflows u32",
                )
            })?;
            Ok(())
        }
        None => Err(HostExecutionWireError::invalid(
            field,
            "dynamic window slots must be assigned by first use starting at one",
        )),
    }
}

fn validate_invocation(
    envelope: &WireHostInvocationEnvelopeV1,
) -> Result<(), HostExecutionWireError> {
    if envelope.schema != HOST_INVOCATION_SCHEMA {
        return Err(HostExecutionWireError::invalid(
            "schema",
            "must be faber.host-invocation",
        ));
    }
    if envelope.version != HOST_INVOCATION_WIRE_VERSION {
        return Err(HostExecutionWireError::invalid("version", "must be 1"));
    }
    require_sha256(
        "invocation.execution_sha256",
        &envelope.invocation.execution_sha256,
    )?;

    let mut previous_slot = None;
    for window in &envelope.invocation.windows {
        require_nonzero("invocation.windows.window_slot", window.window_slot)?;
        require_nonzero("invocation.windows.byte_length", window.byte_length)?;
        if previous_slot.is_some_and(|previous| previous >= window.window_slot) {
            return Err(HostExecutionWireError::invalid(
                "invocation.windows",
                "must be in strict window-slot order",
            ));
        }
        previous_slot = Some(window.window_slot);
    }
    require_strict_ids(
        "invocation.requested_observations",
        &envelope.invocation.requested_observations,
    )
}

fn require_nonempty(field: &'static str, value: &str) -> Result<(), HostExecutionWireError> {
    if value.is_empty() {
        Err(HostExecutionWireError::invalid(field, "must not be empty"))
    } else {
        Ok(())
    }
}

fn require_nonzero<T>(field: &'static str, value: T) -> Result<(), HostExecutionWireError>
where
    T: Copy + PartialEq + From<u8>,
{
    if value == T::from(0) {
        Err(HostExecutionWireError::invalid(field, "must be nonzero"))
    } else {
        Ok(())
    }
}

fn require_canonical_id(
    field: &'static str,
    actual: u32,
    index: usize,
) -> Result<(), HostExecutionWireError> {
    let expected = u32::try_from(index + 1)
        .map_err(|_| HostExecutionWireError::invalid(field, "contains too many rows"))?;
    if actual == expected {
        Ok(())
    } else {
        Err(HostExecutionWireError::invalid(
            field,
            "must be nonzero sequential IDs in canonical order",
        ))
    }
}

fn require_strict_strings(
    field: &'static str,
    values: &[String],
) -> Result<(), HostExecutionWireError> {
    let mut previous = None;
    for value in values {
        require_nonempty(field, value)?;
        if previous.is_some_and(|prior| prior >= value.as_str()) {
            return Err(HostExecutionWireError::invalid(
                field,
                "must be strict lexicographic order",
            ));
        }
        previous = Some(value.as_str());
    }
    Ok(())
}

fn require_strict_ids(field: &'static str, values: &[u32]) -> Result<(), HostExecutionWireError> {
    let mut previous = None;
    for &value in values {
        require_nonzero(field, value)?;
        if previous.is_some_and(|prior| prior >= value) {
            return Err(HostExecutionWireError::invalid(
                field,
                "must be strict ascending numeric order",
            ));
        }
        previous = Some(value);
    }
    Ok(())
}

fn require_sha256(field: &'static str, value: &str) -> Result<(), HostExecutionWireError> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(HostExecutionWireError::invalid(
            field,
            "must use sha256:<64 lowercase hex digits>",
        ))
    }
}

fn validate_dimensions(
    field: &'static str,
    dimensions: [u32; 3],
) -> Result<(), HostExecutionWireError> {
    let mut product = 1_u64;
    for dimension in dimensions {
        require_nonzero(field, dimension)?;
        product = product.checked_mul(u64::from(dimension)).ok_or_else(|| {
            HostExecutionWireError::invalid(field, "total dimension count overflows u64")
        })?;
    }
    Ok(())
}

fn require_range(
    field: &'static str,
    byte_offset: u64,
    byte_length: u64,
    byte_capacity: u64,
) -> Result<(), HostExecutionWireError> {
    let end = byte_offset
        .checked_add(byte_length)
        .ok_or_else(|| HostExecutionWireError::invalid(field, "byte range overflows u64"))?;
    if end > byte_capacity {
        Err(HostExecutionWireError::invalid(
            field,
            "byte range exceeds resource capacity",
        ))
    } else {
        Ok(())
    }
}

fn resource_by_id<T, F>(rows: &[T], id: u32, identifier: F) -> Option<&T>
where
    F: Fn(&T) -> u32,
{
    rows.iter().find(|row| identifier(row) == id)
}

fn launch_index(launches: &[WireHostLaunchV1], id: u32) -> Result<usize, HostExecutionWireError> {
    launches
        .iter()
        .position(|launch| launch.id == id)
        .ok_or_else(|| {
            HostExecutionWireError::invalid(
                "program.launches",
                "dependency or observation must reference a declared launch",
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DynamicWindow {
    maximum_byte_offset: u64,
    maximum_byte_length: u64,
    byte_alignment: u32,
}

fn observation_key(observation: &WireHostObservationV1) -> (u32, u32, u8, u64, u64, u64, u32) {
    let (kind, first, second, third, alignment) = match &observation.window {
        WireHostBindingWindowV1::Exact {
            byte_offset,
            byte_length,
        } => (0, *byte_offset, *byte_length, 0, 0),
        WireHostBindingWindowV1::Invocation {
            window_slot,
            maximum_byte_offset,
            maximum_byte_length,
            byte_alignment,
        } => (
            1,
            u64::from(*window_slot),
            *maximum_byte_offset,
            *maximum_byte_length,
            *byte_alignment,
        ),
    };
    (
        observation.after_launch,
        observation.resource,
        kind,
        first,
        second,
        third,
        alignment,
    )
}

/// SHA-256 identity of exact encoded execution packet bytes.
#[must_use]
pub fn host_execution_sha256(bytes: &[u8]) -> String {
    sha256_digest(bytes)
}

/// SHA-256 identity of exact encoded invocation packet bytes.
#[must_use]
pub fn host_invocation_sha256(bytes: &[u8]) -> String {
    sha256_digest(bytes)
}
