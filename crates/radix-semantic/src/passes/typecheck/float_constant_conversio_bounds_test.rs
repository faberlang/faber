//! cv1: a constant float `↦` an integer target is infallible only when its
//! truncation toward zero is exactly inside the target range.
//!
//! The bound check must be exact: `i64::MAX` / `u64::MAX` round UP to 2^63 /
//! 2^64 as an `f64`, so comparing in float space accepted those two boundary
//! floats as fitting and the conversion then failed at run time. A fitting
//! constant is accepted with no handler; a constant that does not fit is
//! `SEM011:failable_conversio_requires_handler` without one.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

const ISSUE: &str = "failable_conversio_requires_handler";

/// `(target spelling, lo, hi)` for every trapping integer width.
const TARGETS: [(&str, i128, i128); 9] = [
    ("i8", i8::MIN as i128, i8::MAX as i128),
    ("i16", i16::MIN as i128, i16::MAX as i128),
    ("i32", i32::MIN as i128, i32::MAX as i128),
    ("i64", i64::MIN as i128, i64::MAX as i128),
    ("numerus", i64::MIN as i128, i64::MAX as i128),
    ("u8", 0, u8::MAX as i128),
    ("u16", 0, u16::MAX as i128),
    ("u32", 0, u32::MAX as i128),
    ("u64", 0, u64::MAX as i128),
];

fn check(target: &str, literal: &str, fits: bool) {
    let source =
        format!("functio g() → {target} {{\n    redde {literal} ↦ {target} via Trunc\n}}\n");
    if fits {
        assert_accepts(&source);
    } else {
        assert_rejects_with_issue(&source, ISSUE);
    }
}

/// Exactly-written float literal for an integral `f64` value.
fn float_text(value: f64) -> String {
    format!("{value:.1}")
}

/// Whether `value` (exact, integral) lies in `lo..=hi`.
fn fits(value: f64, lo: i128, hi: i128) -> bool {
    let exact = value as i128;
    assert_eq!(exact as f64, value, "test value must be an exact integer");
    (lo..=hi).contains(&exact)
}

#[test]
fn min_max_and_one_past_each_width_boundary() {
    for (target, lo, hi) in TARGETS {
        for exact in [lo, hi, lo - 1, hi + 1] {
            let value = exact as f64;
            // MIN-1 / MAX+1 of the 64-bit widths are not f64-representable;
            // they round back onto the boundary and are covered by the
            // adjacent-float cases below.
            if value as i128 != exact {
                continue;
            }
            check(target, &float_text(value), fits(value, lo, hi));
        }
    }
}

#[test]
fn two_to_the_63_and_64_boundaries() {
    let p63 = 9_223_372_036_854_775_808.0_f64;
    let p64 = 18_446_744_073_709_551_616.0_f64;
    // The adjacent f64 values just inside / outside each bound.
    let below_p63 = 9_223_372_036_854_774_784.0_f64;
    let below_p64 = 18_446_744_073_709_549_568.0_f64;
    let below_neg_p63 = -9_223_372_036_854_777_856.0_f64;
    for (target, lo, hi) in TARGETS {
        for value in [p63, -p63, p64, -p64, below_p63, below_p64, below_neg_p63] {
            check(target, &float_text(value), fits(value, lo, hi));
        }
    }
}

#[test]
fn the_reported_cases() {
    // 2^63 does not fit `int`; 2^64 does not fit u64; the largest f64 below
    // 2^64 and -2^63 do.
    check("numerus", "9223372036854775808.0", false);
    check("u64", "18446744073709551616.0", false);
    check("u64", "18446744073709549568.0", true);
    check("numerus", "-9223372036854775808.0", true);
}

#[test]
fn truncation_toward_zero_decides_fit() {
    check("u8", "255.9", true);
    check("u8", "256.0", false);
    check("u8", "-0.5", true);
    check("u8", "-1.0", false);
    check("i8", "-128.9", true);
    check("i8", "-129.0", false);
}

#[test]
fn a_handler_still_covers_a_constant_that_does_not_fit() {
    assert_accepts(
        "functio g() → u64 {\n    redde 18446744073709551616.0 ↦ u64 via Trunc ⊥ 0\n}\n",
    );
}
