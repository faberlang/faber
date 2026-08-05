use super::types::{BufferId, BufferLifetime, BufferRole, LaunchId, ObservationCadence};
use crate::abi::MirKernelResourceAccess;
use crate::device_semantics::{DependencyEdge, SemanticValueId};
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
