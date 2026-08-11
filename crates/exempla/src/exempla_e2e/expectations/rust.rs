//! Rust lane expected-failure ledger (`KNOWN_FAILURES`). Shared oracle classifications stay in `super::oracle` (the shared authority); this module owns only rust-lane-specific tracked failures.
//!
//! Rust-lane expected-outcome surface (per-lane-e2e-validation EL-4). The
//! lane harness module consumes only this table via
//! `super::expectations::rust::…`; no other lane may absorb these rows.

// Executable-debt budget for rust e2e. KNOWN_FAILURES is the sole accounting
// mechanism: every listed path must fail, and every observed failure must be
// listed. Fixes remove rows and ratchet accepted/pass counts upward. Do not
// raise MAX_KNOWN_FAILURES to absorb drift — reclassify (oracle) or fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KnownFailureKind {
    FixtureMismatch,
    BuildFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KnownFailure {
    pub(crate) path: &'static str,
    pub(crate) kind: KnownFailureKind,
}

// Ceiling held at 13 for historical budget; live rows shrink as debt clears.
pub(crate) const MAX_KNOWN_FAILURES: usize = 13;
pub(crate) const KNOWN_FAILURES: &[KnownFailure] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_lane_table_contains_exactly_its_own_rows() {
        // The rust lane's expected-failure ledger owns exactly the rows the
        // rust e2e harness accounts for (done_when EL-4-1). Today the ledger
        // is empty by design: expected compile failures are shared oracle
        // classifications (`super::oracle` is the authority — the rust
        // harness must not grow independent copies), and no rust-lane tracked
        // failure is currently pending. A row may be added here only when it
        // is attributable to the rust lane (fixture mismatch or build
        // failure); borrowing a row from another lane's table is absorption.
        assert!(
            KNOWN_FAILURES.is_empty(),
            "rust lane table holds rows it does not own: {KNOWN_FAILURES:?}"
        );
    }

    #[test]
    fn rust_lane_table_never_exceeds_budget() {
        // The accounting ceiling ratchets downward only; raising it to absorb
        // drift is forbidden (reclassify via the oracle, or fix).
        assert!(
            KNOWN_FAILURES.len() <= MAX_KNOWN_FAILURES,
            "rust lane expected-failure ledger {} rows above ceiling {MAX_KNOWN_FAILURES}",
            KNOWN_FAILURES.len()
        );
    }
}
