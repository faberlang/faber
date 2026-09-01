//! Wire-level semantic admission for FMIR device programs.
//!
//! These checks run on the serialized wire form at the decode boundary and
//! fail closed on the first violation. They never reconstruct missing facts
//! or proceed to host construction. Plan-internal consistency of a
//! reconstructed distributed plan belongs to the fail-before-launch gate.

use super::SHA256_IDENTITY_PREFIX;
use super::device::{FmirSessionSection, FmirSessionSectionV2};
use super::distributed::FmirDistributedExecutionSection;
use super::wire::{
    WireBroadcastDeclaration, WireBufferIdentity, WireBufferLifetime, WireBufferRole,
    WireCollectionKernelPlan, WireComposedMatMulStageKind, WireDeviceProgram,
    WireInitializationPolicy, WireInputUpdateCadence, WireInvocationMode,
    WireKvCacheCapacityPolicy, WireKvCacheDtype, WireKvCacheRopePairing, WireKvCacheSemanticV2,
    WireMatMulLayout, WireObservationCadence, WireProgramLifetime, WireReduceOp,
    WireReducedProjection, WireResourceAccess, WireSemanticValueOrigin, WireSessionInput,
    WireSessionObservationCadence, WireStorageLayout,
};

// ---------------------------------------------------------------------------
// Device program
// ---------------------------------------------------------------------------

/// Admit the carried F1–F7 facts, declared step count, broadcast facts, and
/// reduced-resource projections.
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
    validate_wire_composed_matmul_plans(wire)?;
    validate_wire_reduced_projections(wire)?;
    Ok(())
}

/// A carried `RankExtensionAdd` fact must be internally consistent: the
/// higher-rank operand is exactly one rank above the lower-rank operand,
/// the lower-rank dims equal the higher-rank trailing dims, and the result
/// is the higher-rank shape. The fact is carried, never inferred.
fn validate_wire_broadcast_facts(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    for kernel in &wire.kernels {
        let WireCollectionKernelPlan::RankExtensionAdd(fact) = &kernel.plan else {
            continue;
        };
        let (higher, lower) = if fact.lhs_shape.len() >= fact.rhs_shape.len() {
            (&fact.lhs_shape, &fact.rhs_shape)
        } else {
            (&fact.rhs_shape, &fact.lhs_shape)
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

/// Validate the composed gate/up/SwiGLU/down plan and its typed stage
/// resources before FMIR admission. The wire carries all facts explicitly;
/// this function never reconstructs them from names, counts, or emitted text.
fn validate_wire_composed_matmul_plans(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    for kernel in &wire.kernels {
        let WireCollectionKernelPlan::ComposedMatMul(plan) = &kernel.plan else {
            continue;
        };
        if plan.stages.len() != 4 || plan.edges.len() != 3 {
            return Err(wire_invalid(format!(
                "kernel '{}' carries an incomplete composed matmul plan ({} stages, {} edges)",
                kernel.entry,
                plan.stages.len(),
                plan.edges.len()
            )));
        }
        let expected = [
            WireComposedMatMulStageKind::Gate,
            WireComposedMatMulStageKind::Up,
            WireComposedMatMulStageKind::SwiGlu,
            WireComposedMatMulStageKind::Down,
        ];
        let mut locals = std::collections::BTreeSet::new();
        let mut used_resources = std::collections::BTreeSet::new();
        for (index, (stage, expected_kind)) in plan.stages.iter().zip(expected).enumerate() {
            let expected_access = if index == 3 {
                WireResourceAccess::Write
            } else {
                WireResourceAccess::ReadWrite
            };
            if stage.kind != expected_kind
                || stage.output.element_count == 0
                || stage.output.element_ty != WireMatMulLayout::F32
                || stage.output.access != expected_access
                || !locals.insert(stage.output.source_local)
            {
                return Err(wire_invalid(format!(
                    "kernel '{}' carries invalid composed matmul stage {index}",
                    kernel.entry
                )));
            }
            match index {
                0 | 1 | 3 => {
                    let Some(shape) = stage.shape else {
                        return Err(wire_invalid(format!(
                            "kernel '{}' composed matmul stage {index} has no shape",
                            kernel.entry
                        )));
                    };
                    let Some(output_extent) = shape.m.checked_mul(shape.n) else {
                        return Err(wire_invalid(format!(
                            "kernel '{}' composed matmul stage {index} shape overflows its output extent",
                            kernel.entry
                        )));
                    };
                    if shape.m == 0
                        || shape.k == 0
                        || shape.n == 0
                        || shape.left_layout != WireMatMulLayout::F32
                        || shape.right_layout != WireMatMulLayout::F32
                        || output_extent != stage.output.element_count
                    {
                        return Err(wire_invalid(format!(
                            "kernel '{}' composed matmul stage {index} has invalid shape",
                            kernel.entry
                        )));
                    }
                }
                2 => {
                    if stage.shape.is_some()
                        || stage.output.element_count != plan.stages[0].output.element_count
                        || stage.output.element_count != plan.stages[1].output.element_count
                    {
                        return Err(wire_invalid(format!(
                            "kernel '{}' composed SwiGLU stage has mismatched extent",
                            kernel.entry
                        )));
                    }
                }
                _ => unreachable!("validated composed stage count"),
            }
            let expected_role = if index == 3 {
                WireBufferRole::Output
            } else {
                WireBufferRole::InOut
            };
            let resource = kernel.resources.iter().find(|resource| {
                resource.buffer.role == expected_role
                    && resource.buffer.storage == WireStorageLayout::DeviceHandle
                    && resource.version.element_count == stage.output.element_count
                    && resource.version.element_ty == "f32"
                    && resource.access == stage.output.access
                    && wire.semantic_values.iter().any(|value| {
                        value.id == resource.buffer.semantic_value
                            && matches!(
                                value.origin,
                                WireSemanticValueOrigin::MirLocal { function, local }
                                    if function == kernel.function
                                        && local == stage.output.source_local
                            )
                    })
                    && !used_resources.contains(&resource.buffer.id)
            });
            let Some(resource) = resource else {
                return Err(wire_invalid(format!(
                    "kernel '{}' composed stage {index} has no matching typed stage resource",
                    kernel.entry
                )));
            };
            used_resources.insert(resource.buffer.id);
        }
        for (index, edge) in plan.edges.iter().enumerate() {
            let producer = usize::from(edge.producer);
            let consumer = usize::from(edge.consumer);
            if producer != index || consumer != index + 1 || consumer >= plan.stages.len() {
                return Err(wire_invalid(format!(
                    "kernel '{}' composed edge {index} is not adjacent",
                    kernel.entry
                )));
            }
            let output = &plan.stages[producer].output;
            if edge.source_local != output.source_local
                || edge.element_ty != output.element_ty
                || edge.element_count != output.element_count
            {
                return Err(wire_invalid(format!(
                    "kernel '{}' composed edge {index} disagrees with its producer",
                    kernel.entry
                )));
            }
        }
        if !matches!(
            plan.handoff,
            super::wire::WireComposedMatMulHandoff::WorkgroupBarrier
        ) || kernel.launch.dispatch_size.x != 1
            || kernel.launch.dispatch_size.y != 1
            || kernel.launch.dispatch_size.z != 1
        {
            return Err(wire_invalid(format!(
                "kernel '{}' carries an unsupported composed matmul synchronization handoff",
                kernel.entry
            )));
        }
    }
    Ok(())
}

/// Admit every carried reduced-resource projection fail-closed.
///
/// The producer-defined mapping is carried, never reconstructed: extents
/// and strides must be positive, plan and write-slot stamps of the same
/// fact must agree, `element_count` must be a positive multiple of
/// `inner_stride`, `axis_extent · inner_stride` must fit in `u64` (the
/// keep-dims consumer divides by that width), and `AxisReduction` admits
/// `Sum` only.
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
        // Only `Sum` is emitted today. When an emitter lands another
        // axis-reduction op, extend this allow-list in lockstep with the
        // emitter and the faber boundary. `Mean` is tree-reduction-only.
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
            if let Some(carried) = resource.version.reduced_projection
                && carried != projection
            {
                return Err(wire_invalid(format!(
                    "kernel '{}' carries an axis-reduction plan projection {projection:?} that disagrees with the reduced output buffer's carried projection {carried:?}; the same fact must agree everywhere it is stamped (council-8 CB-3)",
                    kernel.entry
                )));
            }
        }
    }
    Ok(())
}

/// One carried projection must be positive, its buffer count a positive
/// multiple of `inner_stride`, and its block width representable in `u64`.
/// Positivity is checked before the modulus so a zero stride cannot panic.
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
    if element_count < 1 {
        return Err(wire_invalid(format!(
            "{carrier} carries a reduced-resource projection on a zero-element buffer ({element_count} elements, {projection:?}); a zero-count reduced buffer cannot address a real reduction (council-13 cxo CB-W8)"
        )));
    }
    if projection
        .axis_extent
        .checked_mul(projection.inner_stride)
        .is_none()
    {
        return Err(wire_invalid(format!(
            "{carrier} carries a reduced-resource projection whose block width saturates u64 ({projection:?}: axis_extent · inner_stride overflows); a saturated block width cannot address a real reduction (council-13 cxo CB-W8)"
        )));
    }
    if !element_count.is_multiple_of(projection.inner_stride) {
        return Err(wire_invalid(format!(
            "{carrier} element count {element_count} is not divisible by its carried projection's inner stride {} ({projection:?}); the keep-dims mapping cannot address the buffer (council-8 CB-3)",
            projection.inner_stride
        )));
    }
    Ok(())
}

/// A `RepeatingStep` program must declare at least one step. The image's
/// step count is the route's authority; a count the route could never drive
/// is rejected before host construction.
fn validate_wire_step_count(wire: &WireDeviceProgram) -> Result<(), crate::error::FmirImageError> {
    if let WireProgramLifetime::RepeatingStep(declared) = wire.lifetime
        && declared == 0
    {
        return Err(wire_invalid(
                "RepeatingStep program declares step count 0; a training program must declare at least one step".to_owned(),
            ));
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
    // Two distinct write/read-write slots may not claim the same
    // (value, generation).
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
    // A read consumes its generation; a read-write consumes the previous
    // generation. Every consumed generation is produced by a different slot
    // or covered by the buffer's initialization policy.
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
    // A generation producer hanging off the graph could never be scheduled.
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

/// Depth-first back-edge detection (F3).
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
    // F5: the init axis is a single fact, never derived from role/lifetime.
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
        // F6: a result whose observation contradicts its producer is a
        // writable intermediate exposed without an explicit observation fact.
        if result.observation.at_launch != result.produced_by {
            return Err(wire_invalid(format!(
                "result on buffer {} names producing launch {}, but its observation fact names launch {}; a result is a declared observation point at its producing launch",
                result.buffer.id, result.produced_by, result.observation.at_launch
            )));
        }
        // Declared cadence is the only authority for readback timing.
        // PerStep names an ObservationPoint; EndOfRun names PerStep or
        // PerProgram. Never infer timing from shape or role.
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

fn wire_invalid(detail: String) -> crate::error::FmirImageError {
    crate::error::FmirImageError::WireProgramInvalid { detail }
}

// ---------------------------------------------------------------------------
// Distributed section
// ---------------------------------------------------------------------------

/// Admit the optional distributed section against the carried single-device
/// program: every placement names a declared F1 value and a referenced
/// buffer, every byte contract is positive, and the logical-plan hash uses
/// the canonical `sha256:<64-hex>` spelling.
pub(crate) fn validate_wire_distributed_section(
    section: &FmirDistributedExecutionSection,
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
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

fn distributed_invalid(detail: String) -> crate::error::FmirImageError {
    crate::error::FmirImageError::DistributedSectionInvalid { detail }
}

// ---------------------------------------------------------------------------
// Session section
// ---------------------------------------------------------------------------

/// Admit the optional cadence/session section against the carried
/// single-device program. Admission inspects the delivered session facts,
/// never a self-declared hash.
pub(crate) fn validate_wire_session_section(
    section: &FmirSessionSection,
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
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
    if !matches!(kv.dtype, WireKvCacheDtype::F32) {
        return Err(session_invalid(format!(
            "kv layout declares dtype {kv:?}; the dtype surface is closed to f32 (a dtype change is a contract revision, never a silent widening)"
        )));
    }
    validate_session_schedule(
        &section.inputs,
        &section.invocation_modes,
        section.observation_cadence,
        wire,
    )
}

/// Admit the A4S v2 static semantic record and the shared session schedule.
///
/// The v2 carrier is intentionally separate from the legacy layout carrier:
/// no field is defaulted or inferred from `WireKvCacheLayout`. The semantic
/// version is checked before any model fact is interpreted, then every typed
/// fact is checked against the closed KV contract.
#[allow(dead_code)]
pub(crate) fn validate_wire_session_section_v2(
    section: &FmirSessionSectionV2,
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    validate_wire_kv_cache_semantic_v2(&section.semantic)?;
    validate_session_schedule(
        &section.inputs,
        &section.invocation_modes,
        section.observation_cadence,
        wire,
    )
}

#[allow(dead_code)]
fn validate_wire_kv_cache_semantic_v2(
    semantic: &WireKvCacheSemanticV2,
) -> Result<(), crate::error::FmirImageError> {
    if semantic.semantic_contract_version != super::wire::WIRE_KV_CACHE_SEMANTIC_CONTRACT_VERSION {
        return Err(session_invalid(format!(
            "KV semantic contract version {} is unsupported; expected {}",
            semantic.semantic_contract_version,
            super::wire::WIRE_KV_CACHE_SEMANTIC_CONTRACT_VERSION
        )));
    }
    if !matches!(semantic.kv_dtype_k, WireKvCacheDtype::F32)
        || !matches!(semantic.kv_dtype_v, WireKvCacheDtype::F32)
    {
        return Err(session_invalid(format!(
            "KV semantic record carries K dtype {} and V dtype {}; both distinct dtype facts must be present on the closed f32 surface",
            semantic.kv_dtype_k.spelling(),
            semantic.kv_dtype_v.spelling()
        )));
    }
    for (name, value) in [
        ("query_head_count", semantic.query_head_count),
        ("kv_head_count", semantic.kv_head_count),
        ("head_dim", semantic.head_dim),
        ("layer_count", semantic.layer_count),
        ("context_capacity", semantic.context_capacity),
    ] {
        if value == 0 {
            return Err(session_invalid(format!(
                "KV semantic record declares `{name}` 0; {name} must be positive"
            )));
        }
    }
    if !semantic
        .query_head_count
        .is_multiple_of(semantic.kv_head_count)
    {
        return Err(session_invalid(format!(
            "KV semantic record declares query_head_count {} not divisible by kv_head_count {}; GQA requires an integral query-to-KV head ratio",
            semantic.query_head_count, semantic.kv_head_count
        )));
    }
    if !matches!(semantic.capacity_policy, WireKvCacheCapacityPolicy::Fixed) {
        return Err(session_invalid(format!(
            "KV semantic record declares capacity policy {}; only fixed-capacity caches are admitted",
            semantic.capacity_policy.spelling()
        )));
    }
    if !semantic.rope_frequency_base.is_finite() || semantic.rope_frequency_base <= 0.0 {
        return Err(session_invalid(format!(
            "KV semantic record declares RoPE frequency base {}; it must be finite and positive",
            semantic.rope_frequency_base
        )));
    }
    if !semantic.rope_frequency_scale.is_finite() || semantic.rope_frequency_scale <= 0.0 {
        return Err(session_invalid(format!(
            "KV semantic record declares RoPE frequency scale {}; it must be finite and positive",
            semantic.rope_frequency_scale
        )));
    }
    if semantic.rope_dimension == 0 {
        return Err(session_invalid(
            "KV semantic record declares `rope_dimension` 0; the rotated dimension must be positive"
                .to_owned(),
        ));
    }
    if semantic.rope_dimension > semantic.head_dim {
        return Err(session_invalid(format!(
            "KV semantic record declares rope_dimension {} larger than head_dim {}; RoPE cannot address dimensions outside one head",
            semantic.rope_dimension, semantic.head_dim
        )));
    }
    let pairing_requires_pairs = match semantic.rope_pairing {
        WireKvCacheRopePairing::ConsecutivePair | WireKvCacheRopePairing::RotateHalf => true,
    };
    if pairing_requires_pairs && !semantic.rope_dimension.is_multiple_of(2) {
        return Err(session_invalid(format!(
            "KV semantic record declares rope_dimension {} with {} pairing; the typed pairing requires an even rotated dimension",
            semantic.rope_dimension,
            semantic.rope_pairing.spelling()
        )));
    }
    if !semantic.k_stored_post_rope {
        return Err(session_invalid(
            "KV semantic record declares cached K before RoPE; the contract requires K stored after RoPE"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_session_schedule(
    inputs: &[WireSessionInput],
    invocation_modes: &[WireInvocationMode],
    observation_cadence: WireSessionObservationCadence,
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    if invocation_modes.is_empty() {
        return Err(session_invalid(
            "session declares no invocation mode; a session that executes no workload mode is a contradiction"
                .to_owned(),
        ));
    }
    for (index, mode) in invocation_modes.iter().enumerate() {
        if invocation_modes[index + 1..].contains(mode) {
            return Err(session_invalid(format!(
                "session declares invocation mode {} more than once",
                mode.spelling()
            )));
        }
    }
    let buffers = wire_buffers(wire);
    for (index, input) in inputs.iter().enumerate() {
        if inputs[index + 1..]
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
    for input in inputs {
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
                    input.slot, buffer.name
                )));
            }
            _ => {}
        }
    }
    if invocation_modes.contains(&WireInvocationMode::ScalarDecode)
        && observation_cadence != WireSessionObservationCadence::PerInvocation
    {
        return Err(session_invalid(format!(
            "session executes {} (a one-token decode step) but declares observation cadence {}; per-token decode reads back its observation per token (the full-vocab logits are sampled host-side on every invocation)",
            WireInvocationMode::ScalarDecode.spelling(),
            observation_cadence.spelling()
        )));
    }
    Ok(())
}

fn session_invalid(detail: String) -> crate::error::FmirImageError {
    crate::error::FmirImageError::SessionSectionInvalid { detail }
}

#[cfg(test)]
#[path = "validate_test.rs"]
mod tests;
