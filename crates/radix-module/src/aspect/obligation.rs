//! Obligation types — backend-neutral identity and typed records for
//! aspect verification dispatch.

use crate::hir::HirId;

use super::{HirAspectKind, HirAspectRecord};

/// Uniquely identifies one verification obligation.
///
/// Combines the aspect kind, the originating HIR node (if any), and an
/// explicit ordinal so that multiple obligations from the same node remain
/// distinguishable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirAspectObligationId {
    pub aspect: HirAspectKind,
    pub node_id: Option<HirId>,
    pub ordinal: usize,
}

/// Backend that a verifier uses to discharge obligations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirAspectBackendKind {
    Lean,
}

/// A typed view of an emit-ready verification record.
///
/// Wraps a report record together with its assigned obligation identity so
/// that backends can consume it without re-deriving the identity from
/// report structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectObligation {
    pub id: HirAspectObligationId,
    pub record: HirAspectRecord,
}
