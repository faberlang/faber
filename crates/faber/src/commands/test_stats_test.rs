use super::test_stats::{self, TestStatsArtifact};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn faber_binary() -> PathBuf {
    super::format_test::faber_binary()
}

fn write_package(label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix(&format!("faber-test-stats-{label}-"))
        .tempdir()
        .expect("create package directory");
    let src = dir.path().join("src");
    fs::create_dir_all(&src).expect("create source directory");
    fs::write(
        dir.path().join("faber.toml"),
        r#"
[package]
name = "stats-test-fixture"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[locale]
locale = "en"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r#"+++
locale = "en"
+++
main {
}
"#,
    )
    .expect("write source");
    fs::write(
        src.join("main.proba"),
        r#"+++
locale = "en"
+++
test "adds" {
    assert 1 + 1 ≡ 2
}
test "counts" {
    mut int i ← 0
    while i ≺ 8 {
        i ← i + 1
    }
    assert i ≡ 8
}
test "parked" skip "not a unit fixture" {
    assert 1 ≡ 2
}
"#,
    )
    .expect("write tests");
    (dir, src.join("math.proba"))
}

fn run_faber_test(package: &Path, flags: &[&str]) -> Output {
    Command::new(faber_binary())
        .arg("test")
        .args(flags)
        .arg(package)
        .output()
        .expect("run faber test")
}

fn framed_artifact(stdout: &[u8]) -> (Vec<u8>, TestStatsArtifact) {
    let marker = format!("{}\n", test_stats::STATS_MARKER);
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
    let mut stream = serde_json::Deserializer::from_slice(json).into_iter::<TestStatsArtifact>();
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

/// Collapse per-case durations (`(N ms)`) so report comparison tolerates
/// scheduler jitter while every other stdout byte must still match.
fn normalize_report(stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(stdout);
    let mut out = String::with_capacity(text.len());
    let mut rest = &*text;
    while let Some(end) = rest.find(" ms)") {
        let (head, tail) = rest.split_at(end);
        let digits = head
            .len()
            .saturating_sub(head.trim_end_matches(|c: char| c.is_ascii_digit()).len());
        out.push_str(&head[..head.len() - digits]);
        out.push_str("<d> ms)");
        rest = &tail[" ms)".len()..];
    }
    out.push_str(rest);
    out
}

#[test]
fn success_stats_frames_after_report_and_writes_equivalent_file() {
    let (dir, _) = write_package("ok");
    let baseline = run_faber_test(dir.path(), &[]);
    assert!(
        baseline.status.success(),
        "unstatsed test failed: {baseline:?}"
    );

    let file_dir = tempfile::tempdir().expect("create stats output directory");
    let stats_file = file_dir.path().join("test.json");
    let file_arg = stats_file.to_str().expect("UTF-8 stats path");
    let file_only = run_faber_test(dir.path(), &["--stats-file", file_arg]);
    assert!(
        file_only.status.success(),
        "file-only stats test failed: {file_only:?}"
    );
    assert_eq!(
        normalize_report(&file_only.stdout),
        normalize_report(&baseline.stdout),
        "file-only stats must preserve ordinary stdout except per-case durations"
    );
    let file_artifact: TestStatsArtifact =
        serde_json::from_slice(&fs::read(&stats_file).expect("stats file")).expect("file schema");
    assert_eq!(file_artifact.kind, "faber-test-stats");
    assert_eq!(file_artifact.package.name, "stats-test-fixture");
    assert_eq!(file_artifact.run.passed, 2);
    assert_eq!(file_artifact.run.skipped, 1);
    assert_eq!(file_artifact.run.failed, 0);
    assert_eq!(file_artifact.run.exit, 0);

    let dual_file = file_dir.path().join("dual.json");
    let dual_arg = dual_file.to_str().expect("UTF-8 dual stats path");
    let dual = run_faber_test(dir.path(), &["--stats", "--stats-file", dual_arg]);
    assert!(dual.status.success(), "dual stats test failed: {dual:?}");
    let (prefix, stdout_artifact) = framed_artifact(&dual.stdout);
    assert_eq!(
        normalize_report(&prefix),
        normalize_report(&baseline.stdout),
        "stats framing must preserve report prefix except per-case durations"
    );
    let dual_file_artifact: TestStatsArtifact =
        serde_json::from_slice(&fs::read(&dual_file).expect("dual stats file"))
            .expect("dual file schema");
    assert_eq!(stdout_artifact.kind, dual_file_artifact.kind);
    assert_eq!(stdout_artifact.package, dual_file_artifact.package);
    assert_eq!(stdout_artifact.run.passed, dual_file_artifact.run.passed);
    assert_eq!(stdout_artifact.cases.len(), 3);

    let counts = stdout_artifact
        .cases
        .iter()
        .find(|case| case.name == "counts")
        .expect("counts case");
    assert_eq!(counts.status, "passed");
    assert!(
        !counts.functions.is_empty(),
        "executed case should carry function rows: {counts:?}"
    );
    let parked = stdout_artifact
        .cases
        .iter()
        .find(|case| case.name == "parked")
        .expect("parked case");
    assert_eq!(parked.status, "skipped");
    assert_eq!(
        parked.functions,
        [] as [super::run_stats::FunctionMetadata; 0]
    );
    assert_eq!(
        parked.intrinsics,
        [] as [super::run_stats::IntrinsicMetadata; 0]
    );
}

#[test]
fn report_comparison_collapses_only_durations() {
    let zero_ms = b"ok   src/main.fab (0 ms)\nskip src/main.fab (not a unit fixture; 0 ms)\n";
    let one_ms = b"ok   src/main.fab (1 ms)\nskip src/main.fab (not a unit fixture; 2 ms)\n";
    assert_eq!(
        normalize_report(zero_ms),
        normalize_report(one_ms),
        "same report with different durations must compare equal"
    );
    let failed = b"ok   src/main.fab (0 ms)\nFAIL src/main.fab (0 ms)\n";
    assert_ne!(
        normalize_report(zero_ms),
        normalize_report(failed),
        "non-timing stdout change must still fail the comparison"
    );
}

#[test]
fn list_with_stats_is_refused_without_artifact() {
    let (dir, _) = write_package("list");
    let output_dir = tempfile::tempdir().expect("create stats output directory");
    let stats_file = output_dir.path().join("list.json");
    let stats_arg = stats_file.to_str().expect("UTF-8 stats path");
    let output = run_faber_test(
        dir.path(),
        &["--list", "--stats", "--stats-file", stats_arg],
    );
    assert!(
        !output.status.success(),
        "list + stats must refuse: {output:?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("test statistics require executing cases"),
        "refusal must name the list/stats incompatibility: {stderr}"
    );
    assert!(
        !output
            .stdout
            .windows(test_stats::STATS_MARKER.len())
            .any(|window| window == test_stats::STATS_MARKER.as_bytes()),
        "refusal must emit no stdout stats"
    );
    assert!(
        !stats_file.exists(),
        "refusal must emit no stats file: {}",
        stats_file.display()
    );
}

#[test]
fn unstatsed_test_emits_no_marker() {
    let (dir, _) = write_package("plain");
    let output = run_faber_test(dir.path(), &[]);
    assert!(output.status.success(), "plain test failed: {output:?}");
    assert!(
        !output
            .stdout
            .windows(test_stats::STATS_MARKER.len())
            .any(|window| window == test_stats::STATS_MARKER.as_bytes()),
        "unstatsed test must not frame stats"
    );
}
