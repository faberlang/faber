//! Behavioural coverage for driver entrypoints and fail-closed target routes.

use super::{Config, ModuleCompileRequest, Session, WarnPolicy, compile, compile_analyzed};
use crate::codegen::Target;
use crate::diagnostics::{Diagnostic, Severity};

#[test]
fn compile_refuses_routes_owned_by_package_or_artifact_workflows() {
    let refused_targets = [
        (Target::MirStepper, "stepper_not_codegen_target"),
        (Target::MirScena, "package_artifact_route_required"),
        (Target::MirFmir, "package_artifact_route_required"),
        (Target::MirFmirBinary, "package_artifact_route_required"),
        (Target::HirFhir, "fhir_serialization_dispatch_only"),
    ];

    for (target, issue) in refused_targets {
        let session = Session::new(Config::default().with_latin_locale().with_target(target));
        let result = compile(
            &session,
            "route.fab",
            "functio septem() → numerus { redde 7 }",
            ModuleCompileRequest::default(),
        );

        assert!(
            result.output.is_none(),
            "{target} must not emit through the ordinary compile route"
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == Some("CODEGEN001") && diagnostic.issue() == Some(issue)),
            "{target} must report its route refusal ({issue}): {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.issue()))
                .collect::<Vec<_>>(),
        );
    }
}

#[test]
fn compile_analyzed_promotes_denied_warning_before_codegen() {
    let analysis_session = Session::new(Config::default().with_latin_locale());
    let source = "functio septem() → numerus { redde 7 }";
    let mut analyzed = super::analyze_source(&analysis_session, "denied.fab", source, super::AnalyzeRequest::default())
        .expect("source analysis");
    analyzed
        .diagnostics
        .push(Diagnostic::warning("unused binding").with_code("WARN001"));

    let compile_session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_warn_policy(WarnPolicy { deny_all_warnings: false, deny_codes: vec!["WARN001".to_owned()] }),
    );
    let result = compile_analyzed(&compile_session, "denied.fab", source, analyzed);

    assert!(result.output.is_none(), "a promoted analysis warning must stop code generation");
    let denied = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == Some("WARN001"))
        .expect("the denied warning remains in the result");
    assert_eq!(denied.severity, Severity::Error);
}

#[cfg(feature = "mir-llvm")]
#[test]
fn cuda_descriptor_rejects_non_llvm_target_before_analysis() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirRust),
    );
    let diagnostics = super::cuda_kernel_descriptor_json(&session, "descriptor.fab", "not parsed").unwrap_err();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, Some("CUDA_DESCRIPTOR_TARGET"));
}

#[test]
fn diagnostic_line_adjustment_uses_only_complete_utf8_prefixes() {
    let mut diagnostic = Diagnostic::error("failure");
    diagnostic.line = Some(2);
    super::adjust_diagnostic_lines_for_body(std::slice::from_mut(&mut diagnostic), "+++\nlocale = 'la'\n+++\nbody", 22);
    assert_eq!(diagnostic.line, Some(5));

    // An offset into a multibyte code point is not a valid source boundary;
    // the diagnostic remains unchanged rather than being assigned a false line.
    let mut invalid_boundary = Diagnostic::error("failure");
    invalid_boundary.line = Some(2);
    super::adjust_diagnostic_lines_for_body(std::slice::from_mut(&mut invalid_boundary), "é\nbody", 1);
    assert_eq!(invalid_boundary.line, Some(2));
}
