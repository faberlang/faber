//! Loop shape rule (need, ruling O1): inside a loop over the local
//! `X`, a method call on `X` that changes its shape or order is rejected with
//! `loop_collection_shape_change`; a bracket store on `X` is accepted.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support;

const ISSUE: &str = "loop_collection_shape_change";

fn rejects(source: &str) {
    test_support::assert_rejects_with_issue(source, ISSUE);
}

#[test]
fn lista_appende_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs fixum x {\n        xs.appende(x)\n    }\n}",
    );
}

#[test]
fn lista_detrahe_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs fixum x {\n        xs.detrahe()\n    }\n}",
    );
}

#[test]
fn lista_decapita_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs fixum x {\n        xs.decapita()\n    }\n}",
    );
}

#[test]
fn lista_inverte_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs fixum x {\n        xs.inverte()\n    }\n}",
    );
}

#[test]
fn lista_ordina_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [3, 1, 2]\n    itera ex xs fixum x {\n        xs.ordina()\n    }\n}",
    );
}

#[test]
fn copia_adde_in_loop_over_same_set_is_rejected() {
    rejects(
        "incipit {\n    varia copia<numerus> s ← vacua\n    itera ex s fixum x {\n        s.adde(x)\n    }\n}",
    );
}

#[test]
fn copia_dele_in_loop_over_same_set_is_rejected() {
    rejects(
        "incipit {\n    varia copia<numerus> s ← vacua\n    itera ex s fixum x {\n        s.dele(x)\n    }\n}",
    );
}

#[test]
fn tabula_pone_in_loop_over_same_map_is_rejected() {
    rejects(
        "incipit {\n    varia tabula<textus, numerus> m ← vacua\n    itera de m fixum k {\n        m.pone(k, 1)\n    }\n}",
    );
}

#[test]
fn tabula_dele_in_loop_over_same_map_is_rejected() {
    rejects(
        "incipit {\n    varia tabula<textus, numerus> m ← vacua\n    itera de m fixum k {\n        m.dele(k)\n    }\n}",
    );
}

#[test]
fn tabula_pone_in_ex_loop_over_same_map_is_rejected() {
    rejects(
        "incipit {\n    varia tabula<textus, numerus> m ← vacua\n    itera ex m fixum v {\n        m.pone(\"a\", v)\n    }\n}",
    );
}

#[test]
fn lista_appende_in_lockstep_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2]\n    varia lista<numerus> ys ← [3, 4]\n    itera ex xs, ys fixum x, y {\n        xs.appende(x)\n    }\n}",
    );
}

#[test]
fn lista_appende_in_nested_block_of_the_loop_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs fixum x {\n        si x ≻ 1 {\n            xs.appende(x)\n        }\n    }\n}",
    );
}

#[test]
fn bracket_store_on_list_in_loop_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs fixum x {\n        xs[0] ← x\n    }\n}",
    );
}

#[test]
fn bracket_store_on_map_in_loop_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia tabula<textus, numerus> m ← vacua\n    itera de m fixum k {\n        m[k] ← 1\n    }\n}",
    );
}

#[test]
fn method_on_a_different_collection_in_loop_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    sit lista<numerus> xs ← [1, 2, 3]\n    varia lista<numerus> ys ← []\n    itera ex xs fixum x {\n        ys.appende(x)\n    }\n}",
    );
}

#[test]
fn shape_method_after_the_loop_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs fixum x {\n        nota x\n    }\n    xs.appende(4)\n}",
    );
}
