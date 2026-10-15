//! Structured fail-closed diagnostics for the C leaf.

use radix_codegen_shared::CodegenError;
use radix_diagnostics::DiagnosticArg;

/// Issue slug carried by every rejection from this leaf.
pub(crate) const UNSUPPORTED_ISSUE: &str = "c_target_unsupported";

/// Build the named "unsupported on target c" rejection for `construct`.
///
/// Anything outside the subset fails with a name the author can act on, never
/// with wrong or partial C. For this target "wrong" includes undefined
/// behavior: a construct whose C form could overflow, over-shift or alias is
/// rejected until its exact form lands.
#[must_use]
pub(crate) fn unsupported(construct: &str) -> CodegenError {
    CodegenError {
        message: format!("unsupported on target c: {construct}"),
        args: vec![
            DiagnosticArg::new("issue", UNSUPPORTED_ISSUE),
            DiagnosticArg::new("construct", construct),
            DiagnosticArg::new("target", "c"),
        ],
    }
}
