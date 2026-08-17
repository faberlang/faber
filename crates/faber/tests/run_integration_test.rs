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

/// Container `examples/` root: `FABER_EXAMPLES_HOME` override, then the
/// faberlang container root via `radix::paths::faberlang_home()`.
fn examples_home() -> PathBuf {
    if let Ok(home) = std::env::var("FABER_EXAMPLES_HOME") {
        return PathBuf::from(home);
    }
    radix::paths::faberlang_home()
        .expect("faberlang container root")
        .join("examples")
}

fn reader_locale_example_root(locale: &str) -> PathBuf {
    examples_home().join("reader-locale").join(locale)
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
    assert!(
        stderr.contains("cannot read 'missing-package': No such file or directory"),
        "expected missing path error, got:\n{stderr}"
    );
}

#[test]
fn test_reader_locale_accepts_direct_entry_file_input() {
    let source = write_single_file(
        "test-reader-locale-single-file",
        r#"
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
fn test_reader_locale_accepts_manifest_file_input() {
    let package = write_basic_package(
        "test-reader-locale-manifest",
        r#"
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
fn test_reader_locale_missing_package_path_surfaces_missing_path_error() {
    let (stdout, stderr, ok) = run_faber(&["test", "--locale", "th-TH", "missing-package"]);

    assert!(!ok, "missing package path should fail");
    assert!(
        stdout.is_empty(),
        "missing package path should not write stdout: {stdout}"
    );
    assert!(
        !stderr.contains("--locale th-TH requires a package path or .fab entry file"),
        "missing package path should reach the underlying filesystem error, got:\n{stderr}"
    );
    assert!(
        stderr.contains("cannot read 'missing-package': No such file or directory"),
        "expected missing path error, got:\n{stderr}"
    );
}

#[test]
fn test_reader_locale_rejects_existing_non_package_file() {
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
fn test_reader_locale_rejects_stdin_input() {
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
    assert!(
        stderr.contains("cannot read 'missing-package': No such file or directory"),
        "expected missing path error, got:\n{stderr}"
    );
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
fn fmir_bin_norma_solum_package_reads_file_after_source_is_removed() {
    let package = write_basic_package(
        "fmir-bin-solum",
        r#"import from "norma:solum" fileio

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
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    // Under VM-U2 a `@ private` member imported through a file-namespace
    // alias fails at the import seam (SEM006) instead of typecheck
    // namespace_missing_export (SEM004).
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
    // interpreted script path (chorda.reverse runs). The script must execute
    // without emitting Rust artifacts.
    let package = temp_dir("norma-linked");
    fs::write(
        package.join("main.fab"),
        r#"
import from "norma:chorda" chorda

main {
  print chorda.reverse("roma")
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
    assert_eq!(stdout, "verum\n");
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
    assert_eq!(stdout, "verum\n");
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
    assert_eq!(stdout, "verum\n");
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
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    // Under VM-U2 a `@ private` member imported through a file-namespace
    // alias fails at the import seam (SEM006).
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
fn run_norma_solum_package_reads_file_through_kernel_bridge() {
    // One `norma:*` import block; interpreted package execution satisfies it
    // through the runner kernel (Stage 1b host bridge). No second runner-only
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
            r#"import from "norma:solum" fileio

main {{
  print fileio.read_file<string>("{payload_str}")
}}
"#
        ),
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["run", package.to_str().expect("utf8 package path")]);

    assert!(ok, "norma:solum package bridge failed:\n{stderr}");
    assert_eq!(stdout, "bridge-ok\n");
    assert!(
        !stderr.contains("Compiling") && !stderr.contains("cargo"),
        "interpreted package bridge must not invoke Cargo:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_norma_solum_unsupported_verb_fails_closed() {
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
        r#"import from "norma:solum" fileio

main {
  fileio.read_file_async<string>("/nonexistent")
}
"#,
    )
    .expect("write entry");

    let (stdout, stderr, ok) = run_faber(&["run", package.to_str().expect("utf8 package path")]);

    assert!(!ok, "unsupported norma:solum verb should fail closed");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("package MIR kernel bridge does not support `norma:solum.read_file_async`"),
        "expected kernel bridge fail-closed diagnostic:\n{stderr}"
    );
    assert_no_generated_rust(&package);
}

#[test]
fn run_norma_processus_argumenta_bridges_to_kernel() {
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
        r#"import from "norma:processus" processus

main args args {
  print processus.argumenta()[0]
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

    assert!(ok, "norma:processus bridge failed:\n{stderr}");
    assert_eq!(stdout, "first-arg\n");
    assert_no_generated_rust(&package);
}
