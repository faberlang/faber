// WHY: the float assertions are exact-bit claims (the helpers are pure comparisons), and the
// panic messages are the pinned trap texts.
#![allow(clippy::float_cmp)]

use super::{
    approx_f32, approx_f64, approx_int, cmp_int_float, cmp_scaled_float, div, fit, int, pow, rem,
    shl, shr, store,
};
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

const LOW: i128 = -9_223_372_036_854_775_808;
const HIGH: i128 = 18_446_744_073_709_551_615;

#[test]
fn int_accepts_the_window_and_traps_outside_it() {
    assert_eq!(int(Some(LOW)), LOW);
    assert_eq!(int(Some(HIGH)), HIGH);
    assert_eq!(int(Some(0)), 0);
    assert_eq!(
        panic_text(|| int(Some(LOW - 1))).as_deref(),
        Some("numerus overflow")
    );
    assert_eq!(
        panic_text(|| int(Some(HIGH + 1))).as_deref(),
        Some("numerus overflow")
    );
    assert_eq!(
        panic_text(|| int(None)).as_deref(),
        Some("numerus overflow")
    );
}

#[test]
fn fit_narrows_or_traps_numerus_overflow() {
    assert_eq!(fit::<i8>(127), 127_i8);
    assert_eq!(fit::<u8>(0), 0_u8);
    assert_eq!(
        panic_text(|| fit::<i8>(128)).as_deref(),
        Some("numerus overflow")
    );
    assert_eq!(
        panic_text(|| fit::<u8>(-1)).as_deref(),
        Some("numerus overflow")
    );
}

#[test]
fn pow_is_exact_and_traps_a_negative_exponent_or_overflow() {
    assert_eq!(pow(2, 10), 1024);
    assert_eq!(pow(-3, 3), -27);
    assert_eq!(pow(7, 0), 1);
    assert_eq!(pow(2, 63), 1_i128 << 63);
    assert_eq!(
        panic_text(|| pow(2, -1)).as_deref(),
        Some("numerus potentia failed: negative exponent")
    );
    assert_eq!(
        panic_text(|| pow(2, 65)).as_deref(),
        Some("numerus overflow")
    );
}

#[test]
fn div_floors_and_rem_takes_the_divisor_sign() {
    assert_eq!(div(7, 2), 3);
    assert_eq!(div(-7, 2), -4);
    assert_eq!(div(7, -2), -4);
    assert_eq!(div(-7, -2), 3);
    assert_eq!(rem(7, 2), 1);
    assert_eq!(rem(-7, 2), 1);
    assert_eq!(rem(7, -2), -1);
    assert_eq!(rem(-7, -2), -1);
    assert_eq!(
        panic_text(|| div(1, 0)).as_deref(),
        Some("numerus division failed")
    );
    assert_eq!(
        panic_text(|| rem(1, 0)).as_deref(),
        Some("numerus division failed")
    );
}

#[test]
fn shifts_are_unmasked_and_trap_a_negative_count() {
    assert_eq!(shl(1, 10), 1024);
    assert_eq!(shl(0, 1_000), 0);
    assert_eq!(shl(1, 63), 1_i128 << 63);
    assert_eq!(shr(1024, 10), 1);
    assert_eq!(shr(-1, 200), -1);
    assert_eq!(shr(5, 200), 0);
    assert_eq!(shr(-8, 1), -4);
    assert_eq!(
        panic_text(|| shl(1, -1)).as_deref(),
        Some("negative shift count")
    );
    assert_eq!(
        panic_text(|| shr(1, -1)).as_deref(),
        Some("negative shift count")
    );
    assert_eq!(
        panic_text(|| shl(1, 65)).as_deref(),
        Some("numerus overflow")
    );
    assert_eq!(
        panic_text(|| shl(HIGH, 1)).as_deref(),
        Some("numerus overflow")
    );
}

#[test]
fn store_narrows_or_traps_with_the_slot_the_site_and_the_unsigned_note() {
    assert_eq!(store::<u8>(255, "return", ""), 255_u8);
    assert_eq!(store::<i64>(-5, "return", ""), -5_i64);
    assert_eq!(
        panic_text(|| store::<i8>(300, "x", "")).as_deref(),
        Some("300 does not fit in `i8` (x)")
    );
    assert_eq!(
        panic_text(|| store::<i8>(300, "x", " (inferred)")).as_deref(),
        Some("300 does not fit in `i8` (x) (inferred)")
    );
    assert_eq!(
        panic_text(|| store::<u8>(-1, "x", "")).as_deref(),
        Some("-1 does not fit in `u8` (x) (a negative value cannot be stored in an unsigned slot)")
    );
    assert_eq!(
        panic_text(|| store::<u8>(-1, "x", " (inferred)")).as_deref(),
        Some(
            "-1 does not fit in `u8` (x) (inferred) (a negative value cannot be stored in an unsigned slot)"
        )
    );
    // A negative value outside a signed slot carries no unsigned note.
    assert_eq!(
        panic_text(|| store::<i8>(-129, "x", "")).as_deref(),
        Some("-129 does not fit in `i8` (x)")
    );
}

#[test]
fn approx_comparisons_use_the_one_part_in_a_billion_tolerance() {
    assert!(approx_int(1_000_000_000, 1_000_000_001));
    assert!(!approx_int(1_000_000_000, 1_000_000_002));
    assert!(approx_int(0, 0));
    assert!(approx_f64(1.0, 1.0 + 1e-10));
    assert!(!approx_f64(1.0, 1.0 + 1e-8));
    assert!(approx_f32(1.0, 1.0 + 1e-10));
    assert!(!approx_f32(1.0, 1.0 + 1e-3));
}

#[test]
fn cmp_int_float_compares_true_values() {
    assert_eq!(cmp_int_float(3, 3.0), Some(Ordering::Equal));
    assert_eq!(cmp_int_float(3, 3.5), Some(Ordering::Less));
    assert_eq!(cmp_int_float(3, 2.5), Some(Ordering::Greater));
    assert_eq!(cmp_int_float(-3, -3.5), Some(Ordering::Greater));
    assert_eq!(cmp_int_float(0, f64::NAN), None);
    assert_eq!(
        cmp_int_float(i128::MAX, f64::INFINITY),
        Some(Ordering::Less)
    );
    assert_eq!(
        cmp_int_float(i128::MIN, f64::NEG_INFINITY),
        Some(Ordering::Greater)
    );
    // 2^63 + 1 is not representable as a float: the true values differ.
    assert_eq!(
        cmp_int_float((1_i128 << 63) + 1, 9_223_372_036_854_775_808.0),
        Some(Ordering::Greater)
    );
}

#[test]
fn cmp_scaled_float_compares_true_values() {
    // 1.5 as a scale-10 carrier.
    assert_eq!(cmp_scaled_float(15, 10, 1.5), Some(Ordering::Equal));
    assert_eq!(cmp_scaled_float(15, 10, 1.6), Some(Ordering::Less));
    assert_eq!(cmp_scaled_float(15, 10, 1.4), Some(Ordering::Greater));
    assert_eq!(cmp_scaled_float(-15, 10, -1.5), Some(Ordering::Equal));
    assert_eq!(cmp_scaled_float(-15, 10, -1.4), Some(Ordering::Less));
    assert_eq!(cmp_scaled_float(1, 10, f64::NAN), None);
    // 0.1 is not the float 0.1: the exact decimal is smaller than the double.
    assert_eq!(cmp_scaled_float(1, 10, 0.1), Some(Ordering::Less));
    assert_eq!(cmp_scaled_float(0, 1, f64::INFINITY), Some(Ordering::Less));
    assert_eq!(
        cmp_scaled_float(0, 1, f64::NEG_INFINITY),
        Some(Ordering::Greater)
    );
}

/// `approx_int` is a `const fn` (a genus `static` field above 2^53 is a Rust
/// associated const): every verdict below is evaluated by the compiler, and the
/// runtime assertion repeats it. The rows are the edges the exact rule exists
/// for: opposite signs, `MIN` magnitudes, scaled products past 2^64.
#[test]
fn approx_int_is_const_evaluable_and_exact_at_the_edges() {
    const U64_MAX: i128 = 18_446_744_073_709_551_615;
    const I64_MIN: i128 = -9_223_372_036_854_775_808;
    const I64_MAX: i128 = 9_223_372_036_854_775_807;
    const I32_MIN: i128 = -2_147_483_648;
    const ROWS: [(bool, bool); 16] = [
        (approx_int(5, -5), false),
        (approx_int(-5, 5), false),
        (approx_int(0, 0), true),
        (approx_int(I64_MAX, -I64_MAX), false),
        (approx_int(I64_MIN, I64_MIN + 1), true),
        (approx_int(I64_MIN, 0), false),
        (approx_int(I32_MIN, I32_MIN + 1), true),
        (approx_int(I32_MIN, I32_MIN + 10), false),
        (approx_int(1_000_000_000, 999_999_999), true),
        (approx_int(999_999_999, 999_999_998), false),
        (approx_int(U64_MAX, U64_MAX - 1000), true),
        (approx_int(U64_MAX, U64_MAX - 18_446_744_073), true),
        (approx_int(U64_MAX, U64_MAX - 18_446_744_074), false),
        (approx_int(U64_MAX, U64_MAX - 20_000_000_000), false),
        (approx_int(U64_MAX, 0), false),
        (
            approx_int(9_007_199_254_740_993, 9_007_199_254_740_992),
            true,
        ),
    ];
    for (index, (got, want)) in ROWS.into_iter().enumerate() {
        assert_eq!(got, want, "row {index}");
    }
}
