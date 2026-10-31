//! The corpus command judges file and package routes on the selected target.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn smoke_case() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/incipit/salve-munde.fab")
}

fn rust_runtime() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../runtime/rust")
        .canonicalize()
        .expect("packet Rust runtime")
}

fn records(output: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|line| serde_json::from_str(line).expect("porcelain JSON line"))
        .collect()
}

#[test]
fn corpus_accepts_existing_file_golden_and_rejects_mismatch() {
    let input = smoke_case();
    let pass = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("corpus")
        .arg(&input)
        .output()
        .expect("run corpus file");
    assert!(
        pass.status.success(),
        "{}",
        String::from_utf8_lossy(&pass.stdout)
    );
    assert_eq!(records(&pass.stdout)[0]["status"], "pass");

    let temp = tempfile::tempdir().expect("temporary golden directory");
    let wrong = temp.path().join("wrong.expected");
    fs::write(&wrong, "wrong output\n").expect("write wrong golden");
    let fail = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("corpus")
        .arg(&input)
        .arg("--expected")
        .arg(&wrong)
        .output()
        .expect("run corpus mismatch");
    assert!(!fail.status.success(), "a wrong golden must fail");
    assert_eq!(records(&fail.stdout)[0]["reason"], "stdout mismatch");
}

#[test]
fn corpus_accepts_package_directory_with_entry_golden() {
    let temp = tempfile::tempdir().expect("temporary package directory");
    let src = temp.path().join("src");
    fs::create_dir(&src).expect("create source directory");
    fs::copy(smoke_case(), src.join("main.fab")).expect("copy corpus source");
    fs::copy(
        smoke_case().with_extension("expected"),
        src.join("main.expected"),
    )
    .expect("copy corpus golden");
    fs::write(
        temp.path().join("faber.toml"),
        "[package]\nname = \"corpus-smoke\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n",
    )
    .expect("write package manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("corpus")
        .arg(temp.path())
        .output()
        .expect("run corpus package");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_runs_pilot_suite_across_source_package_and_rejection_cases() {
    let suite = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus-suites/pilot");
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(suite)
        .output()
        .expect("run corpus suite");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let rows = records(&output.stdout);
    assert_eq!(rows.len(), 7);
    assert_eq!(rows[6]["cases"], 6);
    assert_eq!(rows[6]["passed"], 6);
    assert_eq!(rows[6]["failed"], 0);
    let rejection = rows
        .iter()
        .find(|row| row["case"] == "diagnostics/unknown-type")
        .expect("intentional rejection case");
    assert_eq!(rejection["status"], "pass");
    assert_eq!(rejection["exit"], 65);
    assert!(
        rows.iter()
            .any(|row| row["case"] == "packages/proba-canary" && row["status"] == "pass")
    );
    assert!(
        rows.iter()
            .take(6)
            .all(|row| row["timing_ms"]["run"].is_number())
    );
}

#[test]
fn corpus_suite_rooted_input_uses_explicit_golden_and_original_working_directory() {
    let temp = tempfile::tempdir().expect("temporary rooted suite");
    let suite = temp.path().join("suite");
    let case = suite.join("cases/hello");
    let package_case = suite.join("cases/regio");
    let inputs = temp.path().join("inputs");
    fs::create_dir_all(&case).expect("create case directory");
    fs::create_dir_all(&package_case).expect("create package case directory");
    fs::create_dir_all(&inputs).expect("create input root");
    fs::copy(smoke_case(), inputs.join("main.fab")).expect("copy input");
    fs::copy(
        smoke_case().with_extension("expected"),
        case.join("stdout.expected"),
    )
    .expect("copy suite golden");
    let product_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus-product");
    fs::write(
        suite.join("corpus.toml"),
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[roots]\ninput = \"../inputs\"\nproduct = {}\n",
            toml::Value::String(product_root.canonicalize().unwrap().display().to_string())
        ),
    )
    .expect("write suite manifest");
    fs::write(
        case.join("case.toml"),
        "version = 1\nid = \"rooted/hello\"\ninput_root = \"input\"\ninput = \"main.fab\"\n\n[expect]\nstdout = \"stdout.expected\"\n\n[targets.runner]\noutcome = \"pass\"\n",
    )
    .expect("write case manifest");
    fs::write(
        package_case.join("case.toml"),
        "version = 1\nid = \"product/regio\"\ninput_root = \"product\"\ninput = \"regio\"\n\n[expect]\ncheck_only = true\n\n[targets.runner]\noutcome = \"pass\"\n",
    )
    .expect("write package case manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(&suite)
        .output()
        .expect("run rooted suite");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let rows = records(&output.stdout);
    assert_eq!(rows[0]["case"], "rooted/hello");
    assert_eq!(rows[0]["status"], "pass");
    let expected_cwd = inputs.canonicalize().unwrap();
    assert_eq!(rows[0]["command"]["cwd"].as_str(), expected_cwd.to_str());
    assert_eq!(rows[1]["case"], "product/regio");
    assert_eq!(rows[1]["status"], "pass");
    let expected_package_cwd = product_root.join("regio").canonicalize().unwrap();
    assert_eq!(
        rows[1]["command"]["cwd"].as_str(),
        expected_package_cwd.to_str()
    );
}

#[test]
fn corpus_suite_allows_check_only_cases_and_target_expectation_overrides() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/check-only");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[expectations.check.runner]\noutcome = \"pass\"\n",
    )
    .expect("write suite manifest");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    fs::write(case.join("wrong.expected"), "wrong\n").expect("write base golden");
    fs::write(
        case.join("right.expected"),
        fs::read(smoke_case().with_extension("expected")).expect("read source golden"),
    )
    .expect("write target golden");
    fs::write(
        case.join("case.toml"),
        "version = 1\ngroup = \"check\"\ninput = \"main.fab\"\n\n[expect]\nstdout = \"wrong.expected\"\n\n[targets.runner]\noutcome = \"pass\"\n\n[targets.runner.expect]\nstdout = \"right.expected\"\n",
    )
    .expect("write case manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run target override suite");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(records(&output.stdout)[0]["status"], "pass");

    fs::remove_file(case.join("right.expected")).expect("remove golden for check-only");
    fs::write(
        case.join("case.toml"),
        "version = 1\ngroup = \"check\"\ninput = \"main.fab\"\n\n[expect]\ncheck_only = true\n",
    )
    .expect("switch to check-only case");
    let check_only = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run check-only suite");
    assert!(
        check_only.status.success(),
        "{}",
        String::from_utf8_lossy(&check_only.stdout)
    );
    assert_eq!(records(&check_only.stdout)[0]["status"], "pass");
}

#[test]
fn corpus_suite_matches_intentional_nonzero_exit_and_stderr_fragment() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/runtime-trap");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n",
    )
    .expect("write suite manifest");
    let trap = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/iace/failable-call-main-panic.fab");
    fs::copy(trap, case.join("main.fab")).expect("copy runtime trap");
    fs::write(
        case.join("case.toml"),
        "version = 1\nid = \"runtime-trap\"\ninput = \"main.fab\"\n\n[expect]\ncheck_only = true\n\n[targets.runner]\noutcome = \"pass\"\n\n[targets.runner.expect]\nexit_nonzero = true\nstderr_contains = \"negative input\"\n",
    )
    .expect("write case manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run runtime-trap suite");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(records(&output.stdout)[0]["status"], "pass");
}

#[test]
fn corpus_suite_pins_expected_stdout_mismatch_as_known_failure() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/output-mismatch");
    fs::create_dir_all(&case).expect("create case directory");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    fs::write(case.join("expected.stdout"), "different output\n").expect("write expected output");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n",
    )
    .expect("write suite manifest");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[targets.runner]\noutcome = \"known-fail\"\nphase = \"run\"\nfailure_kind = \"stdout-mismatch\"\n\n[targets.runner.expect]\nstdout = \"expected.stdout\"\n",
    )
    .expect("write case manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run suite with known stdout mismatch");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(records(&output.stdout)[0]["status"], "expected-fail");

    fs::copy(
        smoke_case().with_extension("expected"),
        case.join("expected.stdout"),
    )
    .expect("pin matching output");
    let unexpected_pass = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run suite after known mismatch disappears");
    assert!(!unexpected_pass.status.success());
    assert_eq!(
        records(&unexpected_pass.stdout)[0]["reason"],
        "unexpected output match; update target expectation"
    );
}

#[cfg(feature = "hir-go")]
#[test]
fn corpus_external_target_pins_a_nonzero_exit_without_diagnostics() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/process-failure");
    fs::create_dir_all(&case).expect("create case directory");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy Faber source");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[targets.go]\nemit = \"go\"\nextension = \"go\"\nrun = [\"sh\", \"-c\", \"exit 1\"]\n",
    )
    .expect("write suite manifest");
    let case_manifest = case.join("case.toml");
    fs::write(
        &case_manifest,
        "version = 1\ninput = \"main.fab\"\n\n[expect]\ncheck_only = true\n\n[targets.go]\noutcome = \"known-fail\"\nphase = \"run\"\nfailure_kind = \"nonzero-exit\"\n\n[targets.go.expect]\nexit_nonzero = true\n",
    )
    .expect("write case manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "go"])
        .arg(temp.path())
        .output()
        .expect("run suite with pinned process failure");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(records(&output.stdout)[0]["status"], "expected-fail");

    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[targets.go]\nemit = \"go\"\nextension = \"go\"\nrun = [\"sh\", \"-c\", \"exit 0\"]\n",
    )
    .expect("change process outcome");
    let unexpected_pass = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "go"])
        .arg(temp.path())
        .output()
        .expect("run suite after nonzero outcome disappears");
    assert!(!unexpected_pass.status.success());
    assert_eq!(
        records(&unexpected_pass.stdout)[0]["reason"],
        "known nonzero exit changed: expected nonzero, got exit status: 0"
    );
}

#[test]
fn corpus_external_target_forwards_case_specific_arguments() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/arguments");
    fs::create_dir_all(&case).expect("create case directory");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy Faber source");
    fs::write(case.join("expected.stdout"), "left right\n").expect("write expected output");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[targets.probe]\nemit = \"faber\"\nextension = \"fab\"\nrun = [\"python3\", \"-c\", \"import sys; print(' '.join(sys.argv[1:]))\"]\n",
    )
    .expect("write suite manifest");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\nargs = [\"default\"]\n\n[expect]\nstdout = \"expected.stdout\"\n\n[targets.probe]\noutcome = \"pass\"\nargs = [\"left\", \"right\"]\n",
    )
    .expect("write case manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "probe"])
        .arg(temp.path())
        .output()
        .expect("run external target with target-specific arguments");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(records(&output.stdout)[0]["status"], "pass");
    let rows = records(&output.stdout);
    assert_eq!(rows[0]["command"]["argv"][3], "left");
    assert_eq!(rows[0]["command"]["argv"][4], "right");
}

#[test]
fn corpus_suite_rejects_rooted_input_path_traversal() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/escape");
    let inputs = temp.path().join("inputs");
    fs::create_dir_all(&case).expect("create case directory");
    fs::create_dir(&inputs).expect("create input root");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[roots]\ninput = \"inputs\"\n",
    )
    .expect("write suite manifest");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput_root = \"input\"\ninput = \"../outside.fab\"\n\n[targets.runner]\noutcome = \"pass\"\n",
    )
    .expect("write case manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run invalid suite");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("case path must stay relative"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_pilot_builds_once_and_checks_generated_binaries() {
    let suite = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus-suites/pilot");
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(&suite)
        .output()
        .expect("run Rust corpus suite");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = records(&output.stdout);
    assert_eq!(rows.len(), 7);
    assert_eq!(rows[6]["passed"], 6);
    assert_eq!(rows[6]["build"]["cargo_invocations"], 1);
    assert!(rows[6]["timing_ms"]["build"].is_number());
    let rejection = rows
        .iter()
        .find(|row| row["case"] == "diagnostics/unknown-type")
        .expect("intentional Rust emit rejection");
    assert_eq!(rejection["status"], "pass");
    assert_eq!(rejection["phase"], "emit");
    assert_eq!(rejection["exit"], 65);
    assert!(rows[0]["emit_command"]["argv"].is_array());
    assert_eq!(
        rows.iter()
            .filter(|row| row["type"] == "case" && row["phase"] == "run")
            .count(),
        5
    );

    let generated = suite.join("target/corpus/rust/corpus_case_0000/src/main.rs");
    let stable = PathBuf::from(
        rows[0]["command"]["argv"][0]
            .as_str()
            .expect("stable binary path"),
    );
    let source_modified = fs::metadata(&generated)
        .expect("generated Rust source")
        .modified()
        .expect("source mtime");
    let binary_modified = fs::metadata(&stable)
        .expect("stable Rust binary")
        .modified()
        .expect("binary mtime");
    let warm = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(&suite)
        .output()
        .expect("run warm Rust corpus suite");
    assert!(
        warm.status.success(),
        "{}",
        String::from_utf8_lossy(&warm.stdout)
    );
    let warm_rows = records(&warm.stdout);
    assert_eq!(warm_rows[6]["executables"]["reused"], 5);
    assert_eq!(warm_rows[6]["executables"]["installed_or_updated"], 0);
    assert_eq!(warm_rows[0]["artifact"]["cargo_fresh"], true);
    assert_eq!(
        fs::metadata(&generated)
            .expect("unchanged source")
            .modified()
            .expect("source mtime"),
        source_modified
    );
    assert_eq!(
        fs::metadata(&stable)
            .expect("unchanged binary")
            .modified()
            .expect("binary mtime"),
        binary_modified
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_never_runs_a_stale_executable_after_build_failure() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/probe");
    fs::create_dir_all(&case).expect("create case directory");
    let suite_manifest = temp.path().join("corpus.toml");
    fs::write(
        &suite_manifest,
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[targets.rust]\nruntime = {}\n",
            toml::Value::String(rust_runtime().display().to_string())
        ),
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"lista.fab\"\n\n[expect]\nstdout = \"lista.expected\"\n\n[targets.rust]\noutcome = \"pass\"\n",
    )
    .expect("write case");
    let original = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lista");
    fs::copy(original.join("lista.fab"), case.join("lista.fab")).expect("copy source");
    fs::copy(original.join("lista.expected"), case.join("lista.expected")).expect("copy golden");
    let first = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("build first binary");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let stable = PathBuf::from(
        records(&first.stdout)[0]["command"]["argv"][0]
            .as_str()
            .expect("stable binary path"),
    );
    assert!(stable.is_file());

    let broken_runtime = temp.path().join("broken-runtime");
    fs::create_dir(&broken_runtime).expect("create broken runtime");
    fs::write(broken_runtime.join("Cargo.toml"), "[package]\nname = \"faber\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\npath = \"lib.rs\"\n")
        .expect("write broken runtime manifest");
    fs::write(broken_runtime.join("lib.rs"), "").expect("write broken runtime");
    fs::write(
        &suite_manifest,
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[targets.rust]\nruntime = {}\n",
            toml::Value::String(broken_runtime.display().to_string())
        ),
    )
    .expect("switch runtime");
    let broken = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("run failing build");
    assert!(!broken.status.success());
    let rows = records(&broken.stdout);
    assert_eq!(rows[0]["phase"], "build");
    assert_eq!(rows[0]["status"], "fail");
    assert!(
        rows[0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("no current artifact"))
    );
    assert!(
        stable.is_file(),
        "the old executable remains cached but must not run"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_replaces_stable_executable_when_emitted_program_changes() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[targets.rust]\nruntime = {}\n",
            toml::Value::String(rust_runtime().display().to_string())
        ),
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[expect]\nstdout = \"stdout.expected\"\n\n[targets.rust]\noutcome = \"pass\"\n",
    )
    .expect("write case");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    fs::copy(
        smoke_case().with_extension("expected"),
        case.join("stdout.expected"),
    )
    .expect("copy golden");
    let first = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("build first binary");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    assert_eq!(records(&first.stdout)[0]["artifact"]["state"], "installed");

    let source = fs::read_to_string(case.join("main.fab")).expect("read source");
    assert!(source.contains("Salve, Munde!"));
    fs::write(
        case.join("main.fab"),
        source.replace("Salve, Munde!", "Changed output!"),
    )
    .expect("change source");
    fs::write(case.join("stdout.expected"), "Changed output!\n").expect("change golden");
    let changed = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("build changed binary");
    assert!(
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stdout)
    );
    let rows = records(&changed.stdout);
    assert_eq!(rows[0]["status"], "pass");
    assert_eq!(rows[0]["artifact"]["cargo_fresh"], false);
    assert_eq!(rows[0]["artifact"]["state"], "updated");
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_rejects_a_wrong_golden_after_building() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[targets.rust]\nruntime = {}\n",
            toml::Value::String(rust_runtime().display().to_string())
        ),
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[expect]\nstdout = \"wrong.expected\"\n\n[targets.rust]\noutcome = \"pass\"\n",
    )
    .expect("write case");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    fs::write(case.join("wrong.expected"), "wrong output\n").expect("write golden");
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("run Rust corpus suite");
    assert!(!output.status.success());
    let rows = records(&output.stdout);
    assert_eq!(rows[0]["phase"], "run");
    assert_eq!(rows[0]["reason"], "stdout mismatch");
    assert_eq!(rows[1]["build"]["cargo_invocations"], 1);
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_known_failure_becoming_green_requires_reclassification() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[targets.rust]\nruntime = {}\n",
            toml::Value::String(rust_runtime().display().to_string())
        ),
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[targets.rust]\noutcome = \"known-fail\"\nphase = \"emit\"\ndiagnostic_contains = \"SEM002:unknown_type\"\n",
    )
    .expect("write case");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/typi/inf-prefix-error.fab"),
        case.join("main.fab"),
    )
    .expect("copy rejected source");
    let red = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("run known failure");
    assert!(red.status.success());
    assert_eq!(records(&red.stdout)[0]["status"], "expected-fail");
    assert_eq!(records(&red.stdout)[1]["build"]["cargo_invocations"], 0);

    fs::copy(smoke_case(), case.join("main.fab")).expect("replace with valid source");
    let green = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("run unexpected pass");
    assert!(!green.status.success());
    assert_eq!(
        records(&green.stdout)[0]["reason"],
        "unexpected pass; update target expectation"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_reports_a_rejection_as_its_own_outcome() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[targets.rust]\nruntime = {}\n",
            toml::Value::String(rust_runtime().display().to_string())
        ),
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[targets.rust]\noutcome = \"rejects\"\nphase = \"emit\"\ndiagnostic_contains = \"SEM002:unknown_type\"\n",
    )
    .expect("write case");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/typi/inf-prefix-error.fab"),
        case.join("main.fab"),
    )
    .expect("copy rejected source");
    let rejected = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("run rejection case");
    assert!(
        rejected.status.success(),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    let rows = records(&rejected.stdout);
    assert_eq!(rows[0]["status"], "rejected");
    let summary = rows.last().expect("summary");
    assert_eq!(summary["rejected"], 1);
    assert_eq!(summary["expected_failures"], 0);
    assert_eq!(summary["failed"], 0);
    assert_eq!(summary["build"]["cargo_invocations"], 0);
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_canonical_reports_a_rejection_as_its_own_outcome() {
    let suite = probe_suite(
        &runtime_recipe("canonical"),
        "\n[targets.canonical]\noutcome = \"rejects\"\nphase = \"emit\"\ndiagnostic_contains = \"unknown_type\"\n",
    );
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/typi/inf-prefix-error.fab"),
        suite.path().join("cases/probe/main.fab"),
    )
    .expect("copy rejected source");
    let rejected = run_corpus(suite.path(), "canonical");
    assert!(
        rejected.status.success(),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    let rows = records(&rejected.stdout);
    assert_eq!(rows[0]["status"], "rejected");
    let summary = rows.last().expect("summary");
    assert_eq!(summary["rejected"], 1);
    assert_eq!(summary["expected_failures"], 0);
    assert_eq!(summary["failed"], 0);
}

#[test]
fn corpus_roundtrip_reports_a_rejection_as_its_own_outcome() {
    let suite = probe_suite(
        "\n[targets.roundtrip]\nmode = \"roundtrip\"\n",
        "\n[targets.roundtrip]\noutcome = \"rejects\"\nphase = \"emit\"\ndiagnostic_contains = \"unknown_type\"\n",
    );
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/typi/inf-prefix-error.fab"),
        suite.path().join("cases/probe/main.fab"),
    )
    .expect("copy rejected source");
    let rejected = run_corpus(suite.path(), "roundtrip");
    assert!(
        rejected.status.success(),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    let rows = records(&rejected.stdout);
    assert_eq!(rows[0]["status"], "rejected");
    let summary = rows.last().expect("summary");
    assert_eq!(summary["rejected"], 1);
    assert_eq!(summary["expected_failures"], 0);
    assert_eq!(summary["failed"], 0);
}

#[test]
fn corpus_known_failure_becoming_green_requires_reclassification() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases").join("probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n",
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[targets.runner]\noutcome = \"known-fail\"\nphase = \"run\"\ndiagnostic_contains = \"SEM002:unknown_type\"\n",
    )
    .expect("write case");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/typi/inf-prefix-error.fab"),
        case.join("main.fab"),
    )
    .expect("copy rejected source");
    let red = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run known failure");
    assert!(red.status.success());
    assert_eq!(records(&red.stdout)[0]["status"], "expected-fail");

    fs::copy(smoke_case(), case.join("main.fab")).expect("replace with valid source");
    let green = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run unexpected pass");
    assert!(!green.status.success());
    assert_eq!(
        records(&green.stdout)[0]["reason"],
        "unexpected pass; update target expectation"
    );
}

#[test]
fn corpus_runner_reports_a_rejection_as_its_own_outcome() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases").join("probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n",
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[targets.runner]\noutcome = \"rejects\"\nphase = \"run\"\ndiagnostic_contains = \"SEM002:unknown_type\"\n",
    )
    .expect("write case");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/typi/inf-prefix-error.fab"),
        case.join("main.fab"),
    )
    .expect("copy rejected source");
    let rejected = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run rejection case");
    assert!(
        rejected.status.success(),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    let rows = records(&rejected.stdout);
    assert_eq!(rows[0]["status"], "rejected");
    let summary = rows.last().expect("summary");
    assert_eq!(summary["rejected"], 1);
    assert_eq!(summary["expected_failures"], 0);
    assert_eq!(summary["failed"], 0);
    assert_eq!(summary["status"], "pass");

    fs::copy(smoke_case(), case.join("main.fab")).expect("replace with valid source");
    let green = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .output()
        .expect("run unexpected pass");
    assert!(!green.status.success());
    assert_eq!(
        records(&green.stdout)[0]["reason"],
        "unexpected pass; update target expectation"
    );
}

#[test]
fn corpus_requires_each_case_to_classify_the_selected_target() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases").join("probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n",
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[targets.go]\noutcome = \"pass\"\n",
    )
    .expect("write case");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("corpus")
        .arg(temp.path())
        .output()
        .expect("run suite");
    assert!(!output.status.success());
    assert!(
        records(&output.stdout)[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("target runner has no expectation"))
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_external_recipe_builds_and_checks_the_program_golden() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases").join("probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[targets.native]\nemit = \"rust\"\nextension = \"rs\"\nbuild = [\"rustc\", \"--edition=2024\", \"{source}\", \"-o\", \"{artifact}\"]\nrun = [\"{artifact}\"]\n",
    )
    .expect("write suite");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[expect]\nstdout = \"stdout.expected\"\n\n[targets.native]\noutcome = \"pass\"\n",
    )
    .expect("write case");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    fs::copy(
        smoke_case().with_extension("expected"),
        case.join("stdout.expected"),
    )
    .expect("copy golden");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_faber"))
            .args(["corpus", "--target", "native"])
            .arg(temp.path())
            .output()
            .expect("run native corpus recipe")
    };
    let pass = run();
    assert!(
        pass.status.success(),
        "{}",
        String::from_utf8_lossy(&pass.stdout)
    );
    let pass_rows = records(&pass.stdout);
    assert_eq!(pass_rows[0]["status"], "pass");
    assert_eq!(pass_rows[0]["phase"], "run");
    assert_eq!(pass_rows[1]["passed"], 1);

    fs::write(case.join("stdout.expected"), "wrong output\n").expect("write wrong golden");
    let mismatch = run();
    assert!(
        !mismatch.status.success(),
        "a wrong target golden must fail"
    );
    assert_eq!(records(&mismatch.stdout)[0]["reason"], "stdout mismatch");
}

/// A rooted case whose input is a package directory: `faber build` owns emission
/// and linking, so the file-shaped emit and the recipe build step must not run.
#[cfg(feature = "hir-go")]
#[test]
#[ignore = "requires a go toolchain on PATH; manual external-recipe proof"]
fn corpus_external_recipe_builds_a_package_directory_golden() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases").join("package");
    let package = temp.path().join("inputs").join("probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::create_dir_all(package.join("src")).expect("create package source directory");
    fs::write(
        package.join("faber.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\ntarget = \"go\"\nkind = \"bin\"\n",
    )
    .expect("write package manifest");
    fs::write(
        package.join("src/main.fab"),
        "main {\n    print \"package ok\"\n}\n",
    )
    .expect("write package source");
    fs::write(case.join("stdout.expected"), "package ok\n").expect("write golden");
    fs::write(
        case.join("case.toml"),
        "version = 1\nid = \"package\"\ninput_root = \"input\"\ninput = \"probe\"\n\n[expect]\nstdout = \"stdout.expected\"\n\n[targets.go]\noutcome = \"pass\"\n",
    )
    .expect("write case");
    // The build recipe names a source that cannot exist: a package case must skip it.
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[roots]\ninput = \"inputs\"\n\n[targets.go]\nemit = \"go\"\nextension = \"go\"\nbuild = [\"go\", \"build\", \"-o\", \"{artifact}\", \"absent-source.go\"]\nrun = [\"{artifact}\"]\n",
    )
    .expect("write suite");
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "go"])
        .arg(temp.path())
        .output()
        .expect("run package corpus recipe");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let rows = records(&output.stdout);
    assert_eq!(rows[0]["status"], "pass");
    assert_eq!(rows[0]["phase"], "run");
    assert_eq!(rows[0]["emit_command"]["argv"][1], "build");
    assert!(rows[0]["build_command"].is_null());
    assert_eq!(rows[1]["passed"], 1);
}

/// A one-case suite whose case lists `case_extra` and whose source is the smoke case.
fn probe_suite(suite_extra: &str, case_extra: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/probe");
    fs::create_dir_all(&case).expect("create case directory");
    fs::write(
        temp.path().join("corpus.toml"),
        format!("version = 1\ncases_dir = \"cases\"\n{suite_extra}"),
    )
    .expect("write suite manifest");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    fs::write(
        case.join("case.toml"),
        format!("version = 1\ninput = \"main.fab\"\n{case_extra}"),
    )
    .expect("write case manifest");
    // A stand-in runtime crate: these suites never reach a Cargo build, so only its
    // manifest has to exist for the recipe to validate.
    fs::create_dir(temp.path().join("runtime")).expect("create stand-in runtime");
    fs::write(temp.path().join("runtime/Cargo.toml"), "").expect("write stand-in runtime");
    temp
}

fn run_corpus(suite: &std::path::Path, target: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", target])
        .arg(suite)
        .output()
        .expect("run corpus suite")
}

fn runtime_recipe(target: &str) -> String {
    format!("\n[targets.{target}]\nruntime = \"runtime\"\n")
}

#[test]
fn corpus_loader_accepts_rejects_and_known_fail_why() {
    let rejection_source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/typi/inf-prefix-error.fab");
    let with_rejection_source = |suite: &tempfile::TempDir| {
        fs::copy(&rejection_source, suite.path().join("cases/probe/main.fab"))
            .expect("copy rejection source");
    };

    let rejects = "\n[targets.runner]\noutcome = \"rejects\"\nphase = \"run\"\ndiagnostic_contains = \"SEM002:unknown_type\"\n";
    let suite = probe_suite("", rejects);
    with_rejection_source(&suite);
    let output = run_corpus(suite.path(), "runner");
    assert!(
        output.status.success(),
        "a rejects row must load and pass: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let known_fail = "\n[targets.runner]\noutcome = \"known-fail\"\nphase = \"run\"\ndiagnostic_contains = \"SEM002:unknown_type\"\nknown_fail_why = \"the checker refuses unknown types\"\n";
    let suite = probe_suite("", known_fail);
    with_rejection_source(&suite);
    let output = run_corpus(suite.path(), "runner");
    assert!(
        output.status.success(),
        "a known-fail row with a reason must load and pass: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let rejects_with_why = "\n[targets.runner]\noutcome = \"rejects\"\nphase = \"run\"\ndiagnostic_contains = \"SEM002:unknown_type\"\nknown_fail_why = \"x\"\n";
    let suite = probe_suite("", rejects_with_why);
    let output = run_corpus(suite.path(), "runner");
    assert_eq!(
        output.status.code(),
        Some(78),
        "a reason on a rejection must be refused as a bad configuration"
    );
    assert!(
        records(&output.stdout)[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("known_fail_why")),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let empty_why = "\n[targets.runner]\noutcome = \"known-fail\"\nphase = \"run\"\ndiagnostic_contains = \"SEM002:unknown_type\"\nknown_fail_why = \"\"\n";
    let suite = probe_suite("", empty_why);
    let output = run_corpus(suite.path(), "runner");
    assert_eq!(
        output.status.code(),
        Some(78),
        "an empty known_fail_why must be refused as a bad configuration"
    );
    assert!(
        records(&output.stdout)[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("known_fail_why")),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[cfg(feature = "hir-go")]
#[test]
fn corpus_external_target_pins_a_wrong_exit_status() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/ignored-exit");
    fs::create_dir_all(&case).expect("create case directory");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy Faber source");
    let suite = |run: &str| {
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[targets.go]\nemit = \"go\"\nextension = \"go\"\nrun = [\"sh\", \"-c\", \"{run}\"]\n"
        )
    };
    fs::write(temp.path().join("corpus.toml"), suite("exit 0")).expect("write suite manifest");
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[expect]\ncheck_only = true\n\n[targets.go]\noutcome = \"known-fail\"\nphase = \"run\"\nfailure_kind = \"exit-mismatch\"\n\n[targets.go.expect]\nexit = 3\n",
    )
    .expect("write case manifest");

    // The program should end with 3 and ends with 0: the pin holds.
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "go"])
        .arg(temp.path())
        .output()
        .expect("run suite with pinned wrong exit");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(records(&output.stdout)[0]["status"], "expected-fail");

    // The program now ends with 3: the pin is stale.
    fs::write(temp.path().join("corpus.toml"), suite("exit 3")).expect("fix the exit");
    let unexpected = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "go"])
        .arg(temp.path())
        .output()
        .expect("run suite after the exit is fixed");
    assert!(!unexpected.status.success());
    assert_eq!(
        records(&unexpected.stdout)[0]["reason"],
        "unexpected exit match; update target expectation"
    );

    // The pin names a nonzero expected exit: without one it is refused.
    fs::write(
        case.join("case.toml"),
        "version = 1\ninput = \"main.fab\"\n\n[expect]\ncheck_only = true\n\n[targets.go]\noutcome = \"known-fail\"\nphase = \"run\"\nfailure_kind = \"exit-mismatch\"\n",
    )
    .expect("write case manifest without an exit");
    let refused = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "go"])
        .arg(temp.path())
        .output()
        .expect("run suite with an exit-mismatch pin that names no exit");
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stdout).contains("exit-mismatch oracle"),
        "{}",
        String::from_utf8_lossy(&refused.stdout)
    );
}

#[test]
fn corpus_rejects_pins_the_target_cannot_judge_instead_of_panicking() {
    let known_fail = |target: &str, phase: &str, kind: &str| {
        format!(
            "\n[targets.{target}]\noutcome = \"known-fail\"\nphase = \"{phase}\"\nfailure_kind = \"{kind}\"\n\n[targets.{target}.expect]\nstdout = \"expected.stdout\"\nexit_nonzero = true\n"
        )
    };
    let diagnostic_known_fail = |target: &str, phase: &str| {
        format!(
            "\n[targets.{target}]\noutcome = \"known-fail\"\nphase = \"{phase}\"\ndiagnostic_contains = \"anything\"\n"
        )
    };
    let gpu_recipe = "\n[targets.gpu]\nmode = \"gpu\"\n".to_owned();
    let shapes = [
        (
            "runner",
            String::new(),
            known_fail("runner", "run", "nonzero-exit"),
            "nonzero-exit",
        ),
        (
            "runner",
            String::new(),
            "\n[targets.runner]\noutcome = \"pass\"\nphase = \"emit\"\n".to_owned(),
            "phase other than run",
        ),
        (
            "runner",
            String::new(),
            "\n[expect]\ncheck_only = true\n\n[targets.runner]\noutcome = \"pass\"\ntimeout_secs = 0\n"
                .to_owned(),
            "timeout_secs must be positive",
        ),
        (
            "canonical",
            runtime_recipe("canonical"),
            known_fail("canonical", "run", "stdout-mismatch"),
            "stdout-mismatch",
        ),
        (
            "canonical",
            runtime_recipe("canonical"),
            diagnostic_known_fail("canonical", "run"),
            "run phase",
        ),
        (
            "canonical",
            runtime_recipe("canonical"),
            "\n[expect]\ncheck_only = true\n\n[targets.canonical]\noutcome = \"pass\"\nphase = \"run\"\n"
                .to_owned(),
            "run phase",
        ),
        (
            "gpu",
            gpu_recipe.clone(),
            known_fail("gpu", "run", "stdout-mismatch"),
            "stdout-mismatch",
        ),
        (
            "gpu",
            gpu_recipe,
            diagnostic_known_fail("gpu", "build"),
            "outside the emit phase",
        ),
    ];
    for (target, suite_extra, case_extra, fragment) in shapes {
        let suite = probe_suite(&suite_extra, &case_extra);
        let output = run_corpus(suite.path(), target);
        assert_eq!(
            output.status.code(),
            Some(78),
            "{target} {fragment}: a rejected pin must exit 78 (bad configuration), not panic: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let message = records(&output.stdout)[0]["message"]
            .as_str()
            .expect("validation error message")
            .to_owned();
        assert!(
            message.contains("probe") && message.contains(target) && message.contains(fragment),
            "{target} {fragment}: unexpected message {message}"
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_and_canonical_emit_known_failure_cannot_hide_an_unexpected_pass() {
    for target in ["rust", "canonical"] {
        // Emit succeeds and the emitted program contains the pinned fragment: the exit
        // status decides, so this is an unexpected pass and not an expected failure.
        let suite = probe_suite(
            &runtime_recipe(target),
            &format!(
                "\n[targets.{target}]\noutcome = \"known-fail\"\nphase = \"emit\"\ndiagnostic_contains = \"Salve, Munde!\"\n"
            ),
        );
        let output = run_corpus(suite.path(), target);
        assert!(!output.status.success(), "{target} must not pass");
        let rows = records(&output.stdout);
        assert_eq!(rows[0]["status"], "fail", "{target}");
        assert_eq!(
            rows[0]["reason"], "unexpected pass; update target expectation",
            "{target}"
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_and_canonical_emit_pass_without_a_diagnostic_fragment() {
    for target in ["rust", "canonical"] {
        let suite = probe_suite(
            &runtime_recipe(target),
            &format!("\n[targets.{target}]\noutcome = \"pass\"\nphase = \"emit\"\n"),
        );
        let output = run_corpus(suite.path(), target);
        assert!(
            output.status.success(),
            "{target}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let rows = records(&output.stdout);
        assert_eq!(rows[0]["status"], "pass", "{target}");
        assert_eq!(rows[0]["phase"], "emit", "{target}");
    }
}

#[test]
fn corpus_times_out_a_hanging_case_and_kills_its_descendants() {
    let suite_extra = "\n[targets.probe]\nemit = \"faber\"\nextension = \"fab\"\nrun = [\"sh\", \"-c\", \"sleep 40 & echo $! > {case_dir}/child.pid; wait\"]\n";
    let suite = probe_suite(
        suite_extra,
        "\n[expect]\ncheck_only = true\n\n[targets.probe]\noutcome = \"pass\"\ntimeout_secs = 1\n",
    );
    let started = std::time::Instant::now();
    let output = run_corpus(suite.path(), "probe");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "the hanging case must be cut off by its bound"
    );
    assert!(!output.status.success(), "a timeout is a failure");
    let rows = records(&output.stdout);
    assert_eq!(rows[0]["status"], "fail");
    assert_eq!(rows[0]["phase"], "run");
    assert!(
        rows[0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("timeout") && reason.contains("1s")),
        "{}",
        rows[0]["reason"]
    );

    let pid = fs::read_to_string(suite.path().join("target/corpus/probe/case_0000/child.pid"))
        .expect("the hanging case recorded its child")
        .trim()
        .to_owned();
    let alive = || {
        Command::new("sh")
            .args(["-c", "kill -0 \"$1\"", "sh", &pid])
            .output()
            .expect("probe child")
            .status
            .success()
    };
    for _ in 0..40 {
        if !alive() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!alive(), "the timed-out case's descendant must be killed");
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_does_not_report_an_all_inapplicable_run_as_green() {
    let not_applicable = |target: &str| {
        format!("\n[targets.{target}]\noutcome = \"not-applicable\"\nreason = \"probe\"\n")
    };
    let shapes = [
        ("runner", String::new()),
        ("rust", runtime_recipe("rust")),
        ("canonical", runtime_recipe("canonical")),
        (
            "roundtrip",
            "\n[targets.roundtrip]\nmode = \"roundtrip\"\n".to_owned(),
        ),
    ];
    for (target, suite_extra) in shapes {
        let suite = probe_suite(&suite_extra, &not_applicable(target));
        let output = run_corpus(suite.path(), target);
        assert!(
            !output.status.success(),
            "{target}: all cases not-applicable must not exit green"
        );
        let rows = records(&output.stdout);
        let summary = rows.last().expect("summary");
        assert_eq!(summary["not_applicable"], 1, "{target}");
        assert_eq!(summary["status"], "no-applicable-cases", "{target}");
    }
}

/// A runner-only suite case with no output assertions, for filter tests.
fn check_only_case(suite: &std::path::Path, name: &str) {
    let case = suite.join("cases").join(name);
    fs::create_dir_all(&case).expect("create case directory");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    fs::write(
        case.join("case.toml"),
        format!(
            "version = 1\nid = \"{name}\"\ninput = \"main.fab\"\n\n[expect]\ncheck_only = true\n\n[targets.runner]\noutcome = \"pass\"\n"
        ),
    )
    .expect("write case manifest");
}

fn corpus_error_message(output: &std::process::Output) -> String {
    records(&output.stdout)[0]["message"]
        .as_str()
        .expect("corpus error message")
        .to_owned()
}

#[test]
fn corpus_filter_selects_matching_cases() {
    let temp = tempfile::tempdir().expect("temporary suite");
    check_only_case(temp.path(), "alpha");
    check_only_case(temp.path(), "beta");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n",
    )
    .expect("write suite manifest");

    let filtered = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner", "--filter", "alpha"])
        .arg(temp.path())
        .output()
        .expect("run filtered suite");
    assert!(
        filtered.status.success(),
        "{}",
        String::from_utf8_lossy(&filtered.stdout)
    );
    let rows = records(&filtered.stdout);
    assert_eq!(rows[0]["case"], "alpha");
    let summary = rows.last().expect("summary");
    assert_eq!(summary["cases"], 1);
    assert_eq!(summary["passed"], 1);

    let none = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args([
            "corpus",
            "--target",
            "runner",
            "--filter",
            "zzz-no-such-case",
        ])
        .arg(temp.path())
        .output()
        .expect("run non-matching filter");
    assert!(!none.status.success(), "a non-matching filter must fail");
    assert!(corpus_error_message(&none).contains("no cases match"));

    let single = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--filter", "alpha"])
        .arg(smoke_case())
        .output()
        .expect("run filter on a single input");
    assert!(
        !single.status.success(),
        "--filter must reject a single input"
    );
    assert!(corpus_error_message(&single).contains("--filter applies only to a suite"));
}

#[test]
fn corpus_embedded_only_ignores_library_home_env() {
    let temp = tempfile::tempdir().expect("temporary suite");
    check_only_case(temp.path(), "smoke");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\nembedded_only = true\n",
    )
    .expect("write suite manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .arg(temp.path())
        .env("FABER_LIBRARY_HOME", "/nonexistent-faber-library-home")
        .output()
        .expect("run embedded-only suite under a library home");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = records(&output.stdout);
    let summary = rows.last().expect("summary");
    assert_eq!(summary["status"], "pass");
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_embedded_only_rust_builds_against_materialized_runtime() {
    let temp = tempfile::tempdir().expect("temporary suite");
    let case = temp.path().join("cases/smoke");
    fs::create_dir_all(&case).expect("create case directory");
    fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
    fs::copy(
        smoke_case().with_extension("expected"),
        case.join("main.expected"),
    )
    .expect("copy source golden");
    fs::write(
        case.join("case.toml"),
        "version = 1\nid = \"smoke\"\ninput = \"main.fab\"\n\n[expect]\nstdout = \"main.expected\"\n\n[targets.rust]\noutcome = \"pass\"\n",
    )
    .expect("write case manifest");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\nembedded_only = true\n\n[targets.rust]\n",
    )
    .expect("write suite manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(temp.path())
        .output()
        .expect("run embedded-only Rust suite");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = records(&output.stdout);
    let summary = rows.last().expect("summary");
    assert_eq!(summary["target"], "rust");
    assert_eq!(summary["passed"], 1);
    assert_eq!(summary["failed"], 0);
}

/// A suite with one runner-passing case and a `go` external recipe, so the
/// `runner` target and a named external target are both selectable.
fn exit_class_suite() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temporary suite");
    check_only_case(temp.path(), "alpha");
    fs::write(
        temp.path().join("cases/alpha/case.toml"),
        "version = 1\nid = \"alpha\"\ninput = \"main.fab\"\n\n[expect]\ncheck_only = true\n\n[targets.runner]\noutcome = \"pass\"\n\n[targets.go]\noutcome = \"pass\"\n\n[targets.fab]\noutcome = \"pass\"\n\n[targets.rust]\noutcome = \"pass\"\n",
    )
    .expect("write case manifest");
    // A stand-in runtime crate: the rust recipe must validate so a build
    // without the rust leaf reaches its own target gate
    // (`corpus_target_missing_from_the_build_exits_69`); these suites never
    // reach a Cargo build.
    fs::create_dir(temp.path().join("runtime")).expect("create stand-in runtime");
    fs::write(temp.path().join("runtime/Cargo.toml"), "").expect("write stand-in runtime");
    fs::write(
        temp.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[targets.go]\nemit = \"go\"\nextension = \"go\"\nrun = [\"sh\", \"-c\", \"exit 0\"]\n\n[targets.fab]\nemit = \"faber\"\nextension = \"fab\"\nrun = [\"sh\", \"-c\", \"exit 0\"]\n\n[targets.rust]\nruntime = \"runtime\"\n",
    )
    .expect("write suite manifest");
    temp
}

/// Run `faber corpus` with `args` and return the exit status and the parsed
/// JSON-lines stdout (every line must parse: a failure keeps the stream valid).
fn corpus_exit(args: &[&str], input: &std::path::Path) -> (Option<i32>, Vec<serde_json::Value>) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("corpus")
        .args(args)
        .arg(input)
        .output()
        .expect("run corpus");
    (output.status.code(), records(&output.stdout))
}

const USAGE: i32 = 64;
const NO_INPUT: i32 = 66;
const CANT_CREATE: i32 = 73;
const CONFIG: i32 = 78;

#[test]
fn corpus_invocation_and_config_exit_classes() {
    let suite = exit_class_suite();

    // A target the suite has no recipe for is a bad invocation of a valid suite.
    let (code, rows) = corpus_exit(&["--target", "nosuch"], suite.path());
    assert_eq!(code, Some(USAGE), "{rows:?}");
    assert_eq!(rows[0]["type"], "error");
    // A non-matching filter, and a suite-only option on a single input.
    let (code, _) = corpus_exit(&["--target", "runner", "--filter", "zzz"], suite.path());
    assert_eq!(code, Some(USAGE));
    let (code, _) = corpus_exit(&["--filter", "alpha"], &smoke_case());
    assert_eq!(code, Some(USAGE));
    // A valid suite and target still exit 0. The `go` recipe needs the go
    // leaf, so builds without it (the public cut) prove the same exit class
    // with the `fab` recipe, whose `emit = "faber"` every build serves.
    let (code, _) = corpus_exit(&["--target", "fab"], suite.path());
    assert_eq!(code, Some(0), "a valid suite and target still exit 0");
    #[cfg(feature = "hir-go")]
    {
        let (code, _) = corpus_exit(&["--target", "go"], suite.path());
        assert_eq!(code, Some(0), "a valid suite and target still exit 0");
    }

    // A failed expectation is an outcome, not a configuration error: still 1.
    let failing = tempfile::tempdir().expect("temporary suite");
    check_only_case(failing.path(), "alpha");
    fs::write(
        failing.path().join("cases/alpha/case.toml"),
        "version = 1\nid = \"alpha\"\ninput = \"main.fab\"\n\n[expect]\nstdout_contains = \"no-such-text\"\n\n[targets.runner]\noutcome = \"pass\"\n",
    )
    .expect("rewrite case manifest");
    fs::write(
        failing.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n",
    )
    .expect("write suite manifest");
    let (code, rows) = corpus_exit(&["--target", "runner"], failing.path());
    assert_eq!(code, Some(1), "{rows:?}");
    assert_eq!(rows.last().expect("summary")["status"], "fail");

    // A bad corpus.toml or case.toml, or a version it does not support.
    let bad = exit_class_suite();
    fs::write(bad.path().join("corpus.toml"), "version = [\n").expect("break toml");
    let (code, rows) = corpus_exit(&["--target", "runner"], bad.path());
    // Not a suite (no readable corpus.toml table), so it is judged as a single
    // input only when corpus.toml is absent; a present-but-invalid one is 78.
    assert_eq!(code, Some(CONFIG), "{rows:?}");
    fs::write(
        bad.path().join("corpus.toml"),
        "version = 9\ncases_dir = \"cases\"\n",
    )
    .expect("break version");
    let (code, _) = corpus_exit(&["--target", "runner"], bad.path());
    assert_eq!(code, Some(CONFIG));
    fs::write(
        bad.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"no-such-cases\"\n",
    )
    .expect("break cases dir");
    let (code, _) = corpus_exit(&["--target", "runner"], bad.path());
    assert_eq!(code, Some(CONFIG));
    fs::write(
        bad.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n",
    )
    .expect("restore suite");
    fs::write(bad.path().join("cases/alpha/case.toml"), "version = [\n").expect("break case");
    let (code, _) = corpus_exit(&["--target", "runner"], bad.path());
    assert_eq!(code, Some(CONFIG));

    // A declared external recipe with a missing required field.
    let recipe = exit_class_suite();
    fs::write(
        recipe.path().join("corpus.toml"),
        "version = 1\ncases_dir = \"cases\"\n\n[targets.go]\nextension = \"go\"\n",
    )
    .expect("write incomplete recipe");
    let (code, _) = corpus_exit(&["--target", "go"], recipe.path());
    assert_eq!(code, Some(CONFIG));

    // A missing input.
    let missing = suite.path().join("no-such-input.fab");
    let (code, rows) = corpus_exit(&["--target", "runner"], &missing);
    assert_eq!(code, Some(NO_INPUT), "{rows:?}");
}

#[cfg(not(feature = "hir-rust"))]
#[test]
fn corpus_target_missing_from_the_build_exits_69() {
    let suite = exit_class_suite();
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "rust"])
        .arg(suite.path())
        .output()
        .expect("run corpus");
    assert_eq!(output.status.code(), Some(69));
}

#[test]
fn corpus_write_failures_exit_73() {
    // The suite's `target` entry is a file, so no workspace can be created
    // under it.
    let suite = exit_class_suite();
    fs::write(suite.path().join("target"), "not a directory").expect("block workspace");
    let (code, rows) = corpus_exit(&["--target", "go"], suite.path());
    assert_eq!(code, Some(CANT_CREATE), "{rows:?}");
    assert_eq!(rows.last().expect("error row")["type"], "error");

    // The runner target stages nothing, so the same suite still runs.
    let (code, rows) = corpus_exit(&["--target", "runner"], suite.path());
    assert_eq!(code, Some(0), "{rows:?}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn corpus_rust_workspace_write_failure_exits_73() {
    let suite = probe_suite(
        &runtime_recipe("rust"),
        "[expect]\ncheck_only = true\n\n[targets.rust]\noutcome = \"pass\"\n",
    );
    fs::write(suite.path().join("target"), "not a directory").expect("block workspace");
    let (code, rows) = corpus_exit(&["--target", "rust"], suite.path());
    assert_eq!(code, Some(CANT_CREATE), "{rows:?}");
    assert_eq!(rows.last().expect("error row")["type"], "error");
}

/// A three-case runner suite: `cases/{a,b,c}` with the given `corpus.toml` extras
/// and per-case manifest extras (`category`/`domain` lines).
fn selection_suite(suite_extra: &str, tags: [&str; 3]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temporary suite");
    fs::write(
        temp.path().join("corpus.toml"),
        format!(
            "version = 1\ncases_dir = \"cases\"\n\n[expectations.check.runner]\noutcome = \"pass\"\n{suite_extra}"
        ),
    )
    .expect("write suite manifest");
    for (name, tag) in ["a", "b", "c"].into_iter().zip(tags) {
        let case = temp.path().join("cases").join(name);
        fs::create_dir_all(&case).expect("create case directory");
        fs::copy(smoke_case(), case.join("main.fab")).expect("copy source");
        fs::write(
            case.join("case.toml"),
            format!(
                "version = 1\ngroup = \"check\"\ninput = \"main.fab\"\n{tag}\n[expect]\ncheck_only = true\n"
            ),
        )
        .expect("write case manifest");
    }
    temp
}

fn tagged_selection_suite() -> tempfile::TempDir {
    selection_suite(
        "\n[domains]\nd1 = [\"alpha\", \"beta\"]\nd2 = [\"gamma\"]\n",
        [
            "category = \"alpha\"\n",
            "category = \"beta\"\ndomain = \"d1\"\n",
            "category = \"gamma\"\n",
        ],
    )
}

/// Run the suite with selector arguments; the status and the ids of the cases it ran.
fn selected(suite: &std::path::Path, selectors: &[&str]) -> (Option<i32>, Vec<String>) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["corpus", "--target", "runner"])
        .args(selectors)
        .arg(suite)
        .output()
        .expect("run corpus selection");
    let mut ids: Vec<String> = records(&output.stdout)
        .iter()
        .filter(|row| row["type"] == "case")
        .map(|row| row["case"].as_str().expect("case id").to_owned())
        .collect();
    ids.sort();
    (output.status.code(), ids)
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn corpus_selection_cli_selectors_union_and_every_domain_selects_the_suite() {
    let suite = tagged_selection_suite();
    let path = suite.path();
    assert_eq!(selected(path, &[]), (Some(0), strings(&["a", "b", "c"])));
    assert_eq!(
        selected(path, &["--domain", "d1"]),
        (Some(0), strings(&["a", "b"]))
    );
    assert_eq!(
        selected(path, &["--domain", "d2", "--case", "a"]),
        (Some(0), strings(&["a", "c"]))
    );
    assert_eq!(
        selected(path, &["--category", "beta,gamma"]),
        (Some(0), strings(&["b", "c"]))
    );
    assert_eq!(
        selected(path, &["--filter", "c", "--category", "alpha"]),
        (Some(0), strings(&["a", "c"]))
    );
    assert_eq!(
        selected(path, &["--domain", "d1,d2"]),
        (Some(0), strings(&["a", "b", "c"]))
    );
}

#[test]
fn corpus_selection_cli_refuses_unknown_names_with_the_usage_status() {
    let suite = tagged_selection_suite();
    let path = suite.path();
    for selectors in [
        ["--case", "a,zzz"],
        ["--domain", "zzz"],
        ["--category", "zzz"],
    ] {
        assert_eq!(selected(path, &selectors), (Some(64), Vec::new()));
    }
}

#[test]
fn corpus_selection_cli_domain_and_category_need_a_domain_table() {
    let suite = selection_suite("", ["", "", ""]);
    let path = suite.path();
    assert_eq!(selected(path, &["--domain", "d1"]), (Some(64), Vec::new()));
    assert_eq!(
        selected(path, &["--category", "alpha"]),
        (Some(64), Vec::new())
    );
    assert_eq!(
        selected(path, &["--case", "b", "--filter", "c"]),
        (Some(0), strings(&["b", "c"]))
    );
}

#[test]
fn corpus_selection_cli_loader_rejects_tags_outside_the_partition() {
    let untagged = selection_suite(
        "\n[domains]\nd1 = [\"alpha\"]\n",
        [
            "category = \"alpha\"\n",
            "category = \"alpha\"\n",
            "category = \"beta\"\n",
        ],
    );
    assert_eq!(selected(untagged.path(), &[]).0, Some(78));
    let disagreeing = selection_suite(
        "\n[domains]\nd1 = [\"alpha\"]\nd2 = [\"beta\"]\n",
        [
            "category = \"alpha\"\n",
            "category = \"alpha\"\ndomain = \"d2\"\n",
            "category = \"beta\"\n",
        ],
    );
    assert_eq!(selected(disagreeing.path(), &[]).0, Some(78));
}
