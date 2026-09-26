//! Pass-2 checking of the `¶` format operator (DECISIONS D2.1–D2.5).
//!
//! Pass 1 guaranteed the spec is a string literal. Here the spec text is
//! parsed with the shared `radix_types::format_spec` parser and validated
//! against the left side's type. Every mismatch is a named compile error, so
//! `¶` itself can never fail at run time (D2.4). The result is `textus`.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_lexer::Span;
use radix_types::format_spec::{
    FormatSpecError, FormatSubject, format_subject, parse_format_spec, validate_format_spec,
};

const ISSUE_FORMAT_SPEC_MALFORMED: &str = "format_spec_malformed";
const ISSUE_FORMAT_SPEC_PART_INVALID: &str = "format_spec_part_invalid";
const ISSUE_FORMAT_SPEC_PRESET_REQUIRED: &str = "format_spec_preset_required";
const ISSUE_FORMAT_SPEC_PRESET_NOT_ALLOWED: &str = "format_spec_preset_not_allowed";
const ISSUE_FORMAT_TYPE_UNSUPPORTED: &str = "format_type_unsupported";
const ISSUE_FORMAT_DECIMAL_UNSUPPORTED: &str = "format_decimal_unsupported";

/// Check `value ¶ "spec"` and produce `textus`.
pub(super) fn check_format_expr(
    checker: &mut TypeChecker<'_>,
    value: &mut HirExpression,
    spec: Symbol,
    span: Span,
) -> TypeId {
    let value_ty = checker.check_expr(value);
    let textus = checker.textus_type();
    let resolved = checker.resolve_inferred_type(value_ty);
    if matches!(checker.types.get(resolved), Type::Error) {
        return textus;
    }
    let subject = format_subject(checker.types, resolved);
    match subject {
        FormatSubject::Decimal => {
            // Fork F4: decimal display policy is held; do not guess one.
            checker.invalid_operand_types(ISSUE_FORMAT_DECIMAL_UNSUPPORTED, span);
            return textus;
        }
        FormatSubject::Unsupported => {
            checker.invalid_operand_types(ISSUE_FORMAT_TYPE_UNSUPPORTED, span);
            return textus;
        }
        _ => {}
    }
    let Some(text) = checker
        .interner
        .map(|interner| interner.resolve(spec).to_owned())
    else {
        return textus;
    };
    let result = parse_format_spec(&text).and_then(|parsed| validate_format_spec(&parsed, subject));
    match result {
        Ok(()) => {}
        Err(FormatSpecError::Malformed) => {
            checker.invalid_operand_types(ISSUE_FORMAT_SPEC_MALFORMED, span);
        }
        Err(FormatSpecError::PartNotAllowed(part)) => checker.invalid_operand_types_with_args(
            ISSUE_FORMAT_SPEC_PART_INVALID,
            span,
            &[("part", part.as_str().to_owned())],
        ),
        Err(FormatSpecError::PresetRequired) => {
            checker.invalid_operand_types(ISSUE_FORMAT_SPEC_PRESET_REQUIRED, span);
        }
        Err(FormatSpecError::PresetNotAllowed) => {
            checker.invalid_operand_types(ISSUE_FORMAT_SPEC_PRESET_NOT_ALLOWED, span);
        }
    }
    textus
}
