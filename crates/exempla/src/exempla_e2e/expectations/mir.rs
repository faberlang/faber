//! MIR (stepper, no-backend) lane expected-outcome floors (moved verbatim under per-lane ownership).
//!
//! Mir-lane expected-outcome surface (per-lane-e2e-validation EL-4). The
//! lane harness module consumes only this table via
//! `super::expectations::mir::…`; no other lane may absorb these rows.

pub(crate) const EXPECTED_FRONTEND_ANALYZED_FLOOR: usize = 283;
pub(crate) const EXPECTED_MIR_LOWERED_FLOOR: usize = 262;
