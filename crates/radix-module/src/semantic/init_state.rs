//! Definite-assignment init-state lattice shared across semantic analysis.
//!
//! The definite-assignment pass merges path states at control-flow joins
//! (`si`/`discerne` arms) using [`InitState::join_paths`]. A binding reads as
//! initialized only when it is definitely assigned on every path reaching the
//! read. Downstream consumers (MIR zero-init elision) may consume the same
//! lattice, so a future full-CFG 3b upgrade refines precision without changing
//! the contract.

/// Per-`DefId` initialization state at binding granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InitState {
    /// No assignment has been observed before this read site.
    #[default]
    Unassigned,

    /// Every control-flow path reaching the read has assigned the binding.
    Assigned,

    /// Assigned on some paths only; not safe to read.
    MaybeAssigned,
}

impl InitState {
    /// Returns whether a non-`sponte` binding may be read at this program point.
    #[must_use]
    pub fn is_definitely_initialized(self) -> bool {
        matches!(self, Self::Assigned)
    }

    /// Merge two path states at a control-flow join.
    ///
    /// A binding is definitely initialized after the join iff it was definitely
    /// initialized on every incoming path. `Unassigned ∧ Unassigned` stays
    /// `Unassigned` (preserves the pre-join state of untouched bindings); any
    /// mixed pair degrades to `MaybeAssigned`.
    #[must_use]
    pub fn join_paths(self, other: Self) -> Self {
        match (self, other) {
            (Self::Assigned, Self::Assigned) => Self::Assigned,
            (Self::Unassigned, Self::Unassigned) => Self::Unassigned,
            _ => Self::MaybeAssigned,
        }
    }
}

#[cfg(test)]
#[path = "init_state_test.rs"]
mod tests;
