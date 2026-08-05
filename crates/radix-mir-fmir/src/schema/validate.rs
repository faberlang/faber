use super::distributed::FmirDistributedExecutionSection;
use super::wire::{
    WireBroadcastDeclaration, WireBufferIdentity, WireBufferLifetime, WireCollectionKernelPlan,
    WireDeviceProgram, WireInitializationPolicy, WireObservationCadence, WireProgramLifetime,
    WireResourceAccess,
};
use super::SHA256_IDENTITY_PREFIX;

// ---------------------------------------------------------------------------
// Wire-level semantic admission (Stage 3R F1–F7)
// ---------------------------------------------------------------------------
//
// The radix-mir `DeviceSemantics` rules, run on the serialized wire form at
// the decode boundary: semantic identity (F1), generations (F2), roots +
// dependencies (F3), the lossless relation (F4), and the independent
// initialization/observation axes (F5/F6). Fail-closed: the first violation
// fails admission before host construction or launch.

/// Validate the carried wire facts fail-closed (F1–F7 + the S5-U5b declared
/// step count + the S6-C2 broadcast fact).
pub(crate) fn validate_wire_program(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    let launches: Vec<u32> = wire.launches.iter().map(|launch| launch.id).collect();
    validate_wire_semantic_values(wire)?;
    validate_wire_generations(wire)?;
    validate_wire_graph(wire, &launches)?;
    validate_wire_relations(wire)?;
    validate_wire_axes(wire, &launches)?;
    validate_wire_step_count(wire)?;
    validate_wire_broadcast_facts(wire)?;
    Ok(())
}

/// Rule S6-C2 (A7): every carried rank-extension broadcast fact (the
/// `RankExtensionAdd` plan payload) must be internally consistent — the
/// higher-rank operand is exactly one rank above the lower-rank operand, the
/// lower-rank dims equal the higher-rank trailing dims, and the result is
/// the higher-rank shape. A shape mismatch fails admission closed (the fact
/// is carried, never inferred — A3).
fn validate_wire_broadcast_facts(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    for kernel in &wire.kernels {
        let WireCollectionKernelPlan::RankExtensionAdd(fact) = &kernel.plan else {
            continue;
        };
        let higher = if fact.lhs_shape.len() >= fact.rhs_shape.len() {
            &fact.lhs_shape
        } else {
            &fact.rhs_shape
        };
        let lower = if fact.lhs_shape.len() >= fact.rhs_shape.len() {
            &fact.rhs_shape
        } else {
            &fact.lhs_shape
        };
        if fact.declaration != WireBroadcastDeclaration::RankExtension {
            return Err(wire_invalid(format!(
                "kernel '{}' carries the unsupported broadcast declaration {fact:?}; only rank-extension is admitted (S6-C2 A7)",
                kernel.entry
            )));
        }
        if higher.len() != lower.len() + 1 || lower != &higher[1..] {
            return Err(wire_invalid(format!(
                "kernel '{}' carries a mismatched rank-extension broadcast fact (lhs {:?}, rhs {:?}); the lower-rank dims must equal the higher-rank trailing dims (S6-C2 A7)",
                kernel.entry, fact.lhs_shape, fact.rhs_shape
            )));
        }
        if fact.result_shape != *higher {
            return Err(wire_invalid(format!(
                "kernel '{}' carries a rank-extension broadcast fact whose result shape {:?} does not equal the higher-rank operand shape {:?} (S6-C2 A7)",
                kernel.entry, fact.result_shape, *higher
            )));
        }
    }
    Ok(())
}

/// Rule S5-U5b: a `RepeatingStep` program must carry a declared step count
/// of at least 1 — a zero-count training program is a contradiction and fails
/// admission closed (the image's step count is the route's authority; a count
/// the route could never drive is rejected before host construction).
fn validate_wire_step_count(wire: &WireDeviceProgram) -> Result<(), crate::error::FmirImageError> {
    if let WireProgramLifetime::RepeatingStep(declared) = wire.lifetime {
        if declared == 0 {
            return Err(wire_invalid(
                "RepeatingStep program declares step count 0; a training program must declare at least one step".to_owned(),
            ));
        }
    }
    Ok(())
}

/// Rule F1: unique semantic-value identities; never two origins for one id,
/// never two ids for one origin; every buffer reference names a declared
/// value; one buffer holds one value.
fn validate_wire_semantic_values(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    for left in &wire.semantic_values {
        for right in &wire.semantic_values {
            if left.id == right.id && left != right {
                return Err(wire_invalid(format!(
                    "semantic value id {} is declared with conflicting facts ({} vs {})",
                    left.id, left.name, right.name
                )));
            }
            if left.id != right.id && left.origin == right.origin {
                return Err(wire_invalid(format!(
                    "semantic values {} and {} both derive from origin {left:?}; distinct values must derive from distinct carried facts",
                    left.id, right.id
                )));
            }
        }
    }
    for buffer in wire_buffers(wire) {
        let value = buffer.semantic_value;
        if !wire.semantic_values.iter().any(|row| row.id == value) {
            return Err(wire_invalid(format!(
                "buffer {} references semantic value {value}, which is never declared",
                buffer.id
            )));
        }
    }
    for left in wire_buffers(wire) {
        for right in wire_buffers(wire) {
            if left.id == right.id && left.semantic_value != right.semantic_value {
                return Err(wire_invalid(format!(
                    "buffer {} holds semantic values {} and {}; one buffer holds one value",
                    left.id, left.semantic_value, right.semantic_value
                )));
            }
        }
    }
    Ok(())
}

/// Rule F2: every slot carries an explicit generation (0 is the reserved
/// unset sentinel); a (value, generation) has exactly one producer; every
/// generation a slot consumes is produced elsewhere or covered by an
/// initialization fact (complete inputs).
fn validate_wire_generations(wire: &WireDeviceProgram) -> Result<(), crate::error::FmirImageError> {
    for kernel in &wire.kernels {
        for resource in &kernel.resources {
            if resource.generation == 0 {
                return Err(wire_invalid(format!(
                    "a slot on buffer {} carries generation 0 (the reserved unset sentinel)",
                    resource.buffer.id
                )));
            }
        }
    }
    // Single definition per value generation: two DISTINCT write/read-write
    // slots may not claim the same (value, generation).
    let producers: Vec<(u32, u32, usize, usize)> = wire
        .kernels
        .iter()
        .enumerate()
        .flat_map(|(kernel_index, kernel)| {
            kernel
                .resources
                .iter()
                .enumerate()
                .filter_map(move |(slot_index, resource)| {
                    if matches!(
                        resource.access,
                        WireResourceAccess::Write | WireResourceAccess::ReadWrite
                    ) {
                        Some((
                            resource.buffer.semantic_value,
                            resource.generation,
                            kernel_index,
                            slot_index,
                        ))
                    } else {
                        None
                    }
                })
        })
        .collect();
    for (index, (value, generation, kernel, slot)) in producers.iter().enumerate() {
        for (other_index, (other_value, other_generation, other_kernel, other_slot)) in
            producers.iter().enumerate()
        {
            if index < other_index && value == other_value && generation == other_generation {
                return Err(wire_invalid(format!(
                    "semantic value {value} generation {generation} has a second producer (kernels {kernel}/{other_kernel}, slots {slot}/{other_slot}); a later write is a new generation"
                )));
            }
        }
    }
    // Complete inputs: a read consumes its generation; a read-write consumes
    // the previous generation and produces its own. Every consumed generation
    // is produced by a DIFFERENT slot, or is the buffer's initial state
    // covered by its initialization policy.
    for (kernel_index, kernel) in wire.kernels.iter().enumerate() {
        for (slot_index, resource) in kernel.resources.iter().enumerate() {
            let value = resource.buffer.semantic_value;
            let consumed: Option<u32> = match resource.access {
                WireResourceAccess::Read => Some(resource.generation),
                WireResourceAccess::ReadWrite => Some(resource.generation - 1),
                WireResourceAccess::Write => None,
            };
            let Some(consumed) = consumed else {
                continue;
            };
            let produced_elsewhere = wire
                .kernels
                .iter()
                .enumerate()
                .flat_map(|(other_kernel, other_unit)| {
                    other_unit.resources.iter().enumerate().filter_map(
                        move |(other_slot, other)| {
                            if (other_kernel != kernel_index || other_slot != slot_index)
                                && other.buffer.semantic_value == value
                                && other.generation == consumed
                                && matches!(
                                    other.access,
                                    WireResourceAccess::Write | WireResourceAccess::ReadWrite
                                )
                            {
                                Some(())
                            } else {
                                None
                            }
                        },
                    )
                })
                .next()
                .is_some();
            if produced_elsewhere {
                continue;
            }
            let init_defined = consumed <= 1
                && wire_buffer_init_policy(wire, resource.buffer.id).is_some_and(|policy| {
                    matches!(
                        policy,
                        WireInitializationPolicy::HostProvided | WireInitializationPolicy::ZeroFill
                    )
                });
            if !init_defined {
                return Err(wire_invalid(format!(
                    "buffer {} reads semantic value {value} generation {consumed}, which no launch produces and no initialization defines (incomplete input)",
                    resource.buffer.id
                )));
            }
        }
    }
    Ok(())
}

/// Rule F3: legal, unique, non-empty roots; well-formed, duplicate-free,
/// acyclic dependencies; every generation producer reachable from a root.
fn validate_wire_graph(
    wire: &WireDeviceProgram,
    launches: &[u32],
) -> Result<(), crate::error::FmirImageError> {
    if wire.roots.is_empty() && !wire.launches.is_empty() {
        return Err(wire_invalid(
            "the program declares launches but no execution root (F3: roots are carried, never inferred)"
                .to_owned(),
        ));
    }
    for root in &wire.roots {
        if !launches.contains(root) {
            return Err(wire_invalid(format!(
                "root {root} is not a declared launch"
            )));
        }
        if wire
            .roots
            .iter()
            .filter(|candidate| *candidate == root)
            .count()
            > 1
        {
            return Err(wire_invalid(format!(
                "root {root} is declared more than once"
            )));
        }
    }
    for edge in &wire.dependencies {
        if edge.producer == edge.consumer {
            return Err(wire_invalid(format!(
                "dependency edge runs from launch {} to itself",
                edge.producer
            )));
        }
        if !launches.contains(&edge.producer) {
            return Err(wire_invalid(format!(
                "dependency edge names producer launch {}, which does not exist",
                edge.producer
            )));
        }
        if !launches.contains(&edge.consumer) {
            return Err(wire_invalid(format!(
                "dependency edge names consumer launch {}, which does not exist",
                edge.consumer
            )));
        }
        if wire
            .dependencies
            .iter()
            .filter(|candidate| {
                candidate.producer == edge.producer
                    && candidate.consumer == edge.consumer
                    && candidate.buffer == edge.buffer
                    && candidate.version == edge.version
            })
            .count()
            > 1
        {
            return Err(wire_invalid(format!(
                "dependency edge {} → {} is declared more than once",
                edge.producer, edge.consumer
            )));
        }
    }
    reject_wire_dependency_cycles(wire)?;
    // Missing dependency: every generation producer must be reachable from a
    // root; a producer hanging off the graph could never be scheduled.
    let producers: Vec<u32> = wire
        .launches
        .iter()
        .filter(|launch| {
            wire.kernels
                .get(launch.kernel_index as usize)
                .is_some_and(|kernel| {
                    kernel.resources.iter().any(|resource| {
                        matches!(
                            resource.access,
                            WireResourceAccess::Write | WireResourceAccess::ReadWrite
                        )
                    })
                })
        })
        .map(|launch| launch.id)
        .collect();
    for producer in &producers {
        if !wire_reachable_from_root(wire, *producer) {
            return Err(wire_invalid(format!(
                "launch {producer} produces a value but is not reachable from any root (missing dependency)"
            )));
        }
    }
    Ok(())
}

/// Whether a launch is reachable from a declared root via the dependency
/// edges.
fn wire_reachable_from_root(wire: &WireDeviceProgram, launch: u32) -> bool {
    let mut seen = wire.roots.clone();
    let mut frontier = wire.roots.clone();
    while let Some(current) = frontier.pop() {
        if current == launch {
            return true;
        }
        for edge in &wire.dependencies {
            if edge.producer == current && !seen.contains(&edge.consumer) {
                seen.push(edge.consumer);
                frontier.push(edge.consumer);
            }
        }
    }
    false
}

/// Reject a dependency cycle (F3) — depth-first back-edge detection.
fn reject_wire_dependency_cycles(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    let mut visited: Vec<u32> = Vec::new();
    let mut on_stack: Vec<u32> = Vec::new();
    for edge in &wire.dependencies {
        if wire_cycle_through(wire, edge.producer, &mut visited, &mut on_stack) {
            return Err(wire_invalid(format!(
                "dependency graph contains a cycle through launch {}",
                edge.producer
            )));
        }
    }
    Ok(())
}

fn wire_cycle_through(
    wire: &WireDeviceProgram,
    launch: u32,
    visited: &mut Vec<u32>,
    on_stack: &mut Vec<u32>,
) -> bool {
    if on_stack.contains(&launch) {
        return true;
    }
    if visited.contains(&launch) {
        return false;
    }
    visited.push(launch);
    on_stack.push(launch);
    for edge in &wire.dependencies {
        if edge.producer == launch && wire_cycle_through(wire, edge.consumer, visited, on_stack) {
            return true;
        }
    }
    on_stack.retain(|seen| *seen != launch);
    false
}

/// Rule F4: relation rows are unique per primal and internally well-formed;
/// duplicate or contradictory rows fail admission.
fn validate_wire_relations(wire: &WireDeviceProgram) -> Result<(), crate::error::FmirImageError> {
    for relation in &wire.relations {
        if relation.primal == relation.companion {
            return Err(wire_invalid(format!(
                "relation row for primal {} declares the same function as companion",
                relation.primal
            )));
        }
        let mut positions: Vec<u32> = Vec::new();
        for selected in &relation.selected_inputs {
            if positions.contains(&selected.position) {
                return Err(wire_invalid(format!(
                    "relation row for primal {} repeats selected-input position {}",
                    relation.primal, selected.position
                )));
            }
            positions.push(selected.position);
        }
        let mut output_positions: Vec<u32> = Vec::new();
        for selected in &relation.selected_outputs {
            if output_positions.contains(&selected.position) {
                return Err(wire_invalid(format!(
                    "relation row for primal {} repeats selected-output position {}",
                    relation.primal, selected.position
                )));
            }
            output_positions.push(selected.position);
        }
    }
    for relation in &wire.relations {
        if wire
            .relations
            .iter()
            .filter(|candidate| candidate.primal == relation.primal)
            .count()
            > 1
        {
            return Err(wire_invalid(format!(
                "relation rows for primal {} are duplicate or contradictory",
                relation.primal
            )));
        }
    }
    Ok(())
}

/// Rule F5/F6: initialization facts agree per buffer (independent axis);
/// every observation names a real launch; results are declared observation
/// points — every result row carries an explicit observation fact whose
/// boundary is the producing launch (a result whose observation contradicts
/// its producer is "without an explicit observation fact" and fails closed).
fn validate_wire_axes(
    wire: &WireDeviceProgram,
    launches: &[u32],
) -> Result<(), crate::error::FmirImageError> {
    // Init policies per buffer must agree (F5: the axis is a single fact,
    // never derived from role/lifetime).
    for resource in wire
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
    {
        let policy = resource.initialization;
        if wire_buffer_init_policy(wire, resource.buffer.id).is_some_and(|first| first != policy) {
            return Err(wire_invalid(format!(
                "buffer {} declares conflicting initialization policies",
                resource.buffer.id
            )));
        }
    }
    for result in &wire.results {
        if !launches.contains(&result.observation.at_launch) {
            return Err(wire_invalid(format!(
                "result on buffer {} names observation launch {}, which does not exist",
                result.buffer.id, result.observation.at_launch
            )));
        }
        // F6: the observation fact is the result's explicit declaration —
        // its completion boundary must be the producing launch. A result
        // whose observation contradicts its producer is a writable
        // intermediate exposed without an explicit observation fact.
        if result.observation.at_launch != result.produced_by {
            return Err(wire_invalid(format!(
                "result on buffer {} names producing launch {}, but its observation fact names launch {}; a result is a declared observation point at its producing launch",
                result.buffer.id, result.produced_by, result.observation.at_launch
            )));
        }
        // S5A-U1 (A7): the declared observation cadence must be consistent
        // with the buffer's lifetime class — a PerStep readback names an
        // ObservationPoint buffer (read back every step, read-then-release);
        // an EndOfRun readback names a PerStep or PerProgram buffer (kept
        // live through the final step). A cadence that contradicts the
        // lifetime is rejected before host construction — the wire's
        // declared cadence is the only authority for readback timing, never
        // a shape/role coincidence.
        match result.observation.cadence {
            WireObservationCadence::PerStep
                if result.buffer.lifetime != WireBufferLifetime::ObservationPoint =>
            {
                return Err(wire_invalid(format!(
                    "result on buffer {} declares a per-step observation cadence, but the buffer's lifetime class is {}; a per-step readback names an observation-point buffer",
                    result.buffer.id,
                    result.buffer.lifetime.spelling()
                )));
            }
            WireObservationCadence::EndOfRun
                if !matches!(
                    result.buffer.lifetime,
                    WireBufferLifetime::PerStep | WireBufferLifetime::PerProgram
                ) =>
            {
                return Err(wire_invalid(format!(
                    "result on buffer {} declares an end-of-run observation cadence, but the buffer's lifetime class is {}; an end-of-run readback names a per-step or per-program buffer",
                    result.buffer.id,
                    result.buffer.lifetime.spelling()
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// The distinct buffers referenced by the program, in first-reference order.
fn wire_buffers(wire: &WireDeviceProgram) -> Vec<WireBufferIdentity> {
    let mut buffers: Vec<WireBufferIdentity> = Vec::new();
    for kernel in &wire.kernels {
        for resource in &kernel.resources {
            if !buffers.iter().any(|buffer| buffer.id == resource.buffer.id) {
                buffers.push(resource.buffer.clone());
            }
        }
    }
    for result in &wire.results {
        if !buffers.iter().any(|buffer| buffer.id == result.buffer.id) {
            buffers.push(result.buffer.clone());
        }
    }
    buffers
}

/// The first initialization policy declared for a buffer, if any.
fn wire_buffer_init_policy(
    wire: &WireDeviceProgram,
    buffer: u32,
) -> Option<WireInitializationPolicy> {
    wire.kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .find(|resource| resource.buffer.id == buffer)
        .map(|resource| resource.initialization)
}

/// A focused `WireProgramInvalid` admission error.
fn wire_invalid(detail: String) -> crate::error::FmirImageError {
    crate::error::FmirImageError::WireProgramInvalid { detail }
}

// ---------------------------------------------------------------------------
// Wire-level distributed-section validation (MD2-W1)
// ---------------------------------------------------------------------------

/// Wire-level validation of the optional distributed execution section
/// (MD2-W1): the declared placement facts must **resolve against the carried
/// single-device program** — every fact names a declared F1 semantic value
/// and a referenced device buffer, and every byte contract is positive; and
/// the declared `logical_distributed_plan_hash` must be the canonical
/// `sha256:<64-hex>` spelling (MD2-H1). Plan-internal consistency is the
/// fail-before-launch gate's (MD2-V1), run on the reconstructed plan at
/// admission — this module checks only the facts that live on the wire
/// section and reference the program's own wire surface.
pub(crate) fn validate_wire_distributed_section(
    section: &FmirDistributedExecutionSection,
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    // MD2-H1 canonical format: `sha256:` + 64 hex.
    if !section
        .logical_distributed_plan_hash
        .starts_with(SHA256_IDENTITY_PREFIX)
    {
        return Err(distributed_invalid(format!(
            "distributed section declares a logical-plan hash without the `{SHA256_IDENTITY_PREFIX}` prefix ({}); the hash spelling is frozen (MD2-H1)",
            section.logical_distributed_plan_hash
        )));
    }
    let digest = &section.logical_distributed_plan_hash[SHA256_IDENTITY_PREFIX.len()..];
    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(distributed_invalid(format!(
            "distributed section declares a logical-plan hash with a non-canonical digest ({}); expected `sha256:` + 64 hex digits",
            section.logical_distributed_plan_hash
        )));
    }

    for fact in &section.declared_placement {
        if !wire
            .semantic_values
            .iter()
            .any(|value| value.id == fact.value)
        {
            return Err(distributed_invalid(format!(
                "declared placement fact names semantic value {}, which the single-device program never declares (F1)",
                fact.value
            )));
        }
        if !wire_buffers(wire)
            .iter()
            .any(|buffer| buffer.id == fact.buffer)
        {
            return Err(distributed_invalid(format!(
                "declared placement fact names buffer {}, which the single-device program never references",
                fact.buffer
            )));
        }
        if fact.byte_count == 0 {
            return Err(distributed_invalid(format!(
                "declared placement fact for semantic value {} carries byte count 0 — no byte/shape contract (exit-gate bullet 2)",
                fact.value
            )));
        }
    }
    Ok(())
}

/// A focused `DistributedSectionInvalid` admission error.
fn distributed_invalid(detail: String) -> crate::error::FmirImageError {
    crate::error::FmirImageError::DistributedSectionInvalid { detail }
}
