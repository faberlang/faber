use crate::valor::{FromValor, Valor};
use std::collections::{BTreeMap, HashMap};

#[test]
fn from_valor_extracts_lista_atomically() {
    assert_eq!(Vec::<i64>::from_valor(&Valor::Lista(vec![])), Some(vec![]));

    let valor = Valor::Lista(vec![Valor::Numerus(1), Valor::Numerus(2)]);
    assert_eq!(Vec::<i64>::from_valor(&valor), Some(vec![1, 2]));
    assert_eq!(
        Vec::<i64>::from_valor(&Valor::Lista(vec![
            Valor::Numerus(1),
            Valor::Textus("x".into())
        ])),
        None
    );
}

#[test]
fn from_valor_lista_rejects_wrong_valor_variant() {
    assert_eq!(Vec::<i64>::from_valor(&Valor::Nihil), None);
    assert_eq!(
        Vec::<i64>::from_valor(&Valor::Tabula(BTreeMap::new())),
        None
    );
    assert_eq!(Vec::<i64>::from_valor(&Valor::Numerus(42)), None);
}

#[test]
fn from_valor_lista_with_empty_inner_types() {
    // Lista of empty tuples maps to Vec<()>
    assert_eq!(
        Vec::<()>::from_valor(&Valor::Lista(vec![Valor::Nihil, Valor::Nihil])),
        Some(vec![(), ()])
    );
}

#[test]
fn from_valor_lista_mixed_types_produces_none_for_typed_vec() {
    assert_eq!(
        Vec::<i64>::from_valor(&Valor::Lista(vec![
            Valor::Numerus(1),
            Valor::Textus("bad".into()),
        ])),
        None
    );
}

#[test]
fn from_valor_tabula_extracts_empty_map() {
    assert_eq!(
        HashMap::<String, i64>::from_valor(&Valor::Tabula(BTreeMap::new())),
        Some(HashMap::new())
    );
}

#[test]
fn from_valor_tabula_extracts_filled_map() {
    let mut tab = BTreeMap::new();
    tab.insert("a".to_owned(), Valor::Numerus(1));
    tab.insert("b".to_owned(), Valor::Numerus(2));
    let valor = Valor::Tabula(tab);
    let mut expected = HashMap::new();
    expected.insert("a".to_owned(), 1);
    expected.insert("b".to_owned(), 2);
    assert_eq!(HashMap::<String, i64>::from_valor(&valor), Some(expected));
}

#[test]
fn from_valor_tabula_rejects_wrong_valor_variant() {
    assert_eq!(
        HashMap::<String, i64>::from_valor(&Valor::Lista(vec![Valor::Numerus(1)])),
        None
    );
}

#[test]
fn from_valor_tabula_rejects_mismatched_value_types() {
    let mut tab = BTreeMap::new();
    tab.insert("a".to_owned(), Valor::Numerus(1));
    tab.insert("b".to_owned(), Valor::Textus("bad".into()));
    let valor = Valor::Tabula(tab);
    assert_eq!(HashMap::<String, i64>::from_valor(&valor), None);
}
