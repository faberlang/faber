//! Package-MIR diagnostic constructors.

use super::{Diagnostic, DiagnosticPhase, Path, RunnerError};

pub fn mir_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message).with_file(path.display().to_string())
}

pub(super) fn mir_lowering_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    mir_diag(path, message).with_phase(DiagnosticPhase::Mir)
}

pub(super) fn mir_issue_diag(
    path: &Path,
    issue: &'static str,
    message: impl Into<String>,
) -> Diagnostic {
    mir_diag(path, message).with_arg("issue", issue)
}

pub(super) fn runner_diagnostics(path: &Path, errors: Vec<RunnerError>) -> Vec<Diagnostic> {
    errors
        .into_iter()
        .map(|error| mir_diag(path, error.message))
        .collect()
}
