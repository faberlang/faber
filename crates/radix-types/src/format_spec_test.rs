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
    use FormatSubject::{Float, Instans, Signed, Text, Unsigned};
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
        (u64::MAX as i128, "x", "ffffffffffffffff"),
        (i64::MIN as i128, "", "-9223372036854775808"),
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
        (3.14159, ".3", "3.142"),
        (-3.14159, ".1", "-3.1"),
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
