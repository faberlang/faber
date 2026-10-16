//! Tests for the run-tier classifier: traps, crashes and self-imposed stops.

use super::{
    RunTierHostTrap, RunTierTrapHost, SLUG_STEP_BUDGET, SLUG_WALL_CLOCK, classify_panic_payload,
    classify_run_errors,
};
use crate::{
    BufferHost, Host, RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED, RUNNER_ISSUE_WALL_CLOCK_TIMEOUT,
    RunnerError,
};
use radix_lexer::Span;
use radix_mir::{CapabilityGap, Lowerability};

#[test]
fn processus_exit_0_traps_exit() {
    let exit_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut inner = BufferHost::default();
        let mut trap = RunTierTrapHost { inner: &mut inner };
        // Process exit is live through `Host::exit` (the `processus:exi` route);
        // the tier host must trap it so it never reaches the real process.
        trap.exit(0);
    }));
    match exit_result {
        Err(payload) => {
            let trap = payload.downcast_ref::<RunTierHostTrap>();
            assert!(
                matches!(trap, Some(RunTierHostTrap::Exit(0))),
                "process exit 0 must produce Exit(0) trap, got: {trap:?}"
            );
        }
        Ok(()) => {
            panic!("process exit 0 must trap via RunTierHostTrap, not return Ok")
        }
    }
}

/// A non-trap panic payload is a runner crash, not a target verdict.
#[test]
fn classify_panic_payload_reports_a_runner_crash_as_a_runtime_panic() {
    let payload: Box<dyn std::any::Any + Send> =
        Box::new("index out of bounds: the len is 2 but the index is 7");

    let verdict = classify_panic_payload(payload.as_ref());
    let Lowerability::Rejected(gaps) = verdict else {
        panic!("a runner panic must not classify as Capable: {verdict:?}");
    };
    let gap = gaps.first().expect("a runner panic yields one gap");
    assert!(
        matches!(gap, CapabilityGap::RuntimePanic { .. }),
        "a non-trap runner panic must not be reported as a capability gap, got {gap:?}"
    );
    assert_eq!(gap.slug(), "runtime-panic");
    assert_eq!(
        gap.shape(),
        "runner panic: index out of bounds: the len is 2 but the index is 7"
    );
}

/// The intentional traps keep their existing classification.
#[test]
fn classify_panic_payload_keeps_intentional_traps_unchanged() {
    let exit_0: Box<dyn std::any::Any + Send> = Box::new(RunTierHostTrap::Exit(0));
    assert!(
        matches!(
            classify_panic_payload(exit_0.as_ref()),
            Lowerability::Capable
        ),
        "processus.exit_process(0) stays Capable"
    );

    let exit_3: Box<dyn std::any::Any + Send> = Box::new(RunTierHostTrap::Exit(3));
    let Lowerability::Rejected(gaps) = classify_panic_payload(exit_3.as_ref()) else {
        panic!("a non-zero exit is a policy rejection");
    };
    assert_eq!(gaps.first().expect("gap").shape(), "runner exit: 3");
    assert_eq!(gaps.first().expect("gap").slug(), "target-policy-rejection");

    let abort: Box<dyn std::any::Any + Send> =
        Box::new(RunTierHostTrap::Abort("kernel abort".to_owned()));
    let Lowerability::Rejected(gaps) = classify_panic_payload(abort.as_ref()) else {
        panic!("an intentional abort is a policy rejection");
    };
    assert_eq!(
        gaps.first().expect("gap").shape(),
        "runner abort: kernel abort"
    );
    assert_eq!(gaps.first().expect("gap").slug(), "target-policy-rejection");
}

#[test]
fn run_tier_stops_report_their_own_gap_shape() {
    // WHY: the run tier bounds the execution it starts, so a stop the tier
    // imposed is reported as the classifier's own limit — never as a
    // capability verdict the program earned.
    let budget = RunnerError::internal_with_issue(
        Span::default(),
        "interpreter step budget exhausted after 10000000 steps",
        RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED,
    );
    let Lowerability::Rejected(gaps) = classify_run_errors(&[budget]) else {
        panic!("a step-budget stop is a rejection, not Capable");
    };
    let CapabilityGap::UnsupportedMirShape { slug, .. } = &gaps[0] else {
        panic!("expected the shape bucket, got: {gaps:?}");
    };
    assert_eq!(*slug, SLUG_STEP_BUDGET);

    let deadline = RunnerError::internal_with_issue(
        Span::default(),
        "wall-clock timeout exceeded",
        RUNNER_ISSUE_WALL_CLOCK_TIMEOUT,
    );
    let Lowerability::Rejected(gaps) = classify_run_errors(&[deadline]) else {
        panic!("a deadline stop is a rejection, not Capable");
    };
    let CapabilityGap::UnsupportedMirShape { slug, .. } = &gaps[0] else {
        panic!("expected the shape bucket, got: {gaps:?}");
    };
    assert_eq!(*slug, SLUG_WALL_CLOCK);
}
