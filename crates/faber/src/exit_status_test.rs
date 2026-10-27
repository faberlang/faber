use super::*;
use radix::module::diagnostics::Severity;

fn error(phase: DiagnosticPhase) -> Diagnostic {
    Diagnostic::error("x").with_phase(phase)
}

#[test]
fn compiler_phase_errors_are_data_errors() {
    for phase in [
        DiagnosticPhase::Lex,
        DiagnosticPhase::Parse,
        DiagnosticPhase::Resolve,
        DiagnosticPhase::Lower,
        DiagnosticPhase::Typecheck,
        DiagnosticPhase::Analysis,
        DiagnosticPhase::Mir,
        DiagnosticPhase::Codegen,
    ] {
        assert_eq!(status_for_diagnostic(&error(phase)), DATA_ERROR, "{phase}");
    }
}

#[test]
fn io_and_tool_errors_have_no_class() {
    assert_eq!(status_for_diagnostic(&error(DiagnosticPhase::Io)), FAILURE);
    assert_eq!(
        status_for_diagnostic(&error(DiagnosticPhase::Tool)),
        FAILURE
    );
}

#[test]
fn a_tagged_diagnostic_takes_its_class() {
    assert_eq!(status_for_diagnostic(&config_diagnostic_error("x")), CONFIG);
    assert_eq!(status_for_diagnostic(&usage_diagnostic_error("x")), USAGE);
}

#[test]
fn a_diagnostic_naming_a_manifest_file_is_config() {
    let diag = error(DiagnosticPhase::Tool).with_file("/pkg/faber.toml");
    assert_eq!(status_for_diagnostic(&diag), CONFIG);
    let other = error(DiagnosticPhase::Tool).with_file("/pkg/main.fab");
    assert_eq!(status_for_diagnostic(&other), FAILURE);
}

#[test]
fn run_diagnostics_leave_back_end_phases_to_the_program() {
    for phase in [DiagnosticPhase::Mir, DiagnosticPhase::Codegen] {
        assert_eq!(status_for_run_diagnostic(&error(phase)), FAILURE, "{phase}");
    }
    assert_eq!(
        status_for_run_diagnostic(&error(DiagnosticPhase::Analysis)),
        DATA_ERROR
    );
    assert_eq!(
        status_for_run_diagnostic(&config_diagnostic_error("x")),
        CONFIG
    );
}

#[test]
fn warnings_do_not_classify_a_failure() {
    let warning = error(DiagnosticPhase::Analysis).with_severity(Severity::Warning);
    assert_eq!(status_for_diagnostics(&[warning]), FAILURE);
}

#[test]
fn a_set_takes_usage_then_config_then_data() {
    let data = error(DiagnosticPhase::Analysis);
    let config = config_diagnostic_error("x");
    let usage = usage_diagnostic_error("x");
    assert_eq!(status_for_diagnostics(&[data.clone()]), DATA_ERROR);
    assert_eq!(
        status_for_diagnostics(&[data.clone(), config.clone()]),
        CONFIG
    );
    assert_eq!(status_for_diagnostics(&[data, config, usage]), USAGE);
}

#[test]
fn a_refused_compile_defaults_to_data_error() {
    assert_eq!(status_for_compile_failure(&[]), DATA_ERROR);
    assert_eq!(
        status_for_compile_failure(&[error(DiagnosticPhase::Tool)]),
        DATA_ERROR
    );
    assert_eq!(
        status_for_compile_failure(&[config_diagnostic_error("x")]),
        CONFIG
    );
}
