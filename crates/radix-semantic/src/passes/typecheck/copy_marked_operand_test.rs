//! A glyph tensor operand that carries the `copy` access marker
//! (`varia tensor<f32, [2, 2]> acc ← copy w`) must reach the glyph
//! operator checks value-transparently, like any other copy-marked
//! carrier: `glyph_operand_types` peels the marker before dispatch, so
//! `acc · w` typechecks instead of failing closed SEM011
//! `glyph_operands_unsupported`.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::SemanticErrorKind;
use crate::test_support;

/// No diagnostic may be the glyph operand rejection itself: kind
/// `OperandTypesInvalid` (SEM011) or any `glyph_*` issue.
fn assert_no_glyph_operand_rejection(source: &str) {
    let result = test_support::analyze_source(source);
    let rejected = result
        .diagnostics
        .iter()
        .find(|err| {
            err.is_error()
                && (matches!(err.kind, SemanticErrorKind::OperandTypesInvalid)
                    || test_support::issue(err).is_some_and(|issue| issue.starts_with("glyph_")))
        })
        .map(|err| (err.kind, test_support::issue(err)));
    assert!(
        rejected.is_none(),
        "expected no glyph operand rejection, got {rejected:?} in {:?}",
        result.diagnostics
    );
}

#[test]
fn glyph_matmul_accepts_copy_marked_tensor_operand() {
    assert_no_glyph_operand_rejection(
        "functio product(tensor<f32, [2, 2]> w) → tensor<f32, [2, 2]> {\n    varia tensor<f32, [2, 2]> acc ← copy w\n    sit tensor<f32, [2, 2]> prod ← acc · w\n    nota prod\n    redde acc\n}",
    );
}

/// Marker-free control: the same square tensor matmul shape is valid on this
/// harness when no access marker is involved, so the rejection above is about
/// the marker, not the shape.
#[test]
fn glyph_square_tensor_matmul_without_marker_is_accepted() {
    test_support::assert_accepts(
        "functio product(tensor<f32, [2, 2]> w) → tensor<f32, [2, 2]> {\n    redde w · w\n}",
    );
}
