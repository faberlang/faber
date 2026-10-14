use super::{Case, Outcome, TargetExpectation, runner_run_args};
use std::path::PathBuf;
use std::time::Duration;

fn case(args: &[&str]) -> Case {
    Case {
        id: "c".to_owned(),
        dir: PathBuf::from("/d"),
        input: PathBuf::from("/d/main.fab"),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        stdin: None,
        expect: None,
        target: TargetExpectation {
            outcome: Outcome::Pass,
            phase: None,
            failure_kind: None,
            args: None,
            diagnostic_contains: None,
            reason: None,
            expect: None,
            timeout_secs: None,
            known_fail_why: None,
            rejects: false,
        },
        timeout: Duration::from_secs(1),
    }
}

#[test]
fn runner_target_pins_the_cpu_device() {
    assert_eq!(
        runner_run_args(&case(&[])),
        ["run", "/d/main.fab", "--device", "cpu"]
    );
    assert_eq!(
        runner_run_args(&case(&["a", "b"])),
        ["run", "/d/main.fab", "--device", "cpu", "--", "a", "b"]
    );
}
