//! `Valor::Magnus` carrier tests: canonical form, extraction, display and JSON wire.

use crate::display::display_valor;
use crate::valor::{FromValor, Valor};
use crate::{Json, Magnus};

fn big() -> Magnus {
    Magnus::parse_decimal("18446744073709551616").expect("2^64")
}

#[test]
fn from_magnus_is_canonical_numerus_when_it_fits_i64() {
    assert_eq!(Valor::from(Magnus::from_i64(-7)), Valor::Numerus(-7));
    assert_eq!(
        Valor::from(Magnus::from_i64(i64::MAX)),
        Valor::Numerus(i64::MAX)
    );
    assert_eq!(Valor::from(big()), Valor::Magnus(big()));
}

#[test]
fn from_valor_extracts_magnus_from_either_integer_carrier() {
    assert_eq!(
        Magnus::from_valor(&Valor::Numerus(5)),
        Some(Magnus::from_i64(5))
    );
    assert_eq!(Magnus::from_valor(&Valor::Magnus(big())), Some(big()));
    assert_eq!(Magnus::from_valor(&Valor::Fractus(1.0)), None);
    assert_eq!(i64::from_valor(&Valor::Magnus(big())), None);
}

#[test]
fn display_renders_the_decimal_digits() {
    assert_eq!(display_valor(&Valor::Magnus(big())), "18446744073709551616");
}

#[test]
fn json_renders_a_magnus_field_as_its_decimal_digits() {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("n".to_owned(), Valor::Magnus(big()));
    fields.insert("s".to_owned(), Valor::from(Magnus::from_i64(i64::MAX)));
    let json = Json::from_object(fields).expect("a Magnus field is valid JSON");
    assert_eq!(
        json.to_wire(),
        r#"{"n":18446744073709551616,"s":9223372036854775807}"#
    );
}

#[test]
fn json_parses_integer_tokens_of_any_length_into_the_canonical_carrier() {
    let json = Json::parse(
        r#"{"a":18446744073709551616,"b":-18446744073709551617,"c":9223372036854775808,"d":-9223372036854775808,"e":-5,"f":1.5}"#,
    )
    .expect("any-length integer tokens parse");
    let fields = json.as_object();
    let magnus = |text: &str| Valor::Magnus(Magnus::parse_decimal(text).expect("digits"));
    assert_eq!(fields["a"], magnus("18446744073709551616"));
    assert_eq!(fields["b"], magnus("-18446744073709551617"));
    // One past `i64::MAX` is already a `Magnus`; `i64::MIN` still fits `Numerus`.
    assert_eq!(fields["c"], magnus("9223372036854775808"));
    assert_eq!(fields["d"], Valor::Numerus(i64::MIN));
    assert_eq!(fields["e"], Valor::Numerus(-5));
    assert_eq!(fields["f"], Valor::Fractus(1.5));
    assert_eq!(Magnus::from_valor(&fields["a"]), Some(big()));
}

#[test]
fn json_round_trips_a_thousand_digit_integer_as_a_bare_number() {
    let digits = "7".repeat(1000);
    let wire = format!(r#"{{"n":{digits},"xs":[{digits},-{digits}]}}"#);
    let json = Json::parse(&wire).expect("1000-digit token");
    assert_eq!(json.to_wire(), wire);
    assert_eq!(
        Magnus::from_valor(&json.as_object()["n"]).map(|n| n.to_string()),
        Some(digits)
    );
}
