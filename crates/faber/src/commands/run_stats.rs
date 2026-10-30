//! Binding and sinks for `faber run` diagnostic statistics.
//!
//! The runner owns observations; this module owns the stable product schema and
//! the two CLI output contracts.  The artifact is diagnostic-only and is never
//! used as a benchmark input.
//!
//! # Device timers: which overlap
//!
//! The `device` block mixes wall windows and the GPU's own clock. Read them as
//! follows; none of the overlapping ones may be summed.
//!
//! - `execution_ns`: the whole session window (host interpretation plus device
//!   execution). Contains every other wall timer below.
//! - `runtime.blocking_wait_ns`: host wall time blocked in commit-and-wait
//!   calls, wherever they happen (readbacks, syncs, and mid-encode commits).
//! - `host_blocked_ns`: the part of the blocked time that fell inside the
//!   launch-encode window. A subset of `runtime.blocking_wait_ns`.
//! - `host_encode_ns`: launch-encode wall time with `host_blocked_ns` taken
//!   out. This is the host's own encode cost and is disjoint from
//!   `host_blocked_ns`.
//! - `gpu_busy_ns`: the GPU's own clock over completed command buffers.
//!
//! Versioning: `host_blocked_ns` was added at `format_version` 1 as an
//! optional field, the way `gpu_timer_available` was. An artifact without it
//! predates the split, and its `host_encode_ns` includes the blocked waits
//! (GPU time counted a second time), so it must not be read as a host cost.

use radix_mir_runner::{FunctionStats, IntrinsicStats, RunnerStats};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const STATS_MARKER: &str = "--- faber-run-stats ---";
pub(crate) const FORMAT_VERSION: u32 = 1;
pub(crate) const ROUTE: &str = "scena";
pub(crate) const CLOCK: &str = "monotonic-ns";
pub(crate) const DURATION_METHOD: &str = "reservoir";
pub(crate) const DURATION_SIZE: u32 = 256;
pub(crate) const SELF_TIME: &str = "child-subtracted via call stack";
pub(crate) const NOTE: &str = "diagnostic only — never a bench-gate input";

/// The binding's `format_version: 1` runner artifact.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunStatsArtifact {
    pub(crate) kind: String,
    pub(crate) format_version: u32,
    pub(crate) generated_at: String,
    pub(crate) faber: FaberMetadata,
    pub(crate) package: PackageMetadata,
    pub(crate) route: String,
    pub(crate) run: RunMetadata,
    pub(crate) meter: MeterMetadata,
    pub(crate) functions: Vec<FunctionMetadata>,
    pub(crate) intrinsics: Vec<IntrinsicMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) device: Option<DeviceMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) cold_start: Option<ColdStartMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ColdStartMetadata {
    pub(crate) image_build_ns: u64,
    pub(crate) emission_ns: u64,
    pub(crate) ptx_compile_ns: u64,
    pub(crate) module_compile_ns: u64,
    pub(crate) execution_ns: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceMetadata {
    pub(crate) admit_ns: u64,
    pub(crate) prepare_ns: u64,
    pub(crate) image_build_ns: u64,
    pub(crate) module_compile_ns: u64,
    /// The session window: host interpretation plus device execution. This is
    /// not GPU time; read `gpu_busy_ns` for that. See the module docs for
    /// which device timers overlap.
    pub(crate) execution_ns: u64,
    /// GPU nanoseconds of completed command buffers (device time only). 0
    /// when the driver cannot read a device clock.
    pub(crate) gpu_busy_ns: u64,
    pub(crate) gpu_timing: String,
    pub(crate) timed_launches: u64,
    pub(crate) untimed_launches: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) gpu_timer_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) gpu_sample_launch_capacity: Option<u64>,
    /// Host time spent encoding launches (argument binding, buffer binding,
    /// launch encode). Excludes `host_blocked_ns`. Present with
    /// `host_blocked_ns`; an artifact without that field predates the split
    /// and its `host_encode_ns` includes the blocked waits (GPU time counted
    /// a second time), so it is not a host cost.
    pub(crate) host_encode_ns: u64,
    /// Host time blocked in commit-and-wait calls the driver made while a
    /// launch was being encoded. GPU wait, not host work. A subset of
    /// `runtime.blocking_wait_ns` (which also counts readback and sync waits
    /// outside the encode window), so never add the two. Absent in artifacts
    /// written before the split; 0 on a runtime that keeps no wait clock
    /// (CPU, CUDA).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) host_blocked_ns: Option<u64>,
    /// Cumulative host bytes fresh kernel-output twins materialized during
    /// the run: a launch allocates no host array for its output (the twin is
    /// deferred zeros) and the first host read that needs real bytes
    /// materializes it once. A steady launch loop that reads one element
    /// reports the same figure regardless of its iteration count.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) host_fresh_output_bytes: u64,
    pub(crate) uploads: u64,
    pub(crate) downloads: u64,
    pub(crate) uploaded_bytes: u64,
    pub(crate) downloaded_bytes: u64,
    pub(crate) runtime: DeviceRuntimeMetadata,
    pub(crate) entries: Vec<DeviceEntryMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceRuntimeMetadata {
    pub(crate) launch_count: u64,
    pub(crate) command_submit_count: u64,
    pub(crate) blocking_wait_count: u64,
    pub(crate) blocking_wait_ns: u64,
    pub(crate) live_modules: u64,
    pub(crate) live_buffers: u64,
    pub(crate) allocated_bytes: u64,
    pub(crate) peak_allocated_bytes: u64,
    /// Device buffer allocation calls that reached the driver, counted by the
    /// runtime adapters (never from logical receipts); a pooled reuse is not
    /// one. With `buffer_release_count` this is the steady-loop allocation
    /// promise's evidence: a steady loop (same kernels, same shapes) leaves
    /// both unchanged after its first iteration. Absent in artifacts from
    /// before the counters; read as 0 there. See
    /// `docs/design/device-allocation-promise.md`.
    #[serde(default)]
    pub(crate) buffer_alloc_count: u64,
    /// Device buffer release calls that reached the driver; parking a
    /// released buffer for reuse is not one, while evicting a parked buffer
    /// under budget pressure is.
    #[serde(default)]
    pub(crate) buffer_release_count: u64,
    /// Allocations served from the runtime's buffer pool: no driver call.
    #[serde(default)]
    pub(crate) buffer_pool_hits: u64,
    /// Buffers parked in the runtime's buffer pool at the end of the run.
    #[serde(default)]
    pub(crate) buffer_pool_buffers: u64,
    /// Bytes parked in the pool at the end of the run: real device memory
    /// (it counts against the budget) backing no live buffer.
    #[serde(default)]
    pub(crate) buffer_pool_bytes: u64,
    pub(crate) host_to_device_bytes: u64,
    pub(crate) device_to_host_bytes: u64,
    /// A logical device spanning several partitions (rule 18) only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) span: Option<DeviceSpanMetadata>,
}

/// Partition traffic and launch modes of a spanned logical device.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceSpanMetadata {
    pub(crate) partitions: u64,
    pub(crate) moved_bytes: u64,
    pub(crate) transfers: u64,
    pub(crate) replica_write_bytes: u64,
    pub(crate) split_launches: u64,
    pub(crate) replicated_launches: u64,
    pub(crate) first_launches: u64,
    /// Bytes that moved device to device (peer and same-device pairs).
    #[serde(default)]
    pub(crate) peer_bytes: u64,
    /// Bytes that moved through host memory (host-staged pairs and the fault
    /// words the combine gathers). With `peer_bytes` they are `moved_bytes`.
    #[serde(default)]
    pub(crate) host_staged_bytes: u64,
    /// How each ordered pair of partitions moves bytes, chosen when the
    /// logical device opened, with what moved over it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) links: Vec<DeviceSpanLinkMetadata>,
    /// The device-to-device copies the partitions enqueued and the dependency
    /// latency measured around them; absent when there were none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) peer: Option<DevicePeerCopyMetadata>,
    /// Every cross-partition sync, totalled per kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) syncs: Vec<DeviceSpanSyncMetadata>,
    /// The planner audit: one row per entry and sync kind the dispatcher
    /// classified as avoidable, with that kind's counters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) avoidable: Vec<DeviceSpanAvoidableMetadata>,
    /// What the partitions can hold and what they held (the capacity rule):
    /// the sum and the smallest of the partition budgets, the bytes held
    /// replicated and as slabs, and each partition's peak. A peak above
    /// `min_budget` proves a slab did the work. Absent on a logical device
    /// that does not allocate per partition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) capacity: Option<DeviceSpanCapacityMetadata>,
    /// How every live logical buffer was held, by logical buffer id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) placement: Vec<DeviceSpanPlacementMetadata>,
    pub(crate) entries: Vec<DeviceSpanEntryMetadata>,
}

/// The capacity rule's numbers (`device.runtime.span.capacity`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceSpanCapacityMetadata {
    /// The sum of the partition budgets: the bound on a working set of slabs.
    pub(crate) sum_budget: u64,
    /// The smallest partition budget: the bound on any one replica.
    pub(crate) min_budget: u64,
    /// Logical bytes of the live buffers held as replicas.
    pub(crate) replicated_bytes: u64,
    /// Logical bytes of the live buffers held as slabs (halos not counted).
    pub(crate) slab_bytes: u64,
    /// Each partition's peak allocated bytes, in partition order.
    pub(crate) peak_per_partition: Vec<u64>,
}

/// How one logical buffer was held (`device.runtime.span.placement[]`):
/// `layout` is `replica` or `slab`; a slab has its cut `axis` and `align`
/// (rows per alignment unit); `shares` are the bytes each partition owns, in
/// partition order; `forced_by` names the binding that kept a slab-eligible
/// buffer a replica.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceSpanPlacementMetadata {
    pub(crate) buffer: u64,
    pub(crate) bytes: u64,
    pub(crate) layout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) axis: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) align: Option<u64>,
    pub(crate) shares: Vec<u64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) halo_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) forced_by: Option<String>,
}

/// How bytes move from partition `from` to partition `to` (`peer`,
/// `same-device` or `host-staged`), and what moved.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceSpanLinkMetadata {
    pub(crate) from: u32,
    pub(crate) to: u32,
    pub(crate) class: String,
    pub(crate) transfers: u64,
    pub(crate) bytes: u64,
}

/// Device-to-device copies and their dependency latency. A copy is ordered on
/// the devices by events, so its latency is the consumer stream's time from
/// reaching the dependency to holding the bytes (the stall on the producer
/// plus the copy), measured for `timed` of the `copies`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevicePeerCopyMetadata {
    pub(crate) copies: u64,
    pub(crate) bytes: u64,
    pub(crate) timed: u64,
    pub(crate) dependency_us: u64,
    pub(crate) max_dependency_us: u64,
}

/// The cross-partition syncs of one kind. `count` is transport calls for a
/// byte-moving kind and waits for `host-wait-all`; `peer_bytes` is the part
/// of `bytes` that moved device to device. `blocked_ns` is the host wall time
/// spent in them: for a host-staged pair that includes the copy; for a
/// device-to-device pair it is only the host's time enqueueing copies and
/// events, and the dependency's own latency is `span.peer`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceSpanSyncMetadata {
    pub(crate) kind: String,
    pub(crate) count: u64,
    pub(crate) bytes: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) peer_bytes: u64,
    pub(crate) blocked_ns: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl DeviceSpanSyncMetadata {
    pub(crate) fn new(kind: &str, count: u64, bytes: u64, blocked_ns: u64) -> Self {
        Self {
            kind: kind.to_owned(),
            count,
            bytes,
            peer_bytes: 0,
            blocked_ns,
        }
    }

    #[cfg(feature = "device-runtime")]
    pub(crate) fn with_peer_bytes(mut self, peer_bytes: u64) -> Self {
        self.peer_bytes = peer_bytes;
        self
    }
}

/// An avoidable sync: the dispatcher classified entry `entry`'s syncs of
/// `kind` as `avoidable` (`unbounded-read`, `fusable-with-previous`,
/// `first-fallback`), with `detail` (the missing
/// read set, the entry it fuses into, the fallback reason) and the runtime's
/// counters for them.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceSpanAvoidableMetadata {
    pub(crate) entry: String,
    pub(crate) kind: String,
    pub(crate) avoidable: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
    pub(crate) count: u64,
    pub(crate) bytes: u64,
    pub(crate) blocked_ns: u64,
}

/// How one entry's launches ran on a spanned logical device; `fallback`
/// names why it ran on the first partition.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceSpanEntryMetadata {
    pub(crate) entry: String,
    pub(crate) split_launches: u64,
    pub(crate) replicated_launches: u64,
    pub(crate) first_launches: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fallback: Option<String>,
    /// The entry's syncs by kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) syncs: Vec<DeviceSpanSyncMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceEntryMetadata {
    pub(crate) entry: String,
    pub(crate) launches: u64,
    pub(crate) gpu_us: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FaberMetadata {
    pub(crate) version: String,
    pub(crate) commit: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageMetadata {
    pub(crate) name: String,
    pub(crate) entry: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunMetadata {
    pub(crate) wall_ms: u64,
    pub(crate) exit: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct MeterMetadata {
    pub(crate) clock: String,
    pub(crate) durations: DurationMetadata,
    pub(crate) self_time: String,
    pub(crate) note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DurationMetadata {
    pub(crate) method: String,
    pub(crate) size: u32,
    pub(crate) exact: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FunctionMetadata {
    pub(crate) module: String,
    pub(crate) name: String,
    pub(crate) calls: u64,
    pub(crate) total_ns: u64,
    pub(crate) self_ns: u64,
    pub(crate) p50_ns: u64,
    pub(crate) p95_ns: u64,
    pub(crate) max_ns: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntrinsicMetadata {
    pub(crate) name: String,
    pub(crate) calls: u64,
    pub(crate) total_ns: u64,
    pub(crate) self_ns: u64,
    pub(crate) p95_ns: u64,
}

impl RunStatsArtifact {
    pub(crate) fn new(
        package: PackageMetadata,
        stats: RunnerStats,
        wall_ms: u64,
        exit: i32,
    ) -> Self {
        Self::with_generated_at(package, stats, wall_ms, exit, generated_at())
    }

    pub(crate) fn with_generated_at(
        package: PackageMetadata,
        stats: RunnerStats,
        wall_ms: u64,
        exit: i32,
        generated_at: String,
    ) -> Self {
        let mut functions = stats
            .functions
            .into_iter()
            .map(FunctionMetadata::from)
            .collect::<Vec<_>>();
        functions.sort_by(|left, right| {
            left.module
                .cmp(&right.module)
                .then_with(|| left.name.cmp(&right.name))
        });

        let mut intrinsics = stats
            .intrinsics
            .into_iter()
            .map(IntrinsicMetadata::from)
            .collect::<Vec<_>>();
        intrinsics.sort_by(|left, right| left.name.cmp(&right.name));

        Self {
            kind: "faber-run-stats".to_owned(),
            format_version: FORMAT_VERSION,
            generated_at,
            faber: FaberMetadata {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                commit: None,
            },
            package,
            route: ROUTE.to_owned(),
            run: RunMetadata { wall_ms, exit },
            meter: MeterMetadata {
                clock: CLOCK.to_owned(),
                durations: DurationMetadata {
                    method: DURATION_METHOD.to_owned(),
                    size: DURATION_SIZE,
                    exact: false,
                },
                self_time: SELF_TIME.to_owned(),
                note: NOTE.to_owned(),
            },
            functions,
            intrinsics,
            device: None,
            cold_start: None,
        }
    }

    pub(crate) fn with_cold_start(mut self, cold_start: ColdStartMetadata) -> Self {
        self.cold_start = Some(cold_start);
        self
    }
}

#[cfg(feature = "device-runtime")]
impl DeviceMetadata {
    pub(crate) fn from_receipt(receipt: &faber_cli::package::mir::DeviceRunReceipt) -> Self {
        let runtime = &receipt.runtime;
        Self {
            admit_ns: receipt.admit_ns,
            prepare_ns: receipt.prepare_ns,
            image_build_ns: receipt.image_build_ns,
            module_compile_ns: receipt.module_compile_ns,
            execution_ns: receipt.execution_ns,
            gpu_busy_ns: runtime.gpu_busy_ns,
            gpu_timing: receipt.gpu_timing.clone().as_str().to_owned(),
            timed_launches: runtime.timed_launch_count,
            untimed_launches: runtime.untimed_launch_count,
            gpu_timer_available: runtime.gpu_timer_available,
            gpu_sample_launch_capacity: runtime.gpu_sample_launch_capacity,
            host_encode_ns: receipt.host_encode_ns,
            host_blocked_ns: Some(receipt.host_blocked_ns),
            host_fresh_output_bytes: receipt.host_fresh_output_bytes,
            uploads: receipt.uploads,
            downloads: receipt.downloads,
            uploaded_bytes: receipt.uploaded_bytes,
            downloaded_bytes: receipt.downloaded_bytes,
            runtime: DeviceRuntimeMetadata {
                launch_count: runtime.launch_count,
                command_submit_count: runtime.command_submit_count,
                blocking_wait_count: runtime.blocking_wait_count,
                blocking_wait_ns: runtime.blocking_wait_ns,
                live_modules: runtime.live_modules,
                live_buffers: runtime.live_buffers,
                allocated_bytes: runtime.allocated_bytes,
                peak_allocated_bytes: runtime.peak_allocated_bytes,
                buffer_alloc_count: runtime.buffer_alloc_count,
                buffer_release_count: runtime.buffer_release_count,
                buffer_pool_hits: runtime.buffer_pool_hits,
                buffer_pool_buffers: runtime.buffer_pool_buffers,
                buffer_pool_bytes: runtime.buffer_pool_bytes,
                host_to_device_bytes: runtime.host_to_device_bytes,
                device_to_host_bytes: runtime.device_to_host_bytes,
                span: runtime.span.as_ref().map(|span| {
                    DeviceSpanMetadata::from_stats(span).with_avoidable(&receipt.span_avoidable)
                }),
            },
            entries: runtime
                .entry_gpu
                .iter()
                .map(|entry| DeviceEntryMetadata {
                    entry: entry.entry.clone(),
                    launches: entry.launches,
                    gpu_us: entry.gpu_us,
                })
                .collect(),
        }
    }
}

#[cfg(feature = "device-runtime")]
impl DeviceSpanMetadata {
    pub(crate) fn from_stats(span: &radix_device_core::runtime::SpanStats) -> Self {
        Self {
            partitions: span.partitions,
            moved_bytes: span.moved_bytes,
            transfers: span.transfers,
            replica_write_bytes: span.replica_write_bytes,
            split_launches: span.split_launches,
            replicated_launches: span.replicated_launches,
            first_launches: span.first_launches,
            peer_bytes: span.peer_bytes,
            host_staged_bytes: span.host_staged_bytes,
            links: span
                .links
                .iter()
                .map(|link| DeviceSpanLinkMetadata {
                    from: link.from,
                    to: link.to,
                    class: link.class.as_str().to_owned(),
                    transfers: link.transfers,
                    bytes: link.bytes,
                })
                .collect(),
            peer: (span.peer.copies != 0).then(|| DevicePeerCopyMetadata {
                copies: span.peer.copies,
                bytes: span.peer.bytes,
                timed: span.peer.timed,
                dependency_us: span.peer.dependency_us,
                max_dependency_us: span.peer.max_dependency_us,
            }),
            syncs: span
                .syncs
                .iter()
                .map(|sync| {
                    DeviceSpanSyncMetadata::new(
                        sync.kind.as_str(),
                        sync.count,
                        sync.bytes,
                        sync.blocked_ns,
                    )
                    .with_peer_bytes(sync.peer_bytes)
                })
                .collect(),
            avoidable: Vec::new(),
            capacity: span
                .capacity
                .as_ref()
                .map(|capacity| DeviceSpanCapacityMetadata {
                    sum_budget: capacity.sum_budget,
                    min_budget: capacity.min_budget,
                    replicated_bytes: capacity.replicated_bytes,
                    slab_bytes: capacity.slab_bytes,
                    peak_per_partition: capacity.peak_per_partition.clone(),
                }),
            placement: span
                .placement
                .iter()
                .map(|row| DeviceSpanPlacementMetadata {
                    buffer: row.buffer,
                    bytes: row.bytes,
                    layout: row.layout.as_str().to_owned(),
                    axis: row.axis,
                    align: row.align_rows,
                    shares: row.shares.clone(),
                    halo_bytes: row.halo_bytes,
                    forced_by: row.forced_by.clone(),
                })
                .collect(),
            entries: span
                .entries
                .iter()
                .map(|entry| DeviceSpanEntryMetadata {
                    entry: entry.entry.clone(),
                    split_launches: entry.split_launches,
                    replicated_launches: entry.replicated_launches,
                    first_launches: entry.first_launches,
                    fallback: entry.fallback.map(|reason| reason.as_str().to_owned()),
                    syncs: entry
                        .syncs
                        .iter()
                        .map(|sync| {
                            DeviceSpanSyncMetadata::new(
                                sync.kind.as_str(),
                                sync.count,
                                sync.bytes,
                                sync.blocked_ns,
                            )
                            .with_peer_bytes(sync.peer_bytes)
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Join the dispatcher's classification to the runtime's counters: one
    /// `avoidable` row per classified (entry, kind) that the run synced, with
    /// that entry's counters for the kind. The runtime counts; the dispatcher
    /// classified, so a class whose sync never ran produces no row.
    pub(crate) fn with_avoidable(
        mut self,
        classes: &[faber_cli::package::mir::SpanAvoidable],
    ) -> Self {
        for class in classes {
            let kind = class.kind;
            let counters = self
                .entries
                .iter()
                .find(|entry| entry.entry == class.entry)
                .and_then(|entry| entry.syncs.iter().find(|sync| sync.kind == kind));
            if let Some(sync) = counters {
                self.avoidable.push(DeviceSpanAvoidableMetadata {
                    entry: class.entry.clone(),
                    kind: kind.to_owned(),
                    avoidable: class.avoidable.to_owned(),
                    detail: class.detail.clone(),
                    count: sync.count,
                    bytes: sync.bytes,
                    blocked_ns: sync.blocked_ns,
                });
            }
        }
        self
    }
}

impl From<FunctionStats> for FunctionMetadata {
    fn from(stats: FunctionStats) -> Self {
        Self {
            module: stats.module,
            name: stats.name,
            calls: stats.calls,
            total_ns: stats.total_ns,
            self_ns: stats.self_ns,
            p50_ns: stats.p50_ns,
            p95_ns: stats.p95_ns,
            max_ns: stats.max_ns,
        }
    }
}

impl From<IntrinsicStats> for IntrinsicMetadata {
    fn from(stats: IntrinsicStats) -> Self {
        Self {
            name: stats.name,
            calls: stats.calls,
            total_ns: stats.total_ns,
            self_ns: stats.self_ns,
            p95_ns: stats.p95_ns,
        }
    }
}

/// Resolve the package identity used in the artifact header.
pub(crate) fn package_metadata(
    input: &Path,
) -> Result<PackageMetadata, Box<radix::module::diagnostics::Diagnostic>> {
    let spec = faber_cli::package::discover_package(input)?;
    let name = if spec.manifest_backed {
        let manifest = faber_cli::package::read_manifest(&spec.package_root.join("faber.toml"))?;
        manifest.package.name
    } else {
        spec.package_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("package")
            .to_owned()
    };
    let entry = spec
        .entry
        .strip_prefix(&spec.package_root)
        .unwrap_or(&spec.entry)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(PackageMetadata { name, entry })
}

/// Serialize exactly one compact schema object.
pub(crate) fn encode(artifact: &RunStatsArtifact) -> io::Result<String> {
    serde_json::to_string(artifact)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Exit for a statistics emission failure: [`exit_status::SOFTWARE`] when the
/// artifact could not be serialized (`encode` marks that with a
/// `serde_json::Error`), [`exit_status::CANT_CREATE`] when the file or stdout
/// could not be written.
pub(crate) fn exit_for_emit_error(what: &str, error: &io::Error) -> ! {
    eprintln!("error: could not write {what}: {error}");
    let serialization = error
        .get_ref()
        .is_some_and(|inner| inner.is::<serde_json::Error>());
    std::process::exit(if serialization {
        faber_cli::exit_status::SOFTWARE
    } else {
        faber_cli::exit_status::CANT_CREATE
    });
}

/// Emit the requested file artifact and/or framed stdout artifact.
///
/// The file is written first so a file-only failure cannot leave a misleading
/// stdout marker behind.  No function in this module is called on the
/// unstatsed route, preserving that route's stdout bytes exactly.
pub(crate) fn emit(
    artifact: &RunStatsArtifact,
    stdout: bool,
    stats_file: Option<&PathBuf>,
) -> io::Result<()> {
    emit_framed(STATS_MARKER, &encode(artifact)?, stdout, stats_file)
}

/// Shared file-then-stdout framing used by `faber run` and `faber test`.
pub(crate) fn emit_framed(
    marker: &str,
    json: &str,
    stdout: bool,
    stats_file: Option<&PathBuf>,
) -> io::Result<()> {
    if let Some(path) = stats_file {
        fs::write(path, json.as_bytes())?;
    }
    if stdout {
        let mut output = io::stdout().lock();
        write!(output, "{marker}\n{json}\n")?;
        output.flush()?;
    }
    Ok(())
}

pub(crate) fn generated_at() -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return "unknown-time".to_owned();
    };
    let seconds = i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX);
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_date(days);
    let hour = day_seconds / 3_600;
    let minute = day_seconds / 60 % 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Inverse of the proleptic Gregorian day-number calculation, with 1970-01-01
// as day zero.  Keeping this local avoids adding a timestamp dependency for a
// diagnostic metadata field.
fn civil_date(days_since_unix_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_unix_epoch + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }).div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_part = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * month_part + 2).div_euclid(5) + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}
