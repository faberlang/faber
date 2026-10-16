//! need: the `at` read (la `apud`) on `lista`, `tabula` and `textus`
//! is a failable read: an out-of-range index or a missing key is reported on
//! `⇥ textus`, so the call needs a `⊥` default, a `fac`/`cape` handler or an
//! enclosing `⇥`. The result type is the element / value type `T`.
//!
//! Fixtures use latin spellings except the en-surface case, which proves that
//! `xs.at(1)` and the `at` loop clause share a source.

use crate::locale::LocalePack;
use radix_target::Target;

fn en_pack() -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path("en");
    LocalePack::from_toml_path(&path).expect("installed en pack should load")
}

#[test]
fn at_read_lista_without_a_handler_is_rejected() {
    crate::test_support::assert_rejects_with_issue(
        "functio f(lista<numerus> xs) → numerus {\n    redde xs.apud(1)\n}\n",
        "failable_call_requires_handler",
    );
}

#[test]
fn at_read_tabula_without_a_handler_is_rejected() {
    crate::test_support::assert_rejects_with_issue(
        "functio f(tabula<textus, numerus> m) → numerus {\n    redde m.apud(\"k\")\n}\n",
        "failable_call_requires_handler",
    );
}

#[test]
fn at_read_textus_without_a_handler_is_rejected() {
    crate::test_support::assert_rejects_with_issue(
        "functio f(textus s) → textus {\n    redde s.apud(0)\n}\n",
        "failable_call_requires_handler",
    );
}

#[test]
fn at_read_default_types_as_the_element_type() {
    let _ = crate::test_support::analyze_ok(
        "functio f(lista<numerus> xs) → numerus {\n    redde xs.apud(1) ⊥ 0\n}\n",
    );
    let _ = crate::test_support::analyze_ok(
        "functio f(tabula<textus, numerus> m) → numerus {\n    redde m.apud(\"k\") ⊥ 0\n}\n",
    );
    let _ = crate::test_support::analyze_ok(
        "functio f(textus s) → textus {\n    redde s.apud(0) ⊥ \"\"\n}\n",
    );
}

#[test]
fn at_read_result_is_not_an_option() {
    crate::test_support::assert_rejects_with_issue(
        "functio f(lista<numerus> xs) → textus {\n    redde xs.apud(1) ⊥ 0\n}\n",
        "return_type_mismatch",
    );
}

#[test]
fn at_read_cape_handler_checks() {
    let _ = crate::test_support::analyze_ok(
        "functio f(lista<numerus> xs) → numerus {\n    varia numerus n ← 0\n    fac {\n        n ← xs.apud(5)\n    } cape err {\n        n ← 9\n    }\n    redde n\n}\n",
    );
}

#[test]
fn at_read_en_surface_shares_a_source_with_the_loop_clause() {
    let source = r"fn pick(list<int> xs) → int {
    for from xs at [i] let v {
    }
    return xs.at(1) ⊥ 0
}";
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(en_pack());
    let result = crate::test_support::analyze_source_with_config(source, config);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

// need: Matrix coordinate methods share Tensor's checked-index typing.
#[test]
fn matrix_method_accipe_is_nullable() {
    let _ = crate::test_support::analyze_ok(
        "functio g(matrix<f32, [2, 2]> m) → f32 { redde m.accipe([0, 1]) vel 0.0 }\n",
    );
    let _ = crate::test_support::analyze_ok(
        "functio h(matrix<f32, [2, 2]> m) → f32 ∪ nihil { redde m.accipe([0, 1]) }\n",
    );
}

#[test]
fn matrix_method_apud_default_types_as_the_element_type() {
    let _ = crate::test_support::analyze_ok(
        "functio f(matrix<f32, [2, 2]> m, numerus i) → f32 { redde m.apud([i, 0]) ⊥ 0.0 }\n",
    );
}

#[test]
fn matrix_method_ponde_accepts() {
    let _ = crate::test_support::analyze_ok(
        "functio p(varia matrix<f32, [2, 2]> m) → vacuum { m.ponde([0, 1], 9.0) }\n",
    );
}

#[test]
fn matrix_method_accipe_literal_out_of_range_is_rejected() {
    crate::test_support::assert_rejects_with_issue(
        "functio f(matrix<f32, [2, 2]> m) → f32 ∪ nihil { redde m.accipe([2, 0]) }\n",
        "index_provably_out_of_bounds",
    );
}
