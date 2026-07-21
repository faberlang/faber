//! Fast HIR application-lane target coverage matrix (G5).
//!
//! One frontend analyze pass per exemplum, then in-process classification for
//! rust / go / ts / faber. No external toolchains. H1 honesty only — does not
//! claim H2 emit or H3 product run.

use super::common::{collect_exempla_files, format_diagnostic_messages};
use radix::codegen::coverage::{classify_hir_coverage, HirCoverageTarget};
use radix::driver::{analyze_source, Config, Session};
use radix::mir::Lowerability;
use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone)]
struct HirTargetMatrixRow {
    path: PathBuf,
    /// Separate denominator: frontend analyze failed (not a target gap).
    analysis_ok: bool,
    analysis_errors: String,
    targets: FxHashMap<HirCoverageTarget, Lowerability>,
}

#[derive(Debug, Default)]
struct TargetSummary {
    capable: usize,
    gap_counts: FxHashMap<&'static str, usize>,
}

/// Honest non-regression floors after first green G5 baseline (2026-07-10).
/// Corpus: examples/corpus (292 files; 279 analysis_ok). Do not lower without
/// a counted debt row in the G5 delivery closeout.
const HIR_ANALYSIS_OK_FLOOR: usize = 279;
const TARGET_CAPABLE_FLOORS: [(HirCoverageTarget, usize); 4] = [
    (HirCoverageTarget::Rust, 276),
    (HirCoverageTarget::Go, 258),
    (HirCoverageTarget::TypeScript, 279),
    (HirCoverageTarget::Faber, 279),
];

/// Run the fast HIR matrix and print a summary to stderr.
///
/// ```text
/// cargo test -p exempla --lib hir_target_coverage_matrix -- --ignored --nocapture
/// ```
#[test]
#[ignore = "slow HIR target matrix; run: cargo test -p exempla --lib hir_target_coverage_matrix -- --ignored --nocapture"]
fn hir_target_coverage_matrix() {
    let started = Instant::now();
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    assert!(
        !exempla.is_empty(),
        "HIR target matrix found no exempla under {}",
        exempla_dir.display()
    );

    let session = Session::new(Config::default());
    let rows = build_matrix_rows(&session, &exempla);
    print_matrix_report(&rows, started.elapsed());
    assert_matrix_ratchet(&rows);
}

/// Emit machine-readable rows for scripts (`ROWS` section).
#[test]
#[ignore = "maintenance HIR matrix emit; run: cargo test -p exempla --lib emit_hir_target_matrix -- --ignored --nocapture"]
fn emit_hir_target_matrix() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    let session = Session::new(Config::default());
    let rows = build_matrix_rows(&session, &exempla);
    println!("ROWS");
    for row in &rows {
        let rel = row.path.display();
        if !row.analysis_ok {
            println!("{rel}\tno-hir\t{}", row.analysis_errors.replace('\t', " "));
            continue;
        }
        for target in HirCoverageTarget::ALL {
            let verdict = row
                .targets
                .get(target)
                .cloned()
                .unwrap_or(Lowerability::Capable);
            match verdict {
                Lowerability::Capable => println!("{rel}\t{}\tcapable", target.name()),
                Lowerability::Rejected(gaps) => {
                    let slugs = gaps.iter().map(|g| g.slug()).collect::<Vec<_>>().join(",");
                    println!("{rel}\t{}\tgaps:{slugs}", target.name());
                }
            }
        }
    }
}

fn build_matrix_rows(session: &Session, exempla: &[PathBuf]) -> Vec<HirTargetMatrixRow> {
    let mut rows = Vec::with_capacity(exempla.len());
    for file in exempla {
        rows.push(classify_matrix_row(session, file));
    }
    rows
}

fn classify_matrix_row(session: &Session, file: &Path) -> HirTargetMatrixRow {
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(err) => {
            return HirTargetMatrixRow {
                path: file.to_path_buf(),
                analysis_ok: false,
                analysis_errors: format!("io:{err}"),
                targets: FxHashMap::default(),
            };
        }
    };
    let name = file.display().to_string();
    match analyze_source(session, &name, &source) {
        Ok(unit) => {
            if unit.diagnostics.iter().any(|d| d.is_error()) {
                return HirTargetMatrixRow {
                    path: file.to_path_buf(),
                    analysis_ok: false,
                    analysis_errors: format_diagnostic_messages(&unit.diagnostics),
                    targets: FxHashMap::default(),
                };
            }
            let mut targets = FxHashMap::default();
            for &target in HirCoverageTarget::ALL {
                let verdict = classify_hir_coverage(target, &unit.hir, &unit.types);
                targets.insert(target, verdict);
            }
            HirTargetMatrixRow {
                path: file.to_path_buf(),
                analysis_ok: true,
                analysis_errors: String::new(),
                targets,
            }
        }
        Err(diagnostics) => HirTargetMatrixRow {
            path: file.to_path_buf(),
            analysis_ok: false,
            analysis_errors: format_diagnostic_messages(&diagnostics),
            targets: FxHashMap::default(),
        },
    }
}

fn print_matrix_report(rows: &[HirTargetMatrixRow], elapsed: std::time::Duration) {
    let total = rows.len();
    let analysis_ok = rows.iter().filter(|r| r.analysis_ok).count();
    let no_hir = total.saturating_sub(analysis_ok);

    eprintln!("=== HIR target coverage matrix (G5 H1) ===");
    eprintln!("exempla: {total}  analysis_ok: {analysis_ok}  no-hir: {no_hir}");
    eprintln!("elapsed: {:.2}s", elapsed.as_secs_f64());

    for &target in HirCoverageTarget::ALL {
        let mut summary = TargetSummary::default();
        for row in rows.iter().filter(|r| r.analysis_ok) {
            match row.targets.get(&target) {
                Some(Lowerability::Capable) | None => summary.capable += 1,
                Some(Lowerability::Rejected(gaps)) => {
                    for gap in gaps {
                        *summary.gap_counts.entry(gap.slug()).or_default() += 1;
                    }
                }
            }
        }
        eprintln!(
            "  {:8} capable={:<5} gaps={}",
            target.name(),
            summary.capable,
            summary.gap_counts.len()
        );
        let mut gaps: Vec<_> = summary.gap_counts.into_iter().collect();
        gaps.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        for (slug, count) in gaps.into_iter().take(8) {
            eprintln!("           {count:>4}  {slug}");
        }
    }
    eprintln!("evidence: H1 classifier only (not H2 emit / H3 product run)");
}

fn assert_matrix_ratchet(rows: &[HirTargetMatrixRow]) {
    let analysis_ok = rows.iter().filter(|r| r.analysis_ok).count();
    assert!(
        analysis_ok >= HIR_ANALYSIS_OK_FLOOR,
        "HIR analysis_ok floor regression: {analysis_ok} < {HIR_ANALYSIS_OK_FLOOR}"
    );

    for (target, floor) in TARGET_CAPABLE_FLOORS {
        let capable = rows
            .iter()
            .filter(|r| r.analysis_ok)
            .filter(|r| matches!(r.targets.get(&target), Some(Lowerability::Capable) | None))
            .count();
        assert!(
            capable >= floor,
            "{} capable floor regression: {capable} < {floor}",
            target.name()
        );
    }
}
