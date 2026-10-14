//! Typed verify service. Printing and process exit live in `radix-cli`.

use crate::aspect::{
    HirAspectBackendKind, HirAspectCheckResult, HirAspectCheckStatus, HirAspectCoverageReport, HirAspectCoverageStatus,
    HirAspectEvidence, HirAspectTlaModule, HirAspectVerifier, TlaAspectCheckerConfig, TlaAspectVerifier,
    emit_tla_modules, report_hir_aspects, summarize_hir_aspect_coverage,
};
use crate::diagnostics::Diagnostic;
use crate::driver::{AnalyzedModule, Config, Session};

const TLC_JAR_UNSET_SUMMARY: &str = "no TLC jar was given (pass --tla-jar)";

#[derive(Debug)]
pub struct VerifySourceResult {
    pub coverage: Option<HirAspectCoverageReport>,
    pub diagnostics: Vec<Diagnostic>,
    pub success: bool,
}

#[must_use]
pub fn verify_source(name: &str, source: &str, checker_config: Option<TlaAspectCheckerConfig>) -> VerifySourceResult {
    let session = Session::new(Config::default().with_dev_locale_root());
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
    checker_config: Option<TlaAspectCheckerConfig>,
) -> HirAspectCoverageReport {
    let aspect_report = report_hir_aspects(&analysis.hir, &analysis.types);
    let modules = emit_tla_modules(&aspect_report);
    let check_results = match checker_config {
        Some(config) => TlaAspectVerifier::new(config).verify(&aspect_report),
        None => checker_unavailable_results(&modules),
    };
    summarize_hir_aspect_coverage(&aspect_report, &check_results, HirAspectBackendKind::Tla)
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
fn checker_unavailable_results(modules: &[HirAspectTlaModule]) -> Vec<HirAspectCheckResult> {
    modules
        .iter()
        .map(|_| HirAspectCheckResult {
            status: HirAspectCheckStatus::ToolchainMissing,
            evidence: HirAspectEvidence(TLC_JAR_UNSET_SUMMARY.to_owned()),
            counterexample: Vec::new(),
            backend: HirAspectBackendKind::Tla,
        })
        .collect()
}
