//! Carried semantic value / generation / graph / relation model (3R contract
//! F1–F7).
//!
//! This module is the radix-mir single-writer authority for the **frozen
//! semantic contract** of the differentiable-GPU campaign (Stage 3R): stable
//! semantic value identity (F1), explicit value generations (F2), carried
//! roots + producer/consumer dependencies (F3), the lossless
//! primal/companion relation (F4), and the independent resource axes —
//! initialization and observation (F5/F6). Every fact here is **carried**,
//! never reconstructed from names, shapes, binding positions, or declaration
//! order.
//!
//! The radix-mir core `device_program::DeviceProgram` shape stays frozen
//! (the emitter crates construct it field-for-field); the semantic facts ride
//! alongside it in [`DeviceSemantics`], validated fail-closed by
//! [`DeviceSemantics::validate`]. The serialized wire mirror of these facts
//! lives in `radix-mir-fmir` (`WireDeviceProgram` sections), which runs the
//! same rules at the decode boundary before host construction or launch.
//!
//! The lossless companion relation ([`LosslessMirCompanionEntry`]) is the
//! carrier that survives AIR → MIR → device materialization → serialized
//! package (F4); [`crate::device::MirCompanionMap`] stores these entries with
//! fail-closed insertion (duplicate or contradictory rows error).

use crate::device::MirCompanionDerivativeKind;
use crate::device_program::{BufferId, DeviceProgramError, LaunchId};
use crate::{MirFunctionId, MirType};
use radix_hir::DefId;
use radix_types::TypeId;
use std::fmt;

/// Program-unique stable identity of one semantic device value (F1).
///
/// Identity is an opaque program fact minted by the materializer from carried
/// MIR/value facts — never from a name, shape, binding position, or
/// declaration coincidence. Two unrelated same-name/same-shape values have
/// distinct ids and never alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SemanticValueId(pub u32);

impl fmt::Display for SemanticValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SemanticValueId({})", self.0)
    }
}

/// The carried MIR/value fact a [`SemanticValueId`] is **derived from** (F1).
///
/// Two values with the same origin are the same value — minting two distinct
/// ids from one origin would silently alias, so the validator rejects it. A
/// rename changes only the diagnostic `name`, never the origin-derived
/// identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticValueOrigin {
    /// The value flows from a specific MIR local of a specific function.
    MirLocal {
        /// The MIR function that defines the local.
        function: MirFunctionId,
        /// The local storage slot (0 is the return slot).
        local: u32,
    },
    /// A host-provided input value, uploaded at program start.
    HostInput,
    /// A synthetic value minted by the materializer (e.g. a gradient slot);
    /// the label discriminates distinct synthetic values.
    Synthetic {
        /// The materializer's discriminative label for this synthetic value.
        label: String,
    },
}

impl SemanticValueOrigin {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(&self) -> String {
        match self {
            Self::MirLocal { function, local } => {
                format!("mir-local({}, {local})", function.0)
            }
            Self::HostInput => "host-input".to_owned(),
            Self::Synthetic { label } => format!("synthetic({label})"),
        }
    }
}

/// One semantic value: the stable identity plus the carried facts that
/// define it (F1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticValue {
    /// Program-unique stable identity.
    pub id: SemanticValueId,
    /// Diagnostic name (never an identity fact).
    pub name: String,
    /// Element type of the value's first generation.
    pub element_ty: MirType,
    /// Element count of the value's first generation.
    pub element_count: u64,
    /// The carried fact this identity is derived from.
    pub origin: SemanticValueOrigin,
}

/// One value → buffer binding (F1): a semantic value lives in exactly one
/// device buffer, and every buffer reference carries the value identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueBinding {
    /// The semantic value held by the buffer.
    pub value: SemanticValueId,
    /// The device buffer that holds the value.
    pub buffer: BufferId,
}

/// An explicit value generation (F2): one logical write of a value.
///
/// A generation is distinct from the shape/layout snapshot version on a
/// [`crate::device_program::BufferVersion`]; every logical write produces a
/// NEW generation, never a second producer of an existing one. Generation 0
/// is reserved as the unset sentinel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueGeneration {
    /// The value written.
    pub value: SemanticValueId,
    /// Monotonic generation number of the value (≥ 1).
    pub generation: u32,
    /// Element type of this generation's shape.
    pub element_ty: MirType,
    /// Element count of this generation's shape.
    pub element_count: u64,
    /// The launch whose kernel performs the logical write.
    pub produced_by: LaunchId,
}

/// A carried producer/consumer dependency edge (F3).
///
/// Hosts schedule the validated graph from these edges and from the declared
/// roots — never from kernel declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DependencyEdge {
    /// The launch that produces the value consumed downstream.
    pub producer: LaunchId,
    /// The launch that consumes it.
    pub consumer: LaunchId,
    /// The buffer whose content flows across the edge.
    pub buffer: BufferId,
    /// The content version that flows.
    pub version: u32,
}

/// Independent initialization axis (F5): how a buffer's storage is brought to
/// its first defined state. Never derived from the buffer role or lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitializationPolicy {
    /// Storage is zero-filled at allocation.
    ZeroFill,
    /// Storage is uploaded from host-provided values.
    HostProvided,
    /// Storage is fully written by a device kernel before any read.
    KernelInitialized,
}

impl InitializationPolicy {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::ZeroFill => "zero-fill",
            Self::HostProvided => "host-provided",
            Self::KernelInitialized => "kernel-initialized",
        }
    }
}

/// One buffer's initialization fact (F5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitializationFact {
    /// The buffer being initialized.
    pub buffer: BufferId,
    /// The independent initialization policy.
    pub policy: InitializationPolicy,
}

/// An explicit observation fact (F5/F6): the buffer is read back at a
/// declared completion boundary. A writable intermediate or persistent state
/// is observable **only** through such a fact — never because it is writable
/// and never because its role or lifetime happens to say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservationFact {
    /// The observed buffer.
    pub buffer: BufferId,
    /// The observed content version.
    pub version: u32,
    /// The launch whose completion boundary makes the observation valid.
    pub at_launch: LaunchId,
}

/// One selected differentiable input of the primal (F4): the gradient-to-primal
/// identity — which companion result-tuple slot carries this parameter's
/// gradient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirCompanionSelectedInput {
    /// The primal parameter this input is.
    pub param: DefId,
    /// Parameter position in the primal signature.
    pub position: u32,
    /// The primal parameter's type.
    pub ty: TypeId,
    /// The companion result-tuple slot carrying this input's gradient.
    pub gradient_slot: u32,
}

/// One selected output of the primal (F4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirCompanionSelectedOutput {
    /// Output position in the primal result tuple.
    pub position: u32,
    /// The output's type.
    pub ty: TypeId,
    /// The upstream gradient's type.
    pub upstream_gradient_ty: TypeId,
}

/// The lossless primal/companion relation row (F4): selected inputs, selected
/// outputs, and the gradient-to-primal identity survive from AIR through MIR,
/// device materialization, and the serialized package. Duplicate or
/// contradictory rows fail closed at the carrier's insertion boundary
/// ([`crate::device::MirCompanionMap`]) and at the wire admission boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LosslessMirCompanionEntry {
    /// The differentiable primal function.
    pub primal: DefId,
    /// The generated companion function.
    pub companion: DefId,
    /// Derivative kind of the companion.
    pub derivative: MirCompanionDerivativeKind,
    /// The primal is an explicitly device-resident forward kernel.
    pub device_resident: bool,
    /// The selected differentiable inputs (gradient-to-primal identity).
    pub selected_inputs: Vec<MirCompanionSelectedInput>,
    /// The selected primal outputs.
    pub selected_outputs: Vec<MirCompanionSelectedOutput>,
}

/// The carried semantic facts of a device program (F1–F7).
///
/// This accompanies the frozen [`crate::device_program::DeviceProgram`]; it is
/// not stored inside it, so the four-field program shape (and every emitter
/// constructor) stays intact. [`DeviceSemantics::validate`] enforces the
/// fail-closed rules; hosts and the serialized wire consume the same facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceSemantics {
    /// All semantic values (F1).
    pub values: Vec<SemanticValue>,
    /// One storage binding per value (F1).
    pub bindings: Vec<ValueBinding>,
    /// Explicit value generations, in program order (F2).
    pub generations: Vec<ValueGeneration>,
    /// Declared legal execution roots (F3).
    pub roots: Vec<LaunchId>,
    /// Carried producer/consumer dependency edges (F3).
    pub dependencies: Vec<DependencyEdge>,
    /// Lossless primal/companion relation rows (F4).
    pub relations: Vec<LosslessMirCompanionEntry>,
    /// Independent initialization facts (F5).
    pub initializations: Vec<InitializationFact>,
    /// Explicit observation facts (F5/F6).
    pub observations: Vec<ObservationFact>,
}

impl DeviceSemantics {
    /// Validate the carried semantic facts fail-closed (F1–F7).
    ///
    /// `launches` is the program's launch-id universe: roots, dependency
    /// endpoints, generation producers, and observation boundaries must all
    /// name a real launch, and every generation producer must be reachable
    /// from a root through the dependency graph (a missing dependency fails
    /// before launch). Validation is order-independent: declaration order of
    /// kernels, dependencies, or generations never changes the verdict.
    ///
    /// # Errors
    ///
    /// Returns the first [`DeviceProgramError`] the facts violate. Duplicate
    /// and contradictory relation rows, identity aliasing, unset/duplicate
    /// generations, graph cycles, and illegal roots all fail closed.
    pub fn validate(&self, launches: &[LaunchId]) -> Result<(), DeviceProgramError> {
        self.validate_values()?;
        self.validate_bindings()?;
        self.validate_generations()?;
        self.validate_roots_and_dependencies(launches)?;
        self.validate_relations()?;
        self.validate_axes(launches)?;
        Ok(())
    }

    /// Rule group F1: unique identities; never two origins for one id, never
    /// two ids for one origin (that would alias).
    fn validate_values(&self) -> Result<(), DeviceProgramError> {
        let mut seen: Vec<&SemanticValue> = Vec::new();
        for value in &self.values {
            if let Some(first) = seen.iter().find(|first| first.id == value.id) {
                if first.name != value.name {
                    return Err(DeviceProgramError::SemanticIdentityConflict {
                        value: value.id,
                        field: "name",
                        first: first.name.clone(),
                        second: value.name.clone(),
                    });
                }
                if first.element_ty != value.element_ty
                    || first.element_count != value.element_count
                {
                    return Err(DeviceProgramError::SemanticIdentityConflict {
                        value: value.id,
                        field: "shape",
                        first: format!(
                            "{} elements of {:?}",
                            first.element_count,
                            first.element_ty.semantic_id()
                        ),
                        second: format!(
                            "{} elements of {:?}",
                            value.element_count,
                            value.element_ty.semantic_id()
                        ),
                    });
                }
                if first.origin != value.origin {
                    return Err(DeviceProgramError::SemanticIdentityConflict {
                        value: value.id,
                        field: "origin",
                        first: first.origin.spelling(),
                        second: value.origin.spelling(),
                    });
                }
            } else {
                seen.push(value);
            }
        }
        // Identity is derived from carried facts: two distinct ids minted from
        // one origin would silently alias — rejected.
        for left in &self.values {
            for right in &self.values {
                if left.id != right.id && left.origin == right.origin {
                    return Err(DeviceProgramError::SemanticOriginAliased {
                        first: left.id,
                        second: right.id,
                        origin: left.origin.spelling(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Rule group F1: one value lives in exactly one buffer; one buffer holds
    /// exactly one value.
    fn validate_bindings(&self) -> Result<(), DeviceProgramError> {
        for binding in &self.bindings {
            if !self.values.iter().any(|value| value.id == binding.value) {
                return Err(DeviceProgramError::UnknownSemanticValue {
                    value: binding.value,
                });
            }
        }
        for left in &self.bindings {
            for right in &self.bindings {
                if left.value == right.value && left.buffer != right.buffer {
                    return Err(DeviceProgramError::DuplicateValueBinding {
                        value: left.value,
                        buffer: left.buffer,
                    });
                }
                if left.buffer == right.buffer && left.value != right.value {
                    return Err(DeviceProgramError::BufferHoldsMultipleValues {
                        buffer: left.buffer,
                        first: left.value,
                        second: right.value,
                    });
                }
            }
        }
        Ok(())
    }

    /// Rule group F2: every logical write produces an explicit generation;
    /// zero/unset generations are rejected; a (value, generation) has exactly
    /// one producer — a second producer of an existing generation is
    /// rejected (each write is a NEW generation, the per-storage-location
    /// monotone counter). Declaration order never changes the verdict;
    /// monotonicity/comparability for consumers is completed by the DDCP1-U2
    /// consumer-binding cross-check (contract rule 3).
    fn validate_generations(&self) -> Result<(), DeviceProgramError> {
        for generation in &self.generations {
            if generation.generation == 0 {
                return Err(DeviceProgramError::UnsetValueGeneration {
                    value: generation.value,
                });
            }
            if !self.values.iter().any(|value| value.id == generation.value) {
                return Err(DeviceProgramError::UnknownSemanticValue {
                    value: generation.value,
                });
            }
        }
        // Every logical write is a NEW generation (F2): a (value, generation)
        // pair is carried at most once, regardless of which launch claims it.
        for left in 0..self.generations.len() {
            for right in (left + 1)..self.generations.len() {
                if self.generations[left].value == self.generations[right].value
                    && self.generations[left].generation == self.generations[right].generation
                {
                    return Err(DeviceProgramError::DuplicateGenerationProducer {
                        value: self.generations[left].value,
                        generation: self.generations[left].generation,
                    });
                }
            }
        }
        Ok(())
    }

    /// Rule group F3: legal roots; well-formed, acyclic dependencies; every
    /// generation producer reachable from a root (a missing dependency fails
    /// before launch).
    fn validate_roots_and_dependencies(
        &self,
        launches: &[LaunchId],
    ) -> Result<(), DeviceProgramError> {
        for root in &self.roots {
            if !launches.contains(root) {
                return Err(DeviceProgramError::IllegalRoot { root: *root });
            }
        }
        for left in &self.roots {
            if self.roots.iter().filter(|root| *root == left).count() > 1 {
                return Err(DeviceProgramError::DuplicateRoot { root: *left });
            }
        }
        for dependency in &self.dependencies {
            if dependency.producer == dependency.consumer {
                return Err(DeviceProgramError::SelfDependency {
                    launch: dependency.producer,
                });
            }
            if !launches.contains(&dependency.producer) {
                return Err(DeviceProgramError::UnknownDependencyEndpoint {
                    launch: dependency.producer,
                });
            }
            if !launches.contains(&dependency.consumer) {
                return Err(DeviceProgramError::UnknownDependencyEndpoint {
                    launch: dependency.consumer,
                });
            }
            if self
                .dependencies
                .iter()
                .filter(|edge| {
                    edge.producer == dependency.producer
                        && edge.consumer == dependency.consumer
                        && edge.buffer == dependency.buffer
                        && edge.version == dependency.version
                })
                .count()
                > 1
            {
                return Err(DeviceProgramError::DuplicateDependencyEdge {
                    producer: dependency.producer,
                    consumer: dependency.consumer,
                });
            }
        }
        self.reject_dependency_cycles()?;
        // DDCP1-U2 / contract scheduling rule 3: a value that is consumed has
        // a producer — a missing producer fails closed. The edge's buffer
        // binds exactly one value, and the producer launch must actually
        // write it (a carried generation).
        for edge in &self.dependencies {
            let Some(binding) = self
                .bindings
                .iter()
                .find(|binding| binding.buffer == edge.buffer)
            else {
                return Err(DeviceProgramError::UnboundEdgeBuffer { buffer: edge.buffer });
            };
            if !self.generations.iter().any(|generation| {
                generation.value == binding.value && generation.produced_by == edge.producer
            }) {
                return Err(DeviceProgramError::MissingProducer {
                    producer: edge.producer,
                    value: binding.value,
                });
            }
        }
        // Complete inputs / missing dependency: every producer launch must be
        // reachable from a root through the carried graph; a launch that
        // produces a value but hangs off the graph is a missing dependency.
        for generation in &self.generations {
            if !self.reachable_from_root(generation.produced_by) {
                return Err(DeviceProgramError::MissingDependency {
                    launch: generation.produced_by,
                    value: generation.value,
                });
            }
        }
        // DDCP1-U2 / contract scheduling rule 2: every non-root launch must be
        // justified by an incoming dependency edge — a launch with no
        // justification is a dangling consumer.
        for launch in launches {
            if self.roots.contains(launch) {
                continue;
            }
            if !self.dependencies.iter().any(|edge| edge.consumer == *launch) {
                return Err(DeviceProgramError::DanglingConsumer { launch: *launch });
            }
        }
        Ok(())
    }

    /// Whether `launch` is reachable from a declared root via dependency
    /// edges (a launch with no incoming path from a root is a missing
    /// dependency).
    fn reachable_from_root(&self, launch: LaunchId) -> bool {
        let mut seen = self.roots.clone();
        let mut frontier = self.roots.clone();
        while let Some(current) = frontier.pop() {
            if current == launch {
                return true;
            }
            for edge in &self.dependencies {
                if edge.producer == current && !seen.contains(&edge.consumer) {
                    seen.push(edge.consumer);
                    frontier.push(edge.consumer);
                }
            }
        }
        false
    }

    /// Reject a dependency cycle (F3: validators enforce acyclicity).
    fn reject_dependency_cycles(&self) -> Result<(), DeviceProgramError> {
        let mut visited: Vec<LaunchId> = Vec::new();
        let mut on_stack: Vec<LaunchId> = Vec::new();
        for edge in &self.dependencies {
            if let Some(cycle_edge) = self.cycle_through(edge.producer, &mut visited, &mut on_stack)
            {
                return Err(DeviceProgramError::DependencyCycle { edge: cycle_edge });
            }
        }
        Ok(())
    }

    /// Depth-first search for a back edge (the edge whose consumer is already
    /// on the current DFS stack, completing a cycle).
    fn cycle_through(
        &self,
        launch: LaunchId,
        visited: &mut Vec<LaunchId>,
        on_stack: &mut Vec<LaunchId>,
    ) -> Option<DependencyEdge> {
        if on_stack.contains(&launch) {
            return self
                .dependencies
                .iter()
                .find(|edge| edge.consumer == launch)
                .copied();
        }
        if visited.contains(&launch) {
            return None;
        }
        visited.push(launch);
        on_stack.push(launch);
        for edge in &self.dependencies {
            if edge.producer == launch {
                if let Some(cycle_edge) = self.cycle_through(edge.consumer, visited, on_stack) {
                    return Some(cycle_edge);
                }
            }
        }
        on_stack.retain(|seen| *seen != launch);
        None
    }

    /// Rule group F4: relation rows are unique per primal and internally
    /// well-formed; duplicate or contradictory rows fail closed.
    fn validate_relations(&self) -> Result<(), DeviceProgramError> {
        for relation in &self.relations {
            if relation.primal == relation.companion {
                return Err(DeviceProgramError::IllegalRelationRow {
                    primal: relation.primal,
                    reason: "primal and companion DefIds must differ",
                });
            }
            let mut positions: Vec<u32> = Vec::new();
            for selected in &relation.selected_inputs {
                if positions.contains(&selected.position) {
                    return Err(DeviceProgramError::IllegalRelationRow {
                        primal: relation.primal,
                        reason: "duplicate selected-input position",
                    });
                }
                positions.push(selected.position);
            }
            let mut output_positions: Vec<u32> = Vec::new();
            for selected in &relation.selected_outputs {
                if output_positions.contains(&selected.position) {
                    return Err(DeviceProgramError::IllegalRelationRow {
                        primal: relation.primal,
                        reason: "duplicate selected-output position",
                    });
                }
                output_positions.push(selected.position);
            }
        }
        for relation in &self.relations {
            if self
                .relations
                .iter()
                .filter(|candidate| candidate.primal == relation.primal)
                .count()
                > 1
            {
                let conflict = self.relations.iter().find_map(|other| {
                    (other.primal == relation.primal)
                        .then(|| relation_conflict_field(relation, other))?
                });
                if let Some((field, first, second)) = conflict {
                    return Err(DeviceProgramError::ContradictoryRelationRow {
                        primal: relation.primal,
                        field,
                        first,
                        second,
                    });
                }
                return Err(DeviceProgramError::DuplicateRelationRow {
                    primal: relation.primal,
                });
            }
        }
        Ok(())
    }

    /// Rule group F5/F6: initialization facts are unique per buffer;
    /// observation facts name real launches and bound buffers.
    fn validate_axes(&self, launches: &[LaunchId]) -> Result<(), DeviceProgramError> {
        for left in &self.initializations {
            for right in &self.initializations {
                if left.buffer == right.buffer && left.policy != right.policy {
                    return Err(DeviceProgramError::ContradictoryInitialization {
                        buffer: left.buffer,
                        first: left.policy.spelling().to_owned(),
                        second: right.policy.spelling().to_owned(),
                    });
                }
            }
        }
        for observation in &self.observations {
            if !launches.contains(&observation.at_launch) {
                return Err(DeviceProgramError::ObservationOutOfRange {
                    launch: observation.at_launch,
                });
            }
            if !self
                .bindings
                .iter()
                .any(|binding| binding.buffer == observation.buffer)
            {
                return Err(DeviceProgramError::UnknownObservationBuffer {
                    buffer: observation.buffer,
                });
            }
        }
        Ok(())
    }
}

/// The first differing fact between two relation rows for the same primal.
fn relation_conflict_field(
    left: &LosslessMirCompanionEntry,
    right: &LosslessMirCompanionEntry,
) -> Option<(&'static str, String, String)> {
    if left.companion != right.companion {
        return Some((
            "companion",
            format!("DefId({})", left.companion.0),
            format!("DefId({})", right.companion.0),
        ));
    }
    if left.derivative != right.derivative {
        return Some((
            "derivative",
            left.derivative.spelling().to_owned(),
            right.derivative.spelling().to_owned(),
        ));
    }
    if left.device_resident != right.device_resident {
        return Some((
            "device-resident",
            left.device_resident.to_string(),
            right.device_resident.to_string(),
        ));
    }
    if left.selected_inputs != right.selected_inputs {
        return Some((
            "selected-inputs",
            format!("{:?}", left.selected_inputs),
            format!("{:?}", right.selected_inputs),
        ));
    }
    if left.selected_outputs != right.selected_outputs {
        return Some((
            "selected-outputs",
            format!("{:?}", left.selected_outputs),
            format!("{:?}", right.selected_outputs),
        ));
    }
    None
}

#[cfg(test)]
#[path = "device_semantics_test.rs"]
mod tests;
