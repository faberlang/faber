//! Faber-side remainder of the package-MIR tests (RP5).
//!
//! The compiler-owned tests moved to `radix-program/src/mir/mir_test.rs`
//! (constants, FNV-1a, manifest helpers, CLI planning, requirements, manifest
//! validation, MIR append/merge). These tests stay faber-side because they
//! exercise faber product surfaces the compiler subset never sees: the
//! `[device]` payload (the `MirSeams::device_section_builder` seam body).

use super::*;
use radix::program::mir::DeviceSelection;

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
