//! Package-MIR diagnostic constructors.

use super::{Diagnostic, DiagnosticPhase, Path, RunnerError};
use radix_mir_lower::lower::MirError;
use radix_module::lexer::Span;

pub fn mir_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message).with_file(path.display().to_string())
}

pub(super) fn mir_lowering_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    mir_diag(path, message).with_phase(DiagnosticPhase::Mir)
}

/// Package-lowering diagnostic for one MIR lowering error: the unit file, the
/// error span with its physical line (when the file is readable), the named
/// issue, and the secondary definition span (byte offsets in the defining
/// source) for call-site errors that name their callee.
pub(super) fn mir_error_diag(path: &Path, error: MirError) -> Diagnostic {
    let mut diagnostic = mir_lowering_diag(path, error.message);
    if !error.issue.is_empty() {
        diagnostic = diagnostic.with_arg("issue", error.issue);
    }
    if let Some(definition) = error.definition_span {
        diagnostic = diagnostic
            .with_arg("definition_start", definition.start.to_string())
            .with_arg("definition_end", definition.end.to_string());
    }
    if error.span == Span::default() {
        return diagnostic;
    }
    match std::fs::read_to_string(path) {
        Ok(source) => {
            diagnostic.with_source_context(&path.display().to_string(), &source, error.span)
        }
        Err(_) => diagnostic.with_span(error.span),
    }
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
