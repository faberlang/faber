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

/// Offline support root: the repository root holding `runtime/rust`.
fn support_root() -> PathBuf {
    let root = repo_root();
    assert!(
        root.join("runtime/rust/Cargo.toml").exists(),
        "could not find runtime/rust under {}",
        root.display()
    );
    root
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

    for target in ["fmir-text", "fmir", "fmir-bin"] {
        let prefix = format!("{target} ");
        let row = stdout
            .lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("missing {target} row:\n{stdout}"));
        assert!(
            row.contains("check=yes build=yes run=yes package=yes"),
            "Faber package FMIR target row must show package build/run truth:\n{row}"
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

    let workspace = support_root();
    let (stdout, stderr, ok) = run_faber_with_env(
        &[
            "build",
            "--locale",
            "zh-Hans",
            source.to_str().expect("utf8 source path"),
        ],
        &[("FABER_SUPPORT_PATH_OVERRIDE", &workspace)],
    );

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
    // D17.8 (fi6c): a failure directly in `main` is a panic: exit code 1, the
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
    const int x ← risky(-1)
    print x
}
"#,
    );

    let (stdout, stderr, status) =
        run_faber_status(&["run", source.to_str().expect("utf8 source path")]);

    assert_eq!(
        status.code(),
        Some(1),
        "a panic exits with code 1:\n{stderr}"
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

#[test]
#[ignore = "builds a generated fmir-bin Rust runner with Cargo; run in periodic end-to-end coverage"]
fn fmir_bin_package_forwards_runtime_arg_after_source_is_removed() {
    let package = write_basic_package(
        "fmir-bin-cli",
        r#"
@ cli "tool"
@ operand string name
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

    // `faber run` is interpreted execution and no longer executes built
    // targets; the fmir-bin bundle is a self-contained artifact executed
    // directly (compiled artifacts are built, never executed, by faber).
    let (run_stdout, run_stderr, run_ok) = run_executable(&entrypoint, &["runtime"]);
    assert!(run_ok, "fmir-bin entrypoint failed:\n{run_stderr}");
    assert_eq!(run_stdout, "fmir-bin runtime!\n");

    fs::remove_file(package.join("src/main.fab")).expect("remove source");
    fs::remove_file(package.join("target/faber-mir/exe/image.fmir")).expect("remove sidecar image");
    let _relocated_root = temp_dir("fmir-bin-relocated");
    let relocated = _relocated_root.join("run");
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
#[ignore = "builds a generated fmir-bin Rust runner with Cargo; run in periodic end-to-end coverage"]
fn fmir_bin_multifile_package_runs_after_source_mutation() {
    let package = write_basic_package(
        "fmir-bin-multifile",
        r#"
importa ex "./message" message

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
#[ignore = "builds a generated fmir-bin Rust runner with Cargo; run in periodic end-to-end coverage"]
fn fmir_bin_norma_fs_package_reads_file_after_source_is_removed() {
    let package = write_basic_package(
        "fmir-bin-solum",
        r#"import from "norma:fs" fileio

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
    assert!(ok, "direct fmir-bin norma:fs entrypoint failed:\n{stderr}");
    assert_eq!(stdout, "solum-ok\n");
    assert_no_generated_rust(&package);
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
  const list<string> files ← args.files
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
@ imperia "jobs" ex jobs
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
@ imperia "jobs" ex jobs
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
@ imperia "jobs" ex jobs
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
@ imperia "jobs" ex jobs
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
@ imperia "jobs" ex jobs
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
@ imperia "jobs" ex jobs
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
@ imperia "jobs" ex jobs
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
@ imperia "jobs" ex jobs
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
        const string body ← fs.read_file<string>("__PATH__")
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
  const _ partial ← (string msg) → void ⇥ fault.IoFault ∴ write_partial(msg)
  const _ out_bytes ← (bytes data) → void ⇥ fault.IoFault ∴ write_bytes(data)
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
fn run_panic_prints_abort_reason_to_stderr_and_exits_one() {
    // `faber run` surfaces a runner abort the way the runner's own
    // `StdioHost::abort` does: `abort: <message>` on stderr, exit 1, with
    // stdout written before the trap still flushed.
    let file = write_single_file(
        "run-panic-abort",
        "main {\n    print \"before\"\n    panic \"boom\"\n}\n",
    );

    let (stdout, stderr, status) =
        run_faber_status(&["run", file.to_str().expect("utf8 source path")]);

    assert_eq!(status.code(), Some(1), "panic must exit 1:\n{stderr}");
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
        "main {\n    const int b ← 2\n    const int e ← -1\n    print \"before\"\n    print b.power(e)\n    print \"after\"\n}\n",
    );

    let (stdout, stderr, status) =
        run_faber_status(&["run", file.to_str().expect("utf8 source path")]);

    assert_eq!(status.code(), Some(1), "trap must exit 1:\n{stderr}");
    assert_eq!(stdout, "before\n");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: runtime abort reason is the stderr contract.
    assert!(
        stderr.contains("abort: numerus potentia failed: negative exponent"),
        "expected trap reason on stderr:\n{stderr}"
    );
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
fn run_kernel_file_default_route_matches_cpu_or_refuses_and_runs_on_the_cpu_device() {
    let file = write_single_file("run-kernel-cpu-device", KERNEL_SOURCE);
    let path = file.to_str().expect("utf8 source path");

    // Default route: `auto` over a kernel package takes the device session and
    // never the CPU: a host with an admitted device runs the kernel there (the
    // same result), a host without one refuses.
    let (stdout, stderr, status) = run_faber_status(&["run", path]);
    assert_default_route_matches_cpu_or_refuses(&stdout, &stderr, status.code());
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "auto", path]);
    assert_default_route_matches_cpu_or_refuses(&stdout, &stderr, status.code());

    // CPU device: the kernel runs with its sequential meaning.
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "cpu", path]);
    assert!(
        status.success(),
        "`--device cpu` must run the kernel:\n{stderr}"
    );
    assert_eq!(stdout, "2.0\n");
}

#[test]
fn run_kernel_package_default_route_and_cpu_device_by_flag_manifest_and_with_stats() {
    let package = write_basic_package("run-kernel-package-cpu-device", KERNEL_SOURCE);
    let path = package.to_str().expect("utf8 package path");

    let (stdout, stderr, status) = run_faber_status(&["run", path]);
    assert_default_route_matches_cpu_or_refuses(&stdout, &stderr, status.code());

    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "cpu", path]);
    assert!(
        status.success(),
        "`--device cpu` must run the package kernel:\n{stderr}"
    );
    assert_eq!(stdout, "2.0\n");

    // `--device cpu --stats` works like the default route: the program output
    // and the framed statistics artifact both arrive.
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "cpu", "--stats", path]);
    assert!(
        status.success(),
        "`--device cpu --stats` must run:\n{stderr}"
    );
    assert!(
        stdout.starts_with("2.0\n"),
        "program output first:\n{stdout}"
    );
    assert!(
        stdout.contains("--- faber-run-stats ---"),
        "stats artifact framed on stdout:\n{stdout}"
    );

    // The manifest selects the CPU device; an explicit flag still wins.
    let manifest = fs::read_to_string(package.join("faber.toml")).expect("read manifest");
    fs::write(
        package.join("faber.toml"),
        format!("{manifest}\n[device]\ndevice = \"cpu\"\n"),
    )
    .expect("write manifest with a cpu device");
    let (stdout, stderr, status) = run_faber_status(&["run", path]);
    assert!(
        status.success(),
        "a manifest `cpu` device must run the kernel:\n{stderr}"
    );
    assert_eq!(stdout, "2.0\n");
    // `--device auto` overrides the manifest's `cpu`: the device route, so it
    // matches the CPU result or refuses, never a different result.
    let (stdout, stderr, status) = run_faber_status(&["run", "--device", "auto", path]);
    assert_default_route_matches_cpu_or_refuses(&stdout, &stderr, status.code());
}
