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
/// Ids are handed out by independent allocators — there is no shared counter;
/// the bands below are what keeps them disjoint. The band is the wire value
/// ([`DefId`] derives `Serialize`/`Deserialize`), so provenance is a property
/// of the value: test it with the [`DefId`] predicates, never raw comparisons.
///
/// | Range | Allocator | Predicate |
/// | --- | --- | --- |
/// | `[1, USER_DEF_ID_BASE)` | builtins, assigned sequentially during builtin registration | [`DefId::is_builtin`] |
/// | `[USER_DEF_ID_BASE, HIR_SYNTHETIC_DEF_ID_BASE)` | resolver (`Scope::next_def_id` starts at `USER_DEF_ID_BASE`) | [`DefId::is_user`] |
/// | `[HIR_SYNTHETIC_DEF_ID_BASE, CLOSURE_LIFT_DEF_ID_BASE)` | HIR-lowering synthetics: lambda captures, pattern bindings (`Lowerer::next_def_id` starts at `HIR_SYNTHETIC_DEF_ID_BASE`) | [`DefId::is_hir_synthetic`] |
/// | `[CLOSURE_LIFT_DEF_ID_BASE, PACKAGE_MIR_SYNTHETIC_DEF_BASE)` | lifted AIR closures (`air_closure_lift`) | [`DefId::is_hir_synthetic`] |
/// | `[PACKAGE_MIR_SYNTHETIC_DEF_BASE, PROVIDER_DEF_ID_BASE)` | package-MIR linked synthetic defs | [`DefId::is_package_mir_synthetic`] |
/// | `[PROVIDER_DEF_ID_BASE, COMPANION_DEF_ID_FLAG)` | synthesized provider externs (package-aware MIR lowering; 4,096 ids wide) | [`DefId::is_hir_synthetic`] |
/// | `[COMPANION_DEF_ID_FLAG, u32::MAX]` | backward companion ids: low bits are the companion index | [`DefId::is_companion`] |
///
/// The predicates are value tests over the raw `u32`, so a companion-flagged
/// id also satisfies [`DefId::is_hir_synthetic`] and
/// [`DefId::is_package_mir_synthetic`]; check [`DefId::is_companion`] first
/// when the distinction matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DefId(pub u32);

/// First id allocated by HIR lowering for synthetic bindings (lambda
/// captures, pattern bindings). Ordered above the resolver's user range.
pub const HIR_SYNTHETIC_DEF_ID_BASE: u32 = 1_000_000;

/// First id allocated by AIR closure lifting.
pub const CLOSURE_LIFT_DEF_ID_BASE: u32 = 0x4000_0000;

/// First id allocated by the package-MIR linker for linked synthetic defs.
/// Must stay above every HIR-lowering band so rewritten namespace calls
/// cannot collide with import/local bindings.
pub const PACKAGE_MIR_SYNTHETIC_DEF_BASE: u32 = 2_000_000_000;

/// First id allocated by package-aware MIR lowering for synthesized provider
/// externs (one per imported file-interface verb). Sits in the window between
/// the package-MIR base and the companion flag.
pub const PROVIDER_DEF_ID_BASE: u32 = 0x7FFF_F000;

/// Bit-31 flag marking backward companion ids; the low 31 bits are the
/// companion index ([`DefId::companion_index`]).
pub const COMPANION_DEF_ID_FLAG: u32 = 0x8000_0000;

impl DefId {
    /// Sentinel for "not yet allocated"; compare against this value rather
    /// than raw `u32::MAX`.
    ///
    /// Only a companion index of `0x7FFF_FFFF`, or ~4 billion user ids,
    /// would reach it. The range predicates do not exclude it — `u32::MAX`
    /// sits above every base and carries [`COMPANION_DEF_ID_FLAG`] — so check
    /// the sentinel before classifying.
    pub const INVALID: Self = Self(u32::MAX);

    /// True for builtin ids in `[1, USER_DEF_ID_BASE)`. Zero is not a
    /// builtin: no allocator mints it.
    #[must_use]
    pub fn is_builtin(self) -> bool {
        self.0 > 0 && self.0 < crate::USER_DEF_ID_BASE
    }

    /// True for resolver-allocated user ids in
    /// `[USER_DEF_ID_BASE, HIR_SYNTHETIC_DEF_ID_BASE)`.
    #[must_use]
    pub fn is_user(self) -> bool {
        self.0 >= crate::USER_DEF_ID_BASE && self.0 < HIR_SYNTHETIC_DEF_ID_BASE
    }

    /// True for every lowering-owned synthetic id: anything at or above
    /// [`HIR_SYNTHETIC_DEF_ID_BASE`] — HIR-lowering synthetics, lifted AIR
    /// closures, provider externs, package-MIR synthetics, and
    /// companion-flagged ids. Builtins and authored user declarations are
    /// false.
    #[must_use]
    pub fn is_hir_synthetic(self) -> bool {
        self.0 >= HIR_SYNTHETIC_DEF_ID_BASE
    }

    /// True for backward companion ids carrying the
    /// [`COMPANION_DEF_ID_FLAG`] high bit.
    #[must_use]
    pub fn is_companion(self) -> bool {
        self.0 & COMPANION_DEF_ID_FLAG != 0
    }

    /// True for package-MIR linked synthetic ids at or above
    /// [`PACKAGE_MIR_SYNTHETIC_DEF_BASE`]. Ids in the provider band and
    /// companion-flagged ids (both above the base) also satisfy this range
    /// test.
    #[must_use]
    pub fn is_package_mir_synthetic(self) -> bool {
        self.0 >= PACKAGE_MIR_SYNTHETIC_DEF_BASE
    }

    /// The low 31 bits of a companion id — the index the
    /// [`COMPANION_DEF_ID_FLAG`] was OR-ed onto at mint time. Non-companion
    /// ids are returned unchanged; test [`DefId::is_companion`] first.
    #[must_use]
    pub fn companion_index(self) -> u32 {
        self.0 & !COMPANION_DEF_ID_FLAG
    }
}

// Band disjointness is a compile error away: each band starts at or above
// the previous band's base, and the companion flag sits above every band.
const _: () = assert!(crate::USER_DEF_ID_BASE < HIR_SYNTHETIC_DEF_ID_BASE);
const _: () = assert!(HIR_SYNTHETIC_DEF_ID_BASE < CLOSURE_LIFT_DEF_ID_BASE);
const _: () = assert!(CLOSURE_LIFT_DEF_ID_BASE < PACKAGE_MIR_SYNTHETIC_DEF_BASE);
const _: () = assert!(PACKAGE_MIR_SYNTHETIC_DEF_BASE < PROVIDER_DEF_ID_BASE);
const _: () = assert!(PROVIDER_DEF_ID_BASE < COMPANION_DEF_ID_FLAG);
const _: () = assert!(COMPANION_DEF_ID_FLAG.is_power_of_two());
