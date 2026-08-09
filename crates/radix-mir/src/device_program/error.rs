use super::types::{BufferId, BufferLifetime, BufferRole, LaunchId, ObservationCadence, RegionId};
use crate::abi::MirKernelResourceAccess;
use crate::device_semantics::{DependencyEdge, DeviceEffectBoundaryKind, SemanticValueId};
use crate::kernel_plan::PlanValidationError;
use radix_hir::DefId;
use std::fmt;

/// Device-program validation failure (see [`DeviceProgram::validate`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceProgramError {
    /// The program declares no kernels.
    EmptyKernels,
    /// The program declares kernels but no ordered launch sequence.
    EmptyLaunches,
    /// A kernel has an empty entry name.
    EmptyEntryName { kernel: usize },
    /// The kernel's collection-op plan failed its own validation.
    Plan {
        /// The underlying plan failure.
        error: PlanValidationError,
    },
    /// A kernel binds the same (group, binding) slot twice.
    DuplicateBinding {
        kernel: usize,
        group: u32,
        binding: u32,
    },
    /// A resource or result references version 0 (the reserved unset value).
    ZeroVersion { buffer: BufferId, version: u32 },
    /// A slot's access contradicts the buffer's program role.
    RoleAccessMismatch {
        buffer: BufferId,
        role: BufferRole,
        access: MirKernelResourceAccess,
    },
    /// A launch references a kernel index outside the kernel list.
    LaunchOutOfRange {
        launch: LaunchId,
        kernel_index: usize,
        kernel_count: usize,
    },
    /// A launch id appears more than once in the ordered sequence.
    DuplicateLaunchId { launch: LaunchId },
    /// A result names a producing launch that does not exist.
    UnknownProducingLaunch {
        result: usize,
        produced_by: LaunchId,
    },
    /// A result's role is neither output nor in-out.
    ResultRole { result: usize, role: BufferRole },
    /// A result's observation role disagrees with its buffer's identity role.
    ResultBufferRoleMismatch {
        result: usize,
        role: BufferRole,
        buffer_role: BufferRole,
    },
    /// A result's observation cadence contradicts its buffer's lifetime class
    /// (S5A-U1): a `PerStep` readback is valid only for an `ObservationPoint`
    /// buffer; an `EndOfRun` readback only for `PerStep` or `PerProgram` buffers.
    ResultCadenceMismatch {
        result: usize,
        buffer: BufferId,
        cadence: ObservationCadence,
        lifetime: BufferLifetime,
    },
    /// A result's named producing launch does not actually write the version.
    ResultNotProduced {
        result: usize,
        produced_by: LaunchId,
        buffer: BufferId,
        version: u32,
    },
    /// Two references to the same buffer id disagree on an identity fact.
    BufferIdentityConflict {
        buffer: BufferId,
        field: &'static str,
        first: String,
        second: String,
    },
    /// Two references to the same (buffer, version) disagree on the shape —
    /// a shape change must be a new version (no in-place reinterpretation).
    VersionShapeConflict {
        buffer: BufferId,
        version: u32,
        first: String,
        second: String,
    },
    // ── Stage 3R carried-semantics contract (F1–F7) ────────────────────────
    /// A semantic value repeats with conflicting carried facts (F1).
    SemanticIdentityConflict {
        value: SemanticValueId,
        field: &'static str,
        first: String,
        second: String,
    },
    /// Two distinct semantic values are minted from the same carried origin —
    /// they would silently alias (F1).
    SemanticOriginAliased {
        first: SemanticValueId,
        second: SemanticValueId,
        origin: String,
    },
    /// A value id is referenced that no [`SemanticValue`](crate::device_semantics::SemanticValue)
    /// declares (F1).
    UnknownSemanticValue { value: SemanticValueId },
    /// A semantic value is bound to more than one buffer (F1).
    DuplicateValueBinding {
        value: SemanticValueId,
        buffer: BufferId,
    },
    /// Two different values claim the same buffer (F1).
    BufferHoldsMultipleValues {
        buffer: BufferId,
        first: SemanticValueId,
        second: SemanticValueId,
    },
    /// A write carries generation 0 — the reserved unset sentinel (F2).
    UnsetValueGeneration { value: SemanticValueId },
    /// A second producer claims an existing (value, generation) (F2).
    DuplicateGenerationProducer {
        value: SemanticValueId,
        generation: u32,
    },
    /// A root names a launch that does not exist (F3).
    IllegalRoot { root: LaunchId },
    /// A root appears more than once (F3).
    DuplicateRoot { root: LaunchId },
    /// A dependency edge runs from a launch to itself (F3).
    SelfDependency { launch: LaunchId },
    /// A dependency edge names a launch that does not exist (F3).
    UnknownDependencyEndpoint { launch: LaunchId },
    /// The same dependency edge is declared twice (F3).
    DuplicateDependencyEdge {
        producer: LaunchId,
        consumer: LaunchId,
    },
    /// The carried dependency graph contains a cycle (F3).
    DependencyCycle { edge: DependencyEdge },
    /// A generation producer is not reachable from any root — a missing
    /// dependency (F3).
    MissingDependency {
        launch: LaunchId,
        value: SemanticValueId,
    },
    /// A relation row is malformed (primal equals companion, duplicate
    /// selected positions, …) (F4).
    IllegalRelationRow { primal: DefId, reason: &'static str },
    /// Two relation rows name the same primal with identical facts (F4).
    DuplicateRelationRow { primal: DefId },
    /// Two relation rows for the same primal disagree on a fact (F4).
    ContradictoryRelationRow {
        primal: DefId,
        field: &'static str,
        first: String,
        second: String,
    },
    /// Two initialization facts for the same buffer disagree (F5).
    ContradictoryInitialization {
        buffer: BufferId,
        first: String,
        second: String,
    },
    /// An observation names a launch that does not exist (F5/F6).
    ObservationOutOfRange { launch: LaunchId },
    /// An observation names a buffer no value is bound to (F6).
    UnknownObservationBuffer { buffer: BufferId },
    /// A result row has no explicit observation fact in the carried
    /// semantics — a writable intermediate exposed as a result without being
    /// declared observed (F6).
    ResultWithoutObservationFact {
        result: usize,
        buffer: BufferId,
        version: u32,
    },
    /// An observation fact has no declared result row — a readback the host
    /// would never be told to perform (F6).
    UndeclaredObservationFact {
        buffer: BufferId,
        version: u32,
        at_launch: LaunchId,
    },
    // ── DDCP1-U2: stable identity + explicit generations + scheduling ──────
    /// A non-root launch has no incoming dependency edge — a dangling
    /// consumer with no justifying producer (F3 / contract scheduling rule 2).
    DanglingConsumer { launch: LaunchId },
    /// A dependency edge flows a buffer no semantic value is bound to — the
    /// consumer cannot inherit a value identity (F3 / contract rule 3).
    UnboundEdgeBuffer { buffer: BufferId },
    /// A consumer edge references a value its producer launch never writes —
    /// a missing producer (F3 / contract scheduling rule 3).
    MissingProducer {
        producer: LaunchId,
        value: SemanticValueId,
    },
    /// A consumer edge's flowing buffer version has a different shape than
    /// the producer's carried generation for the value — the consumer cannot
    /// bind a consistent generation (contract rule 3).
    EdgeGenerationShapeMismatch {
        value: SemanticValueId,
        producer: LaunchId,
        version: u32,
    },
    /// A producer writes the value in more than one generation matching one
    /// flowing buffer version — the consumer binding is ambiguous (contract
    /// rule 3).
    AmbiguousEdgeGeneration {
        value: SemanticValueId,
        producer: LaunchId,
        version: u32,
    },
    // ── DDCP1-U3: resource axes + explicit observations + per-output ───────
    /// A buffer is read by a kernel before any initialization is declared —
    /// a first read before initialization (contract §Resource axes rule 1).
    UninitializedBufferRead { buffer: BufferId, launch: LaunchId },
    /// A buffer declares the read-back lifetime class (`ObservationPoint`)
    /// but is never declared observed — a use outside the declared
    /// allocation lifetime (contract §Resource axes rule 3).
    ObservationPointBufferNotObserved { buffer: BufferId },
    /// A kernel output's own bounds/index domain is not covered by the
    /// shared dispatch extent — an under-fill of a heterogeneous or single
    /// output (contract §Per-output bounds rules 1–3).
    OutputExtentUnderfilled {
        kernel: usize,
        group: u32,
        binding: u32,
        element_count: u64,
        dispatched: u64,
    },
    // ── DDCP1-U4: companion identity + coexistence ─────────────────────────
    /// A companion relation carries an upstream gradient shape incompatible
    /// with the primal output it differentiates — a contradictory companion
    /// fact vs the primal's recorded facts (contract rule 4).
    ContradictoryCompanionFact {
        primal: DefId,
        field: &'static str,
        first: String,
        second: String,
    },
    /// A consumer binds an OLDER generation of a value while the producer
    /// carries a newer one, in a companion-bearing program — a stale
    /// companion consuming superseded primal state (contract rule 5).
    StaleCompanionFact {
        value: SemanticValueId,
        bound_generation: u32,
        latest_generation: u32,
    },
    /// A declared companion references no companion facts (no selected
    /// inputs and no selected outputs) — a missing companion fact. A
    /// forward-only program declares no companions and needs none; the rule
    /// applies only where companion outputs are actually referenced (contract
    /// rule 6/7). Graph-direction references (a launch consuming a companion
    /// output with no relation) are proven by the independent oracle
    /// (G6 KernelSequence).
    MissingCompanionFact {
        primal: DefId,
        reason: &'static str,
    },
    // ── DDCP1-U5: prepared submission regions ────────────────────────────
    /// A prepared submission region contains a declared observation point in
    /// its interior — the host would have to read back between members, a
    /// CPU observation the region's single host call forbids (contract
    /// §Prepared submission regions rule 3).
    RegionCrossesBoundary {
        region: RegionId,
        launch: LaunchId,
    },
    /// A prepared submission region's members are not path-connected through
    /// the carried dependency graph — a candidate group spanning disconnected
    /// launches (contract rule 1).
    RegionMembersDisconnected { region: RegionId },
    /// A launch is not assigned to exactly one prepared submission region —
    /// an incomplete or overlapping region partition (contract rules 1/4).
    RegionUnassignedLaunch { launch: LaunchId },
    // ── DDCP1-U6: device/AIR legality ────────────────────────────────────
    /// A device effect-boundary fact names a launch outside the device
    /// program — a malformed legality record (DDCP1-U6).
    UnknownDeviceEffectLaunch { launch: LaunchId },
    /// A device function (launch) carries a device-hostile effect — `ad`,
    /// Sermo, cancellation, or an unresolved host (CPU) effect. Stable
    /// structured rejection with the named effect (DDCP1-U6 / contract
    /// §ad/Sermo legality).
    DeviceFunctionHostileEffect {
        launch: LaunchId,
        kind: DeviceEffectBoundaryKind,
    },
}

impl fmt::Display for DeviceProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKernels => write!(f, "device program declares no kernels"),
            Self::EmptyLaunches => write!(f, "device program declares no launches"),
            Self::EmptyEntryName { kernel } => {
                write!(f, "kernel {kernel} has an empty entry name")
            }
            Self::Plan { error } => write!(f, "kernel plan validation failed: {error}"),
            Self::DuplicateBinding {
                kernel,
                group,
                binding,
            } => write!(
                f,
                "kernel {kernel} binds group {group} binding {binding} more than once"
            ),
            Self::ZeroVersion { buffer, version } => write!(
                f,
                "buffer {buffer} references version {version}; version 0 is reserved (unset)"
            ),
            Self::RoleAccessMismatch {
                buffer,
                role,
                access,
            } => write!(
                f,
                "buffer {buffer} has role {} but a slot accesses it as {access:?}",
                role.spelling()
            ),
            Self::LaunchOutOfRange {
                launch,
                kernel_index,
                kernel_count,
            } => write!(
                f,
                "launch {launch} references kernel {kernel_index} but the program has {kernel_count} kernels"
            ),
            Self::DuplicateLaunchId { launch } => {
                write!(f, "launch id {launch} appears more than once in the launch sequence")
            }
            Self::UnknownProducingLaunch { result, produced_by } => write!(
                f,
                "result {result} names producing launch {produced_by}, which does not exist"
            ),
            Self::ResultRole { result, role } => write!(
                f,
                "result {result} has role {}; results must be output or in-out",
                role.spelling()
            ),
            Self::ResultBufferRoleMismatch {
                result,
                role,
                buffer_role,
            } => write!(
                f,
                "result {result} has observation role {} but its buffer's identity role is {}",
                role.spelling(),
                buffer_role.spelling()
            ),
            Self::ResultCadenceMismatch {
                result,
                buffer,
                cadence,
                lifetime,
            } => write!(
                f,
                "result {result} declares buffer {buffer} with a {} observation cadence, but the buffer's lifetime class is {}; a per-step readback names an observation-point buffer, an end-of-run readback names a per-step or per-program buffer",
                cadence.spelling(),
                lifetime.spelling()
            ),
            Self::ResultNotProduced {
                result,
                produced_by,
                buffer,
                version,
            } => write!(
                f,
                "result {result} declares launch {produced_by} as producer of buffer {buffer} version {version}, but that launch does not write it"
            ),
            Self::BufferIdentityConflict {
                buffer,
                field,
                first,
                second,
            } => write!(
                f,
                "buffer {buffer} declares conflicting {field}: first {first}, second {second}"
            ),
            Self::VersionShapeConflict {
                buffer,
                version,
                first,
                second,
            } => write!(
                f,
                "buffer {buffer} version {version} has conflicting shapes: first {first}, second {second}"
            ),
            Self::SemanticIdentityConflict {
                value,
                field,
                first,
                second,
            } => write!(
                f,
                "semantic value {value} declares conflicting {field}: first {first}, second {second}"
            ),
            Self::SemanticOriginAliased {
                first,
                second,
                origin,
            } => write!(
                f,
                "semantic values {first} and {second} both derive from origin {origin}; distinct values must derive from distinct carried facts"
            ),
            Self::UnknownSemanticValue { value } => {
                write!(f, "semantic value {value} is referenced but never declared")
            }
            Self::DuplicateValueBinding { value, buffer } => write!(
                f,
                "semantic value {value} is bound to more than one buffer (including {buffer})"
            ),
            Self::BufferHoldsMultipleValues {
                buffer,
                first,
                second,
            } => write!(
                f,
                "buffer {buffer} holds semantic values {first} and {second}; one buffer holds one value"
            ),
            Self::UnsetValueGeneration { value } => write!(
                f,
                "a write of semantic value {value} carries generation 0 (the reserved unset sentinel)"
            ),
            Self::DuplicateGenerationProducer { value, generation } => write!(
                f,
                "semantic value {value} generation {generation} has a second producer; a later write is a new generation"
            ),
            Self::IllegalRoot { root } => {
                write!(f, "root {root} is not a declared launch")
            }
            Self::DuplicateRoot { root } => {
                write!(f, "root {root} is declared more than once")
            }
            Self::SelfDependency { launch } => write!(
                f,
                "dependency edge runs from launch {launch} to itself"
            ),
            Self::UnknownDependencyEndpoint { launch } => write!(
                f,
                "dependency edge names launch {launch}, which does not exist"
            ),
            Self::DuplicateDependencyEdge {
                producer,
                consumer,
            } => write!(
                f,
                "dependency edge {producer} → {consumer} is declared more than once"
            ),
            Self::DependencyCycle { edge } => write!(
                f,
                "dependency graph contains a cycle through launch {consumer} (edge {producer} → {consumer})",
                producer = edge.producer,
                consumer = edge.consumer,
            ),
            Self::MissingDependency { launch, value } => write!(
                f,
                "launch {launch} produces semantic value {value} but is not reachable from any root (missing dependency)"
            ),
            Self::IllegalRelationRow { primal, reason } => write!(
                f,
                "relation row for primal DefId({}) is illegal: {reason}",
                primal.0
            ),
            Self::DuplicateRelationRow { primal } => write!(
                f,
                "duplicate relation rows for primal DefId({}); duplicate or contradictory rows fail closed",
                primal.0
            ),
            Self::ContradictoryRelationRow {
                primal,
                field,
                first,
                second,
            } => write!(
                f,
                "relation rows for primal DefId({}) contradict on {field}: first {first}, second {second}",
                primal.0
            ),
            Self::ContradictoryInitialization {
                buffer,
                first,
                second,
            } => write!(
                f,
                "buffer {buffer} declares conflicting initialization policies: first {first}, second {second}"
            ),
            Self::ObservationOutOfRange { launch } => write!(
                f,
                "observation names launch {launch}, which does not exist"
            ),
            Self::UnknownObservationBuffer { buffer } => write!(
                f,
                "observation names buffer {buffer}, which no semantic value is bound to"
            ),
            Self::ResultWithoutObservationFact {
                result,
                buffer,
                version,
            } => write!(
                f,
                "result {result} exposes buffer {buffer} version {version} as a readback, but the carried semantics declare no observation fact for it; a writable intermediate or persistent state is a result only through an explicit observation fact (F6)"
            ),
            Self::UndeclaredObservationFact {
                buffer,
                version,
                at_launch,
            } => write!(
                f,
                "observation fact declares buffer {buffer} version {version} read back at launch {at_launch}, but no result row declares it (F6)"
            ),
            Self::DanglingConsumer { launch } => write!(
                f,
                "launch {launch} is not a declared root and has no incoming dependency edge — a dangling consumer with no justifying producer (F3)"
            ),
            Self::UnboundEdgeBuffer { buffer } => write!(
                f,
                "dependency edge flows buffer {buffer}, which no semantic value is bound to (F3)"
            ),
            Self::MissingProducer { producer, value } => write!(
                f,
                "a consumer edge references semantic value {value} but its producer launch {producer} never writes it (missing producer) (F3)"
            ),
            Self::EdgeGenerationShapeMismatch {
                value,
                producer,
                version,
            } => write!(
                f,
                "the consumer edge for semantic value {value} flows buffer version {version} whose shape disagrees with the generation launch {producer} writes for the value (contract rule 3)"
            ),
            Self::AmbiguousEdgeGeneration {
                value,
                producer,
                version,
            } => write!(
                f,
                "the consumer edge for semantic value {value} cannot bind a generation: launch {producer} writes several generations matching buffer version {version} (contract rule 3)"
            ),
            Self::UninitializedBufferRead { buffer, launch } => write!(
                f,
                "launch {launch} reads buffer {buffer} before any initialization is declared — a first read before initialization fails closed (resource axes rule 1)"
            ),
            Self::ObservationPointBufferNotObserved { buffer } => write!(
                f,
                "buffer {buffer} declares the read-back lifetime class but is never declared observed — a use outside its declared allocation lifetime fails closed (resource axes rule 3)"
            ),
            Self::OutputExtentUnderfilled {
                kernel,
                group,
                binding,
                element_count,
                dispatched,
            } => write!(
                f,
                "kernel {kernel} output slot ({group}, {binding}) has its own bounds/index domain of {element_count} elements, but the shared dispatch extent covers only {dispatched} lanes — per-output bounds under-fill fails closed (per-output domains rules 1-3)"
            ),
            Self::ContradictoryCompanionFact {
                primal,
                field,
                first,
                second,
            } => write!(
                f,
                "companion relation for primal DefId({}) is contradictory vs the primal's recorded facts on {field}: first {first}, second {second} (companion identity rule 4)",
                primal.0
            ),
            Self::StaleCompanionFact {
                value,
                bound_generation,
                latest_generation,
            } => write!(
                f,
                "a consumer binds generation {bound_generation} of semantic value {value} while the producer carries the newer generation {latest_generation} — stale companion consumption of superseded primal state fails closed (companion identity rule 5)"
            ),
            Self::MissingCompanionFact { primal, reason } => write!(
                f,
                "the companion declared for primal DefId({}) is a missing companion fact: {reason} (companion identity rule 6; a forward-only program declares no companions and needs none)",
                primal.0
            ),
            Self::RegionCrossesBoundary { region, launch } => write!(
                f,
                "prepared submission region {region} contains the observation point at launch {launch} in its interior — a prepared submission region cannot cross an observation boundary; the candidate group is split at that point or the program fails closed (prepared submission regions rule 3)"
            ),
            Self::RegionMembersDisconnected { region } => write!(
                f,
                "prepared submission region {region} has members that are not path-connected through the carried dependency graph — a candidate group spanning disconnected launches fails closed (prepared submission regions rule 1)"
            ),
            Self::RegionUnassignedLaunch { launch } => write!(
                f,
                "launch {launch} is not assigned to exactly one prepared submission region — an incomplete or overlapping region partition fails closed (prepared submission regions rules 1/4)"
            ),
            Self::UnknownDeviceEffectLaunch { launch } => write!(
                f,
                "device effect-boundary fact names launch {launch}, which is not in the device program (device/AIR legality)"
            ),
            Self::DeviceFunctionHostileEffect { launch, kind } => write!(
                f,
                "device function (launch {launch}) carries the device-hostile effect {} — device functions cannot contain ad, Sermo, cancellation, or unresolved host-effect operations (device/AIR legality)",
                kind.spelling()
            ),
        }
    }
}

impl std::error::Error for DeviceProgramError {}

impl fmt::Display for BufferId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BufferId({})", self.0)
    }
}

impl fmt::Display for LaunchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LaunchId({})", self.0)
    }
}

impl fmt::Display for RegionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RegionId({})", self.0)
    }
}
