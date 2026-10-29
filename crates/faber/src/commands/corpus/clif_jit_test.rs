//! Unit proofs for the `clif-jit` corpus lane: the command shape it runs, the
//! compile-refusal detection, and the judging of a refused or running case
//! against the runner lane's own expectations.

use super::super::{Case, Expected, Judged, Outcome, TargetExpectation};
use super::{clif_jit_run_args, judge_refusal, refused_at_compile, run_case};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

fn base_case() -> Case {
    Case {
        id: "c".to_owned(),
        category: None,
        domain: None,
        dir: PathBuf::from("/d"),
        input: PathBuf::from("/d/main.fab"),
        args: Vec::new(),
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
        timeout: Duration::from_secs(30),
    }
}

fn exited(code: i32, stdout: &str, stderr: &str) -> Output {
    use std::os::unix::process::ExitStatusExt;
    Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn clif_jit_target_pins_the_engine_and_passes_case_args() {
    assert_eq!(
        clif_jit_run_args(&base_case()),
        ["run", "--target", "clif-jit", "/d/main.fab"]
    );
    let mut with_args = base_case();
    with_args.args = ["a", "b"].iter().map(|arg| (*arg).to_owned()).collect();
    assert_eq!(
        clif_jit_run_args(&with_args),
        ["run", "--target", "clif-jit", "/d/main.fab", "--", "a", "b"]
    );
}

#[test]
fn the_compile_refusal_is_detected_by_its_closing_line() {
    assert!(refused_at_compile(&exited(
        65,
        "",
        "error[TST001:clif-policy] tool main.fab\nphase: tool\ncompilation failed\n"
    )));
    // A trap prints its own error and exits with the trap status: a run, not a refusal.
    assert!(!refused_at_compile(&exited(
        70,
        "before\n",
        "error: numerus index outside the list\n"
    )));
    // A program that prints the sentinel text itself to stderr is not a refusal.
    assert!(!refused_at_compile(&exited(
        0,
        "",
        "compilation failed\noutput\n"
    )));
}

#[test]
fn a_refusal_is_expected_failure_for_a_passing_case_and_keeps_a_rejects_pin_rejected() {
    let refused = exited(
        65,
        "",
        "error[TST001:clif-policy] clif target declines: shape\ncompilation failed\n",
    );
    let (status, reason) = judge_refusal(&base_case(), Judged::Pass, &refused);
    assert_eq!(status, "expected-fail");
    let reason = reason.expect("the refusal is named");
    assert!(
        reason.contains("clif-jit refuses the program"),
        "reason names the refusal: {reason}"
    );
    assert!(
        reason.contains("clif target declines"),
        "reason carries the refusal diagnostic: {reason}"
    );

    let mut rejects = base_case();
    rejects.target.outcome = Outcome::KnownFail;
    rejects.target.rejects = true;
    rejects.target.phase = Some("run".to_owned());
    rejects.target.diagnostic_contains = Some("clif target declines".to_owned());
    let (status, reason) = judge_refusal(&rejects, Judged::KnownFail, &refused);
    assert_eq!(status, "rejected");
    assert_eq!(reason, None);

    // The pinned fragment must still name the refusal: a different refusal is a divergence.
    rejects.target.diagnostic_contains = Some("some_other_fragment".to_owned());
    let (status, _) = judge_refusal(&rejects, Judged::KnownFail, &refused);
    assert_eq!(status, "fail");
}

#[test]
fn a_refusal_where_the_run_was_pinned_fails() {
    let mut mismatch = base_case();
    mismatch.target.outcome = Outcome::KnownFail;
    mismatch.target.phase = Some("run".to_owned());
    mismatch.target.failure_kind = Some("stdout-mismatch".to_owned());
    let refused = exited(65, "", "compilation failed\n");
    let (status, reason) = judge_refusal(&mismatch, Judged::KnownFail, &refused);
    assert_eq!(status, "fail");
    assert!(
        reason
            .unwrap()
            .contains("refused where a run-phase failure was pinned")
    );
}

/// A stand-in `faber` for the clif-jit lane: `run --target clif-jit` either
/// refuses the program (diagnostics plus the closing sentinel) or runs it
/// (stdout plus exit status).
#[cfg(unix)]
fn fake_faber(dir: &Path, behavior: &str) -> PathBuf {
    let script = match behavior {
        "refuse" => {
            "#!/bin/sh\necho 'error[TST001:clif-policy] clif target declines: shape' >&2\necho 'compilation failed' >&2\nexit 65\n"
        }
        "run" => "#!/bin/sh\necho hello\nexit 0\n",
        other => panic!("unknown behavior {other}"),
    };
    let path = dir.join("faber");
    fs::write(&path, script).expect("write fake faber");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake faber");
    path
}

#[cfg(unix)]
fn running_case(dir: &Path) -> Case {
    let mut case = base_case();
    case.dir = dir.to_path_buf();
    case.input = dir.join("main.fab");
    fs::write(&case.input, "main {\n    print \"hello\"\n}\n").expect("write source");
    let golden = dir.join("stdout.expected");
    fs::write(&golden, "hello\n").expect("write golden");
    case.expect = Some(Expected {
        exit: 0,
        exit_nonzero: false,
        stdout: Some(golden),
        stderr: None,
        stdout_contains: None,
        stderr_contains: None,
        check_only: false,
    });
    case
}

#[cfg(unix)]
#[test]
fn a_running_case_is_judged_against_the_runner_expectation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let faber = fake_faber(dir.path(), "run");
    let row = run_case(&running_case(dir.path()), &faber);
    assert_eq!(row["status"], "pass");
    assert_eq!(row["target"], "clif-jit");
    assert_eq!(row["exit"], 0);
}

#[cfg(unix)]
#[test]
fn a_refused_running_case_is_expected_failure_never_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let faber = fake_faber(dir.path(), "refuse");
    let mut case = running_case(dir.path());
    case.expect.as_mut().expect("expectation").check_only = true;
    let row = run_case(&case, &faber);
    assert_eq!(row["status"], "expected-fail");
    let reason = row["reason"].as_str().expect("reason");
    assert!(reason.contains("clif-jit refuses the program"), "{reason}");
    assert_eq!(row["exit"], 65);
}

#[cfg(unix)]
#[test]
fn a_case_that_runs_but_differs_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Runs, but prints something other than the golden: the parity worklist.
    let faber = fake_faber(dir.path(), "run");
    let mut case = running_case(dir.path());
    let expect = case.expect.as_mut().expect("expectation");
    let golden = dir.path().join("stdout.expected");
    fs::write(&golden, "goodbye\n").expect("write golden");
    expect.stdout = Some(golden);
    let row = run_case(&case, &faber);
    assert_eq!(row["status"], "fail");
    assert_eq!(row["reason"], "stdout mismatch");
}

/// `parse_case` resolves a `clif-jit` pin, then the case's `runner` pin, then
/// the group's `clif-jit`/`runner` expectation — the runner lane's own.
#[test]
fn clif_jit_expectations_fall_back_to_the_runner_pins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest_path = dir.path().join("case.toml");
    let reject_pin = r#"
version = 1
input = "main.fab"

[expect]
check_only = true

[targets.runner]
outcome = "rejects"
phase = "run"
diagnostic_contains = "frag"
"#;
    let case = super::super::parse_case(
        reject_pin,
        &manifest_path,
        dir.path(),
        "clif-jit",
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .expect("clif-jit falls back to the case's runner pin");
    assert!(matches!(case.target.outcome, Outcome::KnownFail));
    assert!(case.target.rejects);
    assert_eq!(case.target.diagnostic_contains.as_deref(), Some("frag"));

    // An explicit clif-jit pin wins over the runner pin.
    let own_pin = r#"
version = 1
input = "main.fab"

[expect]
check_only = true

[targets.clif-jit]
outcome = "not-applicable"
reason = "lane excluded"
"#;
    let case = super::super::parse_case(
        own_pin,
        &manifest_path,
        dir.path(),
        "clif-jit",
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .expect("the clif-jit pin is used directly");
    assert!(matches!(case.target.outcome, Outcome::NotApplicable));

    // Group fallback: no case pin, the group's runner expectation applies.
    let unpinned =
        "version = 1\ngroup = \"language\"\ninput = \"main.fab\"\n\n[expect]\ncheck_only = true\n";
    let pass = TargetExpectation {
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
    };
    let groups = BTreeMap::from([(
        "language".to_owned(),
        BTreeMap::from([("runner".to_owned(), pass)]),
    )]);
    let case = super::super::parse_case(
        unpinned,
        &manifest_path,
        dir.path(),
        "clif-jit",
        &BTreeMap::new(),
        &groups,
    )
    .expect("clif-jit falls back to the group's runner expectation");
    assert!(matches!(case.target.outcome, Outcome::Pass));

    // A target with no pin and no group fallback is still an error.
    let error = match super::super::parse_case(
        unpinned,
        &manifest_path,
        dir.path(),
        "oracle",
        &BTreeMap::new(),
        &BTreeMap::new(),
    ) {
        Err(message) => message,
        Ok(_) => panic!("an unpinned target must be an error"),
    };
    assert!(
        error.contains("target oracle has no expectation"),
        "{error}"
    );
}
