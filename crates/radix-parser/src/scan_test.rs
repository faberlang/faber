//! Parser pins for the source-shaped ordered prefix expression.

use crate::test_support::{assert_parse_error_issue, parse_ok_la};
use radix_syntax::{BinOp, ExprKind, ReductaOp, ScanTotal, SortDirection, StmtKind};

#[test]
fn scan_node_keeps_default_inclusive_row_major_order() {
    let source = "functio t(tensor<numerus, [4]> xs) → tensor<numerus, [4]> { redde scan via summa ex xs apud [i] filum lane sit value { redde value } }";
    let result = parse_ok_la(source);
    let program = result.program.expect("program");
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let StmtKind::Redde(statement) = &function.body.as_ref().unwrap().statements[0].kind else {
        panic!("expected return");
    };
    let ExprKind::Scan(scan) = &statement.value.as_ref().expect("return value").kind else {
        panic!("expected Scan node");
    };
    assert_eq!(scan.op, ReductaOp::Sum);
    assert_eq!(scan.total, ScanTotal::Inclusive);
    assert_eq!(scan.order, SortDirection::Ascending);
    assert_eq!(scan.apud.len(), 1);
    assert!(scan.group.is_none());
    assert_eq!(result.interner.resolve(scan.filum.name), "lane");
    assert_eq!(result.interner.resolve(scan.binding.name), "value");
}

#[test]
fn scan_node_keeps_explicit_total_order_and_group_clauses() {
    let source = "functio t(tensor<numerus, [4]> xs, tensor<numerus, [4]> keys) → tensor<numerus, [4]> { redde scan via summa total exclusive ex xs apud [i] pro keys[i] order descendens filum lane sit value { redde value } }";
    let result = parse_ok_la(source);
    let program = result.program.expect("program");
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let StmtKind::Redde(statement) = &function.body.as_ref().unwrap().statements[0].kind else {
        panic!("expected return");
    };
    let ExprKind::Scan(scan) = &statement.value.as_ref().expect("return value").kind else {
        panic!("expected Scan node");
    };
    assert_eq!(scan.total, ScanTotal::Exclusive);
    assert_eq!(scan.order, SortDirection::Descending);
    assert!(scan.group.is_some());
}

#[test]
fn trailing_coalesce_wraps_the_whole_scan_expression() {
    let source = "functio t(tensor<numerus, [4]> xs) → tensor<numerus, [4]> { redde scan via summa ex xs apud [i] filum lane sit value { redde value } vel xs }";
    let result = parse_ok_la(source);
    let program = result.program.expect("program");
    let StmtKind::Func(function) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let StmtKind::Redde(statement) = &function.body.as_ref().unwrap().statements[0].kind else {
        panic!("expected return");
    };
    let ExprKind::Binary(coalesce) = &statement.value.as_ref().expect("return value").kind else {
        panic!("expected outer coalesce");
    };
    assert_eq!(coalesce.op, BinOp::Coalesce);
    assert!(matches!(&coalesce.lhs.kind, ExprKind::Scan(_)));
}

#[test]
fn scan_rejects_unsupported_operator_names_and_missing_required_clauses() {
    assert_parse_error_issue(
        "functio t(tensor<numerus, [4]> xs) → tensor<numerus, [4]> { scan via argmax ex xs apud [i] filum lane sit value { redde value } }",
        "expected_keyword",
    );
    assert_parse_error_issue(
        "functio t(tensor<numerus, [4]> xs) → tensor<numerus, [4]> { scan via summa total arbitrary ex xs apud [i] filum lane sit value { redde value } }",
        "expected_keyword",
    );
    assert_parse_error_issue(
        "functio t(tensor<numerus, [4]> xs) → tensor<numerus, [4]> { scan via summa ex xs filum lane sit value { redde value } }",
        "expected_keyword",
    );
}
