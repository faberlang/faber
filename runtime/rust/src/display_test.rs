use crate::Valor;
use crate::display::{
    bivalens, fractus, option, option_bivalens, option_fractus, option_vacuum, text_payload, valor,
};
use std::collections::BTreeMap;

#[test]
fn display_fractus_keeps_integral_decimal_marker() {
    assert_eq!(fractus(0.0_f64), "0.0");
    assert_eq!(fractus(1.0_f32), "1.0");
}

#[test]
fn display_fractus_preserves_width_native_stringification() {
    assert_eq!(fractus(3.25_f64), "3.25");
    assert_eq!(fractus(3.25_f32), "3.25");
}

#[test]
fn display_fractus_f32_integral_keeps_shortest_digits_plus_point_zero() {
    // The MIR runner (the oracle) prints an integral `f32` as its shortest
    // round-trip digits plus `.0`, never the exact binary expansion.
    assert_eq!(
        fractus(f32::MAX),
        "340282350000000000000000000000000000000.0"
    );
    assert_eq!(fractus(1.0e20_f32), "100000000000000000000.0");
    assert_eq!(fractus(16_777_216.0_f32), "16777216.0");
    assert_eq!(fractus(-0.0_f32), "-0.0");
    assert_eq!(option_fractus(Some(f32::MAX)), fractus(f32::MAX));
}

#[test]
fn display_fractus_f64_integral_keeps_the_exact_expansion() {
    assert_eq!(fractus(1.0e21_f64), "1000000000000000000000.0");
    assert_eq!(
        fractus(18_446_744_073_709_551_616.0_f64),
        "18446744073709551616.0"
    );
    assert_eq!(fractus(-0.0_f64), "-0.0");
}

#[test]
fn display_bivalens_uses_faber_words() {
    assert_eq!(bivalens(true), "verum");
    assert_eq!(bivalens(false), "falsum");
}

#[test]
fn display_text_payload_returns_payload_without_debug_wrapper() {
    assert_eq!(text_payload("pg:query"), "pg:query");
}

#[test]
fn display_valor_nihil() {
    assert_eq!(valor(&Valor::Nihil), "nihil");
}

#[test]
fn display_valor_bivalens() {
    assert_eq!(valor(&Valor::Bivalens(true)), "verum");
}

#[test]
fn display_valor_numerus() {
    assert_eq!(valor(&Valor::Numerus(42)), "42");
}

#[test]
fn display_valor_fractus() {
    assert_eq!(valor(&Valor::Fractus(1.0)), "1.0");
}

#[test]
fn display_valor_textus() {
    assert_eq!(valor(&Valor::Textus("salve".into())), "salve");
    assert_eq!(valor(&Valor::Textus(String::new())), "", "empty textus");
}

#[test]
fn display_valor_formats_aggregate_payloads_without_carrier_tags() {
    let mut map = BTreeMap::new();
    map.insert("n".to_owned(), Valor::Numerus(7));

    assert_eq!(
        valor(&Valor::Lista(vec![
            Valor::Numerus(1),
            Valor::Textus("a".into())
        ])),
        "[1, a]"
    );
    assert_eq!(valor(&Valor::Tabula(map)), r#"{"n": 7}"#);
}

#[test]
fn display_valor_empty_lista() {
    assert_eq!(valor(&Valor::Lista(vec![])), "[]");
}

#[test]
fn display_valor_empty_tabula() {
    assert_eq!(valor(&Valor::Tabula(BTreeMap::new())), "{}");
}

#[test]
fn display_option_uses_payload_or_nihil() {
    let text = "Roma".to_owned();

    assert_eq!(option(Some(&text)), "Roma");
    assert_eq!(option::<String>(None), "nihil");
}

#[test]
fn display_option_bivalens_some_verum() {
    assert_eq!(option_bivalens(Some(true)), "verum");
}

#[test]
fn display_option_bivalens_none_nihil() {
    assert_eq!(option_bivalens(None), "nihil");
}

#[test]
fn display_option_fractus_some_preserves_spelling() {
    assert_eq!(option_fractus(Some(1.0_f64)), "1.0");
}

#[test]
fn display_option_fractus_none_nihil() {
    assert_eq!(option_fractus::<f64>(None), "nihil");
}

#[test]
fn display_option_vacuum_some_vacuum() {
    assert_eq!(option_vacuum(Some(())), "vacuum");
}

#[test]
fn display_option_vacuum_none_nihil() {
    assert_eq!(option_vacuum::<()>(None), "nihil");
}
