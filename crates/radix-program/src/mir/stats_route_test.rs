//! RSM-2 route tests: the stats-capable package run route.
//!
//! Proves the CPU/FMIR stats leg returns the collected summary beside the
//! started run's status, and that a device-bearing image or explicit GPU
//! selection fails closed on the runner route (gpu-reset C2 U7).

use super::*;
use radix_mir_runner::BufferHost;

// ── fixtures ────────────────────────────────────────────────────────────────

fn write_stats_package(dir: &std::path::Path, label: &str, source: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create package src");
    std::fs::write(
        dir.join("faber.toml"),
        format!(
            r#"
[package]
name = "stats-route-{label}"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#
        ),
    )
    .expect("write faber.toml");
    std::fs::write(src.join("main.fab"), source).expect("write main.fab");
    src.join("main.fab")
}

// Default-locale fixtures (the run_test.rs BufferHost temp-package shape).
const STATS_MAIN: &str =
    "fn dupla(int x) → int {\n    return x * 2\n}\n\nmain {\n    print dupla(21)\n}\n";
const RUNTIME_ERROR_MAIN: &str =
    "main {\n    print \"before\"\n    const list<int> base ← [1, 2]\n    print base[5]\n}\n";
const COMPILE_ERROR_MAIN: &str = "main {\n    print numquam\n}\n";

// ── focused tests ───────────────────────────────────────────────────────────

#[test]
fn stats_package_run_returns_per_function_rows_and_preserves_output() {
    let dir = crate::test_support::test_temp_dir("rsm2-stats-package");
    let entry = write_stats_package(dir.as_ref(), "rows", STATS_MAIN);
    let config = Config::default();

    let mut host = BufferHost::default();
    let outcome = run_package_mir_with_selection_and_stats(
        &config,
        &entry,
        DeviceSelection::Auto,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("a CPU package run with stats must succeed");

    assert!(outcome.result.is_ok(), "started run must report success");
    // Ordinary program output unchanged.
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);
    // Per-function rows with exact call counts (entry + the helper).
    assert!(
        outcome
            .stats
            .functions
            .iter()
            .any(|row| row.name == "dupla" && row.calls == 1),
        "the helper function must carry an exact calls row: {:?}",
        outcome.stats.functions
    );
    assert_eq!(
        outcome.stats.functions.len(),
        2,
        "the entry and the helper must each carry a row: {:?}",
        outcome.stats.functions
    );
    assert!(
        outcome.stats.functions.iter().all(|row| row.calls == 1),
        "each function must record exactly one call: {:?}",
        outcome.stats.functions
    );
}

#[test]
fn stats_package_run_started_runtime_error_keeps_summary_and_status() {
    let dir = crate::test_support::test_temp_dir("rsm2-stats-runtime-error");
    let entry = write_stats_package(dir.as_ref(), "runtime-error", RUNTIME_ERROR_MAIN);
    let config = Config::default();

    let mut host = BufferHost::default();
    let outcome = run_package_mir_with_selection_and_stats(
        &config,
        &entry,
        DeviceSelection::Auto,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("a started run's runtime diagnostic must still return the outcome");

    assert!(
        outcome.result.is_err(),
        "the uncaught runtime throw must be reported as the run's status"
    );
    assert!(
        !outcome.stats.functions.is_empty(),
        "the summary must survive the runtime failure: {:?}",
        outcome.stats
    );
    // Output emitted before the runtime failure is preserved.
    assert_eq!(host.stdout_lines, vec!["before".to_owned()]);
}

#[test]
fn stats_package_run_compile_failure_returns_diagnostics_without_summary() {
    let dir = crate::test_support::test_temp_dir("rsm2-stats-compile-error");
    let entry = write_stats_package(dir.as_ref(), "compile-error", COMPILE_ERROR_MAIN);
    let config = Config::default();

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir_with_selection_and_stats(
        &config,
        &entry,
        DeviceSelection::Auto,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect_err("a compile failure must return diagnostics");

    assert!(
        !diagnostics.is_empty(),
        "compile failures return diagnostics without a summary"
    );
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn runner_route_refuses_explicit_gpu_selection_without_a_device_image() {
    let dir = crate::test_support::test_temp_dir("rsm2-stats-gpu-refused");
    let entry = write_stats_package(dir.as_ref(), "gpu-refused", STATS_MAIN);

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir_with_selection_and_stats(
        &Config::default(),
        &entry,
        DeviceSelection::Metal,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect_err("an explicit GPU selection cannot run on the runner route");

    assert!(
        diagnostics.iter().any(|d| d
            .args
            .iter()
            .any(|arg| arg.name == "selection" && arg.value == "metal")),
        "the refusal must name the selection: {diagnostics:?}"
    );
    assert!(host.stdout_lines.is_empty(), "the CPU leg must not run");
}
