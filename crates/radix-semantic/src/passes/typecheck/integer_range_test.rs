//! d11-6 U3: range typing of trapping integer expressions, untyped integer
//! constants, and number-family crossing (rulings 8, 12, 13, 16, 19, 20, 21).
//!
//! Types are read from `sit r ← <expr>` locals, so the tests pin the
//! expression's own type and never the store rule (U4).

use super::test_support::{self, entry_local_tys};
use crate::types::{Primitive, Type};
use radix_types::NumericWidth::{self, I8, I16, I32, I64, U8, U16, U32, U64};
use radix_types::numeric_width::numerus_integer_width;

/// Analyze an `incipit` whose last local is `sit r ← <expr>` and return
/// that local's integer width.
fn width_of(decls: &str, expr: &str) -> Option<NumericWidth> {
    let source = format!("incipit {{\n{decls}\nsit r ← {expr}\n}}\n");
    let result = test_support::analyze_ok_la(&source);
    let tys = entry_local_tys(&result);
    let ty = *tys.last().expect("locals");
    numerus_integer_width(&result.types, ty)
}

const U8S: &str = "sit u8 a ← 200\nsit u8 b ← 100";
const I8S: &str = "sit i8 a ← 100\nsit i8 b ← 100";

#[test]
fn u8_table_types() {
    assert_eq!(width_of(U8S, "a + b"), Some(U16));
    assert_eq!(width_of(U8S, "a * b"), Some(U16));
    assert_eq!(width_of(U8S, "a - b"), Some(I16));
    assert_eq!(width_of(U8S, "a / b"), Some(U8));
    assert_eq!(width_of(U8S, "a % b"), Some(U8));
    assert_eq!(width_of(U8S, "-a"), Some(I16));
    assert_eq!(width_of(U8S, "¬a"), Some(I16));
    assert_eq!(width_of(U8S, "a ∧ b"), Some(U8));
    assert_eq!(width_of(U8S, "a ∨ b"), Some(U8));
    assert_eq!(width_of(U8S, "a ⊻ b"), Some(U8));
    assert_eq!(width_of(U8S, "a ⇒ 2"), Some(U8));
    assert_eq!(width_of(U8S, "a ⇐ 4"), Some(U16));
}

#[test]
fn constants_take_part_by_value() {
    assert_eq!(width_of(U8S, "a + 1"), Some(U16));
    assert_eq!(width_of(U8S, "a ∧ 255"), Some(U8));
    assert_eq!(width_of(U8S, "a % 10"), Some(U8));
    assert_eq!(width_of(U8S, "1000 - a"), Some(U16));
    assert_eq!(width_of(U8S, "a - 1000"), Some(I16));
    assert_eq!(width_of(I8S, "a + 1"), Some(I16));
}

#[test]
fn mixed_widths_join_by_range_not_by_identity() {
    let decls = "sit u8 a ← 1\nsit i16 b ← 1";
    assert_eq!(width_of(decls, "a + b"), Some(I32));
    let decls = "sit i32 a ← 1\nsit i64 b ← 1";
    assert_eq!(width_of(decls, "a + b"), Some(I64));
}

#[test]
fn sixty_four_bit_cap_keeps_the_sign() {
    let decls = "sit u64 a ← 1\nsit u64 b ← 1";
    assert_eq!(width_of(decls, "a - b"), Some(I64));
    assert_eq!(width_of(decls, "a + b"), Some(U64));
    assert_eq!(width_of(decls, "a * b"), Some(U64));
    assert_eq!(width_of(decls, "a + 1"), Some(U64));
    assert_eq!(width_of(decls, "a - 1"), Some(I64));
    let decls = "sit u32 a ← 1\nsit u32 b ← 1";
    assert_eq!(width_of(decls, "a - b"), Some(I64));
}

#[test]
fn untyped_constants_default_to_int() {
    let ten = width_of("", "10 - 100");
    assert_eq!(ten, Some(I64));
    let result = test_support::analyze_ok_la("incipit {\nsit d ← 10 - 100\n}\n");
    let ty = entry_local_tys(&result)[0];
    assert!(matches!(
        result.types.get(ty),
        Type::Primitive(Primitive::Numerus)
    ));
    assert_eq!(width_of("", "7 / 2"), Some(I64));
    assert_eq!(width_of("", "1 ⇐ 4"), Some(I64));
}

#[test]
fn integer_constant_slot_takes_its_type() {
    let result = test_support::analyze_ok_la("incipit {\nsit u8 d ← 10 + 20\n}\n");
    let ty = entry_local_tys(&result)[0];
    assert_eq!(numerus_integer_width(&result.types, ty), Some(U8));
}

#[test]
fn collection_literal_of_constants_is_lista_of_int() {
    let result = test_support::analyze_ok_la("incipit {\nsit xs ← [1, 2, 3]\n}\n");
    let ty = entry_local_tys(&result)[0];
    let (Type::Array(elem) | Type::BoundedArray { element: elem, .. }) = result.types.get(ty)
    else {
        panic!("expected lista, got {:?}", result.types.get(ty));
    };
    assert!(matches!(
        result.types.get(*elem),
        Type::Primitive(Primitive::Numerus)
    ));
}

#[test]
fn branch_join_is_the_smallest_type_holding_both() {
    let decls = "sit u8 a ← 1\nsit i8 b ← 1\nsit bivalens c ← verum";
    assert_eq!(width_of(decls, "c ✓ a ✗ b"), Some(I16));
}

#[test]
fn float_beside_integer_constant_accepts() {
    test_support::assert_analysis_accepts_la("functio f(f64 x) → f64 {\n    redde x + 1\n}\n");
    test_support::assert_analysis_accepts_la("functio f(f32 x) → f32 {\n    redde 2 * x\n}\n");
}

#[test]
fn inexact_integer_constant_beside_float_rejects() {
    test_support::assert_analysis_rejects_with_issue_la(
        "functio f(f64 x) → f64 {\n    redde x + 9007199254740993\n}\n",
        "float_constant_inexact",
    );
}

#[test]
fn typed_integer_beside_float_rejects() {
    test_support::assert_analysis_rejects_with_issue_la(
        "functio f(f64 x, i64 n) → f64 {\n    redde x + n\n}\n",
        "numeric_family_crossing_requires_conversio",
    );
    test_support::assert_analysis_rejects_with_issue_la(
        "functio f(i32 n, f32 x) → f32 {\n    redde n * x\n}\n",
        "numeric_family_crossing_requires_conversio",
    );
}

#[test]
fn float_constant_beside_integer_rejects() {
    test_support::assert_analysis_rejects_with_issue_la(
        "functio f(i32 n) → numerus {\n    redde n + 1.5\n}\n",
        "numeric_family_crossing_requires_conversio",
    );
}

#[test]
fn decimal_beside_integer_rejects() {
    test_support::assert_analysis_rejects_with_issue_la(
        "functio f(d64 d, i32 n) {\n    sit r ← d + n\n}\n",
        "numeric_family_crossing_requires_conversio",
    );
}

#[test]
fn float_and_decimal_widths_join_by_the_lattice() {
    test_support::assert_analysis_accepts_la(
        "functio f(f32 x, f64 y) → f64 {\n    redde x + y\n}\n",
    );
    test_support::assert_analysis_accepts_la(
        "functio f(d64 x, d64 y) → d64 {\n    redde x + y\n}\n",
    );
}

#[test]
fn u64_with_signed_typed_operand_rejects() {
    for op in ["+", "-", "*", "/", "%"] {
        test_support::assert_analysis_rejects_with_issue_la(
            &format!("functio f(u64 a, i32 b) {{\n    sit r ← a {op} b\n}}\n"),
            "u64_signed_arithmetic_requires_conversion",
        );
    }
    test_support::assert_analysis_rejects_with_issue_la(
        "functio f(i64 a, u64 b) {\n    sit r ← a + b\n}\n",
        "u64_signed_arithmetic_requires_conversion",
    );
}

#[test]
fn u64_with_signed_bitwise_operand_rejects() {
    for op in ["∧", "∨", "⊻"] {
        test_support::assert_analysis_rejects_with_issue_la(
            &format!("functio f(u64 a, i32 b) {{\n    sit r ← a {op} b\n}}\n"),
            "u64_signed_arithmetic_requires_conversion",
        );
    }
}

#[test]
fn u64_with_constants_is_not_an_error() {
    test_support::assert_analysis_accepts_la(
        "functio f(u64 a) {\n    sit r ← a - 1\n    sit s ← a + -5\n}\n",
    );
}

#[test]
fn u64_range_endpoint_beside_signed_typed_endpoint_rejects() {
    for (a, b) in [("u64 a", "numerus b"), ("i32 a", "u64 b")] {
        test_support::assert_analysis_rejects_with_issue_la(
            &format!("functio f({a}, {b}) {{\n    sit r ← a‥b\n}}\n"),
            "u64_signed_arithmetic_requires_conversion",
        );
    }
    test_support::assert_analysis_accepts_la("functio f(u64 a) {\n    sit r ← 0‥a\n}\n");
}

#[test]
fn modular_and_saturating_words_keep_their_ring() {
    let decls = "sit modulus<u8> a ← 200\nsit modulus<u8> b ← 100";
    let source = format!("incipit {{\n{decls}\nsit r ← a + b\n}}\n");
    let result = test_support::analyze_ok_la(&source);
    let ty = *entry_local_tys(&result).last().unwrap();
    assert!(matches!(result.types.get(ty), Type::ModularWord(U8)));
    let decls = "sit saturatus<u8> a ← 200\nsit saturatus<u8> b ← 100";
    let source = format!("incipit {{\n{decls}\nsit r ← a + b\n}}\n");
    let result = test_support::analyze_ok_la(&source);
    let ty = *entry_local_tys(&result).last().unwrap();
    assert!(matches!(result.types.get(ty), Type::SaturatingWord(U8)));
}

#[test]
fn signed_negation_grows() {
    assert_eq!(width_of(I8S, "-a"), Some(I16));
    let decls = "sit i64 a ← 1";
    assert_eq!(width_of(decls, "-a"), Some(I64));
    assert_eq!(width_of(U8S, "a + b + a"), Some(U32));
}

#[test]
fn integer_widths_beyond_i8_are_reachable() {
    let decls = "sit i8 a ← 1\nsit i8 b ← 1";
    assert_eq!(width_of(decls, "a * b"), Some(I16));
    assert_eq!(width_of(decls, "a / b"), Some(I16));
    assert_eq!(width_of("", "-1"), Some(I64));
    assert_ne!(I8, U8);
}

#[test]
fn strict_equality_takes_an_integer_constant_by_value() {
    test_support::assert_analysis_accepts_la(
        "functio even(numerus n) → bivalens {\n    redde n % 2 ≡ 0\n}\n",
    );
    test_support::assert_analysis_accepts_la(
        "functio p(u8 a) → bivalens {\n    redde a ≡ 200\n}\n",
    );
    // A constant the width cannot hold compares exactly and is simply false
    // (ruling 22); it is not a type error.
    test_support::assert_analysis_accepts_la(
        "functio p(u8 a) → bivalens {\n    redde a ≡ 300\n}\n",
    );
    // Typed operands of different widths stay exact-type only.
    test_support::assert_analysis_rejects_with_issue_la(
        "functio p(u8 a, u16 b) → bivalens {\n    redde a ≡ b\n}\n",
        "equality_exact_types_required",
    );
}

#[test]
fn range_endpoints_join_like_a_binary_pair() {
    // An untyped constant joins the typed endpoint by value (ruling 12).
    for source in [
        "functio p(u8 x) {\n    itera ab 0‥(x + x - 398) fixum i {\n        nota i\n    }\n}\n",
        "functio p(u8 x) {\n    itera ab 0‥x fixum i {\n        nota i\n    }\n}\n",
        "functio p(u8 x) {\n    itera ab (x + x - 398)‥3 fixum i {\n        nota i\n    }\n}\n",
        "functio p(u8 x) {\n    itera ab x‥(x + x) fixum i {\n        nota i\n    }\n}\n",
    ] {
        test_support::assert_analysis_accepts_la(source);
    }
    // A family crossing is still an error.
    test_support::assert_analysis_rejects_with_issue_la(
        "functio p(u8 x, fractus f) {\n    itera ab 0‥f fixum i {\n        nota i\n    }\n}\n",
        "equality_exact_types_required",
    );
}
