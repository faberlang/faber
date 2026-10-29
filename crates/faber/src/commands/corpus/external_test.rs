use super::{judge_run_output, judge_stale_build_pin, judge_unexecuted_pin};
use crate::commands::corpus::{CASE_TIMEOUT_SECS, Case, Expected, Outcome, TargetExpectation};
use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Output};
use std::time::Duration;

fn case(outcome: Outcome, phase: Option<&str>) -> Case {
    Case {
        id: "case".to_owned(),
        category: None,
        domain: None,
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

/// The `llvm` lane builds through the product command: a `product = true`
/// recipe parses, and carries the product build as its one emit-and-link step
/// (no `prepare`, no hand-written `build`).
#[test]
fn product_recipe_parses_and_names_the_product_target() {
    let recipe: crate::commands::corpus::SuiteTarget = toml::from_str(
        "emit = \"llvm-host\"\nextension = \"ll\"\nproduct = true\nrun = [\"{artifact}\"]\n",
    )
    .expect("product recipe parses");
    assert!(recipe.product);
    assert_eq!(recipe.emit.as_deref(), Some("llvm-host"));
    assert!(recipe.prepare.is_empty() && recipe.build.is_empty());

    let plain: crate::commands::corpus::SuiteTarget =
        toml::from_str("emit = \"go\"\nextension = \"go\"\nrun = [\"{artifact}\"]\n")
            .expect("plain recipe parses");
    assert!(
        !plain.product,
        "a recipe is not a product lane unless it says so"
    );
}

fn external_recipe() -> crate::commands::corpus::SuiteTarget {
    toml::from_str("emit = \"sample\"\nextension = \"src\"\n").expect("external recipe")
}

#[test]
fn command_expansion_sorts_globs_and_reads_a_stdin_fixture() {
    let dir = tempfile::tempdir().expect("temporary command directory");
    std::fs::write(dir.path().join("z.wat"), "").expect("write glob match");
    std::fs::write(dir.path().join("a.wat"), "").expect("write glob match");
    let stdin = dir.path().join("stdin");
    std::fs::write(&stdin, b"fixture input").expect("write stdin fixture");
    let paths = super::Paths {
        suite: dir.path(),
        workspace: dir.path(),
        runtime: None,
        case_dir: dir.path(),
        source: &stdin,
        artifact: &stdin,
    };
    let (argv, output) = super::run_command(
        &[
            "printf".into(),
            "%s\\n".into(),
            format!("{}/*.wat", dir.path().display()),
        ],
        &paths,
        dir.path(),
        None,
        Duration::from_secs(2),
    )
    .expect("wildcard command succeeds");
    assert_eq!(argv[2], dir.path().join("a.wat").display().to_string());
    assert_eq!(argv[3], dir.path().join("z.wat").display().to_string());
    assert_eq!(
        output.stdout,
        format!("{}\n{}\n", argv[2], argv[3]).as_bytes()
    );

    let (_, output) = super::run_command(
        &["cat".into()],
        &paths,
        dir.path(),
        Some(&stdin),
        Duration::from_secs(2),
    )
    .expect("stdin command succeeds");
    assert_eq!(output.stdout, b"fixture input");
    assert!(super::run_command(&[], &paths, dir.path(), None, Duration::from_secs(1)).is_err());
    assert!(
        super::run_command(
            &["echo".into(), format!("{}/*.missing", dir.path().display())],
            &paths,
            dir.path(),
            None,
            Duration::from_secs(1),
        )
        .is_err()
    );
}

#[test]
fn staged_files_expand_templates_and_create_runtime_links_idempotently() {
    let dir = tempfile::tempdir().expect("temporary staging directory");
    let runtime = dir.path().join("runtime");
    std::fs::create_dir(&runtime).expect("create runtime directory");
    let library = runtime.join("library");
    std::fs::write(&library, b"runtime").expect("write runtime fixture");
    let case_dir = dir.path().join("case");
    std::fs::create_dir(&case_dir).expect("create case directory");
    let source = case_dir.join("main.src");
    let artifact = case_dir.join("program");
    let paths = super::Paths {
        suite: dir.path(),
        workspace: dir.path(),
        runtime: Some(&runtime),
        case_dir: &case_dir,
        source: &source,
        artifact: &artifact,
    };
    let mut recipe = external_recipe();
    recipe.files.insert(
        "nested/config".into(),
        "{suite}|{workspace}|{runtime}|{case_dir}|{source}|{artifact}".into(),
    );
    recipe
        .links
        .insert("nested/library".into(), "{runtime}/library".into());

    for _ in 0..2 {
        super::prepare_files(&recipe, &paths, b"emitted").expect("stage files");
    }
    assert_eq!(
        std::fs::read(&source).expect("read emitted source"),
        b"emitted"
    );
    assert_eq!(
        std::fs::read_to_string(case_dir.join("nested/config")).expect("read config"),
        format!(
            "{}|{}|{}|{}|{}|{}",
            dir.path().display(),
            dir.path().display(),
            runtime.display(),
            case_dir.display(),
            source.display(),
            artifact.display()
        )
    );
    assert_eq!(
        std::fs::read_link(case_dir.join("nested/library")).expect("read link"),
        library
    );

    recipe.files.insert("../outside".into(), "refused".into());
    assert!(super::prepare_files(&recipe, &paths, b"emitted").is_err());
    assert!(!dir.path().join("outside").exists());
}

#[test]
fn stdout_line_replacements_preserve_unmapped_lines_and_invalid_bytes() {
    let mut output = exited(0, "");
    output.stdout = b"raw\nkeep\nraw".to_vec();
    let replacements = BTreeMap::from([("raw".to_owned(), "stable".to_owned())]);
    super::normalize_stdout_lines(&mut output, &replacements);
    assert_eq!(output.stdout, b"stable\nkeep\nstable");

    let invalid = vec![0xff, b'\n'];
    output.stdout = invalid.clone();
    super::normalize_stdout_lines(&mut output, &replacements);
    assert_eq!(output.stdout, invalid);
}

#[test]
fn external_case_runs_emitted_source_and_applies_stdout_normalization() {
    let dir = tempfile::tempdir().expect("temporary external target");
    // WHY: a stand-in for the `faber` binary whose `emit` prints fixed source on every
    // platform; BSD printf rejects the extra operands of `printf emit -t ...`.
    let faber = dir.path().join("fake-faber");
    std::fs::write(&faber, "#!/bin/sh\nprintf emit\n").expect("write stand-in faber");
    std::fs::set_permissions(&faber, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("mark stand-in faber executable");
    let mut test_case = case(Outcome::Pass, None);
    test_case.dir = dir.path().to_path_buf();
    test_case.input = dir.path().join("input.fab");
    test_case.expect = Some(Expected {
        exit: 0,
        exit_nonzero: false,
        stdout: None,
        stderr: None,
        stdout_contains: Some("stable".to_owned()),
        stderr_contains: None,
        check_only: false,
    });
    let mut recipe = external_recipe();
    recipe.build = vec!["true".to_owned()];
    recipe.run = vec!["printf".to_owned(), "raw\\n".to_owned()];
    recipe
        .stdout_lines
        .insert("raw".to_owned(), "stable".to_owned());
    let suite = dir.path();
    let workspace = suite.join("target/corpus/sample");
    let mut build_ms = 0.0;
    let row = super::run_case(
        &test_case,
        0,
        "custom",
        "external",
        &recipe,
        &faber,
        suite,
        &workspace,
        std::time::Instant::now(),
        &mut build_ms,
    );
    assert_eq!(row["status"], "pass");
    assert_eq!(
        std::fs::read(workspace.join("case_0000/main.src")).expect("read staged source"),
        b"emit"
    );
}
