//! Parser pins for the contextual `reducta via Op` expression.

use crate::test_support::{
    ParseExpectation, assert_parse_error_issue, assert_parse_error_matches, parse_ok, parse_program,
};
use radix_syntax::{ExprKind, ReductaOp, StmtKind};

#[test]
fn reduction_head_requires_via_and_known_operator() {
    assert_parse_error_issue(
        "functio t(tensor<numerus, [4]> xs) → numerus { reducta ex xs }",
        "reducta_requires_via",
    );
    assert_parse_error_issue(
        "functio t(tensor<numerus, [4]> xs) → numerus { reducta via incognitum ex xs }",
        "reducta_unknown_operator",
    );
    for (source, issue) in [
        (
            "functio t(tensor<numerus, [4]> xs) → numerus { reducta ex xs }",
            "reducta_requires_via",
        ),
        (
            "functio t(tensor<numerus, [4]> xs) → numerus { reducta via incognitum ex xs }",
            "reducta_unknown_operator",
        ),
    ] {
        assert_parse_error_matches(source, ParseExpectation::Issue(issue));
        assert_parse_error_matches(
            source,
            ParseExpectation::Arg {
                name: "operators",
                value: "summa, factum, maxima, minima, argmaxima, argminima, omnia, quilibet, numeratio, prima",
            },
        );
    }
}

#[test]
fn reduction_node_owns_optional_clauses_and_body() {
    let result = parse_ok(
        "functio t(tensor<numerus, [8]> xs) → numerus { reducta via summa ex xs apud [i] pro i sit x { redde x } }",
    );
    let program = result.program.expect("program");
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let statement = &function.body.as_ref().expect("function body").statements[0];
    let StmtKind::Expr(statement) = &statement.kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Reducta(reduction) = &statement.expr.kind else {
        panic!("expected Reduction node");
    };
    assert_eq!(reduction.op, ReductaOp::Sum);
    assert_eq!(reduction.apud.len(), 1);
    assert!(reduction.group.is_some());
    assert_eq!(
        result
            .interner
            .resolve(reduction.binding.as_ref().unwrap().name),
        "x"
    );
    assert!(reduction.body.is_some());
}

#[test]
fn body_lookahead_does_not_claim_a_following_call() {
    let result =
        parse_ok("functio t(tensor<numerus, [4]> xs) → numerus { reducta via summa ex xs }");
    let program = result.program.expect("program");
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let StmtKind::Expr(statement) = &function.body.as_ref().unwrap().statements[0].kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Reducta(reduction) = &statement.expr.kind else {
        panic!("expected Reduction node");
    };
    assert!(reduction.binding.is_none());
    assert!(reduction.body.is_none());

    let declaration = parse_ok(
        "functio t(tensor<numerus, [4]> xs) → numerus { sit numerus total ← reducta via summa ex xs redde total }",
    );
    let program = declaration.program.expect("program");
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().unwrap();
    let StmtKind::Var(local) = &body.statements[0].kind else {
        panic!("expected a following local declaration");
    };
    let ExprKind::Reducta(reduction) = &local.init.as_ref().unwrap().kind else {
        panic!("expected a bodiless reduction initializer");
    };
    assert!(reduction.body.is_none());
    assert_eq!(body.statements.len(), 2);
}

#[test]
fn operator_word_and_head_remain_contextual_identifiers() {
    for source in [
        "functio t() → numerus { summa(2, 3) }",
        "functio t(numerus summa) → numerus { redde summa }",
        "functio t(numerus reducta) → numerus { redde reducta }",
        "functio t(lista<numerus> xs) → vacuum { xs.reducta() }",
    ] {
        assert!(
            parse_program(source).success(),
            "source did not parse: {source}"
        );
    }
}

#[test]
fn prima_parses_as_the_first_reduction_operator_after_via() {
    let result = parse_ok(
        "functio t(tensor<fractus, [4]> xs) → fractus ∪ nihil { reducta via prima ex xs sit v { redde v ≻ 0.0 } }",
    );
    let program = result.program.expect("program");
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let StmtKind::Expr(statement) = &function.body.as_ref().unwrap().statements[0].kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Reducta(reduction) = &statement.expr.kind else {
        panic!("expected Reduction node");
    };
    assert_eq!(reduction.op, ReductaOp::First);
    assert!(reduction.body.is_some());
}

#[test]
fn prima_remains_an_ordinary_identifier_outside_the_via_slot() {
    for source in [
        "functio t(numerus prima) → numerus { redde prima }",
        "functio t(lista<numerus> xs) → numerus ∪ nihil { redde xs.prima(2) }",
    ] {
        assert!(
            parse_program(source).success(),
            "source did not parse: {source}"
        );
    }
}

#[test]
fn scan_refuses_the_first_operator() {
    let result = parse_program(
        "functio t(tensor<fractus, [4]> xs) → tensor<fractus, [4]> { scan via prima ex xs apud [i] filum f sit v { redde v } }",
    );
    assert!(!result.success(), "scan admits only value-prefix operators");
}
