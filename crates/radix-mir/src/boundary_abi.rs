//! Versioned host→device call ABI + compile-time boundary validation (NGAB1-U2).
//!
//! The NGAB0 §Abi boundary is a **versioned typed surface**: one host call →
//! one prepared submission region containing one or more kernels, the region —
//! not the kernel — being the call's submission unit (NGAB0-R1 amendment).
//! This module owns that surface and its enforcement:
//!
//! - [`HOST_CALL_ABI_VERSION`] — the boundary's own version ratchet. It
//!   **rides the accepted wire version** (the MD2-W1 sibling-field precedent:
//!   a sibling field with its own ratchet, no
//!   `WIRE_DEVICE_PROGRAM_VERSION` bump); the faber wire seam asserts
//!   `HOST_CALL_ABI_VERSION == DEVICE_RUN_PLAN_VERSION`.
//! - [`PreparedRegion`] + [`BoundaryCall`] — the amended granularity: one
//!   host function invokes one prepared region containing the kernels its
//!   call statements target.
//! - [`validate_boundary_abi`] — the compile-time gate: invalid cross-boundary
//!   values (wrong type, shape, lifetime, mutation of a read-only resource,
//!   observation of unlaunched work) are rejected with a typed
//!   [`BoundaryAbiError`] before any launch. The checks consume typed MIR
//!   facts only (the call statement's callee, operands, destination, and the
//!   kernel's declared param types) — never emitted text (§Partition/§Abi
//!   "never reconstructed").
//!
//! # The five boundary classes
//!
//! Each class is a typed diagnostic named in the NGAB1-U2 `done_when`:
//!
//! - **Type** — a crossing value's element type does not exactly equal the
//!   kernel's declared boundary element type. The boundary is typed-exact
//!   (§Abi: every value crossing carries its type fact); implicit widening a
//!   host call is allowed to do in ordinary MIR does not happen at the
//!   boundary, so a widened value is rejected here.
//! - **Shape** — a crossing value's element count does not equal the kernel's
//!   declared boundary element count (or the count is not statically
//!   resolvable). The boundary is static-typed: a shape the compiler cannot
//!   prove is a shape violation.
//! - **Lifetime** — the boundary call's output is written into a host
//!   **function parameter**: the region's output would land in caller-owned
//!   storage, escaping the program's session lifetime (`§ResourceIdentity`: no
//!   resource outlives its session; the region owns its outputs). At the
//!   source level the semantic analyzer already blocks parameter writes
//!   (SEM020); the gate guards MIR-level producers.
//! - **Mutation** — the boundary call's output is routed into the storage of
//!   a read-only input value: the kernel's output lands in the same storage
//!   that held a crossing input (the device program binds the input and
//!   output as distinct buffers; one storage cannot hold a read-only input
//!   AND a written output). Direct destination aliasing and the value-flow
//!   `_1 = %t` copy (a call's temp output assigned into an argument local)
//!   are both rejected.
//! - **Observation** — a boundary call passes the output of an earlier call
//!   to the same kernel as an input: the host observes the region's declared
//!   result buffer while the program's single launch for that kernel is the
//!   producer — the observed work is not launched ahead of the consuming
//!   input (§Abi cross-boundary validity: invalid values fail at compile
//!   time for static facts; never silently reach launch).
//!
//! # Enforcement surface
//!
//! Every class is enforced with a typed [`BoundaryAbiError`] from
//! [`validate_boundary_abi`], which `derive_host_partition` runs after the
//! device program derives — the compile-time gate before any composite
//! build/link or launch. The faber seam maps the classes onto
//! `E_DEVICE_BOUNDARY_ABI` diagnostics (`class` arg); the source-reachable
//! classes (type, mutation, observation) are proven by faber negative
//! fixtures on the NGAB0-U11 shape, and the typed diagnostics of every class
//! are proven by the radix-mir unit tests.

use crate::abi::device_view_element_count;
use crate::device_program::{BufferRole, LaunchId};
use crate::device_semantics::DeviceEffectBoundaryKind;
use crate::host_partition::{CallsiteId, HostPartition, PartitionRecord, callee_function_ids};
use crate::layout::MirLayoutKind;
use crate::{
    MirBlockId, MirCallee, MirConstant, MirFunction, MirFunctionId, MirLocalId, MirOperand,
    MirPlace, MirPlaceBase, MirProgram, MirStatementKind, MirTempId, MirType, MirValidationContext,
    MirValueId, MirValueKind,
};
use radix_types::Type;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The versioned host→device call-ABI ratchet (NGAB0 §Abi — "Versioned").
///
/// The boundary is a versioned surface with its own ratchet that **rides the
/// accepted wire version** (the MD2-W1 sibling-field precedent: no
/// `WIRE_DEVICE_PROGRAM_VERSION` bump). The faber wire seam asserts
/// `HOST_CALL_ABI_VERSION == DEVICE_RUN_PLAN_VERSION`, so a wire-version bump
/// moves this constant in lockstep.
pub const HOST_CALL_ABI_VERSION: u32 = 8;

/// One prepared submission region — the call's submission unit (NGAB0-R1).
///
/// One host call submits one region containing one or more kernels; the
/// one-kernel case is the minimal prepared region, not a separate ABI shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRegion {
    /// The kernels the region submits, in the host call's invocation order.
    pub kernels: Vec<MirFunctionId>,
}

impl PreparedRegion {
    /// Whether the region is the minimal one-kernel case.
    #[must_use]
    pub fn is_minimal(&self) -> bool {
        self.kernels.len() == 1
    }
}

/// One declared cross-boundary call: a host function invoking one prepared
/// submission region (NGAB0 §Partition/§Abi — one host call → one region).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryCall {
    /// The calling host-side function.
    pub host: MirFunctionId,
    /// The prepared submission region the call submits.
    pub region: PreparedRegion,
}

/// The typed diagnostic class of one invalid cross-boundary value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryAbiKind {
    /// A crossing value's element type does not equal the declared boundary
    /// element type.
    Type,
    /// A crossing value's element count does not equal the declared count.
    Shape,
    /// The boundary call writes into caller-owned parameter storage.
    Lifetime,
    /// The boundary call mutates a read-only input resource.
    Mutation,
    /// The host observes a region result before its producing work launched.
    Observation,
    /// The boundary call's callee shape cannot be lowered to the prepared
    /// region's direct call — a `Definition` or non-function `Value` callee
    /// targeting a device kernel. The direct region call is the ONLY emitted
    /// boundary form (DDCP3-U5 / gate 6); the gate admits exactly the
    /// direct-call shapes (`Function`, a function-constant `Value`, or a
    /// `Closure`) and rejects a callsite the lane cannot map to that form
    /// before emission.
    CallsiteShape,
}

impl BoundaryAbiKind {
    /// The stable diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Shape => "shape",
            Self::Lifetime => "lifetime",
            Self::Mutation => "mutation",
            Self::Observation => "observation",
            Self::CallsiteShape => "callsite-shape",
        }
    }
}

/// A fail-closed typed diagnostic for one invalid cross-boundary value.
///
/// Every variant names the host function, the kernel, and the typed fact that
/// failed — nothing is text-parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryAbiError {
    /// The call's argument count does not equal the kernel's declared param
    /// count (a malformed call shape).
    Arity {
        /// The calling host-side function.
        host: MirFunctionId,
        /// The invoked device kernel.
        kernel: MirFunctionId,
        /// Expected parameter count.
        expected: usize,
        /// Actual argument count.
        found: usize,
    },
    /// A crossing value's element type does not exactly equal the kernel's
    /// declared boundary element type.
    Type {
        /// The calling host-side function.
        host: MirFunctionId,
        /// The invoked device kernel.
        kernel: MirFunctionId,
        /// The declared param position (0-based).
        position: usize,
        /// The kernel's declared boundary element type.
        expected: MirType,
        /// The crossing value's carried element type.
        found: MirType,
    },
    /// A crossing value's element count does not equal the kernel's declared
    /// boundary element count, or the count is not statically resolvable.
    Shape {
        /// The calling host-side function.
        host: MirFunctionId,
        /// The invoked device kernel.
        kernel: MirFunctionId,
        /// The declared param position (0-based).
        position: usize,
        /// The kernel's declared boundary element count.
        expected_count: u64,
        /// The crossing value's carried element count, when resolvable.
        found_count: Option<u64>,
    },
    /// The boundary call writes its output into a host function parameter —
    /// the region's output escapes into caller-owned storage.
    Lifetime {
        /// The calling host-side function.
        host: MirFunctionId,
        /// The invoked device kernel.
        kernel: MirFunctionId,
        /// The parameter local the output is written into.
        parameter: MirLocalId,
    },
    /// The boundary call's destination aliases an argument place — the
    /// kernel's output is written into the storage of a read-only input
    /// value.
    Mutation {
        /// The calling host-side function.
        host: MirFunctionId,
        /// The invoked device kernel.
        kernel: MirFunctionId,
        /// The aliased local.
        local: MirLocalId,
    },
    /// A boundary call passes the output of an earlier call to the same
    /// kernel as an input — the host observes the region's result buffer
    /// while the program's single launch for that kernel is the producer.
    Observation {
        /// The calling host-side function.
        host: MirFunctionId,
        /// The invoked device kernel.
        kernel: MirFunctionId,
        /// The local holding the earlier call's output.
        local: MirLocalId,
    },
    /// A boundary call whose callee the direct region-call ABI cannot lower
    /// — a `Definition` or non-function `Value` callee targeting a device
    /// kernel. The host-partition lane emits exactly ONE boundary form (the
    /// direct call to the prepared-region symbol); only the direct-call
    /// shapes (`Function`, a function-constant `Value`, or a `Closure` —
    /// each mapping to the kernel id) lower to that form, so any other
    /// callee is rejected before emission (DDCP3-U5 / gate 6) — never a
    /// malformed module with a call to a never-emitted kernel body.
    UnlowerableCallsiteShape {
        /// The calling host-side function.
        host: MirFunctionId,
        /// The invoked device kernel.
        kernel: MirFunctionId,
    },
}

impl BoundaryAbiError {
    /// The typed class of this diagnostic.
    #[must_use]
    pub fn class(&self) -> BoundaryAbiKind {
        match self {
            Self::Arity { .. } | Self::Shape { .. } => BoundaryAbiKind::Shape,
            Self::Type { .. } => BoundaryAbiKind::Type,
            Self::Lifetime { .. } => BoundaryAbiKind::Lifetime,
            Self::Mutation { .. } => BoundaryAbiKind::Mutation,
            Self::Observation { .. } => BoundaryAbiKind::Observation,
            Self::UnlowerableCallsiteShape { .. } => BoundaryAbiKind::CallsiteShape,
        }
    }
}

impl fmt::Display for BoundaryAbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arity {
                host,
                kernel,
                expected,
                found,
            } => write!(
                f,
                "boundary call f{} → kernel f{} passes {found} arguments but the kernel declares {expected} parameters (wrong call shape)",
                host.0, kernel.0
            ),
            Self::Type {
                host,
                kernel,
                position,
                expected,
                found,
            } => write!(
                f,
                "boundary call f{} → kernel f{} argument {position} carries element type ty#{} but the boundary declares ty#{} (wrong type)",
                host.0,
                kernel.0,
                found.semantic_id().0,
                expected.semantic_id().0
            ),
            Self::Shape {
                host,
                kernel,
                position,
                expected_count,
                found_count,
            } => write!(
                f,
                "boundary call f{} → kernel f{} argument {position} carries element count {} but the boundary declares {expected_count} (wrong shape)",
                host.0,
                kernel.0,
                found_count.map_or_else(|| "unresolvable".to_owned(), |count| count.to_string(),),
            ),
            Self::Lifetime {
                host,
                kernel,
                parameter,
            } => write!(
                f,
                "boundary call f{} → kernel f{} writes its output into host parameter _{}; the region's output escapes into caller-owned storage (wrong lifetime)",
                host.0, kernel.0, parameter.0
            ),
            Self::Mutation {
                host,
                kernel,
                local,
            } => write!(
                f,
                "boundary call f{} → kernel f{} writes its output into the same storage as its input _{}; the device input slot is read-only, writing it mutates a read-only resource",
                host.0, kernel.0, local.0
            ),
            Self::Observation {
                host,
                kernel,
                local,
            } => write!(
                f,
                "boundary call f{} → kernel f{} passes the kernel's own output (from _{} ) back in as an input; the region result is observed before the producing launch ran (observation of unlaunched work)",
                host.0, kernel.0, local.0
            ),
            Self::UnlowerableCallsiteShape { host, kernel } => write!(
                f,
                "boundary call f{} → kernel f{} goes through a callee the direct region-call ABI cannot lower (a Definition or non-function Value callee) — the prepared-region direct call is the only emitted boundary form (unsupported callsite shape)",
                host.0, kernel.0
            ),
        }
    }
}

impl std::error::Error for BoundaryAbiError {}

/// Derive the declared cross-boundary calls: every host function's prepared
/// submission region, containing the device kernels its call statements
/// target (in invocation order, deduplicated).
///
/// The call surface is a typed MIR fact (the call statement's callee), never
/// reconstructed from emitted text or naming conventions.
#[must_use]
pub fn derive_boundary_calls(
    program: &MirProgram,
    device_kernels: &[MirFunctionId],
) -> Vec<BoundaryCall> {
    let mut calls = Vec::new();
    for function in &program.functions {
        if device_kernels.contains(&function.id) {
            continue;
        }
        let mut region = Vec::new();
        for block in &function.blocks {
            for statement in &block.statements {
                let MirStatementKind::Call { callee, .. } = &statement.kind else {
                    continue;
                };
                for target in callee_function_ids(callee, program) {
                    if device_kernels.contains(&target) && !region.contains(&target) {
                        region.push(target);
                    }
                }
            }
        }
        if !region.is_empty() {
            calls.push(BoundaryCall {
                host: function.id,
                region: PreparedRegion { kernels: region },
            });
        }
    }
    calls
}

// ── DDCP3-U4: the typed `§ArtifactPacket` `HostDeviceCall` surface ──────────

/// The bounded numeric invocation-field layout of one prepared region's
/// direct call (the typed `§ArtifactPacket` `HostDeviceCall` argument-layout
/// fact; `§HotPathGate` rules 1 + 3). The layout is **bounded numeric fields
/// only** — the direct call carries the dispatch extent against the static
/// region symbol, never a string, map, `Valor`, route/backend tag, or tensor
/// payload. Every field value is fixed at preparation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionArgumentLayout {
    /// The region's dispatch extent (total work-items) — the bounded numeric
    /// invocation field the direct call carries.
    pub dispatch_extent: u64,
}

/// One member kernel/entrypoint identity of a typed [`HostDeviceCall`]
/// (DDCP3-U4): the region's member kernels plus their typed logical
/// entrypoint identities — carried facts, never reconstructed from emitted
/// text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberKernel {
    /// The member kernel's MIR function identity.
    pub kernel: MirFunctionId,
    /// The member kernel's logical entrypoint identity (typed fact).
    pub entry: String,
}

/// The program-level role of one typed [`CallBinding`] — the wire/typed
/// mirror of [`BufferRole`] (DDCP3-U4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallBindingRole {
    /// Host-provided input; every slot accesses it read-only.
    Input,
    /// Device-produced result; read back at an observation point.
    Output,
    /// Device-resident intermediate shared across kernels.
    InOut,
}

impl CallBindingRole {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::InOut => "in-out",
        }
    }

    /// The canonical domain tag used by the execution-descriptor bytes.
    fn domain_tag(self) -> u8 {
        match self {
            Self::Input => 1,
            Self::Output => 2,
            Self::InOut => 3,
        }
    }
}

impl From<BufferRole> for CallBindingRole {
    fn from(role: BufferRole) -> Self {
        match role {
            BufferRole::Input => Self::Input,
            BufferRole::Output => Self::Output,
            BufferRole::InOut => Self::InOut,
        }
    }
}

/// One typed input/output/resource binding of a call's prepared region
/// (DDCP3-U4 / `§ArtifactPacket` `HostDeviceCall` — typed input/output/resource
/// bindings; nothing on the `§HotPathGate` ABI denylist). Typed facts, never
/// reconstructed from emitted text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallBinding {
    /// The program buffer identity (`BufferId` value — the identity key,
    /// never the buffer name).
    pub buffer: u32,
    /// The buffer's program role.
    pub role: CallBindingRole,
    /// The slot's target-neutral first-level binding index.
    pub group: u32,
    /// The slot's second-level binding index within the group.
    pub binding: u32,
    /// The slot's element count (shape fact).
    pub element_count: u64,
    /// The slot's element type (semantic type id — a typed fact).
    pub element_type: u32,
}

/// The recorded failure/cancellation behavior of a typed call (DDCP3-U4 /
/// `§ArtifactPacket` `HostDeviceCall` — failure/cancellation behavior):
/// recorded and decided before the hot loop, never on the hot path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallFailureBehavior {
    /// The region is prepared outside the hot loop; a cache miss prepares
    /// before the hot call (`§ArtifactPacket` cache-miss behavior; `§HotPathGate`
    /// rules 3 + 5).
    PrepareOutsideHotLoop,
    /// The hot call fails closed on a miss — the hot path never interprets a
    /// kernel on a cache miss (`§ArtifactPacket`; `§HotPathGate` rule 5).
    FailClosed,
}

impl CallFailureBehavior {
    /// The canonical domain tag used by the execution-descriptor bytes.
    fn domain_tag(self) -> u8 {
        match self {
            Self::PrepareOutsideHotLoop => 1,
            Self::FailClosed => 2,
        }
    }
}

/// The execution-descriptor domain tag — the typed call/region-descriptor
/// identity domain (`§IdentityDomains` row 4; CTO9-5). Folded into the
/// canonical descriptor bytes so the execution-descriptor domain can never
/// collide with an artifact digest domain (`content_sha256`/`packet_sha256`).
const EXECUTION_DESCRIPTOR_DOMAIN_TAG: &[u8] = b"faber.execution-descriptor.v1\0";

/// The typed `§ArtifactPacket` `HostDeviceCall` shape (radix side; DDCP3-U4).
///
/// The frozen field table as typed facts (ddcp0-contract `§ArtifactPacket`):
/// host callsite identity (static — no route string, `Valor`, descriptor
/// map, backend tag, or kernel name on the hot path), prepared
/// submission-region identity, typed input/output/resource bindings (nothing
/// on the `§HotPathGate` ABI denylist), the bounded numeric argument layout,
/// the recorded effect/observation boundary, and the failure/cancellation
/// behavior (recorded and decided before the hot loop). The `region`
/// identity is the compiler-owned stable region index — the SAME index the
/// host-partition lane's direct region symbols carry
/// (`__faber_prepared_region_{index}`, DDCP3-U2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDeviceCall {
    /// Host callsite identity (the stable (host, block, statement) identity —
    /// the region's first callsite in the partition record; the canonical
    /// callsite of the boundary call).
    pub callsite: CallsiteId,
    /// Prepared submission-region identity (stable 1-based region index).
    pub region: u32,
    /// The region's member kernel/entrypoint identities (typed facts).
    pub members: Vec<MemberKernel>,
    /// Typed input/output/resource bindings of the region.
    pub bindings: Vec<CallBinding>,
    /// The bounded numeric argument layout of the region's direct call
    /// (fixed at preparation; `§HotPathGate` rule 3).
    pub argument_layout: RegionArgumentLayout,
    /// The recorded effect/observation boundary kinds of the region's member
    /// launches (empty for a legal program — the fail-closed envelope; a
    /// prepared region never crosses `ad`/Sermo/observation/cancellation).
    pub effect_boundary: Vec<DeviceEffectBoundaryKind>,
    /// Failure/cancellation behavior — recorded and decided before the hot
    /// loop.
    pub failure_behavior: CallFailureBehavior,
}

impl HostDeviceCall {
    /// The canonical descriptor bytes of this typed call — the
    /// canonical-descriptor-equality substrate (`§IdentityDomains` rule 6 /
    /// `§HotPathGate` rule 6) and the input to
    /// [`execution_descriptor_hash`]. The bytes begin with the
    /// execution-descriptor domain tag, so the execution-descriptor domain is
    /// type-level distinct from the artifact digest domains.
    #[must_use]
    pub fn canonical_descriptor_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(EXECUTION_DESCRIPTOR_DOMAIN_TAG);
        bytes.extend_from_slice(&self.region.to_le_bytes());
        bytes.extend_from_slice(&self.argument_layout.dispatch_extent.to_le_bytes());
        for member in &self.members {
            bytes.extend_from_slice(&member.kernel.0.to_le_bytes());
            push_len_prefixed_bytes(&mut bytes, member.entry.as_bytes());
        }
        for binding in &self.bindings {
            bytes.extend_from_slice(&binding.buffer.to_le_bytes());
            bytes.push(binding.role.domain_tag());
            bytes.extend_from_slice(&binding.group.to_le_bytes());
            bytes.extend_from_slice(&binding.binding.to_le_bytes());
            bytes.extend_from_slice(&binding.element_count.to_le_bytes());
            bytes.extend_from_slice(&binding.element_type.to_le_bytes());
        }
        for kind in &self.effect_boundary {
            push_len_prefixed_bytes(&mut bytes, kind.spelling().as_bytes());
        }
        bytes.push(self.failure_behavior.domain_tag());
        bytes
    }

    /// The typed execution-descriptor identity of this call — the distinct
    /// typed domain of `§IdentityDomains` row 4 (CTO9-5). Computed over the
    /// canonical descriptor bytes; identical typed calls always produce
    /// identical descriptors (canonical), and the digest is a
    /// collision-resistant SHA-256 — never FNV.
    #[must_use]
    pub fn execution_descriptor(&self) -> ExecutionDescriptor {
        ExecutionDescriptor(execution_descriptor_hash(
            &self.canonical_descriptor_bytes(),
        ))
    }
}

/// The typed execution-descriptor identity of one host→device call
/// (CTO9-5 / `§IdentityDomains` row 4 — `execution_descriptor_hash`).
///
/// **Type-level distinction from artifact identity:** this is a newtype over
/// the descriptor digest, never a bare artifact-digest string — it can never
/// be passed where a `content_sha256`/`packet_sha256` artifact identity is
/// expected (and vice versa). The descriptor identifies the typed call/region
/// descriptor (its call semantics); artifact materialization can never change
/// its call semantics. Loaded-module + artifact cache/session identity
/// consumes `content_sha256`/`packet_sha256` — never this type and never FNV.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExecutionDescriptor(String);

impl ExecutionDescriptor {
    /// The canonical `sha256:<64-hex>` digest spelling.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.0
    }
}

/// The collision-resistant digest of the execution-descriptor domain
/// (CTO9-5 / `§IdentityDomains` row 4): a SHA-256 over the canonical descriptor
/// bytes, canonical `sha256:<64-hex>` spelling — never FNV. The
/// execution-descriptor identity is a **distinct typed domain/purpose** from
/// artifact identity (`content_sha256`/`packet_sha256`): it identifies the
/// typed call/region descriptor so artifact materialization can never change
/// its call semantics; loaded-module + artifact cache/session identity
/// consumes `content_sha256`/`packet_sha256`, never this value and never FNV.
#[must_use]
pub fn execution_descriptor_hash(canonical_descriptor_bytes: &[u8]) -> String {
    format!(
        "sha256:{}",
        handroll::sha256_hex(canonical_descriptor_bytes)
    )
}

/// Length-prefixed canonical byte encoding of one descriptor field (the
/// canonical-descriptor-equality substrate).
fn push_len_prefixed_bytes(out: &mut Vec<u8>, field: &[u8]) {
    let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(field);
}

/// The launch ids of the member kernels of a prepared region (typed launch
/// facts — the observation/effect-boundary mapping source).
pub(crate) fn region_launch_ids(
    partition: &HostPartition,
    member_kernels: &[MirFunctionId],
) -> Vec<LaunchId> {
    partition
        .device_program
        .launches
        .iter()
        .filter(|launch| {
            partition
                .device_program
                .kernels
                .get(launch.kernel_index)
                .is_some_and(|unit| member_kernels.contains(&unit.function))
        })
        .map(|launch| launch.id)
        .collect()
}

/// Derive the typed `§ArtifactPacket` `HostDeviceCall` rows of a partition
/// (DDCP3-U4): one row per derived boundary call, from typed partition +
/// record facts — never reconstructed from emitted text. The bounded numeric
/// argument layout (dispatch extent), member kernel/entrypoint identities,
/// typed bindings, callsite identity, and the recorded effect-boundary kinds
/// all come from the typed surfaces. The callsite fallback (the boundary
/// call's host function with a sentinel block/statement) and the zero extent
/// are recorded defensive defaults for a record/partition that are
/// inconsistent with each other — the completeness gate
/// ([`crate::artifact_completeness::check_preparation_completeness`])
/// rejects such a shape; derived partitions always carry the real facts.
#[must_use]
pub fn derive_host_device_calls(
    partition: &HostPartition,
    record: &PartitionRecord,
) -> Vec<HostDeviceCall> {
    let mut calls = Vec::with_capacity(partition.boundary_calls.len());
    for (index, boundary) in partition.boundary_calls.iter().enumerate() {
        let region = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let mut members = Vec::new();
        let mut bindings = Vec::new();
        let mut dispatch_extent = 0;
        for (position, kernel) in boundary.region.kernels.iter().enumerate() {
            let Some(unit) = partition
                .device_program
                .kernels
                .iter()
                .find(|unit| unit.function == *kernel)
            else {
                continue;
            };
            members.push(MemberKernel {
                kernel: *kernel,
                entry: unit.entry.clone(),
            });
            for resource in &unit.resources {
                bindings.push(CallBinding {
                    buffer: resource.buffer.id.0,
                    role: CallBindingRole::from(resource.buffer.role),
                    group: resource.binding.group,
                    binding: resource.binding.binding,
                    element_count: resource.version.element_count,
                    element_type: resource.version.element_ty.semantic_id().0,
                });
            }
            if position == 0 {
                // The bounded numeric dispatch extent of the region's first
                // member kernel launch (the DDCP3-U2-frozen direct-call ABI).
                dispatch_extent =
                    unit.launch.workgroup_count.x * u64::from(unit.launch.workgroup.x);
            }
        }
        let callsite = record
            .callsites
            .iter()
            .find(|row| row.region == region)
            .map_or_else(
                || CallsiteId {
                    host: boundary.host,
                    block: MirBlockId(u32::MAX),
                    statement: u32::MAX,
                },
                |row| row.callsite,
            );
        let launch_ids = region_launch_ids(partition, &boundary.region.kernels);
        let effect_boundary = record
            .effect_boundaries
            .iter()
            .filter(|row| launch_ids.contains(&LaunchId(row.launch)))
            .map(|row| row.kind)
            .collect();
        calls.push(HostDeviceCall {
            callsite,
            region,
            members,
            bindings,
            argument_layout: RegionArgumentLayout { dispatch_extent },
            effect_boundary,
            failure_behavior: CallFailureBehavior::PrepareOutsideHotLoop,
        });
    }
    calls
}

/// Validate the cross-boundary call surface of a host/device partition.
///
/// Runs after general MIR validation, so the call's MIR signature already
/// type-checks; this gate rejects the cross-boundary violations the general
/// validator cannot see (typed-exact boundary element types, statically
/// provable shapes, output lifetime/aliasing, and observation of the region's
/// own result). Fails closed with the first [`BoundaryAbiError`] — the
/// compile-time rejection is the NGAB0 §Abi contract; nothing reaches launch.
///
/// # Errors
/// Returns a [`BoundaryAbiError`] for the first invalid cross-boundary value.
pub fn validate_boundary_abi(
    program: &MirProgram,
    calls: &[BoundaryCall],
    validation: &MirValidationContext<'_>,
) -> Result<(), BoundaryAbiError> {
    let function_index = program.function_index();
    for call in calls {
        let Some(host_function) = program.indexed_function(&function_index, call.host) else {
            continue;
        };
        for &kernel in &call.region.kernels {
            let Some(kernel_function) = program.indexed_function(&function_index, kernel) else {
                continue;
            };
            validate_host_calls(host_function, kernel_function, program, validation)?;
        }
    }
    Ok(())
}

/// Validate every call statement a host function makes into one kernel.
///
/// The host lowering is value-flow: a boundary call's result lands in a temp
/// (`%t = call kernel(_0)`) and a later `Assign` copies it into a local
/// (`_1 = %t`). The checks therefore track where a kernel's output flows:
/// a temp pending its local destination, and the locals that hold a kernel's
/// output. Direct local destinations (hand-built MIR) follow the same rules.
fn validate_host_calls(
    host: &MirFunction,
    kernel: &MirFunction,
    program: &MirProgram,
    validation: &MirValidationContext<'_>,
) -> Result<(), BoundaryAbiError> {
    let scope = HostFunctionScope::new(host);
    // Call-destination temps whose output has not yet been assigned into a
    // local: temp → (call args' base locals). A call's output is read-only
    // until the host moves it into fresh storage.
    let mut pending_outputs: FxHashMap<MirTempId, Vec<MirLocalId>> = FxHashMap::default();
    // Locals that hold a kernel's output (from an earlier boundary call into
    // this kernel): observing one of them as a call input is an observation
    // of unlaunched work — the program's single launch is the producer.
    let mut local_holds_output: Vec<MirLocalId> = Vec::new();
    for block in &host.blocks {
        let mut values: FxHashMap<MirValueId, MirType> = FxHashMap::default();
        for statement in &block.statements {
            match &statement.kind {
                MirStatementKind::Call {
                    destination,
                    callee,
                    args,
                } => {
                    let targets = callee_function_ids(callee, program);
                    if !targets.contains(&kernel.id) {
                        continue;
                    }
                    let destination_local = destination.as_ref().and_then(place_base_local);

                    // ── Unsupported callsite shape (DDCP3-U5 / gate 6) ─────
                    // The host-partition lane emits exactly ONE boundary form
                    // (the direct call to the prepared-region symbol with
                    // bounded numeric fields). A boundary call whose callee
                    // is not a direct-call shape — `Function`, a
                    // function-constant value, or a closure — cannot be
                    // lowered to that form, so it fails closed here, before
                    // emission, instead of producing a module that calls a
                    // never-emitted kernel body.
                    if !matches!(
                        callee,
                        MirCallee::Function(_)
                            | MirCallee::Value(MirOperand::Constant(MirConstant::Function(_)))
                            | MirCallee::Closure(_)
                    ) {
                        return Err(BoundaryAbiError::UnlowerableCallsiteShape {
                            host: host.id,
                            kernel: kernel.id,
                        });
                    }

                    // ── Wrong call shape (arity) ────────────────────────────
                    if args.len() != kernel.params.len() {
                        return Err(BoundaryAbiError::Arity {
                            host: host.id,
                            kernel: kernel.id,
                            expected: kernel.params.len(),
                            found: args.len(),
                        });
                    }

                    // ── Per-argument boundary checks (type + shape) ─────────
                    let arg_locals = args.iter().map(operand_base_local).collect::<Vec<_>>();
                    check_call_argument_boundary(args, kernel, host, validation, &values, &scope)?;

                    check_call_observation(
                        args,
                        host,
                        kernel,
                        &pending_outputs,
                        &local_holds_output,
                        &arg_locals,
                    )?;

                    check_call_destination(destination_local, &arg_locals, &scope, host, kernel)?;

                    // Record the call's pending output for the value-flow
                    // checks (the temp destination, or the direct local).
                    match destination.as_ref().and_then(place_base_temp) {
                        Some(temp) => {
                            pending_outputs
                                .insert(temp, arg_locals.iter().flatten().copied().collect());
                        }
                        None => {
                            if let Some(local) = destination_local
                                && !local_holds_output.contains(&local)
                            {
                                local_holds_output.push(local);
                            }
                        }
                    }
                }
                MirStatementKind::Assign { place, value } => {
                    // The value-flow copy: `_1 = %t` moves a call's pending
                    // output into a local. Routing the output into a read-only
                    // input value's storage is a mutation; a host parameter
                    // destination escapes the region's lifetime.
                    let Some(local) = place_base_local(place) else {
                        values.insert(value.id, value.ty);
                        continue;
                    };
                    if let MirValueKind::Operand(MirOperand::Temp(temp)) = &value.kind
                        && let Some(arg_locals) = pending_outputs.remove(temp)
                    {
                        if scope.parameter_local(local) {
                            return Err(BoundaryAbiError::Lifetime {
                                host: host.id,
                                kernel: kernel.id,
                                parameter: local,
                            });
                        }
                        if arg_locals.contains(&local) {
                            return Err(BoundaryAbiError::Mutation {
                                host: host.id,
                                kernel: kernel.id,
                                local,
                            });
                        }
                        if !local_holds_output.contains(&local) {
                            local_holds_output.push(local);
                        }
                        continue;
                    }
                    // A non-call-output assignment overwrites the local: it no
                    // longer holds a kernel output, so a later boundary call
                    // cannot observe stale region work through it.
                    values.insert(value.id, value.ty);
                    local_holds_output.retain(|candidate| *candidate != local);
                }
                // `RuntimeCall`/`Construct` produce no call-output or
                // statement-value facts — nothing to record here.
                _ => {}
            }
        }
    }
    Ok(())
}

/// Per-argument boundary checks: each carried (non-literal) argument's element
/// type and element count must match the kernel's declared boundary param.
fn check_call_argument_boundary(
    args: &[MirOperand],
    kernel: &MirFunction,
    host: &MirFunction,
    validation: &MirValidationContext<'_>,
    values: &FxHashMap<MirValueId, MirType>,
    scope: &HostFunctionScope<'_>,
) -> Result<(), BoundaryAbiError> {
    for (position, argument) in args.iter().enumerate() {
        // Constant literals carry their plain primitive in MIR
        // and the general validator already gated them against
        // the declared param type at the call site — the
        // boundary's exact-type check applies to carried
        // *values*, not literals.
        if matches!(argument, MirOperand::Constant(_)) {
            continue;
        }
        let Some(argument_ty) = operand_ty(scope, values, argument) else {
            continue;
        };
        let declared = &kernel.params[position];

        // Wrong type: the crossing value's element type must
        // exactly equal the kernel's declared boundary element
        // type.
        let argument_element = element_type_of(argument_ty, validation);
        let declared_element = element_type_of(declared.ty, validation);
        if let (Some(found), Some(expected)) = (argument_element, declared_element)
            && !validation
                .types
                .equals(found.semantic_id(), expected.semantic_id())
        {
            return Err(BoundaryAbiError::Type {
                host: host.id,
                kernel: kernel.id,
                position,
                expected,
                found,
            });
        }

        // Wrong shape: the crossing value's element count must
        // equal the kernel's declared boundary element count.
        // An aggregate or unresolvable count is a shape
        // violation — the boundary is static-typed.
        let argument_count = element_count_of(argument_ty, validation);
        let declared_count = element_count_of(declared.ty, validation);
        if let Some(expected_count) = declared_count
            && argument_count != Some(expected_count)
        {
            return Err(BoundaryAbiError::Shape {
                host: host.id,
                kernel: kernel.id,
                position,
                expected_count,
                found_count: argument_count,
            });
        }
    }
    Ok(())
}

/// Observation checks: no call input may hold this kernel's own earlier output
/// (the region result observed before its launch ran).
fn check_call_observation(
    args: &[MirOperand],
    host: &MirFunction,
    kernel: &MirFunction,
    pending_outputs: &FxHashMap<MirTempId, Vec<MirLocalId>>,
    local_holds_output: &[MirLocalId],
    arg_locals: &[Option<MirLocalId>],
) -> Result<(), BoundaryAbiError> {
    for arg_local in arg_locals.iter().flatten() {
        if local_holds_output.contains(arg_local) {
            return Err(BoundaryAbiError::Observation {
                host: host.id,
                kernel: kernel.id,
                local: *arg_local,
            });
        }
    }
    // A direct temp argument that is still an un-assigned call
    // output (`scale_kernel(scale_kernel(x))` shape) observes
    // the producing launch's result as an input.
    for argument in args {
        if let MirOperand::Temp(temp) = argument
            && pending_outputs.contains_key(temp)
        {
            return Err(BoundaryAbiError::Observation {
                host: host.id,
                kernel: kernel.id,
                local: MirLocalId(temp.0),
            });
        }
    }
    Ok(())
}

/// Destination checks: the call's output must not land in a host parameter
/// (a lifetime escape) nor be routed into the storage of a read-only input.
fn check_call_destination(
    destination_local: Option<MirLocalId>,
    arg_locals: &[Option<MirLocalId>],
    scope: &HostFunctionScope<'_>,
    host: &MirFunction,
    kernel: &MirFunction,
) -> Result<(), BoundaryAbiError> {
    // ── Lifetime: the output must not land in a host
    //    parameter (direct local destination). ─────────────
    if let Some(local) = destination_local
        && scope.parameter_local(local)
    {
        return Err(BoundaryAbiError::Lifetime {
            host: host.id,
            kernel: kernel.id,
            parameter: local,
        });
    }

    // ── Mutation: the output must not be routed into the
    //    storage of a read-only input value (direct aliasing
    //    or the value-flow `_1 = %t` pattern). ─────────────
    if let Some(local) = destination_local
        && arg_locals.contains(&Some(local))
    {
        return Err(BoundaryAbiError::Mutation {
            host: host.id,
            kernel: kernel.id,
            local,
        });
    }
    Ok(())
}

/// Resolve a call argument's MIR type from the host function's locals, temps,
/// and per-block statement values. Constant operands never reach here — the
/// caller skips them before resolving (the boundary's exact-type check applies
/// to carried values, not literals).
fn operand_ty(
    scope: &HostFunctionScope<'_>,
    values: &FxHashMap<MirValueId, MirType>,
    operand: &MirOperand,
) -> Option<MirType> {
    match operand {
        MirOperand::Place(place) => scope.place_ty(place),
        MirOperand::Temp(id) => scope.function.temp_ty(*id),
        MirOperand::Value(id) => values.get(id).copied(),
        MirOperand::Constant(_) => None,
    }
}

/// The element type of a boundary value, from its MIR type: a scalar carries
/// itself; a device view carries its tensor element type. Anything else has
/// no statically-resolvable crossing element type (`None` — the boundary
/// shape/type checks skip unresolved values; the general validator already
/// constrained the full type at the call site).
fn element_type_of(ty: MirType, validation: &MirValidationContext<'_>) -> Option<MirType> {
    let layout = validation.layouts.layout_for_type(ty).ok()?;
    match layout.kind {
        MirLayoutKind::Scalar(_) => Some(ty),
        MirLayoutKind::DeviceView(_) => tensor_element_type(ty, validation),
        _ => None,
    }
}

/// The element count of a boundary value: 1 for a scalar, the literal tensor
/// extent for a device view, otherwise unresolvable (`None`).
fn element_count_of(ty: MirType, validation: &MirValidationContext<'_>) -> Option<u64> {
    let layout = validation.layouts.layout_for_type(ty).ok()?;
    match layout.kind {
        MirLayoutKind::Scalar(_) => Some(1),
        MirLayoutKind::DeviceView(_) => device_view_element_count(ty, validation).ok(),
        _ => None,
    }
}

/// The tensor element type of a device-view type, unwrapping aliases.
fn tensor_element_type(ty: MirType, validation: &MirValidationContext<'_>) -> Option<MirType> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(element, _) => Some(MirType::semantic(*element)),
        Type::Alias(_, target) => tensor_element_type(MirType::semantic(*target), validation),
        _ => None,
    }
}

/// The base local of a place, when the place is a plain local (no projection
/// and no temp base) — the storage identity the boundary tracks for
/// lifetime/mutation/observation aliasing.
fn place_base_local(place: &MirPlace) -> Option<MirLocalId> {
    match place.base {
        MirPlaceBase::Local(local) if place.projections.is_empty() => Some(local),
        _ => None,
    }
}

/// The base temp of a place, when the place is a plain temp — the value-flow
/// destination a boundary call's output lands in before its `Assign` copy.
fn place_base_temp(place: &MirPlace) -> Option<MirTempId> {
    match place.base {
        MirPlaceBase::Temp(temp) if place.projections.is_empty() => Some(temp),
        _ => None,
    }
}

/// The base local of an operand, when it is a plain local place.
fn operand_base_local(operand: &MirOperand) -> Option<MirLocalId> {
    match operand {
        MirOperand::Place(place) => place_base_local(place),
        _ => None,
    }
}

/// Type lookup over one host function's params, locals, and temps — the
/// minimal scope the boundary validation needs to resolve crossing values.
struct HostFunctionScope<'a> {
    function: &'a MirFunction,
    parameter_locals: Vec<MirLocalId>,
}

impl<'a> HostFunctionScope<'a> {
    fn new(function: &'a MirFunction) -> Self {
        Self {
            function,
            parameter_locals: function.params.iter().map(|param| param.local).collect(),
        }
    }

    /// Whether a local is a host function parameter.
    #[must_use]
    fn parameter_local(&self, local: MirLocalId) -> bool {
        self.parameter_locals.contains(&local)
    }

    #[must_use]
    fn place_ty(&self, place: &MirPlace) -> Option<MirType> {
        match place.base {
            MirPlaceBase::Local(local) => self
                .function
                .locals
                .iter()
                .find(|candidate| candidate.id == local)
                .map(|candidate| candidate.ty)
                .or_else(|| {
                    // Parameters are modeled as locals; the boundary resolves
                    // a param place's type through the param list.
                    self.function
                        .params
                        .iter()
                        .find(|candidate| candidate.local == local)
                        .map(|candidate| candidate.ty)
                }),
            MirPlaceBase::Temp(temp) => self.function.temp_ty(temp),
        }
    }
}

#[cfg(test)]
#[path = "boundary_abi_test.rs"]
mod tests;
