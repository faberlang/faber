//! Typecheck pins for the retired `via Bits` hint (need, ruling
//! 1dce2442 C3 ADDITION 1): every spelling is the named retirement
//! diagnostic, and the `bN` replacements type.

use crate::test_support;

const RETIRED: &str = "conversio_via_bits_retired";

#[test]
fn via_bits_is_retired_for_every_pair() {
    for (param, ret, body) in [
        ("u32 n", "f32", "n ↦ f32 via Bits"),
        ("f32 x", "u32", "x ↦ u32 via Bits"),
        ("u64 n", "f64", "n ↦ f64 via Bits"),
        ("f64 x", "u64", "x ↦ u64 via Bits"),
        ("u16 n", "f16", "n ↦ f16 via Bits"),
        ("f16 x", "u16", "x ↦ u16 via Bits"),
        ("u16 n", "bf16", "n ↦ bf16 via Bits"),
        ("bf16 x", "u16", "x ↦ u16 via Bits"),
        ("u16 n", "f32", "n ↦ f32 via Bits"),
        ("u32 n", "f64", "n ↦ f64 via Bits"),
        ("d64 n", "f32", "n ↦ f32 via Bits"),
        ("inf a", "f32", "a ↦ f32 via Bits"),
    ] {
        test_support::assert_rejects_with_issue_la(
            &format!("functio bits({param}) → {ret} {{ redde {body} }}"),
            RETIRED,
        );
    }
}

#[test]
fn retirement_help_names_the_bn_replacement() {
    let help = crate::catalog_semantic::semantic_help(
        crate::error::SemanticErrorKind::ConversionInvalid,
        &[radix_diagnostics::DiagnosticArg::new("issue", RETIRED)],
    )
    .expect("catalog carries help for the retirement");
    assert!(
        help.contains("↦ b32"),
        "help names the bN replacement: {help}"
    );
    assert!(
        help.contains("u16 ↦ b16 ↦ f16"),
        "help shows the number route: {help}"
    );
}

#[test]
fn float_to_pattern_is_the_bn_marker() {
    test_support::assert_accepts_la("functio bits(f32 x) → b32 { redde x ↦ b32 }");
    test_support::assert_accepts_la("functio bits(f64 x) → b64 { redde x ↦ b64 }");
    test_support::assert_accepts_la("functio bits(f16 x) → b16 { redde x ↦ b16 }");
    test_support::assert_accepts_la("functio bits(bf16 x) → b16 { redde x ↦ b16 }");
}

#[test]
fn pattern_to_float_is_the_bn_marker() {
    test_support::assert_accepts_la("functio bits(b32 x) → f32 { redde x ↦ f32 }");
    test_support::assert_accepts_la("functio bits(b64 x) → f64 { redde x ↦ f64 }");
    test_support::assert_accepts_la("functio bits(b16 x) → f16 { redde x ↦ f16 }");
    test_support::assert_accepts_la("functio bits(b16 x) → bf16 { redde x ↦ bf16 }");
}

#[test]
fn number_holding_a_pattern_goes_through_its_marker() {
    // `u16 ↦ b16 ↦ f16`: the author says the u16 holds a pattern, then reads
    // it as a half float.
    test_support::assert_accepts_la("functio bits(u16 n) → f16 { redde n ↦ b16 ↦ f16 }");
    test_support::assert_accepts_la("functio bits(u16 n) → f32 { redde n ↦ b16 ↦ f16 ↦ f32 }");
    test_support::assert_accepts_la("functio bits(u16 n) → bf16 { redde n ↦ b16 ↦ bf16 }");
    test_support::assert_accepts_la("functio bits(u32 n) → f32 { redde n ↦ b32 ↦ f32 }");
    test_support::assert_accepts_la("functio bits(u64 n) → f64 { redde n ↦ b64 ↦ f64 }");
    test_support::assert_accepts_la("functio bits(f32 x) → u32 { redde x ↦ b32 ↦ u32 }");
}

#[test]
fn bit_width_must_match_the_float() {
    test_support::assert_rejects_with_issue_la(
        "functio bits(u16 n) → f32 { redde n ↦ b16 ↦ f32 }",
        "bits_conversion_width_mismatch",
    );
}

#[test]
fn without_a_marker_value_conversion_stays() {
    test_support::assert_accepts_la("functio val(u32 n) → f64 { redde n ↦ f64 }");
    test_support::assert_accepts_la("functio val(u16 dbits) → f32 { redde dbits ↦ f32 }");
    test_support::assert_accepts_la("functio val(u16 dbits) → f16 { redde dbits ↦ f16 }");
}

#[test]
fn radix_hint_accept_row_stays_green() {
    test_support::assert_accepts_la("incipit { fixum _ n ← \"ff\" ↦ u32 via Hex }");
}

#[test]
fn typed_destination_conversio_assign_accepts_u16_to_f16() {
    test_support::assert_accepts_la(
        "incipit {\n  fixum u16 dbits ← 0x3E00\n  fixum f16 d ↤ dbits\n}",
    );
}

// --- CTR-02: channel contract for the builtin `Bits` rows ---
//
// A `↦ bN` reinterpretation is a total conversion: a bare `↦` in a failable
// function needs no inline recovery and composes no failure channel, whatever
// the alternate-exit payload. And a builtin row (no conversion function, hence no
// declared `⇥ E`) never admits failable-call propagation through the registry
// seam — the SEM010-shaped absence diagnostic is retained.

use super::TypeChecker;
use crate::scope::ConversionPair;
use crate::{FuncSig, NumericWidth, Primitive, Resolver, TypeTable};
use radix_lexer::{Interner, Span};
use radix_target::Target;

#[test]
fn bn_reinterpretation_bare_arrow_needs_no_recovery_in_failable_function() {
    test_support::assert_accepts_la("functio bits(f32 x) → b32 ⇥ textus { redde x ↦ b32 }");
    test_support::assert_accepts_la("functio bits(f32 x) → b32 ⇥ numerus { redde x ↦ b32 }");
    test_support::assert_accepts_la("functio bits(f64 x) → b64 ⇥ textus { redde x ↦ b64 }");
}

#[test]
fn bits_builtin_row_carries_no_declared_channel() {
    // A compiler-shipped builtin row has no conversion function and therefore
    // no declared `⇥ E` channel: the registry seam must not turn it into a
    // failable-call conversion.
    let mut types = TypeTable::new();
    let mut resolver = Resolver::new();
    let interner = Interner::new();
    let u32 = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    resolver
        .register_builtin_conversion(ConversionPair::new(u32, f32), Span::new(1, 2))
        .expect("builtin row enrollment");

    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    checker.current_error = Some(super::ErrorSink::Function(f32));
    let vacuum = checker.vacuum_type();
    let sig = FuncSig {
        type_params: Vec::new(),
        params: Vec::new(),
        ret: vacuum,
        err: Some(u32),
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
            .any(|arg| arg.name == "source" && arg.value == "u32")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "destination" && arg.value == "f32")
    );
}
