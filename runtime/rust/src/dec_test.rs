// WHY: the float assertions are exact claims (the conversions are pure divisions of
// representable values), and the panic messages are the pinned trap texts.
#![allow(clippy::float_cmp)]

use super::{
    add, cmp, div, f64, fmt, fmt_list, fmt_option, int, mul, neg, of, parse, rem, rhe, round,
    store, sub, try_parse, try_store,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// The text of the panic a call raises (`None` when it returns).
fn panic_text<R>(call: impl FnOnce() -> R) -> Option<String> {
    catch_unwind(AssertUnwindSafe(call)).err().map(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
            })
            .unwrap_or_default()
    })
}

const OUTSIDE: &str = "decimal carrier is outside the declared width";
const MALFORMED: &str = "textus to numerus conversion failed";

#[test]
fn a_stored_value_lifts_at_scale_eight_and_an_integer_at_scale_zero() {
    assert_eq!(of(150_000_000), (150_000_000, 8));
    assert_eq!(int(7), (7, 0));
}

#[test]
fn add_and_sub_align_to_the_larger_scale() {
    assert_eq!(add(of(150_000_000), int(2)), (350_000_000, 8));
    assert_eq!(sub(of(150_000_000), int(2)), (-50_000_000, 8));
    assert_eq!(add(int(1), int(2)), (3, 0));
}

#[test]
fn mul_adds_scales_then_trims_trailing_zeros_above_scale_eight() {
    // 1.5 * 1.5 = 2.25: the carrier at scale 16 trims back to scale 8.
    assert_eq!(mul(of(150_000_000), of(150_000_000)), (225_000_000, 8));
    assert_eq!(mul(int(6), int(7)), (42, 0));
}

#[test]
fn div_rounds_half_even_at_the_larger_operand_scale() {
    assert_eq!(div(of(100_000_000), of(300_000_000)), (33_333_333, 8));
    // At scale zero the quotient is an integer: 1 / 3 rounds to 0.
    assert_eq!(div(int(1), int(3)), (0, 0));
    assert_eq!(div(int(5), int(2)), (2, 0));
    assert_eq!(div(int(7), int(2)), (4, 0));
}

#[test]
fn rem_takes_the_sign_of_the_divisor() {
    assert_eq!(rem(int(-7), int(3)), (2, 0));
    assert_eq!(rem(int(7), int(-3)), (-2, 0));
    assert_eq!(rem(int(7), int(3)), (1, 0));
}

#[test]
fn neg_is_exact() {
    assert_eq!(neg(of(150_000_000)), (-150_000_000, 8));
    assert_eq!(neg(int(0)), (0, 0));
}

#[test]
fn cmp_is_three_way_and_survives_an_operand_too_large_to_align() {
    assert_eq!(cmp(of(150_000_000), int(1)), 1);
    assert_eq!(cmp(int(1), of(150_000_000)), -1);
    assert_eq!(cmp(of(100_000_000), int(1)), 0);
    // `i128::MAX` at scale 0 cannot be raised to scale 8: it compares by sign.
    assert_eq!(cmp((i128::MAX, 0), (1, 8)), 1);
    assert_eq!(cmp((i128::MIN, 0), (1, 8)), -1);
    assert_eq!(cmp((1, 8), (i128::MAX, 0)), -1);
    assert_eq!(cmp((1, 8), (i128::MIN, 0)), 1);
}

#[test]
fn fmt_trims_trailing_fraction_zeros_and_keeps_the_sign() {
    assert_eq!(fmt((150_000_000, 8)), "1.5");
    assert_eq!(fmt((-5, 1)), "-0.5");
    assert_eq!(fmt((100, 0)), "100");
    assert_eq!(fmt((0, 0)), "0");
    assert_eq!(fmt((100_000_000, 8)), "1");
}

#[test]
fn fmt_list_and_fmt_option_render_stored_values() {
    assert_eq!(fmt_list(&[150_000_000, -50_000_000]), "[1.5, -0.5]");
    assert_eq!(fmt_list(&[]), "[]");
    assert_eq!(fmt_option(Some(150_000_000)), "1.5");
    assert_eq!(fmt_option(None), "nihil");
}

#[test]
fn try_store_rounds_half_even_to_scale_eight_and_refuses_outside_the_carrier() {
    assert_eq!(try_store((1, 0)), Some(100_000_000));
    assert_eq!(try_store((1, 9)), Some(0));
    assert_eq!(try_store((5, 9)), Some(0));
    assert_eq!(try_store((15, 9)), Some(2));
    assert_eq!(try_store((25, 9)), Some(2));
    assert_eq!(try_store((i128::MAX, 0)), None);
    assert_eq!(try_store((1_i128 << 80, 0)), None);
}

#[test]
fn store_returns_the_carrier_inside_the_window() {
    assert_eq!(store(add(of(150_000_000), int(2)), "", ""), 350_000_000);
}

#[test]
fn store_traps_outside_the_carrier_with_the_site_text() {
    assert_eq!(
        panic_text(|| store((1_i128 << 80, 0), "declaration of `x`", "")).as_deref(),
        Some("1208925819614629174706176 does not fit in `d64` (declaration of `x`)")
    );
    assert_eq!(
        panic_text(|| store((1_i128 << 80, 0), "return", " [x was inferred as d64]")).as_deref(),
        Some("1208925819614629174706176 does not fit in `d64` (return) [x was inferred as d64]")
    );
}

#[test]
fn round_is_half_even_and_none_when_the_scale_overflows_the_carrier() {
    assert_eq!(round((25, 1)), Some(2));
    assert_eq!(round((35, 1)), Some(4));
    assert_eq!(round((-25, 1)), Some(-2));
    assert_eq!(round((1, 40)), None);
}

#[test]
fn f64_divides_the_carrier_by_the_scale_factor() {
    assert_eq!(f64((150_000_000, 8)), 1.5);
    assert_eq!(f64((1, 40)), 1e-40);
    assert_eq!(f64((7, 0)), 7.0);
}

#[test]
fn rhe_breaks_a_midpoint_toward_the_even_quotient() {
    assert_eq!(rhe(5, 2), 2);
    assert_eq!(rhe(7, 2), 4);
    assert_eq!(rhe(-5, 2), -2);
    assert_eq!(rhe(-7, 2), -4);
    assert_eq!(rhe(10, 5), 2);
    assert_eq!(rhe(7, 3), 2);
}

#[test]
fn arithmetic_traps_keep_their_texts() {
    let big = (i128::MAX, 0);
    assert_eq!(
        panic_text(|| add(big, int(1))).as_deref(),
        Some("numerus overflow")
    );
    assert_eq!(
        panic_text(|| sub((i128::MIN, 0), int(1))).as_deref(),
        Some("numerus overflow")
    );
    assert_eq!(
        panic_text(|| mul(big, int(2))).as_deref(),
        Some("numerus overflow")
    );
    assert_eq!(
        panic_text(|| neg((i128::MIN, 0))).as_deref(),
        Some("numerus overflow")
    );
    // The scale factor to align the operands overflows.
    assert_eq!(
        panic_text(|| add((1, 0), (1, 40))).as_deref(),
        Some("numerus overflow")
    );
    // The scales add past `u32`.
    assert_eq!(
        panic_text(|| mul((1, u32::MAX), (1, 1))).as_deref(),
        Some("numerus overflow")
    );
}

#[test]
fn division_by_zero_and_an_unrepresentable_quotient_trap_numerus_division_failed() {
    let failed = Some("numerus division failed");
    assert_eq!(panic_text(|| div(int(1), int(0))).as_deref(), failed);
    assert_eq!(panic_text(|| rem(int(1), int(0))).as_deref(), failed);
    assert_eq!(panic_text(|| rhe(1, 0)).as_deref(), failed);
    // The numerator needs a scale factor beyond the `i128` window.
    assert_eq!(panic_text(|| div((1, 0), (1, 40))).as_deref(), failed);
}

#[test]
fn rhe_rounds_the_widest_midpoint_without_leaving_the_carrier() {
    // `i128::MAX / 2` truncates to an odd quotient at an exact midpoint, so it
    // steps away from zero by one through the checked add.
    assert_eq!(
        rhe(i128::MAX, 2),
        85_070_591_730_234_615_865_843_651_857_942_052_864
    );
    assert_eq!(rhe(i128::MIN, i128::MAX), -1);
}

#[test]
fn try_parse_matches_the_compile_time_tier_on_boundary_texts() {
    // The expected carriers are the ones `radix_codegen_shared::decimal` yields
    // at the `d64` scale (8) for the same text.
    let cases: &[(&str, Result<i128, &str>)] = &[
        // Round-half-even and sub-ulp values.
        ("0.0000000005", Ok(0)),
        ("0.0000000015", Ok(0)),
        ("-0.0000000015", Ok(0)),
        ("0.0000000006", Ok(0)),
        ("0.00000000015", Ok(0)),
        // Plain carriers.
        ("1.23456789", Ok(123_456_789)),
        ("4.2", Ok(420_000_000)),
        ("1e-3", Ok(100_000)),
        ("-0.5", Ok(-50_000_000)),
        ("1.25", Ok(125_000_000)),
        ("+7", Ok(700_000_000)),
        // A beyond-i128 divisor rounds to the reasoned zero.
        ("1e-100", Ok(0)),
        // 38 digits parse; the rounding reaches an exact carrier.
        ("9.9999999999999999999999999999999999999", Ok(1_000_000_000)),
        // Inside i128 but beyond i64: the width check is the caller's.
        ("92233720368.54775808", Ok(9_223_372_036_854_775_808)),
        ("1e29", Ok(10_i128.pow(37))),
        // Scale-factor and mantissa overflow.
        ("1e31", Err(OUTSIDE)),
        ("1e40", Err(OUTSIDE)),
        ("2e30", Err(OUTSIDE)),
        // Mantissa accumulation: 39 digits and exactly 2^128 overflow `i128`.
        ("999999999999999999999999999999999999999", Err(OUTSIDE)),
        ("340282366920938463463374607431768211456", Err(OUTSIDE)),
        // Exponent accumulation overflows `i32`.
        ("0.01e-2147483647", Err(OUTSIDE)),
        // Malformed text.
        ("4.2eX", Err(MALFORMED)),
        ("abc", Err(MALFORMED)),
        ("", Err(MALFORMED)),
        ("4.2x", Err(MALFORMED)),
    ];
    for (text, expected) in cases {
        assert_eq!(try_parse(text, 8), *expected, "text {text:?}");
    }
}

#[test]
fn parse_is_the_panicking_form_of_try_parse() {
    assert_eq!(parse("4.2", 8), 420_000_000);
    assert_eq!(parse("1e-100", 8), 0);
    assert_eq!(panic_text(|| parse("1e40", 8)).as_deref(), Some(OUTSIDE));
    assert_eq!(panic_text(|| parse("4.2eX", 8)).as_deref(), Some(MALFORMED));
    assert_eq!(panic_text(|| parse("", 8)).as_deref(), Some(MALFORMED));
}
