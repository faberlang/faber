//! Every output of the `¶` renderers is pinned with a literal expected value.

use super::{Fmt, float, instans, int, magnus, magnus_sci, pad, sci, text};
use crate::Magnus;

/// The default spec: space fill, right aligned, no flags, no width, no
/// precision, default kind.
const BASE: Fmt = Fmt {
    fill: ' ',
    align: '>',
    plus: false,
    zero: false,
    width: 0,
    precision: None,
    kind: ' ',
};

fn width(width: usize) -> Fmt {
    Fmt { width, ..BASE }
}

fn kind(kind: char) -> Fmt {
    Fmt { kind, ..BASE }
}

fn precision(precision: usize) -> Fmt {
    Fmt {
        precision: Some(precision),
        ..BASE
    }
}

fn big(digits: &str) -> Magnus {
    digits.parse::<Magnus>().expect("digits")
}

#[test]
fn pad_aligns_with_the_fill_and_never_truncates() {
    assert_eq!(pad("", "42", width(5), false), "   42");
    assert_eq!(
        pad(
            "",
            "42",
            Fmt {
                align: '<',
                width: 5,
                ..BASE
            },
            false
        ),
        "42   "
    );
    // Centre: the extra fill goes to the right.
    assert_eq!(
        pad(
            "",
            "42",
            Fmt {
                align: '^',
                width: 5,
                ..BASE
            },
            false
        ),
        " 42  "
    );
    assert_eq!(
        pad(
            "",
            "42",
            Fmt {
                align: '^',
                width: 6,
                ..BASE
            },
            false
        ),
        "  42  "
    );
    assert_eq!(
        pad(
            "-",
            "7",
            Fmt {
                fill: '*',
                width: 4,
                ..BASE
            },
            false
        ),
        "**-7"
    );
    assert_eq!(pad("-", "12345", width(3), false), "-12345");
}

#[test]
fn zero_padding_goes_between_the_sign_and_the_body() {
    assert_eq!(pad("-", "42", width(6), true), "-00042");
    assert_eq!(pad("", "42", width(4), true), "0042");
    assert_eq!(pad("+", "42", width(2), true), "+42");
}

#[test]
fn sci_renders_the_exponent_with_or_without_precision() {
    assert_eq!(sci(1500.0, None), "1.5e3");
    assert_eq!(sci(1500.0, Some(2)), "1.50e3");
    assert_eq!(sci(1234.5, Some(1)), "1.2e3");
    assert_eq!(sci(0.0, None), "0e0");
    assert_eq!(sci(0.00125, None), "1.25e-3");
}

#[test]
fn int_prints_the_default_body_and_the_precision_pad() {
    assert_eq!(int(42, BASE), "42");
    assert_eq!(int(-42, BASE), "-42");
    assert_eq!(int(0, BASE), "0");
    assert_eq!(int(42, precision(2)), "42.00");
    assert_eq!(int(42, precision(0)), "42");
    assert_eq!(
        int(i128::MIN, BASE),
        "-170141183460469231731687303715884105728"
    );
}

#[test]
fn int_radix_kinds_print_sign_plus_magnitude() {
    assert_eq!(int(42, kind('x')), "2a");
    assert_eq!(int(-42, kind('x')), "-2a");
    assert_eq!(int(5, kind('b')), "101");
    assert_eq!(int(-5, kind('b')), "-101");
    assert_eq!(int(8, kind('o')), "10");
    assert_eq!(int(-8, kind('o')), "-10");
}

#[test]
fn int_scientific_goes_through_the_magnitude() {
    assert_eq!(int(1500, kind('e')), "1.5e3");
    assert_eq!(int(-1500, kind('e')), "-1.5e3");
    assert_eq!(
        int(
            1500,
            Fmt {
                kind: 'e',
                precision: Some(2),
                ..BASE
            }
        ),
        "1.50e3"
    );
}

#[test]
fn int_sign_flag_and_zero_pad() {
    assert_eq!(int(5, Fmt { plus: true, ..BASE }), "+5");
    assert_eq!(int(-5, Fmt { plus: true, ..BASE }), "-5");
    assert_eq!(int(0, Fmt { plus: true, ..BASE }), "+0");
    assert_eq!(
        int(
            -42,
            Fmt {
                zero: true,
                width: 6,
                ..BASE
            }
        ),
        "-00042"
    );
    assert_eq!(
        int(
            255,
            Fmt {
                zero: true,
                width: 6,
                kind: 'x',
                ..BASE
            }
        ),
        "0000ff"
    );
    assert_eq!(int(42, width(5)), "   42");
}

#[test]
fn float_non_finite_values_print_words_and_never_zero_pad() {
    assert_eq!(float(f64::NAN, "NaN", BASE), "NaN");
    assert_eq!(
        float(
            f64::NAN,
            "NaN",
            Fmt {
                zero: true,
                width: 6,
                ..BASE
            }
        ),
        "   NaN"
    );
    assert_eq!(float(f64::INFINITY, "inf", BASE), "∞");
    assert_eq!(float(f64::NEG_INFINITY, "inf", BASE), "-∞");
    assert_eq!(
        float(f64::INFINITY, "inf", Fmt { plus: true, ..BASE }),
        "+∞"
    );
    assert_eq!(
        float(
            f64::NEG_INFINITY,
            "inf",
            Fmt {
                zero: true,
                width: 5,
                ..BASE
            }
        ),
        "   -∞"
    );
}

#[test]
fn float_uses_the_display_body_unless_a_precision_or_kind_asks() {
    assert_eq!(float(3.5, "3.5", BASE), "3.5");
    assert_eq!(float(-3.5, "3.5", BASE), "-3.5");
    assert_eq!(float(1.23456, "1.23456", precision(2)), "1.23");
    assert_eq!(float(2.0, "2", precision(3)), "2.000");
    assert_eq!(float(1234.5, "1234.5", kind('e')), "1.2345e3");
    assert_eq!(
        float(
            1234.5,
            "1234.5",
            Fmt {
                kind: 'e',
                precision: Some(1),
                ..BASE
            }
        ),
        "1.2e3"
    );
    // A negative zero keeps its sign.
    assert_eq!(float(-0.0, "0", BASE), "-0");
}

#[test]
fn float_sign_flag_and_zero_pad() {
    assert_eq!(float(2.5, "2.5", Fmt { plus: true, ..BASE }), "+2.5");
    assert_eq!(
        float(
            -3.5,
            "3.5",
            Fmt {
                zero: true,
                width: 8,
                precision: Some(2),
                ..BASE
            }
        ),
        "-0003.50"
    );
    assert_eq!(float(3.5, "3.5", width(6)), "   3.5");
}

#[test]
fn text_truncates_to_the_precision_in_characters() {
    assert_eq!(text("hello", BASE), "hello");
    assert_eq!(text("héllo", precision(3)), "hél");
    assert_eq!(text("hi", precision(5)), "hi");
    assert_eq!(text("hi", precision(0)), "");
    assert_eq!(
        text(
            "hi",
            Fmt {
                align: '<',
                width: 5,
                ..BASE
            }
        ),
        "hi   "
    );
    // Text never zero-pads.
    assert_eq!(
        text(
            "hi",
            Fmt {
                zero: true,
                width: 4,
                ..BASE
            }
        ),
        "  hi"
    );
}

#[test]
fn instans_presets_split_the_iso_text() {
    let iso = "2024-03-05T12:34:56.789+00:00";
    assert_eq!(instans(iso.to_owned(), 'd'), "2024-03-05");
    assert_eq!(instans(iso.to_owned(), 't'), "12:34:56");
    assert_eq!(instans(iso.to_owned(), 'i'), iso);
    assert_eq!(instans("2024-03-05".to_owned(), 't'), "");
    assert_eq!(instans("12".to_owned(), 'd'), "12");
}

#[test]
fn magnus_prints_digits_radix_kinds_and_the_precision_pad() {
    let two_100 = big("1267650600228229401496703205376");
    assert_eq!(magnus(&two_100, BASE), "1267650600228229401496703205376");
    assert_eq!(
        magnus(&two_100, precision(2)),
        "1267650600228229401496703205376.00"
    );
    assert_eq!(magnus(&two_100, kind('x')), "10000000000000000000000000");
    assert_eq!(magnus(&big("-255"), kind('x')), "-ff");
    assert_eq!(magnus(&big("5"), kind('b')), "101");
    assert_eq!(magnus(&big("-8"), kind('o')), "-10");
    assert_eq!(magnus(&big("255"), precision(0)), "255");
}

#[test]
fn magnus_sign_flag_and_zero_pad() {
    assert_eq!(magnus(&big("5"), Fmt { plus: true, ..BASE }), "+5");
    assert_eq!(magnus(&big("-5"), Fmt { plus: true, ..BASE }), "-5");
    assert_eq!(
        magnus(
            &big("-42"),
            Fmt {
                zero: true,
                width: 6,
                ..BASE
            }
        ),
        "-00042"
    );
    assert_eq!(magnus(&big("42"), width(5)), "   42");
}

#[test]
fn magnus_scientific_within_f64_matches_the_float_body() {
    assert_eq!(magnus(&big("12345"), kind('e')), "1.2345e4");
    assert_eq!(magnus(&big("-1500"), kind('e')), "-1.5e3");
    assert_eq!(magnus_sci(&big("1500"), Some(2)), "1.50e3");
}

#[test]
fn magnus_scientific_beyond_f64_renders_from_the_digits() {
    // 123 followed by 397 zeros: 400 digits, exponent 399, beyond `f64`.
    let huge = big(&format!("123{}", "0".repeat(397)));
    assert_eq!(magnus_sci(&huge, None), "1.23e399");
    assert_eq!(magnus_sci(&huge, Some(0)), "1e399");
    assert_eq!(magnus_sci(&huge, Some(1)), "1.2e399");
    assert_eq!(magnus_sci(&huge, Some(5)), "1.23000e399");
    // A coefficient of one digit has no point.
    let ten_400 = big(&format!("1{}", "0".repeat(400)));
    assert_eq!(magnus_sci(&ten_400, None), "1e400");
    assert_eq!(
        magnus(
            &-&huge,
            Fmt {
                kind: 'e',
                plus: true,
                ..BASE
            }
        ),
        "-1.23e399"
    );
}
