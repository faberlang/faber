use super::device::FmirSessionSection;
use super::distributed::FmirDistributedExecutionSection;
use super::wire::{
    WireBroadcastDeclaration, WireBufferIdentity, WireBufferLifetime, WireCollectionKernelPlan,
    WireDeviceProgram, WireInitializationPolicy, WireInputUpdateCadence, WireInvocationMode,
    WireKvCacheDtype, WireObservationCadence, WireProgramLifetime, WireReduceOp,
    WireReducedProjection, WireResourceAccess, WireSessionObservationCadence,
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
/// step count + the S6-C2 broadcast fact + the council-8 CB-3 reduced-resource
/// projection fact).
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
    validate_wire_reduced_projections(wire)?;
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

/// Rule council-8 CB-3 (A7): every carried reduced-resource projection is
/// admitted fail-closed at the decode boundary — the producer-defined
/// mapping is carried, never reconstructed, so a producer bug stamping an
/// impossible projection must fail here (the first keep-dims consumer
/// trusting the carried fact would otherwise hit a division-by-zero or
/// silent mis-addressing). Three sub-rules:
///
/// 1. **Positivity** — every carried projection (the reduced buffer
///    version's AND the `AxisReduction` plan's) has `axis_extent >= 1` and
///    `inner_stride >= 1` (a zero extent/stride is not a mapping);
/// 2. **Plan ↔ buffer-version agreement** — when an `AxisReduction` plan's
///    reduced output buffer version also carries a projection, the two MUST
///    be identical (the plan projection and the buffer version stamp the
///    SAME producer fact);
/// 3. **Divisibility** — a reduced buffer's `element_count` is exactly
///    divisible by its carried projection's `inner_stride` (the keep-dims
///    mapping is `axis_extent` strides of `inner_stride`; a non-divisible
///    count means the carried fact cannot address the buffer);
/// 4. **Saturation (council-13 cxo CB-W8)** — the block width
///    `axis_extent · inner_stride` must be representable in `u64` (the
///    keep-dims consumer divides the flat index by it; an overflowing width
///    silently saturates and collapses every keep-dims element onto the first
///    `inner_stride` slots — silent mis-addressing), and a projection-carrying
///    reduced buffer must hold at least one element (a zero-count carrier
///    cannot hold any real reduction output and passes divisibility
///    vacuously);
/// 5. **Operator (council-13 cxo CB-W8)** — an `AxisReduction` plan admits
///    `Sum` only: the `TensorSumAxis` resolution and every axis-reduction
///    emitter (the Metal emitter rejects non-`Sum` explicitly) cover `Sum`
///    today, so a wire image carrying `Mean`/`Product`/`Max`/`Min` would
///    pass decode and hit an unimplemented emitter path.
fn validate_wire_reduced_projections(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    for resource in wire
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
    {
        let Some(projection) = resource.version.reduced_projection else {
            continue;
        };
        validate_carried_reduced_projection(
            &format!("buffer '{}'", resource.buffer.name),
            resource.version.element_count,
            projection,
        )?;
    }
    for result in &wire.results {
        let Some(projection) = result.version.reduced_projection else {
            continue;
        };
        validate_carried_reduced_projection(
            &format!("result buffer '{}'", result.buffer.name),
            result.version.element_count,
            projection,
        )?;
    }
    for kernel in &wire.kernels {
        let WireCollectionKernelPlan::AxisReduction(plan) = &kernel.plan else {
            continue;
        };
        // council-13 cxo CB-W8: only `Sum` is emitted today — the
        // `TensorSumAxis` resolution (S6-P1) and every axis-reduction
        // emitter (the Metal emitter rejects non-`Sum` explicitly) cover
        // `Sum` only, so a non-`Sum` wire op would pass decode and then hit
        // an unimplemented emitter path. `Mean` is deliberately
        // tree-reduction-only today; the day an emitter lands a non-`Sum`
        // axis-reduction op, extend THIS allow-list in lockstep with the
        // emitter (and the faber boundary admission).
        if plan.op != WireReduceOp::Sum {
            return Err(wire_invalid(format!(
                "kernel '{}' carries an axis-reduction plan with op {:?}; only Sum is emitted today, a non-Sum op would hit an unimplemented emitter path (council-13 cxo CB-W8)",
                kernel.entry, plan.op
            )));
        }
        let projection = plan.projection;
        if projection.axis_extent < 1 || projection.inner_stride < 1 {
            return Err(wire_invalid(format!(
                "kernel '{}' carries an axis-reduction plan with a non-positive reduced-resource projection ({projection:?}); the mapping is carried positive (council-8 CB-3)",
                kernel.entry
            )));
        }
        // council-13 cxo CB-W8: the block width must be representable — the
        // keep-dims consumer divides by it and an overflowing width would
        // silently saturate to `u64::MAX`, collapsing the addressing.
        if projection
            .axis_extent
            .checked_mul(projection.inner_stride)
            .is_none()
        {
            return Err(wire_invalid(format!(
                "kernel '{}' carries an axis-reduction plan whose reduced-resource projection {projection:?} saturates u64 (axis_extent · inner_stride overflows); a saturated block width cannot address a real reduction (council-13 cxo CB-W8)",
                kernel.entry
            )));
        }
        for resource in &kernel.resources {
            if !matches!(
                resource.access,
                WireResourceAccess::Write | WireResourceAccess::ReadWrite
            ) {
                continue;
            }
            if let Some(carried) = resource.version.reduced_projection {
                if carried != projection {
                    return Err(wire_invalid(format!(
                        "kernel '{}' carries an axis-reduction plan projection {projection:?} that disagrees with the reduced output buffer's carried projection {carried:?}; the same fact must agree everywhere it is stamped (council-8 CB-3)",
                        kernel.entry
                    )));
                }
            }
        }
    }
    Ok(())
}

/// council-8 CB-3 + council-13 cxo CB-W8: one carried reduced-resource
/// projection must be positive, its reduced buffer's element count exactly
/// divisible by its inner stride — checked BEFORE any arithmetic on the
/// stride, so a zero stride is rejected rather than panicking the
/// divisibility modulus — and neither degenerate nor saturated (a
/// zero-element reduced buffer cannot hold a real reduction output, and a
/// block width `axis_extent · inner_stride` that overflows `u64` would
/// silently saturate the keep-dims consumer's addressing).
fn validate_carried_reduced_projection(
    carrier: &str,
    element_count: u64,
    projection: WireReducedProjection,
) -> Result<(), crate::error::FmirImageError> {
    if projection.axis_extent < 1 || projection.inner_stride < 1 {
        return Err(wire_invalid(format!(
            "{carrier} carries a reduced-resource projection with a non-positive axis extent or inner stride ({projection:?}); the mapping is carried positive (council-8 CB-3)"
        )));
    }
    // council-13 cxo CB-W8: a zero-element reduced buffer cannot address a
    // real reduction (a keep-dims consumer has nothing to read), and it
    // passes the divisibility rule vacuously (`0 % inner_stride == 0`).
    if element_count < 1 {
        return Err(wire_invalid(format!(
            "{carrier} carries a reduced-resource projection on a zero-element buffer ({element_count} elements, {projection:?}); a zero-count reduced buffer cannot address a real reduction (council-13 cxo CB-W8)"
        )));
    }
    // council-13 cxo CB-W8: the block width must be representable — the
    // keep-dims consumer divides the flat index by `axis_extent · inner_stride`
    // (a saturating multiply would silently collapse every keep-dims element
    // onto the first `inner_stride` slots).
    if projection
        .axis_extent
        .checked_mul(projection.inner_stride)
        .is_none()
    {
        return Err(wire_invalid(format!(
            "{carrier} carries a reduced-resource projection whose block width saturates u64 ({projection:?}: axis_extent · inner_stride overflows); a saturated block width cannot address a real reduction (council-13 cxo CB-W8)"
        )));
    }
    if element_count % projection.inner_stride != 0 {
        return Err(wire_invalid(format!(
            "{carrier} element count {element_count} is not divisible by its carried projection's inner stride {} ({projection:?}); the keep-dims mapping cannot address the buffer (council-8 CB-3)",
            projection.inner_stride
        )));
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

// ---------------------------------------------------------------------------
// Wire-level session-section validation (GI4-2)
// ---------------------------------------------------------------------------

/// Wire-level validation of the optional cadence/session section (GI4-2):
/// the carried session facts are admitted fail-closed against the carried
/// single-device program — the equivalence-gate discipline (admission
/// inspects the actually-delivered session facts, never a self-declared
/// hash). Rules, in order:
///
/// 1. **KV layout dims** — every dimension is positive (a zero
///    slot/context/layer/head layout is not a layout; it would silently
///    produce a zero-byte KV — the `KvCacheLayout::new` mirror);
/// 2. **KV dtype** — closed to `f32` (a dtype change is a contract revision,
///    never a silent widening);
/// 3. **invocation modes** — non-empty and duplicate-free (a session that
///    executes no workload mode is a contradiction);
/// 4. **session-input slots** — unique, and every slot resolves against a
///    buffer the carried single-device program actually declares;
/// 5. **cadence vs buffer facts** — a `Resident` input names a `PerProgram`
///    buffer (a resident input is allocated once for the whole session), and
///    a once-init (`PerProgram` + `HostProvided`) buffer named as a session
///    input is `Resident` (a per-invocation cadence on a once-init buffer is
///    the `SingleRun` re-copy semantic the GI4-1 discovery proved
///    infeasible);
/// 6. **observation cadence** — a session executing `scalar_decode` declares
///    a per-invocation observation cadence (per-token decode reads back its
///    observation per token).
pub(crate) fn validate_wire_session_section(
    section: &FmirSessionSection,
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    // Rule 1: the KV layout dims are all positive.
    let kv = &section.kv_layout;
    if kv.slots == 0 {
        return Err(session_invalid(
            "kv layout declares `slots` 0; a zero-slot layout is not a layout (it would silently produce a zero-byte KV)".to_owned(),
        ));
    }
    if kv.context_length == 0 {
        return Err(session_invalid(
            "kv layout declares `context_length` 0; a zero-context layout is not a layout (it would silently produce a zero-byte KV)".to_owned(),
        ));
    }
    if kv.layer_count == 0 {
        return Err(session_invalid(
            "kv layout declares `layer_count` 0; a zero-layer layout is not a layout (it would silently produce a zero-byte KV)".to_owned(),
        ));
    }
    if kv.kv_head_count == 0 {
        return Err(session_invalid(
            "kv layout declares `kv_head_count` 0; a zero-head layout is not a layout (it would silently produce a zero-byte KV)".to_owned(),
        ));
    }
    if kv.head_dim == 0 {
        return Err(session_invalid(
            "kv layout declares `head_dim` 0; a zero-dim layout is not a layout (it would silently produce a zero-byte KV)".to_owned(),
        ));
    }
    // Rule 2: the dtype is closed.
    if !matches!(kv.dtype, WireKvCacheDtype::F32) {
        return Err(session_invalid(format!(
            "kv layout declares dtype {kv:?}; the dtype surface is closed to f32 (a dtype change is a contract revision, never a silent widening)"
        )));
    }
    // Rule 3: at least one workload mode, no duplicates.
    if section.invocation_modes.is_empty() {
        return Err(session_invalid(
            "session declares no invocation mode; a session that executes no workload mode is a contradiction"
                .to_owned(),
        ));
    }
    for (index, mode) in section.invocation_modes.iter().enumerate() {
        if section.invocation_modes[index + 1..].contains(mode) {
            return Err(session_invalid(format!(
                "session declares invocation mode {} more than once",
                mode.spelling()
            )));
        }
    }
    // Rule 4: unique session-input slots that resolve against the carried
    // program.
    let buffers = wire_buffers(wire);
    for (index, input) in section.inputs.iter().enumerate() {
        if section.inputs[index + 1..]
            .iter()
            .any(|other| other.slot == input.slot)
        {
            return Err(session_invalid(format!(
                "session declares input slot {} more than once; one cadence per slot",
                input.slot
            )));
        }
        if !buffers.iter().any(|buffer| buffer.id == input.slot) {
            return Err(session_invalid(format!(
                "session input slot {} does not resolve against a buffer the carried single-device program declares",
                input.slot
            )));
        }
    }
    // Rule 5: cadence vs the carried buffer facts.
    for input in &section.inputs {
        let Some(buffer) = buffers.iter().find(|buffer| buffer.id == input.slot) else {
            continue;
        };
        match input.cadence {
            WireInputUpdateCadence::Resident
                if buffer.lifetime != WireBufferLifetime::PerProgram =>
            {
                return Err(session_invalid(format!(
                    "session input slot {} (buffer '{}') declares cadence resident, but the buffer is {}; a resident input is uploaded once at session creation and lives for the session (per-program)",
                    input.slot,
                    buffer.name,
                    buffer.lifetime.spelling()
                )));
            }
            WireInputUpdateCadence::PerInvocation
                if buffer.lifetime == WireBufferLifetime::PerProgram
                    && wire_buffer_init_policy(wire, buffer.id)
                        == Some(WireInitializationPolicy::HostProvided) =>
            {
                return Err(session_invalid(format!(
                    "session input slot {} (buffer '{}') declares cadence per_invocation, but the buffer is a once-init per-program host-provided buffer; a per-invocation cadence on a once-init buffer is the SingleRun re-copy semantic the GI4-1 discovery proved infeasible (weights would re-upload per invocation)",
                    input.slot,
                    buffer.name
                )));
            }
            _ => {}
        }
    }
    // Rule 6: a scalar-decode session observes per invocation.
    if section
        .invocation_modes
        .contains(&WireInvocationMode::ScalarDecode)
        && section.observation_cadence != WireSessionObservationCadence::PerInvocation
    {
        return Err(session_invalid(format!(
            "session executes {} (a one-token decode step) but declares observation cadence {}; per-token decode reads back its observation per token (the full-vocab logits are sampled host-side on every invocation)",
            WireInvocationMode::ScalarDecode.spelling(),
            section.observation_cadence.spelling()
        )));
    }
    Ok(())
}

/// A focused `SessionSectionInvalid` admission error.
fn session_invalid(detail: String) -> crate::error::FmirImageError {
    crate::error::FmirImageError::SessionSectionInvalid { detail }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The wire types the fixture names directly (the glob above resolves the
    // validation surface from validate.rs's own imports; the schema's
    // `pub use wire::*` re-exports are reachable through the explicit path).
    use super::super::wire::{
        WireAxisReductionPlan, WireBinding, WireBufferIdentity, WireBufferLifetime, WireBufferRole,
        WireBufferVersion, WireCollectionKernelPlan, WireDeviceProgram, WireDeviceResource,
        WireDispatchSize, WireInitializationPolicy, WireKernelLaunchPlan, WireKernelUnit,
        WireLaunchUnit, WireObservationCadence, WireObservationFact, WireProgramLifetime,
        WireReduceOp, WireReducedProjection, WireResourceAccess, WireResultBuffer,
        WireSemanticValue, WireSemanticValueOrigin, WireStorageLayout, WireWorkgroupCount,
        WireWorkgroupSize,
    };

    /// The council-8 CB-3 fixture: a one-kernel axis-reduction program
    /// (`[8, 8]` → 8 elements along axis 0, `Sum`). The reduced output
    /// buffer version and the `AxisReduction` plan carry the SAME
    /// producer-defined projection (`axis_extent` 8, `inner_stride` 8);
    /// the reduced element count (8) is divisible by the stride; and the
    /// F1–F7 facts mirror the session-section fixture (a read
    /// host-provided input, a write kernel-initialized observation-point
    /// output, one root) so the program passes `validate_wire_program`
    /// apart from the projection under test.
    fn reduced_projection_fixture() -> WireDeviceProgram {
        let w = WireBufferIdentity {
            id: 1,
            name: "w".to_owned(),
            role: WireBufferRole::Input,
            storage: WireStorageLayout::DeviceHandle,
            lifetime: WireBufferLifetime::PerProgram,
            semantic_value: 1,
        };
        let o = WireBufferIdentity {
            id: 2,
            name: "o".to_owned(),
            role: WireBufferRole::Output,
            storage: WireStorageLayout::DeviceHandle,
            lifetime: WireBufferLifetime::ObservationPoint,
            semantic_value: 2,
        };
        let v_w = WireBufferVersion {
            version: 1,
            element_ty: "f32".to_owned(),
            element_count: 64,
            reduced_projection: None,
        };
        let v_o = WireBufferVersion {
            version: 1,
            element_ty: "f32".to_owned(),
            element_count: 8,
            reduced_projection: Some(WireReducedProjection {
                axis_extent: 8,
                inner_stride: 8,
            }),
        };
        let launch = WireKernelLaunchPlan {
            workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
            dispatch_size: WireDispatchSize { x: 1, y: 1, z: 1 },
            workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
        };
        WireDeviceProgram {
            kernels: vec![WireKernelUnit {
                function: 0,
                entry: "reduce".to_owned(),
                plan: WireCollectionKernelPlan::AxisReduction(WireAxisReductionPlan {
                    op: WireReduceOp::Sum,
                    axis: 0,
                    projection: WireReducedProjection {
                        axis_extent: 8,
                        inner_stride: 8,
                    },
                }),
                resources: vec![
                    WireDeviceResource {
                        buffer: w.clone(),
                        version: v_w.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 0,
                        },
                        access: WireResourceAccess::Read,
                        generation: 1,
                        initialization: WireInitializationPolicy::HostProvided,
                    },
                    WireDeviceResource {
                        buffer: o.clone(),
                        version: v_o.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 1,
                        },
                        access: WireResourceAccess::Write,
                        generation: 1,
                        initialization: WireInitializationPolicy::KernelInitialized,
                    },
                ],
                launch,
            }],
            launches: vec![WireLaunchUnit {
                id: 1,
                kernel_index: 0,
            }],
            lifetime: WireProgramLifetime::SingleRun,
            results: vec![WireResultBuffer {
                buffer: o.clone(),
                version: v_o.clone(),
                role: WireBufferRole::Output,
                produced_by: 1,
                observation: WireObservationFact {
                    at_launch: 1,
                    cadence: WireObservationCadence::PerStep,
                },
            }],
            semantic_values: vec![
                WireSemanticValue {
                    id: 1,
                    name: "w".to_owned(),
                    origin: WireSemanticValueOrigin::HostInput,
                },
                WireSemanticValue {
                    id: 2,
                    name: "o".to_owned(),
                    origin: WireSemanticValueOrigin::Synthetic {
                        label: "reduce-output".to_owned(),
                    },
                },
            ],
            roots: vec![1],
            dependencies: Vec::new(),
            relations: Vec::new(),
        }
    }

    /// council-8 CB-3: a fully consistent reduced-resource projection (plan
    /// and buffer version agree, both positive, element count divisible by
    /// the stride) admits at the decode boundary.
    #[test]
    fn valid_reduced_projection_admits() {
        validate_wire_program(&reduced_projection_fixture())
            .expect("a consistent reduced-resource projection admits fail-closed");
    }

    /// council-8 CB-3: a non-positive carried projection fails closed — a
    /// zero `axis_extent` on the plan and a zero `inner_stride` on the
    /// carried buffer-version projection (which would panic the divisibility
    /// modulus if it reached it) both reject with the positivity diagnostic.
    #[test]
    fn zero_reduced_projection_extent_rejected() {
        let mut wire = reduced_projection_fixture();
        let WireCollectionKernelPlan::AxisReduction(plan) = &mut wire.kernels[0].plan else {
            panic!("fixture carries an axis-reduction plan");
        };
        plan.projection.axis_extent = 0;
        let error = validate_wire_program(&wire)
            .expect_err("a zero axis extent fails the projection admission closed");
        assert!(
            error.to_string().contains("non-positive"),
            "the fail-closed diagnostic names the positivity rule: {error}"
        );

        let mut wire = reduced_projection_fixture();
        wire.kernels[0].resources[1].version.reduced_projection = Some(WireReducedProjection {
            axis_extent: 8,
            inner_stride: 0,
        });
        let error = validate_wire_program(&wire)
            .expect_err("a zero inner stride fails the projection admission closed");
        assert!(
            error.to_string().contains("non-positive"),
            "the fail-closed diagnostic names the positivity rule: {error}"
        );
    }

    /// council-8 CB-3: a plan projection that disagrees with the reduced
    /// output buffer version's carried projection fails closed — the SAME
    /// producer fact must agree everywhere it is stamped.
    #[test]
    fn mismatched_plan_vs_buffer_projection_rejected() {
        let mut wire = reduced_projection_fixture();
        let WireCollectionKernelPlan::AxisReduction(plan) = &mut wire.kernels[0].plan else {
            panic!("fixture carries an axis-reduction plan");
        };
        plan.projection.inner_stride = 4;
        let error = validate_wire_program(&wire)
            .expect_err("a plan projection that disagrees with the buffer version fails closed");
        assert!(
            error.to_string().contains("disagrees"),
            "the fail-closed diagnostic names the plan-vs-buffer mismatch: {error}"
        );
    }

    /// council-8 CB-3: a reduced buffer whose element count is not exactly
    /// divisible by its carried projection's inner stride fails closed — the
    /// keep-dims mapping (`axis_extent` strides of `inner_stride`) cannot
    /// address the buffer.
    #[test]
    fn non_divisible_reduced_element_count_rejected() {
        let mut wire = reduced_projection_fixture();
        wire.kernels[0].resources[1].version.element_count = 9;
        let error = validate_wire_program(&wire)
            .expect_err("a non-divisible reduced element count fails the projection admission");
        assert!(
            error.to_string().contains("not divisible"),
            "the fail-closed diagnostic names the divisibility rule: {error}"
        );
    }

    /// council-13 cxo CB-W8: an axis-reduction plan admitting only `Sum`
    /// fails closed on a non-`Sum` op (`Mean` is tree-reduction-only today;
    /// every axis-reduction emitter covers `Sum` only, so a non-`Sum` wire
    /// op would pass decode and hit an unimplemented emitter path).
    #[test]
    fn non_sum_axis_reduction_op_rejected() {
        let mut wire = reduced_projection_fixture();
        let WireCollectionKernelPlan::AxisReduction(plan) = &mut wire.kernels[0].plan else {
            panic!("fixture carries an axis-reduction plan");
        };
        plan.op = WireReduceOp::Mean;
        let error = validate_wire_program(&wire)
            .expect_err("a non-Sum axis-reduction op fails the admission closed");
        assert!(
            error.to_string().contains("only Sum"),
            "the fail-closed diagnostic names the op allow-list: {error}"
        );
    }

    /// council-13 cxo CB-W8: a saturated projection — a block width
    /// `axis_extent · inner_stride` that overflows `u64` — fails closed (the
    /// keep-dims consumer divides by the width; a saturating multiply would
    /// silently collapse the addressing). Both the plan and the carried
    /// buffer/result projections are stamped identically so the agreement
    /// rule does not shadow the saturation rule.
    #[test]
    fn saturated_overflowing_projection_rejected() {
        let mut wire = reduced_projection_fixture();
        let WireCollectionKernelPlan::AxisReduction(plan) = &mut wire.kernels[0].plan else {
            panic!("fixture carries an axis-reduction plan");
        };
        let saturated = WireReducedProjection {
            axis_extent: u64::MAX,
            inner_stride: 2,
        };
        plan.projection = saturated;
        wire.kernels[0].resources[1].version.reduced_projection = Some(saturated);
        wire.kernels[0].resources[1].version.element_count = u64::MAX;
        wire.results[0].version.reduced_projection = Some(saturated);
        wire.results[0].version.element_count = u64::MAX;
        let error = validate_wire_program(&wire)
            .expect_err("an overflowing block width fails the projection admission");
        assert!(
            error.to_string().contains("saturates u64"),
            "the fail-closed diagnostic names the saturation rule: {error}"
        );
    }

    /// council-13 cxo CB-W8: a degenerate zero-element reduced buffer
    /// carrying a projection fails closed — a zero-count carrier cannot hold
    /// any real reduction output (and passes the divisibility rule
    /// vacuously, so it must be rejected before it).
    #[test]
    fn zero_element_reduced_buffer_rejected() {
        let mut wire = reduced_projection_fixture();
        wire.kernels[0].resources[1].version.element_count = 0;
        let error = validate_wire_program(&wire)
            .expect_err("a zero-element reduced buffer fails the projection admission");
        assert!(
            error.to_string().contains("zero-element"),
            "the fail-closed diagnostic names the degenerate-count rule: {error}"
        );
    }
}
