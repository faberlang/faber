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
fn class_values_are_the_sysexits_ones() {
    assert_eq!(
        [
            FAILURE,
            USAGE,
            DATA_ERROR,
            NO_INPUT,
            UNAVAILABLE,
            SOFTWARE,
            CANT_CREATE,
            CONFIG
        ],
        [1, 64, 65, 66, 69, 70, 73, 78]
    );
}

#[test]
fn read_failures_are_no_input_and_tool_errors_have_no_class() {
    assert_eq!(status_for_diagnostic(&error(DiagnosticPhase::Io)), NO_INPUT);
    assert_eq!(
        status_for_diagnostic(&error(DiagnosticPhase::Tool)),
        FAILURE
    );
}

#[test]
fn a_write_failure_is_cant_create_even_in_the_io_phase() {
    let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let diag = write_diagnostic_error(std::path::Path::new("/out/a.rs"), &err);
    assert_eq!(diag.phase, DiagnosticPhase::Io);
    assert_eq!(
        diag.args
            .iter()
            .find(|arg| arg.name == "path")
            .map(|arg| arg.value.as_str()),
        Some("/out/a.rs")
    );
    assert_eq!(status_for_diagnostic(&diag), CANT_CREATE);
    let read = Diagnostic::io_error(std::path::Path::new("/in/a.fab"), &err);
    assert_eq!(status_for_diagnostic(&read), NO_INPUT);
}

#[test]
fn tagged_classes_map_to_their_statuses() {
    for (class, status) in [
        (CLASS_UNAVAILABLE, UNAVAILABLE),
        (CLASS_SOFTWARE, SOFTWARE),
        (CLASS_CANT_CREATE, CANT_CREATE),
    ] {
        let diag = error(DiagnosticPhase::Tool).with_arg(CLASS_ARG, class);
        assert_eq!(status_for_diagnostic(&diag), status, "{class}");
    }
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

#[test]
fn a_diagnostic_naming_a_missing_target_is_unavailable_in_any_phase() {
    for issue in [
        "target_unavailable",
        "package_target_unavailable",
        "target_not_in_this_build",
    ] {
        for phase in [DiagnosticPhase::Codegen, DiagnosticPhase::Tool] {
            let diag = error(phase).with_arg("issue", issue);
            assert_eq!(status_for_diagnostic(&diag), UNAVAILABLE, "{issue} {phase}");
        }
    }
}

#[test]
fn a_set_with_a_read_failure_and_source_errors_keeps_the_source_class() {
    let read = error(DiagnosticPhase::Io);
    let data = error(DiagnosticPhase::Analysis);
    assert_eq!(status_for_diagnostics(&[read.clone()]), NO_INPUT);
    assert_eq!(status_for_diagnostics(&[read, data]), DATA_ERROR);
}
