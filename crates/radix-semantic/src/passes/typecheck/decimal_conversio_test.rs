//! decimal-widths D3: the semantic conversio/equality arms for
//! `numerus<d32>` / `numerus<d64>`.
//!
//! Matrix (CTO ruling + D3 unit): within-width `≡` is exact; cross-width bare
//! `≡` rejects per the SEM010 exact-types law (explicit `↦` re-enables);
//! cross-family `≡` rejects (no decimal-wins promotion). The `↦` rows are
//! decimal↔integer (round-half-even), fractus→decimal (string round-trip),
//! decimal→fractus (defined rounding), d32→d64 widening, and textus→decimal
//! (text-parse row, settled at D3 — consistent with plain `textus ↦ numerus`).
//! The reverse d64→d32 is a narrowing with no `↦` arm and rejects.

use crate::test_support;

// --- equality lattice ---

#[test]
fn within_width_decimal_equality_accepts() {
    test_support::assert_accepts("functio same(numerus<d32> a) → bivalens { redde a ≡ a }");
}

#[test]
fn within_width_d64_equality_accepts() {
    test_support::assert_accepts("functio same(numerus<d64> a) → bivalens { redde a ≡ a }");
}

#[test]
fn cross_width_decimal_equality_rejects_bare() {
    // SEM010 pin (existing law): d32 ≡ d64 needs the explicit `↦` side.
    test_support::assert_rejects_with_issue(
        "functio same(numerus<d32> a, numerus<d64> b) → bivalens { redde a ≡ b }",
        "equality_exact_types_required",
    );
}

#[test]
fn cross_width_decimal_equality_accepts_with_explicit_arrow() {
    test_support::assert_accepts(
        "functio same(numerus<d32> a, numerus<d64> b) → bivalens { redde a ↦ numerus<d64> ≡ b }",
    );
}

#[test]
fn cross_family_decimal_integer_equality_rejects() {
    // No decimal-wins promotion: a decimal value never compares bare with an
    // integer value, even though both are numerus-family widths.
    test_support::assert_rejects_with_issue(
        "functio same(numerus<d32> a, numerus n) → bivalens { redde a ≡ n }",
        "equality_exact_types_required",
    );
}

#[test]
fn cross_family_decimal_fractus_equality_rejects() {
    test_support::assert_rejects_with_issue(
        "functio same(numerus<d32> a) → bivalens { redde a ≡ 4.2 }",
        "equality_exact_types_required",
    );
}

// --- conversio rows: decimal ↔ integer (round-half-even) ---

#[test]
fn decimal_to_integer_arrow_accepts() {
    test_support::assert_accepts(
        "functio c(numerus<d32> a) → numerus<i64> { redde a ↦ numerus<i64> }",
    );
}

#[test]
fn decimal_to_unsigned_integer_arrow_accepts() {
    test_support::assert_accepts(
        "functio c(numerus<d64> a) → numerus<u32> { redde a ↦ numerus<u32> }",
    );
}

#[test]
fn integer_to_decimal_arrow_accepts() {
    test_support::assert_accepts(
        "functio c(numerus<i64> a) → numerus<d32> { redde a ↦ numerus<d32> }",
    );
}

#[test]
fn unsigned_integer_to_decimal_arrow_accepts() {
    test_support::assert_accepts(
        "functio c(numerus<u32> a) → numerus<d64> { redde a ↦ numerus<d64> }",
    );
}

// --- conversio rows: fractus ↔ decimal (string round-trip / defined rounding) ---

#[test]
fn fractus_to_decimal_arrow_accepts() {
    test_support::assert_accepts("functio c(fractus x) → numerus<d32> { redde x ↦ numerus<d32> }");
}

#[test]
fn sized_fractus_to_decimal_arrow_accepts() {
    test_support::assert_accepts(
        "functio c(fractus<f32> x) → numerus<d64> { redde x ↦ numerus<d64> }",
    );
}

#[test]
fn decimal_to_fractus_arrow_accepts() {
    test_support::assert_accepts("functio c(numerus<d32> a) → fractus { redde a ↦ fractus }");
}

#[test]
fn decimal_to_sized_fractus_arrow_accepts() {
    test_support::assert_accepts(
        "functio c(numerus<d64> a) → fractus<f32> { redde a ↦ fractus<f32> }",
    );
}

// --- conversio rows: d32 → d64 widening; reverse rejects ---

#[test]
fn d32_to_d64_arrow_accepts() {
    test_support::assert_accepts(
        "functio c(numerus<d32> a) → numerus<d64> { redde a ↦ numerus<d64> }",
    );
}

#[test]
fn d64_to_d32_arrow_rejects_with_teaching_issue() {
    test_support::assert_rejects_with_issue(
        "functio c(numerus<d64> a) → numerus<d32> { redde a ↦ numerus<d32> }",
        "decimal_width_narrowing_rejected",
    );
}

#[test]
fn d64_to_d32_conversio_assign_rejects_with_teaching_issue() {
    // The `↤` conversion-directed initializer lowers through the same
    // `check_conversio` arm as `↦`.
    test_support::assert_rejects_with_issue(
        r"functio c(numerus<d64> a) → numerus<d32> {
    fixum numerus<d32> b ↤ a
    redde b
}",
        "decimal_width_narrowing_rejected",
    );
}

#[test]
fn integer_narrowing_arrow_still_accepts() {
    // The decimal narrowing arm must not disturb the established integer
    // explicit-narrowing law (i64 → i32 stays a `↦` row).
    test_support::assert_accepts(
        "functio c(numerus<i64> a) → numerus<i32> { redde a ↦ numerus<i32> }",
    );
}

#[test]
fn float_narrowing_arrow_still_accepts() {
    test_support::assert_accepts(
        "functio c(fractus<f64> x) → fractus<f32> { redde x ↦ fractus<f32> }",
    );
}

// --- conversio rows: textus → decimal (settled at D3) ---

#[test]
fn textus_to_decimal_arrow_accepts() {
    // D3 settles the D2 deferral: decimal text parse is a conversio row,
    // consistent with plain `textus ↦ numerus`. Round-half-even parsing is
    // the D4 runtime carrier.
    test_support::assert_accepts("functio c(textus s) → numerus<d32> { redde s ↦ numerus<d32> }");
}

#[test]
fn textus_to_d64_arrow_accepts() {
    test_support::assert_accepts("functio c(textus s) → numerus<d64> { redde s ↦ numerus<d64> }");
}

// --- CTR-02: channel contract for the builtin decimal rows ---
//
// The `textus → numerus<d32|d64>` text-parse row is failable and keeps the
// builtin textus payload law: a bare `↦` with no inline recovery is admitted
// only into a `⇥ textus` channel, and a non-textus channel retains the
// `conversio_failure_channel_textus_required` diagnostic. The d32→d64
// widening row is total and needs no channel. A builtin row (no conversion
// function, hence no declared `⇥ E`) never admits failable-call propagation
// through the registry seam.

use super::TypeChecker;
use crate::scope::ConversionPair;
use crate::{FuncSig, NumericWidth, Primitive, Resolver, TypeTable};
use radix_lexer::{Interner, Span};
use radix_target::Target;

#[test]
fn textus_to_decimal_bare_arrow_requires_textus_channel() {
    test_support::assert_accepts(
        "functio c(textus s) → numerus<d32> ⇥ textus { redde s ↦ numerus<d32> }",
    );
    test_support::assert_accepts(
        "functio c(textus s) → numerus<d64> ⇥ textus { redde s ↦ numerus<d64> }",
    );
    test_support::assert_rejects_with_issue(
        "functio c(textus s) → numerus<d32> ⇥ numerus { redde s ↦ numerus<d32> }",
        "conversio_failure_channel_textus_required",
    );
    test_support::assert_rejects_with_issue(
        "functio c(textus s) → numerus<d64> ⇥ numerus { redde s ↦ numerus<d64> }",
        "conversio_failure_channel_textus_required",
    );
}

#[test]
fn decimal_widening_bare_arrow_needs_no_recovery_in_failable_function() {
    // d32 → d64 widening is total: a bare `↦` composes no failure channel and
    // needs no inline recovery.
    test_support::assert_accepts(
        "functio c(numerus<d32> a) → numerus<d64> ⇥ textus { redde a ↦ numerus<d64> }",
    );
    test_support::assert_accepts(
        "functio c(numerus<d32> a) → numerus<d64> ⇥ numerus { redde a ↦ numerus<d64> }",
    );
}

#[test]
fn decimal_builtin_row_carries_no_declared_channel() {
    // A compiler-shipped builtin row has no conversion function and therefore
    // no declared `⇥ E` channel: the registry seam must not turn it into a
    // failable-call conversion. The `textus → numerus<d32>` text-parse row is
    // a real builtin conversion, and the numeric widening law is bypassed by
    // using `textus` as the thrown payload.
    let mut types = TypeTable::new();
    let mut resolver = Resolver::new();
    let interner = Interner::new();
    let textus = types.primitive(Primitive::Textus);
    let d32 = types.sized_numeric(Primitive::Numerus, NumericWidth::D32);
    resolver
        .register_builtin_conversion(ConversionPair::new(textus, d32), Span::new(1, 2))
        .expect("builtin row enrollment");

    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    checker.current_error = Some(super::ErrorSink::Function(d32));
    let vacuum = checker.vacuum_type();
    let sig = FuncSig {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
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
    assert!(err.args.iter().any(|arg| arg.name == "source" && arg.value == "textus"));
    assert!(err.args.iter().any(|arg| arg.name == "destination" && arg.value == "numerus<d32>"));
}
