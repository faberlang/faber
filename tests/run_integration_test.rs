//! Subprocess checks for the interpreted-source command surface.
//!
//! These exercise `faber script`, the canonical interpreted-source entry
//! point. The same code path remains reachable via `faber run --interpret`
//! until the Stage 6 clean break; tests migrated to `script` to prove the new
//! command routes the identical single-file / package / archive surface.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("faber-run-integration-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn workspace_root() -> PathBuf {
    // faberlang container root (siblings: norma, radix, examples).
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("faberlang container")
        .to_path_buf()
}

fn norma_library_home() -> PathBuf {
    let workspace = workspace_root();
    for candidate in workspace.ancestors() {
        if candidate.join("norma/src").exists() {
            return candidate.to_path_buf();
        }
    }
    panic!("could not find norma/src beside workspace");
}

fn run_faber(args: &[&str]) -> (String, String, bool) {
    let (stdout, stderr, status) = run_faber_status(args);
    (stdout, stderr, status.success())
}

fn run_faber_status(args: &[&str]) -> (String, String, std::process::ExitStatus) {
    run_faber_status_with_env(args, &[])
}

fn run_faber_status_with_env(
    args: &[&str],
    envs: &[(&str, &Path)],
) -> (String, String, std::process::ExitStatus) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(envs.iter().map(|(key, value)| (*key, value)));
    let mut child = command.spawn().expect("spawn faber run");

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut stdout)
        .expect("read stdout");

    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    let status = child.wait().expect("wait");
    (stdout, stderr, status)
}

fn run_faber_with_env(args: &[&str], envs: &[(&str, &Path)]) -> (String, String, bool) {
    let (stdout, stderr, status) = run_faber_status_with_env(args, envs);
    (stdout, stderr, status.success())
}

fn run_executable(path: &Path, args: &[&str]) -> (String, String, bool) {
    let mut child = Command::new(path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn executable");

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut stdout)
        .expect("read stdout");

    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    let status = child.wait().expect("wait");
    (stdout, stderr, status.success())
}

fn write_numeric_package(label: &str) -> PathBuf {
    let package = temp_dir(label);
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "run-interpret"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##,
    )
    .expect("write entry");
    fs::write(
        src.join("thing.fab"),
        r#"
incipit {
  nota "module entry must not run"
}

functio label() → numerus {
  redde 7
}
"#,
    )
    .expect("write module");
    package
}

fn write_basic_package(label: &str, source: &str) -> PathBuf {
    let package = temp_dir(label);
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        format!(
            r#"
[package]
name = "{label}"

[paths]
source = "src"
entry = "main.fab"
"#
        ),
    )
    .expect("write manifest");
    fs::write(src.join("main.fab"), source).expect("write entry");
    package
}

fn write_single_file(label: &str, source: &str) -> PathBuf {
    let dir = temp_dir(label);
    let file = dir.join("main.fab");
    fs::write(&file, source).expect("write single-file source");
    file
}

fn assert_no_generated_rust(package: &Path) {
    assert!(
        !package.join("target/faber/Cargo.toml").exists(),
        "interpreted package run must not emit a generated Rust crate"
    );
}

#[test]
fn manifest_fmir_text_build_target_is_used_without_cli_target() {
    let package = write_basic_package("manifest-fmir-text", "incipit { nota \"manifest\" }");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "manifest-fmir-text"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir-text"
"#,
    )
    .expect("write manifest");

    let (stdout, stderr, ok) = run_faber(&["build", package.to_str().expect("utf8 package path")]);

    assert!(ok, "manifest fmir-text build failed:\n{stderr}");
    assert_eq!(
        PathBuf::from(stdout.trim()),
        package.join("target/faber-mir/image.fmir.txt")
    );
    assert_no_generated_rust(&package);
}

#[test]
fn build_reader_locale_accepts_direct_entry_file_input() {
    let source = write_single_file(
        "build-reader-locale-single-file",
        r#"
incipit {
  nota "salve"
}
"#,
    );

    let (stdout, stderr, ok) = run_faber(&[
        "build",
        "--reader-locale",
        "zh-Hans",
        source.to_str().expect("utf8 source path"),
    ]);

    assert!(ok, "single-file build with reader locale failed:\n{stderr}");
    let binary = PathBuf::from(stdout.trim());
    assert!(
        binary.exists(),
        "expected built binary path in stdout, got:\n{stdout}"
    );
}

#[test]
fn build_reader_locale_accepts_manifest_file_input() {
    let package = write_basic_package(
        "build-reader-locale-manifest",
        r#"
incipit {
  nota "salve"
}
"#,
    );
    let manifest = package.join("faber.toml");

    let (stdout, stderr, ok) = run_faber(&[
        "build",
        "--reader-locale",
        "zh-Hans",
        manifest.to_str().expect("utf8 manifest path"),
    ]);

    assert!(ok, "manifest build with reader locale failed:\n{stderr}");
    let binary = PathBuf::from(stdout.trim());
    assert!(
        binary.exists(),
        "expected built binary path in stdout, got:\n{stdout}"
    );
}

#[test]
fn run_reader_locale_accepts_direct_entry_file_input() {
    let source = write_single_file(
        "run-reader-locale-single-file",
        r#"
incipit {
  nota "salve"
}
"#,
    );

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        "--reader-locale",
        "zh-Hans",
        source.to_str().expect("utf8 source path"),
    ]);

    assert!(ok, "single-file run with reader locale failed:\n{stderr}");
    assert_eq!(stdout, "salve\n");
}

#[test]
fn run_reader_locale_accepts_manifest_file_input() {
    let package = write_basic_package(
        "run-reader-locale-manifest",
        r#"
incipit {
  nota "salve"
}
"#,
    );
    let manifest = package.join("faber.toml");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        "--reader-locale",
        "zh-Hans",
        manifest.to_str().expect("utf8 manifest path"),
    ]);

    assert!(ok, "manifest run with reader locale failed:\n{stderr}");
    assert_eq!(stdout, "salve\n");
}

#[test]
fn run_interpret_rejects_reader_locale() {
    let source = write_single_file(
        "run-reader-locale-interpret-reject",
        r#"
incipit {
  nota "salve"
}
"#,
    );

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        "--interpret",
        "--reader-locale",
        "zh-Hans",
        source.to_str().expect("utf8 source path"),
    ]);

    assert!(!ok, "run --interpret should reject reader locale");
    assert!(stdout.is_empty(), "rejected run should not write stdout: {stdout}");
    assert!(
        stderr.contains("--reader-locale is not supported with `faber run --interpret`"),
        "expected interpret reader-locale rejection, got:\n{stderr}"
    );
}

#[test]
fn test_reader_locale_accepts_direct_entry_file_input() {
    let source = write_single_file(
        "test-reader-locale-single-file",
        r#"
incipit {
  nota "salve"
}
"#,
    );

    let (stdout, stderr, ok) = run_faber(&[
        "test",
        "--reader-locale",
        "zh-Hans",
        source.to_str().expect("utf8 source path"),
    ]);

    assert!(ok, "single-file test with reader locale failed:\n{stderr}");
    assert!(
        stdout.contains("test result: ok."),
        "expected cargo test success output, got:\n{stdout}"
    );
}

#[test]
fn test_reader_locale_accepts_manifest_file_input() {
    let package = write_basic_package(
        "test-reader-locale-manifest",
        r#"
incipit {
  nota "salve"
}
"#,
    );
    let manifest = package.join("faber.toml");

    let (stdout, stderr, ok) = run_faber(&[
        "test",
        "--reader-locale",
        "zh-Hans",
        manifest.to_str().expect("utf8 manifest path"),
    ]);

    assert!(ok, "manifest test with reader locale failed:\n{stderr}");
    assert!(
        stdout.contains("test result: ok."),
        "expected cargo test success output, got:\n{stdout}"
    );
}

#[test]
fn fmir_bin_package_forwards_runtime_arg_after_source_is_removed() {
    let package = write_basic_package(
        "fmir-bin-cli",
        r#"
@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota "fmir-bin §!"(args.name)
}
"#,
    );

    let (build_stdout, build_stderr, build_ok) = run_faber(&[
        "build",
        "--target",
        "fmir-bin",
        package.to_str().expect("utf8 package path"),
    ]);
    assert!(build_ok, "fmir-bin build failed:\n{build_stderr}");
    let entrypoint = PathBuf::from(build_stdout.trim());
    assert_eq!(entrypoint, package.join("target/faber-mir/exe/run"));
    assert!(
        package.join("target/faber-mir/exe/image.fmir").exists(),
        "expected colocated FMIR image"
    );
    let runner_manifest =
        fs::read_to_string(package.join("target/faber-mir/exe/runner/Cargo.toml"))
            .expect("read generated runner manifest");
    assert!(
        runner_manifest.contains(&format!(
            r#"faber = {{ path = "{}", version = "={}" }}"#,
            env!("CARGO_MANIFEST_DIR"),
            env!("CARGO_PKG_VERSION")
        )),
        "runner manifest must pin the faber path dependency version:\n{runner_manifest}"
    );

    let (run_stdout, run_stderr, run_ok) = run_faber(&[
        "run",
        "--target",
        "fmir-bin",
        package.to_str().expect("utf8 package path"),
        "--",
        "runtime",
    ]);
    assert!(run_ok, "faber run --target fmir-bin failed:\n{run_stderr}");
    assert_eq!(run_stdout, "fmir-bin runtime!\n");

    fs::remove_file(package.join("src/main.fab")).expect("remove source");
    fs::remove_file(package.join("target/faber-mir/exe/image.fmir")).expect("remove sidecar image");
    let relocated = temp_dir("fmir-bin-relocated").join("run");
    fs::copy(&entrypoint, &relocated).expect("relocate entrypoint");
    let (direct_stdout, direct_stderr, direct_ok) = run_executable(&relocated, &["runtime"]);
    assert!(
        direct_ok,
        "relocated fmir-bin entrypoint failed after source and sidecar removal:\n{direct_stderr}"
    );
    assert_eq!(direct_stdout, run_stdout);
    assert_no_generated_rust(&package);
}

#[test]
fn fmir_bin_multifile_package_runs_after_source_mutation() {
    let package = write_basic_package(
        "fmir-bin-multifile",
        r#"
importa ex "./message" privata message

@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota message.text(args.name)
}
"#,
    );
    fs::write(
        package.join("src/message.fab"),
        r#"
functio text(textus name) → textus {
  redde "multi §!"(name)
}
"#,
    )
    .expect("write module");

    let (build_stdout, build_stderr, build_ok) = run_faber(&[
        "build",
        "--target",
        "fmir-bin",
        package.to_str().expect("utf8 package path"),
    ]);
    assert!(build_ok, "fmir-bin build failed:\n{build_stderr}");
    let entrypoint = PathBuf::from(build_stdout.trim());
    fs::write(package.join("src/main.fab"), "incipit { nota \"mutated\" }").expect("mutate entry");
    fs::write(
        package.join("src/message.fab"),
        "functio text(textus name) → textus { redde \"mutated\" }",
    )
    .expect("mutate module");

    let (stdout, stderr, ok) = run_executable(&entrypoint, &["runtime"]);
    assert!(ok, "direct fmir-bin entrypoint failed:\n{stderr}");
    assert_eq!(stdout, "multi runtime!\n");
    assert_no_generated_rust(&package);
}

#[test]
fn fmir_bin_norma_solum_package_reads_file_after_source_is_removed() {
    let package = write_basic_package(
        "fmir-bin-solum",
        r#"importa ex "norma:solum" privata fileio

incipit {
  nota fileio.lege<textus>("__PAYLOAD__")
}
"#,
    );
    let payload = package.join("payload.txt");
    fs::write(&payload, "solum-ok").expect("write payload");
    let source = fs::read_to_string(package.join("src/main.fab"))
        .expect("read source")
        .replace("__PAYLOAD__", &payload.to_string_lossy());
    fs::write(package.join("src/main.fab"), source).expect("write source");

    let library_home = norma_library_home();
    let (build_stdout, build_stderr, build_ok) = run_faber_with_env(
        &[
            "build",
            "--target",
            "fmir-bin",
            package.to_str().expect("utf8 package path"),
        ],
        &[("FABER_LIBRARY_HOME", &library_home)],
    );
    assert!(build_ok, "fmir-bin build failed:\n{build_stderr}");
    let entrypoint = PathBuf::from(build_stdout.trim());
    fs::remove_file(package.join("src/main.fab")).expect("remove source");

    let (stdout, stderr, ok) = run_executable(&entrypoint, &[]);
    assert!(
        ok,
        "direct fmir-bin norma:solum entrypoint failed:\n{stderr}"
    );
    assert_eq!(stdout, "solum-ok\n");
    assert_no_generated_rust(&package);
}

fn write_zip_archive(label: &str, entries: &[(&str, &str)]) -> PathBuf {
    let dir = temp_dir(label);
    let archive = dir.join(format!("{label}.zip"));
    let file = fs::File::create(&archive).expect("create zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for (name, body) in entries {
        zip.start_file(name, options).expect("start zip file");
        zip.write_all(body.as_bytes()).expect("write zip file");
    }
    zip.finish().expect("finish zip");
    archive
}

fn numeric_manifest_entries(prefix: &str) -> Vec<(String, String)> {
    let root = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    };
    vec![
        (
            format!("{root}faber.toml"),
            r#"
[package]
name = "archive-interpret"

[paths]
source = "src"
entry = "main.fab"
"#
            .to_owned(),
        ),
        (
            format!("{root}src/main.fab"),
            r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##
            .to_owned(),
        ),
        (
            format!("{root}src/thing.fab"),
            r#"
functio label() → numerus {
  redde 7
}
"#
            .to_owned(),
        ),
    ]
}

fn write_numeric_manifest_archive(label: &str, prefix: &str) -> PathBuf {
    let entries = numeric_manifest_entries(prefix);
    let borrowed = entries
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect::<Vec<_>>();
    write_zip_archive(label, &borrowed)
}

#[test]
fn script_private_namespace_failure_does_not_emit_rust() {
    let package = temp_dir("private-namespace");
    fs::write(
        package.join("main.fab"),
        r##"
importa ex "./auxilium" privata Aux

incipit {
  nota Aux.secretum()
}
"##,
    )
    .expect("write entry");
    fs::write(
        package.join("auxilium.fab"),
        r#"
@ privata
functio secretum() → numerus {
  redde 1
}
"#,
    )
    .expect("write module");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package
            .join("main.fab")
            .to_str()
            .expect("utf8 package path"),
    ]);

    assert!(!ok, "private namespace package should fail");
    assert_eq!(stdout, "");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("SEM004.namespace_missing_export"),
        "expected private namespace diagnostic:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_import_cycle_failure_does_not_emit_rust() {
    let package = temp_dir("import-cycle");
    fs::write(
        package.join("main.fab"),
        r#"importa ex "./jobs" privata jobs

incipit {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"importa ex "./main" privata main

functio run() → vacuum {}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package
            .join("main.fab")
            .to_str()
            .expect("utf8 package path"),
    ]);

    assert!(!ok, "import cycle package should fail");
    assert_eq!(stdout, "");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("import cycle detected"),
        "expected import cycle diagnostic:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_norma_import_failure_does_not_emit_rust() {
    let package = temp_dir("norma-unsupported");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "norma:chorda" privata chorda

incipit {
  nota chorda.retorta("roma")
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["script", package.to_str().expect("utf8 package path")]);

    assert!(!ok, "norma package interpretation should fail explicitly");
    assert_eq!(stdout, "");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("package MIR does not yet support library imports"),
        "expected library import diagnostic:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_package_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-root");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
incipit argumenta args {
  nota "cli root"
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["script", package.to_str().expect("utf8 package path")]);

    assert!(ok, "CLI root package interpretation failed:\n{stderr}");
    assert_eq!(stdout, "cli root\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-operand");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "Ian",
    ]);

    assert!(ok, "CLI operand package interpretation failed:\n{stderr}");
    assert_eq!(stdout, "Ian\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_rest_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-rest-operand");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ operandus textus input
@ operandus ceteri textus files
incipit argumenta args {
  nota args.input
  fixum lista<textus> files ← args.files
  nota files.longitudo()
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "seed",
        "alpha",
        "beta",
    ]);

    assert!(
        ok,
        "CLI rest operand package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "seed\n2\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI rest operand package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_fixed_exit_uses_process_status_without_rust_emit() {
    let package = temp_dir("cli-fixed-exit");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
incipit argumenta args exitus 7 {
  nota "done"
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, status) =
        run_faber_status(&["script", package.to_str().expect("utf8 package path")]);

    assert_eq!(status.code(), Some(7), "stderr:\n{stderr}");
    assert_eq!(stdout, "done\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI fixed exit package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_flag_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-flag-option");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ optio verbose brevis "v" longum "verbose" typus bivalens
incipit argumenta args {
  nota args.verbose
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "--verbose",
    ]);

    assert!(
        ok,
        "CLI flag option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "verum\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_defaulted_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-defaulted-option");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ optio name longum "name" typus textus vel "worker"
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "--name=Ian",
    ]);

    assert!(
        ok,
        "CLI defaulted option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "Ian\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_optional_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-optional-option");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ optio name longum "name" typus textus
incipit argumenta args {
  nota args.name vel "worker"
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "--name=Ian",
    ]);

    assert!(
        ok,
        "CLI optional option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "Ian\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI optional option package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_global_flag_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-global-flag-option");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ optio verbose longum "verbose" typus bivalens ubique
incipit argumenta args {
  nota args.verbose
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "--verbose",
    ]);

    assert!(
        ok,
        "CLI global flag option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "verum\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI global option package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_root_defaulted_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-defaulted-operand");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ operandus numerus count vel 9
incipit argumenta args {
  nota args.count
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["script", package.to_str().expect("utf8 package path")]);

    assert!(
        ok,
        "CLI defaulted operand package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "9\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_mounted_command_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota "running"
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "jobs",
        "run",
    ]);

    assert!(ok, "mounted CLI package interpretation failed:\n{stderr}");
    assert_eq!(stdout, "running\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_mounted_global_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-global-option");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ optio verbose longum "verbose" typus bivalens ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota args.verbose
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "--verbose",
        "jobs",
        "run",
    ]);

    assert!(
        ok,
        "mounted CLI global option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "verum\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI global option interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_mounted_global_optional_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-global-optional-option");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ optio name longum "name" typus textus ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota args.name vel "worker"
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "--name=Ian",
        "jobs",
        "run",
    ]);

    assert!(
        ok,
        "mounted CLI global optional option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "Ian\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI global optional option interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_mounted_global_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-global-operand");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ operandus textus tenant ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ imperium "run"
@ operandus textus name
functio run() argumenta args {
  nota "§:§"(args.tenant, args.name)
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "jobs",
        "run",
        "acme",
        "Ian",
    ]);

    assert!(
        ok,
        "mounted CLI global operand package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "acme:Ian\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI global operand interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_mounted_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-operand");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ imperium "run"
@ operandus textus name
functio run() argumenta args {
  nota args.name
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "jobs",
        "run",
        "Ian",
    ]);

    assert!(
        ok,
        "mounted CLI operand package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "Ian\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI operand package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_mounted_numerus_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-numerus-operand");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ imperium "run"
@ operandus numerus count
functio run() argumenta args {
  nota args.count
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "jobs",
        "run",
        "7",
    ]);

    assert!(
        ok,
        "mounted CLI numerus operand package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "7\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI numerus operand package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_mounted_defaulted_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-defaulted-option");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ imperium "run"
@ optio name longum "name" typus textus vel "worker"
functio run() argumenta args {
  nota args.name
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "jobs",
        "run",
        "--name=Ian",
    ]);

    assert!(
        ok,
        "mounted CLI defaulted option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "Ian\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI defaulted option interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_cli_mounted_alias_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-alias");
    fs::write(
        package.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ imperium "run"
@ alias "start"
@ operandus textus name
functio run() argumenta args {
  nota args.name
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "jobs",
        "start",
        "Ian",
    ]);

    assert!(
        ok,
        "mounted CLI alias package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "Ian\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI alias package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_archive_root_manifest_executes_without_cargo_or_rust_emit() {
    let archive = write_numeric_manifest_archive("archive-root-manifest", "");

    let (stdout, stderr, ok) = run_faber(&["script", archive.to_str().expect("utf8 archive path")]);

    assert!(ok, "archive package interpretation failed:\n{stderr}");
    assert_eq!(stdout, "7\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "archive interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(archive.parent().expect("archive parent"));
}

#[test]
fn script_archive_wrapped_package_executes_without_cargo_or_rust_emit() {
    let archive = write_numeric_manifest_archive("archive-wrapped-package", "pkg");

    let (stdout, stderr, ok) = run_faber(&["script", archive.to_str().expect("utf8 archive path")]);

    assert!(
        ok,
        "wrapped archive package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "7\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "archive interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(archive.parent().expect("archive parent"));
}

#[test]
fn script_archive_manifestless_root_executes_without_cargo_or_rust_emit() {
    let archive = write_zip_archive(
        "archive-manifestless-root",
        &[
            (
                "main.fab",
                r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##,
            ),
            (
                "thing.fab",
                r#"
functio label() → numerus {
  redde 7
}
"#,
            ),
        ],
    );

    let (stdout, stderr, ok) = run_faber(&["script", archive.to_str().expect("utf8 archive path")]);

    assert!(ok, "manifestless archive interpretation failed:\n{stderr}");
    assert_eq!(stdout, "7\n");
    assert_no_generated_rust(archive.parent().expect("archive parent"));
}

#[test]
fn script_archive_rejects_parent_traversal_entry() {
    let archive = write_zip_archive(
        "archive-parent-traversal",
        &[("../escape.fab", "incipit { nota 1 }\n")],
    );
    let escaped = archive
        .parent()
        .expect("archive parent")
        .parent()
        .expect("outer temp parent")
        .join("escape.fab");
    let _ = fs::remove_file(&escaped);

    let (stdout, stderr, ok) = run_faber(&["script", archive.to_str().expect("utf8 archive path")]);

    assert!(!ok, "unsafe archive should fail");
    assert_eq!(stdout, "");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("unsafe archive entry `../escape.fab`"),
        "expected unsafe archive diagnostic:\n{stderr}"
    );
    assert!(
        !escaped.exists(),
        "archive extraction must not write outside its temp root"
    );
}

#[test]
fn script_archive_reports_missing_package_root() {
    let archive = write_zip_archive(
        "archive-missing-root",
        &[
            ("a/main.fab", "incipit { nota 1 }\n"),
            ("b/main.fab", "incipit { nota 2 }\n"),
        ],
    );

    let (stdout, stderr, ok) = run_faber(&["script", archive.to_str().expect("utf8 archive path")]);

    assert!(!ok, "ambiguous archive root should fail");
    assert_eq!(stdout, "");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("archive package root must contain `faber.toml`, `main.fab`, or one top-level package directory"),
        "expected archive root diagnostic:\n{stderr}"
    );
}

#[test]
fn script_archive_package_diagnostic_reports_archive_member_path() {
    let archive = write_zip_archive(
        "archive-private-diagnostic",
        &[
            (
                "main.fab",
                r##"
importa ex "./auxilium" privata Aux

incipit {
  nota Aux.secretum()
}
"##,
            ),
            (
                "auxilium.fab",
                r#"
@ privata
functio secretum() → numerus {
  redde 1
}
"#,
            ),
        ],
    );

    let (stdout, stderr, ok) = run_faber(&["script", archive.to_str().expect("utf8 archive path")]);

    assert!(!ok, "private archive package should fail");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("!/main.fab"),
        "expected archive member diagnostic path:\n{stderr}"
    );
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("SEM004.namespace_missing_export"),
        "expected private namespace diagnostic:\n{stderr}"
    );
    assert_no_generated_rust(archive.parent().expect("archive parent"));
}

#[test]
fn script_manifestless_entry_package_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("manifestless-numeric");
    fs::write(
        package.join("main.fab"),
        r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##,
    )
    .expect("write entry");
    fs::write(
        package.join("thing.fab"),
        r#"
functio label() → numerus {
  redde 7
}
"#,
    )
    .expect("write module");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package
            .join("main.fab")
            .to_str()
            .expect("utf8 package path"),
    ]);

    assert!(ok, "faber run --interpret failed:\n{stderr}");
    assert_eq!(stdout, "7\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "interpreted package run must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_package_inputs_execute_without_cargo_or_rust_emit() {
    let package = write_numeric_package("numeric");
    let manifest = package.join("faber.toml");
    let entry = package.join("src/main.fab");

    for input in [&package, &manifest, &entry] {
        let (stdout, stderr, ok) =
            run_faber(&["script", input.to_str().expect("utf8 package path")]);
        assert!(ok, "faber run --interpret failed:\n{stderr}");
        assert_eq!(stdout, "7\n");
        assert!(
            !stderr.contains("Compiling") && !stderr.contains("cargo"),
            "interpreted package run must not invoke Cargo:\n{stderr}"
        );
        assert_no_generated_rust(&package);
    }
}

#[test]
fn script_text_return_package_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("text-remap");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "run-interpret-text"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##,
    )
    .expect("write entry");
    fs::write(
        src.join("thing.fab"),
        r#"
functio label() → textus {
  redde "ok"
}
"#,
    )
    .expect("write module");

    let (stdout, stderr, ok) = run_faber(&["script", package.to_str().expect("utf8 path")]);

    assert!(ok, "text-return package interpretation failed:\n{stderr}");
    assert_eq!(stdout, "ok\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "text-return package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_manifestless_single_file_uses_single_source_stepper() {
    let dir = temp_dir("single-file");
    let script = dir.join("script.fab");
    fs::write(&script, "incipit { nota \"single\" }\n").expect("write script");

    let (stdout, stderr, ok) = run_faber(&["script", script.to_str().expect("utf8 path")]);

    assert!(ok, "single-file interpretation failed:\n{stderr}");
    assert_eq!(stdout, "single\n");
    assert_no_generated_rust(&dir);
}

#[test]
fn script_norma_solum_package_reads_file_through_kernel_bridge() {
    // One `norma:*` import block; interpreted package execution satisfies it
    // through the stepper kernel (Stage 1b host bridge). No second stepper-only
    // source file, no compiled-Rust fallback.
    let package = temp_dir("norma-solum-bridge");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "norma-solum-bridge"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    let payload = package.join("payload.txt");
    fs::write(&payload, "bridge-ok").expect("write payload");
    let payload_str = payload.to_str().expect("utf8 payload path");
    fs::write(
        src.join("main.fab"),
        format!(
            r#"importa ex "norma:solum" privata fileio

incipit {{
  nota fileio.lege<textus>("{payload_str}")
}}
"#
        ),
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["script", package.to_str().expect("utf8 package path")]);

    assert!(ok, "norma:solum package bridge failed:\n{stderr}");
    assert_eq!(stdout, "bridge-ok\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "interpreted package bridge must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_norma_solum_unsupported_verb_fails_closed() {
    let package = temp_dir("norma-solum-unsupported");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "norma-solum-unsupported"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r#"importa ex "norma:solum" privata fileio

incipit {
  fixum octeti bytes ← fileio.hauriet("/nonexistent")
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["script", package.to_str().expect("utf8 package path")]);

    assert!(!ok, "unsupported norma:solum verb should fail closed");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("package MIR kernel bridge does not support `norma:solum.hauriet`"),
        "expected kernel bridge fail-closed diagnostic:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn script_norma_processus_argumenta_bridges_to_kernel() {
    let package = temp_dir("norma-processus-bridge");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "norma-processus-bridge"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r#"importa ex "norma:processus" privata processus

incipit argumenta args {
  nota processus.argumenta()[0]
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "script",
        package.to_str().expect("utf8 package path"),
        "--",
        "first-arg",
    ]);

    assert!(ok, "norma:processus bridge failed:\n{stderr}");
    assert_eq!(stdout, "first-arg\n");
    assert_no_generated_rust(&package);
}
