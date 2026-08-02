//! Definition identity shared by the type system and HIR.
//!
//! `DefId` is an opaque handle allocated by name resolution / lowering. Type
//! shapes reference declarations through this id; HIR nodes use the same
//! identity for bindings and items.

use serde::{Deserialize, Serialize};

/// Opaque definition identity within one analyzed unit.
///
/// Most values come from name resolution; lowering also allocates ids for
/// synthetic bindings. One definition may own many HIR nodes.
///
/// # Domain ranges
///
/// Builtin definitions occupy the low range `[1, USER_DEF_ID_BASE)`. User
/// definitions allocated by the resolver start at `USER_DEF_ID_BASE`. Synthetic
/// ids from HIR lowering (lambda captures, pattern bindings) use the same
/// counter so they never collide with resolver ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DefId(pub u32);

impl DefId {
    /// Sentinel for "not yet allocated."
    ///
    /// Replaces raw `u32::MAX` comparisons so sentinel checks are type-safe.
    /// Real ids never reach this value because `USER_DEF_ID_BASE` starts at
    /// `0x0000_1000` and the allocator would need ~4 billion definitions to
    /// approach `u32::MAX`.
    pub const INVALID: Self = Self(u32::MAX);
}
