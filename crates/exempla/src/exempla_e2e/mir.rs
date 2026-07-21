use super::common::{collect_exempla_files, format_diagnostic_messages};
use radix::driver::Session;
use radix::Config;
use rustc_hash::FxHashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MirTier {
    SourceReadable,
    FrontendAnalyzed,
    MirLowered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MirOutcomeBucket {
    SourceReadFailed,
    FrontendFailed,
    MirLoweringFailed,
    MirLowered,
}

#[derive(Debug, Clone)]
pub(crate) struct MirE2eResult {
    pub path: PathBuf,
    pub tier: MirTier,
    pub bucket: MirOutcomeBucket,
    pub lowering_issue: Option<String>,
    pub reason: String,
}

const EXPECTED_FRONTEND_ANALYZED_FLOOR: usize = 283;
const EXPECTED_MIR_LOWERED_FLOOR: usize = 262;

#[test]
#[ignore = "slow mir e2e; run: cargo test -p exempla --lib exempla_mir_e2e -- --ignored --nocapture"]
fn exempla_mir_e2e() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    assert!(
        !exempla.is_empty(),
        "MIR e2e harness found no exempla files"
    );

    let session = Session::new(Config::default());
    let mut results = Vec::with_capacity(exempla.len());

    for file in &exempla {
        results.push(classify_mir_exemplum(&session, file));
    }

    print_mir_e2e_report(&results);
    assert_mir_expected_floors(&results);
}

pub(crate) fn classify_mir_exemplum(session: &Session, file: &Path) -> MirE2eResult {
    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(err) => {
            return mir_result(
                file,
                MirTier::SourceReadable,
                MirOutcomeBucket::SourceReadFailed,
                None,
                format!("cannot read source: {err}"),
            );
        }
    };

    let mut analysis =
        match radix::driver::analyze_source(session, &file.display().to_string(), &source) {
            Ok(analysis) => analysis,
            Err(diagnostics) => {
                return mir_result(
                    file,
                    MirTier::SourceReadable,
                    MirOutcomeBucket::FrontendFailed,
                    None,
                    format!(
                        "frontend failed: {}",
                        format_diagnostic_messages(&diagnostics)
                    ),
                );
            }
        };

    match radix::mir::lower_analyzed_unit_with_context(&mut analysis) {
        Ok(_mir) => mir_result(
            file,
            MirTier::MirLowered,
            MirOutcomeBucket::MirLowered,
            None,
            "validated MIR lowered".to_owned(),
        ),
        Err(errors) => {
            let issues = errors
                .iter()
                .map(|error| error.issue.clone())
                .collect::<Vec<_>>();
            mir_result(
                file,
                MirTier::FrontendAnalyzed,
                MirOutcomeBucket::MirLoweringFailed,
                issues.first().cloned(),
                format!("MIR lowering failed: {}", issues.join(" | ")),
            )
        }
    }
}

fn mir_result(
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

pub(crate) fn count_mir_tier(results: &[MirE2eResult], tier: MirTier) -> usize {
    results.iter().filter(|result| result.tier >= tier).count()
}

pub(crate) fn count_mir_bucket(results: &[MirE2eResult], bucket: MirOutcomeBucket) -> usize {
    results
        .iter()
        .filter(|result| result.bucket == bucket)
        .count()
}

fn print_mir_e2e_report(results: &[MirE2eResult]) {
    let total = results.len();
    eprintln!("MIR e2e exempla:");
    eprintln!(
        "  frontend analyzed: {}/{}",
        count_mir_tier(results, MirTier::FrontendAnalyzed),
        total
    );
    eprintln!(
        "  MIR lowered: {}/{}",
        count_mir_tier(results, MirTier::MirLowered),
        total
    );
    eprintln!(
        "  frontend failed: {}",
        count_mir_bucket(results, MirOutcomeBucket::FrontendFailed)
    );
    eprintln!(
        "  MIR lowering failed: {}",
        count_mir_bucket(results, MirOutcomeBucket::MirLoweringFailed)
    );

    for result in results
        .iter()
        .filter(|result| result.tier < MirTier::MirLowered)
    {
        eprintln!(
            "[mir:{:?}] {} :: {}",
            result.tier,
            result.path.display(),
            result.reason
        );
    }
}

pub(crate) fn classify_ledger_bucket(result: &MirE2eResult) -> &'static str {
    match result.bucket {
        MirOutcomeBucket::MirLowered => "mir-lowered",
        MirOutcomeBucket::SourceReadFailed | MirOutcomeBucket::FrontendFailed => "upstream-typing",
        MirOutcomeBucket::MirLoweringFailed
            if result.lowering_issue.as_deref()
                == Some("unsupported_mir_lowering_cli_program_specific_mir_lowering") =>
        {
            "intentional-subset"
        }
        MirOutcomeBucket::MirLoweringFailed
            if result
                .lowering_issue
                .as_deref()
                .is_some_and(|issue| issue.starts_with("invalid_mir_")) =>
        {
            "lowering-bug"
        }
        MirOutcomeBucket::MirLoweringFailed => "deferred-surface",
    }
}

fn tier_label(tier: MirTier) -> &'static str {
    match tier {
        MirTier::MirLowered => "mir-lowered",
        MirTier::FrontendAnalyzed => "frontend-analyzed",
        MirTier::SourceReadable => "source-readable",
    }
}

fn assert_mir_expected_floors(results: &[MirE2eResult]) {
    let frontend = count_mir_tier(results, MirTier::FrontendAnalyzed);
    let mir = count_mir_tier(results, MirTier::MirLowered);

    let regressions = [
        (
            "frontend analyzed",
            frontend,
            EXPECTED_FRONTEND_ANALYZED_FLOOR,
        ),
        ("MIR lowered", mir, EXPECTED_MIR_LOWERED_FLOOR),
    ]
    .into_iter()
    .filter_map(|(label, actual, expected)| {
        (actual < expected).then_some(format!(
            "{label} expected at least {expected}, got {actual}"
        ))
    })
    .collect::<Vec<_>>();

    assert!(
        regressions.is_empty(),
        "unexpected MIR e2e tier regressions:\n{}",
        regressions.join("\n")
    );
}

#[test]
#[ignore = "maintenance mir ledger; run: cargo test -p exempla --lib emit_mir_baseline_ledger -- --ignored --nocapture"]
fn emit_mir_baseline_ledger() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    let session = Session::new(Config::default());
    let mut results: Vec<_> = exempla
        .iter()
        .map(|file| classify_mir_exemplum(&session, file))
        .collect();
    results.sort_by(|left, right| left.path.cmp(&right.path));

    let mut counts: FxHashMap<&'static str, usize> = FxHashMap::default();
    for result in &results {
        *counts.entry(classify_ledger_bucket(result)).or_default() += 1;
    }

    println!("TIER_COUNTS");
    println!("total:{}", results.len());
    println!(
        "mir_lowered:{}",
        count_mir_tier(&results, MirTier::MirLowered)
    );
    println!(
        "lowering_failed:{}",
        count_mir_bucket(&results, MirOutcomeBucket::MirLoweringFailed)
    );
    for (bucket, count) in counts.iter() {
        println!("bucket:{bucket}:{count}");
    }

    println!("ROWS");
    for result in &results {
        let path = ledger_path(&result.path);
        let note = if result.bucket == MirOutcomeBucket::MirLowered {
            "validated MIR lowered".to_owned()
        } else {
            result
                .reason
                .chars()
                .take(120)
                .collect::<String>()
                .replace('|', "\\|")
        };
        println!(
            "{}|{}|{}|{}",
            path,
            tier_label(result.tier),
            classify_ledger_bucket(result),
            note
        );
    }
}

fn ledger_path(path: &Path) -> String {
    let corpus_dir = crate::paths::corpus_dir();
    let relative = path.strip_prefix(&corpus_dir).unwrap_or(path);
    let rendered = relative.display().to_string().replace('\\', "/");
    if rendered.starts_with("examples/corpus/")
        || rendered.starts_with("examples/")
        || rendered.starts_with("norma/exempla/")
        || rendered.starts_with("crates/exempla/corpus/")
    {
        rendered
    } else {
        format!("examples/corpus/{rendered}")
    }
}

#[cfg(test)]
#[path = "mir_test.rs"]
mod tests;
