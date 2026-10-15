//! Bidirectional literal-width inference: comprehensive edge-case coverage.
//!
//! These tests cover every expression surface where a bare numeric literal
//! must infer its width from context. The test suite was written to expose
//! and pin the `check_unary` expected-type propagation bug (negative literals
//! and bitwise-not dropped the expected type) and its cascade through binary
//! arithmetic and collection literals.
//!
//! All tests use the analysis path (parse → typecheck, no codegen) for speed.

use crate::test_support;

// ===========================================================================
// Section 1: Positive controls — confirm baseline inference works
//
// These must pass before and after the unary-negation fix. They prove the
// bidirectional inference mechanism is sound for positive literals.
// ===========================================================================

#[test]
fn pos_f32_local_init() {
    test_support::assert_analysis_accepts(
        r"
functio f() → f32 {
    sit f32 x ← 1.0
    redde x
}
",
    );
}

#[test]
fn pos_f32_return() {
    test_support::assert_analysis_accepts(
        r"
functio f() → f32 {
    redde 1.0
}
",
    );
}

#[test]
fn pos_f32_call_arg() {
    test_support::assert_analysis_accepts(
        r"
functio g(f32 x) → f32 { redde x }
functio f() → f32 {
    redde g(1.0)
}
",
    );
}

#[test]
fn pos_f32_field_assign() {
    test_support::assert_analysis_accepts(
        r"
genus Vec { varia f32 x }
functio f() → Vec {
    redde Vec { x = 1.0 }
}
",
    );
}

#[test]
fn pos_f32_arithmetic_rhs() {
    test_support::assert_analysis_accepts(
        r"
functio f(f32 x) → f32 {
    redde x + 1.0
}
",
    );
}

#[test]
fn pos_f32_lista_literal() {
    test_support::assert_analysis_accepts(
        r"
functio f() → lista<f32> {
    redde [1.0, 2.0, 3.0]
}
",
    );
}

#[test]
fn pos_i32_local_init() {
    test_support::assert_analysis_accepts(
        r"
functio f() → i32 {
    sit i32 x ← 1
    redde x
}
",
    );
}

#[test]
fn pos_i32_return() {
    test_support::assert_analysis_accepts(
        r"
functio f() → i32 {
    redde 1
}
",
    );
}

// ===========================================================================
// Section 2: Negative float literals — the confirmed bug
//
// RED before fix: -1.0 desugars to Unary(Neg, Lit(1.0)). check_unary drops
// the expected type, the literal defaults to f64, unification fails.
// GREEN after fix: expected type propagates through Neg, literal infers f32.
// ===========================================================================

#[test]
fn neg_f32_local_init() {
    test_support::assert_analysis_accepts(
        r"
functio f() → f32 {
    sit f32 x ← -1.0
    redde x
}
",
    );
}

#[test]
fn neg_f32_return() {
    test_support::assert_analysis_accepts(
        r"
functio f() → f32 {
    redde -1.0
}
",
    );
}

#[test]
fn neg_f32_call_arg() {
    test_support::assert_analysis_accepts(
        r"
functio g(f32 x) → f32 { redde x }
functio f() → f32 {
    redde g(-1.0)
}
",
    );
}

#[test]
fn neg_f32_field_assign() {
    test_support::assert_analysis_accepts(
        r"
genus Vec { varia f32 x }
functio f() → Vec {
    redde Vec { x = -1.0 }
}
",
    );
}

#[test]
fn neg_f32_arithmetic_rhs() {
    test_support::assert_analysis_accepts(
        r"
functio f(f32 x) → f32 {
    redde x + -1.0
}
",
    );
}

#[test]
fn neg_f32_arithmetic_lhs() {
    test_support::assert_analysis_accepts(
        r"
functio f(f32 x) → f32 {
    redde -1.0 + x
}
",
    );
}

#[test]
fn neg_f32_lista_literal() {
    test_support::assert_analysis_accepts(
        r"
functio f() → lista<f32> {
    redde [1.0, -1.0, 3.0]
}
",
    );
}

#[test]
fn neg_f32_lista_passed_to_param() {
    test_support::assert_analysis_accepts(
        r"
functio g(lista<f32> xs) → f32 { redde xs[0] }
functio f() → f32 {
    redde g([1.0, -1.0])
}
",
    );
}

// ===========================================================================
// Section 3: Negative integer literals — symmetric bug
//
// Same mechanism: -1 desugars to Unary(Neg, Lit(1)). The integer literal
// defaults to i64, unification with i32 fails.
// ===========================================================================

#[test]
fn neg_i32_local_init() {
    test_support::assert_analysis_accepts(
        r"
functio f() → i32 {
    sit i32 x ← -1
    redde x
}
",
    );
}

#[test]
fn neg_i32_return() {
    test_support::assert_analysis_accepts(
        r"
functio f() → i32 {
    redde -1
}
",
    );
}

#[test]
fn neg_i32_call_arg() {
    test_support::assert_analysis_accepts(
        r"
functio g(i32 x) → i32 { redde x }
functio f() → i32 {
    redde g(-1)
}
",
    );
}

#[test]
fn neg_i32_arithmetic_rhs() {
    test_support::assert_analysis_accepts(
        r"
functio f(i32 x) → i64 {
    redde x + -1
}
",
    );
}

#[test]
fn neg_i32_lista_literal() {
    test_support::assert_analysis_accepts(
        r"
functio f() → lista<i32> {
    redde [1, -1, 3]
}
",
    );
}

// ===========================================================================
// Section 4: Bitwise not — same check_unary path
//
// ¬1 desugars to Unary(BitNot, Lit(1)). Same expected-type drop. BitNot is
// only valid on integers, but the width inference gap is identical.
// ===========================================================================

#[test]
fn bitnot_i32_local_init() {
    test_support::assert_analysis_accepts(
        r"
functio f() → i32 {
    sit i32 x ← ¬1
    redde x
}
",
    );
}

#[test]
fn bitnot_i32_call_arg() {
    test_support::assert_analysis_accepts(
        r"
functio g(i32 x) → i32 { redde x }
functio f() → i32 {
    redde g(¬1)
}
",
    );
}

// ===========================================================================
// Section 5: f16 inference — pin behavior
//
// f16 has zero literal-inference coverage in the existing suite. These tests
// confirm the inference mechanism handles the third float width.
// ===========================================================================

#[test]
fn pos_f16_local_init() {
    test_support::assert_analysis_accepts(
        r"
functio f() → f16 {
    sit f16 x ← 1.0
    redde x
}
",
    );
}

#[test]
fn neg_f16_local_init() {
    test_support::assert_analysis_accepts(
        r"
functio f() → f16 {
    sit f16 x ← -1.0
    redde x
}
",
    );
}

// ===========================================================================
// Section 6: Coalesce fallback — literal inference through vel
//
// The vel path was recently fixed (commit 86a980629) to propagate expected
// type to numeric literals. These pin that behavior for negative literals.
// ===========================================================================

#[test]
fn neg_f32_coalesce_fallback() {
    test_support::assert_analysis_accepts(
        r"
functio f(f32 ∪ nihil x) → f32 {
    redde x vel -1.0
}
",
    );
}

// ===========================================================================
// Section 7: Sad paths — legitimate rejections that must survive the fix
//
// These must fail before AND after the fix. They guard against the fix
// accidentally widening inference to accept type-family violations.
// ===========================================================================

#[test]
fn neg_float_to_textus_rejects() {
    test_support::assert_analysis_rejects_with_issue(
        r"
functio f() → textus {
    redde -1.0
}
",
        "return_type_mismatch",
    );
}

#[test]
fn float_to_i32_family_mismatch_rejects() {
    test_support::assert_analysis_rejects_with_issue(
        r"
functio f() → i32 {
    redde 1.0
}
",
        "return_type_mismatch",
    );
}

#[test]
fn neg_float_to_bivalens_rejects() {
    test_support::assert_analysis_rejects_with_issue(
        r"
functio f() → bivalens {
    redde -1.0
}
",
        "return_type_mismatch",
    );
}

// ===========================================================================
// Section 8: Default-width behavior — no context means f64/i64
//
// When there is no constraining context, a bare literal must default to
// the family default width. These confirm the default and ensure the fix
// doesn't change defaulting behavior.
// ===========================================================================

#[test]
fn bare_float_literal_defaults_to_f64() {
    test_support::assert_analysis_accepts(
        r"
functio f() → f64 {
    redde -1.0
}
",
    );
}

#[test]
fn bare_integer_literal_defaults_to_i64() {
    test_support::assert_analysis_accepts(
        r"
functio f() → i64 {
    redde -1
}
",
    );
}
