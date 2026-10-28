//! The records the producer + whole-grid reduction fusion passes between the
//! host scan, the Metal emitter and the dispatcher. They name functions and
//! argument positions only: no backend type appears here.
//!
//! A whole-grid `max`/`min` reduction that consumes the output of the lane
//! kernel launched just before it can run in that kernel's epilogue (the Metal
//! emitter's `emit::lane::fuse_reduce` holds the proof and the mechanism).

use crate::MirFunctionId;

/// A request to fuse the whole-grid `max`/`min` reduction `reducer` into the
/// epilogue of the lane kernel `producer` .
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReduceFusionRequest {
    /// The lane kernel the reduction's source is written by.
    pub producer: MirFunctionId,
    /// The whole-grid reduction kernel.
    pub reducer: MirFunctionId,
    /// For each reducer call argument, the producer call argument it is bound
    /// to (call arguments exclude the `Lane` builtin parameter).
    pub reducer_to_producer: Vec<usize>,
}

/// One fused producer + reduction entry the artifact carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusedReduceEntry {
    /// The lane kernel.
    pub producer: MirFunctionId,
    /// The reduction it absorbed.
    pub reducer: MirFunctionId,
    /// The producer's own (unfused) entry.
    pub producer_entry: String,
    /// The fused entry: the producer's arguments, then the scratch binding.
    pub entry: String,
    /// The binding of the fused entry's scratch buffer
    /// ([`FUSION_SCRATCH_BYTES`] bytes, zeroed, left zeroed by every launch);
    /// the next binding is a count-one `u32` holding the lane-domain element
    /// count.
    pub scratch_binding: u32,
    /// The reducer call argument holding the reduced tensor.
    pub source_argument: usize,
    /// The reducer call argument holding the tensor the result is stored into.
    pub destination_argument: usize,
    /// For each reducer call argument, the producer call argument it is bound
    /// to.
    pub reducer_to_producer: Vec<usize>,
}

/// A requested fusion that was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReduceFusionRefusal {
    /// The lane kernel.
    pub producer: MirFunctionId,
    /// The reduction.
    pub reducer: MirFunctionId,
    /// Why the pair does not fuse.
    pub reason: String,
}

/// Bytes of the scratch buffer a fused entry binds: the key word and the
/// ticket word.
pub const FUSION_SCRATCH_BYTES: u64 = 8;
