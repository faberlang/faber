//! inf track U3: the typing rules of the unbounded integer `inf` — literals of
//! any size (ruling 34), the join and range rule (35), exact arithmetic types
//! (36), comparisons (37), stores (38) and the conversion rows (40/41).
//!
//! Types are read from `sit r ← <expr>` locals, so the tests pin the
//! expression's own type; rejections assert the structured `issue`.

use super::test_support::{self, entry_local_tys};
use crate::types::{Primitive, Type};
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{HirExpression, HirExpressionKind};
use radix_types::NumericWidth::{self, I64, Inf, U64};
use radix_types::numeric_width::numerus_integer_width;

const INF: &str = "sit inf x ← 3\nsit inf y ← 4";
const INF_AND_FRIENDS: &str =
    "sit inf x ← 3\nsit inf y ← 4\nsit u64 u ← 5\nsit i8 s ← 6\nsit u8 b ← 7";

fn analyze(decls: &str, expr: &str) -> test_support::AnalyzedFixture {
    let source = format!("incipit {{\n{decls}\nsit _ r ← {expr}\nnota r\n}}\n");
    test_support::analyze_ok_la(&source)
}

/// Integer width of `sit r ← <expr>`.
fn width_of(decls: &str, expr: &str) -> Option<NumericWidth> {
    let result = analyze(decls, expr);
    let ty = *entry_local_tys(&result).last().expect("locals");
    numerus_integer_width(&result.types, ty)
}

fn is_f64(decls: &str, expr: &str) -> bool {
    let result = analyze(decls, expr);
    let ty = *entry_local_tys(&result).last().expect("locals");
    matches!(result.types.get(ty), Type::Primitive(Primitive::Fractus))
}

fn assert_rejects(source: &str, issue: &str) {
    test_support::assert_rejects_with_issue_la(source, issue);
}

fn in_function(decls: &str, body: &str) -> String {
    format!("functio f({decls}) → numerus {{\n{body}\nredde 0\n}}\n")
}

// ---------------------------------------------------------------------------
// Ruling 34: literals of any size
// ---------------------------------------------------------------------------

const TWO_TO_64: &str = "18446744073709551616";

#[test]
fn a_literal_beyond_u64_is_accepted_in_an_inf_slot() {
    for literal in [
        TWO_TO_64,
        "0x10000000000000000",
        "0b1_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000",
        "0o2000000000000000000000",
        "100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    ] {
        test_support::assert_accepts_la(&format!(
            "incipit {{\nsit inf x ← {literal}\nnota x\n}}\n"
        ));
    }
}

#[test]
fn a_negated_big_literal_is_accepted_in_an_inf_slot() {
    test_support::assert_accepts_la(&format!(
        "incipit {{\nsit inf x ← -{TWO_TO_64}\nnota x\n}}\n"
    ));
}

#[test]
fn a_literal_beyond_u64_is_out_of_range_everywhere_else() {
    for slot in ["_ ", "numerus ", "u64 ", "i64 ", "u8 "] {
        assert_rejects(
            &format!("incipit {{\nsit {slot}x ← {TWO_TO_64}\nnota x\n}}\n"),
            "numerus_literal_out_of_range",
        );
    }
    assert_rejects(
        &format!("incipit {{\nnota {TWO_TO_64}\n}}\n"),
        "numerus_literal_out_of_range",
    );
}

#[test]
fn a_constant_expression_beyond_every_64_bit_type_is_out_of_range_unless_an_inf_takes_it() {
    assert_rejects(
        "incipit {\nnota 2 ⇐ 200\n}\n",
        "numerus_literal_out_of_range",
    );
    assert_rejects(
        &format!("incipit {{\nsit _ x ← {TWO_TO_64} + 1\nnota x\n}}\n"),
        "numerus_literal_out_of_range",
    );
    test_support::assert_accepts_la("incipit {\nsit inf x ← 2 ⇐ 200\nnota x\n}\n");
    test_support::assert_accepts_la(&format!(
        "incipit {{\nsit inf x ← {TWO_TO_64} + 1\nnota x\n}}\n"
    ));
    // A constant whose exact value fits is fine however it is spelled.
    test_support::assert_accepts_la(&format!(
        "incipit {{\nsit _ x ← {TWO_TO_64} - {TWO_TO_64}\nnota x\n}}\n"
    ));
}

#[test]
fn a_constant_beyond_u64_beside_a_bounded_operand_is_out_of_range() {
    assert_rejects(
        &format!("incipit {{\nsit u8 b ← 1\nnota b + {TWO_TO_64}\n}}\n"),
        "numerus_literal_out_of_range",
    );
}

#[test]
fn a_big_literal_beside_an_inf_operand_joins_by_exact_value() {
    assert_eq!(width_of(INF, &format!("x + {TWO_TO_64}")), Some(Inf));
    assert_eq!(width_of(INF, &format!("{TWO_TO_64} * x")), Some(Inf));
    assert_eq!(width_of(INF, &format!("x ∧ {TWO_TO_64}")), Some(Inf));
}

#[test]
fn a_big_literal_in_a_float_slot_follows_the_float_constant_rule() {
    // 2^64 is exactly an f64; 10^40 is not representable in f32's range.
    test_support::assert_accepts_la(&format!(
        "incipit {{\nsit f64 f ← {TWO_TO_64}\nnota f\n}}\n"
    ));
    assert_rejects(
        "incipit {\nsit f32 f ← 10000000000000000000000000000000000000000\nnota f\n}\n",
        "float_constant_out_of_range",
    );
}

#[test]
fn a_big_literal_pattern_on_an_inf_subject_is_a_constant_pattern() {
    test_support::assert_accepts_la(&format!(
        "functio f(inf x) → numerus {{\nelige x {{\ncasu {TWO_TO_64} {{ redde 1 }}\nceterum {{ redde 0 }}\n}}\n}}\n"
    ));
    test_support::assert_accepts_la(
        "functio f(inf x) → numerus {\nelige x {\ncasu 5 { redde 1 }\nceterum { redde 0 }\n}\n}\n",
    );
}

// ---------------------------------------------------------------------------
// Ruling 35: a typed inf operand makes the result inf; bounded stays capped
// ---------------------------------------------------------------------------

#[test]
fn every_integer_operator_on_inf_types_as_inf() {
    for expr in [
        "x + y",
        "x - y",
        "x * y",
        "x / y",
        "x % y",
        "x ∧ y",
        "x ∨ y",
        "x ⊻ y",
        "x ⇐ 3",
        "x ⇒ 3",
        "x ⇐ y",
        "-x",
        "¬x",
        "x + 1",
        "1 + x",
        "x * x * x",
        "x.potentia(5)",
        "x.potentia(y)",
        "x.absolutum()",
    ] {
        assert_eq!(width_of(INF, expr), Some(Inf), "{expr}");
    }
}

#[test]
fn an_untyped_constant_beside_inf_and_any_bounded_width_joins_to_inf() {
    for expr in ["u + x", "x - u", "s * x", "b ⇐ x", "x ∧ u", "u - x"] {
        assert_eq!(width_of(INF_AND_FRIENDS, expr), Some(Inf), "{expr}");
    }
}

#[test]
fn true_division_of_inf_is_f64() {
    assert!(is_f64(INF, "x ÷ y"));
    assert!(is_f64(INF_AND_FRIENDS, "x ÷ b"));
}

#[test]
fn summa_of_a_list_of_inf_is_inf() {
    assert_eq!(
        width_of("sit inf x ← 3\nsit lista<inf> xs ← [x, x]", "xs.summa()"),
        Some(Inf)
    );
}

#[test]
fn branches_and_collections_of_inf_are_inf() {
    let decls = "sit inf x ← 3\nsit bivalens c ← verum";
    assert_eq!(width_of(decls, "c ✓ x ✗ 0"), Some(Inf));
    assert_eq!(width_of(decls, "c ✓ 0 ✗ x"), Some(Inf));
    let decls = "sit inf x ← 3\nsit u8 b ← 1\nsit bivalens c ← verum";
    assert_eq!(width_of(decls, "c ✓ b ✗ x"), Some(Inf));
    test_support::assert_accepts_la(
        "incipit {\nsit inf x ← 3\nsit u8 b ← 1\nsit lista<inf> xs ← [b, x, 7]\nnota xs\n}\n",
    );
}

#[test]
fn a_saturating_word_never_meets_inf_in_arithmetic() {
    // `inf` is a trapping type: the saturating family check still applies.
    assert_rejects(
        "functio f(saturatus<u8> s, inf x) → numerus { sit _ r ← s + x\nredde 0 }\n",
        "saturating_word_family_mismatch",
    );
    assert_rejects(
        "functio f(saturatus<u8> s, inf x) → numerus { sit _ r ← x ∧ s\nredde 0 }\n",
        "saturating_word_family_mismatch",
    );
}

#[test]
fn u64_minus_u64_stays_i64() {
    let decls = "sit u64 a ← 7\nsit u64 b ← 5";
    assert_eq!(width_of(decls, "a - b"), Some(I64));
    assert_eq!(width_of(decls, "a + b"), Some(U64));
    assert_eq!(width_of(decls, "a * b"), Some(U64));
}

#[test]
fn a_bounded_operand_expression_keeps_the_64_bit_cap() {
    let decls = "sit i64 a ← 7";
    assert_eq!(width_of(decls, "a * a"), Some(I64));
    assert_eq!(width_of(decls, "a + 1"), Some(I64));
}

#[test]
fn u64_with_a_signed_operand_is_still_an_error_with_both_fixes() {
    assert_rejects(
        "functio f(u64 a, i8 b) → numerus { sit _ r ← a + b\nredde 0 }\n",
        "u64_signed_arithmetic_requires_conversion",
    );
    // `a ↦ inf` is the lossless fix the ruling names.
    test_support::assert_accepts_la(
        "functio f(u64 a, i8 b) → numerus {\nsit _ r ← (a ↦ inf) + b\nnota r\nredde 0 }\n",
    );
}

#[test]
fn every_bounded_width_widens_into_inf_without_a_conversion() {
    test_support::assert_accepts_la(
        "functio f(u64 u, i64 i, u8 b, i8 s) → inf {\n\
         sit inf a ← u\nsit inf c ← i\nsit inf d ← b\nsit inf e ← s\nnota c\nnota d\nnota e\nredde a\n}\n",
    );
}

#[test]
fn inf_widens_into_nothing_implicitly_a_bounded_slot_takes_a_checked_narrowing() {
    // The implicit store of ruling 38: accepted, a trap at run time.
    test_support::assert_accepts_la("functio f(inf a) → u8 {\nsit u8 z ← a\nredde z\n}\n");
    test_support::assert_accepts_la("functio f(inf a) → numerus {\nredde a\n}\n");
    // Policy-word slots still need an authored conversion.
    for slot in ["modulus<u8>", "saturatus<u8>"] {
        assert_rejects(
            &format!("functio f(inf a) → numerus {{\nsit {slot} z ← a\nredde 0\n}}\n"),
            "expression_type_mismatch",
        );
    }
}

// ---------------------------------------------------------------------------
// Ruling 37: comparisons and equality
// ---------------------------------------------------------------------------

#[test]
fn inf_compares_by_value_against_every_numeric_family() {
    let decls = "sit inf x ← 3\nsit u64 u ← 5\nsit i64 i ← 5\nsit fractus f ← 1.5\nsit d64 d ← 1.5";
    for expr in [
        "x ≺ u",
        "u ≺ x",
        "x ≻ i",
        "x ≤ f",
        "f ≥ x",
        "x ≅ u",
        "x ≇ i",
        "x ≅ f",
        "x ≺ d",
        "d ≻ x",
        "x ≅ 5",
        "x ≺ 18446744073709551616",
    ] {
        test_support::assert_accepts_la(&format!("incipit {{\n{decls}\nnota {expr}\n}}\n"));
    }
}

#[test]
fn inf_equality_stays_exact_type() {
    assert_rejects(
        "functio f(inf a, u64 b) → bivalens { redde a ≡ b }\n",
        "equality_exact_types_required",
    );
    test_support::assert_accepts_la("functio f(inf a, inf b) → bivalens { redde a ≡ b }\n");
    // An untyped constant compares by value.
    test_support::assert_accepts_la("functio f(inf a) → bivalens { redde a ≡ 5 }\n");
    test_support::assert_accepts_la(&format!(
        "functio f(inf a) → bivalens {{ redde a ≡ {TWO_TO_64} }}\n"
    ));
}

// ---------------------------------------------------------------------------
// Ruling 38: stores
// ---------------------------------------------------------------------------

#[test]
fn a_store_into_an_inf_slot_is_total() {
    test_support::assert_accepts_la(
        "functio f(u64 a, u64 b) → inf {\nsit inf d ← a - b\nredde d\n}\n",
    );
    test_support::assert_accepts_la(
        "functio f(i64 a) → inf {\nvaria inf d ← 0\nd ← a * a\nredde d\n}\n",
    );
}

#[test]
fn an_inf_store_into_a_bounded_slot_is_an_implicit_checked_narrowing() {
    let source = "functio f(inf a) → u8 {\nsit u8 z ← a\nredde z\n}\n";
    let fixture = test_support::analyze_ok_la(source);
    let hir = fixture.hir.as_ref().expect("hir");
    let mut stores = ImplicitStores::default();
    walk_program(&mut stores, hir);
    assert_eq!(
        stores.count, 1,
        "one implicit store node carries the narrowing"
    );
    // Into an inf slot there is nothing to check, so no node.
    let total = test_support::analyze_ok_la(
        "functio f(u64 a, u64 b) → inf {\nsit inf d ← a - b\nredde d\n}\n",
    );
    let mut none = ImplicitStores::default();
    walk_program(&mut none, total.hir.as_ref().expect("hir"));
    assert_eq!(none.count, 0, "a store into inf is total");
}

#[derive(Default)]
struct ImplicitStores {
    count: usize,
}

impl HirVisitor for ImplicitStores {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(
            &expr.kind,
            HirExpressionKind::Conversio {
                implicit: Some(_),
                ..
            }
        ) {
            self.count += 1;
        }
        walk_expr(self, expr);
    }
}

#[test]
fn a_constant_into_a_word_slot_beyond_u64_follows_the_slot_policy() {
    test_support::assert_accepts_la(&format!(
        "incipit {{\nsit saturatus<u8> s ← 2 ⇐ 200\nsit modulus<u8> w ← {TWO_TO_64} + 5\nnota s\nnota w\n}}\n"
    ));
}

#[test]
fn a_constant_expression_beyond_a_bounded_trapping_slot_is_a_compile_error() {
    assert_rejects(
        "incipit {\nsit u8 x ← 2 ⇐ 200\nnota x\n}\n",
        "store_constant_out_of_range",
    );
}

// ---------------------------------------------------------------------------
// Ruling 40/41: conversion rows and D17.8
// ---------------------------------------------------------------------------

fn conversion(param: &str, target: &str, expr: &str) -> String {
    format!("functio f({param}) → {target} {{\nredde {expr}\n}}\n")
}

#[test]
fn a_bounded_integer_converts_into_inf_and_never_fails() {
    for source in ["u8", "i64", "u64", "modulus<u16>", "saturatus<i32>"] {
        test_support::assert_accepts_la(&conversion(&format!("{source} a"), "inf", "a ↦ inf"));
    }
}

#[test]
fn inf_into_a_bounded_trapping_integer_is_failable() {
    for target in ["u8", "i64", "u64", "numerus"] {
        assert_rejects(
            &conversion("inf a", target, &format!("a ↦ {target}")),
            "failable_conversio_requires_handler",
        );
        test_support::assert_accepts_la(&conversion("inf a", target, &format!("a ↦ {target} ⊥ 0")));
    }
    // `main` is exempt: an unhandled failure there panics.
    test_support::assert_accepts_la("incipit {\nsit inf x ← 300\nnota x ↦ u8\n}\n");
}

#[test]
fn a_constant_that_fits_converts_out_of_inf_without_a_handler() {
    test_support::assert_accepts_la("functio f() → u8 {\nredde 200 ↦ u8\n}\n");
}

#[test]
fn inf_into_a_policy_word_reduces_or_clamps_and_cannot_fail() {
    test_support::assert_accepts_la(&conversion("inf a", "modulus<u8>", "a ↦ modulus<u8>"));
    test_support::assert_accepts_la(&conversion("inf a", "saturatus<u8>", "a ↦ saturatus<u8>"));
}

#[test]
fn inf_into_a_float_never_fails() {
    for target in ["f16", "f32", "fractus"] {
        test_support::assert_accepts_la(&conversion("inf a", target, &format!("a ↦ {target}")));
    }
}

#[test]
fn a_float_into_inf_fails_only_at_nan_and_infinity() {
    assert_rejects(
        &conversion("fractus a", "inf", "a ↦ inf via Trunc"),
        "failable_conversio_requires_handler",
    );
    test_support::assert_accepts_la(&conversion("fractus a", "inf", "a ↦ inf via Trunc ⊥ 0"));
    // A finite constant is proven.
    test_support::assert_accepts_la(&conversion("numerus a", "inf", "3.5 ↦ inf via Trunc"));
    assert_rejects(
        &conversion("numerus a", "inf", "∞ ↦ inf via Trunc"),
        "failable_conversio_requires_handler",
    );
}

#[test]
fn d64_into_inf_never_fails_and_inf_into_d64_is_checked() {
    test_support::assert_accepts_la(&conversion("d64 a", "inf", "a ↦ inf via HalfEven"));
    assert_rejects(
        &conversion("inf a", "d64", "a ↦ d64"),
        "failable_conversio_requires_handler",
    );
    test_support::assert_accepts_la(&conversion("inf a", "d64", "a ↦ d64 ⊥ 0"));
    test_support::assert_accepts_la(&conversion("numerus a", "d64", "5 ↦ d64"));
}

#[test]
fn text_into_inf_is_failable_and_inf_into_text_is_total() {
    assert_rejects(
        &conversion("textus a", "inf", "a ↦ inf"),
        "failable_conversio_requires_handler",
    );
    test_support::assert_accepts_la(&conversion("textus a", "inf", "a ↦ inf ⊥ 0"));
    test_support::assert_accepts_la(&conversion("inf a", "textus", "a ↦ textus"));
}

#[test]
fn inf_packs_into_ascii_digits_through_a_radix_hint() {
    test_support::assert_accepts_la(&conversion(
        "inf a",
        "ascii<magnitudo 40>",
        "a ↦ ascii<magnitudo 40> via Hex ⊥ 'x'",
    ));
}

#[test]
fn inf_converts_to_and_from_octeti_through_an_endian_hint() {
    test_support::assert_accepts_la(&conversion("lista<b8> a", "inf", "a ↦ inf via Be"));
    test_support::assert_accepts_la(&conversion("lista<b8> a", "inf", "a ↦ inf via Le"));
    test_support::assert_accepts_la(&conversion("inf a", "lista<b8>", "a ↦ lista<b8> via Be"));
    // Without an endian hint there is no meaning, exactly as for every width.
    assert_rejects(
        &conversion("inf a", "lista<b8>", "a ↦ lista<b8>"),
        "numeric_to_octeti_unspecified",
    );
    assert_rejects(
        &conversion("lista<b8> a", "inf", "a ↦ inf"),
        "octeti_to_numerus_unspecified",
    );
    // A bounded width still has no byte encoding.
    assert_rejects(
        &conversion("u32 a", "lista<b8>", "a ↦ lista<b8> via Be"),
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn inf_and_valor_convert_both_ways() {
    test_support::assert_accepts_la(&conversion("inf a", "valor", "a ↦ valor"));
    test_support::assert_accepts_la(&conversion("valor a", "inf", "a ↦ inf ⊥ 0"));
}

#[test]
fn inf_has_no_bits_view() {
    assert_rejects(
        &conversion("inf a", "b32", "a ↦ b32"),
        "bits_conversion_width_mismatch",
    );
    assert_rejects(
        &conversion("b64 a", "inf", "a ↦ inf"),
        "bits_conversion_width_mismatch",
    );
}

#[test]
fn a_fact_ascription_widens_into_inf_and_holds_any_constant() {
    test_support::assert_accepts_la(&conversion("u8 a", "inf", "a ∷ inf"));
    test_support::assert_accepts_la(&conversion("u64 a", "inf", "a ∷ inf"));
    test_support::assert_accepts_la(&conversion("numerus a", "inf", "5 ∷ inf"));
    assert_rejects(
        &conversion("inf a", "u64", "a ∷ u64"),
        "numeric_ascription_requires_conversion",
    );
}

#[test]
fn a_conversion_into_a_float_of_a_constant_beyond_its_range_is_a_compile_error() {
    // Ruling 20 for constants of any size: 10^40 is beyond f32, exact in f64.
    assert_rejects(
        "incipit {\nsit f32 f ← 10000000000000000000000000000000000000000 ↦ f32\nnota f\n}\n",
        "float_constant_out_of_range",
    );
}

// ---------------------------------------------------------------------------
// Value flow and unrelated code keep working
// ---------------------------------------------------------------------------

#[test]
fn increment_and_range_loops_over_inf_type_check() {
    test_support::assert_accepts_la("incipit {\nvaria inf x ← 1\nx ↑\nnota x\n}\n");
    test_support::assert_accepts_la(
        "incipit {\nsit inf n ← 3\nitera ab 0‥n sit i {\nnota i\n}\n}\n",
    );
}

#[test]
fn shift_and_potentia_counts_follow_the_existing_negative_rules_on_inf() {
    assert_rejects(
        &in_function("inf a", "sit _ r ← a ⇐ -1"),
        "shift_count_negative",
    );
    assert_rejects(
        &in_function("inf a", "sit _ r ← a.potentia(-1)"),
        "potentia_exponent_negative",
    );
}

// ---------------------------------------------------------------------------
// Constants beside inf, float retyping, discerne patterns
// ---------------------------------------------------------------------------

#[test]
fn a_constant_of_any_sign_and_size_beside_inf_joins_by_exact_value() {
    for constant in [
        "9223372036854775808",
        "-9223372036854775809",
        "-18446744073709551616",
        "(2 ⇐ 100)",
    ] {
        assert_eq!(
            width_of(INF, &format!("x + {constant}")),
            Some(Inf),
            "{constant}"
        );
        assert_eq!(
            width_of(INF, &format!("{constant} - y")),
            Some(Inf),
            "{constant}"
        );
    }
}

#[derive(Default)]
struct BigLiterals {
    big: usize,
    floats: usize,
    digits: Vec<String>,
}

impl HirVisitor for BigLiterals {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Literal(radix_hir::HirLiteral::BigInt(text)) => {
                self.big += 1;
                self.digits.push(text.to_string());
            }
            HirExpressionKind::Literal(radix_hir::HirLiteral::Float(_)) => self.floats += 1,
            _ => {}
        }
        walk_expr(self, expr);
    }
}

fn literals_of(source: &str) -> BigLiterals {
    let fixture = test_support::analyze_ok_la(source);
    let hir = fixture.hir.as_ref().expect("hir");
    let mut literals = BigLiterals::default();
    walk_program(&mut literals, hir);
    literals
}

#[test]
fn a_big_literal_in_a_float_slot_becomes_a_float_constant() {
    let literals = literals_of(&format!(
        "incipit {{\nsit f64 f ← {TWO_TO_64}\nnota f\n}}\n"
    ));
    assert_eq!(literals.big, 0, "no integer literal carries a float type");
    assert_eq!(literals.floats, 1);
}

#[test]
fn a_big_literal_normalises_to_canonical_decimal_in_hir() {
    let literals = literals_of("incipit {\nsit inf x ← 0x10000000000000000\nnota x\n}\n");
    assert_eq!(literals.digits, vec![TWO_TO_64.to_owned()]);
}

#[test]
fn a_big_literal_pattern_on_an_inf_subject_elige_is_accepted() {
    test_support::assert_accepts_la(&format!(
        "functio f(inf x) → numerus {{\nelige x {{\ncasu {TWO_TO_64} ergo redde 1\ncasu -{TWO_TO_64} ergo redde 2\nceterum ergo redde 0\n}}\n}}\n"
    ));
}
