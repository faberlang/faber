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
