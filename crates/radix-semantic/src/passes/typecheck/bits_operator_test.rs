//! Typecheck pins for expressions over the raw-bits markers
//! `b8 b16 b32 b64` (need, unit 1.3a): equality, unsigned ordering
//! and the bit operations are accepted; arithmetic, mixing with numbers or
//! another width, and out-of-range literals are named errors.

use crate::test_support;

fn accepts(source: &str) {
    test_support::assert_accepts_la(source);
}

fn rejects(source: &str, issue: &str) {
    test_support::assert_rejects_with_issue_la(source, issue);
}

fn rejects_any(source: &str) {
    let result = test_support::analyze_source_la(source);
    assert!(
        !result.is_ok(),
        "expected failure for {source:?}, got: {:?}",
        result.diagnostics
    );
}

// --- accepted: bit operations, equality, ordering ---

#[test]
fn bits_and_or_xor_accept_same_width() {
    accepts("functio f(b8 a, b8 b) → b8 { redde a ∧ b }");
    accepts("functio f(b16 a, b16 b) → b16 { redde a ∨ b }");
    accepts("functio f(b32 a, b32 b) → b32 { redde a ⊻ b }");
    accepts("functio f(b64 a, b64 b) → b64 { redde a ∧ b }");
}

#[test]
fn bits_not_keeps_the_width() {
    accepts("functio f(b8 a) → b8 { redde ¬ a }");
    accepts("functio f(b64 a) → b64 { redde ¬ a }");
}

#[test]
fn bits_shift_takes_an_integer_count_and_keeps_the_width() {
    accepts("functio f(b8 a) → b8 { redde a ⇐ 3 }");
    accepts("functio f(b8 a) → b8 { redde a ⇒ 3 }");
    accepts("functio f(b16 a, u8 n) → b16 { redde a ⇐ n }");
    accepts("functio f(b32 a, numerus n) → b32 { redde a ⇒ n }");
}

#[test]
fn bits_equality_and_ordering_give_bool() {
    accepts("functio f(b8 a, b8 b) → bivalens { redde a ≡ b }");
    accepts("functio f(b8 a, b8 b) → bivalens { redde a ≢ b }");
    accepts("functio f(b8 a, b8 b) → bivalens { redde a ≺ b }");
    accepts("functio f(b16 a, b16 b) → bivalens { redde a ≤ b }");
    accepts("functio f(b32 a, b32 b) → bivalens { redde a ≻ b }");
    accepts("functio f(b64 a, b64 b) → bivalens { redde a ≥ b }");
}

// --- accepted: literals take the cell's type (B3) ---

#[test]
fn integer_literal_in_a_bits_slot_takes_the_bits_type() {
    accepts("incipit { sit b8 w ← 0xff }");
    accepts("incipit { sit b16 v ← 0xffff }");
    accepts("incipit { sit b32 v ← 0xffffffff }");
    accepts("incipit { sit b64 v ← 0xffffffffffffffff }");
    accepts("incipit { sit b8 z ← 0 }");
    accepts("incipit { sit b8 z ← 0b1010 }");
}

#[test]
fn integer_literal_beside_a_bits_operand_takes_the_bits_type() {
    accepts("functio f(b8 a) → bivalens { redde a ≡ 0x7f }");
    accepts("functio f(b8 a) → bivalens { redde 0x7f ≡ a }");
    accepts("functio f(b8 a) → bivalens { redde a ≥ 0x80 }");
    accepts("functio f(b8 a) → b8 { redde a ∧ 0x0f }");
    accepts("functio f(b8 a) → b8 { redde 0xf0 ∨ a }");
    accepts("incipit { sit b8 m ← 0x0f\n sit b8 n ← ¬ m }");
}

// --- refused: arithmetic (3) ---

#[test]
fn bits_arithmetic_is_refused() {
    for op in ["+", "-", "*", "/", "%", "÷"] {
        rejects(
            &format!("functio f(b8 a, b8 b) → b8 {{ redde a {op} b }}"),
            "bits_arithmetic_refused",
        );
    }
    rejects(
        "functio f(b8 a) → b8 { redde a + 1 }",
        "bits_arithmetic_refused",
    );
}

#[test]
fn bits_unary_minus_is_refused() {
    rejects(
        "functio f(b8 a) → b8 { redde -a }",
        "bits_arithmetic_refused",
    );
}

// --- refused: mixing (4) ---

#[test]
fn bits_mixed_with_a_number_is_refused() {
    rejects(
        "functio f(b8 a, u8 b) → b8 { redde a ∧ b }",
        "bits_operand_mismatch",
    );
    rejects(
        "functio f(b8 a, u8 b) → bivalens { redde a ≡ b }",
        "bits_operand_mismatch",
    );
    rejects(
        "functio f(b8 a, u8 b) → bivalens { redde a ≺ b }",
        "bits_operand_mismatch",
    );
}

#[test]
fn bits_mixed_with_another_width_is_refused() {
    rejects(
        "functio f(b8 a, b16 b) → b8 { redde a ∧ b }",
        "bits_operand_mismatch",
    );
    rejects(
        "functio f(b8 a, b16 b) → bivalens { redde a ≡ b }",
        "bits_operand_mismatch",
    );
}

#[test]
fn bits_shift_count_is_an_integer_not_bits() {
    rejects(
        "functio f(b8 a, b8 n) → b8 { redde a ⇐ n }",
        "bits_operand_mismatch",
    );
    rejects(
        "functio f(u8 a, b8 n) → u8 { redde a ⇐ n }",
        "bits_operand_mismatch",
    );
}

#[test]
fn negative_literal_shift_count_is_refused_by_name() {
    rejects(
        "functio f(b8 a) → b8 { redde a ⇐ -1 }",
        "shift_count_negative",
    );
}

#[test]
fn bits_is_not_a_number() {
    // B1: a bN is not assignable where a number is expected; `↦` comes
    // with the conversion unit.
    rejects_any("functio f(b8 a) → u8 { redde a }");
    rejects_any("incipit { sit b8 a ← 0x01\n sit u8 z ← a }");
    rejects_any("functio f(u8 a) → b8 { redde a }");
}

// --- refused: literals (5) ---

#[test]
fn literal_wider_than_the_bits_slot_is_refused() {
    rejects("incipit { sit b8 x ← 0x1ff }", "bits_literal_out_of_range");
    rejects("incipit { sit b8 x ← 256 }", "bits_literal_out_of_range");
    rejects(
        "incipit { sit b16 x ← 0x10000 }",
        "bits_literal_out_of_range",
    );
    rejects(
        "incipit { sit b32 x ← 0x100000000 }",
        "bits_literal_out_of_range",
    );
}

/// The named refusal is the only error for the fault: the refused literal
/// takes the cell's type, so no store mismatch follows it.
#[test]
fn refused_bits_literal_reports_once_without_cascade() {
    for source in [
        "incipit { sit b8 x ← 0x1ff }",
        "incipit { sit b16 x ← 0x1ffff }",
        "incipit { sit b8 x ← -1 }",
    ] {
        let issues: Vec<String> = test_support::analyze_source_la(source)
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .filter_map(|d| {
                d.args
                    .iter()
                    .find(|arg| arg.name == "issue")
                    .map(|arg| arg.value.clone())
            })
            .collect();
        assert_eq!(
            issues,
            vec!["bits_literal_out_of_range".to_string()],
            "{source:?}"
        );
    }
}

#[test]
fn negative_literal_in_a_bits_slot_is_refused() {
    rejects("incipit { sit b8 x ← -1 }", "bits_literal_out_of_range");
}

#[test]
fn oversized_literal_beside_a_bits_operand_is_refused() {
    rejects(
        "functio f(b8 a) → bivalens { redde a ≡ 0x1ff }",
        "bits_literal_out_of_range",
    );
}

#[test]
fn number_expression_in_a_bits_slot_is_refused() {
    // `3 ⇐ 2` is a number shift; only a written literal takes the cell type.
    rejects_any("incipit { sit b8 y ← 3 ⇐ 2 }");
}

// --- A5 regression: number types are unchanged ---

#[test]
fn number_shifts_and_arithmetic_still_check() {
    accepts("functio f(u8 a) → u8 { redde a ⇐ 3 }");
    accepts("functio f(u8 a) → u8 { redde a ⇒ 3 }");
    accepts("functio f(u8 a, u8 b) → u8 { redde a ∧ b }");
    accepts("functio f(u8 a, u8 b) → u8 { redde a + b }");
    accepts("functio f(u8 a, u8 b) → bivalens { redde a ≺ b }");
    accepts("functio f(u8 a) → u8 { redde ¬ a }");
    accepts("functio f(u8 a) → u8 { redde a ∧ 0x0f }");
    accepts("incipit { sit u8 x ← 0xff }");
}

#[test]
fn number_literal_overflow_diagnostics_are_unchanged() {
    rejects_any("incipit { sit u8 x ← 0x1ff }");
}
