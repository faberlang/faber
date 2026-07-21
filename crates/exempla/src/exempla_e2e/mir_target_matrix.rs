//! Fast MIR target coverage matrix over the exempla corpus.
//!
//! Analyzes and lowers each exemplum once, then classifies every MIR-backed
//! target in-process. No external toolchains, disk writes, or per-target
//! re-analysis.

use super::common::{collect_exempla_files, format_diagnostic_messages};
use super::mir::{classify_ledger_bucket, MirE2eResult, MirOutcomeBucket, MirTier};
use radix::driver::Session;
use radix::mir::classify_stepper_lowerability;
use radix::mir::MirDeviceContext;
use radix::mir::{
    classify_mir_coverage, device_roles_from_hir, lower_analyzed_unit_with_context, CapabilityGap,
    Lowerability, MirCoverageTarget, MIR_COVERAGE_TARGETS,
};
use radix::Config;
use rustc_hash::FxHashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone)]
struct MirTargetMatrixRow {
    path: PathBuf,
    mir_bucket: &'static str,
    mir_capable: bool,
    targets: FxHashMap<MirCoverageTarget, Lowerability>,
    /// Structural (pre-execution) stepper lowerability. `None` for rows that
    /// never reach validated MIR. The run tier lives under `MirCoverageTarget::Scena`.
    scena_structural: Option<Lowerability>,
}

#[derive(Debug, Default)]
struct TargetSummary {
    capable: usize,
    gap_counts: FxHashMap<String, usize>,
}

const MIR_CAPABLE_FLOOR: usize = 253;
// Stage 1 also exposed current structural scena floor drift. Stage 3 owns the
// package/script boundary burn-up; do not lower this again without a new
// counted debt row.
const SCENA_STRUCTURAL_CAPABLE_FLOOR: usize = 205;
const KNOWN_SCENA_STRUCTURAL_RUN_MISMATCHES: [(&str, &str); 3] = [
    (
        "examples/corpus/est/est.fab",
        "runtime assertion in est variant-check exemplar",
    ),
    (
        "examples/corpus/operatores/numerus-overflow.fab",
        "intentional checked numerus overflow trap",
    ),
    (
        "examples/corpus/tensor/method-errors.fab",
        "intentional tensor structa hard-error policy",
    ),
];
const TARGET_CAPABLE_FLOORS: [(MirCoverageTarget, usize); 8] = [
    (MirCoverageTarget::LlvmText, 250),
    // Stage 1 of the MIR lane usability campaign exposed existing Wasm/WAT
    // floor drift after the LLVM floor recovered. Stage 4 owns the import/type
    // floor burn-up; do not lower these again without a new counted debt row.
    (MirCoverageTarget::WasmText, 203),
    (MirCoverageTarget::Wasm, 201),
    (MirCoverageTarget::MetalText, 6),
    (MirCoverageTarget::WgslText, 6),
    (MirCoverageTarget::SexpStructural, 193),
    (MirCoverageTarget::Sexp, 193),
    (MirCoverageTarget::Scena, 215),
];

/// Run the fast matrix and print a summary to stderr.
///
/// Invoke explicitly:
/// `cargo test -p exempla --lib mir_target_coverage_matrix -- --ignored --nocapture`
#[test]
#[ignore = "slow MIR target matrix; run: cargo test -p exempla --lib mir_target_coverage_matrix -- --ignored --nocapture"]
fn mir_target_coverage_matrix() {
    let started = Instant::now();
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    assert!(
        !exempla.is_empty(),
        "MIR target matrix found no exempla files"
    );

    let session = Session::new(Config::default());
    let rows = build_matrix_rows(&session, &exempla);
    print_matrix_report(&rows, started.elapsed());
    assert_matrix_ratchet(&rows);
}

/// Emit machine-readable rows for scripts (`ROWS` section).
#[test]
#[ignore = "maintenance mir matrix emit; run: cargo test -p exempla --lib emit_mir_target_matrix -- --ignored --nocapture"]
fn emit_mir_target_matrix() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    let session = Session::new(Config::default());
    let rows = build_matrix_rows(&session, &exempla);

    let mir_capable = rows.iter().filter(|row| row.mir_capable).count();
    println!("SUMMARY");
    println!("total:{}", rows.len());
    println!("mir_capable:{mir_capable}");
    for target in MIR_COVERAGE_TARGETS {
        let capable = rows
            .iter()
            .filter(|row| row.mir_capable && target_capable(&row.targets, target))
            .count();
        println!("target:{}:capable:{capable}", target.name());
    }

    println!("ROWS");
    for row in &rows {
        let path = display_path(&row.path);
        let mut cells = Vec::with_capacity(MIR_COVERAGE_TARGETS.len());
        for target in MIR_COVERAGE_TARGETS {
            cells.push(target_cell(row, target));
        }
        println!("{path}|{}|{}", row.mir_bucket, cells.join("|"));
    }
}

fn build_matrix_rows(session: &Session, exempla: &[PathBuf]) -> Vec<MirTargetMatrixRow> {
    let mut rows = Vec::with_capacity(exempla.len());
    for file in exempla {
        rows.push(classify_matrix_row(session, file));
    }
    rows
}

fn classify_matrix_row(session: &Session, file: &Path) -> MirTargetMatrixRow {
    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(err) => {
            let mir = mir_stand_in(
                file,
                MirTier::SourceReadable,
                MirOutcomeBucket::SourceReadFailed,
                None,
                format!("cannot read source: {err}"),
            );
            return matrix_row_from_mir(&mir, FxHashMap::default());
        }
    };

    let mut analysis =
        match radix::driver::analyze_source(session, &file.display().to_string(), &source) {
            Ok(analysis) => analysis,
            Err(diagnostics) => {
                let mir = mir_stand_in(
                    file,
                    MirTier::SourceReadable,
                    MirOutcomeBucket::FrontendFailed,
                    None,
                    format!(
                        "frontend failed: {}",
                        format_diagnostic_messages(&diagnostics)
                    ),
                );
                return matrix_row_from_mir(&mir, FxHashMap::default());
            }
        };

    let device_roles = device_roles_from_hir(&analysis.hir);
    let lowered = match lower_analyzed_unit_with_context(&mut analysis) {
        Ok(lowered) => lowered,
        Err(errors) => {
            let issues = errors
                .iter()
                .map(|error| error.issue.clone())
                .collect::<Vec<_>>();
            let mir = mir_stand_in(
                file,
                MirTier::FrontendAnalyzed,
                MirOutcomeBucket::MirLoweringFailed,
                issues.first().cloned(),
                format!("MIR lowering failed: {}", issues.join(" | ")),
            );
            return matrix_row_from_mir(&mir, FxHashMap::default());
        }
    };

    let mut device = MirDeviceContext::from_hir_roles(device_roles);
    device.attach_program(&lowered.program);

    let mut targets = FxHashMap::default();
    for target in MIR_COVERAGE_TARGETS {
        let verdict = classify_mir_coverage(
            target,
            &lowered.program,
            &lowered.validation,
            &device,
            &lowered.interner,
        );
        targets.insert(target, verdict);
    }

    // WHY: structural scena tier (SA-001) classifies lowerability without
    // executing; it is a lower bound on the run tier below it.
    let scena_structural = Some(classify_stepper_lowerability(
        &lowered.program,
        &lowered.validation,
    ));

    let mir = mir_stand_in(
        file,
        MirTier::MirLowered,
        MirOutcomeBucket::MirLowered,
        None,
        "validated MIR lowered".to_owned(),
    );
    let mut row = matrix_row_from_mir(&mir, targets);
    row.scena_structural = scena_structural;
    row
}

fn mir_stand_in(
    file: &Path,
    tier: MirTier,
    bucket: MirOutcomeBucket,
    lowering_issue: Option<String>,
    reason: String,
) -> MirE2eResult {
    MirE2eResult {
        path: file.to_path_buf(),
        tier,
        bucket,
        lowering_issue,
        reason,
    }
}

fn matrix_row_from_mir(
    mir: &MirE2eResult,
    targets: FxHashMap<MirCoverageTarget, Lowerability>,
) -> MirTargetMatrixRow {
    MirTargetMatrixRow {
        path: mir.path.clone(),
        mir_bucket: classify_ledger_bucket(mir),
        mir_capable: mir.bucket == MirOutcomeBucket::MirLowered,
        targets,
        scena_structural: None,
    }
}

fn print_matrix_report(rows: &[MirTargetMatrixRow], elapsed: std::time::Duration) {
    let total = rows.len();
    let mir_capable = rows.iter().filter(|row| row.mir_capable).count();
    let mut summaries: FxHashMap<MirCoverageTarget, TargetSummary> = FxHashMap::default();

    for target in MIR_COVERAGE_TARGETS {
        summaries.insert(target, TargetSummary::default());
    }

    for row in rows.iter().filter(|row| row.mir_capable) {
        for target in MIR_COVERAGE_TARGETS {
            let summary = summaries.get_mut(&target).expect("summary slot");
            let verdict = row.targets.get(&target).expect("target verdict");
            if verdict.is_capable() {
                summary.capable += 1;
                continue;
            }
            if let Some(gap) = verdict.first_gap() {
                *summary
                    .gap_counts
                    .entry(gap.shape().to_owned())
                    .or_default() += 1;
            }
        }
    }

    eprintln!(
        "MIR target coverage matrix (in-process emit + scena run tier; no external toolchains)"
    );
    eprintln!("  corpus: {total}");
    eprintln!("  mir-capable: {mir_capable}/{total}");
    eprintln!("  elapsed: {:.2}s", elapsed.as_secs_f64());
    eprintln!();
    eprintln!(
        "{:<12} {:>8} {:>8} {:>7}  top gap",
        "target", "capable", "of-mir", "pct"
    );

    for target in MIR_COVERAGE_TARGETS {
        let summary = summaries.get(&target).expect("summary");
        let pct = if mir_capable == 0 {
            0.0
        } else {
            (summary.capable as f64 / mir_capable as f64) * 100.0
        };
        let top_gap = summary
            .gap_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(shape, count)| format!("{shape} ({count})"))
            .unwrap_or_else(|| "—".to_owned());
        eprintln!(
            "{:<12} {:>8} {:>8} {:>6.1}%  {top_gap}",
            target.name(),
            summary.capable,
            mir_capable,
            pct
        );
    }

    // Structural scena tier (SA-001): pre-execution classifier column paired
    // with the scena run tier above it.
    let mut structural = TargetSummary::default();
    for row in rows.iter().filter(|row| row.mir_capable) {
        match &row.scena_structural {
            Some(Lowerability::Capable) => structural.capable += 1,
            Some(Lowerability::Rejected(gaps)) => {
                if let Some(gap) = gaps.first() {
                    *structural
                        .gap_counts
                        .entry(gap.shape().to_owned())
                        .or_default() += 1;
                }
            }
            None => {}
        }
    }
    let structural_pct = if mir_capable == 0 {
        0.0
    } else {
        (structural.capable as f64 / mir_capable as f64) * 100.0
    };
    let structural_top = structural
        .gap_counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(shape, count)| format!("{shape} ({count})"))
        .unwrap_or_else(|| "—".to_owned());
    eprintln!(
        "{:<12} {:>8} {:>8} {:>6.1}%  {structural_top}",
        "scena-str", structural.capable, mir_capable, structural_pct
    );

    eprintln!();
    eprintln!("Per-target gap histogram (mir-capable exempla only):");
    for target in MIR_COVERAGE_TARGETS {
        let summary = summaries.get(&target).expect("summary");
        if summary.gap_counts.is_empty() {
            continue;
        }
        eprintln!("  {}:", target.name());
        let mut gaps: Vec<_> = summary.gap_counts.iter().collect();
        gaps.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
        for (shape, count) in gaps.iter().take(8) {
            eprintln!("    {shape}: {count}");
        }
    }
    if !structural.gap_counts.is_empty() {
        eprintln!("  scena-structural:");
        let mut gaps: Vec<_> = structural.gap_counts.iter().collect();
        gaps.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
        for (shape, count) in gaps.iter().take(8) {
            eprintln!("    {shape}: {count}");
        }
    }
}

fn assert_matrix_ratchet(rows: &[MirTargetMatrixRow]) {
    let mir_capable = rows.iter().filter(|row| row.mir_capable).count();
    assert!(
        mir_capable >= MIR_CAPABLE_FLOOR,
        "MIR target matrix mir-capable floor regressed: {mir_capable} < {MIR_CAPABLE_FLOOR}"
    );

    for (target, floor) in TARGET_CAPABLE_FLOORS {
        let capable = rows
            .iter()
            .filter(|row| row.mir_capable && target_capable(&row.targets, target))
            .count();
        assert!(
            capable >= floor,
            "MIR target matrix {} floor regressed: {capable} < {floor}",
            target.name()
        );
    }

    let structural = rows
        .iter()
        .filter(|row| row.mir_capable)
        .filter(|row| matches!(row.scena_structural, Some(Lowerability::Capable)))
        .count();
    assert!(
        structural >= SCENA_STRUCTURAL_CAPABLE_FLOOR,
        "MIR target matrix scena-structural floor regressed: {structural} < {SCENA_STRUCTURAL_CAPABLE_FLOOR}"
    );

    let structural_run_mismatches = structural_run_mismatch_paths(rows);
    let unexpected_mismatches: Vec<_> = structural_run_mismatches
        .iter()
        .filter(|path| !known_structural_run_mismatch(path))
        .cloned()
        .collect();
    let stale_known_mismatches: Vec<_> = KNOWN_SCENA_STRUCTURAL_RUN_MISMATCHES
        .iter()
        .map(|(path, _)| (*path).to_owned())
        .filter(|path| !structural_run_mismatches.contains(path))
        .collect();
    assert!(
        unexpected_mismatches.is_empty() && stale_known_mismatches.is_empty(),
        "MIR target matrix scena structural/run mismatch debt changed; unexpected: [{}]; stale known: [{}]",
        unexpected_mismatches.join(", "),
        stale_known_mismatches.join(", ")
    );
}

fn known_structural_run_mismatch(path: &str) -> bool {
    KNOWN_SCENA_STRUCTURAL_RUN_MISMATCHES
        .iter()
        .any(|(known, _)| *known == path)
}

fn structural_run_mismatch_paths(rows: &[MirTargetMatrixRow]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.mir_capable)
        .filter(|row| matches!(row.scena_structural, Some(Lowerability::Capable)))
        .filter(|row| !target_capable(&row.targets, MirCoverageTarget::Scena))
        .map(|row| display_path(&row.path))
        .collect()
}

fn target_capable(
    targets: &FxHashMap<MirCoverageTarget, Lowerability>,
    target: MirCoverageTarget,
) -> bool {
    targets.get(&target).is_some_and(Lowerability::is_capable)
}

fn target_cell(row: &MirTargetMatrixRow, target: MirCoverageTarget) -> String {
    if !row.mir_capable {
        return "n/a".to_owned();
    }
    match row.targets.get(&target) {
        Some(Lowerability::Capable) => "ok".to_owned(),
        Some(Lowerability::Rejected(gaps)) => gaps
            .first()
            .map(CapabilityGap::shape)
            .unwrap_or("rejected")
            .replace('|', "\\|"),
        None => "missing".to_owned(),
    }
}

fn display_path(path: &Path) -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest
        .parent()
        .and_then(Path::parent)
        .expect("exempla crate should live below repo root");
    let relative = path
        .strip_prefix(repo)
        .unwrap_or(path)
        .display()
        .to_string();
    let normalized = relative.replace('\\', "/");
    if normalized.starts_with("examples/corpus/")
        || normalized.starts_with("examples/")
        || normalized.starts_with("norma/exempla/")
        || normalized.starts_with("crates/exempla/corpus/")
    {
        normalized
    } else if let Ok(rel) = path.strip_prefix(crate::paths::corpus_dir()) {
        format!(
            "examples/corpus/{}",
            rel.display().to_string().replace('\\', "/")
        )
    } else {
        format!("examples/corpus/{normalized}")
    }
}

#[cfg(test)]
#[path = "mir_target_matrix_test.rs"]
mod tests;
