//! Typecheck pins for the `Bits` bitcast hint (`numerus<u32|u64> ↔ fractus<f32|f64>`).

use crate::test_support;

#[test]
fn u32_to_f32_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ f32 via Bits }",
    );
}

#[test]
fn f32_to_u32_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(fractus<f32> x) → numerus<u32> { redde x ↦ u32 via Bits }",
    );
}

#[test]
fn u64_to_f64_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(numerus<u64> n) → fractus<f64> { redde n ↦ f64 via Bits }",
    );
}

#[test]
fn f64_to_u64_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(fractus<f64> x) → numerus<u64> { redde x ↦ u64 via Bits }",
    );
}

// U1 red drivers (goal `bits-f16-pair`, unit U1) flipped green by U2: the
// `u16 ↔ f16` `Bits` row is admitted both directions in `bits_pair_issue`.
//
// Expected accept values fixed in advance for the runner pin (U3):
//   u16 0x3C00 ↔ 1.0, 0x3E00 ↔ 1.5, 0xBC00 ↔ -1.0, 0x7BFF ↔ 65504,
//   subnormal 0x0001, NaN payload 0x7E00 round-trips to the original bits.

#[test]
fn u16_to_f16_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(numerus<u16> n) → fractus<f16> { redde n ↦ f16 via Bits }",
    );
}

#[test]
fn f16_to_u16_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(fractus<f16> x) → numerus<u16> { redde x ↦ u16 via Bits }",
    );
}

#[test]
fn u16_to_f32_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u16> n) → fractus<f32> { redde n ↦ f32 via Bits }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn u16_to_f64_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u16> n) → fractus<f64> { redde n ↦ f64 via Bits }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn unsized_fractus_to_u32_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(fractus x) → numerus<u32> { redde x ↦ u32 via Bits }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn u32_to_f64_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u32> n) → fractus<f64> { redde n ↦ f64 via Bits }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn f16_to_u32_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(fractus<f16> x) → numerus<u32> { redde x ↦ u32 via Bits }",
        "bits_pair_width_mismatch",
    );
}

// OQ3 (settled by U2): `Bits` never co-occurs with `Hex`/`Bin`/`Oct` in one
// hint list — one interpretation family per `↦`; the structured issue is
// `conversio_bits_format_hint_conflict`.
#[test]
fn bits_with_hex_hint_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ fractus<f32, Bits, Hex> }",
        "conversio_bits_format_hint_conflict",
    );
}

#[test]
fn bits_with_bin_hint_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ fractus<f32, Bits, Bin> }",
        "conversio_bits_format_hint_conflict",
    );
}

#[test]
fn bits_with_oct_hint_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ fractus<f32, Bits, Oct> }",
        "conversio_bits_format_hint_conflict",
    );
}

#[test]
fn without_bits_value_conversion_stays() {
    test_support::assert_accepts(
        "functio val(numerus<u32> n) → fractus<f64> { redde n ↦ fractus<f64> }",
    );
}

#[test]
fn radix_hint_accept_row_stays_green() {
    test_support::assert_accepts("incipit { fixum _ n ← \"ff\" ↦ u32 via Hex }");
}

// U4 chain + derived-form pins (goal `bits-f16-pair`, unit U4): the
// left-folded chain types as exact widen, the `↤` typed-destination form
// routes through the same conversion table (zero new implementation), and
// the no-hint form stays the distinct integer-value conversion.

#[test]
fn u16_bits_chain_to_f32_types_as_exact_widen() {
    // `dbits ↦ f16 via Bits ↦ fractus<f32>`: Bits hop then the exact
    // f16 → f32 widen — the value-level "no intermediate binding" proof is
    // U5's runner exemplum; the type level pins the fold here.
    test_support::assert_accepts(
        "functio fold(numerus<u16> dbits) → fractus<f32> { redde dbits ↦ f16 via Bits ↦ fractus<f32> }",
    );
}

#[test]
fn u16_bits_chain_to_f32_typed_destination_accepts() {
    // `df` names `fractus<f32>`; the chain must fold to exactly that type
    // (not `fractus<f16>`), or the declaration rejects.
    test_support::assert_accepts(
        "incipit {\n  fixum numerus<u16> dbits ← 0x3E00\n  fixum fractus<f32> df ← dbits ↦ f16 via Bits ↦ fractus<f32>\n}",
    );
}

#[test]
fn typed_destination_conversio_assign_accepts_u16_to_f16() {
    // `↤` routes the right side through the same conversion registry as an
    // explicit `dbits ↦ fractus<f16>` — the destination type picks the pair
    // implicitly. Derived test: zero new implementation by design.
    test_support::assert_accepts(
        "incipit {\n  fixum numerus<u16> dbits ← 0x3E00\n  fixum fractus<f16> d ↤ dbits\n}",
    );
}

#[test]
fn u16_to_f32_without_bits_stays_integer_value_conversion() {
    // No `Bits` hint: this is the integer-value conversion, distinct from the
    // reinterpretation row — u16 0x3E00 (15872) becomes f32 15872.0, not the
    // f16-bits value 1.0. The value-level distinction is U5's corpus exemplum
    // on the runner; the type level pins the distinct admission here.
    test_support::assert_accepts(
        "functio val(numerus<u16> dbits) → fractus<f32> { redde dbits ↦ fractus<f32> }",
    );
}

#[test]
fn u16_to_f16_without_bits_stays_integer_value_conversion() {
    // Same distinct-value rule at the f16 width: no hint, no reinterpretation.
    test_support::assert_accepts(
        "functio val(numerus<u16> dbits) → fractus<f16> { redde dbits ↦ fractus<f16> }",
    );
}

// --- CTR-02: channel contract for the builtin `Bits` rows ---
//
// The `Bits` bitcast is a total conversion: a bare `↦` in a failable function
// needs no inline recovery and composes no failure channel, whatever the
// alternate-exit payload. And a builtin row (no conversion function, hence no
// declared `⇥ E`) never admits failable-call propagation through the registry
// seam — the SEM010-shaped absence diagnostic is retained.

use super::TypeChecker;
use crate::scope::ConversionPair;
use crate::{FuncSig, NumericWidth, Primitive, Resolver, TypeTable};
use radix_lexer::{Interner, Span};
use radix_target::Target;

#[test]
fn bits_bitcast_bare_arrow_needs_no_recovery_in_failable_function() {
    test_support::assert_accepts(
        "functio bits(numerus<u32> n) → fractus<f32> ⇥ textus { redde n ↦ f32 via Bits }",
    );
    test_support::assert_accepts(
        "functio bits(numerus<u32> n) → fractus<f32> ⇥ numerus { redde n ↦ f32 via Bits }",
    );
    test_support::assert_accepts(
        "functio bits(numerus<u64> n) → fractus<f64> ⇥ textus { redde n ↦ f64 via Bits }",
    );
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
            .any(|arg| arg.name == "source" && arg.value == "numerus<u32>")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "destination" && arg.value == "fractus<f32>")
    );
}
