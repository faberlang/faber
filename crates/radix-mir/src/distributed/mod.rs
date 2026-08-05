//! Target-neutral multi-device logical execution model (MD2).
//!
//! This module is the **distributed plan seam** of the
//! gpu-inference-multi-device campaign (MD2-D1, the serial gate): the four
//! logical plan classes per the naming contract §3 (CTO `06ac0f04`) composed
//! **above** the single-device semantic leaf
//! (`crate::device_program::DeviceProgram` — never edited here).
//!
//! ```text
//! DistributedExecutionPlan { declared_placement, partitions, graph, version }
//! DeviceProgramPartition
//! DistributedExecutionGraph over ExecutionOperation::{Launch,Transfer,Collective,Barrier}
//! LogicalPlacement / PlacementConstraint / PlacementObjective
//! ```
//!
//! # Contract anchors
//!
//! - **Portable and target-neutral**: the plan never references a
//!   `PhysicalDeviceId` (radix-mir does not depend on faber-runtime, FC18).
//! - **Stable ids only**: partitions reference the semantic program by
//!   launch/buffer ids, the program's own kernel index space, and
//!   `(buffer, version, producing launch)` result triples — never by name,
//!   order, or binding position (A3).
//! - **Admissible collectives (v1)**: [`CollectiveKind::Broadcast`] only;
//!   AllReduce/AllGather/AllToAll/ReduceScatter are declared unsupported and
//!   fail before launch (T2 §3/§8).
//! - **Host-staged transfers**: every transfer is labeled
//!   [`TransportPath::HostStaged`] (T2 §7 — silent host staging forbidden)
//!   and carries byte count, direction, dtype/layout, producer/consumer
//!   generations, and a completion boundary.
//! - **`ExecutionCommitBoundary`**, never "execution generation" (naming
//!   contract §3).
//! - **Deterministic derivation**: [`derive`] is a pure function —
//!   identical input yields byte-identical output (exit-gate bullet 1).
//! - **Canonical encoding**: [`encode_logical_plan`] is the stable
//!   field-ordered substrate for `logical_distributed_plan_hash` (MD2-H1/W1).

pub mod canonical;
pub mod derive;
pub mod types;
pub mod validate;

pub use canonical::{encode_logical_plan, CANONICAL_MAGIC, CANONICAL_REVISION};
pub use derive::derive;
pub use types::*;

#[cfg(test)]
#[path = "distributed_test.rs"]
mod tests;
