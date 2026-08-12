//! Scalar [`FromValor`] extraction tests for `valor.rs`.

use crate::valor::{FromValor, Valor};
use crate::{Ascii, Instans, InstansPraecisio};

#[test]
fn from_valor_extracts_nihil() {
    assert_eq!(<() as FromValor>::from_valor(&Valor::Nihil), Some(()));
    assert_eq!(<() as FromValor>::from_valor(&Valor::Numerus(1)), None);
}

#[test]
fn from_valor_extracts_bool() {
    assert_eq!(bool::from_valor(&Valor::Bivalens(true)), Some(true));
    assert_eq!(bool::from_valor(&Valor::Bivalens(false)), Some(false));
}

#[test]
fn from_valor_bool_rejects_non_bivalens() {
    assert_eq!(bool::from_valor(&Valor::Numerus(1)), None);
    assert_eq!(bool::from_valor(&Valor::Nihil), None);
    assert_eq!(bool::from_valor(&Valor::Textus("true".into())), None);
}

#[test]
fn from_valor_extracts_i64() {
    assert_eq!(i64::from_valor(&Valor::Numerus(42)), Some(42));
    assert_eq!(i64::from_valor(&Valor::Numerus(-1)), Some(-1));
    assert_eq!(i64::from_valor(&Valor::Numerus(0)), Some(0));
}

#[test]
fn from_valor_i64_rejects_non_numerus() {
    assert_eq!(i64::from_valor(&Valor::Fractus(3.0)), None);
    assert_eq!(i64::from_valor(&Valor::Textus("42".into())), None);
    assert_eq!(i64::from_valor(&Valor::Nihil), None);
}

#[test]
fn from_valor_extracts_f64() {
    assert_eq!(f64::from_valor(&Valor::Fractus(1.5)), Some(1.5));
    assert_eq!(f64::from_valor(&Valor::Fractus(0.0)), Some(0.0));
    assert_eq!(f64::from_valor(&Valor::Fractus(-3.25)), Some(-3.25));
}

#[test]
fn from_valor_extracts_string() {
    assert_eq!(
        String::from_valor(&Valor::Textus("hi".into())),
        Some("hi".into())
    );
    assert_eq!(
        String::from_valor(&Valor::Textus("".into())),
        Some("".into())
    );
}

#[test]
fn from_valor_string_rejects_non_textus() {
    assert_eq!(String::from_valor(&Valor::Numerus(1)), None);
    assert_eq!(String::from_valor(&Valor::Nihil), None);
}

#[test]
fn from_valor_fractus_widens_numerus() {
    assert_eq!(f64::from_valor(&Valor::Numerus(7)), Some(7.0));
    assert_eq!(f64::from_valor(&Valor::Textus("x".into())), None);
}

#[test]
fn from_valor_u8_accepts_byte_range_numerus() {
    assert_eq!(u8::from_valor(&Valor::Numerus(0)), Some(0));
    assert_eq!(u8::from_valor(&Valor::Numerus(255)), Some(255));
    assert_eq!(u8::from_valor(&Valor::Numerus(256)), None);
    assert_eq!(u8::from_valor(&Valor::Numerus(-1)), None);
}

#[test]
fn from_valor_textus_accepts_instans_wire() {
    let wire = "1979-05-27T07:32:00Z".to_string();
    assert_eq!(
        String::from_valor(&Valor::Instans(wire.clone())),
        Some(wire)
    );
}

#[test]
fn from_valor_ascii_extracts_valid_textus() {
    assert_eq!(
        Ascii::from_valor(&Valor::Textus("ascii".into())),
        Some(Ascii::new("ascii"))
    );
}

#[test]
fn from_valor_ascii_extracts_valid_instans_wire() {
    assert_eq!(
        Ascii::from_valor(&Valor::Instans("1979-05-27T07:32:00Z".into())),
        Some(Ascii::new("1979-05-27T07:32:00Z"))
    );
}

#[test]
fn from_valor_ascii_rejects_non_ascii_textus() {
    assert_eq!(Ascii::from_valor(&Valor::Textus("π".into())), None);
    assert_eq!(Ascii::from_valor(&Valor::Textus("über".into())), None);
}

#[test]
fn from_valor_ascii_accepts_empty_textus() {
    assert_eq!(
        Ascii::from_valor(&Valor::Textus("".into())),
        Some(Ascii::new(""))
    );
}

#[test]
fn from_valor_ascii_rejects_non_textus() {
    assert_eq!(Ascii::from_valor(&Valor::Numerus(42)), None);
    assert_eq!(Ascii::from_valor(&Valor::Bivalens(true)), None);
    assert_eq!(Ascii::from_valor(&Valor::Nihil), None);
}

#[test]
fn from_valor_instans_uses_bare_seconds_precision() {
    let instant = Instans::from_valor(&Valor::Instans("1979-05-27T07:32:00.123Z".into()))
        .expect("instans wire");
    assert_eq!(instant.praecisio(), InstansPraecisio::Secunda);
    assert_eq!(instant.to_rfc3339(), "1979-05-27T07:32:00Z");
}

#[test]
fn from_valor_extracts_valor_identity() {
    let tab = Valor::Tabula(std::collections::BTreeMap::from([(
        "k".into(),
        Valor::Numerus(1),
    )]));
    assert_eq!(Valor::from_valor(&tab), Some(tab.clone()));
    assert_eq!(
        Vec::<Valor>::from_valor(&Valor::Lista(vec![Valor::Numerus(2)])),
        Some(vec![Valor::Numerus(2)])
    );
}

#[test]
fn from_option_valor_maps_none_to_nihil() {
    assert_eq!(Valor::from(None), Valor::Nihil);
    assert_eq!(
        Valor::from(Some(Valor::Textus("x".into()))),
        Valor::Textus("x".into())
    );
}
