//! Pairwise Rust-oracle versus LLVM-host corpus parity.

use super::oracle::{normalize_pairwise_output, RustOracleOutcome};
use super::{common, llvm, llvm_runtime, oracle, rust};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const GAP_LEDGER: &str = include_str!("../../data/llvm_host_gaps.toml");

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessOutcome {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Boundary {
    Frontend,
    Mir,
    Emit,
    Verify,
    Link,
    Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LlvmOutcome {
    Failed { boundary: Boundary, issue: String },
    Ran(ProcessOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Comparison {
    Pass,
    RustNegative,
    Mismatch { boundary: Boundary, issue: String },
}

fn compare_pair(
    oracle: RustOracleOutcome,
    rust: Option<&ProcessOutcome>,
    llvm: Option<&LlvmOutcome>,
    expected_stdout: Option<&[u8]>,
) -> Comparison {
    if !oracle.is_executable() {
        return Comparison::RustNegative;
    }
    let Some(rust) = rust else {
        return Comparison::Mismatch {
            boundary: Boundary::Outcome,
            issue: "rust_oracle_missing".to_owned(),
        };
    };
    let Some(llvm) = llvm else {
        return Comparison::Mismatch {
            boundary: Boundary::Outcome,
            issue: "llvm_outcome_missing".to_owned(),
        };
    };

    match oracle {
        RustOracleOutcome::RunSuccess { exit_code, .. }
        | RustOracleOutcome::DeclarationOnly { exit_code, .. }
        | RustOracleOutcome::ExpectedNonzeroExit { exit_code, .. }
            if rust.exit_code != Some(exit_code) =>
        {
            return Comparison::Mismatch {
                boundary: Boundary::Outcome,
                issue: "rust_oracle_exit_mismatch".to_owned(),
            };
        }
        RustOracleOutcome::ExpectedRuntimeFailure {
            stderr_contains, ..
        } if rust.exit_code == Some(0)
            || !String::from_utf8_lossy(&rust.stderr).contains(stderr_contains) =>
        {
            return Comparison::Mismatch {
                boundary: Boundary::Outcome,
                issue: "rust_oracle_runtime_mismatch".to_owned(),
            };
        }
        _ => {}
    }
    let llvm = match llvm {
        LlvmOutcome::Ran(llvm) => llvm,
        LlvmOutcome::Failed { boundary, issue } => {
            return Comparison::Mismatch {
                boundary: *boundary,
                issue: issue.clone(),
            };
        }
    };

    let rust_stdout = normalize_pairwise_output(&String::from_utf8_lossy(&rust.stdout));
    let llvm_stdout = normalize_pairwise_output(&String::from_utf8_lossy(&llvm.stdout));
    if let Some(expected) = expected_stdout {
        let expected = normalize_pairwise_output(&String::from_utf8_lossy(expected));
        if rust_stdout != expected {
            return Comparison::Mismatch {
                boundary: Boundary::Outcome,
                issue: "rust_fixture_mismatch".to_owned(),
            };
        }
        if llvm_stdout != expected {
            return Comparison::Mismatch {
                boundary: Boundary::Outcome,
                issue: "llvm_fixture_mismatch".to_owned(),
            };
        }
    }
    match oracle {
        RustOracleOutcome::RunSuccess { exit_code, .. }
        | RustOracleOutcome::DeclarationOnly { exit_code, .. }
        | RustOracleOutcome::ExpectedNonzeroExit { exit_code, .. } => {
            if llvm.exit_code != Some(exit_code) {
                return Comparison::Mismatch {
                    boundary: Boundary::Outcome,
                    issue: "exit_code_mismatch".to_owned(),
                };
            }
            if rust_stdout != llvm_stdout {
                return Comparison::Mismatch {
                    boundary: Boundary::Outcome,
                    issue: "stdout_mismatch".to_owned(),
                };
            }
        }
        RustOracleOutcome::ExpectedRuntimeFailure {
            stderr_contains, ..
        } => {
            let stderr = String::from_utf8_lossy(&llvm.stderr);
            if llvm.exit_code == Some(0) || !stderr.contains(stderr_contains) {
                return Comparison::Mismatch {
                    boundary: Boundary::Outcome,
                    issue: "runtime_failure_mismatch".to_owned(),
                };
            }
        }
        RustOracleOutcome::ExpectedCompileFailure { .. }
        | RustOracleOutcome::ExplicitWrongLane { .. } => unreachable!(),
    }
    Comparison::Pass
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapLedger {
    schema_version: u32,
    measured: String,
    rust_executable_denominator: usize,
    gap_ceiling: usize,
    gap: Vec<GapRow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapRow {
    path: String,
    owner_stage: u8,
    boundary: Boundary,
    issue: String,
    reason: String,
    first_seen: String,
}

fn parse_gap_ledger(source: &str) -> Result<GapLedger, String> {
    let ledger: GapLedger = toml::from_str(source).map_err(|error| error.to_string())?;
    if ledger.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {}",
            ledger.schema_version
        ));
    }
    if ledger.measured.trim().is_empty() {
        return Err("measured must not be empty".to_owned());
    }
    if ledger.gap.len() > ledger.gap_ceiling {
        return Err(format!(
            "gap count {} exceeds ceiling {}",
            ledger.gap.len(),
            ledger.gap_ceiling
        ));
    }
    let mut paths = BTreeSet::new();
    for row in &ledger.gap {
        if !paths.insert(&row.path) {
            return Err(format!("duplicate gap path {}", row.path));
        }
        if !owner_matches_boundary(row) {
            return Err(format!(
                "gap {} maps {:?} to wrong owner stage {}",
                row.path, row.boundary, row.owner_stage
            ));
        }
        if row.issue.trim().is_empty()
            || row.reason.trim().is_empty()
            || row.first_seen.trim().is_empty()
        {
            return Err(format!("gap {} has empty required metadata", row.path));
        }
    }
    Ok(ledger)
}

fn owner_matches_boundary(row: &GapRow) -> bool {
    match row.boundary {
        Boundary::Frontend => row.owner_stage == 3,
        Boundary::Mir => {
            row.owner_stage == 3
                || (row.owner_stage == 8 && row.issue == "cli_runtime_record_pending")
        }
        Boundary::Emit | Boundary::Verify => row.owner_stage == 4,
        Boundary::Link => matches!(row.owner_stage, 4 | 8),
        Boundary::Outcome => matches!(row.owner_stage, 5 | 8),
    }
}

fn validate_gap_rows(
    corpus_root: &Path,
    comparisons: &BTreeMap<String, Comparison>,
    ledger: &GapLedger,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let rows = ledger
        .gap
        .iter()
        .map(|row| (row.path.as_str(), row))
        .collect::<BTreeMap<_, _>>();

    for row in &ledger.gap {
        if !corpus_root.join(&row.path).is_file() {
            errors.push(format!("unknown gap path {}", row.path));
        }
        match comparisons.get(&row.path) {
            Some(Comparison::Mismatch { boundary, issue }) => {
                if *boundary != row.boundary || issue != &row.issue {
                    errors.push(format!(
                        "wrong live mapping {}: ledger {:?}/{} live {:?}/{}",
                        row.path, row.boundary, row.issue, boundary, issue
                    ));
                }
            }
            Some(Comparison::Pass) => errors.push(format!("stale gap {}", row.path)),
            Some(Comparison::RustNegative) => {
                errors.push(format!("gap {} is Rust-negative", row.path))
            }
            None => errors.push(format!("gap {} has no pairwise result", row.path)),
        }
    }
    for (path, comparison) in comparisons {
        if matches!(comparison, Comparison::Mismatch { .. }) && !rows.contains_key(path.as_str()) {
            errors.push(format!("unexpected untracked mismatch {path}"));
        }
    }
    if comparisons
        .values()
        .filter(|comparison| !matches!(comparison, Comparison::RustNegative))
        .count()
        != ledger.rust_executable_denominator
    {
        errors.push(format!(
            "Rust executable denominator differs from ledger {}",
            ledger.rust_executable_denominator
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[derive(Debug)]
struct RustJob {
    index: usize,
    path: PathBuf,
    relative: String,
    package: String,
    member: String,
}

fn build_rust_lane(
    corpus_root: &Path,
    paths: &[PathBuf],
    temp_root: &Path,
) -> BTreeMap<String, ProcessOutcome> {
    let compiler = radix::Compiler::new(radix::Config::default());
    let target = common::shared_target_dir(temp_root);
    let mut jobs = Vec::new();

    for (index, path) in paths.iter().enumerate() {
        if !oracle::rust_oracle(path).is_executable() {
            continue;
        }
        let relative = path
            .strip_prefix(corpus_root)
            .expect("corpus path must be relative to corpus root")
            .to_string_lossy()
            .into_owned();
        let code = rust::compile_rust_exemplum(&compiler, path, corpus_root)
            .unwrap_or_else(|reason| panic!("Rust oracle compile failed for {relative}: {reason}"));
        let code =
            radix::tool::format_generated_code(radix::codegen::Target::Rust, &code).unwrap_or(code);
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("exemplum");
        let member = format!("rust-{index:03}-{stem}");
        let package = format!("parity_{index:03}_{stem}").replace('-', "_");
        common::write_rust_workspace_member(
            &temp_root.join(&member),
            &package,
            &rust::rust_member_code(path, &code),
        );
        jobs.push(RustJob {
            index,
            path: path.clone(),
            relative,
            package,
            member,
        });
    }

    let members = jobs
        .iter()
        .map(|job| job.member.clone())
        .collect::<Vec<_>>();
    let manifest = common::write_rust_workspace_root(temp_root, &members);
    let mut build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest)
        .env("CARGO_TARGET_DIR", &target);
    let status = common::command_status_with_timeout(&mut build, Duration::from_secs(900))
        .expect("cannot execute batched Rust oracle build");
    assert!(status.success(), "batched Rust oracle build failed");

    jobs.into_iter()
        .map(|job| {
            let mut command = Command::new(target.join(format!("debug/{}", job.package)));
            command.args(oracle::rust_oracle(&job.path).run_args());
            let output = common::command_output_with_timeout(&mut command, Duration::from_secs(10))
                .unwrap_or_else(|error| panic!("cannot run Rust oracle {}: {error}", job.relative));
            eprintln!("[llvm-host parity rust {:03}] {}", job.index, job.relative);
            (
                job.relative,
                ProcessOutcome {
                    exit_code: output.status.code(),
                    stdout: output.stdout,
                    stderr: output.stderr,
                },
            )
        })
        .collect()
}

fn build_llvm_lane(
    corpus_root: &Path,
    paths: &[PathBuf],
    temp_root: &Path,
) -> BTreeMap<String, LlvmOutcome> {
    let toolchain = llvm::detect_llvm_toolchain();
    assert!(
        toolchain.is_available(),
        "LLVM host parity requires llvm-as or opt; no verifier toolchain was found"
    );
    let candidates = paths
        .iter()
        .enumerate()
        .filter(|(_, path)| oracle::rust_oracle(path).is_executable())
        .map(|(index, path)| (index, path.clone()))
        .collect::<Vec<_>>();
    std::fs::create_dir_all(temp_root).expect("cannot create LLVM pairwise temp root");
    let workers = std::thread::available_parallelism()
        .map(|count| count.get().min(4))
        .unwrap_or(4);
    let next = AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::with_capacity(candidates.len()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let next = &next;
            let results = &results;
            let candidates = &candidates;
            let toolchain = &toolchain;
            scope.spawn(move || loop {
                let slot = next.fetch_add(1, Ordering::Relaxed);
                let Some((index, path)) = candidates.get(slot) else {
                    break;
                };
                let session = radix::driver::Session::new(
                    radix::Config::default().with_target(radix::codegen::Target::LlvmText),
                );
                let result =
                    llvm::classify_llvm_exemplum(&session, path, *index, temp_root, toolchain);
                let relative = path
                    .strip_prefix(corpus_root)
                    .expect("corpus path")
                    .to_string_lossy()
                    .into_owned();
                eprintln!("[llvm-host parity llvm {index:03}] {relative}");
                results
                    .lock()
                    .expect("LLVM result mutex poisoned")
                    .push((relative, llvm_pair_outcome(result)));
            });
        }
    });
    results
        .into_inner()
        .expect("LLVM result mutex poisoned")
        .into_iter()
        .collect()
}

fn llvm_pair_outcome(result: llvm::LlvmE2eResult) -> LlvmOutcome {
    if let Some(probe) = result.run_probe {
        match probe.bucket {
            llvm_runtime::LlvmRunBucket::Runnable
            | llvm_runtime::LlvmRunBucket::OutputMatched
            | llvm_runtime::LlvmRunBucket::RunFailed => {
                return LlvmOutcome::Ran(ProcessOutcome {
                    exit_code: probe.exit_code,
                    stdout: probe.stdout.into_bytes(),
                    stderr: probe.stderr.into_bytes(),
                });
            }
            llvm_runtime::LlvmRunBucket::ToolchainMissing => {
                panic!("LLVM host toolchain unavailable: {}", probe.reason)
            }
            llvm_runtime::LlvmRunBucket::LinkFailed => {
                let issue = if probe.reason.contains("__faber_program_entry_v1") {
                    "entry_or_package_link_missing"
                } else {
                    "runtime_import_unresolved"
                };
                return LlvmOutcome::Failed {
                    boundary: Boundary::Link,
                    issue: issue.to_owned(),
                };
            }
        }
    }
    let (boundary, issue) = match result.bucket {
        llvm::LlvmEmissionBucket::FrontendFailed => (Boundary::Frontend, "frontend_rejected"),
        llvm::LlvmEmissionBucket::MirLoweringFailed
            if result.reason.contains("CLI runtime record binding pending") =>
        {
            (Boundary::Mir, "cli_runtime_record_pending")
        }
        llvm::LlvmEmissionBucket::MirLoweringFailed => (Boundary::Mir, "mir_lowering_rejected"),
        llvm::LlvmEmissionBucket::Unsupported | llvm::LlvmEmissionBucket::EmissionFailed => {
            (Boundary::Emit, "llvm_emission_unsupported")
        }
        llvm::LlvmEmissionBucket::VerifierFailed => (Boundary::Verify, "llvm_verifier_rejected"),
        _ => panic!(
            "unclassified LLVM result for {}: {}",
            result.path.display(),
            result.reason
        ),
    };
    LlvmOutcome::Failed {
        boundary,
        issue: issue.to_owned(),
    }
}

#[test]
#[ignore = "slow pairwise host parity; run: cargo test -p exempla --test e2e_harness exempla_llvm_host_parity -- --ignored --nocapture"]
fn exempla_llvm_host_parity() {
    let corpus_root = crate::paths::corpus_dir();
    let paths = common::collect_exempla_files(&corpus_root);
    let ledger = parse_gap_ledger(GAP_LEDGER).expect("checked-in LLVM host gap ledger must parse");
    let temp_root = common::make_temp_root().join("llvm-host-parity");
    std::fs::create_dir_all(&temp_root).expect("cannot create pairwise temp root");
    let rust = build_rust_lane(&corpus_root, &paths, &temp_root.join("rust"));
    let llvm = build_llvm_lane(&corpus_root, &paths, &temp_root.join("llvm"));
    let comparisons = paths
        .iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&corpus_root)
                .expect("corpus path")
                .to_string_lossy()
                .into_owned();
            let expected = std::fs::read(path.with_extension("expected")).ok();
            let comparison = compare_pair(
                oracle::rust_oracle(path),
                rust.get(&relative),
                llvm.get(&relative),
                expected.as_deref(),
            );
            (relative, comparison)
        })
        .collect::<BTreeMap<_, _>>();
    let parity = comparisons
        .values()
        .filter(|value| matches!(value, Comparison::Pass))
        .count();
    let gaps = comparisons
        .values()
        .filter(|value| matches!(value, Comparison::Mismatch { .. }))
        .count();
    let denominator = comparisons
        .values()
        .filter(|value| !matches!(value, Comparison::RustNegative))
        .count();
    println!("LLVM host parity: P={parity}/{denominator}, G={gaps}, unclassified=0, stale=0");
    for boundary in [
        Boundary::Frontend,
        Boundary::Mir,
        Boundary::Emit,
        Boundary::Verify,
        Boundary::Link,
        Boundary::Outcome,
    ] {
        let count = comparisons.values().filter(|value| matches!(value, Comparison::Mismatch { boundary: live, .. } if *live == boundary)).count();
        println!("  {boundary:?}: {count}");
    }
    if let Err(errors) = validate_gap_rows(&corpus_root, &comparisons, &ledger) {
        panic!("LLVM host gap ledger violations:\n{}", errors.join("\n"));
    }
}

#[cfg(test)]
#[path = "llvm_host_test.rs"]
mod tests;
