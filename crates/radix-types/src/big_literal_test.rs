use super::*;

#[test]
fn decimal_beyond_u64_keeps_its_exact_value() {
    let value = parse_big_literal("18446744073709551616").expect("digits parse");
    assert_eq!(value.to_string(), "18446744073709551616");
    assert_eq!(value.to_u64(), None);
    assert_eq!(value.to_i128(), Some(1_i128 << 64));
}

#[test]
fn radix_prefixed_spellings_parse_in_their_radix() {
    let hex = parse_big_literal("0x10000000000000000").expect("hex");
    assert_eq!(hex.to_i128(), Some(1_i128 << 64));
    let bin = parse_big_literal(&format!("0b1{}", "0".repeat(64))).expect("bin");
    assert_eq!(bin.to_i128(), Some(1_i128 << 64));
    let oct = parse_big_literal("0o2000000000000000000000").expect("oct");
    assert_eq!(oct.to_i128(), Some(1_i128 << 64));
}

#[test]
fn a_literal_of_any_length_parses() {
    let digits = "9".repeat(400);
    let value = parse_big_literal(&digits).expect("400 digits");
    assert_eq!(value.to_string(), digits);
}

#[test]
fn malformed_text_has_no_value() {
    for text in ["", "0x", "-1", "+1", "12a", "0b102"] {
        assert!(parse_big_literal(text).is_none(), "{text:?}");
    }
}
