//! Companion-relation carrier: primal → companion device materialization facts.

use crate::device_semantics::LosslessMirCompanionEntry;
use handroll::FxHashMap;
use radix_hir::DefId;

/// The owned companion-relation carrier (S3-A2 / Stage 3R F4): primal →
/// companion device materialization facts carried from the driver onto the
/// lowered MIR so the device-program constructor can lower generated
/// companions into device kernels. The map stores the **lossless** relation
/// rows ([`LosslessMirCompanionEntry`] — selected inputs, selected outputs,
/// gradient-to-primal identity); duplicate or contradictory rows fail closed
/// at insertion and never overwrite. Companion names are diagnostic identity
/// only; this typed map is the routing surface.
#[derive(Debug, Clone, Default)]
pub struct MirCompanionMap {
    by_primal: FxHashMap<DefId, LosslessMirCompanionEntry>,
}

impl MirCompanionMap {
    /// Record one primal → companion materialization fact, fail-closed.
    ///
    /// A duplicate or contradictory row for the same primal is a hard failure
    /// (never an overwrite): this method panics with a focused diagnostic.
    /// Callers that can surface an error use [`try_insert`](Self::try_insert).
    ///
    /// # Panics
    ///
    /// Panics when the map already carries a row for the entry's primal that
    /// is not identical to the new row.
    pub fn insert(&mut self, entry: impl Into<LosslessMirCompanionEntry>) {
        if let Err(error) = self.try_insert(entry) {
            panic!("fail-closed companion relation insert: {error}");
        }
    }

    /// Record one primal → companion materialization fact, fail-closed.
    ///
    /// A duplicate row (the same primal already carried) or a contradictory
    /// row (the same primal with different facts) is rejected with a focused
    /// error — the map never silently overwrites an existing relation.
    ///
    /// # Errors
    ///
    /// Returns [`MirCompanionInsertError::DuplicatePrimal`] for an identical
    /// repeat and [`MirCompanionInsertError::ContradictoryEntry`] when the new
    /// row disagrees with the carried one on any fact.
    pub fn try_insert(
        &mut self,
        entry: impl Into<LosslessMirCompanionEntry>,
    ) -> Result<(), MirCompanionInsertError> {
        let entry = entry.into();
        if let Some(existing) = self.by_primal.get(&entry.primal) {
            if existing == &entry {
                return Err(MirCompanionInsertError::DuplicatePrimal {
                    primal: entry.primal,
                });
            }
            let (field, first, second) = companion_conflict(existing, &entry);
            return Err(MirCompanionInsertError::ContradictoryEntry {
                primal: entry.primal,
                field,
                first,
                second,
            });
        }
        self.by_primal.insert(entry.primal, entry);
        Ok(())
    }

    /// The companion fact for a primal function, if carried.
    #[must_use]
    pub fn companion_for(&self, primal: DefId) -> Option<&LosslessMirCompanionEntry> {
        self.by_primal.get(&primal)
    }

    /// Iterate every carried companion fact (deterministic by primal `DefId`).
    pub fn iter(&self) -> impl Iterator<Item = &LosslessMirCompanionEntry> {
        let mut entries: Vec<&LosslessMirCompanionEntry> = self.by_primal.values().collect();
        entries.sort_by_key(|entry| entry.primal.0);
        entries.into_iter()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_primal.is_empty()
    }
}

/// The compact primal → companion fact (S3-A2), kept for callers that carry
/// only the routing facts (primal, companion, derivative kind,
/// device-residency). The carried map stores the lossless form.
#[derive(Debug, Clone)]
pub struct MirCompanionEntry {
    /// The differentiable primal function.
    pub primal: DefId,
    /// The generated companion function (its MIR function's `source`).
    pub companion: DefId,
    /// Derivative kind of the companion.
    pub derivative: MirCompanionDerivativeKind,
    /// The primal is an explicitly device-resident forward kernel (A5): the
    /// driver records this from the primal's HIR device role, and the
    /// materializer joins the companion to the same `DeviceProgram`.
    pub device_resident: bool,
}

impl From<MirCompanionEntry> for LosslessMirCompanionEntry {
    fn from(entry: MirCompanionEntry) -> Self {
        Self {
            primal: entry.primal,
            companion: entry.companion,
            derivative: entry.derivative,
            device_resident: entry.device_resident,
            selected_inputs: Vec::new(),
            selected_outputs: Vec::new(),
        }
    }
}

/// A fail-closed companion-relation insertion rejection (F4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirCompanionInsertError {
    /// The primal already has an identical carried row.
    DuplicatePrimal {
        /// The primal `DefId` of the repeated row.
        primal: DefId,
    },
    /// The primal already has a carried row that disagrees on a fact.
    ContradictoryEntry {
        /// The primal `DefId` of the contradictory row.
        primal: DefId,
        /// The disagreeing fact name.
        field: &'static str,
        /// The carried row's fact.
        first: String,
        /// The new row's fact.
        second: String,
    },
}

impl std::fmt::Display for MirCompanionInsertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicatePrimal { primal } => {
                write!(
                    f,
                    "duplicate companion relation row for primal DefId({})",
                    primal.0
                )
            }
            Self::ContradictoryEntry {
                primal,
                field,
                first,
                second,
            } => write!(
                f,
                "companion relation row for primal DefId({}) contradicts the carried row on {field}: first {first}, second {second}",
                primal.0
            ),
        }
    }
}

impl std::error::Error for MirCompanionInsertError {}

/// The first fact two relation rows for the same primal disagree on.
fn companion_conflict(
    existing: &LosslessMirCompanionEntry,
    incoming: &LosslessMirCompanionEntry,
) -> (&'static str, String, String) {
    if existing.companion != incoming.companion {
        return (
            "companion",
            format!("DefId({})", existing.companion.0),
            format!("DefId({})", incoming.companion.0),
        );
    }
    if existing.derivative != incoming.derivative {
        return (
            "derivative",
            existing.derivative.spelling().to_owned(),
            incoming.derivative.spelling().to_owned(),
        );
    }
    if existing.device_resident != incoming.device_resident {
        return (
            "device-resident",
            existing.device_resident.to_string(),
            incoming.device_resident.to_string(),
        );
    }
    if existing.selected_inputs != incoming.selected_inputs {
        return (
            "selected-inputs",
            format!("{:?}", existing.selected_inputs),
            format!("{:?}", incoming.selected_inputs),
        );
    }
    (
        "selected-outputs",
        format!("{:?}", existing.selected_outputs),
        format!("{:?}", incoming.selected_outputs),
    )
}

/// The derivative kind a companion computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirCompanionDerivativeKind {
    /// Reverse-mode vector-Jacobian product.
    ReverseModeVjp,
}

impl MirCompanionDerivativeKind {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::ReverseModeVjp => "reverse-mode-vjp",
        }
    }
}
