use super::common::{
    cargo_available, collect_exempla_files, command_available, format_count_floor_line,
    format_tier_line, make_temp_root, write_rust_workspace_member, write_rust_workspace_root,
};
use radix::{Compiler, Config};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const EMIT_OK_FLOOR: usize = 271;
const RUSTFMT_CLEAN_FLOOR: usize = 133;
const RUSTFMT_FORMAT_OK_FLOOR: usize = 262;
const CLIPPY_CHECKED_FLOOR: usize = 240;
const CLIPPY_CLEAN_FLOOR: usize = 39;
const VALOR_HELPER_FILES_MAX: usize = 22;
const VALOR_HELPER_MARKER: &str = "use faber::Valor as valor";

struct CanonicalResult {
    path: PathBuf,
    emit_ok: bool,
    valor_helper: bool,
    rustfmt_clean: Option<bool>,
    rustfmt_format_ok: Option<bool>,
    clippy_clean: Option<bool>,
    reason: String,
}

struct ClippyJob {
    result_idx: usize,
    package_name: String,
    member_path: String,
}

#[test]
#[ignore = "slow rust canonical e2e; run: cargo test -p exempla --test e2e_harness exempla_rust_canonical -- --ignored --nocapture"]
fn exempla_rust_canonical() {
    if !cargo_available() {
        eprintln!("cargo not found on PATH; skipping Rust canonicality harness");
        return;
    }
    if !command_available("rustfmt", &["--version"]) {
        eprintln!("rustfmt not found on PATH; skipping Rust canonicality harness");
        return;
    }

    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    let compiler = Compiler::new(Config::default());
    let temp_root = make_temp_root();
    let total = exempla.len();
    let mut results = Vec::with_capacity(total);
    let mut clippy_jobs = Vec::new();

    eprintln!(
        "[rust-canonical] checking {total} exempla; temp root: {}",
        temp_root.display()
    );

    for (idx, file) in exempla.iter().enumerate() {
        let relative = file
            .strip_prefix(&exempla_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| file.display().to_string());
        let code = match super::rust::compile_rust_exemplum(&compiler, file, &exempla_dir) {
            Ok(code) => code,
            Err(reason) => {
                eprintln!("[rust-canonical {idx:03}/{total}] {relative}  emit-fail");
                results.push(CanonicalResult {
                    path: file.clone(),
                    emit_ok: false,
                    valor_helper: false,
                    rustfmt_clean: None,
                    rustfmt_format_ok: None,
                    clippy_clean: None,
                    reason,
                });
                continue;
            }
        };

        let rustfmt_clean = run_rustfmt_check(&temp_root, idx, &code);
        let rustfmt_format_ok = run_rustfmt_format(&temp_root, idx, &code);
        let clippy_package = if code.contains("fn main") {
            let package_name = format!("canonical_{idx:03}");
            let member_path = format!("clippy/{idx:03}");
            write_rust_workspace_member(&temp_root.join(&member_path), &package_name, &code);
            clippy_jobs.push(ClippyJob {
                result_idx: results.len(),
                package_name: package_name.clone(),
                member_path,
            });
            Some(package_name)
        } else {
            None
        };

        eprintln!(
            "[rust-canonical {idx:03}/{total}] {relative}  rustfmt={}  format={}  clippy={}",
            label(Some(rustfmt_clean)),
            label(Some(rustfmt_format_ok)),
            if clippy_package.is_some() {
                "queued"
            } else {
                "skip"
            }
        );
        results.push(CanonicalResult {
            path: file.clone(),
            emit_ok: true,
            valor_helper: code.contains(VALOR_HELPER_MARKER),
            rustfmt_clean: Some(rustfmt_clean),
            rustfmt_format_ok: Some(rustfmt_format_ok),
            clippy_clean: clippy_package.as_ref().map(|_| true),
            reason: String::new(),
        });
    }

    run_clippy_workspace(&temp_root, &mut results, &clippy_jobs);

    let summary = summarize(&results);
    print_summary(&summary);
    report_failures("emit", results.iter().filter(|r| !r.emit_ok));
    report_failures(
        "rustfmt --check",
        results.iter().filter(|r| r.rustfmt_clean == Some(false)),
    );
    report_failures(
        "rustfmt format",
        results
            .iter()
            .filter(|r| r.rustfmt_format_ok == Some(false)),
    );
    report_failures(
        "cargo clippy -D warnings",
        results.iter().filter(|r| r.clippy_clean == Some(false)),
    );

    assert!(
        summary.emit_ok >= EMIT_OK_FLOOR,
        "Rust canonical emit-ok floor regressed: {} < {}",
        summary.emit_ok,
        EMIT_OK_FLOOR
    );
    assert!(
        summary.rustfmt_clean >= RUSTFMT_CLEAN_FLOOR,
        "Rust canonical rustfmt-clean floor regressed: {} < {}",
        summary.rustfmt_clean,
        RUSTFMT_CLEAN_FLOOR
    );
    assert!(
        summary.rustfmt_format_ok >= RUSTFMT_FORMAT_OK_FLOOR,
        "Rust canonical rustfmt-format floor regressed: {} < {}",
        summary.rustfmt_format_ok,
        RUSTFMT_FORMAT_OK_FLOOR
    );
    assert!(
        summary.clippy_checked >= CLIPPY_CHECKED_FLOOR,
        "Rust canonical clippy-checked floor regressed: {} < {}",
        summary.clippy_checked,
        CLIPPY_CHECKED_FLOOR
    );
    assert!(
        summary.clippy_clean >= CLIPPY_CLEAN_FLOOR,
        "Rust canonical clippy-clean floor regressed: {} < {}",
        summary.clippy_clean,
        CLIPPY_CLEAN_FLOOR
    );
    assert!(
        summary.valor_helper_files <= VALOR_HELPER_FILES_MAX,
        "Rust canonical valor-helper breadth regressed: {} > {}",
        summary.valor_helper_files,
        VALOR_HELPER_FILES_MAX
    );
}

struct CanonicalSummary {
    total: usize,
    emit_ok: usize,
    emit_fail: usize,
    valor_helper_files: usize,
    rustfmt_clean: usize,
    rustfmt_checked: usize,
    rustfmt_format_ok: usize,
    rustfmt_format_checked: usize,
    clippy_clean: usize,
    clippy_checked: usize,
}

fn summarize(results: &[CanonicalResult]) -> CanonicalSummary {
    CanonicalSummary {
        total: results.len(),
        emit_ok: results.iter().filter(|result| result.emit_ok).count(),
        emit_fail: results.iter().filter(|result| !result.emit_ok).count(),
        valor_helper_files: results
            .iter()
            .filter(|result| result.emit_ok && result.valor_helper)
            .count(),
        rustfmt_clean: results
            .iter()
            .filter(|result| result.rustfmt_clean == Some(true))
            .count(),
        rustfmt_checked: results
            .iter()
            .filter(|result| result.rustfmt_clean.is_some())
            .count(),
        rustfmt_format_ok: results
            .iter()
            .filter(|result| result.rustfmt_format_ok == Some(true))
            .count(),
        rustfmt_format_checked: results
            .iter()
            .filter(|result| result.rustfmt_format_ok.is_some())
            .count(),
        clippy_clean: results
            .iter()
            .filter(|result| result.clippy_clean == Some(true))
            .count(),
        clippy_checked: results
            .iter()
            .filter(|result| result.clippy_clean.is_some())
            .count(),
    }
}

fn print_summary(summary: &CanonicalSummary) {
    let line = format!(
        "Rust canonical exempla: {} emit ok / {} total",
        summary.emit_ok, summary.total
    );
    eprintln!("{line}");
    println!("{line}");
    eprintln!("emit_fail: {}", summary.emit_fail);
    let helper_free = summary.emit_ok.saturating_sub(summary.valor_helper_files);
    let helper_free_percent = helper_free
        .saturating_mul(100)
        .checked_div(summary.emit_ok)
        .unwrap_or(0);
    eprintln!(
        "  valor_helper_files: {} (max {})",
        summary.valor_helper_files, VALOR_HELPER_FILES_MAX
    );
    eprintln!(
        "  valor_helper_free: {helper_free}/{} ({helper_free_percent}%)",
        summary.emit_ok
    );
    eprintln!(
        "{}",
        format_tier_line(
            "rustfmt_clean",
            summary.rustfmt_clean,
            summary.rustfmt_checked,
            RUSTFMT_CLEAN_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "rustfmt_format_ok",
            summary.rustfmt_format_ok,
            summary.rustfmt_format_checked,
            RUSTFMT_FORMAT_OK_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "clippy_clean",
            summary.clippy_clean,
            summary.clippy_checked,
            CLIPPY_CLEAN_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_count_floor_line(
            "clippy_checked",
            summary.clippy_checked,
            CLIPPY_CHECKED_FLOOR
        )
    );
}

fn report_failures<'a>(label: &str, failures: impl Iterator<Item = &'a CanonicalResult>) {
    for failure in failures.take(20) {
        eprintln!(
            "[rust-canonical {label} fail] {} :: {}",
            failure.path.display(),
            failure.reason
        );
    }
}

fn run_rustfmt_check(temp_root: &Path, idx: usize, code: &str) -> bool {
    let path = write_temp_rust_file(temp_root, idx, "rustfmt-check", code);
    Command::new("rustfmt")
        .args(["--edition", "2021", "--check"])
        .arg(path)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_rustfmt_format(temp_root: &Path, idx: usize, code: &str) -> bool {
    let path = write_temp_rust_file(temp_root, idx, "rustfmt-format", code);
    Command::new("rustfmt")
        .args(["--edition", "2021"])
        .arg(path)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_clippy_workspace(
    temp_root: &Path,
    results: &mut [CanonicalResult],
    clippy_jobs: &[ClippyJob],
) {
    if clippy_jobs.is_empty() {
        return;
    }

    eprintln!(
        "[rust-canonical] phase 2: single cargo clippy over {} generated members",
        clippy_jobs.len()
    );
    let member_paths = clippy_jobs
        .iter()
        .map(|job| job.member_path.clone())
        .collect::<Vec<_>>();
    let manifest = write_rust_workspace_root(temp_root, &member_paths);
    let output = Command::new("cargo")
        .args([
            "clippy",
            "--workspace",
            "--keep-going",
            "--message-format=json",
            "--manifest-path",
        ])
        .arg(&manifest)
        .args(["--", "-D", "warnings"])
        .output();

    let Ok(output) = output else {
        mark_all_clippy_failed(results, clippy_jobs, "failed to spawn cargo clippy");
        return;
    };
    if output.status.success() {
        return;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let failed_packages = extract_failed_clippy_packages(&stdout);
    if failed_packages.is_empty() {
        mark_all_clippy_failed(
            results,
            clippy_jobs,
            "cargo clippy failed without package diagnostics",
        );
        return;
    }

    for job in clippy_jobs {
        if failed_packages.contains(&job.package_name) {
            if let Some(result) = results.get_mut(job.result_idx) {
                result.clippy_clean = Some(false);
                result.reason = "cargo clippy -D warnings emitted diagnostics".to_owned();
            }
        }
    }
}

fn mark_all_clippy_failed(
    results: &mut [CanonicalResult],
    clippy_jobs: &[ClippyJob],
    reason: &str,
) {
    for job in clippy_jobs {
        if let Some(result) = results.get_mut(job.result_idx) {
            result.clippy_clean = Some(false);
            result.reason = reason.to_owned();
        }
    }
}

fn extract_failed_clippy_packages(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .filter(|line| {
            line.contains(r#""reason":"compiler-message""#)
                && (line.contains(r#""level":"error""#) || line.contains(r#""level":"warning""#))
        })
        .filter_map(extract_canonical_package_name)
        .collect()
}

fn extract_canonical_package_name(line: &str) -> Option<String> {
    let start = line.find("canonical_")?;
    let suffix = &line[start..];
    let end = suffix
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .unwrap_or(suffix.len());
    Some(suffix[..end].to_owned())
}

fn write_temp_rust_file(temp_root: &Path, idx: usize, label: &str, code: &str) -> PathBuf {
    let dir = temp_root.join(label);
    fs::create_dir_all(&dir).expect("create rust canonical temp dir");
    let path = dir.join(format!("{idx:03}.rs"));
    fs::write(&path, code).expect("write generated rust temp file");
    path
}

fn label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "ok",
        Some(false) => "fail",
        None => "skip",
    }
}
