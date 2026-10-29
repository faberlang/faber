//! Subprocess checks for the interpreted-source command surface.
//!
//! These exercise `faber run`, the canonical interpreted execution entry
//! point: `faber run` always interprets (single file, package, or archive)
//! through the runner and never compiles.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use zip::write::SimpleFileOptions;

#[path = "support/temp.rs"]
mod temp;
use temp::{TempDir, TempPath};

fn temp_dir(label: &str) -> TempDir {
    TempDir::new("faber-run-integration", label)
}

fn write_plain_file(label: &str, contents: &str) -> TempPath {
    let dir = temp_dir(label);
    let path = dir.join("plain.txt");
    fs::write(&path, contents).expect("write plain file");
    TempPath::new(dir, path)
}

/// Repository root (`crates/faber` → `../..`), normalized so paths the CLI
/// echoes back compare equal to ours.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

/// Radix-authored reader-locale sample package under `corpus-product/`.
fn reader_locale_example_root(locale: &str) -> PathBuf {
    repo_root()
        .join("corpus-product/reader-locale")
        .join(locale)
}

/// Library home holding Norma: the in-tree `lib/` (`lib/norma/src`).
fn norma_library_home() -> PathBuf {
    let home = repo_root().join("lib");
    assert!(
        home.join("norma/src").exists(),
        "could not find lib/norma/src under {}",
        repo_root().display()
    );
    home
}

fn run_faber(args: &[&str]) -> (String, String, bool) {
    let (stdout, stderr, status) = run_faber_status(args);
    (stdout, stderr, status.success())
}

/// Pin the U1 command-boundary ENOENT renderer: abs path + base + artifact.
fn assert_structured_missing_path(stderr: &str, relative: &str, artifact: &str) {
    let cwd = std::env::current_dir().expect("cwd");
    let path = cwd.join(relative);
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: CLI stderr is the renderer contract for this integration test.
    assert!(
        stderr.contains(&path.display().to_string()),
        "structured missing path must name the absolutized path {}, got:\n{stderr}",
        path.display()
    );
    assert!(
        stderr.contains(&format!(
            "cannot read {artifact} resolved against '{}'",
            cwd.display()
        )),
        "structured missing path must name artifact `{artifact}` and resolution base, got:\n{stderr}"
    );
    assert!(
        !stderr.contains(&format!("cannot read '{relative}'")),
        "must not hand-roll the relative input token, got:\n{stderr}"
    );
}

#[test]
fn targets_reports_faber_package_capability_for_fmir_targets() {
    let (stdout, stderr, ok) = run_faber(&["targets"]);
    assert!(ok, "faber targets failed:\n{stderr}");

    for target in ["fmir-text", "fmir"] {
        let prefix = format!("{target} ");
        let row = stdout
            .lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("missing {target} row:\n{stdout}"));
        assert!(
            row.contains("check=yes build=yes run=no package=yes"),
            "Faber package FMIR target row must show package build truth and no run (faber run refuses built images):\n{row}"
        );
        assert!(
            row.contains("faber build --target"),
            "Faber FMIR target note must point at Faber package build surface:\n{row}"
        );
        assert!(
            !row.contains("radix emit rejects"),
            "Faber target row must not present Radix emit truth as Faber command truth:\n{row}"
        );
    }

    let wgsl = stdout
        .lines()
        .find(|line| line.starts_with("wgsl-text "))
        .expect("missing wgsl-text row");
    assert!(
        wgsl.contains("run=no package=no") && wgsl.contains("not GPU launch or package runtime"),
        "systems emit targets must remain non-package/non-runtime claims:\n{wgsl}"
    );
}

#[test]
fn targets_run_column_claims_only_the_device_routes_faber_run_launches() {
    let (stdout, stderr, ok) = run_faber(&["targets"]);
    assert!(ok, "faber targets failed:\n{stderr}");

    // `faber run` interprets source; the only target routes it reaches are the
    // `--device metal|cuda` launches and the compiled `--target ts|go|llvm-host`
    // builds.
    let runnable: Vec<&str> = stdout
        .lines()
        .filter(|line| line.contains(" run=yes "))
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert!(
        runnable
            .iter()
            .all(|name| ["metal-text", "llvm-text", "ts", "go", "llvm-host"].contains(name)),
        "only the device routes may report run=yes, got {runnable:?}:\n{stdout}"
    );

    for name in ["fmir-text", "fmir", "fhir", "rust"] {
        let prefix = format!("{name} ");
        let row = stdout
            .lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("missing {name} row:\n{stdout}"));
        assert!(
            row.contains(" run=no "),
            "{name} is not run by `faber run`:\n{row}"
        );
        assert!(
            !row.contains("build/run --target") && !row.contains("faber run --target"),
            "{name} note must not advertise a `faber run` target:\n{row}"
        );
    }

    // And the dispatch agrees: `faber run` takes no other `--target` for any of them.
    for name in ["fmir-text", "fmir", "fhir", "rust"] {
        let (_, stderr, status) = run_faber_status(&["run", "--target", name, "."]);
        assert!(
            !status.success(),
            "`faber run --target {name}` must be refused"
        );
        assert!(
            stderr.contains("--target"),
            "refusal must name the unexpected `--target` flag:\n{stderr}"
        );
    }
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

fn write_numeric_package(label: &str) -> TempDir {
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
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        src.join("thing.fab"),
        r#"
main {
  print "module entry must not run"
}

@ public
fn label() → int {
  return 7
}
"#,
    )
    .expect("write module");
    package
}

fn write_basic_package(label: &str, source: &str) -> TempDir {
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

fn write_package_manifest(package: &Path, source: &str, entry: &str) {
    fs::write(
        package.join("faber.toml"),
        format!(
            r#"
[package]
name = "containment-check"

[paths]
source = "{source}"
entry = "{entry}"
"#
        ),
    )
    .expect("write manifest");
}

fn assert_package_path_rejected(stderr: &str, expected: &str) {
    assert!(
        stderr.contains(expected),
        "expected package path rejection containing `{expected}`, got:\n{stderr}"
    );
}

#[test]
fn check_manifest_rejects_source_parent_escape() {
    let workspace = temp_dir("package-source-parent-escape");
    let package = workspace.join("pkg");
    let outside = workspace.join("outside");
    fs::create_dir_all(&package).expect("create package");
    fs::create_dir_all(&outside).expect("create outside");
    fs::write(outside.join("main.fab"), "incipit { nota 1 }\n").expect("write outside entry");
    write_package_manifest(&package, "../outside", "main.fab");

    let (_stdout, stderr, ok) = run_faber(&["check", package.to_str().expect("utf8 package path")]);

    // Stage 0 restricts `paths.source` to the supported set (`src`, `.`), so
    // an escaping source value is rejected at manifest validation before the
    // member-path containment guards run. The check still fails loudly.
    assert!(!ok, "source parent escape must fail");
    assert_package_path_rejected(
        &stderr,
        "faber.toml paths.source '../outside' is not supported: only \"src\" (the default) and \".\" (the package root) are allowed until a usage contract for custom source roots is decided",
    );
}

#[test]
fn check_manifest_rejects_entry_parent_escape() {
    let workspace = temp_dir("package-entry-parent-escape");
    let package = workspace.join("pkg");
    let outside = workspace.join("outside");
    fs::create_dir_all(package.join("src")).expect("create source");
    fs::create_dir_all(&outside).expect("create outside");
    fs::write(outside.join("main.fab"), "incipit { nota 1 }\n").expect("write outside entry");
    write_package_manifest(&package, "src", "../../outside/main.fab");

    let (_stdout, stderr, ok) = run_faber(&["check", package.to_str().expect("utf8 package path")]);

    assert!(!ok, "entry parent escape must fail");
    assert_package_path_rejected(&stderr, "package member path escapes through its parent");
}

#[test]
fn check_manifest_rejects_absolute_package_path() {
    let workspace = temp_dir("package-absolute-path");
    let package = workspace.join("pkg");
    let outside = workspace.join("outside");
    fs::create_dir_all(package.join("src")).expect("create source");
    fs::create_dir_all(&outside).expect("create outside");
    let outside_entry = outside.join("main.fab");
    fs::write(&outside_entry, "incipit { nota 1 }\n").expect("write outside entry");
    write_package_manifest(
        &package,
        "src",
        outside_entry.to_str().expect("utf8 outside entry"),
    );

    let (_stdout, stderr, ok) = run_faber(&["check", package.to_str().expect("utf8 package path")]);

    assert!(!ok, "absolute entry path must fail");
    assert_package_path_rejected(&stderr, "package member path must be relative");
}

#[cfg(unix)]
#[test]
fn check_manifest_rejects_entry_symlink_escape() {
    use std::os::unix::fs::symlink;

    let workspace = temp_dir("package-entry-symlink-escape");
    let package = workspace.join("pkg");
    let source = package.join("src");
    let outside = workspace.join("outside");
    fs::create_dir_all(&source).expect("create source");
    fs::create_dir_all(&outside).expect("create outside");
    fs::write(outside.join("main.fab"), "incipit { nota 1 }\n").expect("write outside entry");
    symlink(&outside, source.join("linked")).expect("link outside source");
    write_package_manifest(&package, "src", "linked/main.fab");

    let (_stdout, stderr, ok) = run_faber(&["check", package.to_str().expect("utf8 package path")]);

    assert!(!ok, "entry symlink escape must fail");
    assert_package_path_rejected(&stderr, "package member resolves outside the package root");
}

fn write_single_file(label: &str, source: &str) -> TempPath {
    let dir = temp_dir(label);
    let file = dir.join("main.fab");
    fs::write(&file, source).expect("write single-file source");
    TempPath::new(dir, file)
}

fn assert_no_generated_rust(package: &Path) {
    assert!(
        !package.join("target/faber/Cargo.toml").exists(),
        "interpreted package run must not emit a generated Rust crate"
    );
}

#[test]
fn manifest_fmir_text_build_target_is_used_without_cli_target() {
    let package = write_basic_package("manifest-fmir-text", "main { print \"manifest\" }");
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

/// `faber run` takes source, never a built image: a `.fmir` / `.fmir.txt`
/// file is refused by name (never read as Faber source), and the package it
/// was built from still runs.
#[test]
fn run_refuses_built_fmir_images_by_name() {
    let package = write_basic_package("run-fmir-image", "main { print \"hi\" }");
    let package_arg = package.to_str().expect("utf8 package path");

    for (target, image, runner_hint) in [
        ("fmir", "target/faber-mir/image.fmir", true),
        ("fmir-text", "target/faber-mir/image.fmir.txt", false),
    ] {
        let (_, stderr, ok) = run_faber(&["build", "--target", target, package_arg]);
        assert!(ok, "{target} build failed:\n{stderr}");
        let image_path = package.join(image);
        assert!(image_path.is_file(), "missing built image {image_path:?}");

        let (stdout, stderr, status) =
            run_faber_status(&["run", image_path.to_str().expect("utf8 image path")]);
        assert!(!status.success(), "{target} image must be refused");
        assert!(stdout.is_empty(), "refused run printed:\n{stdout}");
        assert!(
            stderr.contains("not a runnable input for `faber run`"),
            "expected the refusal message, got:\n{stderr}"
        );
        assert_eq!(
            stderr.contains("faber __fmir-run"),
            runner_hint,
            "image-runner hint mismatch for {target}:\n{stderr}"
        );
        assert!(
            !stderr.contains("UTF-8"),
            "must never report a decode error:\n{stderr}"
        );
    }

    let (stdout, stderr, ok) = run_faber(&["run", package_arg]);
    assert!(ok, "package run failed:\n{stderr}");
    assert_eq!(stdout.trim(), "hi");
}

#[cfg(feature = "hir-rust")]
#[test]
fn build_reader_locale_accepts_direct_entry_file_input() {
    let source = write_single_file(
        "build-reader-locale-single-file",
        r#"
入口 {
  显示 "salve"
}
"#,
    );

    let (stdout, stderr, ok) = run_faber(&[
        "build",
        "--locale",
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
fn build_reader_locale_rejects_forced_package_stdin_input() {
    let (stdout, stderr, ok) = run_faber(&["build", "--package", "--locale", "zh-Hans", "-"]);

    assert!(!ok, "build should reject stdin reader-locale package input");
    assert!(
        stdout.is_empty(),
        "rejected build should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale zh-Hans requires a package path or .fab entry file"),
        "expected stdin shape rejection, got:\n{stderr}"
    );
}

#[test]
fn build_reader_locale_rejects_existing_non_package_file() {
    let plain = write_plain_file("build-reader-locale-plain-file", "temp");

    let (stdout, stderr, ok) = run_faber(&[
        "build",
        "--locale",
        "th-TH",
        plain.to_str().expect("plain path"),
    ]);

    assert!(
        !ok,
        "build should reject existing non-package files for reader-locale input"
    );
    assert!(
        stdout.is_empty(),
        "rejected build should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "expected reader-locale shape rejection, got:\n{stderr}"
    );
}

#[test]
fn build_reader_locale_rejects_multiple_direct_inputs() {
    let entry_a = write_single_file(
        "build-reader-locale-multi-a",
        "incipit { nota \"salve\" }\n",
    );
    let entry_b = write_single_file("build-reader-locale-multi-b", "incipit { nota \"vale\" }\n");

    let (stdout, stderr, ok) = run_faber(&[
        "build",
        "--locale",
        "th-TH",
        entry_a.to_str().expect("entry path"),
        entry_b.to_str().expect("entry path"),
    ]);

    assert!(
        !ok,
        "build should reject reader-locale when multiple direct inputs are supplied"
    );
    assert!(
        stdout.is_empty(),
        "rejected build should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("unexpected argument"),
        "expected clap to reject the extra build input, got:\n{stderr}"
    );
    assert!(
        stderr.contains("Usage: faber build [OPTIONS] <INPUT>"),
        "expected single-input build usage in clap error, got:\n{stderr}"
    );
}

#[test]
fn build_reader_locale_accepts_manifest_file_input() {
    let package = write_basic_package(
        "build-reader-locale-manifest",
        r#"
入口 {
  显示 "salve"
}
"#,
    );
    let manifest = package.join("faber.toml");

    let (stdout, stderr, ok) = run_faber(&[
        "build",
        "--locale",
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
入口 {
  显示 "salve"
}
"#,
    );

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        "--locale",
        "zh-Hans",
        source.to_str().expect("utf8 source path"),
    ]);

    assert!(ok, "single-file run with reader locale failed:\n{stderr}");
    assert_eq!(stdout, "salve\n");
}

#[test]
fn run_unhandled_failable_call_in_main_panics_with_the_error_message() {
    // D17.8 (fi6c): a failure directly in `main` is a panic: exit code 70, the
    // output before it kept, and the error value reported on stderr.
    let source = write_single_file(
        "run-unhandled-failable-call-panics",
        r#"
fn risky(int n) → int ⇥ string {
    if n ≺ 0 {
        throw "negative input"
    }
    return n + 1
}

main {
    print risky(1)
    let int x ← risky(-1)
    print x
}
"#,
    );

    let (stdout, stderr, status) =
        run_faber_status(&["run", source.to_str().expect("utf8 source path")]);

    assert_eq!(
        status.code(),
        Some(70),
        "a panic exits with code 70:\n{stderr}"
    );
    assert_eq!(stdout, "2\n", "output before the panic is kept");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: the runtime abort message is the CLI stderr contract.
    assert!(
        stderr.contains("negative input"),
        "the panic message must reach stderr, got:\n{stderr}"
    );
}

#[test]
fn run_reader_locale_accepts_manifest_file_input() {
    let package = write_basic_package(
        "run-reader-locale-manifest",
        r#"
入口 {
  显示 "salve"
}
"#,
    );
    let manifest = package.join("faber.toml");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        "--locale",
        "zh-Hans",
        manifest.to_str().expect("utf8 manifest path"),
    ]);

    assert!(ok, "manifest run with reader locale failed:\n{stderr}");
    assert_eq!(stdout, "salve\n");
}

#[test]
fn run_reader_locale_rejects_stdin_input() {
    let (stdout, stderr, ok) = run_faber(&["run", "--locale", "zh-Hans", "-"]);

    assert!(!ok, "run should reject stdin reader-locale input");
    assert!(
        stdout.is_empty(),
        "rejected run should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale zh-Hans requires a package path or .fab entry file"),
        "expected stdin reader-locale rejection, got:\n{stderr}"
    );
}

#[test]
fn run_reader_locale_rejects_existing_non_package_file() {
    let plain = write_plain_file("run-reader-locale-plain-file", "temp");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        "--locale",
        "th-TH",
        plain.to_str().expect("plain path"),
    ]);

    assert!(
        !ok,
        "run should reject existing non-package files for reader-locale input"
    );
    assert!(
        stdout.is_empty(),
        "rejected run should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "expected reader-locale shape rejection, got:\n{stderr}"
    );
}

#[test]
fn run_reader_locale_rejects_multiple_direct_inputs() {
    let entry_a = write_single_file("run-reader-locale-multi-a", "incipit { nota \"salve\" }\n");
    let entry_b = write_single_file("run-reader-locale-multi-b", "incipit { nota \"vale\" }\n");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        "--locale",
        "th-TH",
        entry_a.to_str().expect("entry path"),
        entry_b.to_str().expect("entry path"),
    ]);

    assert!(
        !ok,
        "run should reject reader-locale when multiple direct inputs are supplied"
    );
    assert!(
        stdout.is_empty(),
        "rejected run should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("unexpected argument"),
        "expected multiple-input parse rejection, got:\n{stderr}"
    );
}

#[test]
fn run_reader_locale_missing_package_path_surfaces_missing_path_error() {
    let (stdout, stderr, ok) = run_faber(&["run", "--locale", "th-TH", "missing-package"]);

    assert!(!ok, "missing package path should fail");
    assert!(
        stdout.is_empty(),
        "missing package path should not write stdout: {stdout}"
    );
    assert!(
        !stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "missing package path should reach the underlying filesystem error, got:\n{stderr}"
    );
    assert_structured_missing_path(&stderr, "missing-package", "file");
}

#[test]
fn reader_locale_accepts_direct_entry_file_input() {
    let source = write_single_file(
        "test-reader-locale-single-file",
        r#"
测试 "pass" {
    断言 1 ≡ 1
}

入口 {
  显示 "salve"
}
"#,
    );

    let (stdout, stderr, ok) = run_faber(&[
        "test",
        "--locale",
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
fn reader_locale_accepts_manifest_file_input() {
    let package = write_basic_package(
        "test-reader-locale-manifest",
        r#"
测试 "pass" {
    断言 1 ≡ 1
}

入口 {
  显示 "salve"
}
"#,
    );
    let manifest = package.join("faber.toml");

    let (stdout, stderr, ok) = run_faber(&[
        "test",
        "--locale",
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
fn reader_locale_missing_package_path_surfaces_missing_path_error() {
    let (stdout, stderr, ok) = run_faber(&["test", "--locale", "th-TH", "missing-package"]);

    assert!(!ok, "missing package path should fail");
    assert!(
        stdout.is_empty(),
        "missing package path should not write stdout: {stdout}"
    );
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: CLI stderr is the renderer contract for this integration test.
    assert!(
        stderr.contains("error[PKG001:package_path_missing]"),
        "expected structured missing path diagnostic, got:\n{stderr}"
    );
    assert!(
        stderr.contains("missing-package"),
        "structured missing path diagnostic should name the input, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("No such file or directory"),
        "missing package path should not leak the raw filesystem error, got:\n{stderr}"
    );
}

#[test]
fn reader_locale_rejects_existing_non_package_file() {
    let plain = write_plain_file("test-reader-locale-plain-file", "temp");

    let (stdout, stderr, ok) = run_faber(&[
        "test",
        "--locale",
        "th-TH",
        plain.to_str().expect("plain path"),
    ]);

    assert!(
        !ok,
        "test should reject existing non-package files for reader-locale input"
    );
    assert!(
        stdout.is_empty(),
        "rejected test should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "expected reader-locale shape rejection, got:\n{stderr}"
    );
}

#[test]
fn reader_locale_rejects_stdin_input() {
    let (stdout, stderr, ok) = run_faber(&["test", "--locale", "zh-Hans", "-"]);

    assert!(!ok, "test should reject stdin reader-locale input");
    assert!(
        stdout.is_empty(),
        "rejected test should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale zh-Hans requires a package path or .fab entry file"),
        "expected stdin reader-locale rejection, got:\n{stderr}"
    );
}

#[test]
fn check_reader_locale_rejects_forced_package_stdin_input() {
    let (stdout, stderr, ok) = run_faber(&["check", "--package", "--locale", "zh-Hans", "-"]);

    assert!(!ok, "check should reject stdin reader-locale package input");
    assert!(
        stdout.is_empty(),
        "rejected check should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale zh-Hans requires a package path or .fab entry file"),
        "expected stdin shape rejection, got:\n{stderr}"
    );
}

#[test]
fn check_reader_locale_accepts_direct_entry_file_input() {
    let source = reader_locale_example_root("zh-Hans").join("src/main.fab");

    let (stdout, stderr, ok) = run_faber(&[
        "check",
        "--locale",
        "zh-Hans",
        source.to_str().expect("utf8 source path"),
    ]);

    assert!(ok, "single-file check with reader locale failed:\n{stderr}");
    assert!(
        stdout.is_empty(),
        "successful check should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains(&format!("ok: {}", source.display())),
        "expected successful check marker, got:\n{stderr}"
    );
}

#[test]
fn check_reader_locale_accepts_manifest_file_input() {
    let manifest = reader_locale_example_root("zh-Hans").join("faber.toml");

    let (stdout, stderr, ok) = run_faber(&[
        "check",
        "--locale",
        "zh-Hans",
        manifest.to_str().expect("utf8 manifest path"),
    ]);

    assert!(ok, "manifest check with reader locale failed:\n{stderr}");
    assert!(
        stdout.is_empty(),
        "successful check should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains(&format!("ok: {}", manifest.display())),
        "expected successful check marker, got:\n{stderr}"
    );
}

#[test]
fn check_reader_locale_rejects_existing_non_package_file() {
    let plain = write_plain_file("check-reader-locale-plain-file", "temp");

    let (stdout, stderr, ok) = run_faber(&[
        "check",
        "--locale",
        "th-TH",
        plain.to_str().expect("plain path"),
    ]);

    assert!(
        !ok,
        "check should reject existing non-package files for reader-locale input"
    );
    assert!(
        stdout.is_empty(),
        "rejected check should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "expected reader-locale shape rejection, got:\n{stderr}"
    );
}

#[test]
fn check_reader_locale_rejects_multiple_direct_inputs() {
    let entry_a = write_single_file(
        "check-reader-locale-multi-a",
        "incipit { nota \"salve\" }\n",
    );
    let entry_b = write_single_file("check-reader-locale-multi-b", "incipit { nota \"vale\" }\n");

    let (stdout, stderr, ok) = run_faber(&[
        "check",
        "--locale",
        "th-TH",
        entry_a.to_str().expect("entry path"),
        entry_b.to_str().expect("entry path"),
    ]);

    assert!(
        !ok,
        "check should reject reader-locale when multiple direct inputs are supplied"
    );
    assert!(
        stdout.is_empty(),
        "rejected check should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "expected multiple-input reader-locale rejection, got:\n{stderr}"
    );
}

#[test]
fn check_reader_locale_missing_package_path_surfaces_missing_path_error() {
    let (stdout, stderr, ok) = run_faber(&["check", "--locale", "th-TH", "missing-package"]);

    assert!(!ok, "missing package path should fail");
    assert!(
        stdout.is_empty(),
        "missing package path should not write stdout: {stdout}"
    );
    assert!(
        !stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "missing package path should reach the underlying filesystem error, got:\n{stderr}"
    );
    assert_structured_missing_path(&stderr, "missing-package", "file");
}

#[test]
fn check_missing_directory_surfaces_structured_missing_path() {
    let (stdout, stderr, ok) = run_faber(&["check", "./no-such-dir"]);

    assert!(!ok, "missing directory should fail");
    assert!(
        stdout.is_empty(),
        "missing directory check should not write stdout: {stdout}"
    );
    assert_structured_missing_path(&stderr, "no-such-dir", "file");
}

#[test]
fn run_missing_source_file_surfaces_structured_missing_path() {
    let (stdout, stderr, ok) = run_faber(&["run", "no-such-file.fab"]);

    assert!(!ok, "missing source file should fail");
    assert!(
        stdout.is_empty(),
        "missing source run should not write stdout: {stdout}"
    );
    assert_structured_missing_path(&stderr, "no-such-file.fab", "source file");
}

#[test]
fn model_inspect_missing_file_surfaces_structured_missing_path() {
    let (stdout, stderr, ok) = run_faber(&["model", "inspect", "no-such-model.safetensors"]);

    assert!(!ok, "missing model file should fail");
    assert!(
        stdout.is_empty(),
        "missing model inspect should not write stdout: {stdout}"
    );
    assert_structured_missing_path(&stderr, "no-such-model.safetensors", "file");
}

#[test]
fn check_reader_locale_forced_package_rejects_existing_non_package_file() {
    let plain = write_plain_file("check-reader-locale-force-package", "temp");

    let (stdout, stderr, ok) = run_faber(&[
        "check",
        "--package",
        "--locale",
        "th-TH",
        plain.to_str().expect("plain path"),
    ]);

    assert!(
        !ok,
        "forced package reader-locale should reject existing non-package files"
    );
    assert!(
        stdout.is_empty(),
        "rejected check should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "expected reader-locale shape rejection, got:\n{stderr}"
    );
}

#[test]
fn build_reader_locale_forced_package_rejects_existing_non_package_file() {
    let plain = write_plain_file("build-reader-locale-force-package", "temp");

    let (stdout, stderr, ok) = run_faber(&[
        "build",
        "--package",
        "--locale",
        "th-TH",
        plain.to_str().expect("plain path"),
    ]);

    assert!(
        !ok,
        "forced package reader-locale should reject existing non-package files"
    );
    assert!(
        stdout.is_empty(),
        "rejected build should not write stdout: {stdout}"
    );
    assert!(
        stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "expected reader-locale shape rejection, got:\n{stderr}"
    );
}

fn write_zip_archive(label: &str, entries: &[(&str, &str)]) -> TempPath {
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
    TempPath::new(dir, archive)
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
            r#"
import from "./thing" thing

main {
  print thing.label()
}
"#
            .to_owned(),
        ),
        (
            format!("{root}src/thing.fab"),
            r"
@ public
fn label() → int {
  return 7
}
"
            .to_owned(),
        ),
    ]
}

fn write_numeric_manifest_archive(label: &str, prefix: &str) -> TempPath {
    let entries = numeric_manifest_entries(prefix);
    let borrowed = entries
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect::<Vec<_>>();
    write_zip_archive(label, &borrowed)
}

#[test]
fn run_private_namespace_failure_does_not_emit_rust() {
    let package = temp_dir("private-namespace");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./auxilium" Aux

main {
  print Aux.secretum()
}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("auxilium.fab"),
        r"
@ private
fn secretum() → int {
  return 1
}
",
    )
    .expect("write module");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        package
            .join("main.fab")
            .to_str()
            .expect("utf8 package path"),
    ]);

    assert!(!ok, "private namespace package should fail");
    assert_eq!(stdout, "");
    // Under VM-U2 a `@ private` member imported through a file-namespace
    // alias fails at the import seam (SEM006) instead of typecheck
    // namespace_missing_export (SEM004).
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("SEM006.import_module_private"),
        "expected private namespace diagnostic:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_import_cycle_failure_does_not_emit_rust() {
    let package = temp_dir("import-cycle");
    fs::write(
        package.join("main.fab"),
        r#"import from "./jobs" jobs

main {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"import from "./main" main

fn run() → void {}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_norma_library_import_executes_without_rust_emit() {
    // LIB-MIR: non-bridged `norma:*` source libraries link into the
    // interpreted script path (text.reverse runs). The script must execute
    // without emitting Rust artifacts.
    let package = temp_dir("norma-linked");
    fs::write(
        package.join("main.fab"),
        r#"
import from "norma:text" text

main {
  print text.reverse("roma")
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["run", package.to_str().expect("utf8 package path")]);

    assert!(ok, "norma library import should execute:\n{stderr}");
    assert_eq!(stdout, "amor\n");
    assert_no_generated_rust(&package);
}

#[test]
fn run_cli_root_package_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-root");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
main args args {
  print "cli root"
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["run", package.to_str().expect("utf8 package path")]);

    assert!(ok, "CLI root package interpretation failed:\n{stderr}");
    assert_eq!(stdout, "cli root\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_cli_root_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-operand");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_root_rest_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-rest-operand");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ operand string input
@ operand rest string files
main args args {
  print args.input
  let list<string> files ← args.files
  print files.length()
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_root_fixed_exit_uses_process_status_without_rust_emit() {
    let package = temp_dir("cli-fixed-exit");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
main args args exit 7 {
  print "done"
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, status) =
        run_faber_status(&["run", package.to_str().expect("utf8 package path")]);

    assert_eq!(status.code(), Some(7), "stderr:\n{stderr}");
    assert_eq!(stdout, "done\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI fixed exit package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_cli_root_flag_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-flag-option");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ option verbose short "v" long "verbose" type bool
main args args {
  print args.verbose
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        package.to_str().expect("utf8 package path"),
        "--",
        "--verbose",
    ]);

    assert!(
        ok,
        "CLI flag option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "true\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_cli_root_defaulted_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-defaulted-option");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ option name long "name" type string coalesce "worker"
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_optional_chain_captures_receiver_before_operands() {
    // OC-3: `xs?[idx()]` and `f?(arg())` evaluate and test the receiver first;
    // an operand that clears the receiver's place cannot change the result.
    let dir = temp_dir("optional-chain-receiver-first");
    let path = dir.join("main.fab");
    fs::write(
        &path,
        r#"+++
locale = "en"
+++
type Op = (int) → int

fn identity(int x) → int {
    return x
}

main {
    mut list<int> ∪ none xs ← [10]
    let () → int idx ← () → int ∴ do {
        xs ← null
        return 0
    }
    print xs?[idx()]

    mut Op ∪ none f ← identity
    let () → int arg ← () → int ∴ do {
        f ← null
        return 7
    }
    print f?(arg())
}
"#,
    )
    .expect("write source");

    let (stdout, stderr, ok) = run_faber(&["run", path.to_str().expect("utf8 path")]);

    assert!(ok, "optional chain receiver-first run failed:\n{stderr}");
    assert_eq!(stdout, "10\n7\n");
}

#[test]
fn run_cli_root_optional_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-optional-option");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ option name long "name" type string
main args args {
  print args.name coalesce "worker"
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_root_global_flag_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-global-flag-option");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ option verbose long "verbose" type bool global
main args args {
  print args.verbose
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        package.to_str().expect("utf8 package path"),
        "--",
        "--verbose",
    ]);

    assert!(
        ok,
        "CLI global flag option package interpretation failed:\n{stderr}"
    );
    assert_eq!(stdout, "true\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "CLI global option package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_cli_root_defaulted_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-defaulted-operand");
    fs::write(
        package.join("main.fab"),
        r#"
@ cli "tool"
@ operand int count coalesce 9
main args args {
  print args.count
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["run", package.to_str().expect("utf8 package path")]);

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
fn run_cli_mounted_command_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print "running"
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_mounted_global_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-global-option");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ option verbose long "verbose" type bool global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print args.verbose
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
    assert_eq!(stdout, "true\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "mounted CLI global option interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_cli_mounted_global_optional_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-global-optional-option");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ option name long "name" type string global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print args.name coalesce "worker"
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_mounted_global_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-global-operand");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ operand string tenant global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ command "run"
@ operand string name
fn run() args args {
  print "§:§"(args.tenant, args.name)
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_mounted_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-operand");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ command "run"
@ operand string name
fn run() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_mounted_numerus_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-numerus-operand");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ command "run"
@ operand int count
fn run() args args {
  print args.count
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_mounted_defaulted_option_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-defaulted-option");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ command "run"
@ option name long "name" type string coalesce "worker"
fn run() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_cli_mounted_alias_text_operand_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("cli-mounted-alias");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("jobs.fab"),
        r#"
@ command "run"
@ alias "start"
@ operand string name
fn run() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
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
fn run_archive_root_manifest_executes_without_cargo_or_rust_emit() {
    let archive = write_numeric_manifest_archive("archive-root-manifest", "");

    let (stdout, stderr, ok) = run_faber(&["run", archive.to_str().expect("utf8 archive path")]);

    assert!(ok, "archive package interpretation failed:\n{stderr}");
    assert_eq!(stdout, "7\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "archive interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(archive.parent().expect("archive parent"));
}

#[test]
fn run_archive_wrapped_package_executes_without_cargo_or_rust_emit() {
    let archive = write_numeric_manifest_archive("archive-wrapped-package", "pkg");

    let (stdout, stderr, ok) = run_faber(&["run", archive.to_str().expect("utf8 archive path")]);

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
fn run_archive_manifestless_root_executes_without_cargo_or_rust_emit() {
    let archive = write_zip_archive(
        "archive-manifestless-root",
        &[
            (
                "main.fab",
                r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
            ),
            (
                "thing.fab",
                r"
@ public
fn label() → int {
  return 7
}
",
            ),
        ],
    );

    let (stdout, stderr, ok) = run_faber(&["run", archive.to_str().expect("utf8 archive path")]);

    assert!(ok, "manifestless archive interpretation failed:\n{stderr}");
    assert_eq!(stdout, "7\n");
    assert_no_generated_rust(archive.parent().expect("archive parent"));
}

#[test]
fn run_archive_rejects_parent_traversal_entry() {
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

    let (stdout, stderr, ok) = run_faber(&["run", archive.to_str().expect("utf8 archive path")]);

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
fn run_archive_reports_missing_package_root() {
    let archive = write_zip_archive(
        "archive-missing-root",
        &[
            ("a/main.fab", "incipit { nota 1 }\n"),
            ("b/main.fab", "incipit { nota 2 }\n"),
        ],
    );

    let (stdout, stderr, ok) = run_faber(&["run", archive.to_str().expect("utf8 archive path")]);

    assert!(!ok, "ambiguous archive root should fail");
    assert_eq!(stdout, "");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("archive package root must contain `faber.toml`, `main.fab`, or one top-level package directory"),
        "expected archive root diagnostic:\n{stderr}"
    );
}

#[test]
fn run_archive_package_diagnostic_reports_archive_member_path() {
    let archive = write_zip_archive(
        "archive-private-diagnostic",
        &[
            (
                "main.fab",
                r#"
import from "./auxilium" Aux

main {
  print Aux.secretum()
}
"#,
            ),
            (
                "auxilium.fab",
                r"
@ private
fn secretum() → int {
  return 1
}
",
            ),
        ],
    );

    let (stdout, stderr, ok) = run_faber(&["run", archive.to_str().expect("utf8 archive path")]);

    assert!(!ok, "private archive package should fail");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("!/main.fab"),
        "expected archive member diagnostic path:\n{stderr}"
    );
    // Under VM-U2 a `@ private` member imported through a file-namespace
    // alias fails at the import seam (SEM006).
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("SEM006.import_module_private"),
        "expected private namespace diagnostic:\n{stderr}"
    );
    assert_no_generated_rust(archive.parent().expect("archive parent"));
}

#[test]
fn run_manifestless_entry_package_executes_without_cargo_or_rust_emit() {
    let package = temp_dir("manifestless-numeric");
    fs::write(
        package.join("main.fab"),
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        package.join("thing.fab"),
        r"
@ public
fn label() → int {
  return 7
}
",
    )
    .expect("write module");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        package
            .join("main.fab")
            .to_str()
            .expect("utf8 package path"),
    ]);

    assert!(ok, "faber run failed:\n{stderr}");
    assert_eq!(stdout, "7\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "interpreted package run must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_package_inputs_execute_without_cargo_or_rust_emit() {
    let package = write_numeric_package("numeric");
    let manifest = package.join("faber.toml");
    let entry = package.join("src/main.fab");

    for input in [&*package, manifest.as_path(), entry.as_path()] {
        let (stdout, stderr, ok) = run_faber(&["run", input.to_str().expect("utf8 package path")]);
        assert!(ok, "faber run failed:\n{stderr}");
        assert_eq!(stdout, "7\n");
        assert!(
            !stderr.contains("Compiling") && !stderr.contains("cargo"),
            "interpreted package run must not invoke Cargo:\n{stderr}"
        );
        assert_no_generated_rust(&package);
    }
}

#[test]
fn run_text_return_package_executes_without_cargo_or_rust_emit() {
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
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        src.join("thing.fab"),
        r#"
@ public
fn label() → string {
  return "ok"
}
"#,
    )
    .expect("write module");

    let (stdout, stderr, ok) = run_faber(&["run", package.to_str().expect("utf8 path")]);

    assert!(ok, "text-return package interpretation failed:\n{stderr}");
    assert_eq!(stdout, "ok\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "text-return package interpretation must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_manifestless_single_file_uses_single_source_runner() {
    let dir = temp_dir("single-file");
    let script = dir.join("script.fab");
    fs::write(&script, "main { print \"single\" }\n").expect("write script");

    let (stdout, stderr, ok) = run_faber(&["run", script.to_str().expect("utf8 path")]);

    assert!(ok, "single-file interpretation failed:\n{stderr}");
    assert_eq!(stdout, "single\n");
    assert_no_generated_rust(&dir);
}

#[test]
fn run_norma_fs_package_reads_file_through_ad_leaf() {
    // One `norma:*` import block; interpreted package execution runs Norma
    // through `ad` / the shared fs leaf. No runner kernel, no compiled-Rust
    // fallback.
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
            r#"import from "norma:fs" fileio

main {{
  print fileio.read_file<string>("{payload_str}")
}}
"#
        ),
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["run", package.to_str().expect("utf8 package path")]);

    assert!(ok, "norma:fs package through ad/leaf failed:\n{stderr}");
    assert_eq!(stdout, "bridge-ok\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "interpreted package bridge must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_norma_fs_missing_file_fails_closed() {
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
        r#"import from "norma:fs" fileio

main {
  fileio.read_file_async<string>("/nonexistent")
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["run", package.to_str().expect("utf8 package path")]);

    assert!(!ok, "missing-file norma:fs read should fail closed");
    assert_eq!(stdout, "");
    assert!(
        !stderr.contains("package MIR kernel bridge does not support"),
        "norma:fs must not go through the retired kernel bridge:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn missing_file_read_is_caught_on_the_runner() {
    let dir = temp_dir("missing-file-read-caught");
    let missing = dir.join("absent.txt");
    let source = r#"import from "norma:fs" fs
import from "norma:fault" fault

main {
    do {
        let string body ← fs.read_file<string>("__PATH__")
        print body
    }
    catch err {
        print "caught"
    }
}
"#
    .replace("__PATH__", missing.to_str().expect("utf8 path"));
    let source = write_single_file("missing-file-read-caught", &source);

    let (stdout, stderr, ok) = run_faber(&["run", source.to_str().expect("utf8 source path")]);

    assert!(
        ok,
        "missing-file read must be caught on the runner:\n{stderr}"
    );
    assert_eq!(stdout, "caught\n");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: CLI stderr is the renderer contract for this integration test.
    assert!(
        !stderr.contains("No such file"),
        "the catch arm must consume the IoFault, got:\n{stderr}"
    );
}

#[test]
fn uncaught_missing_file_read_prints_fault_code() {
    let dir = temp_dir("uncaught-missing-file-read-fault-code");
    let missing = dir.join("absent.txt");
    let source = r#"import from "norma:fs" fs

main {
    let string body ← fs.read_file<string>("__PATH__")
    print body
}
"#
    .replace("__PATH__", missing.to_str().expect("utf8 path"));
    let source = write_single_file("uncaught-missing-file-read-fault-code", &source);

    let (stdout, stderr, ok) = run_faber(&["run", source.to_str().expect("utf8 source path")]);

    assert!(
        !ok,
        "an uncaught missing-file read must end the run:\n{stderr}"
    );
    assert_eq!(stdout, "");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: CLI stderr is the renderer contract for this integration test.
    assert!(
        stderr.contains("ERR001"),
        "an uncaught compiler-managed fault must print its stable code, got:\n{stderr}"
    );
    assert!(
        stderr.contains(missing.to_str().expect("utf8 path")),
        "the fault message must keep the path payload, got:\n{stderr}"
    );
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: CLI stderr is the renderer contract for this integration test.
    assert!(
        !stderr.contains("No such file"),
        "platform prose must not cross into the fault message, got:\n{stderr}"
    );
}

#[test]
fn missing_file_read_matches_not_found_with_its_path() {
    let dir = temp_dir("missing-file-read-not-found-path");
    let missing = dir.join("absent.txt");
    let source = r#"import from "norma:fs" fs
import from "norma:fault" fault

main {
    do {
        let string body ← fs.read_file<string>("__PATH__")
        print body
    }
    catch err {
        match err {
            case NotFound let path {
                print path
            }
            default {
                print "other"
            }
        }
    }
}
"#
    .replace("__PATH__", missing.to_str().expect("utf8 path"));
    let source = write_single_file("missing-file-read-not-found-path", &source);

    let (stdout, stderr, ok) = run_faber(&["run", source.to_str().expect("utf8 source path")]);

    assert!(
        ok,
        "missing-file read must be caught on the runner:\n{stderr}"
    );
    assert_eq!(stdout, format!("{}\n", missing.display()));
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: CLI stderr is the renderer contract for this integration test.
    assert!(
        !stderr.contains("No such file"),
        "the NotFound arm must consume the IoFault, got:\n{stderr}"
    );
    assert!(!stdout.contains("other"));
}

#[test]
fn run_norma_process_argumenta_reads_host_argv() {
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
        r#"import from "norma:process" process

main args args {
  print process.argv()[0]
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        package.to_str().expect("utf8 package path"),
        "--",
        "first-arg",
    ]);

    assert!(ok, "norma:process bridge failed:\n{stderr}");
    assert_eq!(stdout, "first-arg\n");
    assert_no_generated_rust(&package);
}

fn run_faber_with_stdin(args: &[&str], stdin: &[u8]) -> (String, String, bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn faber run");
    {
        let mut input = child.stdin.take().expect("stdin");
        input.write_all(stdin).expect("write stdin");
    }

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

#[test]
fn run_norma_console_selective_imports_are_byte_exact() {
    // EOT-4R: interpreted lane executes the consolum contract through
    // selective imports. Captured stdout is byte-identical to the
    // concatenated payloads (write_partial, write_bytes, read_line).
    // The writes report on the error channel, so each closure declares its
    // own `⇥ fault.IoFault`; `read_line` is `string ∪ none` and is narrowed
    // at the call site.
    let package = temp_dir("norma-consolum-eot4r");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "norma-consolum-eot4r"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r#"import from "norma:console" const write_partial, write_bytes, read_line
import from "norma:fault" fault

main {
  let _ partial ← (string msg) → void ⇥ fault.IoFault ∴ write_partial(msg)
  let _ out_bytes ← (list<b8> data) → void ⇥ fault.IoFault ∴ write_bytes(data)
  partial("P")
  out_bytes(|41|)
  partial(read_line() coalesce "")
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber_with_stdin(
        &["run", package.to_str().expect("utf8 package path")],
        b"LINE\n",
    );

    assert!(
        ok,
        "norma:console interpreted selective import failed:\n{stderr}"
    );
    assert_eq!(stdout, "PALINE");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "interpreted package bridge must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_console_read_line_eof_is_none_and_blank_line_is_present() {
    // EOF is `none`; a blank line stays a present "" (not EOF). The loop
    // must end at end of input, not at a line cap: before the fix `read_line`
    // always returned a present string and the guard stopped it at 10.
    let package = temp_dir("norma-console-read-line-eof");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "norma-console-read-line-eof"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r#"import from "norma:console" const read_line

main {
    mut int lines ← 0
    mut bool done ← false
    while (not done) and lines ≺ 10 {
        let string ∪ none line ← read_line()
        if line is none {
            done ← true
        }
        if not done {
            lines ← lines + 1
        }
    }
    print lines
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber_with_stdin(
        &["run", package.to_str().expect("utf8 package path")],
        b"A\n\nB\n",
    );

    assert!(ok, "norma:console read_line EOF probe failed:\n{stderr}");
    assert_eq!(
        stdout.trim(),
        "3",
        "two text lines and one blank line are present; the loop ends on EOF"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_console_read_bytes_eof_is_none_and_zero_length_is_present() {
    // EOF is `none`; a zero-length read stays a present empty buffer. Before
    // the fix `read_bytes` always returned a present buffer, so the 4-byte
    // loop ran to its 10-read cap instead of ending after "abcdef" (4 + 2).
    let package = temp_dir("norma-console-read-bytes-eof");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "norma-console-read-bytes-eof"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r#"import from "norma:console" const read_bytes

main {
    mut int reads ← 0
    mut bool done ← false
    while (not done) and reads ≺ 10 {
        let list<b8> ∪ none chunk ← read_bytes(4)
        if chunk is none {
            done ← true
        }
        if not done {
            reads ← reads + 1
        }
    }
    print reads
    let list<b8> ∪ none zero ← read_bytes(0)
    mut bool absent ← false
    if zero is none {
        absent ← true
    }
    print absent
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber_with_stdin(
        &["run", package.to_str().expect("utf8 package path")],
        b"abcdef",
    );

    assert!(ok, "norma:console read_bytes EOF probe failed:\n{stderr}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["2", "false"],
        "two partial reads (4 + 2) precede EOF; a zero-length read is present"
    );
    assert_no_generated_rust(&package);
}

/// d11-6 G1 repair: a constant argument that fits a narrow parameter is a
/// valid MIR call argument (MIR types the constant as plain `numerus`).
#[test]
fn run_accepts_a_fitting_constant_argument_for_a_narrow_parameter() {
    let dir = temp_dir("run-narrow-constant-argument");
    let path = dir.join("narrow.fab");
    fs::write(
        &path,
        "fn f(u8 v) → u8 {\n    return v\n}\nmain {\n    print f(200)\n}\n",
    )
    .expect("write source");
    let path = path.to_str().expect("utf-8 path");
    let (stdout, stderr, ok) = run_faber(&["run", path]);
    assert!(ok, "run failed: {stderr}");
    assert_eq!(stdout.trim(), "200");
}

#[test]
fn run_panic_prints_abort_reason_to_stderr_and_exits_seventy() {
    // `faber run` surfaces a runner abort the way the runner's own
    // `StdioHost::abort` does: `abort: <message>` on stderr, exit 70 (the
    // program-logic class of the trap contract), with
    // stdout written before the trap still flushed.
    let file = write_single_file(
        "run-panic-abort",
        "main {\n    print \"before\"\n    panic \"boom\"\n}\n",
    );

    let (stdout, stderr, status) =
        run_faber_status(&["run", file.to_str().expect("utf8 source path")]);

    assert_eq!(status.code(), Some(70), "panic must exit 70:\n{stderr}");
    assert_eq!(stdout, "before\n");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: runtime abort reason is the stderr contract.
    assert!(
        stderr.contains("abort: boom"),
        "expected abort reason on stderr:\n{stderr}"
    );
}

#[test]
fn run_negative_integer_exponent_trap_prints_reason_to_stderr() {
    let file = write_single_file(
        "run-negative-exponent-abort",
        "main {\n    let int b ← 2\n    let int e ← -1\n    print \"before\"\n    print b.power(e)\n    print \"after\"\n}\n",
    );

    let (stdout, stderr, status) =
        run_faber_status(&["run", file.to_str().expect("utf8 source path")]);

    assert_eq!(status.code(), Some(70), "trap must exit 70:\n{stderr}");
    assert_eq!(stdout, "before\n");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: runtime abort reason is the stderr contract.
    assert!(
        stderr.contains("abort: numerus potentia failed: negative exponent"),
        "expected trap reason on stderr:\n{stderr}"
    );
}

/// Run a single-file program that traps and return its exit status and the
/// first stderr line. The statuses are the sysexits classes of the trap
/// contract (`radix::module::runtime_contract::trap`), not 1.
fn trap_exit(label: &str, source: &str) -> (Option<i32>, String) {
    let file = write_single_file(label, source);
    let (_stdout, stderr, status) =
        run_faber_status(&["run", file.to_str().expect("utf8 source path")]);
    let line = stderr.lines().next().unwrap_or_default().to_owned();
    (status.code(), line)
}

#[test]
fn run_conversion_failure_exits_sixty_five_with_one_error_line() {
    let (code, line) = trap_exit(
        "run-trap-conversion",
        "main {\n    print \"salve\" ↦ bool\n}\n",
    );
    assert_eq!(code, Some(65), "{line}");
    assert_eq!(line, "error: text to bivalens conversion failed");
}

#[test]
fn run_index_trap_exits_seventy_with_one_error_line() {
    let (code, line) = trap_exit(
        "run-trap-index",
        "main {\n    let string s ← \"ab\"\n    let int i ← 5\n    print s[i]\n}\n",
    );
    assert_eq!(code, Some(70), "{line}");
    assert_eq!(line, "error: text index out of bounds");
}

#[test]
fn run_unhandled_missing_file_exits_sixty_six() {
    let (code, line) = trap_exit(
        "run-trap-not-found",
        "import from \"norma:fs\" fs\nimport from \"norma:fault\" fault\nfn read_text(string path) → string ⇥ fault.IoFault {\n    return fs.read_file<string>(path)\n}\nmain {\n    print read_text(\"/nonexistent/faber-trap-contract\")\n}\n",
    );
    assert_eq!(code, Some(66), "{line}");
    assert!(line.starts_with("abort: ERR001 "), "{line}");
}

#[test]
fn run_explicit_process_exit_keeps_its_own_status() {
    let (code, _line) = trap_exit(
        "run-trap-exit-process",
        "import from \"norma:process\" process\nmain {\n    process.exit_process(3)\n}\n",
    );
    assert_eq!(code, Some(3));
}

const KERNEL_SOURCE: &str = r#"
@ kernel
@ public
fn bump(f32 x) → f32 {
    return x + 1.0
}

main {
    print bump(1.0)
}
"#;

/// The default device route over a kernel program is host-dependent: a host
/// with an admitted device (Metal) runs the kernel and must print exactly the
/// CPU device's result; a host without one refuses with exit 1 and prints no
/// result. It never prints a different result.
fn assert_default_route_matches_cpu_or_refuses(stdout: &str, stderr: &str, code: Option<i32>) {
    match code {
        Some(0) => {
            // The device route announces the selected backend ahead of the
            // program output.
            let program: Vec<&str> = stdout
                .lines()
                .filter(|line| !line.starts_with("device: "))
                .collect();
            assert_eq!(
                program,
                ["2.0"],
                "the default route must equal the CPU result"
            );
        }
        Some(1) => assert!(
            !stdout.contains("2.0"),
            "a refused default route prints no kernel result:\n{stdout}\n{stderr}"
        ),
        other => panic!("default route exit {other:?}:\n{stdout}\n{stderr}"),
    }
}

#[test]
fn run_kernel_file_default_route_matches_reference_or_refuses_and_runs_on_the_reference_route() {
    let file = write_single_file("run-kernel-cpu-device", KERNEL_SOURCE);
    let path = file.to_str().expect("utf8 source path");

    // Default route: `auto` over a kernel package takes the device session and
    // never the CPU: a host with an admitted device runs the kernel there (the
    // same result), a host without one refuses.
    let (stdout, stderr, status) = run_faber_status(&["run", path]);
    assert_default_route_matches_cpu_or_refuses(&stdout, &stderr, status.code());
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "auto", path]);
    assert_default_route_matches_cpu_or_refuses(&stdout, &stderr, status.code());

    // Reference route: the kernel runs with its sequential meaning.
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "reference", path]);
    assert!(
        status.success(),
        "`--device reference` must run the kernel:\n{stderr}"
    );
    assert_eq!(stdout, "2.0\n");
}

#[test]
fn run_kernel_package_default_route_and_reference_route_by_flag_manifest_and_with_stats() {
    let package = write_basic_package("run-kernel-package-cpu-device", KERNEL_SOURCE);
    let path = package.to_str().expect("utf8 package path");

    let (stdout, stderr, status) = run_faber_status(&["run", path]);
    assert_default_route_matches_cpu_or_refuses(&stdout, &stderr, status.code());

    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "reference", path]);
    assert!(
        status.success(),
        "`--device reference` must run the package kernel:\n{stderr}"
    );
    assert_eq!(stdout, "2.0\n");

    // `--device reference --stats` works like the default route: the program output
    // and the framed statistics artifact both arrive.
    let (stdout, stderr, status) =
        run_faber_status(&["run", "--device", "reference", "--stats", path]);
    assert!(
        status.success(),
        "`--device reference --stats` must run:\n{stderr}"
    );
    assert!(
        stdout.starts_with("2.0\n"),
        "program output first:\n{stdout}"
    );
    assert!(
        stdout.contains("--- faber-run-stats ---"),
        "stats artifact framed on stdout:\n{stdout}"
    );

    // The manifest selects the reference route; an explicit flag still wins.
    let manifest = fs::read_to_string(package.join("faber.toml")).expect("read manifest");
    fs::write(
        package.join("faber.toml"),
        format!("{manifest}\n[device]\ndevice = \"reference\"\n"),
    )
    .expect("write manifest with the reference route");
    let (stdout, stderr, status) = run_faber_status(&["run", path]);
    assert!(
        status.success(),
        "a manifest `reference` device must run the kernel:\n{stderr}"
    );
    assert_eq!(stdout, "2.0\n");
    // `--device auto` overrides the manifest's `reference`: the device route, so
    // it matches the reference result or refuses, never a different result.
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "auto", path]);
    assert_default_route_matches_cpu_or_refuses(&stdout, &stderr, status.code());
}

/// The CPU device line every device session prints ahead of the program.
const CPU_DEVICE_RECEIPT_PREFIX: &str = "device: selected backend `cpu` on ";

/// `--device cpu` (gpu-reset rule 16.9): the A2 flood stencil's lane-grid and
/// single-lane kernels run through the CPU device session on one worker and
/// on eight, and each run prints exactly what the sequential reference route
/// prints, after the device receipt line.
#[test]
fn run_cpu_device_a2_stencil_matches_the_reference_at_one_and_eight_workers() {
    let path = repo_root().join("acceptance/a2-flood-stencil");
    let path = path.display().to_string();
    let (reference, stderr, ok) = run_faber(&["run", "--device", "reference", &path]);
    assert!(ok, "reference run failed:\n{stderr}");
    assert!(
        !reference.is_empty(),
        "the reference run prints the depth plane"
    );
    for threads in ["1", "8"] {
        let (stdout, stderr, ok) =
            run_faber(&["run", "--device", "cpu", "--threads", threads, &path]);
        assert!(ok, "--threads {threads}: cpu device run failed:\n{stderr}");
        let (receipt, program) = stdout
            .split_once('\n')
            .unwrap_or_else(|| panic!("--threads {threads}: no output:\n{stdout}"));
        assert!(
            receipt.starts_with(CPU_DEVICE_RECEIPT_PREFIX),
            "--threads {threads}: the cpu device session runs the kernels:\n{stdout}"
        );
        assert_eq!(
            program, reference,
            "--threads {threads}: cpu device output differs from the reference"
        );
    }
}

/// `--device cpu --partitions N` (gpu-reset rule 18, rule 16.3): the one
/// logical CPU device spans N core sets that split the workers, and the A2
/// flood stencil prints exactly what the reference route prints at one
/// partition and at three; the spanned receipt names the partition count.
#[test]
fn run_cpu_device_a2_stencil_matches_the_reference_at_one_and_three_partitions() {
    let path = repo_root().join("acceptance/a2-flood-stencil");
    let path = path.display().to_string();
    let (reference, stderr, ok) = run_faber(&["run", "--device", "reference", &path]);
    assert!(ok, "reference run failed:\n{stderr}");
    for (partitions, extra) in [("1", &[][..]), ("3", &["--threads", "6"][..])] {
        let mut args = vec!["run", "--device", "cpu", "--partitions", partitions];
        args.extend_from_slice(extra);
        args.push(&path);
        let (stdout, stderr, ok) = run_faber(&args);
        assert!(
            ok,
            "--partitions {partitions}: cpu device run failed:\n{stderr}"
        );
        let (receipt, program) = stdout
            .split_once('\n')
            .unwrap_or_else(|| panic!("--partitions {partitions}: no output:\n{stdout}"));
        assert!(
            receipt.starts_with(CPU_DEVICE_RECEIPT_PREFIX),
            "--partitions {partitions}: the cpu device session runs the kernels:\n{stdout}"
        );
        assert_eq!(
            receipt.contains(", 3 partitions)"),
            partitions == "3",
            "--partitions {partitions}: the receipt names a spanned device only:\n{receipt}"
        );
        assert_eq!(
            program, reference,
            "--partitions {partitions}: cpu device output differs from the reference"
        );
    }
}

/// The Metal device line every Metal device session prints ahead of the
/// program.
#[cfg(target_os = "macos")]
const METAL_DEVICE_RECEIPT_PREFIX: &str = "device: selected backend `metal` on ";

#[cfg(target_os = "macos")]
fn run_metal_with_partition_memory(
    package: &Path,
    args: &[&str],
    partitions: u32,
    metal_span: faber_cli::package::mir::MetalSpan,
) -> (String, Vec<String>, Vec<String>) {
    use faber_cli::package::mir::{
        CpuDeviceShape, run_package_on_device_session_shaped, system_device_registry_spanning,
    };
    use radix::module::driver::Config;
    use radix::program::mir::DeviceSelection;

    let mut registry = system_device_registry_spanning(metal_span, 1);
    let mut host = radix_mir_runner::BufferHost::with_argumenta(
        args.iter().map(|arg| (*arg).to_owned()).collect(),
    );
    let shape = CpuDeviceShape {
        workers: None,
        partitions: std::num::NonZeroU32::new(partitions).expect("positive partition count"),
    };
    let run = run_package_on_device_session_shaped(
        &Config::default(),
        package,
        DeviceSelection::Metal,
        &mut registry,
        &mut host,
        false,
        shape,
    )
    .unwrap_or_else(|failure| {
        panic!(
            "{metal_span:?}/Metal/{partitions} device run failed: {:?}",
            failure.diagnostics
        )
    });
    (run.receipt, host.stdout_lines, host.stderr_lines)
}

/// `--device metal --partitions N` (gpu-reset rule 18): one logical Metal
/// device spans N partitions of the one GPU. The CLI selects shared storage;
/// the Rust API also exercises the private-copy test mode.
/// The A2 flood stencil (lane-local stencil kernels split by lane range, a
/// single-lane kernel replicated) prints exactly what plain `--device metal`
/// prints at one partition and at three; private copies produce the same full
/// program output. The spanned receipt names the partition count and no kernel
/// falls back to one partition. A macOS host without Metal fails this test.
#[cfg(target_os = "macos")]
#[test]
fn run_metal_device_a2_stencil_matches_plain_metal_at_one_and_three_partitions() {
    let package = repo_root().join("acceptance/a2-flood-stencil");
    let path = package.display().to_string();
    let (plain, stderr, ok) = run_faber(&["run", "--device", "metal", &path]);
    assert!(ok, "plain metal run failed:\n{stderr}");
    let (plain_receipt, plain_program) = plain
        .split_once('\n')
        .unwrap_or_else(|| panic!("plain metal run printed nothing:\n{plain}"));
    assert!(
        plain_receipt.starts_with(METAL_DEVICE_RECEIPT_PREFIX),
        "{plain}"
    );
    assert!(
        !plain_program.is_empty(),
        "the stencil prints its depth plane"
    );
    for partitions in ["1", "3"] {
        let (stdout, stderr, ok) = run_faber(&[
            "run",
            "--device",
            "metal",
            "--partitions",
            partitions,
            &path,
        ]);
        assert!(ok, "--partitions {partitions}: metal run failed:\n{stderr}");
        let (receipt, program) = stdout
            .split_once('\n')
            .unwrap_or_else(|| panic!("--partitions {partitions}: no output:\n{stdout}"));
        assert!(
            receipt.starts_with(METAL_DEVICE_RECEIPT_PREFIX),
            "--partitions {partitions}: the metal device session runs the kernels:\n{stdout}"
        );
        assert_eq!(
            receipt.contains(", 3 partitions)"),
            partitions == "3",
            "--partitions {partitions}: the receipt names a spanned device only:\n{receipt}"
        );
        assert_eq!(
            program, plain_program,
            "--partitions {partitions}: spanned output differs from plain metal"
        );
        assert!(
            !stderr.contains("runs on one partition"),
            "--partitions {partitions}: every stencil kernel splits or replicates:\n{stderr}"
        );
    }

    let (receipt, private_program, private_stderr) = run_metal_with_partition_memory(
        &package,
        &[],
        3,
        faber_cli::package::mir::MetalSpan::PrivateCopies,
    );
    assert!(
        receipt.starts_with(METAL_DEVICE_RECEIPT_PREFIX),
        "{receipt}"
    );
    assert!(
        receipt.contains(", 3 partitions)"),
        "the private-copy receipt names the spanned device: {receipt}"
    );
    assert!(
        private_stderr.is_empty(),
        "private-copy A2 run emitted diagnostics: {private_stderr:?}"
    );
    assert_eq!(
        private_program,
        plain_program.lines().map(str::to_owned).collect::<Vec<_>>(),
        "private-copy A2 output differs from plain Metal"
    );
}

/// Rule 18: the whole-grid reductions of `c4-grid-reduce` (stage 1 split by
/// workgroup range, its partials all-gathered, stage 2 replicated) print
/// exactly what plain `--device metal` prints, at a shape with more
/// workgroups than partitions and at one with a single workgroup, and no
/// reduction falls back to one partition. The Rust API also compares the
/// private-copy mode against plain Metal. A macOS host without Metal fails
/// this test.
#[cfg(target_os = "macos")]
#[test]
fn run_metal_device_grid_reduce_matches_plain_metal_over_partitions() {
    let package = repo_root().join("acceptance/c4-grid-reduce");
    let path = package.display().to_string();
    for shape in [["600", "500"], ["3", "5"]] {
        let mut plain_args = vec!["run", "--device", "metal", &path, "--"];
        plain_args.extend(shape);
        let (plain, stderr, ok) = run_faber(&plain_args);
        assert!(ok, "plain metal run failed:\n{stderr}");
        let (_, plain_program) = plain
            .split_once('\n')
            .unwrap_or_else(|| panic!("plain metal run printed nothing:\n{plain}"));
        for partitions in ["1", "3", "4"] {
            let mut args = vec![
                "run",
                "--device",
                "metal",
                "--partitions",
                partitions,
                &path,
                "--",
            ];
            args.extend(shape);
            let (stdout, stderr, ok) = run_faber(&args);
            assert!(
                ok,
                "{shape:?} --partitions {partitions}: metal run failed:\n{stderr}"
            );
            let (_, program) = stdout.split_once('\n').unwrap_or_else(|| {
                panic!("{shape:?} --partitions {partitions}: no output:\n{stdout}")
            });
            assert_eq!(
                program, plain_program,
                "{shape:?} --partitions {partitions}: spanned output differs from plain metal"
            );
            assert!(
                !stderr.contains("runs on one partition"),
                "{shape:?} --partitions {partitions}: every reduction spans:\n{stderr}"
            );
        }

        for partitions in ["3", "4"] {
            let (receipt, private_program, private_stderr) = run_metal_with_partition_memory(
                &package,
                &shape,
                partitions.parse().expect("literal partition count"),
                faber_cli::package::mir::MetalSpan::PrivateCopies,
            );
            assert!(
                receipt.starts_with(METAL_DEVICE_RECEIPT_PREFIX),
                "{shape:?} private-copy route: {receipt}"
            );
            assert!(
                receipt.contains(&format!(", {partitions} partitions)")),
                "{shape:?} private-copy receipt names its span: {receipt}"
            );
            assert!(
                private_stderr.is_empty(),
                "{shape:?} private-copy run emitted diagnostics: {private_stderr:?}"
            );
            assert_eq!(
                private_program,
                plain_program.lines().map(str::to_owned).collect::<Vec<_>>(),
                "{shape:?} private-copy grid reduction differs from plain Metal"
            );
        }
    }
}

/// `--partitions` belongs to the CPU, Metal and CUDA devices: any other route fails
/// closed before launch instead of ignoring the flag.
#[test]
fn run_partitions_outside_the_cpu_device_fails_closed() {
    let path = repo_root().join("acceptance/a2-flood-stencil");
    let path = path.display().to_string();
    let (_, stderr, ok) = run_faber(&["run", "--device", "reference", "--partitions", "2", &path]);
    assert!(!ok, "--partitions with the reference route must fail");
    assert!(
        stderr.contains("E_PARTITIONS_REQUIRES_CPU"),
        "the refusal names its code:\n{stderr}"
    );
}

/// `--device cpu` runs whole-grid reductions (sum, max, argmax, argmin) as one
/// sequential fold of the reduction node and prints what the reference route
/// prints.
#[test]
fn run_cpu_device_grid_reductions_match_the_reference() {
    for (package, args) in [
        ("acceptance/c4-grid-reduce", &["130", "3"][..]),
        ("acceptance/c4-reduce-int", &["4096"][..]),
    ] {
        let path = repo_root().join(package).display().to_string();
        let mut reference_args = vec!["run", "--device", "reference", &path, "--"];
        reference_args.extend_from_slice(args);
        let (reference, stderr, ok) = run_faber(&reference_args);
        assert!(ok, "{package}: reference run failed:\n{stderr}");
        let mut cpu_args = vec!["run", "--device", "cpu", "--threads", "4", &path, "--"];
        cpu_args.extend_from_slice(args);
        let (stdout, stderr, ok) = run_faber(&cpu_args);
        assert!(ok, "{package}: cpu device run failed:\n{stderr}");
        let (receipt, program) = stdout
            .split_once('\n')
            .unwrap_or_else(|| panic!("{package}: no output:\n{stdout}"));
        assert!(
            receipt.starts_with(CPU_DEVICE_RECEIPT_PREFIX),
            "{package}: the cpu device session runs the kernels:\n{stdout}"
        );
        assert_eq!(program, reference, "{package}: cpu device output differs");
    }
}

/// The kernel-bearing proba package of the CPU device (gpu-reset rule 16.9):
/// its manifest sets `[device] threads = 2`.
fn cpu_proba_kernels_package() -> String {
    repo_root()
        .join("acceptance/cpu-proba-kernels")
        .display()
        .to_string()
}

/// A copy of the CPU proba package whose `[device] threads` line is
/// `threads_line`.
fn cpu_proba_kernels_copy(label: &str, threads_line: &str) -> TempDir {
    let source = repo_root().join("acceptance/cpu-proba-kernels");
    let package = temp_dir(label);
    fs::create_dir_all(package.join("src")).expect("create src");
    for file in ["src/main.fab", "src/main.proba"] {
        fs::copy(source.join(file), package.join(file)).expect("copy source");
    }
    let manifest = fs::read_to_string(source.join("faber.toml")).expect("read manifest");
    assert!(manifest.contains("threads = 2\n"), "{manifest}");
    fs::write(
        package.join("faber.toml"),
        manifest.replace("threads = 2\n", &format!("{threads_line}\n")),
    )
    .expect("write manifest");
    package
}

/// `[device] threads` (gpu-reset rule 16.9): the manifest count drives `faber
/// run --device cpu`, the `--threads` flag wins over it, the result is what
/// the reference route prints, and `--device reference` ignores the key.
#[test]
fn run_cpu_device_manifest_threads_run_and_match_the_reference() {
    let path = cpu_proba_kernels_package();
    let (reference, stderr, ok) = run_faber(&["run", "--device", "reference", &path]);
    assert!(
        ok,
        "the reference route ignores `[device] threads`:\n{stderr}"
    );
    assert_eq!(reference, "1001000.0\n");
    for flags in [
        &["--device", "cpu"][..],
        &["--device", "cpu", "--threads", "3"][..],
    ] {
        let mut args = vec!["run"];
        args.extend_from_slice(flags);
        args.push(&path);
        let (stdout, stderr, ok) = run_faber(&args);
        assert!(ok, "{flags:?}: cpu device run failed:\n{stderr}");
        let (receipt, program) = stdout
            .split_once('\n')
            .unwrap_or_else(|| panic!("{flags:?}: no output:\n{stdout}"));
        assert!(
            receipt.starts_with(CPU_DEVICE_RECEIPT_PREFIX),
            "{flags:?}: the cpu device session runs the kernel:\n{stdout}"
        );
        assert_eq!(program, reference, "{flags:?}: cpu output differs");
    }
}

/// An invalid `[device] threads` fails closed with a diagnostic naming the
/// key, before any program output.
#[test]
fn run_cpu_device_invalid_manifest_threads_fail_closed() {
    for (label, line) in [
        ("threads-zero", "threads = 0"),
        ("threads-negative", "threads = -4"),
        ("threads-fraction", "threads = 1.5"),
    ] {
        let package = cpu_proba_kernels_copy(label, line);
        let path = package.to_str().expect("utf8 package path");
        let (stdout, stderr, status) = run_faber_status(&["run", "--device", "cpu", path]);
        assert!(!status.success(), "{line}: must fail:\n{stdout}");
        assert!(
            stderr.contains("device.threads must be a positive integer"),
            "{line}: the diagnostic names the key:\n{stderr}"
        );
        assert!(stdout.is_empty(), "{line}: nothing runs:\n{stdout}");
    }
    let package = cpu_proba_kernels_copy("threads-metal", "threads = 2\ndevice = \"metal\"");
    let path = package.to_str().expect("utf8 package path");
    let (_, stderr, status) = run_faber_status(&["run", "--device", "cpu", path]);
    assert!(!status.success(), "threads beside metal must fail");
    assert!(
        stderr.contains("device.threads applies only to the cpu device"),
        "{stderr}"
    );
}

/// Drop each case's wall time (`(12 ms)`) from a `faber test` report so two
/// routes' reports compare by verdicts and captured output.
fn without_case_times(report: &str) -> String {
    report
        .lines()
        .map(|line| match line.rfind(" (") {
            Some(at) if line.ends_with(" ms)") => &line[..at],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `faber test --device cpu` (gpu-reset rule 16.9) runs the package's kernels
/// on the CPU device session, with the manifest worker count or `--threads`,
/// and reports exactly what `--device reference` reports. The kernel is
/// dispatched, never run on the MIR runner: the runner's statistics carry a
/// `twice` row on the reference route and none on the device.
#[test]
fn test_cpu_device_runs_proba_kernels_and_matches_the_reference() {
    let path = cpu_proba_kernels_package();
    let (reference, stderr, ok) =
        run_faber(&["test", "--device", "reference", "--nocapture", &path]);
    assert!(ok, "reference proba run failed:\n{stderr}");
    assert!(
        reference.contains("3 passed; 0 failed"),
        "every case passes on the reference route:\n{reference}"
    );
    for flags in [
        &["--device", "cpu"][..],
        &["--device", "cpu", "--threads", "3"][..],
    ] {
        let mut args = vec!["test"];
        args.extend_from_slice(flags);
        args.extend_from_slice(&["--nocapture", &path]);
        let (stdout, stderr, ok) = run_faber(&args);
        assert!(ok, "{flags:?}: cpu proba run failed:\n{stderr}");
        assert_eq!(
            without_case_times(&stdout),
            without_case_times(&reference),
            "{flags:?}: the cpu report differs from the reference"
        );
    }

    let kernel_row = "\"module\":\"root\",\"name\":\"twice\"";
    let (stdout, stderr, ok) = run_faber(&["test", "--device", "reference", "--stats", &path]);
    assert!(ok, "reference stats run failed:\n{stderr}");
    assert!(
        stdout.contains(kernel_row),
        "the reference route runs the kernel on the runner:\n{stdout}"
    );
    let (stdout, stderr, ok) = run_faber(&["test", "--device", "cpu", "--stats", &path]);
    assert!(ok, "cpu stats run failed:\n{stderr}");
    assert!(stdout.contains("\"passed\":3"), "{stdout}");
    assert!(
        !stdout.contains(kernel_row),
        "the cpu device dispatches the kernel instead of running it:\n{stdout}"
    );
}

/// `faber test --threads N` takes the `faber run` contract: valid only with
/// the CPU device.
#[test]
fn test_threads_requires_the_cpu_device() {
    let path = cpu_proba_kernels_package();
    let (stdout, stderr, status) =
        run_faber_status(&["test", "--device", "reference", "--threads", "2", &path]);
    assert!(!status.success(), "must fail:\n{stdout}");
    assert!(
        stderr.contains("`--threads` is valid only with `--device cpu`"),
        "{stderr}"
    );
}

/// Run `package` with `args` on `--device reference` and on `--device cpu` at
/// one and eight workers; the cpu run must succeed and print, after its
/// receipt line, exactly what the reference prints.
fn assert_cpu_device_matches_reference(package: &str, args: &[&str]) {
    let path = repo_root().join(package).display().to_string();
    let mut reference_args = vec!["run", "--device", "reference", &path, "--"];
    reference_args.extend_from_slice(args);
    let (reference, stderr, ok) = run_faber(&reference_args);
    assert!(ok, "{package} {args:?}: reference run failed:\n{stderr}");
    for threads in ["1", "8"] {
        let mut cpu_args = vec!["run", "--device", "cpu", "--threads", threads, &path, "--"];
        cpu_args.extend_from_slice(args);
        let (stdout, stderr, ok) = run_faber(&cpu_args);
        assert!(
            ok,
            "{package} {args:?} --threads {threads}: cpu device run failed:\n{stderr}"
        );
        let (receipt, program) = stdout
            .split_once('\n')
            .unwrap_or_else(|| panic!("{package} --threads {threads}: no output:\n{stdout}"));
        assert!(
            receipt.starts_with(CPU_DEVICE_RECEIPT_PREFIX),
            "{package} --threads {threads}: the cpu device session runs the kernels:\n{stdout}"
        );
        assert_eq!(
            program, reference,
            "{package} {args:?} --threads {threads}: cpu device output differs from the reference"
        );
    }
}

/// `--device cpu` runs matmul kernels, whose GPU ABI is the backend's tiled
/// form, as one whole call of the kernel body. `a1-rungs` holds the assign
/// form of a GEMV (`o ⇇ w · x` rebinds the `mut` parameter) beside a returned
/// result and lane kernels; it prints what the reference prints.
#[test]
fn run_cpu_device_matmul_kernels_match_the_reference() {
    assert_cpu_device_matches_reference("acceptance/a1-rungs", &[]);
}

/// A kernel error the program catches is delivered as a device fault: the
/// handler prints the text of the lowest throwing lane, as the reference does,
/// whatever the worker count. Mode 1 is `do all ... catch`, mode 2 a plain
/// `do ... catch`, modes 5 and 6 the checked `do device ... catch` region
/// (faults run to the block exit and reach the handler once; mode 6 is the
/// clean run); at [9, 6] the lower lanes throw a different site than the
/// last row, so the delivered text names the first lane in launch order.
#[test]
fn run_cpu_device_caught_kernel_errors_match_the_reference() {
    for mode in ["1", "2"] {
        assert_cpu_device_matches_reference("acceptance/c4-kernel-throw", &["9", "6", mode]);
        assert_cpu_device_matches_reference("acceptance/c4-kernel-get-set", &["7", "5", mode]);
    }
    for mode in ["5", "6"] {
        assert_cpu_device_matches_reference("acceptance/c4-kernel-throw", &["9", "6", mode]);
    }
}

/// The `error:` lines of a failed run's stderr.
fn error_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("error:"))
        .collect()
}

/// An unhandled kernel fault ends the run once, with the stable code first and
/// the faulting lane in the text; the wording after the code is only the
/// rendered diagnostic. The one-lane kernel reads past its row at `1 0 0`.
#[test]
fn run_cpu_device_kernel_fault_prints_one_coded_error() {
    let path = repo_root()
        .join("acceptance/c4-single-lane-index-fault")
        .display()
        .to_string();
    let (stdout, stderr, status) = run_faber_status(&[
        "run",
        "--device",
        "cpu",
        "--threads",
        "4",
        &path,
        "--",
        "1",
        "0",
        "0",
    ]);
    assert_eq!(status.code(), Some(70), "{stderr}");
    assert!(stdout.starts_with(CPU_DEVICE_RECEIPT_PREFIX), "{stdout}");
    let errors = error_lines(&stderr);
    assert_eq!(errors.len(), 1, "one error line:\n{stderr}");
    assert!(
        errors[0].starts_with("error: E_DEVICE_KERNEL_FAULT:"),
        "{stderr}"
    );
    assert!(errors[0].contains("lane [0, 0, 0]"), "{stderr}");
}

/// The same fatal fault inside the checked region `do device { } catch`: the
/// handler does not see an index fault, the faults run to the block exit and
/// the run ends once with the stable code, as `do all` does. The in-range run
/// (`0 0 3`) completes cleanly.
#[test]
fn run_cpu_device_region_fault_ends_the_run_once() {
    let path = repo_root()
        .join("acceptance/c4-single-lane-index-fault")
        .display()
        .to_string();
    let (stdout, stderr, status) = run_faber_status(&[
        "run",
        "--device",
        "cpu",
        "--threads",
        "4",
        &path,
        "--",
        "1",
        "0",
        "3",
    ]);
    assert_eq!(status.code(), Some(70), "{stderr}");
    assert!(stdout.starts_with(CPU_DEVICE_RECEIPT_PREFIX), "{stdout}");
    let errors = error_lines(&stderr);
    assert_eq!(errors.len(), 1, "one error line:\n{stderr}");
    assert!(
        errors[0].starts_with("error: E_DEVICE_KERNEL_FAULT:"),
        "{stderr}"
    );
    assert!(errors[0].contains("lane [0, 0, 0]"), "{stderr}");

    let (stdout, stderr, status) = run_faber_status(&[
        "run",
        "--device",
        "cpu",
        "--threads",
        "4",
        &path,
        "--",
        "0",
        "0",
        "3",
    ]);
    assert_eq!(status.code(), Some(0), "{stderr}");
    assert!(stdout.ends_with("done\n"), "{stdout}");
}

const SINGLE_LANE_INDEX_FAULT: &str = "acceptance/c4-single-lane-index-fault";
/// Fault cases from the studio report, including read and all three write modes.
const SINGLE_LANE_KERNEL_FAULT_CASES: [(&str, &str, &str); 4] = [
    ("1", "0", "0"),
    ("0", "5", "0"),
    ("0", "5", "1"),
    ("0", "5", "2"),
];

/// Run each acceptance fault on a device and reference, pinning the shared
/// kernel-fault class to EX_SOFTWARE (70). Returns false if the first device
/// run could not be admitted.
fn assert_single_lane_device_fault_classes_match_reference(
    device: &str,
    receipt_prefix: &str,
) -> bool {
    let path = repo_root()
        .join(SINGLE_LANE_INDEX_FAULT)
        .display()
        .to_string();
    for (index, &(read, write, mode)) in SINGLE_LANE_KERNEL_FAULT_CASES.iter().enumerate() {
        let args = ["run", "--device", device, &path, "--", read, write, mode];
        let (stdout, stderr, status) = run_faber_status(&args);
        if !stdout.starts_with(receipt_prefix) {
            assert_eq!(
                index, 0,
                "{device} disappeared between fault cases:\n{stdout}\n{stderr}"
            );
            return false;
        }
        assert_eq!(
            status.code(),
            Some(70),
            "{device} {read} {write} {mode}: {stderr}"
        );
        let errors = error_lines(&stderr);
        assert_eq!(errors.len(), 1, "one {device} error line:\n{stderr}");
        assert!(
            errors[0].starts_with("error: E_DEVICE_KERNEL_FAULT:"),
            "{device}: {stderr}"
        );
        assert!(errors[0].contains("(lane 0)"), "{device}: {stderr}");

        let reference_args = [
            "run",
            "--device",
            "reference",
            &path,
            "--",
            read,
            write,
            mode,
        ];
        let (_reference_stdout, reference_stderr, reference_status) =
            run_faber_status(&reference_args);
        assert_eq!(
            reference_status.code(),
            Some(70),
            "reference {read} {write} {mode}: {reference_stderr}"
        );
        assert_eq!(
            status.code(),
            reference_status.code(),
            "{device} vs reference"
        );
    }
    true
}

/// [`run_cpu_device_kernel_fault_prints_one_coded_error`] on Metal. The fault
/// cases must match reference's sysexits class; this test skips without Metal.
#[test]
fn run_metal_device_kernel_fault_prints_one_coded_error() {
    if !cfg!(target_os = "macos")
        || !assert_single_lane_device_fault_classes_match_reference(
            "metal",
            METAL_DEVICE_RECEIPT_PREFIX,
        )
    {
        eprintln!("no Metal device: skipped");
    }
}

/// CUDA gets the same fault-class proof on hosts with an admitted device.
#[test]
fn run_cuda_device_kernel_fault_exits_with_the_reference_class() {
    if !assert_single_lane_device_fault_classes_match_reference("cuda", CUDA_DEVICE_RECEIPT_PREFIX)
    {
        assert!(
            std::env::var_os("FABER_REQUIRE_CUDA").is_none(),
            "FABER_REQUIRE_CUDA is set but the fault package did not run on CUDA"
        );
        eprintln!("no CUDA device: skipped");
    }
}

/// A device run that faults after it started executing still writes its
/// `--stats-file`, with exit 70 in the artifact, the device section present,
/// and the one coded error on stderr.
fn assert_faulting_device_run_writes_stats(device: &str, receipt_prefix: &str) {
    let path = repo_root()
        .join(SINGLE_LANE_INDEX_FAULT)
        .display()
        .to_string();
    let dir = temp_dir(&format!("fault-stats-{device}"));
    let stats = dir.join("stats.json");
    let stats_arg = stats.display().to_string();
    let (stdout, stderr, status) = run_faber_status(&[
        "run",
        "--device",
        device,
        "--stats-file",
        &stats_arg,
        &path,
        "--",
        "1",
        "0",
        "0",
    ]);
    if !stdout.starts_with(receipt_prefix) {
        eprintln!("no {device} device: skipped");
        return;
    }
    assert_eq!(status.code(), Some(70), "{stderr}");
    let errors = error_lines(&stderr);
    assert_eq!(errors.len(), 1, "one error line:\n{stderr}");
    assert!(
        errors[0].starts_with("error: E_DEVICE_KERNEL_FAULT:"),
        "{stderr}"
    );
    let artifact: serde_json::Value =
        serde_json::from_slice(&fs::read(&stats).expect("a failed device run writes its stats"))
            .expect("stats file is JSON");
    assert_eq!(artifact["run"]["exit"], 70, "{artifact}");
    assert!(artifact["device"].is_object(), "{artifact}");
    assert!(artifact["device"]["runtime"].is_object(), "{artifact}");
}

#[test]
fn run_cpu_device_kernel_fault_still_writes_the_stats_file() {
    assert_faulting_device_run_writes_stats("cpu", CPU_DEVICE_RECEIPT_PREFIX);
}

/// Skips without a Metal device.
#[test]
fn run_metal_device_kernel_fault_still_writes_the_stats_file() {
    if cfg!(target_os = "macos") {
        assert_faulting_device_run_writes_stats("metal", METAL_DEVICE_RECEIPT_PREFIX);
    }
}

/// `auto` over a kernel package that faults on the GPU ends the run once: the
/// program ran on the device session and is never run again on another route.
/// Mode 2 prints before the fault is delivered, so a second run would repeat
/// the lines. Skips without a Metal device.
#[test]
fn run_auto_device_kernel_fault_runs_the_program_once() {
    let path = repo_root()
        .join(SINGLE_LANE_INDEX_FAULT)
        .display()
        .to_string();
    let (stdout, stderr, status) =
        run_faber_status(&["run", "--device", "auto", &path, "--", "1", "0", "2"]);
    if !cfg!(target_os = "macos") || !stdout.starts_with(METAL_DEVICE_RECEIPT_PREFIX) {
        eprintln!("no Metal device: skipped");
        return;
    }
    assert_eq!(status.code(), Some(70), "{stderr}");
    assert_eq!(
        stdout.lines().filter(|l| *l == "completed").count(),
        1,
        "{stdout}"
    );
    assert_eq!(
        stdout.lines().filter(|l| *l == "done").count(),
        1,
        "{stdout}"
    );
    assert_eq!(
        stdout.matches("device: selected backend").count(),
        1,
        "{stdout}"
    );
    let errors = error_lines(&stderr);
    assert_eq!(errors.len(), 1, "one error line:\n{stderr}");
    assert!(
        errors[0].starts_with("error: E_DEVICE_KERNEL_FAULT:"),
        "{stderr}"
    );
}

/// The corpus-product device tensor fragments must print exactly what their
/// hand-computed `main.ref.json` reference says, on every route.
///
/// `printed` is the program's stdout (no device receipt line) and `route`
/// names the route in a failure message.
fn assert_prints_reference_fixture(package: &str, route: &str, printed: &str) {
    let dir = repo_root()
        .join("corpus-product/tensor-fragment")
        .join(package);
    let fixture_text =
        fs::read_to_string(dir.join("src").join("main.ref.json")).expect("read main.ref.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&fixture_text).expect("parse main.ref.json");
    let tolerance = fixture["tolerance"].as_f64().expect("tolerance");
    let want: Vec<f64> = fixture["reference"]
        .as_array()
        .expect("reference array")
        .iter()
        .map(|value| value.as_f64().expect("reference number"))
        .collect();
    let got: Vec<f64> = printed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.parse::<f64>()
                .unwrap_or_else(|_| panic!("{package}: printed line {line:?} is not a number"))
        })
        .collect();
    assert_eq!(
        got.len(),
        want.len(),
        "{package}: {route} printed {got:?}, reference {want:?}\n{printed}"
    );
    for (index, (got, want)) in got.iter().zip(&want).enumerate() {
        assert!(
            (got - want).abs() <= tolerance,
            "{package}: element {index}: {route} {got} vs main.ref.json {want} (tolerance {tolerance})"
        );
    }
}

fn assert_reference_route_matches_reference_fixture(package: &str) {
    let path = repo_root()
        .join("corpus-product/tensor-fragment")
        .join(package)
        .display()
        .to_string();
    let (stdout, stderr, ok) = run_faber(&["run", "--device", "reference", &path]);
    assert!(ok, "{package}: reference run failed:\n{stderr}");
    assert_prints_reference_fixture(package, "reference route", &stdout);
}

/// `FABER_REQUIRE_METAL` makes a missing Metal device a failure instead of a
/// skip, as `FABER_REQUIRE_CUDA` does for the CUDA hardware tests.
#[cfg(target_os = "macos")]
fn metal_required() -> bool {
    std::env::var_os("FABER_REQUIRE_METAL").is_some()
}

/// The Metal device route must print what the hand-computed `main.ref.json`
/// says, within its tolerance: the same values the reference route prints.
/// Skips without a Metal device; fails when `FABER_REQUIRE_METAL` is set.
#[cfg(target_os = "macos")]
fn assert_metal_route_matches_reference_fixture(package: &str) {
    let path = repo_root()
        .join("corpus-product/tensor-fragment")
        .join(package)
        .display()
        .to_string();
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "metal", &path]);
    let Some((_, program)) = stdout
        .split_once('\n')
        .filter(|(receipt, _)| receipt.starts_with(METAL_DEVICE_RECEIPT_PREFIX))
    else {
        assert!(
            !metal_required(),
            "FABER_REQUIRE_METAL is set but {package} did not run on a Metal device:\n{stdout}\n{stderr}"
        );
        eprintln!("no Metal device: skipped");
        return;
    };
    assert!(status.success(), "{package}: metal run failed:\n{stderr}");
    assert_prints_reference_fixture(package, "metal route", program);
}

/// The CUDA device line every CUDA device session prints ahead of the program.
const CUDA_DEVICE_RECEIPT_PREFIX: &str = "device: selected backend `cuda` on ";

/// The CUDA device route must print what the hand-computed `main.ref.json`
/// says, within its tolerance: the same values the reference route prints.
/// Skips without a CUDA device; fails when `FABER_REQUIRE_CUDA` is set.
fn assert_cuda_route_matches_reference_fixture(package: &str) {
    let path = repo_root()
        .join("corpus-product/tensor-fragment")
        .join(package)
        .display()
        .to_string();
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "cuda", &path]);
    let Some((_, program)) = stdout
        .split_once('\n')
        .filter(|(receipt, _)| receipt.starts_with(CUDA_DEVICE_RECEIPT_PREFIX))
    else {
        assert!(
            std::env::var_os("FABER_REQUIRE_CUDA").is_none(),
            "FABER_REQUIRE_CUDA is set but {package} did not run on a CUDA device:\n{stdout}\n{stderr}"
        );
        eprintln!("no CUDA device: skipped");
        return;
    };
    assert!(status.success(), "{package}: cuda run failed:\n{stderr}");
    assert_prints_reference_fixture(package, "cuda route", program);
}

#[test]
fn tiny_linear_device_reference_route_prints_the_reference_fixture() {
    assert_reference_route_matches_reference_fixture("tiny-linear-device");
}

#[test]
fn tiny_linear_device_relu_reference_route_prints_the_reference_fixture() {
    assert_reference_route_matches_reference_fixture("tiny-linear-device-relu");
}

#[test]
fn tiny_linear_device_two_matmul_reference_route_prints_the_reference_fixture() {
    assert_reference_route_matches_reference_fixture("tiny-linear-device-two-matmul");
}

/// Metal runs the matmul and the fused bias add as one kernel that stores
/// `x·W + b` into the output parameter `y`; the bias must reach the output.
#[cfg(target_os = "macos")]
#[test]
fn tiny_linear_device_metal_route_prints_the_reference_fixture() {
    assert_metal_route_matches_reference_fixture("tiny-linear-device");
}

/// Metal's fused tail ends in the relu: four of the eight outputs are zeroed.
#[cfg(target_os = "macos")]
#[test]
fn tiny_linear_device_relu_metal_route_prints_the_reference_fixture() {
    assert_metal_route_matches_reference_fixture("tiny-linear-device-relu");
}

/// Two matmuls cannot be one recipe kernel, so Metal runs one kernel per matmul
/// and the final one stores `(x·W)·V + b` into the output parameter `y`. The
/// decomposed kernel used to bind the second matmul's result local instead, and
/// `y` printed its zeros.
#[cfg(target_os = "macos")]
#[test]
fn tiny_linear_device_two_matmul_metal_route_prints_the_reference_fixture() {
    assert_metal_route_matches_reference_fixture("tiny-linear-device-two-matmul");
}

/// CUDA runs the matmul and the fused bias add as one NVVM kernel that stores
/// `x·W + b` into the output parameter `y`. The out-parameter form used to
/// store the raw accumulator, so CUDA printed `x·W`.
#[test]
fn tiny_linear_device_cuda_route_prints_the_reference_fixture() {
    assert_cuda_route_matches_reference_fixture("tiny-linear-device");
}

/// CUDA's fused tail ends in the relu: four of the eight outputs are zeroed.
#[test]
fn tiny_linear_device_relu_cuda_route_prints_the_reference_fixture() {
    assert_cuda_route_matches_reference_fixture("tiny-linear-device-relu");
}

/// Two matmuls run as one NVVM kernel per matmul; the final one stores
/// `(x·W)·V + b` into the output parameter `y`.
#[test]
fn tiny_linear_device_two_matmul_cuda_route_prints_the_reference_fixture() {
    assert_cuda_route_matches_reference_fixture("tiny-linear-device-two-matmul");
}

/// A whole-grid reduction with a `coalesce` fallback over an empty run-time
/// sized tensor.
const EMPTY_REDUCTION_PACKAGE: &str = r#"@ kernel
@ public
fn mx<size H, size W>(tensor<f32, [H, W]> src, mut tensor<f32, [4]> out) → void {
    out[0] ← reduce via max from src at [i, j] thread lane let v { return v } coalesce 7.5
}

@ cli "empty-reduce"
@ operand int rows
@ operand int cols
main args args {
    let int rows ← args.rows
    let int cols ← args.cols
    const size R ← rows
    const size C ← cols
    mut tensor<f32, [R, C]> src ← tf32[R, C] {}
    mut tensor<f32, [4]> out ← tf32[4] {}
    mx(src, mut out)
    print out[0]
}
"#;

/// Operator ruling 2026-10-14: a zero-length reduction source returns its
/// `coalesce` fallback on every device, as on the host. The device used to
/// refuse the zero extent (`geometry_zero_extent`) on `--device cpu` and abort
/// the process on `--device metal`; the dispatcher now decides at launch from
/// the run-time extent and stores the fallback. Each device agrees with the
/// reference, empty (a zero in either axis) and not. The Metal leg skips
/// without a Metal device; `FABER_REQUIRE_METAL` makes that a failure.
#[test]
fn empty_grid_reduction_returns_its_coalesce_fallback_on_every_device() {
    let package = write_basic_package("empty-grid-reduction", EMPTY_REDUCTION_PACKAGE);
    let path = package.join("src").join("main.fab").display().to_string();
    for (rows, cols, want) in [("0", "5", "7.5"), ("7", "0", "7.5"), ("7", "5", "0.0")] {
        for device in ["reference", "cpu", "metal"] {
            let (stdout, stderr, status) =
                run_faber_status(&["run", "--device", device, &path, "--", rows, cols]);
            let program = if device == "reference" {
                stdout.as_str()
            } else if let Some((receipt, program)) = stdout.split_once('\n')
                && receipt.starts_with("device: selected backend `")
            {
                program
            } else {
                #[cfg(target_os = "macos")]
                assert!(
                    device != "metal" || !metal_required(),
                    "FABER_REQUIRE_METAL is set but {rows}x{cols} did not run on a Metal device:\n{stdout}\n{stderr}"
                );
                eprintln!("no {device} device: skipped");
                continue;
            };
            assert!(
                status.success(),
                "{device} {rows}x{cols} failed:\n{stdout}\n{stderr}"
            );
            assert_eq!(program.trim(), want, "{device} {rows}x{cols}");
        }
    }
}
