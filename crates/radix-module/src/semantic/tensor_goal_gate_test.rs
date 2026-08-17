//! In-crate re-exports of verification-plan gate assertions.

use super::{run_convert_gate, run_types_gate};

#[test]
fn types_test() {
    run_types_gate();
}

#[test]
fn convert_test() {
    run_convert_gate();
}
