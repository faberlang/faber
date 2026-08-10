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
//!
//! # DDCP1-U3 — Resource axes, explicit observations, per-output domains
//!
//! The five resource axes are **separate** facts — initialization, access
//! mode, allocation lifetime, mutation generation, and observation/readback
//! are declared and validated independently, never collapsed into a single
//! buffer flag (contract §Resource axes). Persistent parameters, optimizer
//! state, model state, and logical-cache state are **never automatically
//! results**: a value becomes a result only through an explicit declared output
//! (an observation/readback fact); results/readbacks are
//! never inferred from writable buffers (contract §Persistent state never
//! automatically results). Every output has its **own bounds**/index domain;
//! the shared dispatch extent never overruns or under-fills heterogeneous
//! outputs — **per-output** extents are derived (`device_program_plans.rs`)
//! or the program fails closed (contract §Per-output bounds).
//!
//! # DDCP1-U6 — Device/AIR legality + CPU host-`ad` preservation
//!
//! Device functions (every launch of a device program) must not carry `ad`,
//! Sermo, cancellation, or unresolved host effect operations. The
//! materializer records the effect-boundary facts
//! ([`DeviceLegalityFacts`]/[`DeviceEffectFact`]) from each kernel
//! function's MIR body; [`DeviceLegalityFacts::validate`] rejects any of
//! them with stable named diagnostics — deterministic rejection, never an
//! ad-hoc error (the DDCP1 hard gate). Ordinary CPU host `ad` through
//! Sermo is untouched: a CPU function is outside the device launch universe
//! and records no device effect fact. These partition facts complete the
//! boundary set DDCP1-U5 deferred — they are the region-boundary sources
//! for `ad`/Sermo/cancellation/CPU-effect boundaries, and because the gate
//! rejects them before region formation, no prepared submission region can
//! ever cross such a boundary (the U5 fail-closed envelope).

use crate::device::MirCompanionDerivativeKind;
use crate::device_program::{BufferId, DeviceProgramError, LaunchId, PreparedRegion, RegionId};
use crate::{MirFunctionId, MirType};
use radix_hir::DefId;
use radix_types::TypeId;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
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

/// The device-hostile effect kinds the device/AIR legality gate rejects
/// inside device functions (DDCP1-U6 / contract §ad/Sermo legality). The
/// materializer records which of these a device kernel's function body
/// carries; the gate rejects any of them with a stable named diagnostic —
/// deterministic rejection, never an ad-hoc error.
///
/// Serialize/Deserialize (added DDCP3-U3, additive): the DDCP3 partition/
/// callsite record's effect-boundary section carries this typed surface
/// directly (no mirror enum — the record rides the legality surface
/// read-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceEffectBoundaryKind {
    /// An automatic-differentiation operation — the `ad` Sermo conversation
    /// (a Sermo open with an `ad` frame) or a gradient-handle runtime
    /// operation inside the function body.
    Ad,
    /// A Sermo stream operation (open/send/receive/finish/view/cursor) —
    /// non-`ad` Sermo machinery inside the function body.
    Sermo,
    /// A cancellation boundary (async suspend / cancellation point) inside
    /// the function body.
    Cancellation,
    /// An unresolved host effect — a host-only (CPU) runtime call with no
    /// device lowering inside the function body.
    UnresolvedHostEffect,
}

impl DeviceEffectBoundaryKind {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Ad => "ad",
            Self::Sermo => "sermo",
            Self::Cancellation => "cancellation",
            Self::UnresolvedHostEffect => "unresolved-host-effect",
        }
    }
}

/// One carried effect-boundary fact (DDCP1-U6): the kernel of `launch`
/// contains a device-hostile effect. These are the host/device partition
/// facts that record effect boundaries explicitly — the `ad`/Sermo/
/// cancellation/unresolved host effect boundary facts DDCP1-U5 deferred.
/// The legality gate rejects a fact on any device launch, so a legal
/// program carries none of them and no prepared submission region can
/// cross such a boundary (the U5 fail-closed envelope, made concrete).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceEffectFact {
    /// The launch whose kernel carries the effect.
    pub launch: LaunchId,
    /// The recorded device-hostile effect kind.
    pub kind: DeviceEffectBoundaryKind,
}

/// The carried host/device partition effect-boundary record (DDCP1-U6).
///
/// The materializer builds this record from each kernel function's MIR body
/// (a device function containing `ad`, Sermo, cancellation, or an
/// unresolved host effect is recorded here) and hands it to
/// [`Self::validate`]. The record is the explicit effect-boundary
/// accounting the direct device ABI needs: it is the ONLY Sermo/effect
/// surface in the carrier, and a legal device program carries no facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceLegalityFacts {
    /// The recorded device-effect boundary facts, one per offending launch.
    pub device_effects: Vec<DeviceEffectFact>,
}

impl DeviceLegalityFacts {
    /// DDCP1-U6 / contract §ad/Sermo legality — the device/AIR legality
    /// gate.
    ///
    /// Rejects `ad`, Sermo, cancellation, and unresolved host effect facts
    /// recorded on device-function launches with stable structured
    /// diagnostics ([`DeviceProgramError::DeviceFunctionHostileEffect`]) —
    /// deterministic, named rejection, never an ad-hoc error. A fact
    /// naming a launch outside the device program is a malformed record
    /// and fails closed ([`DeviceProgramError::UnknownDeviceEffectLaunch`]).
    ///
    /// Ordinary CPU host `ad` through Sermo is untouched: a CPU function is
    /// outside the device launch universe and records no device effect
    /// fact, so preservation and rejection never trade against each other
    /// (reconciliation §3.2). Because this gate runs before prepared-region
    /// formation, a legal program carries no device-hostile fact and no
    /// prepared submission region can cross an `ad`/Sermo/cancellation/CPU-
    /// effect boundary (reconciliation §3.3; the U5 fail-closed envelope).
    ///
    /// # Errors
    ///
    /// Returns the first [`DeviceProgramError`] the record violates.
    pub fn validate(&self, launches: &[LaunchId]) -> Result<(), DeviceProgramError> {
        if let Some(fact) = self.device_effects.first() {
            if !launches.contains(&fact.launch) {
                return Err(DeviceProgramError::UnknownDeviceEffectLaunch {
                    launch: fact.launch,
                });
            }
            return Err(DeviceProgramError::DeviceFunctionHostileEffect {
                launch: fact.launch,
                kind: fact.kind,
            });
        }
        Ok(())
    }
}

/// The result of effect-boundary-aware region derivation (DDCP3-U3 (b)): the
/// re-derived prepared submission regions plus the explicit effect-boundary
/// rows the derivation consumed. Additive to the landed
/// [`DeviceLegalityFacts`] surface — no legality type is renamed or reshaped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreparedRegionDerivation {
    /// The re-derived prepared submission regions (same derivation rules as
    /// [`DeviceSemantics::derive_prepared_regions`], cross-checked against
    /// the recorded device-effect facts).
    pub regions: Vec<PreparedRegion>,
    /// The explicit effect-boundary rows (device-hostile facts) the
    /// derivation consumed — empty for a legal program (the fail-closed
    /// envelope).
    pub effect_boundaries: Vec<DeviceEffectFact>,
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
///
/// # DDCP1-U4 companion identity
///
/// An AIR-generated backward companion's **identity is derived from its
/// primal kernel identity plus the typed companion relation** (contract
/// §Kernel and companion identity rule 2): [`Self::identity_key`] returns the
/// (primal, differentiation point) pair — never a name, symbol, or position.
/// Duplicate companion facts for one identity key fail closed; contradictory
/// companion facts (incompatible shapes vs the primal's recorded facts) fail
/// closed; a stale companion (consuming a superseded primal write generation)
/// fails closed; a missing companion (a declared companion whose outputs are
/// not materialized) fails closed — while a forward-only program declares no
/// companions and needs none (the missing rule applies only where the
/// dependency graph or declared observations actually reference companion
/// outputs).
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

impl LosslessMirCompanionEntry {
    /// The companion's stable identity key: the **primal kernel identity plus
    /// the typed companion relation** — exactly the (primal, differentiation
    /// point) pair the contract keys duplicate/contradictory detection on.
    /// Identity is never recovered from names, symbols, or positions.
    #[must_use]
    pub fn identity_key(&self) -> (DefId, MirCompanionDerivativeKind) {
        (self.primal, self.derivative)
    }
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
    /// monotone counter). Declaration order never changes the verdict.
    /// Generation numbers start at 1 (0 is the rejected unset sentinel), but
    /// the schema does NOT enforce numeric generation monotonicity — a sole
    /// generation-2 write is schema-valid by design, so the materializer
    /// mints monotone generations and the independent radix-air oracle
    /// catches chain deviations (e.g. `mutation_generation_substituted`);
    /// consumer comparability is completed by the DDCP1-U2 consumer-binding
    /// cross-check (contract rule 3).
    ///
    /// Residual (auditor-2 fire-7 R3): numeric generation monotonicity is
    /// materializer + radix-air oracle enforcement, never a schema rule —
    /// the schema must not pre-empt the independent oracle.
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
                return Err(DeviceProgramError::UnboundEdgeBuffer {
                    buffer: edge.buffer,
                });
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
            if !self
                .dependencies
                .iter()
                .any(|edge| edge.consumer == *launch)
            {
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

    /// Rule group F4: relation rows are unique per companion identity and
    /// internally well-formed; duplicate, contradictory, and missing
    /// companion facts fail closed. Duplicate detection is keyed on the
    /// companion identity ([`LosslessMirCompanionEntry::identity_key`] — the
    /// primal plus the typed differentiation point); a forward-only program
    /// declares no companions and needs none.
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
            // DDCP1-U4 / contract rule 4 (contradictory): the companion's
            // upstream gradient shape must match the primal output it
            // differentiates — an incompatible shape is a contradictory
            // companion fact vs the primal's recorded facts.
            for selected in &relation.selected_outputs {
                if selected.upstream_gradient_ty != selected.ty {
                    return Err(DeviceProgramError::ContradictoryCompanionFact {
                        primal: relation.primal,
                        field: "upstream-gradient-type",
                        first: format!("{:?}", selected.ty),
                        second: format!("{:?}", selected.upstream_gradient_ty),
                    });
                }
            }
            // DDCP1-U4 / contract rule 6 (missing companion, reference-
            // scoped): a declared companion must reference its companion
            // facts — a relation with neither selected inputs nor selected
            // outputs declares a companion with no outputs, a missing
            // companion fact. A forward-only program declares no companions
            // and needs none (rule 7); graph-direction references (a launch
            // consuming a companion output with no declared relation) are
            // proven by the independent oracle (G6 KernelSequence).
            if relation.selected_inputs.is_empty() && relation.selected_outputs.is_empty() {
                return Err(DeviceProgramError::MissingCompanionFact {
                    primal: relation.primal,
                    reason: "the relation declares no selected inputs and no selected outputs",
                });
            }
        }
        for relation in &self.relations {
            let key = relation.identity_key();
            if self
                .relations
                .iter()
                .filter(|candidate| candidate.identity_key() == key)
                .count()
                > 1
            {
                let conflict = self.relations.iter().find_map(|other| {
                    (other.identity_key() == key)
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

    /// DDCP1-U5 / contract §Prepared submission regions — derive the
    /// compiler-visible prepared submission regions from the carried
    /// dependency graph.
    ///
    /// A prepared submission region is an ordered group of member launches
    /// encodable/enqueuable as a single host call (NGAB0-R1 granularity:
    /// **one host call → one prepared submission region**) without CPU
    /// observation between members. Boundaries arise ONLY from real sources,
    /// exhaustively:
    ///
    /// - **Real dependencies** (rule 1/4): members of one region are
    ///   path-connected through the carried dependency edges — each
    ///   connected component is its own candidate; disconnected launches
    ///   are separate regions.
    /// - **Declared observations** (rule 3): a launch carrying a declared
    ///   observation fact is a CPU readback point; a prepared submission
    ///   region **cannot cross** an observation boundary, so the candidate
    ///   group is **split at that point** — the observed launch is the last
    ///   member of its region and the following members start a new region.
    ///   A one-member region is the minimal prepared region, not a separate
    ///   ABI shape.
    /// - **Host control** (rule 2): declared roots are host submit-entry
    ///   points and host-provided initialization is a host-upload point —
    ///   both constrain submission between host calls, never inside a
    ///   region.
    ///
    /// No boundary is ever invented by a generic runtime wrapper, a
    /// scheduling policy outside the compiler, or to "match" a runtime
    /// interface. `ad`/Sermo, cancellation, and unresolved CPU-effect
    /// boundaries are not carried facts of this schema; the device/AIR
    /// legality surface (DDCP1-U6) rejects programs carrying them before
    /// region formation — a legal program's regions can never cross them
    /// (fail-closed envelope). Region identity is compiler-owned and static
    /// at compile time: ids are minted deterministically from the launch-id
    /// universe and the carried edge/observation sets, independent of
    /// declaration order and emitted symbol order.
    #[must_use]
    pub fn derive_prepared_regions(&self, launches: &[LaunchId]) -> Vec<PreparedRegion> {
        let mut next_id = 0u32;
        let mut regions = Vec::new();
        for component in self.dependency_components(launches) {
            let ordered = self.topological_order(&component);
            for part in split_at_observation_points(&self.observations, ordered) {
                regions.push(PreparedRegion {
                    id: RegionId(next_id),
                    launches: part,
                });
                next_id += 1;
            }
        }
        regions
    }

    /// DDCP3-U3 (b) — re-derive region formation over the landed
    /// [`DeviceLegalityFacts`]/[`DeviceEffectFact`] surface (additive: the
    /// base [`Self::derive_prepared_regions`] and the legality surface are
    /// untouched). The result carries the re-derived regions plus the
    /// explicit effect-boundary rows the derivation consumed.
    ///
    /// Candidates form exactly as the base derivation (dependency components
    /// → topological order → observation splits). Each candidate region's
    /// member launches are then cross-checked against the recorded
    /// device-effect facts:
    ///
    /// - an **observation** boundary splits the region at that point (the
    ///   base rule 3 — a prepared submission region cannot cross a CPU
    ///   readback point);
    /// - an `ad`/Sermo/cancellation/unresolved-host-effect fact on a member
    ///   launch **fails closed with the named rule**
    ///   ([`DeviceProgramError::DeviceFunctionHostileEffect`] — the DDCP1-U6
    ///   gate): a device function carrying such an effect is illegal on the
    ///   device side, so a candidate region can never cross that boundary
    ///   (the U5 fail-closed envelope, made concrete at region formation).
    ///   This is a rejection, not a split — DDCP1 semantics are not
    ///   re-decided here; a genuinely ambiguous case is recorded + escalated,
    ///   never gate-weakened.
    ///
    /// The consumed effect-boundary rows are returned in
    /// [`PreparedRegionDerivation::effect_boundaries`] so the DDCP3
    /// partition/callsite record carries them explicitly (empty for a legal
    /// program).
    ///
    /// # Errors
    ///
    /// Returns the first [`DeviceProgramError`] a candidate region violates:
    /// a hostile effect on a member launch
    /// ([`DeviceFunctionHostileEffect`](DeviceProgramError::DeviceFunctionHostileEffect)),
    /// or a malformed legality record naming a launch outside the program
    /// ([`UnknownDeviceEffectLaunch`](DeviceProgramError::UnknownDeviceEffectLaunch)).
    pub fn derive_prepared_regions_with_effect_boundaries(
        &self,
        launches: &[LaunchId],
        legality: &DeviceLegalityFacts,
    ) -> Result<PreparedRegionDerivation, DeviceProgramError> {
        let regions = self.derive_prepared_regions(launches);
        // Region-formation-level cross-check: a candidate region whose member
        // launch carries a hostile effect crosses an effect boundary — fail
        // closed with the named rule.
        for region in &regions {
            for launch in &region.launches {
                if let Some(fact) = legality
                    .device_effects
                    .iter()
                    .find(|fact| fact.launch == *launch)
                {
                    return Err(DeviceProgramError::DeviceFunctionHostileEffect {
                        launch: *launch,
                        kind: fact.kind,
                    });
                }
            }
        }
        // The legality record's universe rule: an effect fact naming a launch
        // outside the program is a malformed record and fails closed.
        legality.validate(launches)?;
        Ok(PreparedRegionDerivation {
            regions,
            effect_boundaries: legality.device_effects.clone(),
        })
    }

    /// Connected components of the carried dependency graph over `launches`
    /// (undirected connectivity: every edge joins its endpoints into one
    /// component). Canonical — components and their members are keyed on
    /// [`LaunchId`], never on declaration position.
    fn dependency_components(&self, launches: &[LaunchId]) -> Vec<Vec<LaunchId>> {
        let mut sorted: Vec<LaunchId> = launches.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let index: HashMap<LaunchId, usize> = sorted
            .iter()
            .enumerate()
            .map(|(position, launch)| (*launch, position))
            .collect();
        let mut parent: Vec<usize> = (0..sorted.len()).collect();
        for edge in &self.dependencies {
            if let (Some(&producer), Some(&consumer)) =
                (index.get(&edge.producer), index.get(&edge.consumer))
            {
                union_find(&mut parent, producer, consumer);
            }
        }
        let mut groups: HashMap<usize, Vec<LaunchId>> = HashMap::new();
        for (position, launch) in sorted.iter().enumerate() {
            groups
                .entry(find(&mut parent, position))
                .or_default()
                .push(*launch);
        }
        let mut components: Vec<Vec<LaunchId>> = groups.into_values().collect();
        for component in &mut components {
            component.sort_unstable();
        }
        components.sort_by_key(|component| component[0]);
        components
    }

    /// A deterministic topological order of `component` (producer before
    /// consumer), tie-breaking on [`LaunchId`] — declaration order never
    /// decides the member order. On a cyclic graph (already rejected by
    /// [`validate`](Self::validate)) the un-emitted residue is appended in
    /// launch-id order so the derivation stays total.
    fn topological_order(&self, component: &[LaunchId]) -> Vec<LaunchId> {
        let mut indegree: HashMap<LaunchId, usize> =
            component.iter().map(|launch| (*launch, 0)).collect();
        let mut successors: HashMap<LaunchId, Vec<LaunchId>> = HashMap::new();
        for edge in &self.dependencies {
            if !component.contains(&edge.producer) || !component.contains(&edge.consumer) {
                continue;
            }
            successors
                .entry(edge.producer)
                .or_default()
                .push(edge.consumer);
            *indegree.entry(edge.consumer).or_default() += 1;
        }
        let mut ready: BinaryHeap<Reverse<LaunchId>> = component
            .iter()
            .filter(|launch| indegree.get(launch) == Some(&0))
            .map(|launch| Reverse(*launch))
            .collect();
        let mut order: Vec<LaunchId> = Vec::new();
        while let Some(Reverse(launch)) = ready.pop() {
            order.push(launch);
            if let Some(next) = successors.get(&launch) {
                for consumer in next {
                    let count = indegree
                        .get_mut(consumer)
                        .expect("consumer is a member of the component");
                    *count -= 1;
                    if *count == 0 {
                        ready.push(Reverse(*consumer));
                    }
                }
            }
        }
        // Cyclic residue (impossible for a validated program): keep total.
        let mut residue: Vec<LaunchId> = component
            .iter()
            .filter(|launch| !order.contains(launch))
            .copied()
            .collect();
        residue.sort_unstable();
        order.extend(residue);
        order
    }

    /// DDCP1-U5 / contract §Prepared submission regions — validate a
    /// prepared-submission-region partition fail-closed.
    ///
    /// A prepared submission region **cannot cross** an observation boundary,
    /// a cancellation boundary, or any other CPU-effect/host-control point;
    /// any candidate group containing one is split at that point or the
    /// program fails closed (rule 3). Over the compiler-derived surface this
    /// cross-check proves:
    ///
    /// - **Totality** (rule 1/4): every launch is a member of exactly one
    ///   region — an unassigned or duplicated launch fails closed.
    /// - **No-crossing** (rule 3): an observation point in a region's
    ///   interior (any member except the last) forces a CPU readback
    ///   between members — the region fails closed.
    /// - **Connectivity** (rule 1): the members of one region are
    ///   path-connected through the carried dependency edges.
    ///
    /// `ad`/Sermo, cancellation, and unresolved CPU-effect boundaries are
    /// rejected by the device/AIR legality surface (DDCP1-U6) before region
    /// formation — the regions of a legal program cannot cross them.
    ///
    /// # Errors
    ///
    /// Returns the first [`DeviceProgramError`] the partition violates.
    pub fn validate_regions(
        &self,
        launches: &[LaunchId],
        regions: &[PreparedRegion],
    ) -> Result<(), DeviceProgramError> {
        // Totality: every launch is a member of exactly one region.
        let mut seen: Vec<LaunchId> = Vec::new();
        for region in regions {
            for launch in &region.launches {
                if seen.contains(launch) {
                    return Err(DeviceProgramError::RegionUnassignedLaunch { launch: *launch });
                }
                seen.push(*launch);
            }
        }
        for launch in launches {
            if !seen.contains(launch) {
                return Err(DeviceProgramError::RegionUnassignedLaunch { launch: *launch });
            }
        }

        for region in regions {
            // No-crossing: no interior member may be an observation point.
            let member_count = region.launches.len();
            for (position, launch) in region.launches.iter().enumerate() {
                let observed = self
                    .observations
                    .iter()
                    .any(|observation| observation.at_launch == *launch);
                if observed && position + 1 < member_count {
                    return Err(DeviceProgramError::RegionCrossesBoundary {
                        region: region.id,
                        launch: *launch,
                    });
                }
            }
            // Connectivity: the members form one path-connected group.
            if !self.members_connected(&region.launches) {
                return Err(DeviceProgramError::RegionMembersDisconnected { region: region.id });
            }
        }
        Ok(())
    }

    /// Whether `members` are path-connected through the carried dependency
    /// edges (undirected): every member reachable from the first.
    fn members_connected(&self, members: &[LaunchId]) -> bool {
        if members.len() <= 1 {
            return true;
        }
        let mut seen: Vec<LaunchId> = vec![members[0]];
        let mut frontier: Vec<LaunchId> = vec![members[0]];
        while let Some(current) = frontier.pop() {
            for edge in &self.dependencies {
                let neighbor = if edge.producer == current {
                    Some(edge.consumer)
                } else if edge.consumer == current {
                    Some(edge.producer)
                } else {
                    None
                };
                let Some(neighbor) = neighbor else { continue };
                if members.contains(&neighbor) && !seen.contains(&neighbor) {
                    seen.push(neighbor);
                    frontier.push(neighbor);
                }
            }
        }
        seen.len() == members.len()
    }
}

/// Union-find over the launch index universe: merge two components.
fn union_find(parent: &mut [usize], a: usize, b: usize) {
    let root_a = find(parent, a);
    let root_b = find(parent, b);
    if root_a != root_b {
        parent[root_a] = root_b;
    }
}

/// Union-find root with path halving.
fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Split a topologically-ordered connected component at every declared
/// observation point: a launch carrying a declared observation is a CPU
/// readback boundary — it ends its region and the following members start a
/// new region (contract §Prepared submission regions rule 3).
fn split_at_observation_points(
    observations: &[ObservationFact],
    ordered: Vec<LaunchId>,
) -> Vec<Vec<LaunchId>> {
    let mut parts: Vec<Vec<LaunchId>> = Vec::new();
    let mut current: Vec<LaunchId> = Vec::new();
    for launch in ordered {
        current.push(launch);
        let observed = observations
            .iter()
            .any(|observation| observation.at_launch == launch);
        if observed {
            parts.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
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

#[cfg(test)]
#[path = "ddcp1_fixtures_test.rs"]
mod ddcp1_fixtures;
