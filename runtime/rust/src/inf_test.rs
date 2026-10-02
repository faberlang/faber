// WHY: the float assertions are exact-bit claims (the helpers are pure comparisons and a
// round-to-odd narrowing), and the panic messages are the pinned trap texts.
#![allow(clippy::float_cmp)]

use super::{
    approx, big, cmp_dec, cmp_float, div, f32, narrow, pack, pow, rem, shl, shr, store, to_dec,
    true_div,
};
use crate::{Magnus, Radix};
use std::cmp::Ordering;
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

fn m(value: i128) -> Magnus {
    Magnus::from_i128(value)
}

#[test]
fn a_big_literal_parses_its_digits() {
    assert_eq!(
        big("18446744073709551616").to_string(),
        "18446744073709551616"
    );
    assert_eq!(big("-7"), m(-7));
    assert_eq!(
        panic_text(|| big("12x")).as_deref(),
        Some("integer literal digits: Malformed")
    );
}

#[test]
fn approx_is_ten_to_the_ninth_relative() {
    assert!(approx(&m(1_000_000_000), &m(1_000_000_001)));
    assert!(!approx(&m(1_000_000_000), &m(1_000_000_002)));
    assert!(approx(&m(0), &m(0)));
    assert!(!approx(&m(0), &m(1)));
    // Beyond the 128-bit window the rule is unchanged.
    let huge = big("1000000000000000000000000000000000000000");
    assert!(approx(&huge, &(&huge + &m(1))));
}

#[test]
fn div_and_rem_floor_and_follow_the_divisor_sign() {
    assert_eq!(div(&m(7), &m(2)), m(3));
    assert_eq!(div(&m(-7), &m(2)), m(-4));
    assert_eq!(div(&m(7), &m(-2)), m(-4));
    assert_eq!(rem(&m(7), &m(2)), m(1));
    assert_eq!(rem(&m(-7), &m(2)), m(1));
    assert_eq!(rem(&m(7), &m(-2)), m(-1));
}

#[test]
fn a_zero_divisor_traps() {
    let zero = m(0);
    assert_eq!(
        panic_text(|| div(&m(1), &zero)).as_deref(),
        Some("numerus division failed")
    );
    assert_eq!(
        panic_text(|| rem(&m(1), &zero)).as_deref(),
        Some("numerus division failed")
    );
    assert_eq!(
        panic_text(|| true_div(&m(1), &zero)).as_deref(),
        Some("numerus division failed")
    );
}

#[test]
fn true_div_is_a_float_division() {
    assert_eq!(true_div(&m(7), &m(2)), 3.5);
    assert_eq!(true_div(&m(-1), &m(4)), -0.25);
}

#[test]
fn shifts_are_unmasked_and_a_negative_count_traps() {
    assert_eq!(
        shl(&m(1), &m(100)).to_string(),
        "1267650600228229401496703205376"
    );
    assert_eq!(shr(&m(-5), &m(1)), m(-3));
    assert_eq!(shr(&m(5), &m(1_000)), m(0));
    assert_eq!(
        panic_text(|| shl(&m(1), &m(-1))).as_deref(),
        Some("negative shift count")
    );
    assert_eq!(
        panic_text(|| shr(&m(1), &m(-1))).as_deref(),
        Some("negative shift count")
    );
}

#[test]
fn a_shift_beyond_the_address_space_exhausts_the_size() {
    let count = shl(&m(1), &m(64));
    assert_eq!(
        panic_text(|| shl(&m(1), &count)).as_deref(),
        Some("inf size exhausted in shift")
    );
}

#[test]
fn pow_is_exact_and_traps_on_a_negative_exponent_or_size() {
    assert_eq!(
        pow(&m(2), &m(100)).to_string(),
        "1267650600228229401496703205376"
    );
    assert_eq!(pow(&m(0), &m(0)), m(1));
    assert_eq!(pow(&m(-1), &m(3)), m(-1));
    assert_eq!(
        panic_text(|| pow(&m(2), &m(-1))).as_deref(),
        Some("numerus potentia failed: negative exponent")
    );
    let exponent = shl(&m(1), &m(64));
    assert_eq!(
        panic_text(|| pow(&m(2), &exponent)).as_deref(),
        Some("inf size exhausted in potentia")
    );
}

#[test]
fn narrow_is_none_outside_the_target_range() {
    assert_eq!(narrow::<u8>(&m(255)), Some(255));
    assert_eq!(narrow::<u8>(&m(256)), None);
    assert_eq!(narrow::<u8>(&m(-1)), None);
    assert_eq!(
        narrow::<i64>(&m(-9_223_372_036_854_775_808)),
        Some(i64::MIN)
    );
    assert_eq!(narrow::<i64>(&big("9223372036854775808")), None);
    assert_eq!(
        narrow::<i64>(&big("340282366920938463463374607431768211456")),
        None
    );
}

#[test]
fn store_traps_with_the_position_the_inferred_note_and_the_unsigned_note() {
    assert_eq!(store::<u8>(&m(255), "x", ""), 255);
    assert_eq!(
        panic_text(|| store::<u8>(&m(300), "return", "")).as_deref(),
        Some("300 does not fit in `u8` (return)")
    );
    assert_eq!(
        panic_text(|| store::<u8>(&m(-1), "return", "")).as_deref(),
        Some(
            "-1 does not fit in `u8` (return) \
             (a negative value cannot be stored in an unsigned slot)"
        )
    );
    assert_eq!(
        panic_text(|| store::<i8>(&m(-129), "x", " (inferred from y)")).as_deref(),
        Some("-129 does not fit in `i8` (x) (inferred from y)")
    );
    assert_eq!(
        panic_text(|| store::<i64>(&big("9223372036854775808"), "x", "")).as_deref(),
        Some("9223372036854775808 does not fit in `i64` (x)")
    );
}

#[test]
fn to_dec_scales_into_a_stored_carrier_or_is_none() {
    assert_eq!(to_dec(&m(3), 100_000_000), Some(300_000_000));
    assert_eq!(to_dec(&m(-2), 100_000_000), Some(-200_000_000));
    assert_eq!(to_dec(&big("1180591620717411303424"), 100_000_000), None);
    assert_eq!(
        to_dec(&big("340282366920938463463374607431768211456"), 1),
        None
    );
}

#[test]
fn f32_rounds_to_odd_so_the_narrowing_rounds_once() {
    assert_eq!(f32(&m(16_777_217)), 16_777_216.0);
    assert_eq!(f32(&m(-7)), -7.0);
    // 2^60 + 2^36 + 1 sits just above an `f32` midpoint; rounding to `f64` first
    // would land on the midpoint and round to even (down).
    let above = Magnus::from_u128((1 << 60) | (1 << 36) | 1);
    assert_eq!(f32(&above), (1_u64 << 60) as f32 + (1_u64 << 37) as f32);
    let beyond = m(1).shl(1_100).unwrap();
    assert_eq!(f32(&beyond), f32::INFINITY);
    assert_eq!(f32(&-&beyond), f32::NEG_INFINITY);
}

#[test]
fn pack_zero_pads_the_digits_to_the_width() {
    assert_eq!(pack(&m(255), Radix::Hex, 4), "00ff");
    assert_eq!(pack(&m(5), Radix::Bin, 8), "00000101");
    assert_eq!(pack(&m(8), Radix::Oct, 3), "010");
    assert_eq!(pack(&m(255), Radix::Hex, 1), "ff");
}

#[test]
fn cmp_float_is_exact() {
    assert_eq!(cmp_float(&m(3), 3.5), Some(Ordering::Less));
    assert_eq!(cmp_float(&m(4), 3.5), Some(Ordering::Greater));
    assert_eq!(cmp_float(&m(3), 3.0), Some(Ordering::Equal));
    assert_eq!(cmp_float(&m(-3), -3.5), Some(Ordering::Greater));
    assert_eq!(cmp_float(&m(-4), -3.5), Some(Ordering::Less));
    assert_eq!(cmp_float(&m(1), f64::NAN), None);
    assert_eq!(cmp_float(&m(1), f64::INFINITY), Some(Ordering::Less));
    assert_eq!(cmp_float(&m(1), f64::NEG_INFINITY), Some(Ordering::Greater));
    // 2^64 is exactly representable as a float.
    assert_eq!(
        cmp_float(&big("18446744073709551616"), 18_446_744_073_709_551_616.0),
        Some(Ordering::Equal)
    );
    assert_eq!(
        cmp_float(&big("18446744073709551617"), 18_446_744_073_709_551_616.0),
        Some(Ordering::Greater)
    );
}

#[test]
fn cmp_dec_compares_against_the_carrier_at_its_scale() {
    assert_eq!(cmp_dec(&m(5), (50, 1)), Ordering::Equal);
    assert_eq!(cmp_dec(&m(5), (51, 1)), Ordering::Less);
    assert_eq!(cmp_dec(&m(5), (49, 1)), Ordering::Greater);
    assert_eq!(cmp_dec(&m(-2), (-200_000_000, 8)), Ordering::Equal);
    assert_eq!(cmp_dec(&m(3), (3, 0)), Ordering::Equal);
}
