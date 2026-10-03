//! Tests for Lean checker integration (lake build + axiom/sorry audit).
//! Proves: normalization (proven, failed, error, toolchain-missing,
//! timeout); axiom allowlist (propext, Classical.choice, Quot.sound);
//! sorry detection; combined verdict priority.

use super::{
    AxiomAudit, AxiomDependency, AxiomVerdict, LakeResult, LeanAspectChecker, LeanAspectCheckerConfig, SorryVerdict,
    combined_verdict, normalize_result, parse_axiom_output,
};
use crate::aspect::verifier::{HirAspectCheckResult, HirAspectCheckStatus, HirAspectEvidence, HirAspectVerifier};
use crate::aspect::{
    HirAspectBackendKind, HirAspectDetail, HirAspectKind, HirAspectPrecondition, HirAspectReason, HirAspectRecord,
    HirAspectReport, HirAspectSpan, MaskNeutralityFamily,
};
use crate::lexer::Span;
use std::path::PathBuf;
use std::time::Duration;

// ---------------------------------------------------------------------------
// normalization tests (no Lean required)
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_normalization_valid_build_is_proven() {
    let lake = LakeResult {
        exit_code: Some(0),
        stdout: "Build completed successfully.\n".to_owned(),
        stderr: String::new(),
        timed_out: false,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Proven);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
    assert!(result.evidence.0.contains("Build completed"));
}

#[test]
fn lean_checker_normalization_syntax_error_is_failed() {
    let lake = LakeResult {
        exit_code: Some(1),
        stdout: String::new(),
        stderr: "error: syntax error at MyModule.lean:3:1: unexpected token ';'\n".to_owned(),
        timed_out: false,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
    assert!(result.evidence.0.contains("syntax error"));
}

#[test]
fn lean_checker_normalization_type_error_is_failed() {
    let lake = LakeResult {
        exit_code: Some(1),
        stdout: String::new(),
        stderr: "error: type mismatch\n  has type\n    Nat\n  but is expected to have type\n    String\n".to_owned(),
        timed_out: false,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
    assert!(result.evidence.0.contains("type mismatch"));
}

#[test]
fn lean_checker_normalization_nonzero_exit_is_failed() {
    let lake = LakeResult {
        exit_code: Some(2),
        stdout: String::new(),
        stderr: "error: unknown identifier 'foo'\n".to_owned(),
        timed_out: false,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
}

#[test]
fn lean_checker_normalization_launch_failure_is_error() {
    let lake = LakeResult {
        exit_code: None,
        stdout: String::new(),
        stderr: "failed to launch lake: permission denied".to_owned(),
        timed_out: false,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Error);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
}

#[test]
fn lean_checker_normalization_empty_stdout_zero_exit_is_proven() {
    let lake = LakeResult { exit_code: Some(0), stdout: String::new(), stderr: String::new(), timed_out: false };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Proven);
}

#[test]
fn lean_checker_normalization_no_exit_code_without_toolchain_evidence_is_error() {
    let lake = LakeResult {
        exit_code: None,
        stdout: String::new(),
        stderr: "segmentation fault".to_owned(),
        timed_out: false,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Error);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
}

// ---------------------------------------------------------------------------
// missing toolchain tests (no Lean required)
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_missing_toolchain_detects_missing_binary() {
    // Construct a checker that points to clearly non-existent binaries.
    let temp_dir = crate::test_support::test_temp_dir("lean-missing-toolchain");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };

    let checker = LeanAspectChecker::new(config, temp.clone());

    let lake = checker.run_lake_build(&temp);
    assert!(!lake.timed_out);
    assert_eq!(lake.exit_code, None);
    assert!(lake.stderr.contains("not found on PATH"));

    let result = normalize_result(lake);
    assert_eq!(result.status, HirAspectCheckStatus::ToolchainMissing);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
}

#[test]
fn lean_checker_missing_toolchain_normalizes_prebuilt_lake_result() {
    let lake = LakeResult {
        exit_code: None,
        stdout: String::new(),
        stderr: "lake (lake) or lean (lean) not found on PATH".to_owned(),
        timed_out: false,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::ToolchainMissing);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
    assert!(result.evidence.0.contains("not found on PATH"));
}

// ---------------------------------------------------------------------------
// timeout tests (no Lean required)
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_timeout_normalizes_to_timeout_status() {
    let lake = LakeResult {
        exit_code: None,
        stdout: String::new(),
        stderr: "lake build timed out after 5s".to_owned(),
        timed_out: true,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Timeout);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
    assert!(result.evidence.0.contains("timed out"));
}

#[test]
fn lean_checker_timeout_overrides_exit_code() {
    let lake = LakeResult {
        exit_code: Some(0),
        stdout: "Build completed successfully.\n".to_owned(),
        stderr: "lake build timed out after 30s".to_owned(),
        timed_out: true,
    };

    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Timeout);
}

// ---------------------------------------------------------------------------
// toolchain detection tests
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_config_default_uses_bare_commands() {
    let config = LeanAspectCheckerConfig::default();
    assert_eq!(config.lake_bin, PathBuf::from("lake"));
    assert_eq!(config.lean_bin, PathBuf::from("lean"));
    assert_eq!(config.timeout, Duration::from_secs(super::DEFAULT_TIMEOUT_SECS));
}

#[test]
fn lean_checker_config_custom_timeout() {
    let config = LeanAspectCheckerConfig { timeout: Duration::from_secs(10), ..LeanAspectCheckerConfig::default() };

    let checker = LeanAspectChecker::new(config, PathBuf::from("/tmp/lean-project"));
    assert_eq!(checker.config().timeout, Duration::from_secs(10));
}

#[test]
fn lean_checker_config_exposes_project_dir() {
    let dir = PathBuf::from("/tmp/lean-project");
    let checker = LeanAspectChecker::new(LeanAspectCheckerConfig::default(), dir.clone());
    assert_eq!(checker.project_dir(), dir);
}

// ---------------------------------------------------------------------------
// axiom audit + allowlist tests
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_axiom_parse_output_with_dependencies() {
    let output = "'myThm' depends on axioms: [propext, Classical.choice]\n";
    let audit = parse_axiom_output(output);

    assert_eq!(audit.dependencies.len(), 2);
    assert_eq!(audit.dependencies[0].axiom_name, "propext");
    assert_eq!(audit.dependencies[0].declaration, "myThm");
    assert_eq!(audit.dependencies[1].axiom_name, "Classical.choice");
    assert_eq!(audit.dependencies[1].declaration, "myThm");
}

#[test]
fn lean_checker_axiom_parse_output_no_axioms() {
    let output = "'simpleThm' does not depend on any axioms\n";
    let audit = parse_axiom_output(output);
    assert!(audit.dependencies.is_empty());
}

#[test]
fn lean_checker_axiom_parse_output_multiple_decls() {
    let output = concat!(
        "'thm1' depends on axioms: [propext]\n",
        "'thm2' depends on axioms: [Quot.sound]\n",
        "'thm3' does not depend on any axioms\n",
    );
    let audit = parse_axiom_output(output);

    assert_eq!(audit.dependencies.len(), 2);
    assert_eq!(audit.dependencies[0].axiom_name, "propext");
    assert_eq!(audit.dependencies[0].declaration, "thm1");
    assert_eq!(audit.dependencies[1].axiom_name, "Quot.sound");
    assert_eq!(audit.dependencies[1].declaration, "thm2");
}

#[test]
fn lean_checker_axiom_parse_output_empty() {
    let audit = parse_axiom_output("");
    assert!(audit.dependencies.is_empty());
}

// ---------------------------------------------------------------------------
// APA P2-1 (want, ruling 6a0fafde option (b)): empty #print-axioms
// output — the silent unresolved wrapper import — must never classify Allowed.
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_axiom_empty_print_output_never_classifies_allowed() {
    // An unresolved wrapper import can present as EMPTY output (lean exits
    // quietly without printing anything). No parsed decl lines and no
    // explicit error means the audit has zero evidence: it must be
    // unresolved, never a silent Allowed.
    let audit = parse_axiom_output("");
    assert!(audit.dependencies.is_empty());
    assert!(audit.unresolved_import, "empty audit output must be unresolved_import");
    assert_ne!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::Allowed);

    let result = combined_verdict(
        proven_result(),
        &LeanAspectChecker::classify_axioms(&audit),
        &SorryVerdict::Clean,
    );
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(result.evidence.0.contains("unresolved"));
}

#[test]
fn lean_checker_axiom_garbage_output_without_resolution_is_unresolved() {
    // Output with neither parsed decl lines nor explicit error evidence is
    // not proof that the wrapper import resolved.
    let audit = parse_axiom_output("some random garbage\nnot axiom output\n");
    assert!(audit.unresolved_import);
    assert_ne!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::Allowed);
}

#[test]
fn lean_checker_axiom_explicit_no_axioms_line_is_resolved() {
    // A real "does not depend on any axioms" line proves the import
    // resolved — that stays a trustworthy (allowed) audit.
    let output = "'simpleThm' does not depend on any axioms\n";
    let audit = parse_axiom_output(output);
    assert!(!audit.unresolved_import);
    assert_eq!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::Allowed);
}

#[test]
fn lean_checker_axiom_parse_output_malformed_is_empty() {
    let audit = parse_axiom_output("some random garbage\nnot axiom output\n");
    assert!(audit.dependencies.is_empty());
}

// ---------------------------------------------------------------------------
// fail-closed axiom audit (operator ruling 6a0fafde option (b)): an
// unresolved wrapper import is a FAILURE, never a silent Allowed.
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_axiom_parse_unresolved_import_marks_audit_unresolved() {
    let output = "error: unknown import: FaberGenerated\ninfo: ...";
    let audit = parse_axiom_output(output);
    assert!(audit.unresolved_import);
    assert!(audit.dependencies.is_empty());
}

#[test]
fn lean_checker_axiom_unresolved_import_never_classifies_allowed() {
    // Empty dependency list with an unresolved import must still fail: no
    // allowlist verdict is possible without trustworthy audit evidence.
    let audit = AxiomAudit { dependencies: vec![], unresolved_import: true };
    assert_eq!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::UnresolvedImport);
}

#[test]
fn lean_checker_axiom_unresolved_wrapper_import_is_failure() {
    // End-to-end classification proof (no Lean toolchain needed): when the
    // wrapper import cannot resolve — here because `lean` is unavailable —
    // the audit is unresolved and a Proven build still fails the verdict.
    let temp_dir = crate::test_support::test_temp_dir("lean-axiom-unresolved-import");
    let temp = temp_dir.path().to_path_buf();
    let module = temp.join("Unresolvable.lean");
    std::fs::write(&module, "theorem unresolvable_thm : True := trivial\n").expect("write module");

    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let audit = checker.audit_axioms(&module);
    assert!(audit.unresolved_import, "unavailable lean must leave the audit unresolved");
    let verdict = LeanAspectChecker::classify_axioms(&audit);
    assert_ne!(verdict, AxiomVerdict::Allowed, "unresolved import must never classify Allowed");

    let result = combined_verdict(proven_result(), &verdict, &SorryVerdict::Clean);
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(
        result.evidence.0.contains("unresolved"),
        "expected unresolved-import evidence, got: {}",
        result.evidence.0
    );
}

#[test]
fn lean_checker_axiom_parse_output_all_clean() {
    let output = concat!(
        "'thm1' does not depend on any axioms\n",
        "'thm2' does not depend on any axioms\n",
    );
    let audit = parse_axiom_output(output);
    assert!(audit.dependencies.is_empty());
}

#[test]
fn lean_checker_axiom_allowlist_accepts_propext() {
    let audit = AxiomAudit {
        dependencies: vec![AxiomDependency { axiom_name: "propext".into(), declaration: "thm".into() }],
        unresolved_import: false,
    };
    assert_eq!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::Allowed);
}

#[test]
fn lean_checker_axiom_allowlist_accepts_classical_choice() {
    let audit = AxiomAudit {
        dependencies: vec![AxiomDependency { axiom_name: "Classical.choice".into(), declaration: "thm".into() }],
        unresolved_import: false,
    };
    assert_eq!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::Allowed);
}

#[test]
fn lean_checker_axiom_allowlist_accepts_quot_sound() {
    let audit = AxiomAudit {
        dependencies: vec![AxiomDependency { axiom_name: "Quot.sound".into(), declaration: "thm".into() }],
        unresolved_import: false,
    };
    assert_eq!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::Allowed);
}

#[test]
fn lean_checker_axiom_allowlist_accepts_all_allowed() {
    let audit = AxiomAudit {
        dependencies: vec![
            AxiomDependency { axiom_name: "propext".into(), declaration: "thm1".into() },
            AxiomDependency { axiom_name: "Classical.choice".into(), declaration: "thm1".into() },
            AxiomDependency { axiom_name: "Quot.sound".into(), declaration: "thm2".into() },
        ],
        unresolved_import: false,
    };
    assert_eq!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::Allowed);
}

#[test]
fn lean_checker_axiom_allowlist_rejects_custom_axiom() {
    let audit = AxiomAudit {
        dependencies: vec![AxiomDependency { axiom_name: "myCustomAxiom".into(), declaration: "thm".into() }],
        unresolved_import: false,
    };
    let verdict = LeanAspectChecker::classify_axioms(&audit);
    match verdict {
        AxiomVerdict::Rejected { disallowed } => {
            assert_eq!(disallowed.len(), 1);
            assert_eq!(disallowed[0].axiom_name, "myCustomAxiom");
        }
        AxiomVerdict::Allowed | AxiomVerdict::UnresolvedImport => panic!("expected Rejected"),
    }
}

#[test]
fn lean_checker_axiom_allowlist_rejects_mixed() {
    let audit = AxiomAudit {
        dependencies: vec![
            AxiomDependency { axiom_name: "propext".into(), declaration: "thm".into() },
            AxiomDependency { axiom_name: "userAxiom".into(), declaration: "thm".into() },
        ],
        unresolved_import: false,
    };
    let verdict = LeanAspectChecker::classify_axioms(&audit);
    match verdict {
        AxiomVerdict::Rejected { disallowed } => {
            assert_eq!(disallowed.len(), 1);
            assert_eq!(disallowed[0].axiom_name, "userAxiom");
        }
        AxiomVerdict::Allowed | AxiomVerdict::UnresolvedImport => {
            panic!("expected Rejected for mixed allowed+disallowed")
        }
    }
}

#[test]
fn lean_checker_axiom_allowlist_no_axioms_is_allowed() {
    let audit = AxiomAudit { dependencies: vec![], unresolved_import: false };
    assert_eq!(LeanAspectChecker::classify_axioms(&audit), AxiomVerdict::Allowed);
}

// ---------------------------------------------------------------------------
// sorry detection tests
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_sorry_detects_token() {
    let source = "theorem foo : 1 = 1 := by\n  sorry\n";
    let verdict = LeanAspectChecker::scan_for_sorry(source);
    assert_eq!(verdict, SorryVerdict::Found { line_numbers: vec![2] });
}

#[test]
fn lean_checker_sorry_detects_multiple() {
    let source = "theorem foo : 1 = 1 := by\n  sorry\n\ntheorem bar : 1 = 2 := by\n  sorry\n";
    let verdict = LeanAspectChecker::scan_for_sorry(source);
    assert_eq!(verdict, SorryVerdict::Found { line_numbers: vec![2, 5] });
}

#[test]
fn lean_checker_sorry_clean_when_no_sorry() {
    let source = "theorem foo : 1 = 1 := rfl\n";
    let verdict = LeanAspectChecker::scan_for_sorry(source);
    assert_eq!(verdict, SorryVerdict::Clean);
}

#[test]
fn lean_checker_sorry_ignores_commented_sorry() {
    let source = "theorem foo : 1 = 1 := rfl\n-- sorry this was a placeholder\n";
    let verdict = LeanAspectChecker::scan_for_sorry(source);
    assert_eq!(verdict, SorryVerdict::Clean);
}

#[test]
fn lean_checker_sorry_detects_inline_sorry() {
    let source = "theorem foo : 1 = 1 := by\n  apply?; sorry\n";
    let verdict = LeanAspectChecker::scan_for_sorry(source);
    assert_eq!(verdict, SorryVerdict::Found { line_numbers: vec![2] });
}

#[test]
fn lean_checker_sorry_empty_source() {
    let source = "";
    let verdict = LeanAspectChecker::scan_for_sorry(source);
    assert_eq!(verdict, SorryVerdict::Clean);
}

#[test]
fn lean_checker_sorry_detects_in_string_literal() {
    // `sorry` inside a string literal is still a token match (strings are
    // not stripped before token-splitting).
    let source = "def msg := \"sorry about that\"\n";
    let verdict = LeanAspectChecker::scan_for_sorry(source);
    assert_eq!(verdict, SorryVerdict::Found { line_numbers: vec![1] });
}

// ---------------------------------------------------------------------------
// combined verdict tests
// ---------------------------------------------------------------------------

fn proven_result() -> HirAspectCheckResult {
    HirAspectCheckResult {
        status: HirAspectCheckStatus::Proven,
        evidence: HirAspectEvidence("Build completed successfully.\n".into()),
        counterexample: Vec::new(),
        backend: HirAspectBackendKind::Lean,
    }
}

fn failed_result() -> HirAspectCheckResult {
    HirAspectCheckResult {
        status: HirAspectCheckStatus::Failed,
        evidence: HirAspectEvidence("error: syntax error\n".into()),
        counterexample: Vec::new(),
        backend: HirAspectBackendKind::Lean,
    }
}

#[test]
fn lean_checker_axiom_combined_proven_when_all_clean() {
    let lake = proven_result();
    let axiom_verdict = AxiomVerdict::Allowed;
    let sorry_verdict = SorryVerdict::Clean;

    let result = combined_verdict(lake, &axiom_verdict, &sorry_verdict);
    assert_eq!(result.status, HirAspectCheckStatus::Proven);
}

#[test]
fn lean_checker_axiom_combined_fails_on_sorry() {
    let lake = proven_result();
    let axiom_verdict = AxiomVerdict::Allowed;
    let sorry_verdict = SorryVerdict::Found { line_numbers: vec![3] };

    let result = combined_verdict(lake, &axiom_verdict, &sorry_verdict);
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(result.evidence.0.contains("sorry"));
}

#[test]
fn lean_checker_axiom_combined_fails_on_disallowed_axiom() {
    let lake = proven_result();
    let axiom_verdict = AxiomVerdict::Rejected {
        disallowed: vec![AxiomDependency { axiom_name: "badAxiom".into(), declaration: "thm".into() }],
    };
    let sorry_verdict = SorryVerdict::Clean;

    let result = combined_verdict(lake, &axiom_verdict, &sorry_verdict);
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(result.evidence.0.contains("badAxiom"));
}

#[test]
fn lean_checker_axiom_combined_passes_through_non_proven() {
    let lake = failed_result();
    let axiom_verdict = AxiomVerdict::Allowed;
    let sorry_verdict = SorryVerdict::Clean;

    let result = combined_verdict(lake, &axiom_verdict, &sorry_verdict);
    // Even though axioms are clean and no sorry, the original Failed
    // result takes precedence.
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
}

#[test]
fn lean_checker_axiom_combined_sorry_takes_priority_over_axioms() {
    let lake = proven_result();
    let axiom_verdict = AxiomVerdict::Rejected {
        disallowed: vec![AxiomDependency { axiom_name: "badAxiom".into(), declaration: "thm".into() }],
    };
    let sorry_verdict = SorryVerdict::Found { line_numbers: vec![5] };

    let result = combined_verdict(lake, &axiom_verdict, &sorry_verdict);
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    // sorry is checked first
    assert!(result.evidence.0.contains("sorry"));
}

#[test]
fn lean_checker_sorry_combined_fails_when_sorry_present() {
    let lake = proven_result();
    let axiom_verdict = AxiomVerdict::Allowed;
    let sorry_verdict = SorryVerdict::Found { line_numbers: vec![1, 3] };

    let result = combined_verdict(lake, &axiom_verdict, &sorry_verdict);
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(result.evidence.0.contains("[1, 3]"));
}

#[test]
fn lean_checker_sorry_combined_passes_when_no_sorry_and_axioms_allowed() {
    let lake = proven_result();
    let axiom_verdict = AxiomVerdict::Allowed;
    let sorry_verdict = SorryVerdict::Clean;

    let result = combined_verdict(lake, &axiom_verdict, &sorry_verdict);
    assert_eq!(result.status, HirAspectCheckStatus::Proven);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
}

// ---------------------------------------------------------------------------
// lake integration tests (Lean on PATH required — #[ignore] by default)
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/aspect/testdata/lean")
}

fn fixture_path(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

fn default_checker() -> LeanAspectChecker {
    LeanAspectChecker::new(LeanAspectCheckerConfig::default(), fixtures_dir())
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lake_integration_valid_theorem_proven() {
    let checker = default_checker();
    let lake = checker.run_lake_build(&fixtures_dir());
    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Proven);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
    assert!(!result.evidence.0.is_empty());
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lake_integration_syntax_error_failed() {
    let checker = default_checker();
    let lake = checker.run_lake_build(&fixtures_dir());
    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
    assert!(
        result.evidence.0.to_lowercase().contains("error"),
        "expected error evidence, got: {}",
        result.evidence.0
    );
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lake_integration_type_error_failed() {
    let checker = default_checker();
    let lake = checker.run_lake_build(&fixtures_dir());
    let result = normalize_result(lake);

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
    assert!(
        result.evidence.0.to_lowercase().contains("error"),
        "expected error evidence, got: {}",
        result.evidence.0
    );
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lake_integration_direct_sorry_failed() {
    let checker = default_checker();
    let lake = checker.run_lake_build(&fixtures_dir());

    // Lake build succeeds (sorry is a valid term), but trust check
    // catches the sorry token.
    let source = std::fs::read_to_string(fixture_path("direct_sorry.lean")).expect("failed to read direct_sorry.lean");
    let sorry_verdict = LeanAspectChecker::scan_for_sorry(&source);

    let build_result = normalize_result(lake);
    // Lake build should succeed (sorry type-checks).
    assert_eq!(build_result.status, HirAspectCheckStatus::Proven);

    // Combined verdict: sorry detection must fail the proof.
    let axiom_verdict = AxiomVerdict::Allowed;
    let result = combined_verdict(build_result, &axiom_verdict, &sorry_verdict);

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(
        result.evidence.0.to_lowercase().contains("sorry"),
        "expected sorry evidence, got: {}",
        result.evidence.0
    );
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lake_integration_transitive_sorry_ax_failed() {
    let checker = default_checker();
    let lake = checker.run_lake_build(&fixtures_dir());

    // Lake build succeeds (sorry type-checks), but #print axioms
    // should reveal sorryAx as a transitive dependency.
    let module_path = fixture_path("transitive_sorry_ax.lean");
    let axiom_audit = checker.audit_axioms(&module_path);

    let build_result = normalize_result(lake);
    assert_eq!(build_result.status, HirAspectCheckStatus::Proven);

    // Classify axioms — sorryAx is not in the allowlist.
    let axiom_verdict = LeanAspectChecker::classify_axioms(&axiom_audit);
    let sorry_verdict = SorryVerdict::Clean;

    let result = combined_verdict(build_result, &axiom_verdict, &sorry_verdict);
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(
        result.evidence.0.contains("sorryAx") || result.evidence.0.to_lowercase().contains("disallowed"),
        "expected sorryAx or disallowed evidence, got: {}",
        result.evidence.0
    );
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lake_integration_custom_axiom_failed() {
    let checker = default_checker();
    let lake = checker.run_lake_build(&fixtures_dir());

    // my_custom_axiom is not in the allowlist.
    let module_path = fixture_path("custom_axiom.lean");
    let axiom_audit = checker.audit_axioms(&module_path);
    let axiom_verdict = LeanAspectChecker::classify_axioms(&axiom_audit);

    let build_result = normalize_result(lake);
    assert_eq!(build_result.status, HirAspectCheckStatus::Proven);

    let sorry_verdict = SorryVerdict::Clean;
    let result = combined_verdict(build_result, &axiom_verdict, &sorry_verdict);

    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(
        result.evidence.0.contains("my_custom_axiom") || result.evidence.0.to_lowercase().contains("disallowed"),
        "expected axiom-rejection evidence, got: {}",
        result.evidence.0
    );
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lake_integration_allowed_axioms_proven() {
    let checker = default_checker();
    let lake = checker.run_lake_build(&fixtures_dir());

    // propext, Classical.choice, Quot.sound are all allowed.
    let module_path = fixture_path("allowed_axioms.lean");
    let axiom_audit = checker.audit_axioms(&module_path);
    let axiom_verdict = LeanAspectChecker::classify_axioms(&axiom_audit);

    let build_result = normalize_result(lake);
    assert_eq!(build_result.status, HirAspectCheckStatus::Proven);

    let source = std::fs::read_to_string(&module_path).expect("failed to read allowed_axioms.lean");
    let sorry_verdict = LeanAspectChecker::scan_for_sorry(&source);
    assert_eq!(sorry_verdict, SorryVerdict::Clean);

    let result = combined_verdict(build_result, &axiom_verdict, &sorry_verdict);
    assert_eq!(result.status, HirAspectCheckStatus::Proven);
    assert_eq!(result.backend, HirAspectBackendKind::Lean);
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lake_integration_timeout() {
    // Use a sub-millisecond timeout to force a timeout on any real lake build.
    let config = LeanAspectCheckerConfig { timeout: Duration::from_millis(1), ..LeanAspectCheckerConfig::default() };
    let checker = LeanAspectChecker::new(config, fixtures_dir());
    let lake = checker.run_lake_build(&fixtures_dir());

    // With a 1ms timeout, lake should either time out or fail to start.
    // Either way, normalize_result should produce Timeout or Error.
    let result = normalize_result(lake);

    // If timed_out is true, we get Timeout.  If the process never spawned
    // (exit_code is None, timed_out is false), that's OK in CI — the
    // timeout mechanism is unit-tested separately.
    assert!(
        result.status == HirAspectCheckStatus::Timeout || result.status == HirAspectCheckStatus::Error,
        "expected Timeout or Error with 1ms deadline, got {:?}",
        result.status
    );
}

// ---------------------------------------------------------------------------
// HirAspectVerifier integration tests
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_verifier_empty_report_returns_empty() {
    let config = LeanAspectCheckerConfig::default();
    let checker = LeanAspectChecker::new(config, PathBuf::from("/tmp/nonexistent"));
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![]);
    let results = checker.verify(&report);
    assert!(results.is_empty());
}

#[test]
fn lean_checker_verifier_no_toolchain_returns_toolchain_missing() {
    // Point to non-existent binaries so toolchain check fails.
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-no-tc");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let record =
        HirAspectRecord::recognized(HirAspectKind::DiscerneOmnia, HirAspectSpan::Source(Span::new(0, 10)), None);
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 1, "expected one result per recognized record");
    // Without a real Lean toolchain, status must be ToolchainMissing or Error.
    assert!(
        results[0].status == HirAspectCheckStatus::ToolchainMissing || results[0].status == HirAspectCheckStatus::Error,
        "expected ToolchainMissing or Error, got {:?}",
        results[0].status
    );
    assert_eq!(results[0].backend, HirAspectBackendKind::Lean);
}

#[test]
fn lean_checker_verifier_multiple_records_each_get_result() {
    // Point to non-existent binaries so toolchain check fails.
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-multi");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let records: Vec<HirAspectRecord> = (0..3)
        .map(|i| {
            HirAspectRecord::recognized(
                HirAspectKind::DiscerneOmnia,
                HirAspectSpan::Source(Span::new(i * 10, i * 10 + 10)),
                None,
            )
        })
        .collect();
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, records);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 3, "expected one result per recognized record");
    for result in &results {
        assert_eq!(result.backend, HirAspectBackendKind::Lean);
        assert!(
            result.status == HirAspectCheckStatus::ToolchainMissing || result.status == HirAspectCheckStatus::Error,
            "expected ToolchainMissing or Error, got {:?}",
            result.status
        );
    }
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lean_checker_verifier_integration_full_verify() {
    let config = LeanAspectCheckerConfig::default();
    let fixtures = fixtures_dir();
    let checker = LeanAspectChecker::new(config, fixtures.clone());

    let record =
        HirAspectRecord::recognized(HirAspectKind::DiscerneOmnia, HirAspectSpan::Source(Span::new(0, 10)), None);
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    // verify() generates Lean source, writes a temp project, runs lake build,
    // and runs trust checks.  The generated theorem uses `trivial` which is
    // always provable, so the result should be Proven (when Lean is on PATH).
    let results = checker.verify(&report);
    assert!(!results.is_empty(), "expected at least one result");
    // The generated source uses `trivial` — lake build should succeed.
    // (If lake is not on PATH, the test is #[ignore] anyway.)
    assert_eq!(results[0].status, HirAspectCheckStatus::Proven);
    assert_eq!(results[0].backend, HirAspectBackendKind::Lean);
}

// ---------------------------------------------------------------------------
// APA-1 pins: no AIR proof obligation flows through the Lean checker today.
// Goal: docs/factory/air-proof-aspect. The three AIR proof lines are owned by
// APA-L1/L2/L3; the placeholder below is never AIR evidence.
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_generated_source_is_true_trivial_placeholder_not_air_proof() {
    let record = HirAspectRecord::recognized(
        HirAspectKind::DiscerneOmnia,
        HirAspectSpan::Source(Span::new(0, 10)),
        Some(crate::hir::HirId(1)),
    );

    let source = super::generate_lean_source(&[&record]);

    // Current posture: the generated theorem body is the True/trivial
    // placeholder. It carries no AIR obligation and must never be cited as
    // AIR proof evidence (that is APA-L1/L2/L3 work).
    assert!(source.contains(": True := trivial"));
    assert!(!source.to_lowercase().contains("unroll"));
    assert!(!source.to_lowercase().contains("mask"));
    assert!(!source.to_lowercase().contains("purity"));
}

#[test]
fn lean_checker_verifier_fabricates_no_results_for_unproven_records() {
    // No false Proven: records the recognizer could not prove (NotProven,
    // SkippedCandidate) never reach the checker at all, so no verdict —
    // Proven or otherwise — is fabricated for them.
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa1-no-false-proven");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let span = HirAspectSpan::Source(Span::new(0, 10));
    let records = vec![
        HirAspectRecord::not_proven(
            HirAspectKind::DiscerneOmnia,
            span,
            None,
            HirAspectReason::MissingExplicitVariants,
        ),
        HirAspectRecord::skipped_candidate(HirAspectKind::DiscerneOmnia, span, None, HirAspectReason::NonOmniaDiscerne),
    ];
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, records);

    let results = checker.verify(&report);
    assert!(results.is_empty(), "unproven records must yield no checker results");
}

#[test]
fn lean_checker_missing_toolchain_is_never_proven_for_recognized_records() {
    // Pin (no false Proven): a recognized record with no Lean toolchain must
    // normalize to ToolchainMissing/Error — never a proof. Missing tooling is
    // an explicit unavailable/error result, never a pass.
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa1-missing-tc");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let record =
        HirAspectRecord::recognized(HirAspectKind::DiscerneOmnia, HirAspectSpan::Source(Span::new(0, 10)), None);
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 1);
    assert_ne!(results[0].status, HirAspectCheckStatus::Proven);
    assert!(
        results[0].status == HirAspectCheckStatus::ToolchainMissing || results[0].status == HirAspectCheckStatus::Error,
        "expected ToolchainMissing or Error, got {:?}",
        results[0].status
    );
}

// ---------------------------------------------------------------------------
// APA-L2 pins: additive mask-neutrality source is real proof content, and the
// existing trust gates remain fail-closed for its negative fixtures.
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_mask_neutrality_generates_real_float_free_obligation() {
    let record = HirAspectRecord::recognized(
        HirAspectKind::MaskNeutrality,
        HirAspectSpan::Source(Span::new(0, 10)),
        Some(crate::hir::HirId(7)),
    )
    .with_detail(HirAspectDetail::MaskNeutrality { family: MaskNeutralityFamily::Reduce });

    let source = super::generate_lean_source(&[&record]);

    assert!(source.contains("foldNeutralSuffix"));
    assert!(source.contains("additiveReduceMaskNeutrality"));
    assert!(source.contains("zeroFilledSuffix"));
    assert!(source.contains("live : List Nat"));
    assert!(source.contains("dead : Nat"));
    assert!(!source.contains(": True := trivial"));
    assert!(!source.contains("f32"));
    assert!(!source.contains("f64"));
    assert_eq!(LeanAspectChecker::scan_for_sorry(&source), SorryVerdict::Clean);
}

#[test]
fn lean_checker_mask_neutrality_seeded_sorry_is_rejected() {
    let source = concat!(
        "theorem mask_neutrality_seeded_sorry (live : List Nat) :\n",
        "  live = live := by\n",
        "  sorry\n",
    );
    assert_eq!(
        LeanAspectChecker::scan_for_sorry(source),
        SorryVerdict::Found { line_numbers: vec![3] }
    );
    let result = super::combined_verdict(
        HirAspectCheckResult {
            status: HirAspectCheckStatus::Proven,
            evidence: HirAspectEvidence("lake build succeeded".into()),
            counterexample: Vec::new(),
            backend: HirAspectBackendKind::Lean,
        },
        &AxiomVerdict::Allowed,
        &SorryVerdict::Found { line_numbers: vec![3] },
    );
    assert_eq!(result.status, HirAspectCheckStatus::Failed);
    assert!(result.evidence.0.contains("sorry"));
}

#[test]
fn lean_checker_mask_neutrality_seeded_non_allowlisted_axiom_is_rejected() {
    let audit = AxiomAudit {
        dependencies: vec![AxiomDependency {
            axiom_name: "maskNeutralityExternalAssumption".into(),
            declaration: "mask_neutrality_seeded_axiom".into(),
        }],
        unresolved_import: false,
    };
    let verdict = LeanAspectChecker::classify_axioms(&audit);
    match verdict {
        AxiomVerdict::Rejected { disallowed } => {
            assert_eq!(disallowed.len(), 1);
            assert_eq!(disallowed[0].axiom_name, "maskNeutralityExternalAssumption");
        }
        AxiomVerdict::Allowed | AxiomVerdict::UnresolvedImport => {
            panic!("seeded non-allowlisted axiom must be rejected")
        }
    }
}

#[test]
fn lean_checker_mask_neutrality_missing_toolchain_is_not_proven() {
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa-l2-missing-tc");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);
    let record = HirAspectRecord::recognized(
        HirAspectKind::MaskNeutrality,
        HirAspectSpan::Source(Span::new(0, 10)),
        Some(crate::hir::HirId(7)),
    )
    .with_detail(HirAspectDetail::MaskNeutrality { family: MaskNeutralityFamily::Scan });
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 1);
    assert_ne!(results[0].status, HirAspectCheckStatus::Proven);
    assert!(
        results[0].status == HirAspectCheckStatus::ToolchainMissing || results[0].status == HirAspectCheckStatus::Error,
        "expected missing-toolchain/error result, got {:?}",
        results[0].status
    );
}

// ---------------------------------------------------------------------------
// APA-2 pins: registered AIR kinds carry deterministic labels and cannot fall
// through the generic `True := trivial` generator as if proven.
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_theorem_labels_do_not_collide_across_registered_kinds() {
    let span = HirAspectSpan::Source(Span::new(0, 10));

    let kinds = [
        HirAspectKind::DiscerneOmnia,
        HirAspectKind::UnrollEquivalence,
        HirAspectKind::MaskNeutrality,
        HirAspectKind::GatePurity,
    ];

    // Same node id and index: the label must still separate the kinds.
    let mut labels: Vec<String> = kinds
        .iter()
        .map(|&kind| {
            let record = HirAspectRecord::recognized(kind, span, Some(crate::hir::HirId(7)));
            super::theorem_label(&record, 0)
        })
        .collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), kinds.len(), "labels must be pairwise distinct: {labels:?}");

    // Node-less records fall back to the record index; kind still separates.
    let mut labels: Vec<String> = kinds
        .iter()
        .map(|&kind| {
            let record = HirAspectRecord::recognized(kind, span, None);
            super::theorem_label(&record, 3)
        })
        .collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), kinds.len(), "index labels must be pairwise distinct: {labels:?}");
}

#[test]
fn lean_checker_air_kind_records_never_become_proven_via_placeholder_generator() {
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa2-no-fallthrough");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let span = HirAspectSpan::Source(Span::new(0, 10));
    // APA-L1 landed a real UnrollEquivalence generator, so only the kinds
    // whose owning lines have not landed may be refused here.
    let air_kinds = [HirAspectKind::GatePurity];
    let records: Vec<HirAspectRecord> = air_kinds
        .iter()
        .map(|&kind| HirAspectRecord::recognized(kind, span, Some(crate::hir::HirId(1))))
        .collect();
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, records);

    let results = checker.verify(&report);
    assert_eq!(results.len(), air_kinds.len());
    for result in &results {
        assert_ne!(
            result.status,
            HirAspectCheckStatus::Proven,
            "AIR kind must not be Proven pre-line-proof"
        );
        assert_eq!(
            result.status,
            HirAspectCheckStatus::Failed,
            "expected refused result, got {:?}",
            result.status
        );
        assert_eq!(result.backend, HirAspectBackendKind::Lean);
        assert!(
            result
                .evidence
                .0
                .contains("registered without proof content")
        );
        assert!(result.evidence.0.contains("refusing placeholder proof"));
    }
}

#[test]
fn lean_checker_air_refusal_preserves_per_record_order_alongside_checkable_kinds() {
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa2-order");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let span = HirAspectSpan::Source(Span::new(0, 10));
    let records = vec![
        HirAspectRecord::recognized(HirAspectKind::GatePurity, span, Some(crate::hir::HirId(1))),
        HirAspectRecord::recognized(HirAspectKind::DiscerneOmnia, span, Some(crate::hir::HirId(2))),
    ];
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, records);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 2);
    // The report sorts same-span records by kind, so the deterministic order
    // is DiscerneOmnia, GatePurity. DiscerneOmnia with a missing toolchain is
    // ToolchainMissing/Error, never Proven; the unlanded AIR kind keeps its
    // deterministic refusal, in order.
    assert!(
        results[0].status == HirAspectCheckStatus::ToolchainMissing || results[0].status == HirAspectCheckStatus::Error
    );
    assert_eq!(results[1].status, HirAspectCheckStatus::Failed);
    assert!(results[1].evidence.0.contains("gate_purity"));
}

// ---------------------------------------------------------------------------
// APA-L1 pins: the unroll-equivalence obligation is real proof content, and
// the trust gates stay fail-closed without a toolchain.
// ---------------------------------------------------------------------------

#[test]
fn lean_checker_unroll_equivalence_generates_real_structural_obligation() {
    let record = HirAspectRecord::recognized(
        HirAspectKind::UnrollEquivalence,
        HirAspectSpan::Source(Span::new(0, 10)),
        Some(crate::hir::HirId(9)),
    )
    .with_detail(HirAspectDetail::UnrollEquivalence { route: crate::aspect::UnrollRoute::Ab, lanes: vec![0, 1, 2] });

    let source = super::generate_lean_source(&[&record]);

    assert!(source.contains("unrolledLanesMatchSource"));
    assert!(source.contains("unrollEvaluationPreservesSourceOrder"));
    assert!(source.contains("unrollFailureBehaviorPreserved"));
    assert!(source.contains("unrolledStraightLine [0, 1, 2]"));
    assert!(source.contains("aspect_unroll_equivalence_9"));
    assert!(!source.contains(": True := trivial"));
    assert!(!source.contains("f32"));
    assert!(!source.contains("f64"));
    assert_eq!(LeanAspectChecker::scan_for_sorry(&source), SorryVerdict::Clean);
}

#[test]
fn lean_checker_unroll_equivalence_missing_toolchain_is_not_proven() {
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa-l1-missing-tc");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);
    let record = HirAspectRecord::recognized(
        HirAspectKind::UnrollEquivalence,
        HirAspectSpan::Source(Span::new(0, 10)),
        Some(crate::hir::HirId(9)),
    )
    .with_detail(HirAspectDetail::UnrollEquivalence { route: crate::aspect::UnrollRoute::Ab, lanes: vec![0, 1, 2] });
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 1);
    assert_ne!(results[0].status, HirAspectCheckStatus::Proven);
    assert!(
        results[0].status == HirAspectCheckStatus::ToolchainMissing || results[0].status == HirAspectCheckStatus::Error,
        "expected missing-toolchain/error result, got {:?}",
        results[0].status
    );
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lean_checker_unroll_equivalence_full_verify_real_theorem() {
    // The real Line-1 harness check: lake build + sorry scan + transitive
    // allowlisted axiom audit over the generated unroll-equivalence theorem.
    let config = LeanAspectCheckerConfig::default();
    let temp = std::env::temp_dir();
    let checker = LeanAspectChecker::new(config, temp);
    let record = HirAspectRecord::recognized(
        HirAspectKind::UnrollEquivalence,
        HirAspectSpan::Source(Span::new(0, 10)),
        Some(crate::hir::HirId(9)),
    )
    .with_detail(HirAspectDetail::UnrollEquivalence { route: crate::aspect::UnrollRoute::Ab, lanes: vec![0, 1, 2] });
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].status,
        HirAspectCheckStatus::Proven,
        "expected Proven through real lake build + sorry scan + axiom audit, got: {}",
        results[0].evidence.0
    );
    assert_eq!(results[0].backend, HirAspectBackendKind::Lean);
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lean_checker_unroll_equivalence_axiom_audit_is_live_not_vacuous() {
    // Negative integration pin: a module whose theorem secretly uses `sorry`
    // must surface `sorryAx` through the transitive audit — proving the
    // wrapper import actually resolves against the built module instead of
    // failing open with an empty audit.
    let temp = std::env::temp_dir().join(format!("radix-lean-audit-live-{}", std::process::id()));
    std::fs::create_dir_all(&temp).expect("create audit probe dir");
    let module = temp.join("FaberGenerated.lean");
    std::fs::write(&module, "theorem seeded_sorry_probe : True := by sorry\n").expect("write audit probe module");
    std::fs::write(temp.join("lakefile.toml"), super::generate_lakefile_toml()).expect("write audit probe lakefile");

    let checker = LeanAspectChecker::new(LeanAspectCheckerConfig::default(), temp.clone());
    let lake = checker.run_lake_build(&temp);
    let build_result = normalize_result(lake);
    assert_eq!(
        build_result.status,
        HirAspectCheckStatus::Proven,
        "sorry type-checks; build must pass"
    );

    let audit = checker.audit_axioms(&module);
    let verdict = LeanAspectChecker::classify_axioms(&audit);
    match verdict {
        AxiomVerdict::Rejected { disallowed } => {
            assert!(
                disallowed.iter().any(|dep| dep.axiom_name == "sorryAx"),
                "expected sorryAx in the transitive audit, got: {disallowed:?}"
            );
        }
        AxiomVerdict::Allowed | AxiomVerdict::UnresolvedImport => {
            panic!("transitive axiom audit failed open; it is vacuous")
        }
    }

    let _ = std::fs::remove_dir_all(&temp);
}

// ---------------------------------------------------------------------------
// APA-L3 pins: gate/purity obligations are real proof content, gate-hole
// findings are named and never downgraded, and the trust gates stay
// fail-closed without a toolchain.
// ---------------------------------------------------------------------------

fn admitted_gate_purity_record() -> HirAspectRecord {
    HirAspectRecord::recognized(
        HirAspectKind::GatePurity,
        HirAspectSpan::Source(Span::new(0, 10)),
        Some(crate::hir::HirId(5)),
    )
    .with_detail(HirAspectDetail::GatePurity {
        term: crate::aspect::PurityTerm::Let(
            Box::new(crate::aspect::PurityTerm::Scalar(3)),
            Box::new(crate::aspect::PurityTerm::Call(
                Box::new(crate::aspect::PurityTerm::Scalar(0)),
                vec![
                    crate::aspect::PurityTerm::Scalar(0),
                    crate::aspect::PurityTerm::Scalar(2),
                ],
            )),
        ),
    })
}

#[test]
fn lean_checker_gate_purity_generates_real_structural_obligation() {
    let record = admitted_gate_purity_record();

    let source = super::generate_lean_source(&[&record]);

    assert!(source.contains("gatePurityCorrespondence"));
    assert!(source.contains("gateAdmittedInhabitsPureFragment"));
    assert!(source.contains("gateAdmittedCarriesNoEffects"));
    assert!(source.contains("Term.letIn (Term.scalar 3)"));
    assert!(source.contains("GateAdmits.letIn (GateAdmits.scalar 3)"));
    assert!(source.contains("| io : Effect"));
    assert!(source.contains("| monadic : Effect"));
    assert!(source.contains("ioOpIsNeverAdmitted"));
    assert!(source.contains("monadicBindIsNeverAdmitted"));
    assert!(!source.contains(": True := trivial"));
    assert!(!source.contains("f32"));
    assert!(!source.contains("f64"));
    assert_eq!(LeanAspectChecker::scan_for_sorry(&source), SorryVerdict::Clean);
}

#[test]
fn lean_checker_gate_hole_record_yields_named_finding_never_downgraded() {
    // A gate-hole record is a named finding. It short-circuits before any
    // toolchain consult: with no Lean toolchain at all the result still
    // names the hole and never reads as a mere failed (ignorable) proof,
    // a refusal, or a fabricated Proven.
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa-l3-hole");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let record = HirAspectRecord::recognized(
        HirAspectKind::GatePurity,
        HirAspectSpan::Source(Span::new(0, 10)),
        Some(crate::hir::HirId(6)),
    )
    .with_detail(HirAspectDetail::GatePurityHole { issue: crate::aspect::GateHoleIssue::AwaitingLocalInit });

    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, HirAspectCheckStatus::Failed);
    assert_eq!(results[0].backend, HirAspectBackendKind::Lean);
    assert!(
        results[0]
            .evidence
            .0
            .contains("gate hole (never downgraded)")
    );
    assert!(
        results[0]
            .evidence
            .0
            .contains(crate::aspect::GateHoleIssue::AwaitingLocalInit.label())
    );
    assert!(
        !results[0]
            .evidence
            .0
            .contains("registered without proof content")
    );
}

#[test]
fn lean_checker_gate_purity_missing_toolchain_is_not_proven() {
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa-l3-missing-tc");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);
    let record = admitted_gate_purity_record();
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 1);
    assert_ne!(results[0].status, HirAspectCheckStatus::Proven);
    assert!(
        results[0].status == HirAspectCheckStatus::ToolchainMissing || results[0].status == HirAspectCheckStatus::Error,
        "expected missing-toolchain/error result, got {:?}",
        results[0].status
    );
}

#[test]
fn lean_checker_gate_purity_hole_keeps_named_finding_alongside_checkable_records() {
    // Report order: hole + admitted records interleave without the hole
    // being swallowed by the shared lake result.
    let temp_dir = crate::test_support::test_temp_dir("lean-verifier-apa-l3-order");
    let temp = temp_dir.path().to_path_buf();
    let config = LeanAspectCheckerConfig {
        lake_bin: temp.join("nonexistent-lake"),
        lean_bin: temp.join("nonexistent-lean"),
        timeout: Duration::from_secs(5),
    };
    let checker = LeanAspectChecker::new(config, temp);

    let span = HirAspectSpan::Source(Span::new(0, 10));
    let records = vec![
        HirAspectRecord::recognized(HirAspectKind::GatePurity, span, Some(crate::hir::HirId(6))).with_detail(
            HirAspectDetail::GatePurityHole { issue: crate::aspect::GateHoleIssue::UntranslatableErrorNode },
        ),
        admitted_gate_purity_record(),
    ];
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, records);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 2);
    // The report sorts same-span records by kind then node id; the admitted
    // record (node 5) sorts before the hole (node 6).
    assert!(
        results[0].status == HirAspectCheckStatus::ToolchainMissing || results[0].status == HirAspectCheckStatus::Error
    );
    assert_eq!(results[1].status, HirAspectCheckStatus::Failed);
    assert!(
        results[1]
            .evidence
            .0
            .contains("gate hole (never downgraded)")
    );
    assert!(
        results[1]
            .evidence
            .0
            .contains(crate::aspect::GateHoleIssue::UntranslatableErrorNode.label())
    );
}

#[test]
#[ignore = "tracking: bulk-test-cleanup — requires Lean toolchain CI matrix step"]
fn lean_checker_gate_purity_full_verify_real_theorem() {
    // The real Line-3 harness check: lake build + sorry scan + transitive
    // allowlisted axiom audit over the generated gate-purity theorem.
    let config = LeanAspectCheckerConfig::default();
    let temp = std::env::temp_dir();
    let checker = LeanAspectChecker::new(config, temp);
    let record = admitted_gate_purity_record();
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![record]);

    let results = checker.verify(&report);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].status,
        HirAspectCheckStatus::Proven,
        "expected Proven through real lake build + sorry scan + axiom audit, got: {}",
        results[0].evidence.0
    );
    assert_eq!(results[0].backend, HirAspectBackendKind::Lean);
}
