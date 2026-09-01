//! Typed verify service. Printing and process exit live in `radix-cli`.

use crate::aspect::{
    HirAspectBackendKind, HirAspectCoverageReport, HirAspectCoverageStatus, HirAspectTlaModule, HirAspectVerifier,
    TlaAspectCheckerConfig, TlaAspectVerifier, emit_tla_modules, report_hir_aspects, summarize_hir_aspect_coverage,
};
use crate::diagnostics::Diagnostic;
use crate::driver::{AnalyzedModule, Config, Session};

const TLC_JAR_UNSET_SUMMARY: &str = "FABER_TLA_TLC_JAR is not set";

#[derive(Debug)]
pub struct VerifySourceResult {
    pub coverage: Option<HirAspectCoverageReport>,
    pub diagnostics: Vec<Diagnostic>,
    pub success: bool,
}

#[must_use]
pub fn verify_source(name: &str, source: &str, checker_config: Option<TlaAspectCheckerConfig>) -> VerifySourceResult {
    let session = Session::new(Config::default().with_dev_stdlib());
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
        Some(config) => {
            let verifier = TlaAspectVerifier::new(config);
            let verifier_results = verifier.verify(&aspect_report);
            modules
                .iter()
                .zip(verifier_results)
                .map(|(module, vr)| {
                    let (summary, counterexample) =
                        crate::aspect::tla_checker::split_evidence_with_counterexample(&vr.evidence.0);
                    crate::aspect::tla_checker::HirAspectCheckResult {
                        module_name: module.name.clone(),
                        status: map_verifier_status(vr.status),
                        summary,
                        counterexample,
                    }
                })
                .collect()
        }
        None => checker_unavailable_results(&modules),
    };
    summarize_hir_aspect_coverage(&aspect_report, &check_results, HirAspectBackendKind::Tla)
}

fn map_verifier_status(
    status: crate::aspect::HirAspectCheckStatus,
) -> crate::aspect::tla_checker::HirAspectCheckStatus {
    use crate::aspect::HirAspectCheckStatus as New;
    use crate::aspect::tla_checker::HirAspectCheckStatus as Legacy;
    match status {
        New::Proven => Legacy::Passed,
        New::Failed => Legacy::Failed,
        New::Error => Legacy::CheckerError,
        New::Timeout => Legacy::CheckerError,
        New::ToolchainMissing => Legacy::CheckerUnavailable,
    }
}

#[must_use]
fn coverage_proves_success(coverage: &HirAspectCoverageReport) -> bool {
    let entries = coverage.entries();
    !entries.is_empty()
        && entries
            .iter()
            .all(|entry| entry.status == HirAspectCoverageStatus::Proven)
}

fn checker_unavailable_results(
    modules: &[HirAspectTlaModule],
) -> Vec<crate::aspect::tla_checker::HirAspectCheckResult> {
    modules
        .iter()
        .map(|module| {
            crate::aspect::tla_checker::HirAspectCheckResult::checker_unavailable(&module.name, TLC_JAR_UNSET_SUMMARY)
        })
        .collect()
}
