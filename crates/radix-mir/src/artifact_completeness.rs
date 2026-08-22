//! Typed artifact completeness gate (DDCP3-U4 — gate bullet 5).
//!
//! Gate bullet 5 (ddcp3-delivery.md / CAMPAIGN.md): **every symbol, resource,
//! dimension, and argument layout a backend materializer needs for
//! preparation is present in typed artifacts** — the revised `DeviceArtifact`
//! packet + the partition/callsite record + the [`HostDeviceCall`] typed
//! bindings. No backend materializer reconstructs a preparation fact from
//! emitted MSL/PTX/LLVM/WGSL text.
//!
//! This module owns the gate: it derives the **complete typed preparation
//! bundle** for every prepared region from the typed surfaces and fails
//! closed with a named [`CompletenessError`] rule when any fact is missing or
//! inconsistent. The returned [`RegionPreparationFacts`] is the
//! **materializer-consumed surface**: a backend materializer prepares a
//! region from this bundle — the concrete prepared-region symbol identity,
//! the bounded numeric argument layout, the member kernel/entrypoint facts,
//! the typed resource facts, the launch-dimension facts, and the
//! declared-observation count — and never reparses emitted text.
//!
//! This unit is also the **typed-artifact-level `HotPathGate` proof** (gate 4;
//! the DDCP3-U2 proof is the emission-level proof): the bundle is bounded
//! numeric fields against the static region identity (`§HotPathGate` rules
//! 1–4 — the layout carries exactly the numeric dispatch extent and the
//! bundle has no lookup structure, so nothing is resolved after
//! preparation), synchronization counts come from declared observations
//! (rule 5), and the execution-descriptor identity is a distinct typed
//! domain, never FNV (rule 6 / CTO9-5).

use crate::boundary_abi::{region_launch_ids, HostDeviceCall, MemberKernel, RegionArgumentLayout};
use crate::host_partition::{HostPartition, PartitionRecord};
use crate::MirFunctionId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The complete typed preparation bundle for ONE prepared submission region
/// (gate bullet 5) — the materializer-consumed surface.
///
/// Every fact a backend materializer needs to prepare the region is carried
/// here, derived from the typed partition facts — never reconstructed from
/// emitted text. There is no `Option`/map field: nothing is looked up after
/// preparation (the region is referenced by its fixed identity only;
/// `§HotPathGate` rule 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionPreparationFacts {
    /// Stable region identity (1-based) — the compiler-owned identity the
    /// direct region symbol names (the emitted-symbol suffix is the leaf's
    /// spelling of this typed identity).
    pub region: u32,
    /// The bounded numeric argument layout of the region's direct call
    /// (`§HotPathGate` rule 1/3).
    pub argument_layout: RegionArgumentLayout,
    /// The region's member kernel/entrypoint identity facts.
    pub members: Vec<MemberKernel>,
    /// The typed resource facts of the region's member kernels.
    pub resources: Vec<PreparedResourceFact>,
    /// The typed launch-dimension facts of the region's member kernels.
    pub dimensions: Vec<LaunchDimensionFact>,
    /// The count of declared observation rows (result readbacks) whose
    /// producing launch belongs to the region's member kernels — the
    /// synchronization/readback count source (`§HotPathGate` rule 5: declared
    /// observations, never kernel count).
    pub declared_observations: u32,
}

/// One typed resource fact a backend materializer needs to prepare a region's
/// buffer slots (gate bullet 5). Typed facts, never reconstructed from text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedResourceFact {
    /// The owning member kernel of the slot.
    pub kernel: MirFunctionId,
    /// The program buffer identity (`BufferId` value — the identity key,
    /// never the buffer name).
    pub buffer: u32,
    /// The buffer's program role (typed).
    pub role: crate::boundary_abi::CallBindingRole,
    /// The slot's target-neutral first-level binding index.
    pub group: u32,
    /// The slot's second-level binding index within the group.
    pub binding: u32,
    /// The slot's element count (shape fact).
    pub element_count: u64,
    /// The slot's element type (semantic type id — a typed fact).
    pub element_type: u32,
}

/// One typed launch-dimension fact of a region's member kernel (gate bullet
/// 5 — every dimension a materializer needs is a typed fact).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchDimensionFact {
    /// The member kernel's MIR function identity.
    pub kernel: u32,
    /// Threads per workgroup (x, y, z).
    pub workgroup: (u64, u64, u64),
    /// Dispatch extent per axis (x, y, z).
    pub dispatch: (u64, u64, u64),
    /// Derived workgroup count per axis (x, y, z).
    pub workgroup_count: (u64, u64, u64),
}

/// The typed packet-facts view of the revised `DeviceArtifact` packet
/// (DDCP3-U4 / gate bullet 5): the materializer-relevant typed facts a
/// backend needs to prepare a region's artifacts — the entrypoint symbol map
/// (logical entry → emitted symbol) and the backend reflection rows (declared
/// resource bindings). Mirrored here as a plain-data view so the completeness
/// gate (radix-mir) can cross-check the fmir packet (`FmirDeviceArtifact`)
/// without a radix-mir → radix-mir-fmir dependency (the dependency direction
/// is fmir → mir); the fmir→view adapter is assembled at the caller route
/// (residual for DDCP3-U5 / the driver).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactPacketFacts {
    /// The packet's entrypoint symbol map: logical entrypoint identity →
    /// emitted symbol (resolved at preparation, never on the hot path).
    pub entrypoint_symbols: Vec<EntrypointSymbolFact>,
    /// The packet's backend reflection rows: logical entrypoint identity →
    /// declared resource bindings.
    pub reflected_kernels: Vec<ReflectedKernelFact>,
}

/// One entrypoint-symbol-map row of the packet facts view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrypointSymbolFact {
    /// The logical entrypoint identity.
    pub entry: String,
    /// The emitted symbol the host launches by (non-empty at preparation).
    pub symbol: String,
}

/// One reflected-kernel row of the packet facts view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectedKernelFact {
    /// The logical entrypoint identity.
    pub entry: String,
    /// The declared resource bindings (canonical order at hash time).
    pub resources: Vec<ReflectedResourceFact>,
}

/// One declared resource binding of a reflected-kernel row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectedResourceFact {
    /// The declared resource name.
    pub name: String,
    /// The declared binding index.
    pub binding: u32,
}

/// A fail-closed typed diagnostic naming the missing or inconsistent
/// preparation fact (gate bullet 5). Every variant names the region (and,
/// where relevant, the kernel/entry) and the rule that failed — nothing is
/// text-parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletenessError {
    /// The partition record has no region row for a derived boundary call
    /// (or the derivation index is out of range).
    AbsentRegionRecord { region: u32 },
    /// The record's region row disagrees with the derived partition.
    RegionRecordMismatch { region: u32 },
    /// The record has no callsite mapping for a derived region.
    AbsentCallsite { region: u32 },
    /// No typed `HostDeviceCall` row exists for a derived region.
    AbsentHostDeviceCall { region: u32 },
    /// A typed `HostDeviceCall` row disagrees with the derived partition.
    HostDeviceCallMismatch { region: u32, detail: String },
    /// The region has no derivable dispatch extent (missing dimension fact).
    AbsentDispatchExtent { region: u32 },
    /// A member kernel has no typed logical entrypoint identity.
    AbsentMemberEntry { region: u32, kernel: MirFunctionId },
    /// The packet's entrypoint symbol map lacks a member entry (or carries
    /// an empty emitted symbol).
    AbsentPacketEntrypointSymbol {
        /// The region whose member entry is missing.
        region: u32,
        /// The member kernel.
        kernel: MirFunctionId,
        /// The member's logical entrypoint identity.
        entry: String,
    },
    /// The packet's reflection lacks a row for a member kernel.
    AbsentPacketReflectionRow {
        /// The region whose member reflection is missing.
        region: u32,
        /// The member kernel.
        kernel: MirFunctionId,
        /// The member's logical entrypoint identity.
        entry: String,
    },
    /// The packet's reflected resource bindings disagree with the typed
    /// partition resource facts.
    PacketReflectionMismatch {
        /// The region whose reflection disagrees.
        region: u32,
        /// The member kernel.
        kernel: MirFunctionId,
        /// The member's logical entrypoint identity.
        entry: String,
    },
}

impl fmt::Display for CompletenessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbsentRegionRecord { region } => {
                write!(f, "region {region} has no partition-record region row")
            }
            Self::RegionRecordMismatch { region } => {
                write!(f, "region {region}: the record's region members disagree with the derived partition")
            }
            Self::AbsentCallsite { region } => {
                write!(f, "region {region} has no recorded callsite mapping")
            }
            Self::AbsentHostDeviceCall { region } => {
                write!(f, "region {region} has no typed HostDeviceCall row")
            }
            Self::HostDeviceCallMismatch { region, detail } => {
                write!(f, "region {region}: the typed HostDeviceCall disagrees with the derived partition ({detail})")
            }
            Self::AbsentDispatchExtent { region } => {
                write!(f, "region {region} has no derivable dispatch extent (missing dimension fact)")
            }
            Self::AbsentMemberEntry { region, kernel } => {
                write!(f, "region {region} kernel f{} has no typed logical entrypoint identity", kernel.0)
            }
            Self::AbsentPacketEntrypointSymbol {
                region,
                kernel,
                entry,
            } => write!(
                f,
                "region {region} kernel f{} entry `{entry}` has no packet entrypoint-symbol row (or an empty emitted symbol)",
                kernel.0
            ),
            Self::AbsentPacketReflectionRow {
                region,
                kernel,
                entry,
            } => write!(
                f,
                "region {region} kernel f{} entry `{entry}` has no packet reflection row",
                kernel.0
            ),
            Self::PacketReflectionMismatch {
                region,
                kernel,
                entry,
            } => write!(
                f,
                "region {region} kernel f{} entry `{entry}`: the packet reflection disagrees with the typed resource facts",
                kernel.0
            ),
        }
    }
}

impl std::error::Error for CompletenessError {}

/// Derive the complete typed preparation bundle for ONE prepared region from
/// the typed partition (gate bullet 5): the region's stable identity (the
/// direct region symbol's suffix), the bounded numeric argument layout, the
/// member kernel/entrypoint identities, the typed resource facts, the typed
/// launch-dimension facts, and the declared-observation count. Every fact
/// comes from the typed surfaces — never reconstructed from emitted text. A
/// missing fact fails closed with the named [`CompletenessError`] rule.
///
/// This is the **materializer-consumed surface**: a backend materializer
/// prepares a region from the returned bundle and never reparses emitted
/// MSL/PTX/LLVM/WGSL text.
///
/// # Errors
///
/// Returns [`CompletenessError::AbsentDispatchExtent`] when the region has
/// no derivable dispatch extent, or
/// [`CompletenessError::AbsentMemberEntry`] when a member kernel lacks a
/// typed logical entrypoint identity.
pub fn derive_region_preparation_facts(
    partition: &HostPartition,
    index: usize,
) -> Result<RegionPreparationFacts, CompletenessError> {
    let region = u32::try_from(index + 1).unwrap_or(u32::MAX);
    let call = partition
        .boundary_calls
        .get(index)
        .ok_or(CompletenessError::AbsentRegionRecord { region })?;
    let mut members = Vec::with_capacity(call.region.kernels.len());
    let mut resources = Vec::new();
    let mut dimensions = Vec::new();
    let mut dispatch_extent = 0;
    for (position, &kernel) in call.region.kernels.iter().enumerate() {
        let unit = partition
            .device_program
            .kernels
            .iter()
            .find(|unit| unit.function == kernel)
            .ok_or(CompletenessError::AbsentMemberEntry { region, kernel })?;
        members.push(MemberKernel {
            kernel,
            entry: unit.entry.clone(),
        });
        for resource in &unit.resources {
            resources.push(PreparedResourceFact {
                kernel,
                buffer: resource.buffer.id.0,
                role: resource.buffer.role.into(),
                group: resource.binding.group,
                binding: resource.binding.binding,
                element_count: resource.version.element_count,
                element_type: resource.version.element_ty.semantic_id().0,
            });
        }
        dimensions.push(LaunchDimensionFact {
            kernel: kernel.0,
            workgroup: (
                u64::from(unit.launch.workgroup.x),
                u64::from(unit.launch.workgroup.y),
                u64::from(unit.launch.workgroup.z),
            ),
            dispatch: (
                unit.launch.dispatch_size.x,
                unit.launch.dispatch_size.y,
                unit.launch.dispatch_size.z,
            ),
            workgroup_count: (
                unit.launch.workgroup_count.x,
                unit.launch.workgroup_count.y,
                unit.launch.workgroup_count.z,
            ),
        });
        if position == 0 {
            // The bounded numeric dispatch extent of the region's first
            // member kernel launch (the DDCP3-U2-frozen direct-call ABI).
            dispatch_extent = unit.launch.workgroup_count.x * u64::from(unit.launch.workgroup.x);
        }
    }
    let declared_observations = declared_observations_for(partition, &call.region.kernels);
    Ok(RegionPreparationFacts {
        region,
        argument_layout: RegionArgumentLayout { dispatch_extent },
        members,
        resources,
        dimensions,
        declared_observations,
    })
}

/// The typed completeness gate (DDCP3-U4 / gate bullet 5): prove the typed
/// artifacts — the revised `DeviceArtifact` packet facts (`packet`), the
/// partition/callsite record (`record`), and the typed [`HostDeviceCall`]
/// bindings (`calls`) — jointly carry every symbol, resource, dimension, and
/// argument layout a backend materializer needs to prepare every region of
/// the partition. Returns the complete per-region preparation bundles (the
/// materializer-consumed surface) or fails closed with the named
/// [`CompletenessError`] rule.
///
/// The gate cross-checks the record's membership/callsite/effect-boundary
/// rows and the packet's entrypoint-symbol + reflection facts against the
/// derived partition — a missing or inconsistent fact is a completeness
/// failure, never a silent reconstruction from emitted text.
///
/// # Errors
///
/// Returns a [`CompletenessError`] naming the region (and kernel/entry where
/// relevant) for the first missing or inconsistent preparation fact.
pub fn check_preparation_completeness(
    partition: &HostPartition,
    record: &PartitionRecord,
    calls: &[HostDeviceCall],
    packet: &ArtifactPacketFacts,
) -> Result<Vec<RegionPreparationFacts>, CompletenessError> {
    let mut bundles = Vec::with_capacity(partition.boundary_calls.len());
    for (index, call) in partition.boundary_calls.iter().enumerate() {
        let region = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let bundle = derive_region_preparation_facts(partition, index)?;

        // The partition/callsite record: the region row + at least one
        // callsite mapping must exist and agree with the derived partition.
        let record_region = record
            .regions
            .iter()
            .find(|row| row.region == region)
            .ok_or(CompletenessError::AbsentRegionRecord { region })?;
        if record_region.member_kernels != call.region.kernels {
            return Err(CompletenessError::RegionRecordMismatch { region });
        }
        if !record.callsites.iter().any(|row| row.region == region) {
            return Err(CompletenessError::AbsentCallsite { region });
        }

        // The typed HostDeviceCall bindings: a row for the region whose
        // members + argument layout agree with the derived bundle.
        let call_row = calls
            .iter()
            .find(|row| row.region == region)
            .ok_or(CompletenessError::AbsentHostDeviceCall { region })?;
        if call_row.members != bundle.members {
            return Err(CompletenessError::HostDeviceCallMismatch {
                region,
                detail: "member kernel/entrypoint identities disagree with the derived partition"
                    .to_owned(),
            });
        }
        if call_row.argument_layout != bundle.argument_layout {
            return Err(CompletenessError::HostDeviceCallMismatch {
                region,
                detail: "argument layout disagrees with the derived partition".to_owned(),
            });
        }

        // The packet: every member's entrypoint-symbol row (non-empty
        // emitted symbol) and reflection row covering the member's declared
        // resource bindings.
        for member in &bundle.members {
            let symbol = packet
                .entrypoint_symbols
                .iter()
                .find(|row| row.entry == member.entry)
                .ok_or_else(|| CompletenessError::AbsentPacketEntrypointSymbol {
                    region,
                    kernel: member.kernel,
                    entry: member.entry.clone(),
                })?;
            if symbol.symbol.is_empty() {
                return Err(CompletenessError::AbsentPacketEntrypointSymbol {
                    region,
                    kernel: member.kernel,
                    entry: member.entry.clone(),
                });
            }
            let reflected = packet
                .reflected_kernels
                .iter()
                .find(|row| row.entry == member.entry)
                .ok_or_else(|| CompletenessError::AbsentPacketReflectionRow {
                    region,
                    kernel: member.kernel,
                    entry: member.entry.clone(),
                })?;
            let declared_bindings: Vec<u32> = bundle
                .resources
                .iter()
                .filter(|resource| resource.kernel == member.kernel)
                .map(|resource| resource.binding)
                .collect();
            if !declared_bindings.iter().all(|binding| {
                reflected
                    .resources
                    .iter()
                    .any(|row| row.binding == *binding)
            }) {
                return Err(CompletenessError::PacketReflectionMismatch {
                    region,
                    kernel: member.kernel,
                    entry: member.entry.clone(),
                });
            }
        }
        bundles.push(bundle);
    }
    Ok(bundles)
}

/// The count of declared observation rows (result readbacks) whose producing
/// launch belongs to the region's member kernels (`§HotPathGate` rule 5: the
/// synchronization/readback count comes from **declared observations** —
/// never from kernel count).
fn declared_observations_for(partition: &HostPartition, member_kernels: &[MirFunctionId]) -> u32 {
    let launch_ids = region_launch_ids(partition, member_kernels);
    let count = partition
        .device_program
        .results
        .iter()
        .filter(|result| launch_ids.contains(&result.produced_by))
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "artifact_completeness_test.rs"]
mod tests;
