//! Table-driven proofs for the `¶` spec vocabulary (D2.5, D2.7).

use super::*;

fn layout(text: &str) -> LayoutSpec {
    match parse_format_spec(text) {
        Ok(FormatSpec::Layout(spec)) => spec,
        other => panic!("{text:?} should parse as a layout spec, got {other:?}"),
    }
}

#[test]
fn parses_every_part_of_the_pattern() {
    let spec = layout("*^+08.3e");
    assert_eq!(spec.fill, '*');
    assert_eq!(spec.align, Some(FormatAlign::Center));
    assert!(spec.plus);
    assert!(spec.zero);
    assert_eq!(spec.width, Some(8));
    assert_eq!(spec.precision, Some(3));
    assert_eq!(spec.kind, Some(FormatKind::Scientific));
}

#[test]
fn parse_table() {
    let cases: &[(&str, LayoutSpec)] = &[
        ("", LayoutSpec::default()),
        (
            ".2",
            LayoutSpec {
                precision: Some(2),
                ..LayoutSpec::default()
            },
        ),
        (
            "5",
            LayoutSpec {
                width: Some(5),
                ..LayoutSpec::default()
            },
        ),
        (
            "05",
            LayoutSpec {
                zero: true,
                width: Some(5),
                ..LayoutSpec::default()
            },
        ),
        (
            "08x",
            LayoutSpec {
                zero: true,
                width: Some(8),
                kind: Some(FormatKind::Hex),
                ..LayoutSpec::default()
            },
        ),
        (
            "<6",
            LayoutSpec {
                align: Some(FormatAlign::Left),
                width: Some(6),
                ..LayoutSpec::default()
            },
        ),
        (
            "<<3",
            LayoutSpec {
                fill: '<',
                align: Some(FormatAlign::Left),
                width: Some(3),
                ..LayoutSpec::default()
            },
        ),
        (
            "·>4",
            LayoutSpec {
                fill: '·',
                align: Some(FormatAlign::Right),
                width: Some(4),
                ..LayoutSpec::default()
            },
        ),
        (
            "+",
            LayoutSpec {
                plus: true,
                ..LayoutSpec::default()
            },
        ),
        (
            "b",
            LayoutSpec {
                kind: Some(FormatKind::Binary),
                ..LayoutSpec::default()
            },
        ),
        (
            "o",
            LayoutSpec {
                kind: Some(FormatKind::Octal),
                ..LayoutSpec::default()
            },
        ),
        (
            "10",
            LayoutSpec {
                width: Some(10),
                ..LayoutSpec::default()
            },
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(layout(text), *expected, "spec {text:?}");
    }
}

#[test]
fn parses_named_presets() {
    assert_eq!(
        parse_format_spec("iso"),
        Ok(FormatSpec::Preset(InstansPreset::Iso))
    );
    assert_eq!(
        parse_format_spec("date"),
        Ok(FormatSpec::Preset(InstansPreset::Date))
    );
    assert_eq!(
        parse_format_spec("time"),
        Ok(FormatSpec::Preset(InstansPreset::Time))
    );
}

#[test]
fn malformed_table() {
    for text in [
        ".",
        "2.",
        "xx",
        "5x2",
        "+-",
        "%Y-%m-%d",
        "ISO",
        "5 ",
        "..2",
        "99999999999",
        "^",
    ] {
        // `^` alone is a valid align; everything else is malformed.
        if text == "^" {
            assert!(parse_format_spec(text).is_ok());
            continue;
        }
        assert_eq!(
            parse_format_spec(text),
            Err(FormatSpecError::Malformed),
            "spec {text:?}"
        );
    }
}

#[test]
fn validate_table() {
    use FormatSpecError::{PartNotAllowed, PresetNotAllowed, PresetRequired};
    use FormatSpecPart::{Kind, Precision, Sign, Zero};
    use FormatSubject::{Decimal, Float, Instans, Signed, Text, Unsigned};
    let cases: &[(&str, FormatSubject, Result<(), FormatSpecError>)] = &[
        (".2", Signed, Ok(())),
        ("08x", Unsigned, Ok(())),
        ("+e", Signed, Ok(())),
        (".2x", Signed, Err(PartNotAllowed(Precision))),
        ("*^7", Signed, Ok(())),
        ("<05", Signed, Err(PartNotAllowed(Zero))),
        (".2", Float, Ok(())),
        ("+010.3e", Float, Ok(())),
        ("x", Float, Err(PartNotAllowed(Kind))),
        (".2", Decimal, Ok(())),
        ("*>+10.3", Decimal, Ok(())),
        ("x", Decimal, Err(PartNotAllowed(Kind))),
        ("e", Decimal, Ok(())),
        (".3e", Decimal, Ok(())),
        ("<05", Decimal, Err(PartNotAllowed(Zero))),
        ("iso", Decimal, Err(PresetNotAllowed)),
        ("*<8.3", Text, Ok(())),
        ("+", Text, Err(PartNotAllowed(Sign))),
        ("05", Text, Err(PartNotAllowed(Zero))),
        ("x", Text, Err(PartNotAllowed(Kind))),
        ("iso", Instans, Ok(())),
        ("date", Instans, Ok(())),
        (".2", Instans, Err(PresetRequired)),
        ("iso", Signed, Err(PresetNotAllowed)),
        ("time", Text, Err(PresetNotAllowed)),
    ];
    for (text, subject, expected) in cases {
        let spec = parse_format_spec(text).expect("parses");
        assert_eq!(
            validate_format_spec(&spec, *subject),
            *expected,
            "{text:?} on {subject:?}"
        );
    }
}

#[test]
fn integer_render_table() {
    let cases: &[(i128, &str, &str)] = &[
        (42, "", "42"),
        (42, ".2", "42.00"),
        (-42, ".2", "-42.00"),
        (42, ".0", "42"),
        (42, "5", "   42"),
        (42, "05", "00042"),
        (-42, "05", "-0042"),
        (42, "<5", "42   "),
        (42, "*^7", "**42***"),
        (42, "^5", " 42  "),
        (42, "+", "+42"),
        (0, "+", "+0"),
        (255, "x", "ff"),
        (255, "08x", "000000ff"),
        (-42, "x", "-2a"),
        (5, "b", "101"),
        (8, "o", "10"),
        (-8, "o", "-10"),
        (4200, "e", "4.2e3"),
        (4200, ".2e", "4.20e3"),
        (123_456, "3", "123456"),
        (i128::from(u64::MAX), "x", "ffffffffffffffff"),
        (i128::from(i64::MIN), "", "-9223372036854775808"),
    ];
    for (value, text, expected) in cases {
        assert_eq!(
            render_integer(*value, &layout(text)),
            *expected,
            "{value} ¶ {text:?}"
        );
    }
}

fn plain(v: f64) -> String {
    v.to_string()
}

#[test]
fn float_render_table() {
    let cases: &[(f64, &str, &str)] = &[
        (12.5, ".2", "12.50"),
        (3.1421, ".3", "3.142"),
        (-3.1421, ".1", "-3.1"),
        (1.5, "", "1.5"),
        (1.5, "8.2", "    1.50"),
        (1.5, "08.2", "00001.50"),
        (-1.5, "08.2", "-0001.50"),
        (1.5, "+.1", "+1.5"),
        (1234.5, "e", "1.2345e3"),
        (0.00012, ".1e", "1.2e-4"),
        (1.5, "<6", "1.5   "),
        (f64::NAN, ".2", "NaN"),
        (f64::NAN, "5", "  NaN"),
        (f64::INFINITY, ".2", "∞"),
        (f64::NEG_INFINITY, "", "-∞"),
        (f64::INFINITY, "+", "+∞"),
        (f64::NEG_INFINITY, "05", "   -∞"),
    ];
    for (value, text, expected) in cases {
        assert_eq!(
            render_float(*value, &layout(text), plain),
            *expected,
            "{value} ¶ {text:?}"
        );
    }
}

#[test]
fn text_render_table() {
    let cases: &[(&str, &str, &str)] = &[
        ("Aurelia", ".3", "Aur"),
        ("Aurelia", "", "Aurelia"),
        ("ab", "5", "ab   "),
        ("ab", ">5", "   ab"),
        ("ab", "*^6", "**ab**"),
        ("Aurelia", "3", "Aurelia"),
        ("Aurelia", "*>6.3", "***Aur"),
        ("café", "6", "café  "),
        // 👍🏽 is two scalars (thumbs up + skin tone), so width counts 2.
        ("👍🏽", "4", "👍🏽  "),
        ("👍🏽", ".1", "👍"),
    ];
    for (value, text, expected) in cases {
        assert_eq!(
            render_text(value, &layout(text)),
            *expected,
            "{value:?} ¶ {text:?}"
        );
    }
}

#[test]
fn instans_preset_table() {
    let cases: &[(&str, InstansPreset, &str)] = &[
        (
            "1979-05-27T07:32:00Z",
            InstansPreset::Iso,
            "1979-05-27T07:32:00Z",
        ),
        ("1979-05-27T07:32:00Z", InstansPreset::Date, "1979-05-27"),
        ("1979-05-27T07:32:00.123Z", InstansPreset::Time, "07:32:00"),
        (
            "+10000-01-01T00:00:00Z",
            InstansPreset::Date,
            "+10000-01-01",
        ),
    ];
    for (iso, preset, expected) in cases {
        assert_eq!(
            render_instans_preset(iso, *preset),
            *expected,
            "{iso} ¶ {preset:?}"
        );
    }
}

// inf track U4 (ruling 42): `inf` is a signed integer subject that renders
// through the exact digits of a `Magnus`.

#[test]
fn inf_is_a_signed_format_subject() {
    let mut types = TypeTable::new();
    let inf = types.sized_numeric(Primitive::Numerus, NumericWidth::Inf);
    assert_eq!(format_subject(&types, inf), FormatSubject::Signed);
    let u64_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U64);
    assert_eq!(format_subject(&types, u64_ty), FormatSubject::Unsigned);
}

#[test]
fn render_magnus_matches_render_integer_inside_the_i128_range() {
    let values: [i128; 9] = [
        0,
        7,
        -7,
        255,
        -255,
        1 << 70,
        -(1 << 70),
        i128::from(u64::MAX),
        i128::from(i64::MIN),
    ];
    for text in [
        "", "x", "b", "o", "5", "<8", ">12", "^9", "*>10", "+", "+08", "010", ".2", "e", ".3e",
    ] {
        let spec = layout(text);
        for value in values {
            assert_eq!(
                render_magnus(&Magnus::from_i128(value), &spec),
                render_integer(value, &spec),
                "{value} ¶ {text:?}"
            );
        }
    }
}

#[test]
fn render_magnus_handles_values_beyond_every_machine_integer() {
    let beyond = Magnus::parse_decimal("18446744073709551616").expect("2^64");
    assert_eq!(render_magnus(&beyond, &layout("x")), "10000000000000000");
    assert_eq!(
        render_magnus(&-beyond.clone(), &layout("x")),
        "-10000000000000000"
    );
    assert_eq!(render_magnus(&beyond, &layout("b")).len(), 65);
    assert_eq!(
        render_magnus(&beyond, &layout("o")),
        "2000000000000000000000"
    );
    assert_eq!(
        render_magnus(&beyond, &layout(".2")),
        "18446744073709551616.00"
    );
    assert_eq!(
        render_magnus(&beyond, &layout(">24")),
        "    18446744073709551616"
    );
    assert_eq!(
        render_magnus(&-beyond.clone(), &layout("024")),
        "-00018446744073709551616"
    );
    assert_eq!(
        render_magnus(&beyond, &layout("+")),
        "+18446744073709551616"
    );
    // `e` on a value the f64 path holds agrees with the integer renderer.
    assert_eq!(
        render_magnus(&beyond, &layout("e")),
        "1.8446744073709552e19"
    );
    // Past f64 range the coefficient comes from the exact digits.
    let huge = Magnus::parse_decimal(&format!("1{}", "0".repeat(400))).expect("10^400");
    assert_eq!(render_magnus(&huge, &layout("e")), "1e400");
    assert_eq!(render_magnus(&huge, &layout(".2e")), "1.00e400");
    let other = Magnus::parse_decimal(&format!("12345{}", "0".repeat(400))).expect("1.2345e404");
    assert_eq!(render_magnus(&other, &layout("e")), "1.2345e404");
    assert_eq!(render_magnus(&other, &layout(".1e")), "1.2e404");
}

#[test]
fn decimal_render_table() {
    // (carrier, scale, spec, expected)
    let cases: &[(i128, u32, &str, &str)] = &[
        (12_345, 1, "e", "1.2345e3"),
        (12_345, 1, ".3e", "1.234e3"),
        (125, 1, "e", "1.25e1"),
        (125, 1, "+e", "+1.25e1"),
        (-125, 1, "e", "-1.25e1"),
        (1, 8, "e", "1e-8"),
        (0, 8, "e", "0e0"),
        (0, 8, ".3e", "0.000e0"),
        (9_995, 2, ".1e", "1.0e2"),
        (1_234_500, 3, ".2e", "1.23e3"),
        (100, 0, "e", "1e2"),
        (100, 0, ".2e", "1.00e2"),
        // Stored scale 8: 12.5, 0.25, 0.35, 3.375.
        (1_250_000_000, 8, ".2", "12.50"),
        (25_000_000, 8, ".1", "0.2"),
        (35_000_000, 8, ".1", "0.4"),
        (337_500_000, 8, ".2", "3.38"),
        (1_250_000_000, 8, ">8", "    12.5"),
        // Half-even ties: the even neighbour wins.
        (25, 2, ".1", "0.2"),
        (35, 2, ".1", "0.4"),
        (15, 1, ".0", "2"),
        (25, 1, ".0", "2"),
        (-25, 1, ".0", "-2"),
        // Above the half the value rounds up, below it down.
        (26, 2, ".1", "0.3"),
        (24, 2, ".1", "0.2"),
        (251, 3, ".1", "0.3"),
        (249, 3, ".1", "0.2"),
        // A carry through the digits.
        (995, 3, ".2", "1.00"),
        (9_995, 3, ".2", "10.00"),
        // No precision: the shortest form.
        (1_250_000_000, 8, "", "12.5"),
        (300_000_000, 8, "", "3"),
        (0, 8, "", "0"),
        (-1, 8, "", "-0.00000001"),
        // Precision past the scale pads zeros.
        (125, 1, ".3", "12.500"),
        (5, 0, ".2", "5.00"),
        // Sign, zero, fill and align.
        (1_250_000_000, 8, "+", "+12.5"),
        (-1_250_000_000, 8, "010.2", "-000012.50"),
        (1_250_000_000, 8, "*^9.1", "**12.5***"),
        (1_250_000_000, 8, "<8", "12.5    "),
        // A negative value that rounds to zero keeps its sign.
        (-4, 2, ".1", "-0.0"),
        // Exact past f64 and past a scale-8 carrier.
        (
            123_456_789_012_345_678_901_234_567_890,
            20,
            ".3",
            "1234567890.123",
        ),
        (1, 38, ".2", "0.00"),
        (5, 39, ".0", "0"),
        (i128::MIN, 0, "", "-170141183460469231731687303715884105728"),
    ];
    for (carrier, scale, text, expected) in cases {
        assert_eq!(
            render_decimal(*carrier, *scale, &layout(text)),
            *expected,
            "{carrier}e-{scale} with {text:?}"
        );
    }
}
