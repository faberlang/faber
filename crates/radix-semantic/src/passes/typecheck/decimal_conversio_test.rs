//! decimal-widths D3: the semantic conversio/equality arms for
//! `numerus<d64>` / `numerus<d64>`.
//!
//! Matrix: within-width `≡` is exact; cross-family `≡` rejects (no
//! decimal-wins promotion). The `↦` rows are decimal↔integer
//! (round-half-even), fractus→decimal (string round-trip), decimal→fractus
//! (defined rounding), and textus→decimal (text-parse row, settled at D3 —
//! consistent with plain `textus ↦ numerus`). `d64` is the only decimal width
//! (`d32` was removed, D11.7), so there is no decimal width conversion.

use crate::test_support;

// --- equality lattice ---

#[test]
fn within_width_d64_equality_accepts() {
    test_support::assert_accepts("functio same(d64 a) → bivalens { redde a ≡ a }");
}

#[test]
fn cross_family_decimal_integer_equality_rejects() {
    // No decimal-wins promotion: a decimal value never compares bare with an
    // integer value, even though both are numerus-family widths.
    test_support::assert_rejects_with_issue(
        "functio same(d64 a, numerus n) → bivalens { redde a ≡ n }",
        "equality_exact_types_required",
    );
}

#[test]
fn cross_family_decimal_fractus_equality_rejects() {
    test_support::assert_rejects_with_issue(
        "functio same(d64 a) → bivalens { redde a ≡ 4.2 }",
        "equality_exact_types_required",
    );
}

// --- conversio rows: decimal ↔ integer (round-half-even) ---

#[test]
fn decimal_to_integer_arrow_accepts() {
    test_support::assert_accepts("functio c(d64 a) → i64 { redde a ↦ i64 via HalfEven }");
}

#[test]
fn decimal_to_unsigned_integer_arrow_accepts() {
    test_support::assert_accepts("functio c(d64 a) → u32 ⇥ textus { redde a ↦ u32 via HalfEven }");
}

#[test]
fn integer_to_decimal_arrow_accepts() {
    test_support::assert_accepts("functio c(i64 a) → d64 { redde a ↦ d64 }");
}

#[test]
fn unsigned_integer_to_decimal_arrow_accepts() {
    test_support::assert_accepts("functio c(u32 a) → d64 { redde a ↦ d64 }");
}

// --- conversio rows: fractus ↔ decimal (string round-trip / defined rounding) ---

#[test]
fn fractus_to_decimal_arrow_accepts() {
    test_support::assert_accepts("functio c(fractus x) → d64 { redde x ↦ d64 }");
}

#[test]
fn sized_fractus_to_decimal_arrow_accepts() {
    test_support::assert_accepts("functio c(f32 x) → d64 { redde x ↦ d64 }");
}

#[test]
fn decimal_to_fractus_arrow_accepts() {
    test_support::assert_accepts("functio c(d64 a) → fractus { redde a ↦ fractus }");
}

#[test]
fn decimal_to_sized_fractus_arrow_accepts() {
    test_support::assert_accepts("functio c(d64 a) → f32 { redde a ↦ f32 }");
}

#[test]
fn integer_narrowing_arrow_still_accepts() {
    // The decimal narrowing arm must not disturb the established integer
    // explicit-narrowing law (i64 → i32 stays a `↦` row).
    test_support::assert_accepts("functio c(i64 a) → i32 ⇥ textus { redde a ↦ i32 }");
}

#[test]
fn float_narrowing_arrow_still_accepts() {
    test_support::assert_accepts("functio c(f64 x) → f32 { redde x ↦ f32 }");
}

// --- conversio rows: textus → decimal (settled at D3) ---

#[test]
fn textus_to_d64_arrow_accepts() {
    test_support::assert_accepts("functio c(textus s) → d64 ⇥ textus { redde s ↦ d64 }");
}

// --- CTR-02: channel contract for the builtin decimal rows ---
//
// The `textus → numerus<d64>` text-parse row is failable and keeps the
// builtin textus payload law: a bare `↦` with no inline recovery is admitted
// only into a `⇥ textus` channel, and a non-textus channel retains the
// `conversio_failure_channel_textus_required` diagnostic. The d64→d64
// identity row is total and needs no channel. A builtin row (no conversion
// function, hence no declared `⇥ E`) never admits failable-call propagation
// through the registry seam.

use super::TypeChecker;
use crate::scope::ConversionPair;
use crate::{FuncSig, NumericWidth, Primitive, Resolver, TypeTable};
use radix_lexer::{Interner, Span};
use radix_target::Target;

#[test]
fn textus_to_decimal_bare_arrow_requires_textus_channel() {
    test_support::assert_accepts("functio c(textus s) → d64 ⇥ textus { redde s ↦ d64 }");
    test_support::assert_rejects_with_issue(
        "functio c(textus s) → d64 ⇥ numerus { redde s ↦ d64 }",
        "conversio_failure_channel_textus_required",
    );
}

#[test]
fn decimal_identity_bare_arrow_needs_no_recovery_in_failable_function() {
    // d64 → d64 identity is total: a bare `↦` composes no failure channel and
    // needs no inline recovery.
    test_support::assert_accepts("functio c(d64 a) → d64 ⇥ numerus { redde a ↦ d64 }");
}

#[test]
fn decimal_builtin_row_carries_no_declared_channel() {
    // A compiler-shipped builtin row has no conversion function and therefore
    // no declared `⇥ E` channel: the registry seam must not turn it into a
    // failable-call conversion. The `textus → numerus<d64>` text-parse row is
    // a real builtin conversion, and the numeric widening law is bypassed by
    // using `textus` as the thrown payload.
    let mut types = TypeTable::new();
    let mut resolver = Resolver::new();
    let interner = Interner::new();
    let textus = types.primitive(Primitive::Textus);
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
    resolver
        .register_builtin_conversion(ConversionPair::new(textus, d64), Span::new(1, 2))
        .expect("builtin row enrollment");

    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    checker.current_error = Some(super::ErrorSink::Function(d64));
    let vacuum = checker.vacuum_type();
    let sig = FuncSig {
        type_params: Vec::new(),
        params: Vec::new(),
        ret: vacuum,
        err: Some(textus),
        is_async: false,
        is_generator: false,
    };
    let admitted = checker.reject_failable_call(&sig, crate::test_support::test_span());
    assert!(!admitted);
    assert_eq!(
        checker.pending_conversion, None,
        "a builtin row must not select a conversion function"
    );
    let err = checker
        .errors
        .iter()
        .find(|diag| diag.issue() == Some("failable_call_conversion_absent"))
        .expect("SEM010-shaped absence diagnostic retained");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "source" && arg.value == "textus")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "destination" && arg.value == "d64")
    );
}
