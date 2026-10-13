//! D11.7 pins for the exact decimal helpers: canonical form, display, compare.

use super::*;
use crate::Value;

#[test]
fn canonical_returns_a_stored_carrier_only_when_the_value_is_exactly_one() {
    // 1.449275 at scale 16 drops its zeros and rescales to a scale-8 carrier.
    assert_eq!(
        canonical(14_492_750_000_000_000, 16),
        Value::Int(144_927_500)
    );
    // 0.500000005 needs nine fraction digits.
    assert_eq!(canonical(500_000_005, 9), Value::Dec(500_000_005, 9));
    // Past the i64 carrier the value stays an intermediate.
    assert_eq!(
        canonical(i128::from(i64::MAX) + 1, 8),
        Value::Dec(i128::from(i64::MAX) + 1, 8)
    );
    // A coarser scale widens to scale 8.
    assert_eq!(canonical(5, 0), Value::Int(500_000_000));
}

#[test]
fn format_is_the_shortest_exact_form() {
    assert_eq!(format(144_927_500, 8), "1.449275");
    assert_eq!(format(1_050_724_375, 10), "0.1050724375");
    assert_eq!(format(-5, 1), "-0.5");
    assert_eq!(format(12, 0), "12");
    assert_eq!(format(1_200, 2), "12");
    assert_eq!(format(0, 8), "0");
}

#[test]
fn compare_aligns_scales_and_survives_overflow() {
    assert_eq!(compare((15, 1), (150, 2)), Ordering::Equal);
    assert_eq!(compare((1, 16), (1, 8)), Ordering::Less);
    assert_eq!(compare((i128::MAX, 0), (1, 30)), Ordering::Greater);
    assert_eq!(compare((i128::MIN, 0), (1, 30)), Ordering::Less);
}

#[test]
fn compare_with_a_zero_carrier_past_the_power_of_ten_gap_orders_by_the_other_sign() {
    // Scale 48 against scale 8 is a gap of 40: `10^40` overflows the i128
    // carrier, so a zero carrier cannot be rescaled by multiplication.
    let wide = |carrier: i128| (carrier, 48);
    let stored = |carrier: i128| (carrier, 8);

    // positive vs zero, zero vs positive
    assert_eq!(compare(wide(1), stored(0)), Ordering::Greater);
    assert_eq!(compare(stored(0), wide(1)), Ordering::Less);
    // negative vs zero, zero vs negative
    assert_eq!(compare(wide(-1), stored(0)), Ordering::Less);
    assert_eq!(compare(stored(0), wide(-1)), Ordering::Greater);
    // the zero is the wide side
    assert_eq!(compare(wide(0), stored(1)), Ordering::Less);
    assert_eq!(compare(stored(1), wide(0)), Ordering::Greater);
    assert_eq!(compare(wide(0), stored(-1)), Ordering::Greater);
    assert_eq!(compare(stored(-1), wide(0)), Ordering::Less);
    // equal zero at different scales
    assert_eq!(compare(wide(0), stored(0)), Ordering::Equal);
    assert_eq!(compare(stored(0), wide(0)), Ordering::Equal);
}

#[test]
fn store_rounds_half_even_and_checks_the_carrier_range() {
    assert_eq!(store_carrier(25, 9), Some(2)); // 0.000000025 -> 2 (even tie)
    assert_eq!(store_carrier(35, 9), Some(4)); // 0.000000035 -> 4
    assert_eq!(store_carrier(-25, 9), Some(-2));
    assert_eq!(store_carrier(i128::from(i64::MAX), 8), Some(i64::MAX));
    assert_eq!(store_carrier(i128::from(i64::MAX) + 1, 8), None);
    // Just past the range at scale 9 rounds back into it.
    assert_eq!(
        store_carrier(i128::from(i64::MAX) * 10 + 4, 9),
        Some(i64::MAX)
    );
}
