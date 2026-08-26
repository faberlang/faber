//! Tests for TLA+ checker integration (TLC).
//! Proves: missing-jar, missing-java, and invalid-module handling;
//! stdout normalization (pass, fail, error, timeout); env-path
//! canonicalization.

use super::{check_tla_modules, HirAspectCheckStatus, HirAspectTlaModule, TlaAspectChecker, TlaAspectCheckerConfig};
use std::ffi::OsString;
use std::path::PathBuf;

fn temp_work_dir(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = crate::test_support::test_temp_dir(name);
    let path = dir.path().to_path_buf();
    (dir, path)
}

fn module(text: &str) -> HirAspectTlaModule {
    HirAspectTlaModule { name: "FaberAspectDiscerneOmnia_Hir7".to_owned(), text: text.to_owned() }
}

#[test]
fn hir_aspect_checker_reports_missing_tlc_jar_without_claiming_success() {
    let (_td, work_dir) = temp_work_dir("missing-tlc-jar");
    let module = module("---- MODULE FaberAspectDiscerneOmnia_Hir7 ----\n====\n");
    let config = TlaAspectCheckerConfig {
        java_bin: PathBuf::from("java"),
        tla_tools_jar: work_dir.join("missing-tla2tools.jar"),
        work_dir: work_dir.clone(),
    };

    let results = check_tla_modules(config, &[module]);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, HirAspectCheckStatus::CheckerUnavailable);
    assert!(work_dir.join("FaberAspectDiscerneOmnia_Hir7.tla").is_file());
    assert!(work_dir.join("FaberAspectDiscerneOmnia_Hir7.cfg").is_file());
}

#[test]
fn hir_aspect_checker_reports_missing_java_without_claiming_success() {
    let (_td, work_dir) = temp_work_dir("missing-java");
    let jar_path = work_dir.join("fake-tla2tools.jar");
    std::fs::write(&jar_path, "").expect("fake jar file is written");
    let module = module("---- MODULE FaberAspectDiscerneOmnia_Hir7 ----\n====\n");
    let config = TlaAspectCheckerConfig {
        java_bin: work_dir.join("missing-java"),
        tla_tools_jar: jar_path,
        work_dir: work_dir.clone(),
    };

    let results = check_tla_modules(config, &[module]);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, HirAspectCheckStatus::CheckerUnavailable);
    assert_eq!(results[0].summary, "java executable is unavailable");
}

#[test]
fn hir_aspect_checker_normalizes_tlc_success() {
    let result = super::normalize_tlc_output("Spec", true, "TLC2 Version 2.19\nNo error has been found.\n", "");

    assert_eq!(result.status, HirAspectCheckStatus::Passed);
    assert!(result.counterexample.is_empty());
}

#[test]
fn hir_aspect_checker_normalizes_tlc_failure_trace() {
    let result = super::normalize_tlc_output(
        "Spec",
        false,
        "Error: Invariant Totality is violated.\n\
The following behavior constitutes a counter-example:\n\
State 1: <Initial predicate>\n",
        "",
    );

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert_eq!(result.summary, "Error: Invariant Totality is violated.");
    assert_eq!(
        result.counterexample,
        vec![
            "The following behavior constitutes a counter-example:".to_owned(),
            "State 1: <Initial predicate>".to_owned(),
        ]
    );
}

#[test]
fn hir_aspect_checker_does_not_pass_nonzero_tlc_success_marker() {
    let result = super::normalize_tlc_output("Spec", false, "No error has been found.\n", "");

    assert_eq!(result.status, HirAspectCheckStatus::CheckerError);
    assert_eq!(result.summary, "No error has been found.");
}

#[test]
fn hir_aspect_checker_prefers_failure_markers_on_nonzero_tlc_exit() {
    let result = super::normalize_tlc_output(
        "Spec",
        false,
        "No error has been found.\nError: Invariant Totality is violated.\n",
        "",
    );

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert_eq!(result.summary, "Error: Invariant Totality is violated.");
}

#[test]
fn hir_aspect_checker_normalizes_unknown_tlc_output_as_error() {
    let result = super::normalize_tlc_output("Spec", false, "Unexpected TLC output", "");

    assert_eq!(result.status, HirAspectCheckStatus::CheckerError);
    assert_eq!(result.summary, "Unexpected TLC output");
}

#[test]
fn hir_aspect_checker_env_paths_are_canonicalized_before_work_dir_use() {
    // Hermetic: prove relative-path canonicalization without mutating the
    // process CWD (set_current_dir flipped stdlib/locale resolution for
    // concurrently running en-surface tests).
    fn relative_to_cwd(target: &std::path::Path) -> PathBuf {
        let cwd = std::env::current_dir().expect("current dir");
        let mut cwd_iter = cwd.components();
        let target_iter = target.components();
        let mut common = 0usize;
        let target_components: Vec<_> = target_iter.collect();
        for target_component in &target_components {
            if cwd_iter.next().as_ref() != Some(target_component) {
                break;
            }
            common += 1;
        }
        let depth = cwd.components().count() - common;
        let mut path = PathBuf::new();
        for _ in 0..depth {
            path.push("..");
        }
        for component in &target_components[common..] {
            path.push(component.as_os_str());
        }
        path
    }

    let (_td, work_dir) = temp_work_dir("canonical-env-paths");
    let bin_dir = work_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("temp bin dir");
    std::fs::write(work_dir.join("tla2tools.jar"), "").expect("fake jar");
    std::fs::write(bin_dir.join("java"), "").expect("fake java");

    let checker_work = work_dir.join("checker-work");
    let jar = relative_to_cwd(&work_dir.join("tla2tools.jar")).into_os_string();
    let java = relative_to_cwd(&bin_dir.join("java")).into_os_string();
    let config = TlaAspectCheckerConfig::from_env_values(&checker_work, &jar, &java)
        .expect("relative env paths should canonicalize");

    assert!(config.tla_tools_jar.is_absolute());
    assert!(config.java_bin.is_absolute());
    assert_eq!(
        config.tla_tools_jar,
        std::fs::canonicalize(work_dir.join("tla2tools.jar")).expect("canonical jar")
    );
    assert_eq!(
        config.java_bin,
        std::fs::canonicalize(bin_dir.join("java")).expect("canonical java")
    );
}

#[test]
fn hir_aspect_checker_keeps_bare_java_command_from_env() {
    let (_td, work_dir) = temp_work_dir("bare-java-env");
    let jar = work_dir.join("tla2tools.jar");
    std::fs::write(&jar, "").expect("fake jar");

    let checker_work = work_dir.join("checker-work");
    let java = OsString::from("java");
    let config = TlaAspectCheckerConfig::from_env_values(&checker_work, jar.as_os_str(), &java)
        .expect("absolute jar and bare java should configure");

    assert_eq!(config.java_bin, PathBuf::from("java"));
    assert!(config.tla_tools_jar.is_absolute());
}

#[test]
fn hir_aspect_checker_reports_invalid_tlc_jar_env_path() {
    let (_td, work_dir) = temp_work_dir("invalid-jar-env");
    let checker_work = work_dir.join("checker-work");
    let missing_jar = work_dir.join("missing-tla2tools.jar");
    let java = OsString::from("java");
    let err = TlaAspectCheckerConfig::from_env_values(&checker_work, missing_jar.as_os_str(), &java)
        .expect_err("missing jar path should report a config error");

    assert!(err.contains("FABER_TLA_TLC_JAR path"));
    assert!(err.contains("missing-tla2tools.jar"));
}

#[test]
fn hir_aspect_checker_reports_invalid_pathful_java_env_path() {
    let (_td, work_dir) = temp_work_dir("invalid-java-env");
    let jar = work_dir.join("tla2tools.jar");
    std::fs::write(&jar, "").expect("fake jar");

    let checker_work = work_dir.join("checker-work");
    let missing_java = work_dir.join("bin/missing-java");
    let err = TlaAspectCheckerConfig::from_env_values(&checker_work, jar.as_os_str(), missing_java.as_os_str())
        .expect_err("missing pathful java path should report a config error");

    assert!(err.contains("FABER_TLA_JAVA_BIN path"));
    assert!(err.contains("missing-java"));
}

#[test]
fn hir_aspect_checker_rejects_invalid_module_names() {
    let (_td, work_dir) = temp_work_dir("invalid-module-name");
    let config = TlaAspectCheckerConfig {
        java_bin: PathBuf::from("java"),
        tla_tools_jar: work_dir.join("missing-tla2tools.jar"),
        work_dir: work_dir.clone(),
    };
    let checker = TlaAspectChecker::new(config);
    let module = HirAspectTlaModule { name: "../Bad".to_owned(), text: String::new() };

    let results = checker.check_modules(&[module]);

    assert_eq!(results[0].status, HirAspectCheckStatus::CheckerError);
    assert_eq!(results[0].summary, "invalid TLA module name");
    assert!(
        std::fs::read_dir(&work_dir).is_ok_and(|mut entries| entries.next().is_none()),
        "checker must not create files for invalid module names"
    );
}

#[test]
fn hir_aspect_checker_tlc_integration_runs_when_env_is_set() {
    let (_td, tmp) = temp_work_dir("tlc-integration");
    let Ok(Some(config)) = TlaAspectCheckerConfig::from_env(tmp) else {
        return;
    };
    let module = module(
        "---- MODULE FaberAspectDiscerneOmnia_Hir7 ----\n\
EXTENDS TLC\n\
Variant_2 == \"Variant_2\"\n\
Variants == {Variant_2}\n\
Covered == {Variant_2}\n\
VARIABLE checker_state\n\
Init == checker_state = \"init\"\n\
Next == UNCHANGED checker_state\n\
Spec == Init /\\ [][Next]_checker_state\n\
Totality == Variants \\subseteq Covered\n\
=============================================================================\n",
    );

    let results = check_tla_modules(config, &[module]);

    assert_eq!(results[0].status, HirAspectCheckStatus::Passed);
}

#[test]
fn hir_aspect_checker_verifier_bridge_preserves_counterexample() {
    let legacy = super::HirAspectCheckResult::failed(
        "Spec",
        "Error: Invariant Totality is violated.",
        vec![
            "The following behavior constitutes a counter-example:".to_owned(),
            "State 1: <Initial predicate>".to_owned(),
        ],
    );

    let verifier_result = super::map_legacy_result(legacy);

    assert_eq!(verifier_result.status, crate::aspect::HirAspectCheckStatus::Failed);
    let (summary, counterexample) = super::split_evidence_with_counterexample(&verifier_result.evidence.0);
    assert_eq!(summary, "Error: Invariant Totality is violated.");
    assert_eq!(
        counterexample,
        vec![
            "The following behavior constitutes a counter-example:".to_owned(),
            "State 1: <Initial predicate>".to_owned(),
        ]
    );
}

#[test]
fn hir_aspect_checker_verifier_bridge_preserves_plain_summary_without_counterexample() {
    let legacy = super::HirAspectCheckResult::passed("Spec", "TLC completed with no error");

    let verifier_result = super::map_legacy_result(legacy);

    assert_eq!(verifier_result.status, crate::aspect::HirAspectCheckStatus::Proven);
    let (summary, counterexample) = super::split_evidence_with_counterexample(&verifier_result.evidence.0);
    assert_eq!(summary, "TLC completed with no error");
    assert!(counterexample.is_empty());
}

#[test]
fn hir_aspect_checker_does_not_flip_success_on_loose_invariant_substring() {
    let result = super::normalize_tlc_output(
        "Spec",
        true,
        "TLC2 Version 2.19\nInvariant Totality is being checked\nNo error has been found.\n",
        "",
    );

    assert_eq!(result.status, HirAspectCheckStatus::Passed);
    assert!(result.counterexample.is_empty());
}

#[test]
fn hir_aspect_checker_reports_toolchain_error_without_violation_markers_as_error() {
    let result = super::normalize_tlc_output("Spec", false, "Error: Could not find or load main class tlc2.TLC\n", "");

    assert_eq!(result.status, HirAspectCheckStatus::CheckerError);
    assert_eq!(result.summary, "Error: Could not find or load main class tlc2.TLC");
}

#[test]
fn hir_aspect_checker_reports_stack_trace_quoting_invariant_as_error() {
    let result = super::normalize_tlc_output(
        "Spec",
        false,
        "java.lang.Error: Invariant Totality could not be evaluated\n\tat tlc2.TLC.main(TLC.java)\n",
        "",
    );

    assert_eq!(result.status, HirAspectCheckStatus::CheckerError);
    assert_eq!(result.summary, "java.lang.Error: Invariant Totality could not be evaluated");
}

#[test]
fn hir_aspect_checker_failure_trace_without_error_line_still_failed() {
    let result = super::normalize_tlc_output(
        "Spec",
        true,
        "The following behavior constitutes a counter-example:\nState 1: <Initial predicate>\n",
        "",
    );

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert_eq!(
        result.counterexample,
        vec![
            "The following behavior constitutes a counter-example:".to_owned(),
            "State 1: <Initial predicate>".to_owned(),
        ]
    );
}
