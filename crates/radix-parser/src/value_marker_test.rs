//! Parser coverage for value-side `copy`/`own` markers (operator ruling
//! 2026-10-08): `var T b ← copy a`, `var T b ← own a`, `b ← copy c`.

use crate::test_support::parse_ok;
use radix_syntax::{AssignOp, ExprKind, IfBody, StmtKind, TypeMode};

fn entry_statement(source: &str) -> radix_syntax::Stmt {
    let result = parse_ok(source);
    let program = result.program.expect("program");
    let stmt = program.statements.into_iter().last().expect("statement");
    let StmtKind::Incipit(entry) = stmt.kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(block) = entry.body else {
        panic!("expected block body");
    };
    block
        .statements
        .into_iter()
        .next()
        .expect("first statement")
}

fn declared_mode(source: &str) -> (Option<TypeMode>, bool) {
    let StmtKind::Var(decl) = entry_statement(source).kind else {
        panic!("expected var declaration");
    };
    let init_is_ident = decl
        .init
        .as_ref()
        .is_some_and(|init| matches!(init.kind, ExprKind::Ident(_)));
    (decl.ty.expect("type").mode, init_is_ident)
}

#[test]
fn value_marker_copy_lands_on_the_declared_type() {
    let (mode, init_is_ident) = declared_mode("incipit {\n    fixum lista<numerus> b ← copy a\n}");
    assert_eq!(mode, Some(TypeMode::Copy));
    assert!(init_is_ident, "the marker is not part of the initializer");
}

#[test]
fn value_marker_own_lands_on_the_declared_type() {
    let (mode, init_is_ident) = declared_mode("incipit {\n    fixum lista<numerus> b ← own a\n}");
    assert_eq!(mode, Some(TypeMode::Own));
    assert!(init_is_ident);
}

#[test]
fn value_marker_matches_the_binding_side_spelling() {
    let (value_side, _) = declared_mode("incipit {\n    fixum lista<numerus> b ← copy a\n}");
    let (binding_side, _) = declared_mode("incipit {\n    fixum copy lista<numerus> b ← a\n}");
    assert_eq!(value_side, binding_side);
}

#[test]
fn value_marker_copy_on_assignment_is_recorded() {
    let StmtKind::Expr(expr) = entry_statement("incipit {\n    b ← copy c\n}").kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Assign(assign) = &expr.expr.kind else {
        panic!("expected assignment, got {:?}", expr.expr.kind);
    };
    assert!(matches!(assign.op, AssignOp::Assign));
    assert_eq!(assign.value_marker, Some(TypeMode::Copy));
    assert!(matches!(assign.value.kind, ExprKind::Ident(_)));
}

#[test]
fn value_marker_words_stay_identifiers_without_an_operand() {
    let StmtKind::Expr(expr) = entry_statement("incipit {\n    b ← copy\n}").kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Assign(assign) = &expr.expr.kind else {
        panic!("expected assignment");
    };
    assert_eq!(assign.value_marker, None);
    assert!(matches!(assign.value.kind, ExprKind::Ident(_)));
}
