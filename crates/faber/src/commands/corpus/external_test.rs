use super::{judge_stale_build_pin, judge_unexecuted_pin};
use crate::commands::corpus::{Case, Outcome, TargetExpectation};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Output};
use std::time::Duration;

fn case(outcome: Outcome, phase: Option<&str>) -> Case {
    Case {
        id: "case".to_owned(),
        dir: PathBuf::new(),
        input: PathBuf::new(),
        args: Vec::new(),
        stdin: None,
        expect: None,
        target: TargetExpectation {
            outcome,
            phase: phase.map(str::to_owned),
            failure_kind: None,
            args: None,
            diagnostic_contains: Some("fragment".to_owned()),
            reason: None,
            expect: None,
            timeout_secs: None,
        },
        timeout: Duration::from_secs(1),
    }
}

fn exited(code: i32, stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn known_fail_pinned_past_the_executed_phase_stays_a_known_fail() {
    let run_pin = case(Outcome::KnownFail, Some("run"));
    let (status, reason) = judge_unexecuted_pin(&run_pin, "build").expect("run pin is unexecuted");
    assert_eq!(status, "expected-fail");
    assert!(reason.contains("run phase") && reason.contains("build"));
    assert!(judge_unexecuted_pin(&run_pin, "emit").is_some());
    assert!(judge_unexecuted_pin(&run_pin, "run").is_none());
    assert!(judge_unexecuted_pin(&case(Outcome::KnownFail, Some("build")), "emit").is_some());
    assert!(judge_unexecuted_pin(&case(Outcome::KnownFail, Some("build")), "build").is_none());
    assert!(judge_unexecuted_pin(&case(Outcome::KnownFail, Some("emit")), "emit").is_none());
    assert!(judge_unexecuted_pin(&case(Outcome::Pass, Some("run")), "emit").is_none());
    assert!(judge_unexecuted_pin(&case(Outcome::KnownFail, None), "emit").is_none());
}

#[test]
fn stale_build_pin_reports_the_run_phase_outcome() {
    let pinned = case(Outcome::KnownFail, Some("build"));
    let (status, reason) = judge_stale_build_pin(&pinned, &exited(1, "boom"));
    assert_eq!(status, "fail");
    let reason = reason.expect("reason");
    assert!(reason.contains("build pin is stale") && reason.contains("run phase failed"));
    assert!(reason.contains("boom"));
    assert!(!reason.contains("unexpected pass"));

    let (status, reason) = judge_stale_build_pin(&pinned, &exited(0, ""));
    assert_eq!(status, "fail");
    assert_eq!(
        reason.as_deref(),
        Some("unexpected pass; update target expectation")
    );
}
