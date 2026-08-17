//! Experimental `HIR` aspect verification command backed by TLA+ model checking.
//!
//! Owns [`cmd_verify`], [`verify_source`], TLA+ checker configuration from
//! environment, and aspect-coverage reporting. Delegates semantic analysis to
//! `crate::driver::analyze_source` and TLA+ model emission / checking to
//! `crate::aspect`. Exits with code 1 when verification fails, aspects are
//! unproven, or the checker environment is misconfigured.

use super::super::cli::VerifyCommand;
use super::package::should_treat_as_package_from_input;
use super::source::read_source;
use crate::aspect::{
    emit_tla_modules, render_hir_aspect_coverage, report_hir_aspects, summarize_hir_aspect_coverage,
    HirAspectBackendKind, HirAspectCoverageReport, HirAspectCoverageStatus, HirAspectTlaModule, HirAspectVerifier,
    TlaAspectCheckerConfig, TlaAspectVerifier,
};
use crate::diagnostics::{render_expanded_diagnostics, Diagnostic};
use crate::driver::{analyze_source, AnalyzedModule, Config, Session};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const TLC_JAR_UNSET_SUMMARY: &str = "FABER_TLA_TLC_JAR is not set";

pub fn cmd_verify(command: VerifyCommand) {
    reject_package_verify_if_needed(&command);

    let (name, source) = read_source(&command.input);
    let checker_config = checker_config_from_env_or_exit();
    let result = verify_source(&name, &source, checker_config);
    report_verify_result_or_exit(&result);
}

fn reject_package_verify_if_needed(command: &VerifyCommand) {
    if command.package || should_treat_as_package_from_input(&command.input) {
        eprintln!("error: package verification is owned by the `faber` tool; rerun with `faber verify` after Stage 7");
        std::process::exit(1);
    }
}

fn checker_config_from_env_or_exit() -> Option<TlaAspectCheckerConfig> {
    match TlaAspectCheckerConfig::from_env(default_verify_work_dir()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}

fn report_verify_result_or_exit(result: &VerifySourceResult) {
    if !result.diagnostics.is_empty() {
        eprintln!("{}", render_expanded_diagnostics(&result.diagnostics));
    }
    if let Some(coverage) = &result.coverage {
        print!("{}", render_hir_aspect_coverage(coverage));
    }
    if !result.success {
        std::process::exit(1);
    }
}

#[derive(Debug)]
pub struct VerifySourceResult {
    pub coverage: Option<HirAspectCoverageReport>,
    pub diagnostics: Vec<Diagnostic>,
    pub success: bool,
}

#[must_use]
pub fn verify_source(name: &str, source: &str, checker_config: Option<TlaAspectCheckerConfig>) -> VerifySourceResult {
    let session = Session::new(Config::default().with_dev_stdlib());
    let analysis = match analyze_source(&session, name, source) {
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
    use crate::aspect::tla_checker::HirAspectCheckStatus as Legacy;
    use crate::aspect::HirAspectCheckStatus as New;
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

#[must_use]
fn default_verify_work_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("radix-verify-{}-{nanos}", std::process::id()))
}
