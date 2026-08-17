//! Verification-plan gate: `cargo test -p radix -- types_test --exact`

use radix_module::semantic::tensor_goal_gate::run_types_gate;

#[test]
fn types_test() {
    run_types_gate();
}
