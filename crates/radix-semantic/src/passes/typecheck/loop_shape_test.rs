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
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs sit x {\n        xs.appende(x)\n    }\n}",
    );
}

#[test]
fn lista_detrahe_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs sit x {\n        xs.detrahe()\n    }\n}",
    );
}

#[test]
fn lista_decapita_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs sit x {\n        xs.decapita()\n    }\n}",
    );
}

#[test]
fn lista_inverte_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs sit x {\n        xs.inverte()\n    }\n}",
    );
}

#[test]
fn lista_ordina_in_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [3, 1, 2]\n    itera ex xs sit x {\n        xs.ordina()\n    }\n}",
    );
}

#[test]
fn copia_adde_in_loop_over_same_set_is_rejected() {
    rejects(
        "incipit {\n    varia copia<numerus> s ← vacua\n    itera ex s sit x {\n        s.adde(x)\n    }\n}",
    );
}

#[test]
fn copia_dele_in_loop_over_same_set_is_rejected() {
    rejects(
        "incipit {\n    varia copia<numerus> s ← vacua\n    itera ex s sit x {\n        s.dele(x)\n    }\n}",
    );
}

#[test]
fn tabula_pone_in_loop_over_same_map_is_rejected() {
    rejects(
        "incipit {\n    varia tabula<textus, numerus> m ← vacua\n    itera de m sit k {\n        m.pone(k, 1)\n    }\n}",
    );
}

#[test]
fn tabula_dele_in_loop_over_same_map_is_rejected() {
    rejects(
        "incipit {\n    varia tabula<textus, numerus> m ← vacua\n    itera de m sit k {\n        m.dele(k)\n    }\n}",
    );
}

#[test]
fn tabula_pone_in_ex_loop_over_same_map_is_rejected() {
    rejects(
        "incipit {\n    varia tabula<textus, numerus> m ← vacua\n    itera ex m sit v {\n        m.pone(\"a\", v)\n    }\n}",
    );
}

#[test]
fn lista_appende_in_lockstep_loop_over_same_list_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2]\n    varia lista<numerus> ys ← [3, 4]\n    itera ex xs, ys sit x, y {\n        xs.appende(x)\n    }\n}",
    );
}

#[test]
fn lista_appende_in_nested_block_of_the_loop_is_rejected() {
    rejects(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs sit x {\n        si x ≻ 1 {\n            xs.appende(x)\n        }\n    }\n}",
    );
}

#[test]
fn queue_appende_in_loop_over_same_queue_is_rejected() {
    rejects(
        "functio f(varia queue<numerus> q) → vacuum {\n    itera ex q sit x {\n        q.appende(x)\n    }\n}",
    );
}

#[test]
fn queue_decapita_in_loop_over_same_queue_is_rejected() {
    rejects(
        "functio f(varia queue<numerus> q) → vacuum {\n    itera ex q sit x {\n        q.decapita()\n    }\n}",
    );
}

#[test]
fn stack_appende_in_loop_over_same_stack_is_rejected() {
    rejects(
        "functio f(varia stack<numerus> s) → vacuum {\n    itera ex s sit x {\n        s.appende(x)\n    }\n}",
    );
}

#[test]
fn stack_detrahe_in_loop_over_same_stack_is_rejected() {
    rejects(
        "functio f(varia stack<numerus> s) → vacuum {\n    itera ex s sit x {\n        s.detrahe()\n    }\n}",
    );
}

#[test]
fn queue_appende_in_loop_over_a_different_queue_is_accepted() {
    test_support::assert_accepts(
        "functio f(varia queue<numerus> a, varia queue<numerus> b) → vacuum {\n    itera ex a sit x {\n        b.appende(x)\n    }\n}",
    );
}

#[test]
fn bracket_store_on_list_in_loop_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs sit x {\n        xs[0] ← x\n    }\n}",
    );
}

#[test]
fn bracket_store_on_map_in_loop_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia tabula<textus, numerus> m ← vacua\n    itera de m sit k {\n        m[k] ← 1\n    }\n}",
    );
}

#[test]
fn method_on_a_different_collection_in_loop_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    sit lista<numerus> xs ← [1, 2, 3]\n    varia lista<numerus> ys ← []\n    itera ex xs sit x {\n        ys.appende(x)\n    }\n}",
    );
}

#[test]
fn shape_method_after_the_loop_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    itera ex xs sit x {\n        nota x\n    }\n    xs.appende(4)\n}",
    );
}
