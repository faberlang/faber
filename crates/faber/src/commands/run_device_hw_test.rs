//! The fixed multi-kernel fixture's product device receipt, on the fake
//! runtime and on a physical Metal device: three launches of two kernels
//! (`quadratum`, `cubus`, `quadratum`) over one 8-byte upload and one commit
//! at the print. A warm re-execution is visible against the setup phases.
//!
//! Skips the physical test without Metal.

use radix::program::mir::DeviceSelection;
use radix_device_core::registry::DeviceRegistry;
use radix_device_metal::{FakeMetalDriver, MetalDeviceInfo, MetalRuntime};
use radix_mir_runner::BufferHost;
use std::path::PathBuf;

fn fake_registry(times: Vec<(u64, u64)>) -> DeviceRegistry {
    let mut runtime = MetalRuntime::new(FakeMetalDriver::with_devices(vec![MetalDeviceInfo {
        name: "fake metal".into(),
        registry_id: 77,
        api_total_bytes: 96_000_000,
        max_threads_per_workgroup: 768,
        workgroup_shared_memory_min_bytes: 24_576,
        workgroup_shared_memory_max_bytes: 24_576,
        collective_width: 16,
        unified_memory: false,
    }]));
    runtime.driver_mut().set_last_step_gpu_times(times);
    let mut registry = DeviceRegistry::new();
    registry.register(Box::new(runtime));
    registry
}

fn entry_for(name: &str, source: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("faber-run-device-hw-{name}-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).expect("temp dir");
    let entry = root.join("src").join("main.fab");
    std::fs::write(&entry, source).expect("write source");
    entry
}

fn config() -> radix::module::driver::Config {
    radix::module::driver::Config::default()
        .with_target(radix::module::codegen::Target::MirFmirBinary)
}

const MULTI_KERNEL: &str = r#"@ kernel
@ public { }
fn quadratum(tf32[2] x) → tf32[2] {
    return x ⊙ x
}

@ kernel
@ public { }
fn cubus(tf32[2] x) → tf32[2] {
    return x ⊙ x ⊙ x
}

main {
    print "salve"
    let list<f32> data ← [1.0, 2.0]
    let tensor<f32, [2]> t0 ← data ↦ tensor<f32, [2]>
    let tensor<f32, [2]> t1 ← quadratum(t0)
    let tensor<f32, [2]> t2 ← cubus(t1)
    let tensor<f32, [2]> t3 ← quadratum(t2)
    print t3
}
"#;

#[test]
fn fixed_multi_kernel_fixture_verifies_counts_on_the_fake_runtime() {
    let entry = entry_for("fake-multi", MULTI_KERNEL);
    let stats_file = entry.with_file_name("stats.json");
    let mut registry = fake_registry(vec![(0, 5), (5, 7), (12, 9)]);
    let mut host = BufferHost::default();
    let mut out = Vec::new();
    super::run_on_device_session_with(
        &config(),
        &entry,
        DeviceSelection::Metal,
        &mut registry,
        &mut host,
        true,
        Some(&stats_file),
        &mut out,
    )
    .unwrap_or_else(|d| panic!("device run failed: {d:?}"));
    assert_eq!(
        String::from_utf8(out).expect("utf8"),
        "device: dispatched (measured)\n"
    );
    let json = std::fs::read_to_string(&stats_file).expect("stats file");
    let artifact: super::RunStatsArtifact =
        serde_json::from_str(&json).expect("device stats schema");
    let device = artifact.device.expect("device receipt in stats artifact");

    assert_eq!(device.runtime.launch_count, 3);
    assert_eq!(device.uploads, 1);
    assert_eq!(device.uploaded_bytes, 8);
    assert!(device.downloads >= 1, "{:?}", device.downloads);
    assert!(
        device.downloaded_bytes >= 8,
        "{:?}",
        device.downloaded_bytes
    );
    assert_eq!(device.timed_launches, 3);
    assert_eq!(device.untimed_launches, 0);
    assert_eq!(device.gpu_timing, "per-launch");

    // WHY: the Metal artifact names its kernels by emission order (`k0`,
    // `k1`), not by the Faber function, so the per-entry rows carry the
    // emitted entries (see `package::mir::auto_device_test`); `quadratum` is
    // the first and third launch, `cubus` the second.
    let rows = device
        .entries
        .iter()
        .map(|row| (row.entry.as_str(), row.launches, row.gpu_us))
        .collect::<Vec<_>>();
    assert_eq!(rows, vec![("k0", 2, 14), ("k1", 1, 7)]);

    assert!(device.admit_ns > 0);
    assert!(device.prepare_ns > 0);
    assert!(device.image_build_ns > 0);
    assert!(device.module_compile_ns > 0);
    assert!(device.execution_ns > 0);
    assert!(device.host_encode_ns > 0);
    assert!(device.host_blocked_ns.is_some());
}

#[test]
fn fixed_multi_kernel_fixture_reports_the_product_receipt_on_metal() {
    if radix_device_metal::system_devices().is_empty() {
        eprintln!("no Metal device enumerated; skipping the physical receipt test");
        return;
    }
    let entry = entry_for("metal-multi", MULTI_KERNEL);
    let stats_file = entry.with_file_name("stats.json");
    let mut registry = faber_cli::package::mir::system_device_registry();
    let mut host = BufferHost::default();
    let mut out = Vec::new();
    super::run_on_device_session_with(
        &config(),
        &entry,
        DeviceSelection::Metal,
        &mut registry,
        &mut host,
        true,
        Some(&stats_file),
        &mut out,
    )
    .unwrap_or_else(|d| panic!("device run failed: {d:?}"));
    assert_eq!(
        String::from_utf8(out).expect("utf8"),
        "device: dispatched (measured)\n"
    );
    let json = std::fs::read_to_string(&stats_file).expect("stats file");
    let artifact: super::RunStatsArtifact =
        serde_json::from_str(&json).expect("device stats schema");
    let device = artifact.device.expect("device receipt in stats artifact");

    assert_eq!(device.runtime.launch_count, 3);
    assert_eq!(device.uploads, 1);
    assert_eq!(device.uploaded_bytes, 8);
    assert!(device.downloads >= 1, "{:?}", device.downloads);
    assert!(
        device.downloaded_bytes >= 8,
        "{:?}",
        device.downloaded_bytes
    );
    assert!(device.runtime.command_submit_count >= 1);
    assert!(device.runtime.blocking_wait_count >= 1);
    // A finished run has released its live buffers; the high-water mark is
    // what proves allocation happened, and it never drops below the live set.
    assert!(device.runtime.peak_allocated_bytes > 0);
    assert!(
        device.runtime.peak_allocated_bytes >= device.runtime.allocated_bytes,
        "peak {:?} below live {:?}",
        device.runtime.peak_allocated_bytes,
        device.runtime.allocated_bytes
    );

    // Every Metal device samples timestamps at encoder boundaries (Apple GPUs)
    // or dispatch boundaries (discrete GPUs), so the product receipt carries
    // per-entry GPU time: the field is true, every launch is timed, and the
    // entries table is not empty (need: the M5 Max reported false).
    assert_eq!(device.gpu_timer_available, Some(true));
    assert_eq!(device.timed_launches, 3);
    assert_eq!(device.untimed_launches, 0);
    assert!(!device.entries.is_empty(), "{:?}", device.entries);
    let launches = |name: &str| {
        device
            .entries
            .iter()
            .find(|row| row.entry == name)
            .unwrap_or_else(|| panic!("missing entry {name}: {:?}", device.entries))
    };
    // Emission order: `quadratum` is `k0` (two launches), `cubus` `k1`.
    let quadratum = launches("k0");
    let cubus = launches("k1");
    assert_eq!(quadratum.launches, 2);
    assert_eq!(cubus.launches, 1);
    assert!(quadratum.gpu_us > 0, "{:?}", quadratum);
    assert!(cubus.gpu_us > 0, "{:?}", cubus);

    assert!(device.admit_ns > 0);
    assert!(device.prepare_ns > 0);
    assert!(device.image_build_ns > 0);
    assert!(device.module_compile_ns > 0);
    assert!(device.execution_ns > 0);
    assert!(device.host_encode_ns > 0);
    assert!(device.host_blocked_ns.is_some());
}
