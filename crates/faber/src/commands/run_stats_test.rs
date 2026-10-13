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
    var int total ← 0
    var int i ← 0
    while i ≺ 12 {
        total ← bump(total)
        i ← i + 1
    }
    var string raw ← "41"
    var int parsed ↤ raw
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
    assert!(dual.status.success(), "dual stats run failed: {:?}", dual);
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
        "main {\n    print \"before\"\n    const list<int> base ← [1, 2]\n    print base[5]\n}\n",
    );
    let output_dir = tempfile::tempdir().expect("create stats output directory");
    let stats_file = output_dir.path().join("runtime.json");
    let stats_arg = stats_file.to_str().expect("UTF-8 stats path");
    let output = run_faber(&package, &["--stats", "--stats-file", stats_arg]);
    assert!(!output.status.success(), "runtime error must fail the CLI");
    let (prefix, stdout_artifact) = framed_artifact(&output.stdout);
    assert_eq!(prefix, b"before\n");
    assert_eq!(stdout_artifact.run.exit, 1);
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
            "single-source refusal must be clear: {:?}",
            output
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
    assert!(plain.status.success(), "plain run failed: {:?}", plain);

    let stats = run_faber(&package, &["--stats"]);
    assert!(stats.status.success(), "stats run failed: {:?}", stats);
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

#[test]
fn device_refusal_is_observed_before_launch_with_no_artifact() {
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
    // Both stable codes are valid pre-launch refusals; which one fires depends
    // on whether the Metal driver registers on the host OS. macOS registers
    // Metal, admits the session, and refuses at the no-kernel image check
    // (`E_DEVICE_NONE_ADMITTED`); a host with no registered Metal runtime
    // refuses at admission (`E_DEVICE_UNAVAILABLE`). Either way the explicit
    // selection fails closed before launch and emits no artifact.
    assert!(
        stderr.contains("E_DEVICE_NONE_ADMITTED") || stderr.contains("E_DEVICE_UNAVAILABLE"),
        "a device selection with --stats must refuse with the stable device code: {stderr}"
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
