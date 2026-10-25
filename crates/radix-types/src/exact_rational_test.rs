//! Exact rational arithmetic and once-rounded float conversions. Exact float
//! comparisons are the point of these tests.
#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::unreadable_literal
)]

use super::*;

fn dec(text: &str) -> ExactRational {
    ExactRational::from_decimal_text(text).unwrap_or_else(|| panic!("decimal {text:?}"))
}

fn int(value: i64) -> ExactRational {
    ExactRational::from_integer(Magnus::from_i64(value))
}

#[test]
fn decimal_spellings_are_exact_values() {
    let tenth = dec("0.1");
    assert_eq!(
        tenth,
        ExactRational::from_ratio(Magnus::from_u64(1), Magnus::from_u64(10)).unwrap()
    );
    assert_eq!(
        dec("1.5e-3"),
        ExactRational::from_ratio(Magnus::from_u64(3), Magnus::from_u64(2000)).unwrap()
    );
    assert_eq!(dec("2E3"), int(2000));
    assert_eq!(dec(".5"), dec("0.50"));
    assert_eq!(dec("5."), int(5));
    assert_eq!(dec("-0.25"), dec("0.25").neg());
    for bad in ["", ".", "e5", "1e", "1e+", "0x10", "1.2.3", "abc", "1_0"] {
        assert!(ExactRational::from_decimal_text(bad).is_none(), "{bad:?}");
    }
}

#[test]
fn the_sum_of_point_one_and_point_two_is_exactly_three_tenths() {
    let sum = dec("0.1").add(&dec("0.2")).unwrap();
    assert_eq!(sum, dec("0.3"));
    assert_eq!(sum.to_f64().to_bits(), 0.3_f64.to_bits());
    // The IEEE per-operation sum is the next f64 up.
    assert_ne!((0.1_f64 + 0.2_f64).to_bits(), sum.to_f64().to_bits());
}

#[test]
fn one_third_is_an_exact_rational() {
    let third = int(1).div(&int(3)).unwrap();
    assert_eq!(third.mul(&int(3)).unwrap(), int(1));
    assert_eq!(third.to_f64().to_bits(), (1.0_f64 / 3.0).to_bits());
    assert_eq!(third.to_f32().to_bits(), (1.0_f32 / 3.0).to_bits());
    assert_ne!(third, dec("0.3333333333333333"));
}

#[test]
fn parenthesised_sum_times_ten_is_exactly_three() {
    let value = dec("0.1")
        .add(&dec("0.2"))
        .unwrap()
        .mul(&dec("10.0"))
        .unwrap();
    assert_eq!(value, int(3));
    assert_eq!(value.to_f64(), 3.0);
    // Per operation, the same expression is 3.0000000000000004.
    assert_ne!((0.1_f64 + 0.2_f64) * 10.0, 3.0);
}

#[test]
fn operators_are_exact_and_zero_divisors_refuse() {
    assert_eq!(dec("0.7").sub(&dec("0.2")).unwrap(), dec("0.5"));
    assert_eq!(dec("1.1").mul(&dec("1.1")).unwrap(), dec("1.21"));
    assert_eq!(dec("1").div(&dec("8")).unwrap(), dec("0.125"));
    assert!(dec("1").div(&int(0)).is_none());
    assert!(dec("1").rem_floor(&dec("0.0")).is_none());
}

#[test]
fn remainder_is_the_floor_remainder() {
    assert_eq!(dec("7.5").rem_floor(&dec("2")).unwrap(), dec("1.5"));
    assert_eq!(dec("-7.5").rem_floor(&dec("2")).unwrap(), dec("0.5"));
    assert_eq!(dec("7.5").rem_floor(&dec("-2")).unwrap(), dec("-0.5"));
    assert_eq!(dec("-7.5").rem_floor(&dec("-2")).unwrap(), dec("-1.5"));
    assert_eq!(dec("0.3").rem_floor(&dec("0.1")).unwrap(), int(0));
}

#[test]
fn f64_round_trips_exactly_through_the_rational() {
    for value in [
        0.1,
        -2.5,
        1e300,
        5e-324,
        f64::MAX,
        f64::MIN_POSITIVE,
        123_456.789,
    ] {
        let exact = ExactRational::from_f64(value).unwrap();
        assert_eq!(exact.to_f64().to_bits(), value.to_bits(), "{value}");
    }
    assert!(ExactRational::from_f64(f64::NAN).is_none());
    assert!(ExactRational::from_f64(f64::INFINITY).is_none());
    // 0.1 as an f64 is not the decimal 0.1.
    assert_ne!(ExactRational::from_f64(0.1).unwrap(), dec("0.1"));
}

#[test]
fn large_and_small_exponents_round_correctly() {
    for text in [
        "1e308",
        "1.7976931348623157e308",
        "2.2250738585072014e-308",
        "5e-324",
        "1e-323",
        "4.9e-324",
        "2.4703282292062327e-324",
        "2.4703282292062328e-324",
    ] {
        assert_eq!(
            dec(text).to_f64().to_bits(),
            text.parse::<f64>().unwrap().to_bits(),
            "{text}"
        );
    }
    // 2^1024 - 2^970 (1.797693134862315807937e308) is the tie that rounds to infinity.
    assert_eq!(dec("1.7976931348623158e308").to_f64(), f64::MAX);
    assert_eq!(dec("1.79769313486231581e308").to_f64(), f64::INFINITY);
    assert_eq!(dec("-1e309").to_f64(), f64::NEG_INFINITY);
    // The product of two large literals overflows the f64 but not the exact value.
    let big = dec("1e300").mul(&dec("1e300")).unwrap();
    assert_eq!(big.to_f64(), f64::INFINITY);
    assert_eq!(big.div(&dec("1e300")).unwrap().to_f64(), 1e300);
    let tiny = dec("1e-300").mul(&dec("1e-300")).unwrap();
    assert_eq!(tiny.to_f64(), 0.0);
    assert_eq!(tiny.div(&dec("1e-300")).unwrap().to_f64(), 1e-300);
}

#[test]
fn decimal_conversion_matches_the_std_parser_on_many_spellings() {
    // A deterministic generator of decimal spellings across the exponent range.
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        state >> 33
    };
    for _ in 0..1500 {
        let digits = 1 + next() % 25;
        let mantissa: String = (0..digits)
            .map(|_| char::from(b'0' + u8::try_from(next() % 10).unwrap()))
            .collect();
        let point = usize::try_from(next()).unwrap() % (mantissa.len() + 1);
        let exponent = i64::try_from(next() % 700).unwrap() - 350;
        let text = format!("{}.{}e{exponent}", &mantissa[..point], &mantissa[point..]);
        let exact = dec(&text);
        let std64: f64 = text.parse().unwrap();
        let std32: f32 = text.parse().unwrap();
        assert_eq!(exact.to_f64().to_bits(), std64.to_bits(), "f64 {text}");
        assert_eq!(exact.to_f32().to_bits(), std32.to_bits(), "f32 {text}");
    }
}

#[test]
fn f32_rounds_once_from_the_exact_value_not_twice() {
    // 1 + 2^-24 is the midpoint of two f32s; this value sits 1e-26 above it.
    let text = "1.00000005960464477539062501";
    let exact = dec(text);
    let above = 1.0_f32 + f32::EPSILON;
    assert_eq!(exact.to_f32(), above);
    // Rounding through f64 first lands exactly on the midpoint and ties to even.
    let through_f64 = exact.to_f64();
    assert_eq!(through_f64, 1.0 + f64::from(f32::EPSILON) / 2.0);
    assert_eq!(through_f64 as f32, 1.0_f32);
    assert_ne!(through_f64 as f32, exact.to_f32());
    assert_eq!(
        exact.round_to_width(NumericWidth::F32),
        Some(f64::from(above))
    );
}

#[test]
fn f32_subnormals_overflow_and_ties() {
    assert_eq!(dec("0.3").to_f32().to_bits(), 0.3_f32.to_bits());
    assert_eq!(dec("1e-45").to_f32().to_bits(), 1u32); // smallest subnormal
    assert_eq!(dec("7e-46").to_f32(), 0.0); // below half of it
    assert_eq!(dec("3.4028235e38").to_f32(), f32::MAX);
    assert_eq!(dec("3.40282356e38").to_f32(), f32::MAX);
    assert_eq!(dec("3.40282357e38").to_f32(), f32::INFINITY);
    assert_eq!(dec("-1e39").to_f32(), f32::NEG_INFINITY);
    // 2^24 + 1 is a tie between 2^24 and 2^24 + 2: even wins.
    assert_eq!(dec("16777217").to_f32(), 16_777_216.0);
    assert_eq!(dec("16777219").to_f32(), 16_777_220.0);
}

#[test]
fn f16_rounds_once_with_subnormals_and_overflow() {
    // 0.1 as binary16 is 0x2E66 = 0.0999755859375.
    assert_eq!(dec("0.1").to_f16(), 0.099_975_586);
    assert_eq!(dec("0.3").to_f16(), 0.300_048_83);
    assert_eq!(dec("65504").to_f16(), 65504.0);
    assert_eq!(dec("65519.99").to_f16(), 65504.0);
    assert_eq!(dec("65520").to_f16(), f32::INFINITY);
    assert_eq!(dec("-70000").to_f16(), f32::NEG_INFINITY);
    // 2^-24 is the smallest subnormal; half of it ties to even (zero).
    let smallest = 2.0_f32.powi(-24);
    assert_eq!(
        ExactRational::from_f64(f64::from(smallest))
            .unwrap()
            .to_f16(),
        smallest
    );
    assert_eq!(
        ExactRational::from_f64(f64::from(smallest) / 2.0)
            .unwrap()
            .to_f16(),
        0.0
    );
    assert_eq!(
        ExactRational::from_f64(f64::from(smallest) * 0.5000001)
            .unwrap()
            .to_f16(),
        smallest
    );
    assert_eq!(
        ExactRational::from_f64(f64::from(smallest) * 1.5)
            .unwrap()
            .to_f16(),
        2.0 * smallest
    );
    // 2^-14 is the smallest normal.
    assert_eq!(dec("0.00006103515625").to_f16(), 2.0_f32.powi(-14));
}

#[test]
fn bf16_rounds_once_with_overflow() {
    // 0.1 as bfloat16 is 0x3DCD = 0.10009765625.
    assert_eq!(dec("0.1").to_bf16(), 0.100_097_656);
    assert_eq!(dec("1").to_bf16(), 1.0);
    // 1 + 2^-8 is the midpoint of two bf16s: even wins; a hair more rounds up.
    assert_eq!(dec("1.00390625").to_bf16(), 1.0);
    assert_eq!(dec("1.00390626").to_bf16(), 1.007_812_5);
    // The largest bf16 is 3.3895314e38; the f32 maximum rounds past it.
    assert_eq!(dec("3.3895313892515355e38").to_bf16(), 3.389_531_4e38);
    assert_eq!(dec("3.4028235e38").to_bf16(), f32::INFINITY);
}

#[test]
fn round_to_width_names_only_float_widths() {
    let third = int(1).div(&int(3)).unwrap();
    assert_eq!(third.round_to_width(NumericWidth::F64), Some(1.0 / 3.0));
    assert_eq!(
        third.round_to_width(NumericWidth::F32),
        Some(f64::from(1.0_f32 / 3.0))
    );
    assert!(third.round_to_width(NumericWidth::I64).is_none());
    assert!(third.round_to_width(NumericWidth::D64).is_none());
}

#[test]
fn zero_keeps_no_sign_and_negatives_mirror_positives() {
    assert_eq!(int(0).to_f64().to_bits(), 0.0_f64.to_bits());
    assert_eq!(
        dec("0.1").sub(&dec("0.1")).unwrap().to_f64().to_bits(),
        0.0_f64.to_bits()
    );
    assert_eq!(dec("-0.3").to_f64(), -0.3);
    assert_eq!(dec("-0.3").to_f32(), -0.3_f32);
}

#[test]
fn values_past_the_cap_refuse() {
    // 10^400000 has about 1.33 million bits, over the 2^20 cap.
    assert!(ExactRational::from_decimal_text("1e400000").is_none());
    assert!(ExactRational::from_decimal_text("1e-400000").is_none());
    let big = dec("1e300000");
    assert!(big.mul(&big).is_none());
    assert!(big.add(&int(1)).is_some());
}
