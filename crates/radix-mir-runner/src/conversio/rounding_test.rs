//! Rounding modes of the float and decimal to integer conversion.

use super::rounding::RoundingMode::{self, Ceil, Floor, HalfEven, Trunc};

#[test]
fn lowercase_rounding_words() {
    // The conversion policy words are canonically lower case; the retired
    // title-case spelling stays dual-accepted for one phase and maps to the
    // same mode.
    for (lower, title) in [
        ("floor", "Floor"),
        ("ceil", "Ceil"),
        ("half_even", "HalfEven"),
        ("trunc", "Trunc"),
    ] {
        let lower_mode = RoundingMode::from_hint(lower).unwrap_or_else(|| panic!("{lower}"));
        let title_mode = RoundingMode::from_hint(title).unwrap_or_else(|| panic!("{title}"));
        assert_eq!(lower_mode, title_mode, "{lower} vs {title}");
        for value in [2.5, 3.5, -2.5, -0.5, 0.5, 3.7, -3.7] {
            assert_eq!(
                lower_mode.round_float(value),
                title_mode.round_float(value),
                "{lower} {value}"
            );
        }
        assert_eq!(
            lower_mode.round_quotient(25, 10),
            title_mode.round_quotient(25, 10),
            "{lower} quotient"
        );
    }
}

fn quotients(mode: RoundingMode, numerator: i128, denominator: i128) -> Option<i128> {
    mode.round_quotient(numerator, denominator)
}

#[test]
fn float_modes_round_ties_and_negatives() {
    // (value, floor, ceil, half-even, trunc)
    let rows = [
        (2.5, 2.0, 3.0, 2.0, 2.0),
        (3.5, 3.0, 4.0, 4.0, 3.0),
        (-2.5, -3.0, -2.0, -2.0, -2.0),
        (-0.5, -1.0, 0.0, 0.0, 0.0),
        (0.5, 0.0, 1.0, 0.0, 0.0),
        (3.7, 3.0, 4.0, 4.0, 3.0),
        (-3.7, -4.0, -3.0, -4.0, -3.0),
    ];
    for (value, floor, ceil, half_even, trunc) in rows {
        assert_eq!(Floor.round_float(value), floor, "floor {value}");
        assert_eq!(Ceil.round_float(value), ceil, "ceil {value}");
        assert_eq!(HalfEven.round_float(value), half_even, "half-even {value}");
        assert_eq!(Trunc.round_float(value), trunc, "trunc {value}");
    }
    for mode in [Floor, Ceil, HalfEven, Trunc] {
        assert!(mode.round_float(f64::NAN).is_nan());
        assert!(mode.round_float(f64::INFINITY).is_infinite());
    }
}

#[test]
fn quotient_modes_round_ties_and_negatives() {
    // value = numerator / 10: 25 is 2.5, -5 is -0.5.
    let rows = [
        (25, 2, 3, 2, 2),
        (35, 3, 4, 4, 3),
        (-25, -3, -2, -2, -2),
        (-5, -1, 0, 0, 0),
        (5, 0, 1, 0, 0),
        (37, 3, 4, 4, 3),
        (-37, -4, -3, -4, -3),
        (30, 3, 3, 3, 3),
        (-30, -3, -3, -3, -3),
    ];
    for (numerator, floor, ceil, half_even, trunc) in rows {
        assert_eq!(
            quotients(Floor, numerator, 10),
            Some(floor),
            "floor {numerator}"
        );
        assert_eq!(
            quotients(Ceil, numerator, 10),
            Some(ceil),
            "ceil {numerator}"
        );
        assert_eq!(
            quotients(HalfEven, numerator, 10),
            Some(half_even),
            "half-even {numerator}"
        );
        assert_eq!(
            quotients(Trunc, numerator, 10),
            Some(trunc),
            "trunc {numerator}"
        );
    }
    assert_eq!(Floor.round_quotient(1, 0), None);
}
