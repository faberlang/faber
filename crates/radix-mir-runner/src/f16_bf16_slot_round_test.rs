//! Regression coverage for c93f6788 (GAPS #1), ruling 26(d): slot rounding
//! uses the destination f16/bf16 format while arithmetic remains at f32.

use super::round_float_at_slot;
use radix_types::NumericWidth;

#[test]
fn f16_bf16_slot_round_f16_uses_binary16_boundaries_and_ties() {
    assert_eq!(
        round_float_at_slot(120_000.0, NumericWidth::F16),
        f64::INFINITY
    );
    assert_eq!(
        round_float_at_slot(-120_000.0, NumericWidth::F16),
        f64::NEG_INFINITY
    );
    assert_eq!(
        round_float_at_slot(0.333_333_333_333_333_3, NumericWidth::F16),
        0.333_251_953_125
    );
    assert_eq!(round_float_at_slot(2049.0, NumericWidth::F16), 2048.0);
    assert_eq!(round_float_at_slot(65_519.0, NumericWidth::F16), 65_504.0);
    assert_eq!(
        round_float_at_slot(65_520.0, NumericWidth::F16),
        f64::INFINITY
    );
    assert_eq!(
        round_float_at_slot(0.1, NumericWidth::F16),
        0.099_975_585_937_5
    );
}

#[test]
fn f16_bf16_slot_round_bf16_uses_bfloat16_boundaries_and_ties() {
    assert_eq!(round_float_at_slot(257.0, NumericWidth::BF16), 256.0);
    assert_eq!(round_float_at_slot(1.001, NumericWidth::BF16), 1.0);
    assert_eq!(
        round_float_at_slot(0.1, NumericWidth::BF16),
        0.100_097_656_25
    );
    assert_eq!(
        round_float_at_slot(120_000.0, NumericWidth::BF16),
        119_808.0
    );

    let infinity_tie = (2.0 - 2f64.powi(-8)) * 2f64.powi(127);
    assert_eq!(
        round_float_at_slot(infinity_tie, NumericWidth::BF16),
        f64::INFINITY
    );
    assert_eq!(
        round_float_at_slot(-infinity_tie, NumericWidth::BF16),
        f64::NEG_INFINITY
    );
}

#[test]
fn f16_bf16_slot_round_preserves_nan_and_other_float_widths() {
    for width in [NumericWidth::F16, NumericWidth::BF16] {
        assert!(round_float_at_slot(f64::NAN, width).is_nan());
    }

    let negative_nan = f64::from(f32::from_bits(0xffc0_0001));
    let rounded_negative_nan = round_float_at_slot(negative_nan, NumericWidth::BF16);
    assert!(rounded_negative_nan.is_nan());
    assert!(rounded_negative_nan.is_sign_negative());

    let value = 1.000_000_119_209_289_6;
    assert_eq!(
        round_float_at_slot(value, NumericWidth::F32),
        f64::from(value as f32)
    );
    let f64_value = 1.000_000_000_000_000_2;
    assert_eq!(round_float_at_slot(f64_value, NumericWidth::F64), f64_value);
}
