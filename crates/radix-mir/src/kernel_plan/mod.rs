//! Collection-op kernel plan: the shared algorithmic recipe for compute
//! backends (WGSL, Metal, NVVM).
//!
//! TARGET: the compute-host emitters (`radix-mir-wgsl`, `radix-mir-metal`,
//! `radix-mir-llvm`). The plan is the Phase 0 design-council elevation
//! decision (ELEVATE-WITH-CONDITIONS, 2026-08-02, locks L1-L16): the tiled
//! matmul and tree-reduction bodies the WGSL emitter used to own are captured
//! here as a closed recipe set that every backend lowers to target syntax.
//! The plan never names a target keyword (L4); backends own syntax, the plan
//! owns the algorithm.
//!
//! WHY: three full lanes porting the same barrier-critical, shared-memory,
//! OOB-sensitive algorithm is a correctness defect waiting to happen — the
//! reference body is dense with fix markers (PRP-004 U1/U2/U3, F-035, C5/C6,
//! W6-A2). One authored recipe, checked once, consumed by every backend.
//!
//! The plan is a sibling pass output to [`crate::abi::CollectionOpContract`]
//! (L5): the contract stops at dispatch/workgroup/output-count; the plan adds
//! the algorithm structure — shared-memory layout, barrier schedule, OOB
//! padding policy. [`CollectionKernelPlan::from_contract`] consumes the
//! resolved contract, the statement-scan collection op, and the matmul
//! operand shapes from the MIR statement scan.
//!
//! Reference algorithm: `radix-mir-wgsl/src/lib.rs:1470-1555` (tree
//! reduction) and `:1610-1700` (tiled matmul). The WGSL emitter stays the
//! parity oracle this campaign (L13); it is not refactored to consume the
//! plan.

mod build;
mod packed;
mod packed_dequant;
mod plan;
mod validate;

pub use build::*;
pub use packed::*;
pub use packed_dequant::*;
pub use plan::*;
pub use validate::*;

#[cfg(test)]
#[path = "kernel_plan_test.rs"]
mod tests;
