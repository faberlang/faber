//! D3.4: a `tabula` key or `copia` element must be hashable — no `fractus`
//! (any width), no mutable collections; `iuncta`, `genus` and `discretio`
//! keys are hashable when every part is. Rejected at the declaring type.
//!
//! Assertions are structured (`code` + `issue`), never rendered prose.

use crate::test_support;

fn assert_rejects(source: &str, issue: &str) {
    let result = test_support::analyze_source_la(source);
    let err = result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && err.issue() == Some(issue))
        .unwrap_or_else(|| panic!("missing {issue} in {:?}", result.diagnostics));
    assert_eq!(crate::semantic_spec(err.kind).code, "SEM010");
}

#[test]
fn fractus_tabula_key_rejects() {
    assert_rejects(
        r"
incipit {
    varia tabula<f64, numerus> m ← vacua
    nota m.longitudo()
}",
        "tabula_key_not_hashable",
    );
}

#[test]
fn unsized_fractus_tabula_key_rejects_in_parameter() {
    assert_rejects(
        r"
functio probe(tabula<fractus, numerus> m) → numerus {
    redde m.longitudo()
}",
        "tabula_key_not_hashable",
    );
}

#[test]
fn lista_tabula_key_rejects() {
    assert_rejects(
        r"
functio probe(tabula<lista<numerus>, numerus> m) → numerus {
    redde m.longitudo()
}",
        "tabula_key_not_hashable",
    );
}

#[test]
fn fractus_copia_element_rejects() {
    assert_rejects(
        r"
functio probe(copia<f32> s) → numerus {
    redde s.longitudo()
}",
        "copia_element_not_hashable",
    );
}

#[test]
fn tuple_key_with_fractus_part_rejects() {
    assert_rejects(
        r"
functio probe(tabula<iuncta<textus, f64>, numerus> m) → numerus {
    redde m.longitudo()
}",
        "tabula_key_not_hashable",
    );
}

#[test]
fn genus_key_with_fractus_field_rejects() {
    assert_rejects(
        r"
genus Punctum {
    sit f64 x
}

functio probe(tabula<Punctum, numerus> m) → numerus {
    redde m.longitudo()
}",
        "tabula_key_not_hashable",
    );
}

#[test]
fn nested_map_in_genus_field_type_rejects() {
    assert_rejects(
        r"
genus Index {
    sit tabula<f64, textus> by_weight
}
",
        "tabula_key_not_hashable",
    );
}

#[test]
fn hashable_keys_accept() {
    test_support::assert_accepts_la(
        r"
genus Locus {
    sit textus regio
    sit numerus annus
}

discretio Clavis {
    Nomen { textus nomen },
    Numerus { numerus n }
}

functio probe(
    tabula<textus, numerus> a,
    tabula<iuncta<textus, numerus>, numerus> b,
    tabula<Locus, numerus> c,
    copia<Clavis> d,
    copia<u8> e
) → numerus {
    redde a.longitudo() + b.longitudo() + c.longitudo() + d.longitudo() + e.longitudo()
}",
    );
}
