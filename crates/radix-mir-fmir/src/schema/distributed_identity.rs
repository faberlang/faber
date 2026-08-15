//! Distributed-plan hash domains (MD2-H1).
//!
//! Implements the **three-hash-domain rule** (md0-naming-contract.md §2) at
//! the radix-mir-fmir boundary:
//!
//! 1. **A10 semantic hash** — [`device_identity_hash`](crate::schema::device_identity_hash)
//!    is **unchanged**: SHA-256 over source hashes +
//!    [`canonical_program_bytes`](crate::schema::canonical_program_bytes)
//!    (over the single-device `WireDeviceProgram` only). The distributed
//!    section/plan **never** enters those bytes — a structural fact proven
//!    at the hash-function boundary here (A10-exclusion test) and at the
//!    section boundary in MD2-W1. Physical ids, ordinals, topology, and
//!    priority never enter A10.
//! 2. **`logical_distributed_plan_hash`** — a **new, additional identity,
//!    never a replacement**: `sha256:` over the radix-mir canonical
//!    logical-plan bytes (`radix_mir::distributed::encode_logical_plan` —
//!    MD2-D1's stable field-ordered substrate). The spelling mirrors
//!    [`device_identity_hash`](crate::schema::device_identity_hash) (FC17).
//! 3. **`bound_distributed_plan_hash`** — a **spelling contract frozen
//!    here**, computed in MD2-B1 (`hosts/crates/host-coordinator/src/bound_plan.rs`) over
//!    the `BoundDistributedPlan` canonical bytes (the logical hash + binding
//!    facts). Physical ids enter THIS bound-plan domain — never A10.
//!
//! Every domain shares the frozen [`SHA256_IDENTITY_PREFIX`] spelling:
//! `sha256:<64-hex>`.

use radix_mir::distributed::{encode_logical_plan, DistributedExecutionPlan};

/// The frozen `sha256:` spelling prefix shared by every same-package
/// identity domain (A10, logical plan, bound plan).
pub const SHA256_IDENTITY_PREFIX: &str = "sha256:";

/// SHA-256 digest of the canonical bytes as lowercase hex — the
/// [`SHA256_IDENTITY_PREFIX`]-suffixed part of every identity spelling.
fn sha256_hex(canonical_bytes: &[u8]) -> String {
    handroll::sha256_hex(canonical_bytes)
}

/// The portable logical-plan identity (naming contract §2; MD2-H1).
///
/// `sha256:` over the radix-mir canonical logical-plan bytes
/// ([`encode_logical_plan`]) — the spelling mirrors
/// [`device_identity_hash`](crate::schema::device_identity_hash). The digest
/// covers the logical-plan canonical bytes **only**: it is deterministic
/// (identical plans → identical bytes → identical hash), distinguishes
/// different plans, and the logical plan never enters
/// `canonical_program_bytes` (A10 stays a function of the single-device
/// program bytes alone).
#[must_use]
pub fn logical_distributed_plan_hash(plan: &DistributedExecutionPlan) -> String {
    format!(
        "{SHA256_IDENTITY_PREFIX}{}",
        sha256_hex(&encode_logical_plan(plan))
    )
}
