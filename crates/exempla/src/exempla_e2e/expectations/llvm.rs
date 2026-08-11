//! LLVM lane expected-outcome floors and ceilings (moved verbatim under per-lane ownership).
//!
//! LLVM-lane expected-outcome surface (per-lane-e2e-validation EL-4). The
//! lane harness module consumes only this table via
//! `super::expectations::llvm::…`; no other lane may absorb these rows.

// Tier floors and ceilings are ratcheted by the MIR LLVM baseline ledger.
pub(crate) const EXPECTED_FRONTEND_ANALYZED_FLOOR: usize = 235;
pub(crate) const EXPECTED_MIR_LOWERED_FLOOR: usize = 209;
pub(crate) const EXPECTED_LLVM_EMITTED_FLOOR: usize = 204;
pub(crate) const EXPECTED_LLVM_VERIFIER_VALID_FLOOR: usize = 204;
pub(crate) const EXPECTED_LLVM_RUNNABLE_FLOOR: usize = 48;
pub(crate) const EXPECTED_LLVM_OUTPUT_CHECKED_FLOOR: usize = 8;
/// Maximum exempla that may still hit explicit unsupported diagnostics (lower is better).
/// WHY: ratcheted 5 → 6 on 2026-07-02 to admit `conversio/fallibilis.fab`, a new
/// exemplum hitting the existing `try_call` MIR-to-LLVM gap. Ratcheted 6 → 9 on
/// 2026-07-05 for indexed GPU core type examples (`f16`, matrix, atomic) that
/// intentionally document current LLVM target rejection. This is a counted debt
/// budget, not a fix; Stage 8 (failable-call-cfg) and later GPU core backend
/// stages own ratcheting it back down. See docs/factory/mir-llvm/baseline-ledger.md.
pub(crate) const EXPECTED_UNSUPPORTED_DIAGNOSTIC_CEILING: usize = 9;
