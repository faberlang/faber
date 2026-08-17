//! Verification-plan gate: `cargo test -p radix -- convert_test --exact`

use radix_module::semantic::tensor_goal_gate::run_convert_gate;

#[test]
fn convert_test() {
    run_convert_gate();
}
