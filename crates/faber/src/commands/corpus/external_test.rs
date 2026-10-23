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
            known_fail_why: None,
            rejects: false,
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
    assert_eq!(
        short.setup_timeout(),
        Duration::from_secs(CASE_TIMEOUT_SECS)
    );

    let mut long = case(Outcome::Pass, None);
    long.timeout = Duration::from_secs(300);
    assert_eq!(long.setup_timeout(), Duration::from_secs(300));
}

#[test]
fn external_target_maps_recipe_names_and_rejects_unknown() {
    use radix::module::Target;

    for (name, expected) in [
        ("go", Target::HirGo),
        ("ts", Target::HirTypeScript),
        ("swift", Target::HirSwift),
        ("wasm", Target::MirWasm),
        ("llvm", Target::MirLlvm),
        ("clif", Target::MirClif),
        ("sexp", Target::MirSexp),
        ("c", Target::HirC),
    ] {
        assert_eq!(super::external_target(name), Some(expected), "{name}");
    }
    assert_eq!(super::external_target("oracle"), None);
}

#[test]
fn api_emit_failure_judges_a_rejects_row_on_diagnostic_identity() {
    let message = "unknown library module `norma:nonexistent`";
    let diagnostic = radix::Diagnostic::error(message)
        .with_code("PKG001")
        .with_arg("issue", "unknown_library_module");
    assert_eq!(diagnostic.code, Some("PKG001"));
    assert_eq!(diagnostic.issue(), Some("unknown_library_module"));
    let summary = super::emit_diagnostic_summary(&diagnostic);
    let lookup_key = diagnostic.lookup_key().expect("diagnostic lookup key");
    assert_eq!(lookup_key, "PKG001:unknown_library_module");
    let expected_summary = format!("{lookup_key}: {message}");
    assert_eq!(summary, expected_summary);

    // `load_case` rewrites Rejects to KnownFail and sets the flag.
    let mut rejects = case(Outcome::KnownFail, Some("emit"));
    rejects.target.rejects = true;
    rejects.target.diagnostic_contains = Some("PKG001:unknown_library_module".to_owned());
    let (status, reason) = super::judge_api_emit_failure(&rejects, &summary);
    assert_eq!(status, "expected-fail");
    assert_eq!(reason, None);
    assert_eq!(rejects.relabel_expected_fail(status), "rejected");

    let mut unpinned = case(Outcome::KnownFail, Some("emit"));
    unpinned.target.rejects = true;
    unpinned.target.diagnostic_contains = Some("some_other_issue".to_owned());
    assert_eq!(super::judge_api_emit_failure(&unpinned, &summary).0, "fail");

    let uncoded = radix::Diagnostic::error("plain message");
    assert_eq!(super::emit_diagnostic_summary(&uncoded), "plain message");
}

#[test]
fn emit_diagnostic_summary_reports_code_and_issue() {
    let message = "unknown library module `norma:nonexistent`";
    let coded = radix::Diagnostic::error(message)
        .with_code("PKG001")
        .with_arg("issue", "unknown_library_module");
    assert_eq!(coded.code, Some("PKG001"));
    assert_eq!(coded.issue(), Some("unknown_library_module"));
    let summary = super::emit_diagnostic_summary(&coded);
    let lookup_key = coded.lookup_key().expect("diagnostic lookup key");
    assert_eq!(lookup_key, "PKG001:unknown_library_module");
    let expected_summary = format!("{lookup_key}: {message}");
    assert_eq!(summary, expected_summary);

    let code_only = radix::Diagnostic::error("message").with_code("TYP001");
    assert_eq!(
        super::emit_diagnostic_summary(&code_only),
        "TYP001: message"
    );

    let bare = radix::Diagnostic::error("plain message");
    assert_eq!(super::emit_diagnostic_summary(&bare), "plain message");
}

#[test]
fn package_product_is_classified_against_the_recipe_extension() {
    use super::{PackageProduct, classify_package_product};

    let dir = tempfile::tempdir().expect("temporary package output");
    let source = dir.path().join("main.ts");
    let binary = dir.path().join("program");
    std::fs::write(&source, "").expect("write source");
    std::fs::write(&binary, "").expect("write binary");
    assert_eq!(
        classify_package_product(source.clone(), "ts"),
        PackageProduct::Source(source)
    );
    assert_eq!(
        classify_package_product(binary.clone(), "ts"),
        PackageProduct::Linked(binary)
    );

    let modules = dir.path().join("wasm");
    std::fs::create_dir(&modules).expect("module directory");
    assert_eq!(
        classify_package_product(modules.clone(), "wat"),
        PackageProduct::Missing
    );
    std::fs::write(modules.join("001-b.wat"), "").expect("write module");
    std::fs::write(modules.join("000-a.wat"), "").expect("write module");
    std::fs::write(modules.join("notes.txt"), "").expect("write other file");
    assert_eq!(
        classify_package_product(modules.clone(), "wat"),
        PackageProduct::Modules(vec![modules.join("000-a.wat"), modules.join("001-b.wat")])
    );
}
