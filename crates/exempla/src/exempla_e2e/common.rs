//! Shared helpers for exempla end-to-end harnesses and focused backend smoke tests.
//!
//! These helpers own filesystem discovery, temporary roots, output normalization,
//! external-tool availability probes, and pinned smoke fixtures
//! (`TSC_SMOKE_ARGS`, `GO_MOD_CONTENT`). Target-specific harness behavior stays
//! in the sibling modules; the shared smoke entry points live in `smoke.rs`.

use super::types::E2eResult;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wait_timeout::ChildExt;

pub(crate) fn command_output_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot spawn child: {error}"))?;
    match child
        .wait_timeout(timeout)
        .map_err(|error| format!("cannot wait for child: {error}"))?
    {
        Some(_) => child
            .wait_with_output()
            .map_err(|error| format!("cannot collect child output: {error}")),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("child timed out after {}s", timeout.as_secs()))
        }
    }
}

pub(crate) fn command_status_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::ExitStatus, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot spawn child: {error}"))?;
    match child
        .wait_timeout(timeout)
        .map_err(|error| format!("cannot wait for child: {error}"))?
    {
        Some(status) => Ok(status),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("child timed out after {}s", timeout.as_secs()))
        }
    }
}

/// Returns whether `racket` is on PATH and responds to `--version`.
pub(crate) fn racket_available() -> bool {
    command_available("racket", &["--version"])
}

pub(crate) fn command_available(command: &str, args: &[&str]) -> bool {
    Command::new(command)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

pub(crate) fn make_temp_root() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("radix-rs-e2e-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("create e2e temp root");
    dir
}

/// Returns a shared Cargo target directory for all exempla builds in this harness run.
///
/// Every exemplum declares the same `faber`/`norma`/`tokio` dependencies, so compiling
/// them once into a shared `target/` lets the other 219 exempla reuse the cached
/// `.rlib` instead of rebuilding the dependency tree from scratch each time.
/// Without this, each exemplum's isolated `[workspace]` gets its own empty `target/`
/// and `faber` is recompiled ~220 times per harness run.
pub(crate) fn shared_target_dir(temp_root: &Path) -> PathBuf {
    let dir = temp_root.join("shared-target");
    fs::create_dir_all(&dir).expect("create shared cargo target dir");
    dir
}

pub(crate) fn collect_exempla_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_exempla_files_recursive(dir, &mut files);
    files.sort();
    files
}

pub(crate) fn collect_exempla_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_exempla_files_recursive(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("fab") {
            out.push(path);
        }
    }
}

pub(crate) fn read_expected_stdout(fab_path: &Path) -> Option<String> {
    let expected_path = fab_path.with_extension("expected");
    let content = fs::read_to_string(expected_path).ok()?;
    Some(normalize_newline(&content))
}

pub(crate) fn is_expected_failure(path: &Path, expected_failures: &[&str]) -> bool {
    expected_failures
        .iter()
        .any(|expected| path.ends_with(expected))
}

pub(crate) fn expected_runtime_failure<'a>(
    path: &Path,
    expected_failures: &'a [(&str, &str)],
) -> Option<&'a str> {
    expected_failures
        .iter()
        .find_map(|(expected_path, expected_message)| {
            path.ends_with(expected_path).then_some(*expected_message)
        })
}

pub(crate) fn format_result_paths(results: &[&E2eResult]) -> String {
    results
        .iter()
        .map(|result| result.path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn normalize_newline(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end_matches('\n').to_owned()
}

pub(crate) fn format_diagnostics(result: &radix::CompileResult) -> String {
    format_diagnostic_messages(&result.diagnostics)
}

pub(crate) fn format_forma_diagnostics(result: &radix::forma::FormatCompileResult) -> String {
    format_diagnostic_messages(&result.diagnostics)
}

/// Formats a harness tier summary line with the pinned floor constant beside the live count.
pub(crate) fn format_tier_line(label: &str, live: usize, total: usize, floor: usize) -> String {
    format!("  {label}: {live}/{total} (floor {floor})")
}

/// Formats a harness bucket line with an explicit regression ceiling (lower is better).
pub(crate) fn format_ceiling_line(label: &str, live: usize, ceiling: usize) -> String {
    format!("  {label}: {live} (ceiling {ceiling})")
}

/// Formats a harness bucket line with a minimum-count floor (used by pre-ceiling LLVM baselines).
pub(crate) fn format_count_floor_line(label: &str, live: usize, floor: usize) -> String {
    format!("  {label}: {live} (floor {floor})")
}

pub(crate) fn format_diagnostic_messages(diagnostics: &[radix::Diagnostic]) -> String {
    if diagnostics.is_empty() {
        "no diagnostics".to_owned()
    } else {
        diagnostics
            .iter()
            .map(|diag| format!("{:?}:{:?}", diag.code, diag.issue()))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// Returns whether `rustc` is on PATH and responds to `--version`.
pub(crate) fn rustc_available() -> bool {
    command_available("rustc", &["--version"])
}

/// Returns whether `cargo` is on PATH and responds to `--version`.
pub(crate) fn cargo_available() -> bool {
    command_available("cargo", &["--version"])
}

/// Absolute path to the public `faber-runtime` crate (`use faber::…`).
pub(crate) fn faber_crate_path() -> PathBuf {
    crate::paths::faber_runtime_crate_path()
}

pub(crate) fn generated_rust_needs_tokio(code: &str) -> bool {
    code.contains("tokio::") || code.contains("__faber_block_on")
}

fn render_e2e_cargo_dependencies(code: &str) -> String {
    // Public crate package is `faber-runtime`; library name remains `faber`.
    let mut deps = format!(
        "faber = {{ package = \"faber-runtime\", path = \"{}\" }}\n",
        faber_crate_path().display()
    );
    if generated_rust_needs_tokio(code) {
        deps.push_str("tokio = { version = \"1\", features = [\"rt\", \"net\", \"time\"] }\n");
    }
    deps
}

/// Write a minimal Cargo project linked like `faber build` and return the manifest path.
pub(crate) fn write_rust_cargo_project(
    project_dir: &Path,
    package_name: &str,
    code: &str,
) -> PathBuf {
    fs::create_dir_all(project_dir.join("src")).expect("create cargo src dir");
    fs::write(project_dir.join("src/main.rs"), code).expect("write generated rust");
    let manifest = format!(
        r#"[package]
name = "{package_name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{package_name}"
path = "src/main.rs"

[workspace]

[dependencies]
{deps}"#,
        deps = render_e2e_cargo_dependencies(code)
    );
    let manifest_path = project_dir.join("Cargo.toml");
    fs::write(&manifest_path, manifest).expect("write Cargo.toml");
    manifest_path
}

/// Write a workspace *member* manifest (no `[workspace]` block) for batched e2e builds.
///
/// Unlike [`write_rust_cargo_project`], members inherit the workspace root and share one
/// `target/`, so a single `cargo build` at the root compiles every member in one process
/// instead of paying cargo's per-invocation spawn + fingerprint overhead 220 times.
pub(crate) fn write_rust_workspace_member(
    project_dir: &Path,
    package_name: &str,
    code: &str,
) -> PathBuf {
    fs::create_dir_all(project_dir.join("src")).expect("create member cargo src dir");
    fs::write(project_dir.join("src/main.rs"), code).expect("write member generated rust");
    let manifest = format!(
        r#"[package]
name = "{package_name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{package_name}"
path = "src/main.rs"

[dependencies]
{deps}"#,
        deps = render_e2e_cargo_dependencies(code)
    );
    let manifest_path = project_dir.join("Cargo.toml");
    fs::write(&manifest_path, manifest).expect("write member Cargo.toml");
    manifest_path
}

/// Write the workspace root manifest declaring all batched exempla as members.
///
/// `member_rel_paths` are paths relative to `temp_root` (e.g. `"000-ab"`). A single
/// `cargo build --manifest-path <root>` then compiles every member into the shared
/// `CARGO_TARGET_DIR`, amortizing cargo's spawn/fingerprint cost across the whole corpus.
pub(crate) fn write_rust_workspace_root(temp_root: &Path, member_rel_paths: &[String]) -> PathBuf {
    let members = member_rel_paths
        .iter()
        .map(|p| format!("    \"{}\",", p))
        .collect::<Vec<_>>()
        .join("\n");
    let manifest = format!("[workspace]\nresolver = \"2\"\nmembers = [\n{members}\n]\n");
    let manifest_path = temp_root.join("Cargo.toml");
    fs::write(&manifest_path, manifest).expect("write workspace root Cargo.toml");
    manifest_path
}

/// Returns whether `go` is on PATH and responds to `version`.
pub(crate) fn go_available() -> bool {
    command_available("go", &["version"])
}

/// Returns whether `wasm-tools` is on PATH and responds to `--version`.
pub(crate) fn wasm_tools_available() -> bool {
    command_available("wasm-tools", &["--version"])
}

/// Returns whether `naga` (naga-cli) is on PATH and responds to `--version`.
///
/// Used as the external module-valid gate for promoted `wgsl-text` emit.
pub(crate) fn naga_available() -> bool {
    command_available("naga", &["--version"])
}

/// Returns whether `llvm-as` is on PATH and responds to `--version`.
pub(crate) fn llvm_as_available() -> bool {
    command_available("llvm-as", &["--version"])
}

/// Returns whether a TypeScript structural typechecker is on PATH (`tsc` preferred over `deno`).
pub(crate) fn ts_typechecker_available() -> bool {
    command_available("tsc", &["--version"]) || command_available("deno", &["--version"])
}

/// Pinned `tsc` flags for Family A TypeScript smoke (shared with exempla harness).
pub(crate) const TSC_SMOKE_ARGS: &[&str] = &[
    "--strict", "--noEmit", "--target", "ES2022", "--module", "commonjs",
];

/// Minimal `go.mod` for single-file Go smoke fixtures.
pub(crate) const GO_MOD_CONTENT: &str = "module smoke\n\ngo 1.21\n";

#[cfg(test)]
#[path = "common_test.rs"]
mod tests;
