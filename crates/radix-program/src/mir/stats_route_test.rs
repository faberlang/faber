//! RSM-2 route-transport tests: the stats-capable package run route.
//!
//! Proves the explicit `stats_requested` fact reaches the widened
//! `HostRoutePolicy` seam ahead of the device decision, that the CPU/FMIR
//! stats leg returns the collected summary beside the started run's status,
//! and that existing non-stats callers carry the explicit `false`.

use super::*;
use radix_module::mir::BufferHost;
use std::sync::atomic::{AtomicBool, Ordering};

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
const STATS_MAIN: &str = "fn dupla(int x) → int {\n    return x * 2\n}\n\nmain {\n    print dupla(21)\n}\n";
const RUNTIME_ERROR_MAIN: &str = "main {\n    print \"before\"\n    const list<int> base ← [1, 2]\n    print base[5]\n}\n";
const COMPILE_ERROR_MAIN: &str = "main {\n    print numquam\n}\n";

// ── seam spies ──────────────────────────────────────────────────────────────

static SPY_SEEN: AtomicBool = AtomicBool::new(false);
static SPY_STATS_REQUESTED: AtomicBool = AtomicBool::new(false);

/// CPU-route spy: records the transported fact and keeps the interpreter on
/// the route (`Ok(None)`), mirroring the production no-device posture.
fn spy_host_route(
    _image: &FmirPackageImage,
    _selection: DeviceSelection,
    stats_requested: bool,
) -> Result<Option<()>, Vec<Diagnostic>> {
    SPY_SEEN.store(true, Ordering::SeqCst);
    SPY_STATS_REQUESTED.store(stats_requested, Ordering::SeqCst);
    Ok(None)
}

/// Device-route spy: records the transported fact and reports the device
/// route as already executed, proving the fact arrives before the route
/// decision consumes it.
static SPY_DEVICE_SEEN: AtomicBool = AtomicBool::new(false);
static SPY_DEVICE_STATS_REQUESTED: AtomicBool = AtomicBool::new(false);

fn spy_device_host_route(
    _image: &FmirPackageImage,
    _selection: DeviceSelection,
    stats_requested: bool,
) -> Result<Option<()>, Vec<Diagnostic>> {
    SPY_DEVICE_SEEN.store(true, Ordering::SeqCst);
    SPY_DEVICE_STATS_REQUESTED.store(stats_requested, Ordering::SeqCst);
    Ok(Some(()))
}

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
        true,
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
        outcome
            .stats
            .functions
            .iter()
            .all(|row| row.calls == 1),
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
        true,
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
        true,
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
fn seam_spy_proves_stats_requested_reaches_policy_and_callers_pass_explicit_false() {
    let dir = crate::test_support::test_temp_dir("rsm2-stats-spy");
    let entry = write_stats_package(dir.as_ref(), "spy", STATS_MAIN);
    let config = Config::default();
    let seams = MirSeams {
        host_route: spy_host_route,
        ..crate::test_support::test_mir_seams()
    };

    // The stats-capable route transports `true` into the policy before the
    // device decision; `Ok(None)` keeps the CPU route and its summary.
    SPY_SEEN.store(false, Ordering::SeqCst);
    SPY_STATS_REQUESTED.store(false, Ordering::SeqCst);
    let mut host = BufferHost::default();
    let outcome = run_package_mir_with_selection_and_stats(
        &config,
        &entry,
        DeviceSelection::Auto,
        true,
        &seams,
        &mut host,
    )
    .expect("the spy seam keeps the CPU route");
    assert!(SPY_SEEN.load(Ordering::SeqCst), "the policy must be consulted");
    assert!(
        SPY_STATS_REQUESTED.load(Ordering::SeqCst),
        "stats_requested = true must reach HostRoutePolicy before the device decision"
    );
    assert!(outcome.result.is_ok());
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);

    // Existing non-stats callers carry the explicit `false`.
    SPY_SEEN.store(false, Ordering::SeqCst);
    SPY_STATS_REQUESTED.store(true, Ordering::SeqCst);
    let mut host = BufferHost::default();
    run_package_mir_with_selection(&config, &entry, DeviceSelection::Auto, &seams, &mut host)
        .expect("the unstatsed route behavior is unchanged");
    assert!(SPY_SEEN.load(Ordering::SeqCst), "the policy must be consulted");
    assert!(
        !SPY_STATS_REQUESTED.load(Ordering::SeqCst),
        "non-stats callers must pass the explicit false"
    );
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);

    // A device route executes at the policy with the fact in hand; the
    // interpreter (and its summary) never runs.
    let device_seams = MirSeams {
        host_route: spy_device_host_route,
        ..crate::test_support::test_mir_seams()
    };
    SPY_DEVICE_SEEN.store(false, Ordering::SeqCst);
    SPY_DEVICE_STATS_REQUESTED.store(false, Ordering::SeqCst);
    let mut host = BufferHost::default();
    let outcome = run_package_mir_with_selection_and_stats(
        &config,
        &entry,
        DeviceSelection::Auto,
        true,
        &device_seams,
        &mut host,
    )
    .expect("the device-route spy reports the route as executed");
    assert!(
        SPY_DEVICE_SEEN.load(Ordering::SeqCst),
        "the device decision must observe the transported fact"
    );
    assert!(
        SPY_DEVICE_STATS_REQUESTED.load(Ordering::SeqCst),
        "the device decision must receive stats_requested = true"
    );
    assert!(
        outcome.stats.functions.is_empty(),
        "a device route records no interpreter rows"
    );
    assert!(host.stdout_lines.is_empty(), "the CPU leg must not run");
}
