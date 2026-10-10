//! Parser pins for the contextual `ordinata via Direction` sort expression.

use crate::test_support::{
    ParseExpectation, assert_parse_error_issue, assert_parse_error_matches, parse_ok, parse_program,
};
use radix_syntax::{ExprKind, SortDirection, SortExpr, SortSourceMode, StmtKind};

fn first_sort(source: &str) -> (radix_syntax::Program, radix_lexer::Interner) {
    let result = parse_ok(source);
    (result.program.expect("program"), result.interner)
}

fn sort_of(program: &radix_syntax::Program) -> &SortExpr {
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let StmtKind::Expr(statement) = &function.body.as_ref().expect("body").statements[0].kind
    else {
        panic!("expected expression statement");
    };
    let ExprKind::Sort(sort) = &statement.expr.kind else {
        panic!("expected Sort node");
    };
    sort
}

#[test]
fn sort_values_and_keys_parse_with_direction_and_source_mode() {
    let (program, _) = first_sort(
        "functio t(lista<numerus> xs) → lista<numerus> { ordinata via ascendens ex xs }",
    );
    let sort = sort_of(&program);
    assert_eq!(sort.direction, SortDirection::Ascending);
    assert_eq!(sort.source_mode, SortSourceMode::Values);
    assert!(sort.body.is_none());
    assert!(sort.binding.is_none());

    let (program, _) = first_sort(
        "functio t(tabula<textus, numerus> m) → lista<textus> { ordinata via descendens de m }",
    );
    let sort = sort_of(&program);
    assert_eq!(sort.direction, SortDirection::Descending);
    assert_eq!(sort.source_mode, SortSourceMode::Keys);
}

#[test]
fn sort_body_binds_the_element_and_returns_the_key() {
    let (program, interner) = first_sort(
        "functio t(lista<numerus> xs) → lista<numerus> { ordinata via ascendens ex xs fixum p { redde p } }",
    );
    let sort = sort_of(&program);
    assert_eq!(interner.resolve(sort.binding.as_ref().unwrap().name), "p");
    assert!(sort.body.is_some());
}

#[test]
fn sort_head_requires_via_direction_and_source_word() {
    for (source, issue) in [
        (
            "functio t(lista<numerus> xs) → lista<numerus> { ordinata ex xs }",
            "sort_requires_via",
        ),
        (
            "functio t(lista<numerus> xs) → lista<numerus> { ordinata via incognitum ex xs }",
            "sort_unknown_direction",
        ),
        (
            "functio t(lista<numerus> xs) → lista<numerus> { ordinata via ascendens xs }",
            "sort_requires_source",
        ),
    ] {
        assert_parse_error_issue(source, issue);
    }
    assert_parse_error_matches(
        "functio t(lista<numerus> xs) → lista<numerus> { ordinata via incognitum ex xs }",
        ParseExpectation::Arg {
            name: "directions",
            value: "ascendens, descendens",
        },
    );
}

#[test]
fn itera_binder_is_not_claimed_as_a_sort_body() {
    let result = parse_ok(
        "functio t(tabula<textus, numerus> m) → vacuum { itera ex ordinata via ascendens de m fixum k { scribe k } }",
    );
    let program = result.program.expect("program");
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let StmtKind::Itera(itera) = &function.body.as_ref().unwrap().statements[0].kind else {
        panic!("expected itera statement");
    };
    let ExprKind::Sort(sort) = &itera.sources[0].kind else {
        panic!("expected sort source");
    };
    assert!(sort.body.is_none(), "the loop binder stays the loop's");
    assert_eq!(itera.bindings.len(), 1);
}

#[test]
fn sort_words_and_list_method_remain_contextual_identifiers() {
    for source in [
        "functio t(numerus ordinata) → numerus { redde ordinata }",
        "functio t(numerus ascendens) → numerus { redde ascendens }",
        "functio t(numerus descendens) → numerus { redde descendens }",
        "functio t(lista<numerus> xs) → vacuum { xs.ordina() }",
        "functio t(lista<numerus> xs) → lista<numerus> { redde xs.ordinata() }",
    ] {
        assert!(
            parse_program(source).success(),
            "source did not parse: {source}"
        );
    }
}
