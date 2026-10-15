use super::{
    FormatSpec, parse_format_spec, render_decimal, render_float, render_integer, render_text,
};

fn layout(spec: &str) -> super::LayoutSpec {
    match parse_format_spec(spec) {
        Ok(FormatSpec::Layout(layout)) => layout,
        other => panic!("{spec:?} did not parse as a layout: {other:?}"),
    }
}

#[test]
fn integers_pad_and_change_radix() {
    assert_eq!(render_integer(255, &layout("08x")), "000000ff");
    assert_eq!(render_integer(-42, &layout("x")), "-2a");
    assert_eq!(render_integer(42, &layout(".2")), "42.00");
    assert_eq!(render_integer(7, &layout("*^5")), "**7**");
}

#[test]
fn floats_decimals_and_text_follow_the_reference_rules() {
    assert_eq!(
        render_float(f64::NAN, &layout("08"), |_| unreachable!()),
        "     NaN"
    );
    assert_eq!(render_float(2.5, &layout(".1"), |_| unreachable!()), "2.5");
    assert_eq!(render_decimal(1_250_000_000, 8, &layout(".1")), "12.5");
    assert_eq!(render_decimal(1_250_000_000, 8, &layout("")), "12.5");
    assert_eq!(render_text("abcdef", &layout(">8.3")), "     abc");
}

#[test]
fn presets_and_malformed_specs_are_told_apart() {
    assert!(matches!(
        parse_format_spec("iso"),
        Ok(FormatSpec::Preset(_))
    ));
    assert!(parse_format_spec("5q").is_err());
}
