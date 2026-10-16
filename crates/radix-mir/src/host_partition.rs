//! Host/device partition and partition-record surfaces.
//!
//! The base partition is the NGAB1-U1 vertical slice. The serializable
//! partition record extends it with membership, region/callsite, and legality
//! facts for DDCP3-U3.
//!
//! Given one analyzed and validated MIR program, this surface partitions the
//! package into the **host side** (ordinary functions that become the native
//! executable) and the **device side** (explicit device kernels — functions
//! whose `shader_stage` is [`MirKernelShaderStage::Compute`], the `@ nucleum`
//! entry surface), and derives one **typed, target-neutral
//! [`DeviceProgram`]** for the device side from typed facts only (NGAB0
//! §Partition/§Abi: identity/type/lifetime facts are never reconstructed from
//! emitted LLVM/MSL/PTX text or naming conventions).
//!
//! # What is derived and why it is typed
//!
//! - **Device kernels**: the functions a package declares as device
//!   computation. Identity is the MIR function id plus the logical entry name
//!   (the function's source name) — a carried fact, never a text-derived
//!   symbol.
//! - **Resources**: one typed storage-buffer slot per ABI kernel resource
//!   (`MirKernelResource` → [`DeviceResource`] via the shared ABI synthesis
//!   [`MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry`]).
//!   Every slot carries its element type, element count, binding indices,
//!   role, and access as typed facts.
//! - **Launches**: the ordered launch sequence over the device kernels.
//! - **Lifetimes**: program lifetime [`DeviceProgramLifetime::SingleRun`];
//!   per-buffer lifetime `PerProgram` (one-shot boundary call, host copies
//!   in/out around the single run).
//! - **Observations**: an explicit [`ResultBuffer`] row for each output-role
//!   buffer, read back at [`ObservationCadence::EndOfRun`].
//! - **Boundary calls**: the declared cross-boundary calls — one host
//!   function invokes one prepared submission region containing one or more
//!   kernels ([`BoundaryCall`], NGAB0-R1 granularity). This is the NGAB0
//!   §Partition "one host call → one prepared region" surface, derived from
//!   MIR call statements (typed), never from emitted text. Every boundary
//!   call passes the versioned call-ABI gate
//!   ([`validate_boundary_abi`](crate::boundary_abi::validate_boundary_abi));
//!   invalid cross-boundary values fail at compile time with typed
//!   diagnostics (NGAB1-U2).
//!
//! The derived program is validated through the shared schema
//! ([`DeviceProgram::validate`]) before it is returned — a derived program
//! that violates the schema rules fails closed with the shared typed error
//! instead of reaching a consumer.
//!
//! # Shared hot-path serialization list (NGAB1-U3)
//!
//! The NGAB1 phase respects the accepted serialization list — **one named
//! owner per revision**, no parallel fork:
//!
//! | Surface | Named owner | Revision ratchet |
//! | --- | --- | --- |
//! | `DeviceProgram` (shared schema) | `radix-mir::device_program` — the single-writer schema authority | schema `validate` gates the derived program |
//! | FMIR wire (`WireDeviceProgram`) | `radix-mir-fmir::schema::wire` — the field-for-field wire mirror | `WIRE_DEVICE_PROGRAM_VERSION` |
//! | Materializer | `faber::package::device` — the constructor + host descriptor consumer | `DEVICE_RUN_PLAN_VERSION` (seam-asserted == wire) |
//!
//! This surface is a **producer** of the shared `DeviceProgram`: it composes
//! the schema from typed facts and hands the result to the shared validation
//! gate. It never forks the schema, the wire, or the materializer, and it
//! never parses emitted MSL/PTX text for identity/lifetime/mutation/
//! observation facts. The versioned call-ABI ratchet
//! (`HOST_CALL_ABI_VERSION`) rides the accepted wire version; the faber wire
//! seam asserts `HOST_CALL_ABI_VERSION == DEVICE_RUN_PLAN_VERSION`, so one
//! revision moves the schema, its wire mirror, the materializer, and the ABI
//! ratchet in lockstep under their named owners.

use crate::boundary_abi::{
    BoundaryAbiError, BoundaryCall, derive_boundary_calls, validate_boundary_abi,
};
use crate::device::MirDeviceFunctionMetadataMap;
use crate::device_program::LaunchId;
use crate::device_program::{
    DeviceProgram, DeviceProgramBuildError, DeviceProgramError, DeviceProgramLifetime,
    build_device_program,
};
use crate::device_semantics::{DeviceEffectBoundaryKind, DeviceLegalityFacts};
use crate::{
    MirBlockId, MirCallee, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram,
    MirStatementKind, ValidatedMir,
};
use radix_lexer::Interner;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Fail-closed error for host-partition derivation.
///
/// Every variant names the typed fact that failed; nothing is text-parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPartitionError {
    /// A device kernel's storage-buffer ABI signature could not be synthesized.
    KernelAbi {
        /// The offending kernel's MIR function id.
        function: MirFunctionId,
        /// The ABI synthesis detail.
        detail: String,
    },
    /// A device kernel's algorithmic plan could not be resolved.
    KernelPlan {
        /// The offending kernel's MIR function id.
        function: MirFunctionId,
        /// The plan-resolution detail.
        detail: String,
    },
    /// An ABI slot has no coherent program buffer role.
    BufferRole {
        /// The offending kernel's MIR function id.
        function: MirFunctionId,
        /// The slot's binding index.
        binding: u32,
        /// The ABI role/access pair that has no coherent program role.
        role: String,
        /// The ABI access of the slot.
        access: String,
    },
    /// The derived device program violates the shared schema rules.
    Program(DeviceProgramError),
    /// The shared constructor rejected a selected shape fold or companion
    /// relation before a host view could be formed.
    Constructor(DeviceProgramBuildError),
    /// The cross-boundary call surface violates the versioned call ABI
    /// (NGAB1-U2): an invalid cross-boundary value — wrong type, shape,
    /// lifetime, mutation of a read-only resource, or observation of
    /// unlaunched work — fails at compile time with a typed diagnostic.
    Boundary(BoundaryAbiError),
}

impl fmt::Display for HostPartitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KernelAbi { function, detail } => {
                write!(f, "kernel {} ABI synthesis failed: {detail}", function.0)
            }
            Self::KernelPlan { function, detail } => {
                write!(f, "kernel {} plan resolution failed: {detail}", function.0)
            }
            Self::BufferRole {
                function,
                binding,
                role,
                access,
            } => write!(
                f,
                "kernel {} binding {binding} has no coherent program buffer role ({role} {access})",
                function.0
            ),
            Self::Program(error) => write!(f, "derived device program failed validation: {error}"),
            Self::Constructor(error) => write!(f, "device program constructor: {error}"),
            Self::Boundary(error) => write!(f, "boundary call ABI: {error}"),
        }
    }
}

impl std::error::Error for HostPartitionError {}

impl From<DeviceProgramBuildError> for HostPartitionError {
    fn from(error: DeviceProgramBuildError) -> Self {
        match error {
            DeviceProgramBuildError::KernelAbi { function, detail } => {
                Self::KernelAbi { function, detail }
            }
            DeviceProgramBuildError::KernelPlan { function, detail } => {
                Self::KernelPlan { function, detail }
            }
            DeviceProgramBuildError::BufferRole {
                function,
                binding,
                role,
                access,
            } => Self::BufferRole {
                function,
                binding,
                role,
                access,
            },
            DeviceProgramBuildError::Program(error) => Self::Program(error),
            other => Self::Constructor(other),
        }
    }
}

/// The host/device partition of one analyzed program (NGAB0 §Partition).
///
/// `device_program` is the typed, target-neutral device side; `host_functions`
/// and `device_kernels` are the partition's two sides (MIR function ids — the
/// static split that survives lowering); `boundary_calls` is the declared
/// cross-boundary call surface (one host call → one prepared region, NGAB0-R1);
/// `call_abi_version` is the versioned typed boundary every call rides
/// ([`HOST_CALL_ABI_VERSION`](crate::boundary_abi::HOST_CALL_ABI_VERSION)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPartition {
    /// The device side: every explicit device kernel in the program.
    pub device_kernels: Vec<MirFunctionId>,
    /// The host side: every other function in the program.
    pub host_functions: Vec<MirFunctionId>,
    /// The typed, target-neutral device program derived from `device_kernels`.
    pub device_program: DeviceProgram,
    /// The declared cross-boundary calls (one host call → one prepared region).
    pub boundary_calls: Vec<BoundaryCall>,
    /// The versioned host→device call ABI the boundary rides.
    pub call_abi_version: u32,
}

impl HostPartition {
    /// Whether the program declares any device kernel.
    #[must_use]
    pub fn has_device(&self) -> bool {
        !self.device_kernels.is_empty()
    }
}

/// Derive the host/device partition of a validated MIR program.
///
/// The device side is every function whose `shader_stage` is
/// [`MirKernelShaderStage::Compute`] (the `@ nucleum` entry surface); the host
/// side is every other function. A package with no compute kernels yields an
/// empty partition (`has_device() == false`) — no device payload.
///
/// Source-less synthetic kernel closures are invisible to this entry point:
/// callers that lower kernel closures pass their explicit metadata carrier
/// through [`derive_host_partition_with_metadata`].
///
/// The typed [`DeviceProgram`] is composed from the shared ABI synthesis, the
/// shared plan pass, and the shared schema — never from emitted text:
///
/// - per kernel: [`MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry`]
///   (typed storage-buffer slots), [`kernel_plan_for_function`] (the typed
///   `MirCollectionOp → CollectionKernelPlan` bridge; a body with no recipe
///   op resolves to [`CollectionKernelPlan::Elementwise`]), and
///   [`KernelLaunchPlan::from_signature_and_function`] (the shared
///   workgroup/dispatch/launch derivation);
/// - resources: [`DeviceResource::from_kernel_resource`] maps each ABI
///   storage-buffer resource onto a typed device-program slot (role from
///   [`BufferRole::from_abi_role`], element type/count carried from the
///   resource);
/// - observations: one [`ResultBuffer`] per declared observation-point
///   buffer, produced by its kernel's launch at
///   [`ObservationCadence::PerStep`] (a single-run program's per-execution
///   readback);
/// - the composed program passes [`DeviceProgram::validate`] or derivation
///   fails closed.
///
/// # Errors
///
/// Returns a [`HostPartitionError`] when a kernel's ABI signature or plan
/// cannot be derived, an ABI slot has no coherent program role, or the
/// composed device program fails the shared schema validation.
pub fn derive_host_partition(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
) -> Result<HostPartition, HostPartitionError> {
    derive_host_partition_with_metadata(
        validated,
        interner,
        &MirDeviceFunctionMetadataMap::default(),
    )
}

/// [`derive_host_partition`] with the explicit metadata carrier for
/// source-less synthetic kernel closures (KCM-05): the constructor's
/// selection consumes the carrier facts, so a host-launched private closure
/// derives its one launch region while composed or uncalled synthetic
/// helpers never become launch rows.
///
/// # Errors
///
/// Same contract as [`derive_host_partition`].
pub fn derive_host_partition_with_metadata(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    device_function_metadata: &MirDeviceFunctionMetadataMap,
) -> Result<HostPartition, HostPartitionError> {
    let (device_program, _semantics, _steps) = build_device_program(
        validated,
        interner,
        &crate::device::MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        device_function_metadata,
    )
    .map_err(HostPartitionError::from)?;
    let program = validated.program();
    let device_kernels: Vec<MirFunctionId> = device_program
        .kernels
        .iter()
        .map(|kernel| kernel.function)
        .collect();
    let host_functions = program
        .functions
        .iter()
        .filter(|function| !device_kernels.contains(&function.id))
        .map(|function| function.id)
        .collect();
    let boundary_calls = derive_boundary_calls(program, &device_kernels);

    // NGAB1-U2: the versioned call-ABI gate. Invalid cross-boundary values
    // (wrong type, shape, lifetime, mutation of a read-only resource,
    // observation of unlaunched work) fail here — at compile time, before any
    // composite build/link or launch — with a typed diagnostic (NGAB0 §Abi
    // cross-boundary validity). No boundary call reaches a consumer
    // unvalidated.
    validate_boundary_abi(program, &boundary_calls, validated.validation())
        .map_err(HostPartitionError::Boundary)?;

    Ok(HostPartition {
        device_kernels,
        host_functions,
        device_program,
        boundary_calls,
        call_abi_version: crate::boundary_abi::HOST_CALL_ABI_VERSION,
    })
}

/// One function's role in the host/device partition (DDCP3-U3 gate bullet 1:
/// exact membership — every function is exactly one of host or device).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionRole {
    /// A host-side function (ordinary CPU function outside the device launch
    /// universe).
    Host,
    /// A device-side function (an explicit compute kernel).
    Device,
}

/// Exact membership of one function in the partition (DDCP3-U3 gate bullet 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionMember {
    /// The MIR function id.
    pub function: MirFunctionId,
    /// The function's partition role.
    pub role: PartitionRole,
    /// The device member's logical entrypoint identity (the kernel's source
    /// name — a carried typed fact); `None` for host members.
    pub entry: Option<String>,
}

/// Stable identity of one host callsite (the fire-13 region-identity
/// addendum): product/module (the record's `module` scope) + host function +
/// block + statement. Derived from typed MIR facts — never from emitted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallsiteId {
    /// The calling host-side function.
    pub host: MirFunctionId,
    /// The block containing the call statement.
    pub block: MirBlockId,
    /// The statement index within the block.
    pub statement: u32,
}

/// One prepared submission region of the record. The `region` index is the
/// **stable region identity within one analyzed module ordering**: the
/// boundary call's 1-based position in the derived partition (program order)
/// — the same compiler-owned index the host-partition lane's direct region
/// symbols carry (`__faber_prepared_region_{index}`, DDCP3-U2), so the record
/// cross-checks against the emitted module's direct calls. The index is a
/// program-order position, stable only within that module ordering — NOT a
/// durable cross-build/cross-module identity (fire-16 PIC-U2); a stronger
/// semantic-ID scheme (out of scope) is the path to durable identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionRecord {
    /// Stable region identity within one analyzed module ordering (1-based;
    /// the emitted region-symbol suffix). Not durable cross-build /
    /// cross-module.
    pub region: u32,
    /// The region's member kernels (MIR function ids, in invocation order).
    pub member_kernels: Vec<MirFunctionId>,
    /// The region's member logical entrypoint identities (typed facts).
    pub member_entries: Vec<String>,
}

/// One host callsite's mapping into its prepared submission region
/// (DDCP3-U3 gate bullet 1: host callsite → prepared region → member
/// kernel/entrypoint identities).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallsiteRecord {
    /// The stable callsite identity.
    pub callsite: CallsiteId,
    /// The prepared region the callsite submits (stable region identity
    /// within one analyzed module ordering).
    pub region: u32,
    /// The region's member kernel identities.
    pub member_kernels: Vec<MirFunctionId>,
    /// The region's member logical entrypoint identities.
    pub member_entries: Vec<String>,
}

/// One explicit effect-boundary row of the partition record (DDCP3-U3 gate
/// bullet 2). Derived from the landed [`DeviceLegalityFacts`] surface —
/// `launch` is the launch id and `kind` is the typed
/// [`DeviceEffectBoundaryKind`], never a text-derived fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectBoundaryRecord {
    /// The launch whose kernel carries the effect.
    pub launch: u32,
    /// The recorded device-hostile effect kind.
    pub kind: DeviceEffectBoundaryKind,
}

/// The typed, serializable partition/callsite record (DDCP3-U3 — the
/// compiler artifact proving gate bullets 1 + 2). Produced by the compiler
/// from typed facts only, never from emitted text.
///
/// Stable identity: `module` is the
/// product/module scope, regions carry the stable compiler-minted region
/// identity **within one analyzed module ordering**, and callsites carry the
/// stable (host, block, statement) identity. **Support boundary**: one
/// analyzed module per record — separately-linked multi-module shapes are
/// explicitly OUT of the DDCP3 claim (recorded in the DDCP3 evidence); this
/// record is not a general multi-module ABI promise. Region indices are
/// program-order positions, stable only within that module ordering — NOT
/// durable cross-build/cross-module identities (fire-16 PIC-U2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionRecord {
    /// Product/module scope of the record (the fire-13 stable identity
    /// scope). One analyzed module per record.
    pub module: String,
    /// Exact CPU/device membership — every function exactly one role.
    pub membership: Vec<PartitionMember>,
    /// The prepared submission regions (stable region identity within one
    /// analyzed module ordering + members).
    pub regions: Vec<RegionRecord>,
    /// Host callsite → prepared region → member kernel/entrypoint identities.
    pub callsites: Vec<CallsiteRecord>,
    /// The explicit effect boundaries the partition records (derived from
    /// the landed `DeviceLegalityFacts` surface; empty for a legal program —
    /// the fail-closed envelope).
    pub effect_boundaries: Vec<EffectBoundaryRecord>,
}

/// Derive the typed, serializable partition/callsite record (DDCP3-U3).
///
/// Extends [`derive_host_partition`]: the exact host/device membership, the
/// prepared submission regions, and the host callsite mapping (host callsite
/// → prepared region → member kernel/entrypoint identities) are derived from
/// the typed partition facts — never from emitted text (gate bullet 1). The
/// record's effect-boundary section is derived from the landed
/// [`DeviceLegalityFacts`] surface, and the legality gate runs at derivation
/// (gate bullet 2): a device-hostile fact on any device launch fails closed
/// with the named rule (DDCP1-U6) — a candidate region can never cross an
/// `ad`/Sermo/cancellation/host-effect boundary.
///
/// `module` is the product/module scope of the stable region/callsite
/// identity (fire-13 addendum) — region indices are stable only within one
/// analyzed module ordering (PIC-U2); separately-linked multi-module shapes
/// are out of the DDCP3 claim.
///
/// # Errors
///
/// Returns a [`HostPartitionError`] when the base partition cannot be
/// derived or the legality record fails the named-rule gate.
pub fn derive_partition_record(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    module: &str,
    legality: &DeviceLegalityFacts,
) -> Result<PartitionRecord, HostPartitionError> {
    derive_partition_record_with_metadata(
        validated,
        interner,
        module,
        legality,
        &MirDeviceFunctionMetadataMap::default(),
    )
}

/// Derive a partition record from the explicit source/synthetic function
/// metadata carrier used by the device-program constructor.
///
/// The metadata-aware entry point is required for lowered programs whose
/// launchability is carried by KCM facts rather than inferred from the
/// compute-stage marker. The legacy [`derive_partition_record`] wrapper keeps
/// the fail-closed empty-carrier behavior for callers that have no carrier.
///
/// # Errors
///
/// Returns a [`HostPartitionError`] when the base partition cannot be
/// derived or the legality record fails the named-rule gate.
pub fn derive_partition_record_with_metadata(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    module: &str,
    legality: &DeviceLegalityFacts,
    device_function_metadata: &MirDeviceFunctionMetadataMap,
) -> Result<PartitionRecord, HostPartitionError> {
    let partition =
        derive_host_partition_with_metadata(validated, interner, device_function_metadata)?;

    // Effect boundaries explicit (gate bullet 2): the legality gate runs over
    // the program's launch universe. A hostile fact on any device launch
    // fails closed with the named rule — the record only derives for programs
    // the legality gate admits, so no prepared region can cross an
    // `ad`/Sermo/cancellation/host-effect boundary.
    let launches: Vec<LaunchId> = partition
        .device_program
        .launches
        .iter()
        .map(|launch| launch.id)
        .collect();
    legality
        .validate(&launches)
        .map_err(HostPartitionError::Program)?;

    let program = validated.program();
    let mut membership: Vec<PartitionMember> = partition
        .host_functions
        .iter()
        .map(|function| PartitionMember {
            function: *function,
            role: PartitionRole::Host,
            entry: None,
        })
        .chain(partition.device_kernels.iter().map(|function| {
            let entry = validated
                .function_by_id(*function)
                .map(|function| unique_kernel_entry_name(program, function, interner));
            PartitionMember {
                function: *function,
                role: PartitionRole::Device,
                entry,
            }
        }))
        .collect();
    membership.sort_by_key(|member| member.function);

    let mut regions = Vec::with_capacity(partition.boundary_calls.len());
    let mut callsites = Vec::new();
    for (index, call) in partition.boundary_calls.iter().enumerate() {
        let region = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let member_entries: Vec<String> = call
            .region
            .kernels
            .iter()
            .map(|kernel| {
                validated.function_by_id(*kernel).map_or_else(
                    || format!("kernel{}", kernel.0),
                    |function| unique_kernel_entry_name(program, function, interner),
                )
            })
            .collect();
        regions.push(RegionRecord {
            region,
            member_kernels: call.region.kernels.clone(),
            member_entries: member_entries.clone(),
        });
        // The callsite mapping: every call statement of the host function
        // that targets a member kernel of this region (typed MIR fact).
        if let Some(host) = validated.function_by_id(call.host) {
            for block in &host.blocks {
                for (statement_index, statement) in block.statements.iter().enumerate() {
                    let MirStatementKind::Call { callee, .. } = &statement.kind else {
                        continue;
                    };
                    let targets_region = callee_function_ids(callee, program)
                        .iter()
                        .any(|target| call.region.kernels.contains(target));
                    if targets_region {
                        callsites.push(CallsiteRecord {
                            callsite: CallsiteId {
                                host: call.host,
                                block: block.id,
                                statement: u32::try_from(statement_index).unwrap_or(u32::MAX),
                            },
                            region,
                            member_kernels: call.region.kernels.clone(),
                            member_entries: member_entries.clone(),
                        });
                    }
                }
            }
        }
    }

    let effect_boundaries = legality
        .device_effects
        .iter()
        .map(|fact| EffectBoundaryRecord {
            launch: fact.launch.0,
            kind: fact.kind,
        })
        .collect();

    Ok(PartitionRecord {
        module: module.to_owned(),
        membership,
        regions,
        callsites,
        effect_boundaries,
    })
}

#[cfg(test)]
#[path = "host_partition_test.rs"]
mod tests;

/// The target-neutral logical entry name of a kernel function: the function's
/// source name when it carries one, else the kernel index-based fallback.
#[must_use]
pub fn kernel_entry_name(function: &MirFunction, interner: &Interner) -> String {
    function.name.map_or_else(
        || format!("kernel{}", function.id.0),
        |symbol| interner.resolve(symbol).to_owned(),
    )
}

/// The entry name of one kernel function, unique across `program`: `k` plus
/// the decimal function id. The id is compiler-owned and stable across runs,
/// so the name is deterministic, unique and never derived from the author's
/// source spelling (ca2499dc). Two specializations of one generic kernel are
/// distinct MIR functions with distinct ids, so they get distinct entries.
#[must_use]
pub fn unique_kernel_entry_name(
    _program: &MirProgram,
    function: &MirFunction,
    _interner: &Interner,
) -> String {
    format!("k{}", function.id.0)
}

/// Resolve a call statement's callee to the MIR function ids it may refer to.
///
/// A `Definition` callee carries a semantic def id; the referenced MIR
/// function(s) are found by their carried `source` fact (a decomposition may
/// have produced several). A direct `Function` or constant-function callee
/// carries the id itself. Shared by the host-partition boundary derivation
/// and the boundary-ABI validation gate.
pub fn callee_function_ids(callee: &MirCallee, program: &MirProgram) -> Vec<MirFunctionId> {
    match callee {
        MirCallee::Function(id)
        | MirCallee::Value(MirOperand::Constant(MirConstant::Function(id))) => vec![*id],
        MirCallee::Closure(closure) => vec![closure.function],
        MirCallee::Definition { source, .. } => program
            .functions
            .iter()
            .filter(|function| function.source == Some(*source))
            .map(|function| function.id)
            .collect(),
        MirCallee::Value(_) => Vec::new(),
    }
}
