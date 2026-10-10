//! A bare sort or reduce expression is never a loop source (operator ruling
//! 2026-10-10, need): `PARSE030 itera_source_sort_reduce_unbound`.
//! Bind the value to a name first; a parenthesised expression stays a source.

use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok, parse_program};

const ISSUE: &str = "itera_source_sort_reduce_unbound";

fn refused(source: &str) -> usize {
    parse_program(source)
        .errors
        .iter()
        .filter(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == ISSUE)
        })
        .count()
}

#[test]
fn bare_sort_loop_source_is_refused_with_a_named_issue() {
    assert_parse_error_matches(
        "functio t(lista<numerus> xs) { itera ex ordinata via ascendens ex xs fixum x { nota x } }",
        ParseExpectation::Issue(ISSUE),
    );
}

#[test]
fn bare_sort_with_a_key_body_shape_is_refused_the_same_way() {
    // The loop binder wins the `fixum name {` clash; the sort is still bare.
    assert_parse_error_matches(
        "functio t(tabula<textus, numerus> m) { itera ex ordinata via ascendens de m fixum k { nota k } }",
        ParseExpectation::Issue(ISSUE),
    );
}

#[test]
fn bare_reduce_loop_source_is_refused_with_a_named_issue() {
    assert_eq!(
        refused(
            "functio t(tensor<numerus, [4]> xs) { itera ex reducta via summa ex xs fixum x { nota x } }"
        ),
        1
    );
}

#[test]
fn bound_name_loop_source_is_accepted() {
    let source = "functio t(lista<numerus> xs) { fixum lista<numerus> ys ← ordinata via ascendens ex xs\n itera ex ys fixum x { nota x } }";
    assert_eq!(refused(source), 0);
    parse_ok(source);
}

#[test]
fn parenthesised_sort_loop_source_stays_legal() {
    let source = "functio t(lista<numerus> xs) { itera ex (ordinata via ascendens ex xs) fixum x { nota x } }";
    assert_eq!(refused(source), 0);
    parse_ok(source);
}
