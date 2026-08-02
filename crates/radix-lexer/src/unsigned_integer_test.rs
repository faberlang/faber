//! R0 red carrier contract: ordinary expression integer literals must scan
//! with an unsigned-magnitude `u64` payload so magnitudes from `i64::MAX + 1`
//! through `u64::MAX` survive lexing. These tests compile against the current
//! `lex` API and fail at runtime until R1 lands the `u64` magnitude carrier.

use crate::{lex, TokenKind};

fn integer_token_count(source: &str) -> (usize, usize) {
    let result = lex(source);
    let integers = result
        .tokens
        .iter()
        .filter(|token| matches!(token.kind, TokenKind::Integer(_)))
        .count();
    (integers, result.errors.len())
}

fn first_integer_value(source: &str) -> u64 {
    let result = lex(source);
    for token in &result.tokens {
        if let TokenKind::Integer(value) = token.kind {
            return value;
        }
    }
    panic!(
        "expected at least one Integer token, got {:?}",
        result.tokens
    );
}

#[test]
fn unsigned_integer_scans_decimal_i64_max_plus_one() {
    let source = "9223372036854775808";
    let (integers, errors) = integer_token_count(source);
    assert_eq!(errors, 0, "i64::MAX + 1 must scan without lexer errors");
    assert_eq!(integers, 1, "expected one integer token");
    // 3.1e — verify actual u64 value
    assert_eq!(first_integer_value(source), 9_223_372_036_854_775_808);
}

#[test]
fn unsigned_integer_scans_decimal_u64_max() {
    let source = "18446744073709551615";
    let (integers, errors) = integer_token_count(source);
    assert_eq!(errors, 0, "u64::MAX must scan without lexer errors");
    assert_eq!(integers, 1, "expected one integer token");
    // 3.1e — verify actual u64 value
    assert_eq!(first_integer_value(source), 18_446_744_073_709_551_615);
}

#[test]
fn unsigned_integer_scans_radix_prefixed_u64_max() {
    let cases: &[(&str, u64)] = &[
        ("0xffffffffffffffff", 18_446_744_073_709_551_615),
        (
            "0b1111111111111111111111111111111111111111111111111111111111111111",
            18_446_744_073_709_551_615,
        ),
        ("0o1777777777777777777777", 18_446_744_073_709_551_615),
    ];
    for (source, expected) in cases {
        let (integers, errors) = integer_token_count(source);
        assert_eq!(errors, 0, "{source} must scan without lexer errors");
        assert_eq!(integers, 1, "{source}: expected one integer token");
        // 3.1e — verify actual u64 value
        assert_eq!(
            first_integer_value(source),
            *expected,
            "{source}: value mismatch"
        );
    }
}

#[test]
fn unsigned_integer_scans_signed_range_boundary_unchanged() {
    // i64::MAX stays scannable today; this row is the regression floor for the
    // R1 carrier migration.
    let source = "9223372036854775807";
    let (integers, errors) = integer_token_count(source);
    assert_eq!(errors, 0, "i64::MAX must keep scanning");
    assert_eq!(integers, 1, "expected one integer token");
    // 3.1e — verify actual u64 value
    assert_eq!(first_integer_value(source), 9_223_372_036_854_775_807);
}

#[test]
fn unsigned_integer_rejects_above_u64_max() {
    let result = lex("18446744073709551616");
    assert!(
        !result.errors.is_empty(),
        "u64::MAX + 1 must remain a lexer error after the u64 carrier lands"
    );
}

#[test]
fn unsigned_integer_scans_zero() {
    let source = "0";
    let (integers, errors) = integer_token_count(source);
    assert_eq!(errors, 0, "zero must scan without errors");
    assert_eq!(integers, 1, "expected one integer token");
    assert_eq!(first_integer_value(source), 0);
}

#[test]
fn unsigned_integer_scans_zero_hex_and_octal() {
    let cases: &[&str] = &["0x0", "0o0", "0b0"];
    for source in cases {
        let (integers, errors) = integer_token_count(source);
        assert_eq!(errors, 0, "{source} must scan without errors");
        assert_eq!(integers, 1, "{source}: expected one integer token");
        assert_eq!(first_integer_value(source), 0, "{source}: value mismatch");
    }
}
