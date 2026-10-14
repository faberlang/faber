use super::{judge_run_output, judge_stale_build_pin, judge_unexecuted_pin};
use crate::commands::corpus::{CASE_TIMEOUT_SECS, Case, Expected, Outcome, TargetExpectation};
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

#[test]
fn run_output_compares_normalized_stdout_and_exact_stderr() {
    let dir = tempfile::tempdir().expect("temporary golden directory");
    let stdout = dir.path().join("stdout.golden");
    let stderr = dir.path().join("stderr.golden");
    std::fs::write(&stdout, b"line\n").expect("write stdout golden");
    std::fs::write(&stderr, b"warning\n").expect("write stderr golden");

    let mut golden_case = case(Outcome::Pass, None);
    golden_case.expect = Some(Expected {
        exit: 0,
        exit_nonzero: false,
        stdout: Some(stdout),
        stderr: Some(stderr),
        stdout_contains: None,
        stderr_contains: None,
        check_only: false,
    });

    let matching = Output {
        status: ExitStatus::from_raw(0),
        stdout: b"line\r\n\n".to_vec(),
        stderr: b"warning\n".to_vec(),
    };
    let (status, reason) = judge_run_output(&golden_case, &matching);
    assert_eq!(status, "pass");
    assert_eq!(reason, None);

    let mismatched = Output {
        status: ExitStatus::from_raw(0),
        stdout: b"line\r\n\n".to_vec(),
        stderr: b"different\n".to_vec(),
    };
    let (status, reason) = judge_run_output(&golden_case, &mismatched);
    assert_eq!(status, "fail");
    assert_eq!(reason.as_deref(), Some("stderr mismatch"));
}

#[test]
fn setup_timeout_is_not_consumed_by_a_one_second_run_bound() {
    let short = case(Outcome::Pass, None);
    assert_eq!(short.timeout, Duration::from_secs(1));
    assert_eq!(short.setup_timeout(), Duration::from_secs(CASE_TIMEOUT_SECS));

    let mut long = case(Outcome::Pass, None);
    long.timeout = Duration::from_secs(300);
    assert_eq!(long.setup_timeout(), Duration::from_secs(300));
}
