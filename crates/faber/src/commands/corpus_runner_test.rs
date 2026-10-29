//! Execution-oracle selection and bounded corpus subprocess judging.

use super::{
    Case, ChildError, Expected, Outcome, TargetExpectation, collect_drains, run_bounded, run_case,
    runner_lower_args, runner_run_args,
};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn case(args: &[&str]) -> Case {
    Case {
        id: "c".to_owned(),
        category: None,
        domain: None,
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
fn runner_target_pins_the_reference_route() {
    assert_eq!(
        runner_run_args(&case(&[])),
        [
            "run",
            "/d/main.fab",
            "--target",
            "runner",
            "--device",
            "reference"
        ]
    );
    assert_eq!(
        runner_run_args(&case(&["a", "b"])),
        [
            "run",
            "/d/main.fab",
            "--target",
            "runner",
            "--device",
            "reference",
            "--",
            "a",
            "b"
        ]
    );
}

#[cfg(unix)]
#[test]
fn run_bounded_times_out_when_a_descendant_holds_the_pipe_after_the_parent_exits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pidfile = dir.path().join("pid");
    let mut command = Command::new("sh");
    command
        .args(["-c", "sleep 30 & echo $! > \"$1\"; exit 0", "sh"])
        .arg(&pidfile);
    let started = Instant::now();
    let result = run_bounded(&mut command, Stdio::null(), Duration::from_secs(1));
    let elapsed = started.elapsed();
    let Err(ChildError::Timeout(limit)) = result else {
        panic!("a descendant holding the pipe must hit the deadline, not return output");
    };
    assert_eq!(limit, Duration::from_secs(1));
    assert!(
        elapsed < Duration::from_secs(10),
        "run_bounded took {elapsed:?}"
    );
    let pid = fs::read_to_string(&pidfile)
        .expect("the case recorded its descendant pid")
        .trim()
        .to_owned();
    let alive = || {
        Command::new("sh")
            .args(["-c", "kill -0 \"$1\" 2>/dev/null", "sh", &pid])
            .output()
            .expect("pid probe")
            .status
            .success()
    };
    for _ in 0..40 {
        if !alive() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("descendant {pid} outlived the process-group kill");
}

#[cfg(unix)]
#[test]
fn run_bounded_returns_output_when_child_and_pipes_close_promptly() {
    let mut command = Command::new("sh");
    command.args(["-c", "echo out; echo err 1>&2"]);
    let Ok(output) = run_bounded(&mut command, Stdio::null(), Duration::from_secs(10)) else {
        panic!("a child whose pipes close promptly must return output");
    };
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("out"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("err"));
}

/// A stand-in `faber` for the runner target: `run` refuses the program for the given
/// reason, and `check --lower` exits with the given code. Every invocation is logged
/// to `calls` so a test can assert which commands the target ran.
#[cfg(unix)]
fn fake_faber(
    dir: &std::path::Path,
    run_stderr: &str,
    lower_exit: i32,
    lower_stderr: &str,
) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("faber");
    let script = format!(
        "#!/bin/sh\necho \"$1 $2\" >> \"{calls}\"\n\
         case \"$1\" in\n\
         run) echo '{run_stderr}' >&2; exit 1;;\n\
         check) echo '{lower_stderr}' >&2; exit {lower_exit};;\n\
         esac\n",
        calls = dir.join("calls").display(),
    );
    fs::write(&path, script).expect("write fake faber");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake faber");
    path
}

#[cfg(unix)]
fn library_case(check_only: bool, outcome: Outcome, fragment: Option<&str>) -> Case {
    let mut library = case(&[]);
    library.expect = Some(Expected {
        exit: 0,
        exit_nonzero: false,
        stdout: None,
        stderr: None,
        stdout_contains: None,
        stderr_contains: None,
        check_only,
    });
    library.target.outcome = outcome;
    library.target.diagnostic_contains = fragment.map(str::to_owned);
    library.dir = std::env::temp_dir();
    // The synthetic deadline follows the real corpus floor: Case::timeout takes the
    // maximum of this value and CASE_TIMEOUT_SECS = 120. The 1s bound, then the 30s
    // bound, were each breached under the loaded parallel full-suite gate (a31d7dfef0
    // and this recurrence), so library cases use the production floor.
    library.timeout = Duration::from_secs(crate::commands::corpus::CASE_TIMEOUT_SECS);
    library
}

#[cfg(unix)]
fn status(row: &serde_json::Value) -> &str {
    row["status"].as_str().expect("case row has a status")
}

#[cfg(unix)]
fn lower_calls(dir: &std::path::Path) -> usize {
    fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|line| *line == "check --lower")
        .count()
}

const NO_ENTRY: &str = radix_mir::names::NO_ENTRY_FUNCTION;

#[test]
fn collect_drains_returns_buffered_results_even_when_the_deadline_has_passed() {
    // A parent scheduled late (a saturated machine) must still collect drain results
    // that were already sent; the deadline bounds the wait, not the delivery.
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send((0, b"out".to_vec())).expect("send stdout");
    sender.send((1, b"err".to_vec())).expect("send stderr");
    drop(sender);
    let mut parts = [None, None];
    let passed = collect_drains(
        &receiver,
        &mut parts,
        Instant::now() - Duration::from_secs(1),
    );
    assert!(passed);
    assert_eq!(parts[0].as_deref(), Some(&b"out"[..]));
    assert_eq!(parts[1].as_deref(), Some(&b"err"[..]));
}

#[test]
fn runner_lower_standard_checks_then_lowers() {
    assert_eq!(
        runner_lower_args(&case(&[])),
        ["check", "--lower", "/d/main.fab"]
    );
}

#[cfg(unix)]
#[test]
fn a_library_case_that_lowers_passes_without_executing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let faber = fake_faber(dir.path(), NO_ENTRY, 0, "ok");
    let row = run_case(&library_case(true, Outcome::Pass, None), &faber);
    assert_eq!(status(&row), "pass");
    assert_eq!(row["phase"], "lower");
    assert_eq!(lower_calls(dir.path()), 1);
}

#[cfg(unix)]
#[test]
fn a_library_case_that_checks_but_does_not_lower_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let faber = fake_faber(dir.path(), NO_ENTRY, 1, "checks but does not lower");
    let row = run_case(&library_case(true, Outcome::Pass, None), &faber);
    assert_eq!(status(&row), "fail");
}

#[cfg(unix)]
#[test]
fn a_pinned_lowering_defect_is_expected_and_a_lowering_fix_is_a_stale_pin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let broken = fake_faber(dir.path(), NO_ENTRY, 1, "a lowering defect");
    let pinned = library_case(true, Outcome::KnownFail, Some("a lowering defect"));
    assert_eq!(status(&run_case(&pinned, &broken)), "expected-fail");

    let fixed_dir = tempfile::tempdir().expect("tempdir");
    let fixed = fake_faber(fixed_dir.path(), NO_ENTRY, 0, "ok");
    assert_eq!(status(&run_case(&pinned, &fixed)), "fail");
}

#[cfg(unix)]
#[test]
fn a_stale_no_entry_pin_fails_once_the_library_lowers() {
    // The retired pin shape: known-fail on the run refusal. The lower standard
    // judges the case now, so the pin no longer holds and must be removed.
    let dir = tempfile::tempdir().expect("tempdir");
    let faber = fake_faber(dir.path(), NO_ENTRY, 0, "ok");
    let stale = library_case(true, Outcome::KnownFail, Some(NO_ENTRY));
    assert_eq!(status(&run_case(&stale, &faber)), "fail");
}

#[cfg(unix)]
#[test]
fn only_a_check_only_case_with_no_entry_takes_the_lower_standard() {
    // Asserts output: a missing entry function is a real failure.
    let dir = tempfile::tempdir().expect("tempdir");
    let faber = fake_faber(dir.path(), NO_ENTRY, 0, "ok");
    let row = run_case(&library_case(false, Outcome::Pass, None), &faber);
    assert_eq!(status(&row), "fail");
    assert_eq!(lower_calls(dir.path()), 0);

    // Some other refusal: the case fails on its own terms, `check --lower` never runs.
    let other_dir = tempfile::tempdir().expect("tempdir");
    let other = fake_faber(other_dir.path(), "a type error", 0, "ok");
    let row = run_case(&library_case(true, Outcome::Pass, None), &other);
    assert_eq!(status(&row), "fail");
    assert_eq!(lower_calls(other_dir.path()), 0);
}

#[cfg(unix)]
#[test]
fn jit_refusals_are_compile_time_only_and_do_not_mask_traps_or_crashes() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};
    let output = |code, stderr: &str| Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    };
    assert!(super::is_jit_refusal(&output(
        65,
        "CODEGEN001 clif target declines: mutable union parameter"
    )));
    assert!(super::is_jit_refusal(&output(
        65,
        "CODEGEN001 unsupported MIR lowering: path that does not resolve to a local value"
    )));
    for stderr in [
        "error: numerus overflow",
        "SEM001 unresolved_name",
        "TARGETLANE001:lane_requires_mir_backed_target",
        "error: this faber build has no Cranelift target",
    ] {
        assert!(!super::is_jit_refusal(&output(65, stderr)), "{stderr}");
    }
    assert!(super::is_jit_refusal(&output(
        65,
        "PKG001 clif target declines: mutable union parameter"
    )));
    assert!(!super::is_jit_refusal(&output(
        70,
        "CODEGEN001 clif target declines: runtime text"
    )));
    let mut crash = output(65, "CODEGEN001 clif target declines: not a compile failure");
    crash.status = ExitStatus::from_raw(11);
    assert!(!super::is_jit_refusal(&crash));
    let mut printed = output(65, "CODEGEN001 clif target declines: program text");
    printed.stdout = b"program already ran".to_vec();
    assert!(!super::is_jit_refusal(&printed));
}

#[test]
fn jit_inherits_runner_pins_but_honours_its_own_override() {
    use std::collections::BTreeMap;
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("case.toml");
    let text = "version = 1\ninput = 'main.fab'\n[expect]\ncheck_only = true\n[targets.runner]\noutcome = 'known-fail'\nphase = 'run'\ndiagnostic_contains = 'TARGETLANE001:lane_requires_mir_backed_target'\n[targets.clif]\noutcome = 'known-fail'\nphase = 'run'\ndiagnostic_contains = 'not the JIT oracle'\n";
    let parse = |text: &str| {
        super::parse_case(
            text,
            &path,
            dir.path(),
            "clif-jit",
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .expect("case")
    };
    let inherited = parse(text);
    assert_eq!(
        inherited.target.diagnostic_contains.as_deref(),
        Some("TARGETLANE001:lane_requires_mir_backed_target")
    );
    let overridden = parse(&format!("{text}\n[targets.clif-jit]\noutcome = 'pass'\n"));
    assert!(matches!(overridden.target.outcome, Outcome::Pass));
}

#[cfg(unix)]
#[test]
fn jit_rows_separate_named_refusals_and_check_explicit_compile_pins() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("scratch");
    let faber = dir.path().join("faber");
    fs::write(
        &faber,
        "#!/bin/sh\necho 'CODEGEN001 clif target declines: mutable union parameter' >&2\nexit 65\n",
    )
    .expect("fake compiler");
    fs::set_permissions(&faber, fs::Permissions::from_mode(0o755)).expect("executable");
    let mut fixture = library_case(true, Outcome::Pass, None);
    let row = super::run_jit_case(&fixture, &faber);
    assert_eq!(row["status"], "refused");
    assert_eq!(row["target"], "clif-jit");
    assert_eq!(row["phase"], "emit");
    fixture.target.phase = Some("emit".to_owned());
    fixture.target.diagnostic_contains = Some("another shape".to_owned());
    assert_eq!(super::run_jit_case(&fixture, &faber)["status"], "fail");
}
