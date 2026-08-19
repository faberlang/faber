//! Validation diagnostics and callable-signature metadata.

use crate::MirType;
use radix_lexer::Span;

/// Validation diagnostic for malformed MIR.
///
/// Spans are carried from the offending MIR node so callers can tie structural
/// failures back to the source region that produced the bad representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirValidationError {
    pub message: String,
    pub issue: String,
    pub span: Span,
}

impl MirValidationError {
    pub(crate) fn new(span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        let issue = validation_issue(&message);
        Self {
            message,
            issue,
            span,
        }
    }

    pub(crate) fn with_issue(
        span: Span,
        issue: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            message: message.into(),
            issue: issue.into(),
            span,
        }
    }
}

fn validation_issue(message: &str) -> String {
    let mut issue = String::from("mir_validation");
    let mut separated = false;
    for ch in message.chars() {
        if ch.is_ascii_alphanumeric() {
            if !separated {
                issue.push('_');
            }
            issue.push(ch.to_ascii_lowercase());
            separated = true;
        } else if !issue.ends_with('_') {
            issue.push('_');
            separated = true;
        }
    }
    issue.trim_end_matches('_').to_string()
}

/// One callable parameter slot in a MIR validation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirParamSignature {
    pub ty: MirType,
    pub optional: bool,
}

/// Known callable signature used by MIR validation.
///
/// Direct MIR functions can derive this from their parameter locals and return
/// type. External or not-yet-lowered callees are supplied through
/// `MirValidationContext::functions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFunctionSignature {
    pub params: Vec<MirParamSignature>,
    pub return_ty: MirType,
    pub error_ty: Option<MirType>,
}
