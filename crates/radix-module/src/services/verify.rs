//! Typed verify service. Printing and process exit live in `radix-cli`.

use crate::aspect::{
    HirAspectBackendKind, HirAspectCheckResult, HirAspectCheckStatus, HirAspectCoverageReport, HirAspectCoverageStatus,
    HirAspectEvidence, HirAspectRecordKind, HirAspectReport, HirAspectVerifier, LeanAspectChecker,
    LeanAspectCheckerConfig, report_hir_aspects, summarize_hir_aspect_coverage,
};
use crate::diagnostics::Diagnostic;
use crate::driver::{AnalyzedModule, Config, Session};

const LEAN_UNSET_SUMMARY: &str = "no Lean checker was configured";

#[derive(Debug)]
pub struct VerifySourceResult {
    pub coverage: Option<HirAspectCoverageReport>,
    pub diagnostics: Vec<Diagnostic>,
    pub success: bool,
}

#[must_use]
pub fn verify_source(name: &str, source: &str, checker_config: Option<LeanAspectCheckerConfig>) -> VerifySourceResult {
    verify_source_with_config(name, source, checker_config, &Config::default().with_dev_locale_root())
}

/// [`verify_source`] under a caller-owned locale policy: the session's config
/// carries the language-pack root and product loader a declared frontmatter
/// locale resolves through (faber installs its embedded reader packs here).
#[must_use]
pub fn verify_source_with_config(
    name: &str,
    source: &str,
    checker_config: Option<LeanAspectCheckerConfig>,
    locale_config: &Config,
) -> VerifySourceResult {
    let session = Session::new(locale_config.clone());
    let analysis = match crate::program::analyze_prepared_program_name(&session, name, source) {
        Ok(analysis) => analysis,
        Err(diagnostics) => return VerifySourceResult { coverage: None, diagnostics, success: false },
    };

    let coverage = coverage_for_analysis(&analysis, checker_config);
    let success = coverage_proves_success(&coverage);

    VerifySourceResult { coverage: Some(coverage), diagnostics: analysis.diagnostics, success }
}

fn coverage_for_analysis(
    analysis: &AnalyzedModule,
    checker_config: Option<LeanAspectCheckerConfig>,
) -> HirAspectCoverageReport {
    let aspect_report = report_hir_aspects(&analysis.hir, &analysis.types);
    let check_results = match checker_config {
        Some(config) => LeanAspectChecker::new(config, std::env::temp_dir()).verify(&aspect_report),
        None => checker_unavailable_results(&aspect_report),
    };
    summarize_hir_aspect_coverage(&aspect_report, &check_results, HirAspectBackendKind::Lean)
}

#[must_use]
fn coverage_proves_success(coverage: &HirAspectCoverageReport) -> bool {
    let entries = coverage.entries();
    !entries.is_empty()
        && entries
            .iter()
            .all(|entry| entry.status == HirAspectCoverageStatus::Proven)
}

#[must_use]
fn checker_unavailable_results(report: &HirAspectReport) -> Vec<HirAspectCheckResult> {
    report
        .records()
        .iter()
        .filter(|record| record.record_kind == HirAspectRecordKind::Recognized)
        .map(|_| HirAspectCheckResult {
            status: HirAspectCheckStatus::ToolchainMissing,
            evidence: HirAspectEvidence(LEAN_UNSET_SUMMARY.to_owned()),
            counterexample: Vec::new(),
            backend: HirAspectBackendKind::Lean,
        })
        .collect()
}
