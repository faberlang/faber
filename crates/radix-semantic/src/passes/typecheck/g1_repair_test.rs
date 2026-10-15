//! d11-6 G1 repair: defects the G1 audit found in the typecheck half of
//! rulings 8, 10, 12, 16, 17, 20, 21 and 22. Each test failed before its fix.

use super::test_support::{self, analyze_ok_la, assert_accepts_la, assert_rejects_with_issue_la};
use crate::types::Type;
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{
    HirConversioTarget, HirExpression, HirExpressionKind, HirLiteral, HirStoreKind, HirStoreSite,
};
use radix_types::{NumericWidth, Primitive};

const OUT_OF_RANGE: &str = "float_constant_out_of_range";
const INEXACT: &str = "float_constant_inexact";
const U64_SIGNED: &str = "u64_signed_arithmetic_requires_conversion";

/// Implicit store nodes and integer literal types found in analyzed HIR.
#[derive(Default)]
struct Probe {
    stores: Vec<HirStoreSite>,
    literals: Vec<(u64, radix_types::TypeId)>,
}

impl HirVisitor for Probe {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Conversio {
            target: HirConversioTarget::Type(_),
            implicit: Some(site),
            ..
        } = &expr.kind
        {
            self.stores.push(site.clone());
        }
        if let (HirExpressionKind::Literal(HirLiteral::Int(v)), Some(ty)) = (&expr.kind, expr.ty) {
            self.literals.push((*v, ty));
        }
        walk_expr(self, expr);
    }
}

fn probe(source: &str) -> (Probe, test_support::AnalyzedFixture) {
    let result = analyze_ok_la(source);
    let mut probe = Probe::default();
    walk_program(&mut probe, result.hir.as_ref().expect("hir"));
    (probe, result)
}

fn kinds(source: &str) -> Vec<HirStoreKind> {
    probe(source).0.stores.iter().map(|s| s.kind).collect()
}

// ---------------------------------------------------------------------------
// 1. Float constants outside the finite range (ruling 20(2))
// ---------------------------------------------------------------------------

#[test]
fn float_literal_beside_a_float_outside_the_finite_range_is_rejected() {
    for (ty, literal) in [("f32", "1e40"), ("f16", "70000.0"), ("bf16", "1e39")] {
        let source = format!("incipit {{\nsit {ty} x ← 1.5\nnota x + {literal}\n}}\n");
        assert_rejects_with_issue_la(&source, OUT_OF_RANGE);
        let slot = format!("incipit {{\nsit {ty} y ← {literal}\nnota y\n}}\n");
        assert_rejects_with_issue_la(&slot, OUT_OF_RANGE);
    }
}

#[test]
fn integer_constant_outside_a_float_range_is_rejected() {
    assert_rejects_with_issue_la(
        "incipit {\nsit f16 x ← 1.5\nnota x + 70000\n}\n",
        OUT_OF_RANGE,
    );
    assert_rejects_with_issue_la("incipit {\nsit f16 y ← 65536\nnota y\n}\n", OUT_OF_RANGE);
}

#[test]
fn float_constants_inside_the_range_are_accepted() {
    assert_accepts_la("incipit {\nsit f32 x ← 1.5\nnota x + 3.0e38\n}\n");
    assert_accepts_la("incipit {\nsit f16 y ← 65504.0\nnota y\n}\n");
    assert_accepts_la("incipit {\nsit f64 x ← 1.5\nnota x + 1e300\n}\n");
    assert_accepts_la("incipit {\nsit f32 y ← -∞\nnota y\n}\n");
}

// ---------------------------------------------------------------------------
// 5. Exactness is representability, not a magnitude limit (ruling 20(1))
// ---------------------------------------------------------------------------

#[test]
fn exactly_representable_large_integer_constants_are_accepted() {
    assert_accepts_la("incipit {\nsit f64 y ← 18014398509481984\nnota y\n}\n");
    assert_accepts_la("incipit {\nsit f64 x ← 1.5\nnota x * (1 ⇐ 60)\n}\n");
    assert_accepts_la("incipit {\nsit f32 y ← 1073741824\nnota y\n}\n");
    assert_accepts_la("incipit {\nsit bf16 y ← 1024\nnota y\n}\n");
}

#[test]
fn inexact_integer_constants_are_still_rejected() {
    assert_rejects_with_issue_la(
        "incipit {\nsit f64 y ← 9007199254740993\nnota y\n}\n",
        INEXACT,
    );
    assert_rejects_with_issue_la("incipit {\nsit f32 y ← 16777217\nnota y\n}\n", INEXACT);
    assert_rejects_with_issue_la(
        "incipit {\nsit f64 x ← 1.5\nnota x * ((1 ⇐ 60) + 1)\n}\n",
        INEXACT,
    );
}

// ---------------------------------------------------------------------------
// 2. Calls and conditionals into a narrower slot get the store node
// ---------------------------------------------------------------------------

const BIG: &str = "functio big() → numerus { redde 300 }\n";

#[test]
fn call_result_into_a_narrower_slot_is_a_checked_store() {
    let decl = format!("{BIG}incipit {{\nsit u8 x ← big()\nnota x\n}}\n");
    assert_eq!(kinds(&decl), vec![HirStoreKind::Declaration]);
    let assign = format!("{BIG}incipit {{\nvaria u8 x ← 1\nx ← big()\nnota x\n}}\n");
    assert_eq!(kinds(&assign), vec![HirStoreKind::Assignment]);
    let arg = format!(
        "{BIG}functio take(u8 v) → vacuum {{\n    nota v\n}}\nincipit {{\ntake(big())\n}}\n"
    );
    assert_eq!(kinds(&arg), vec![HirStoreKind::Argument]);
}

#[test]
fn method_result_into_a_narrower_slot_is_a_checked_store() {
    let source = "genus Q {\n    functio m() → numerus {\n        redde 300\n    }\n}\n\
                  functio take(u8 v) → vacuum {\n    nota v\n}\n\
                  functio go(Q q) → vacuum {\n    sit u8 x ← q.m()\n    take(q.m())\n    nota x\n}\n";
    assert_eq!(
        kinds(source),
        vec![HirStoreKind::Declaration, HirStoreKind::Argument]
    );
}

#[test]
fn call_as_a_parameter_default_is_a_checked_store() {
    let source = format!("{BIG}functio f(u8 v vel big()) → u8 {{\n    redde v\n}}\n");
    assert_eq!(kinds(&source), vec![HirStoreKind::ParameterDefault]);
}

#[test]
fn conditional_into_a_narrower_slot_is_a_checked_store() {
    let mixed = "incipit {\nsit numerus n ← 300\nsit bivalens c ← verum\n\
                 sit u8 x ← c ✓ n ✗ 1\nnota x\n}\n";
    assert_eq!(kinds(mixed), vec![HirStoreKind::Declaration]);
    let branches = "incipit {\nsit u8 a ← 200\nsit u16 b ← 300\n\
                    sit bivalens c ← falsum\nsit u8 x ← c ✓ a ✗ b\nnota x\n}\n";
    assert_eq!(kinds(branches), vec![HirStoreKind::Declaration]);
}

#[test]
fn conditional_that_fits_the_slot_needs_no_node() {
    let source = "incipit {\nsit u8 a ← 200\nsit u8 b ← 3\n\
                  sit bivalens c ← falsum\nsit u8 x ← c ✓ a ✗ b\nnota x\n}\n";
    assert!(kinds(source).is_empty());
}

// ---------------------------------------------------------------------------
// 3. Constant-only expressions are computed exactly, then shaped to the slot
// ---------------------------------------------------------------------------

#[test]
fn constant_expressions_take_a_word_slot_after_exact_evaluation() {
    for source in [
        "incipit {\nsit saturatus<u8> z ← 0 - 1 + 2\nnota z\n}\n",
        "incipit {\nsit saturatus<u8> z ← 300 - 100\nnota z\n}\n",
        "incipit {\nsit saturatus<u8> z ← 250 * 2 / 2\nnota z\n}\n",
        "incipit {\nsit modulus<u8> u ← 300 - 50\nnota u\n}\n",
        "incipit {\nsit saturatus<i8> s ← 3 - 10\nnota s\n}\n",
    ] {
        assert_accepts_la(source);
    }
}

#[test]
fn folded_word_constant_carries_the_exact_value_and_the_slot_type() {
    let (probe, result) = probe("incipit {\nsit saturatus<u8> z ← 300 - 100\nnota z\n}\n");
    assert!(probe.stores.is_empty());
    let (value, ty) = probe.literals[0];
    assert_eq!(value, 200);
    assert!(matches!(
        result.types.get(ty),
        Type::SaturatingWord(NumericWidth::U8)
    ));
}

#[test]
fn constant_expression_that_overflows_a_word_slot_follows_its_policy() {
    // 300 does not fit `saturatus<u8>`: the exact value clamps at the store.
    assert_eq!(
        kinds("incipit {\nsit saturatus<u8> z ← 100 + 200\nnota z\n}\n"),
        vec![HirStoreKind::Declaration]
    );
}

// ---------------------------------------------------------------------------
// 4. A constant negative shift count
// ---------------------------------------------------------------------------

#[test]
fn constant_negative_shift_count_is_a_compile_error() {
    for source in [
        "incipit {\nsit u8 a ← 1\nnota a ⇐ -1\n}\n",
        "incipit {\nsit u8 a ← 1\nnota a ⇒ -3\n}\n",
        "incipit {\nnota 1 ⇐ -1\n}\n",
    ] {
        assert_rejects_with_issue_la(source, "shift_count_negative");
    }
}

#[test]
fn typed_and_nonnegative_shift_counts_are_accepted() {
    assert_accepts_la("incipit {\nsit u8 a ← 1\nsit numerus k ← -1\nnota a ⇐ k\nnota a ⇐ 3\n}\n");
}

// ---------------------------------------------------------------------------
// 6. A constant that cannot fit compares exactly
// ---------------------------------------------------------------------------

#[test]
fn strict_equality_with_a_constant_outside_the_type_is_accepted() {
    assert_accepts_la("incipit {\nsit u8 a ← 7\nnota a ≡ 300\nnota -1 ≡ a\n}\n");
}

// ---------------------------------------------------------------------------
// 7. Ruling 16: an integer quotient into a float slot points at `÷`
// ---------------------------------------------------------------------------

#[test]
fn integer_quotient_into_a_float_slot_names_true_division() {
    assert_rejects_with_issue_la(
        "incipit {\nsit numerus a ← 7\nsit numerus b ← 2\nsit fractus avg ← a / b\nnota avg\n}\n",
        "integer_quotient_to_float_requires_true_division",
    );
    // The plain integer-into-float case keeps its own help.
    assert_rejects_with_issue_la(
        "incipit {\nsit numerus a ← 7\nsit fractus f ← a\nnota f\n}\n",
        "integer_to_float_requires_conversio",
    );
}

// ---------------------------------------------------------------------------
// 8. Constant arguments carry the parameter type; i64::MIN is a literal
// ---------------------------------------------------------------------------

#[test]
fn constant_argument_carries_the_parameter_type() {
    let source = "functio f(u8 v) → u8 { redde v }\nincipit {\nnota f(200)\n}\n";
    let (probe, result) = probe(source);
    assert!(probe.stores.is_empty());
    let (value, ty) = probe
        .literals
        .iter()
        .copied()
        .find(|(v, _)| *v == 200)
        .unwrap();
    assert_eq!(value, 200);
    assert!(matches!(
        result.types.get(ty),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8)
    ));
}

#[test]
fn the_smallest_int_is_a_valid_constant() {
    assert_accepts_la("incipit {\nsit numerus a ← -9223372036854775808\nnota a\n}\n");
    assert_accepts_la("incipit {\nsit i64 a ← -9223372036854775808\nnota a\n}\n");
    assert_rejects_with_issue_la(
        "incipit {\nsit i32 a ← -9223372036854775808\nnota a\n}\n",
        "store_constant_out_of_range",
    );
    assert_rejects_with_issue_la(
        "incipit {\nsit numerus a ← -9223372036854775809\nnota a\n}\n",
        "numerus_literal_out_of_range",
    );
}

// ---------------------------------------------------------------------------
// 9. Ruling 8 extended: a typed u64 and a typed signed value never join
// ---------------------------------------------------------------------------

const U64_AND_I8: &str = "sit u64 a ← 18446744073709551615\n\
                          sit i8 b ← -1\nsit bivalens c ← verum\n";

fn with_mix(body: &str) -> String {
    format!("incipit {{\n{U64_AND_I8}{body}\n}}\n")
}

#[test]
fn u64_with_a_signed_type_does_not_join_in_a_conditional() {
    assert_rejects_with_issue_la(&with_mix("sit x ← c ✓ a ✗ b\nnota x"), U64_SIGNED);
}

#[test]
fn u64_with_a_signed_type_does_not_join_in_bitwise_operators() {
    for op in ["∧", "∨", "⊻"] {
        assert_rejects_with_issue_la(&with_mix(&format!("sit x ← a {op} b\nnota x")), U64_SIGNED);
    }
    assert_rejects_with_issue_la(&with_mix("sit i64 x ← a ∨ b\nnota x"), U64_SIGNED);
}

#[test]
fn u64_with_a_signed_type_does_not_join_in_a_collection_literal() {
    assert_rejects_with_issue_la(&with_mix("sit xs ← [a, b]\nnota xs"), U64_SIGNED);
    assert_rejects_with_issue_la(
        &with_mix("sit t ← reducta via summa ex [a, b] fixum v { redde v }\nnota t"),
        U64_SIGNED,
    );
}

#[test]
fn u64_with_an_untyped_constant_still_joins() {
    let head = "sit u64 x ← 5\nsit bivalens c ← verum\n";
    for body in [
        "nota x - 1",
        "nota c ✓ x ✗ 0",
        "nota x ∨ 1",
        "nota x ⊻ 3",
        "nota x ∧ -1",
        "sit xs ← [x, 0]\nnota xs",
        "sit xs ← [0, 1, x]\nnota xs",
        "sit y ← c ✓ 0 ✗ x\nnota y",
    ] {
        assert_accepts_la(&format!("incipit {{\n{head}{body}\n}}\n"));
    }
    // A negative constant with `u64` has no 64-bit type holding both.
    assert_rejects_with_issue_la(
        &format!("incipit {{\n{head}sit xs ← [x, -1]\nnota xs\n}}\n"),
        U64_SIGNED,
    );
}

#[test]
fn unsigned_widths_below_u64_still_join_with_signed_types() {
    assert_accepts_la(
        "incipit {\nsit u32 a ← 5\nsit i8 b ← -1\nsit bivalens c ← verum\n\
         nota c ✓ a ✗ b\nnota a ∨ b\nsit xs ← [a, b]\nnota xs\n}\n",
    );
}
