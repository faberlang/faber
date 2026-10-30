use super::run_stats::{
    self, FunctionMetadata, IntrinsicMetadata, PackageMetadata, RunStatsArtifact,
};
use radix_mir_runner::{FunctionStats, IntrinsicStats, RunnerStats};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// Fixture version tracks the faber release; companion to 9c824f051 per
// operator 1.10 release sequence.
const STATS_FIXTURE: &str = r#"
{
  "kind": "faber-run-stats",
  "format_version": 1,
  "generated_at": "2026-08-28T23:00:00Z",
  "faber": { "version": "1.12.0", "commit": null },
  "package": { "name": "fixture", "entry": "src/main.fab" },
  "route": "scena",
  "run": { "wall_ms": 17, "exit": 0 },
  "meter": {
    "clock": "monotonic-ns",
    "durations": { "method": "reservoir", "size": 256, "exact": false },
    "self_time": "child-subtracted via call stack",
    "note": "diagnostic only — never a bench-gate input"
  },
  "functions": [
    {
      "module": "fixture::lib",
      "name": "work",
      "calls": 2,
      "total_ns": 120,
      "self_ns": 80,
      "p50_ns": 50,
      "p95_ns": 70,
      "max_ns": 70
    }
  ],
  "intrinsics": [
    { "name": "convert", "calls": 1, "total_ns": 10, "self_ns": 10, "p95_ns": 10 }
  ]
}
"#;

/// Named-consumer fixture (en package surface): a hot loop function, one
/// runtime textus→int convert-assign, and a `@ cursor` generator call, so the
/// metered artifact carries discovery targets for every row family the meter
/// emits on the CPU route.
const CONSUMER_FIXTURE: &str = r#"
@ cursor { }
fn digits() → int {
    yield 7
}

fn bump(int n) → int {
    return n + 1
}

main {
    mut int total ← 0
    mut int i ← 0
    while i ≺ 12 {
        total ← bump(total)
        i ← i + 1
    }
    mut string raw ← "41"
    mut int parsed ↤ raw
    total ← bump(parsed)
    print "total=", total
    print digits()
}
"#;

fn fixture_artifact() -> RunStatsArtifact {
    serde_json::from_str(STATS_FIXTURE).expect("binding fixture must deserialize")
}

fn faber_binary() -> PathBuf {
    super::format_test::faber_binary()
}

fn write_package(label: &str, source: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix(&format!("faber-run-stats-{label}-"))
        .tempdir()
        .expect("create package directory");
    let src = dir.path().join("src");
    fs::create_dir_all(&src).expect("create source directory");
    fs::write(
        dir.path().join("faber.toml"),
        r#"
[package]
name = "stats-fixture"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write manifest");
    let entry = src.join("main.fab");
    fs::write(&entry, source).expect("write source");
    (dir, entry)
}

fn run_faber(package: &Path, flags: &[&str]) -> Output {
    Command::new(faber_binary())
        .arg("run")
        .args(flags)
        .arg(package)
        .output()
        .expect("run faber")
}

fn framed_artifact(stdout: &[u8]) -> (Vec<u8>, RunStatsArtifact) {
    let marker = format!("{}\n", run_stats::STATS_MARKER);
    let marker = marker.as_bytes();
    let start = stdout
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("stats marker");
    assert_eq!(
        stdout
            .windows(marker.len())
            .filter(|window| *window == marker)
            .count(),
        1,
        "stats marker must occur once"
    );
    let prefix = stdout[..start].to_vec();
    let json = &stdout[start + marker.len()..];
    let mut stream = serde_json::Deserializer::from_slice(json).into_iter::<RunStatsArtifact>();
    let artifact = stream
        .next()
        .expect("one framed JSON object")
        .expect("schema-valid framed JSON");
    assert!(
        stream.next().is_none(),
        "framing must contain one JSON object"
    );
    (prefix, artifact)
}

#[test]
fn schema_round_trip_pins_binding_fields_and_intrinsic_surface() {
    let artifact = fixture_artifact();
    assert_eq!(artifact.kind, "faber-run-stats");
    assert_eq!(artifact.format_version, 1);
    assert_eq!(artifact.generated_at, "2026-08-28T23:00:00Z");
    assert_eq!(artifact.faber.version, "1.12.0");
    assert_eq!(artifact.faber.commit, None);
    assert_eq!(artifact.package.name, "fixture");
    assert_eq!(artifact.package.entry, "src/main.fab");
    assert_eq!(artifact.route, "scena");
    assert_eq!(artifact.run.wall_ms, 17);
    assert_eq!(artifact.run.exit, 0);
    assert_eq!(artifact.meter.clock, "monotonic-ns");
    assert_eq!(artifact.meter.durations.method, "reservoir");
    assert_eq!(artifact.meter.durations.size, 256);
    assert!(!artifact.meter.durations.exact);
    assert_eq!(artifact.meter.self_time, "child-subtracted via call stack");
    assert_eq!(
        artifact.meter.note,
        "diagnostic only — never a bench-gate input"
    );
    assert_eq!(artifact.functions.len(), 1);
    assert_eq!(
        artifact.functions[0],
        FunctionMetadata {
            module: "fixture::lib".to_owned(),
            name: "work".to_owned(),
            calls: 2,
            total_ns: 120,
            self_ns: 80,
            p50_ns: 50,
            p95_ns: 70,
            max_ns: 70,
        }
    );
    assert_eq!(artifact.intrinsics.len(), 1);
    assert_eq!(
        artifact.intrinsics[0],
        IntrinsicMetadata {
            name: "convert".to_owned(),
            calls: 1,
            total_ns: 10,
            self_ns: 10,
            p95_ns: 10,
        }
    );

    let value: Value = serde_json::from_str(STATS_FIXTURE).expect("fixture JSON");
    let mut keys = value["intrinsics"][0]
        .as_object()
        .expect("intrinsic object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys, vec!["calls", "name", "p95_ns", "self_ns", "total_ns"]);
    let encoded = run_stats::encode(&artifact).expect("encode schema");
    let decoded: RunStatsArtifact = serde_json::from_str(&encoded).expect("round trip schema");
    assert_eq!(decoded, artifact);
}

#[test]
fn device_stats_artifact_round_trips_counters_phases_and_entries() {
    let mut artifact = fixture_artifact();
    assert_eq!(artifact.device, None);
    let fixture_encoded = run_stats::encode(&artifact).expect("encode legacy fixture");
    let fixture_value: Value = serde_json::from_str(&fixture_encoded).expect("legacy JSON");
    assert!(fixture_value.get("device").is_none());

    artifact.device = Some(run_stats::DeviceMetadata {
        admit_ns: 11,
        prepare_ns: 12,
        image_build_ns: 13,
        module_compile_ns: 14,
        execution_ns: 15,
        gpu_busy_ns: 18,
        gpu_timing: "per-launch".to_owned(),
        timed_launches: 3,
        untimed_launches: 1,
        gpu_timer_available: Some(true),
        gpu_sample_launch_capacity: Some(1_024),
        host_encode_ns: 16,
        host_blocked_ns: Some(20),
        uploads: 2,
        downloads: 1,
        uploaded_bytes: 64,
        downloaded_bytes: 32,
        runtime: run_stats::DeviceRuntimeMetadata {
            launch_count: 4,
            command_submit_count: 5,
            blocking_wait_count: 6,
            blocking_wait_ns: 17,
            live_modules: 7,
            live_buffers: 8,
            allocated_bytes: 128,
            peak_allocated_bytes: 256,
            host_to_device_bytes: 64,
            device_to_host_bytes: 32,
            span: None,
            gpu_steps: vec![run_stats::DeviceGpuStepMetadata {
                launches: 4,
                timed_launches: 3,
                gpu_busy_ns: 18,
                wall_ns: Some(25),
                idle_ns: Some(7),
            }],
        },
        entries: vec![
            run_stats::DeviceEntryMetadata {
                entry: "quadratum".to_owned(),
                launches: 2,
                gpu_us: 19,
            },
            run_stats::DeviceEntryMetadata {
                entry: "cubus".to_owned(),
                launches: 1,
                gpu_us: 23,
            },
        ],
    });

    let encoded = run_stats::encode(&artifact).expect("encode device artifact");
    let mut value: Value = serde_json::from_str(&encoded).expect("device artifact JSON");
    assert!(value["device"].is_object());
    assert_eq!(value["device"]["entries"][0]["entry"], "quadratum");
    assert_eq!(value["device"]["gpu_busy_ns"], 18);
    assert_eq!(value["device"]["host_encode_ns"], 16);
    assert_eq!(value["device"]["host_blocked_ns"], 20);
    assert_eq!(value["device"]["gpu_timer_available"], true);
    assert_eq!(value["device"]["gpu_sample_launch_capacity"], 1_024);
    assert_eq!(value["device"]["runtime"]["gpu_steps"][0]["idle_ns"], 7);
    assert_eq!(value["device"]["runtime"]["gpu_steps"][0]["wall_ns"], 25);
    let decoded: RunStatsArtifact = serde_json::from_str(&encoded).expect("device schema");
    assert_eq!(decoded, artifact);

    let device = value["device"].as_object_mut().expect("device object");
    device.remove("gpu_timer_available");
    device.remove("gpu_sample_launch_capacity");
    device.remove("host_blocked_ns");
    value["device"]["runtime"]
        .as_object_mut()
        .expect("runtime object")
        .remove("gpu_steps");
    let old_decoded: RunStatsArtifact =
        serde_json::from_value(value).expect("old device artifact without sampling metadata");
    let old_device = old_decoded.device.expect("old device metadata");
    assert_eq!(old_device.gpu_timer_available, None);
    assert_eq!(old_device.gpu_sample_launch_capacity, None);
    assert!(old_device.runtime.gpu_steps.is_empty());
    // An artifact from before the encode/blocked split reads as unsplit: its
    // `host_encode_ns` still includes the waits.
    assert_eq!(old_device.host_blocked_ns, None);
    assert_eq!(old_device.host_encode_ns, 16);
}

#[test]
fn schema_conversion_sorts_function_and_intrinsic_arrays_deterministically() {
    let artifact = RunStatsArtifact::with_generated_at(
        PackageMetadata {
            name: "fixture".to_owned(),
            entry: "src/main.fab".to_owned(),
        },
        RunnerStats {
            functions: vec![
                FunctionStats {
                    module: "z".to_owned(),
                    name: "b".to_owned(),
                    calls: 1,
                    total_ns: 2,
                    self_ns: 2,
                    p50_ns: 2,
                    p95_ns: 2,
                    max_ns: 2,
                },
                FunctionStats {
                    module: "a".to_owned(),
                    name: "z".to_owned(),
                    calls: 1,
                    total_ns: 1,
                    self_ns: 1,
                    p50_ns: 1,
                    p95_ns: 1,
                    max_ns: 1,
                },
            ],
            intrinsics: vec![
                IntrinsicStats {
                    name: "zeta".to_owned(),
                    calls: 1,
                    total_ns: 2,
                    self_ns: 2,
                    p95_ns: 2,
                },
                IntrinsicStats {
                    name: "assert".to_owned(),
                    calls: 1,
                    total_ns: 1,
                    self_ns: 1,
                    p95_ns: 1,
                },
            ],
        },
        0,
        0,
        "2026-08-28T23:00:00Z".to_owned(),
    );

    assert_eq!(
        artifact
            .functions
            .iter()
            .map(|row| (row.module.as_str(), row.name.as_str()))
            .collect::<Vec<_>>(),
        vec![("a", "z"), ("z", "b")]
    );
    assert_eq!(
        artifact
            .intrinsics
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        vec!["assert", "zeta"]
    );
    let first = run_stats::encode(&artifact).expect("encode first artifact");
    let second = run_stats::encode(&artifact).expect("encode second artifact");
    assert_eq!(first, second, "same observations must produce stable JSON");
}

#[test]
fn package_metadata_uses_manifest_name_and_relative_entry() {
    let (dir, entry) = write_package("metadata", "main { print 1 }\n");
    let metadata = run_stats::package_metadata(&entry).expect("package metadata");
    assert_eq!(metadata.name, "stats-fixture");
    assert_eq!(metadata.entry, "src/main.fab");
    assert_eq!(entry, dir.path().join("src/main.fab"));
}

#[test]
fn success_stats_frames_output_and_writes_equivalent_dual_sink_artifact() {
    let (_dir, package) = write_package("success", "main { print 42 }\n");
    let plain = run_faber(&package, &[]);
    assert!(plain.status.success(), "plain run failed: {:?}", plain);
    assert_eq!(
        plain.stdout, b"42\n",
        "the unstatsed run output is the byte-identical baseline"
    );

    let file_dir = tempfile::tempdir().expect("create stats output directory");
    let stats_file = file_dir.path().join("run.json");
    let file_arg = stats_file.to_str().expect("UTF-8 stats path");
    let file_only = run_faber(&package, &["--stats-file", file_arg]);
    assert!(
        file_only.status.success(),
        "file-only stats run failed: {:?}",
        file_only
    );
    assert_eq!(
        file_only.stdout, plain.stdout,
        "file-only stats must preserve ordinary stdout byte-for-byte"
    );
    let file_artifact: RunStatsArtifact =
        serde_json::from_slice(&fs::read(&stats_file).expect("stats file")).expect("file schema");
    assert_eq!(file_artifact.run.exit, 0);
    assert!(!file_artifact.meter.durations.exact);

    let dual_file = file_dir.path().join("dual.json");
    let dual_arg = dual_file.to_str().expect("UTF-8 dual stats path");
    let dual = run_faber(&package, &["--stats", "--stats-file", dual_arg]);
    assert!(dual.status.success(), "dual stats run failed: {dual:?}");
    let (prefix, stdout_artifact) = framed_artifact(&dual.stdout);
    assert_eq!(
        prefix, plain.stdout,
        "stats framing must preserve output prefix"
    );
    let disk_artifact: RunStatsArtifact =
        serde_json::from_slice(&fs::read(&dual_file).expect("dual stats file"))
            .expect("dual file schema");
    assert_eq!(stdout_artifact, disk_artifact);
    assert_eq!(stdout_artifact.package.name, "stats-fixture");
    assert_eq!(stdout_artifact.package.entry, "src/main.fab");
    assert_eq!(stdout_artifact.route, "scena");
    assert_eq!(stdout_artifact.faber.version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn runtime_error_emits_stats_before_cli_failure_for_both_sinks() {
    let (_dir, package) = write_package(
        "runtime-error",
        "main {\n    print \"before\"\n    let list<int> base ← [1, 2]\n    print base[5]\n}\n",
    );
    let output_dir = tempfile::tempdir().expect("create stats output directory");
    let stats_file = output_dir.path().join("runtime.json");
    let stats_arg = stats_file.to_str().expect("UTF-8 stats path");
    let output = run_faber(&package, &["--stats", "--stats-file", stats_arg]);
    assert!(!output.status.success(), "runtime error must fail the CLI");
    let (prefix, stdout_artifact) = framed_artifact(&output.stdout);
    assert_eq!(prefix, b"before\n");
    // The recorded exit is the process status: the program-logic class (70).
    assert_eq!(stdout_artifact.run.exit, 70);
    let disk_artifact: RunStatsArtifact =
        serde_json::from_slice(&fs::read(&stats_file).expect("runtime stats file"))
            .expect("runtime file schema");
    assert_eq!(disk_artifact, stdout_artifact);
    assert!(
        !output.stderr.is_empty(),
        "runtime diagnostic must be reported after stats emission"
    );
}

#[test]
fn pre_execution_and_single_source_failures_emit_no_artifact() {
    let (_compile_dir, compile_package) =
        write_package("compile-error", "main { print missing }\n");
    let output_dir = tempfile::tempdir().expect("create stats output directory");
    let compile_file = output_dir.path().join("compile.json");
    let compile_arg = compile_file.to_str().expect("UTF-8 compile stats path");
    let compile = run_faber(&compile_package, &["--stats", "--stats-file", compile_arg]);
    assert!(!compile.status.success());
    assert!(
        !compile
            .stdout
            .windows(run_stats::STATS_MARKER.len())
            .any(|window| { window == run_stats::STATS_MARKER.as_bytes() })
    );
    assert!(
        !compile_file.exists(),
        "compile failure must emit no artifact"
    );

    let source_dir = tempfile::tempdir().expect("create single-source directory");
    let source = source_dir.path().join("single.fab");
    fs::write(&source, "main { print 1 }\n").expect("write single source");
    let refused_file = source_dir.path().join("single.json");
    for (flags, artifact_path) in [
        (vec!["--stats"], None),
        (
            vec![
                "--stats-file",
                refused_file.to_str().expect("UTF-8 single stats path"),
            ],
            Some(refused_file.as_path()),
        ),
    ] {
        let output = run_faber(&source, &flags);
        assert!(!output.status.success(), "single-source stats must refuse");
        assert!(
            output
                .stderr
                .windows(b"single `.fab` file".len())
                .any(|window| window == b"single `.fab` file"),
            "single-source refusal must be clear: {output:?}"
        );
        assert!(
            !output
                .stdout
                .windows(run_stats::STATS_MARKER.len())
                .any(|window| window == run_stats::STATS_MARKER.as_bytes())
        );
        assert!(
            artifact_path.is_none_or(|path| !path.exists()),
            "single-source refusal must emit no artifact"
        );
    }
}

#[test]
fn named_consumer_proof_discovers_hot_function_conversion_and_cursor_rows() {
    let (_dir, package) = write_package("consumer", CONSUMER_FIXTURE);
    let plain = run_faber(&package, &[]);
    assert!(plain.status.success(), "plain run failed: {plain:?}");

    let stats = run_faber(&package, &["--stats"]);
    assert!(stats.status.success(), "stats run failed: {stats:?}");
    let (prefix, artifact) = framed_artifact(&stats.stdout);
    assert_eq!(
        prefix, plain.stdout,
        "consumer framing must preserve program output"
    );

    // Binding posture on the real artifact: approximate reservoir, verbatim
    // diagnostic-only note, deterministic function ordering.
    assert!(!artifact.meter.durations.exact);
    assert_eq!(
        artifact.meter.note,
        "diagnostic only — never a bench-gate input"
    );
    assert!(artifact.functions.windows(2).all(|pair| {
        (pair[0].module.as_str(), pair[0].name.as_str())
            < (pair[1].module.as_str(), pair[1].name.as_str())
    }));

    // Discovery: the hot loop function (12 iterations + one convert-fed
    // call), the cursor generator (one row per invocation), and the entry
    // frame.
    let bump = artifact
        .functions
        .iter()
        .find(|row| row.name == "bump")
        .expect("hot function row");
    assert_eq!(bump.calls, 13);
    let digits = artifact
        .functions
        .iter()
        .find(|row| row.name == "digits")
        .expect("cursor generator row");
    assert_eq!(digits.calls, 1, "one function row per generator invocation");
    let entry = artifact
        .functions
        .iter()
        .find(|row| row.name == "incipit")
        .expect("entry frame row");
    assert_eq!(entry.calls, 1);
    assert!(
        artifact
            .functions
            .iter()
            .all(|row| row.self_ns <= row.total_ns),
        "self time is child-subtracted, never exceeding total"
    );

    // Discovery: conversion and cursor intrinsic rows; intrinsic rows are
    // higher-order leaves, so self_ns equals total_ns.
    let convert = artifact
        .intrinsics
        .iter()
        .find(|row| row.name == "convert")
        .expect("conversion row from the runtime textus→int convert-assign");
    // Since N4 U5 (1f2ecc1a1) every checked integer store is a `Convert` node
    // and meters under the same `convert` row: 13 `n + 1` returns in `bump`
    // + 12 `i ← i + 1` stores + the one runtime `↤` = 26.
    assert_eq!(convert.calls, 13 + 12 + 1);
    let cursor = artifact
        .intrinsics
        .iter()
        .find(|row| row.name == "cursor_stream")
        .expect("cursor stream row from the generator call");
    assert_eq!(cursor.calls, 1);
    assert!(
        artifact.intrinsics.iter().any(|row| row.name == "cede"),
        "generator yield row"
    );
    assert!(
        artifact
            .intrinsics
            .iter()
            .all(|row| row.self_ns == row.total_ns),
        "intrinsic rows carry the leaf self-time approximation"
    );
}

#[cfg(feature = "device-runtime")]
#[test]
fn device_refusal_is_observed_before_launch_with_no_artifact() {
    const DEVICE_REFUSALS: [&str; 2] = ["E_DEVICE_NONE_ADMITTED", "E_DEVICE_UNAVAILABLE"];

    // Device-bearing fixture (manifest-declared selection): the seam receives
    // the stats request and refuses before any device callback/launch.
    let (dir, package) = write_package("device-refusal", "main { print 1 }\n");
    fs::write(
        dir.path().join("faber.toml"),
        r#"[package]
name = "stats-fixture"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[device]
device = "metal"
"#,
    )
    .expect("write device manifest");

    let output_dir = tempfile::tempdir().expect("create stats output directory");
    let stats_file = output_dir.path().join("refused.json");
    let stats_arg = stats_file.to_str().expect("UTF-8 stats path");
    let output = run_faber(&package, &["--stats", "--stats-file", stats_arg]);
    assert!(
        !output.status.success(),
        "device route + stats request must refuse: {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The explicit device selection fails closed with a device-admission issue
    // before launch, regardless of which admission/no-kernel refusal applies.
    assert!(
        DEVICE_REFUSALS.iter().any(|code| stderr.contains(code)),
        "a device selection with --stats must fail closed with one of {DEVICE_REFUSALS:?}: {stderr}"
    );
    assert!(
        !output
            .stdout
            .windows(run_stats::STATS_MARKER.len())
            .any(|window| window == run_stats::STATS_MARKER.as_bytes())
    );
    assert!(
        !stats_file.exists(),
        "refusal must emit no artifact: {}",
        stats_file.display()
    );
}

/// A spanned logical device (rule 18) reports its partition traffic and
/// launch modes under `device.runtime.span`, with each fallback named; an
/// unspanned device's artifact has no `span` key at all.
#[cfg(feature = "device-runtime")]
#[test]
fn device_span_stats_appear_only_for_a_spanned_device() {
    use radix_device_core::runtime::{SpanEntryStats, SpanFallback, SpanStats};
    let span = SpanStats {
        partitions: 3,
        moved_bytes: 96,
        transfers: 6,
        replica_write_bytes: 32,
        split_launches: 2,
        replicated_launches: 1,
        first_launches: 1,
        syncs: Vec::new(),
        peer_bytes: 64,
        host_staged_bytes: 32,
        entries: vec![
            SpanEntryStats {
                entry: "gradus".to_owned(),
                split_launches: 2,
                ..SpanEntryStats::default()
            },
            SpanEntryStats {
                entry: "summa".to_owned(),
                first_launches: 1,
                fallback: Some(SpanFallback::WritesNotLaneLocal),
                ..SpanEntryStats::default()
            },
        ],
        ..SpanStats::default()
    };
    let runtime = |span| run_stats::DeviceRuntimeMetadata {
        launch_count: 4,
        command_submit_count: 1,
        blocking_wait_count: 1,
        blocking_wait_ns: 0,
        live_modules: 1,
        live_buffers: 2,
        allocated_bytes: 0,
        peak_allocated_bytes: 0,
        host_to_device_bytes: 16,
        device_to_host_bytes: 8,
        span,
        gpu_steps: Vec::new(),
    };
    let spanned = runtime(Some(run_stats::DeviceSpanMetadata::from_stats(&span)));
    let value = serde_json::to_value(&spanned).expect("spanned JSON");
    assert_eq!(value["span"]["moved_bytes"], 96);
    assert_eq!(value["span"]["replica_write_bytes"], 32);
    assert_eq!(value["span"]["replicated_launches"], 1);
    assert_eq!(
        value["span"]["entries"][1]["fallback"],
        "writes-not-lane-local"
    );
    assert!(value["span"]["entries"][0].get("fallback").is_none());
    let decoded: run_stats::DeviceRuntimeMetadata =
        serde_json::from_value(value).expect("spanned schema");
    assert_eq!(decoded, spanned);
    let plain = serde_json::to_value(runtime(None)).expect("plain JSON");
    assert!(plain.get("span").is_none(), "{plain}");
}

/// The planner audit's stats schema (need, U3): per-kind syncs under
/// `span.syncs` and per entry, and one `span.avoidable[]` row per classified
/// (entry, kind) the run synced. Every field name is pinned, and an artifact
/// written before the audit (no `syncs`, no `avoidable`) still decodes.
#[cfg(all(feature = "device-runtime", feature = "runner"))]
#[test]
fn device_span_stats_carry_the_sync_audit_schema() {
    use faber_cli::package::mir::SpanAvoidable;
    use radix_device_core::runtime::{SpanEntryStats, SpanStats, SpanSyncKind, SpanSyncStats};
    let row = |kind, count, bytes, blocked_ns| SpanSyncStats {
        kind,
        count,
        bytes,
        peer_bytes: 0,
        blocked_ns,
    };
    let gather = row(SpanSyncKind::OwnWriteGather, 2, 32, 700);
    let wait = row(SpanSyncKind::HostWaitAll, 1, 0, 300);
    let partials = row(SpanSyncKind::PartialsAllGather, 2, 8, 90);
    let span = SpanStats {
        partitions: 2,
        moved_bytes: 40,
        transfers: 4,
        split_launches: 2,
        syncs: vec![gather, partials, wait],
        entries: vec![
            SpanEntryStats {
                entry: "k1".to_owned(),
                split_launches: 1,
                syncs: vec![gather, wait],
                ..SpanEntryStats::default()
            },
            SpanEntryStats {
                entry: "k2".to_owned(),
                split_launches: 1,
                syncs: vec![partials],
                ..SpanEntryStats::default()
            },
        ],
        ..SpanStats::default()
    };
    let classes = [
        SpanAvoidable {
            entry: "k1".to_owned(),
            kind: "own-write-gather",
            avoidable: "unbounded-read",
            detail: Some("no read set".to_owned()),
        },
        SpanAvoidable {
            entry: "k2".to_owned(),
            kind: "partials-all-gather",
            avoidable: "fusable-with-previous",
            detail: Some("k1".to_owned()),
        },
        // Classified, never synced: no row.
        SpanAvoidable {
            entry: "k1".to_owned(),
            kind: "first-broadcast",
            avoidable: "first-fallback",
            detail: Some("recipe".to_owned()),
        },
    ];
    let metadata = run_stats::DeviceSpanMetadata::from_stats(&span).with_avoidable(&classes);
    let value = serde_json::to_value(&metadata).expect("span JSON");
    assert_eq!(
        value["syncs"],
        serde_json::json!([
            {"kind": "own-write-gather", "count": 2, "bytes": 32, "blocked_ns": 700},
            {"kind": "partials-all-gather", "count": 2, "bytes": 8, "blocked_ns": 90},
            {"kind": "host-wait-all", "count": 1, "bytes": 0, "blocked_ns": 300},
        ])
    );
    assert_eq!(
        value["entries"][0]["syncs"][1],
        serde_json::json!({"kind": "host-wait-all", "count": 1, "bytes": 0, "blocked_ns": 300})
    );
    assert_eq!(
        value["avoidable"],
        serde_json::json!([
            {
                "entry": "k1", "kind": "own-write-gather", "avoidable": "unbounded-read",
                "detail": "no read set", "count": 2, "bytes": 32, "blocked_ns": 700
            },
            {
                "entry": "k2", "kind": "partials-all-gather", "avoidable": "fusable-with-previous",
                "detail": "k1", "count": 2, "bytes": 8, "blocked_ns": 90
            },
        ])
    );
    let decoded: run_stats::DeviceSpanMetadata =
        serde_json::from_value(value).expect("audit schema");
    assert_eq!(decoded, metadata);

    // An artifact from before the audit has neither field.
    let old = serde_json::json!({
        "partitions": 2, "moved_bytes": 0, "transfers": 0, "replica_write_bytes": 0,
        "split_launches": 1, "replicated_launches": 0, "first_launches": 0,
        "entries": [{"entry": "k", "split_launches": 1, "replicated_launches": 0, "first_launches": 0}]
    });
    let old: run_stats::DeviceSpanMetadata = serde_json::from_value(old).expect("old artifact");
    assert!(old.syncs.is_empty() && old.avoidable.is_empty());
    let plain = serde_json::to_value(&old).expect("JSON");
    assert!(plain.get("syncs").is_none() && plain.get("avoidable").is_none());
}

/// The transport audit (need, U5): peer and host-staged bytes apart,
/// the class chosen for every ordered pair with what moved over it, and the
/// device-to-device copies with their measured dependency latency. A sync row
/// carries `peer_bytes` only when some bytes moved device to device, an
/// artifact from before the transport work still decodes, and a run with no
/// peer copy has no `peer` key.
#[cfg(feature = "device-runtime")]
#[test]
fn device_span_stats_carry_the_transport_audit() {
    use radix_device_core::runtime::{
        PeerCopyStats, PeerLinkClass, SpanLinkStats, SpanStats, SpanSyncKind, SpanSyncStats,
    };
    let span = SpanStats {
        partitions: 2,
        moved_bytes: 100,
        transfers: 3,
        peer_bytes: 60,
        host_staged_bytes: 40,
        syncs: vec![SpanSyncStats {
            kind: SpanSyncKind::OwnWriteGather,
            count: 3,
            bytes: 100,
            peer_bytes: 60,
            blocked_ns: 5,
        }],
        links: vec![
            SpanLinkStats {
                from: 0,
                to: 1,
                class: PeerLinkClass::Peer,
                transfers: 2,
                bytes: 60,
            },
            SpanLinkStats {
                from: 1,
                to: 0,
                class: PeerLinkClass::HostStaged,
                transfers: 1,
                bytes: 40,
            },
        ],
        peer: PeerCopyStats {
            copies: 2,
            bytes: 60,
            timed: 2,
            dependency_us: 90,
            max_dependency_us: 70,
        },
        ..SpanStats::default()
    };
    let metadata = run_stats::DeviceSpanMetadata::from_stats(&span);
    let value = serde_json::to_value(&metadata).expect("span JSON");
    assert_eq!(value["peer_bytes"], 60);
    assert_eq!(value["host_staged_bytes"], 40);
    assert_eq!(
        value["links"],
        serde_json::json!([
            {"from": 0, "to": 1, "class": "peer", "transfers": 2, "bytes": 60},
            {"from": 1, "to": 0, "class": "host-staged", "transfers": 1, "bytes": 40},
        ])
    );
    assert_eq!(
        value["peer"],
        serde_json::json!({
            "copies": 2, "bytes": 60, "timed": 2, "dependency_us": 90, "max_dependency_us": 70
        })
    );
    assert_eq!(
        value["syncs"][0],
        serde_json::json!({
            "kind": "own-write-gather", "count": 3, "bytes": 100, "peer_bytes": 60,
            "blocked_ns": 5
        })
    );
    let decoded: run_stats::DeviceSpanMetadata =
        serde_json::from_value(value).expect("transport schema");
    assert_eq!(decoded, metadata);

    let host_only = run_stats::DeviceSpanMetadata::from_stats(&SpanStats {
        partitions: 2,
        ..SpanStats::default()
    });
    let value = serde_json::to_value(&host_only).expect("span JSON");
    assert!(value.get("peer").is_none(), "{value}");
    assert!(value.get("links").is_none(), "{value}");
    // An artifact from before the transport audit has none of the new keys.
    let old = serde_json::json!({
        "partitions": 2, "moved_bytes": 8, "transfers": 1, "replica_write_bytes": 0,
        "split_launches": 1, "replicated_launches": 0, "first_launches": 0,
        "entries": []
    });
    let decoded: run_stats::DeviceSpanMetadata =
        serde_json::from_value(old).expect("pre-transport artifact");
    assert_eq!((decoded.peer_bytes, decoded.host_staged_bytes), (0, 0));
    assert!(decoded.links.is_empty() && decoded.peer.is_none());
}

/// The owned-slab rows (need, S4): `span.capacity` and
/// `span.placement[]` with every field name pinned; a replica row has no
/// `axis`, `align` or `halo_bytes`, a run on a device that does not allocate
/// per partition has no `capacity`, and an artifact from before them decodes.
#[cfg(feature = "device-runtime")]
#[test]
fn device_span_stats_carry_the_capacity_and_placement_rows() {
    use radix_device_core::runtime::{LayoutKind, SpanCapacity, SpanPlacement, SpanStats};
    let span = SpanStats {
        partitions: 2,
        capacity: Some(SpanCapacity {
            sum_budget: 512,
            min_budget: 256,
            replicated_bytes: 64,
            slab_bytes: 384,
            peak_per_partition: vec![288, 288],
        }),
        placement: vec![
            SpanPlacement {
                buffer: 3,
                bytes: 384,
                layout: LayoutKind::Slab,
                axis: Some(0),
                align_rows: Some(8),
                shares: vec![192, 192],
                halo_bytes: 16,
                forced_by: None,
            },
            SpanPlacement {
                buffer: 4,
                bytes: 64,
                layout: LayoutKind::Replica,
                axis: None,
                align_rows: None,
                shares: vec![64, 64],
                halo_bytes: 0,
                forced_by: Some("bump binding 1 (single-lane)".to_owned()),
            },
        ],
        ..SpanStats::default()
    };
    let metadata = run_stats::DeviceSpanMetadata::from_stats(&span);
    let value = serde_json::to_value(&metadata).expect("span JSON");
    assert_eq!(
        value["capacity"],
        serde_json::json!({
            "sum_budget": 512, "min_budget": 256, "replicated_bytes": 64,
            "slab_bytes": 384, "peak_per_partition": [288, 288]
        })
    );
    assert_eq!(
        value["placement"][0],
        serde_json::json!({
            "buffer": 3, "bytes": 384, "layout": "slab", "axis": 0, "align": 8,
            "shares": [192, 192], "halo_bytes": 16
        })
    );
    assert_eq!(
        value["placement"][1],
        serde_json::json!({
            "buffer": 4, "bytes": 64, "layout": "replica", "shares": [64, 64],
            "forced_by": "bump binding 1 (single-lane)"
        })
    );
    let decoded: run_stats::DeviceSpanMetadata =
        serde_json::from_value(value).expect("slab schema");
    assert_eq!(decoded, metadata);

    let plain = serde_json::to_value(run_stats::DeviceSpanMetadata::from_stats(&SpanStats {
        partitions: 2,
        ..SpanStats::default()
    }))
    .expect("span JSON");
    assert!(plain.get("capacity").is_none(), "{plain}");
    assert!(plain.get("placement").is_none(), "{plain}");
    let old = serde_json::json!({
        "partitions": 2, "moved_bytes": 8, "transfers": 1, "replica_write_bytes": 0,
        "split_launches": 1, "replicated_launches": 0, "first_launches": 0,
        "entries": []
    });
    let decoded: run_stats::DeviceSpanMetadata =
        serde_json::from_value(old).expect("pre-slab artifact");
    assert!(decoded.capacity.is_none() && decoded.placement.is_empty());
}
