//! Faber-side remainder of the package-MIR tests (RP5).
//!
//! The compiler-owned tests moved to `radix-program/src/mir/mir_test.rs`
//! (constants, FNV-1a, manifest helpers, CLI planning, requirements, manifest
//! validation, MIR append/merge). These tests stay faber-side because they
//! exercise faber product surfaces the compiler subset never sees: the
//! generated fmir-bin runner crate (`bin_runner.rs`) and the `[device]`
//! payload (the `MirSeams::device_section_builder` seam body).

use super::bin_runner::{
    make_fmir_bin_entrypoint_executable, render_fmir_bin_runner_main_rs, toml_basic_string_path,
};
use super::*;
use radix::program::mir::DeviceSelection;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

// ── Constant / configuration tests ──────────────────────────────────────────

#[test]
fn fmir_bin_entrypoint_and_runner_names_are_configured() {
    assert_eq!(radix::program::mir::FMIR_BIN_ENTRYPOINT_FILE, "run");
    assert_eq!(FMIR_BIN_RUNNER_CRATE_DIR, "runner");
    assert_eq!(FMIR_BIN_RUNNER_PACKAGE_NAME, "faber-fmir-bin-runner");
}

#[test]
fn fmir_bin_runner_source_fingerprints_embedded_image() {
    let first = render_fmir_bin_runner_main_rs(b"first image");
    let second = render_fmir_bin_runner_main_rs(b"second image");

    assert_ne!(
        first, second,
        "shared cargo target builds must see image changes as source changes"
    );
    assert!(first.contains("embedded image fnv64:"));
    assert!(first.contains("include_bytes!(\"../../image.fmir\")"));
}

// ── toml_basic_string_path ──────────────────────────────────────────────────

#[test]
fn toml_basic_string_path_passes_through_simple_path() {
    let path = Path::new("src/main.fab");
    assert_eq!(toml_basic_string_path(path), "src/main.fab");
}

#[test]
fn toml_basic_string_path_escapes_backslashes() {
    let path = Path::new("windows\\path\\file.fab");
    assert_eq!(toml_basic_string_path(path), "windows\\\\path\\\\file.fab");
}

#[test]
fn toml_basic_string_path_escapes_double_quotes() {
    let path = Path::new("path/with\"quote");
    assert_eq!(toml_basic_string_path(path), "path/with\\\"quote");
}

// ── make_fmir_bin_entrypoint_executable (unix) ─────────────────────────────

#[cfg(unix)]
#[test]
fn make_fmir_bin_entrypoint_executable_sets_755_permissions() {
    let dir = std::env::temp_dir().join(format!(
        "faber-mir-entrypoint-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let entrypoint = dir.join("run");
    std::fs::write(&entrypoint, "#!/bin/sh\necho hello").expect("write entrypoint");

    let result = make_fmir_bin_entrypoint_executable(&entrypoint, &dir);
    assert!(
        result.is_ok(),
        "make_fmir_bin_entrypoint_executable should succeed"
    );

    let metadata = std::fs::metadata(&entrypoint).expect("read metadata");
    let mode = metadata.permissions().mode();
    assert_eq!(mode & 0o777, 0o755, "entrypoint should be executable (755)");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── RSM-4 / gpu-reset C2 U7: the runner route and device selections ──────

fn write_stats_route_package(label: &str, source: &str) -> crate::package::test_support::TestDir {
    let dir = crate::package::test_support::test_temp_dir(label);
    std::fs::write(dir.path().join("main.fab"), source).expect("write entry");
    dir
}

#[test]
fn runner_route_stats_on_cpu_auto_keeps_interpreter_and_collects() {
    let dir = write_stats_route_package(
        "route-stats-cpu-collects",
        "fn labor() → int {\n    return 42\n}\n\nmain {\n    print labor()\n}\n",
    );
    let entry = dir.path().join("main.fab");
    let mut host = radix::module::mir::BufferHost::default();

    let outcome = radix::program::mir::run_package_mir_with_selection_and_stats(
        &radix::module::driver::Config::default(),
        &entry,
        DeviceSelection::Auto,
        &faber_mir_seams(),
        &mut host,
    )
    .expect("CPU/auto with stats stays on the interpreter");

    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned()],
        "the interpreter ran the program"
    );
    assert!(
        outcome
            .stats
            .functions
            .iter()
            .any(|row| row.name == "labor" && row.calls == 1),
        "the meter collected the called function: {:?}",
        outcome.stats.functions
    );
    assert!(outcome.result.is_ok());
}

#[test]
fn runner_route_explicit_gpu_selection_fails_closed_with_or_without_stats() {
    let dir = write_stats_route_package("route-gpu-refused", "main { print 1 }\n");
    let entry = dir.path().join("main.fab");

    for stats in [true, false] {
        let mut host = radix::module::mir::BufferHost::default();
        let err = if stats {
            radix::program::mir::run_package_mir_with_selection_and_stats(
                &radix::module::driver::Config::default(),
                &entry,
                DeviceSelection::Metal,
                &faber_mir_seams(),
                &mut host,
            )
            .err()
        } else {
            radix::program::mir::run_package_mir_with_selection(
                &radix::module::driver::Config::default(),
                &entry,
                DeviceSelection::Metal,
                &faber_mir_seams(),
                &mut host,
            )
            .err()
        }
        .expect("the runner route cannot serve an explicit GPU selection");
        assert!(
            err.iter().any(|diag| diag
                .args
                .iter()
                .any(|arg| arg.name == "selection" && arg.value == "metal")),
            "stats={stats}: {err:?}"
        );
        assert!(host.stdout_lines.is_empty(), "the CPU leg must not run");
    }
}
