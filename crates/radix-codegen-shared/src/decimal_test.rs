//! Decimal carrier math pins for codegen tiers (decimal-widths D5).

use super::{
    carrier_in_width, decimal_scale_factor, decimal_text_to_carrier, round_half_even_quotient,
};
use radix_types::NumericWidth;

#[test]
fn decimal_scale_factors_match_the_cto_ruling() {
    assert_eq!(decimal_scale_factor(NumericWidth::D64), Some(1_000_000_000));
    assert_eq!(decimal_scale_factor(NumericWidth::I32), None);
}

#[test]
fn round_half_even_quotient_pins() {
    assert_eq!(round_half_even_quotient(5, 2), Some(2));
    assert_eq!(round_half_even_quotient(7, 2), Some(4));
    assert_eq!(round_half_even_quotient(-5, 2), Some(-2));
    assert_eq!(round_half_even_quotient(-7, 2), Some(-4));
    assert_eq!(round_half_even_quotient(6, 2), Some(3));
    assert_eq!(round_half_even_quotient(1, 0), None);
}

#[test]
fn decimal_text_to_carrier_rounds_half_even_at_the_width_scale() {
    // d64 scale = 9 fractional digits: 2.0000000005 → 2000000000 (even tie
    // down), 2.0000000015 → 2000000002 (odd tie up).
    assert_eq!(
        decimal_text_to_carrier("2.0000000005", NumericWidth::D64),
        Some(2_000_000_000)
    );
    assert_eq!(
        decimal_text_to_carrier("2.0000000015", NumericWidth::D64),
        Some(2_000_000_002)
    );
    assert_eq!(
        decimal_text_to_carrier("4.2", NumericWidth::D64),
        Some(4_200_000_000)
    );
    assert_eq!(
        decimal_text_to_carrier("42", NumericWidth::D64),
        Some(42_000_000_000)
    );
}

#[test]
fn carrier_in_width_rejects_out_of_range_values() {
    assert_eq!(
        carrier_in_width(i128::from(i64::MAX) + 1, NumericWidth::D64),
        None
    );
    assert_eq!(
        carrier_in_width(i128::from(i64::MAX), NumericWidth::D64),
        Some(i64::MAX)
    );
    assert_eq!(carrier_in_width(-1, NumericWidth::I32), None);
}
