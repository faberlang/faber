//! CUDA spanning (need, U3): an A2 solve at N = 2 on private copies of
//! the one lowest-ordinal GPU, with the `--stats-file` artifact the CLI would
//! write. The one-device private-copies runtime is Rust API only
//! (`radix_device_cuda::system_private_copies_runtime`; no CLI spelling), so
//! this test builds the artifact through the same `DeviceMetadata::from_receipt`
//! path `faber run --stats-file` uses.
//!
//! The package, its arguments and the output file come from the environment
//! (`FABER_SPAN_A2_PACKAGE`, `FABER_SPAN_A2_ARGS` split on `;`,
//! `FABER_SPAN_STATS_FILE`); without the package variable the test skips, so
//! it never runs by accident. `FABER_REQUIRE_CUDA` turns a missing device into
//! a failure.

use super::run_stats::{self, ColdStartMetadata, DeviceMetadata, RunStatsArtifact};
use faber_cli::package::mir::{CpuDeviceShape, run_package_on_device_session_shaped};
use radix::module::driver::Config;
use radix::program::mir::DeviceSelection;
use radix_device_core::registry::DeviceRegistry;
use radix_mir_runner::BufferHost;
use std::num::NonZeroU32;
use std::path::PathBuf;

#[test]
fn a2_at_two_private_partitions_writes_the_span_stats_file() {
    let Some(package) = std::env::var_os("FABER_SPAN_A2_PACKAGE").map(PathBuf::from) else {
        eprintln!("FABER_SPAN_A2_PACKAGE unset: skipped");
        return;
    };
    let args: Vec<String> = std::env::var("FABER_SPAN_A2_ARGS")
        .unwrap_or_default()
        .split(';')
        .filter(|arg| !arg.is_empty())
        .map(str::to_owned)
        .collect();
    let stats_file = PathBuf::from(
        std::env::var_os("FABER_SPAN_STATS_FILE").expect("FABER_SPAN_STATS_FILE names the output"),
    );
    let partitions: u32 = std::env::var("FABER_SPAN_PARTITIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2);
    if !radix_device_cuda::driver_library_present() {
        assert!(
            std::env::var_os("FABER_REQUIRE_CUDA").is_none(),
            "FABER_REQUIRE_CUDA is set but no CUDA driver library is present"
        );
        return;
    }
    let spanning = radix_device_cuda::system_private_copies_runtime()
        .expect("the private-copies runtime opens");
    let mut registry = DeviceRegistry::new();
    registry.register(Box::new(spanning));

    let mut host = BufferHost::with_argumenta(args);
    let started = std::time::Instant::now();
    let run = run_package_on_device_session_shaped(
        &Config::default(),
        &package,
        DeviceSelection::Cuda,
        &mut registry,
        &mut host,
        true,
        CpuDeviceShape {
            workers: None,
            partitions: NonZeroU32::new(partitions).expect("positive partition count"),
        },
    )
    .unwrap_or_else(|failure| {
        panic!(
            "{} on {partitions} partitions: {failure:?}",
            package.display()
        )
    });
    let wall_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    assert!(
        run.receipt.contains(&format!("{partitions} partitions")),
        "admitted as {partitions} partitions: {}",
        run.receipt
    );
    let metadata = run_stats::package_metadata(&package).expect("package metadata");
    let runner_stats = run.stats.expect("host stats were collected");
    let phases = run.phases;
    let mut artifact = RunStatsArtifact::new(metadata, runner_stats, wall_ms, 0).with_cold_start(
        ColdStartMetadata {
            image_build_ns: phases.image_build_ns,
            emission_ns: phases.image_emission_ns,
            ptx_compile_ns: phases.image_ptx_compile_ns,
            module_compile_ns: phases.module_compile_ns,
            execution_ns: phases.execution_ns,
        },
    );
    artifact.device = Some(DeviceMetadata::from_receipt(&run.device));
    let text = run_stats::encode(&artifact).expect("encode the artifact");
    std::fs::write(&stats_file, format!("{text}\n")).expect("write the stats file");
    eprintln!(
        "span-int: wall_ms={wall_ms} stdout_lines={}",
        host.stdout_lines.len()
    );
    for line in host.stdout_lines.iter().take(3) {
        eprintln!("span-int: out: {line}");
    }
}
