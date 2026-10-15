//! Inline regex literals (ruling d381ae56): `regex /pattern/` (the locale's
//! regex type word) is the self-typed `Literal::Regex`; `_ /pattern/` builds
//! the inferred-constructor hole — an `ExprKind::Verte` carrying an `Infer`
//! type over the regex literal, the same node as `_ [ … ]` (ruling
//! 44def1ae, need). A bare `/` in operand position stays division.

use crate::ParseResult;
use crate::test_support::{parse_ok, symbol_name};
use radix_syntax::{BinOp, Expr, ExprKind, IfBody, Literal, Stmt, StmtKind, TypeExprKind};

fn var_initializer(statement: &Stmt) -> &Expr {
    let StmtKind::Var(var) = &statement.kind else {
        panic!("expected variable declaration, got {:?}", statement.kind);
    };
    var.init.as_deref().expect("initializer")
}

fn entry_statement(result: &ParseResult) -> &Stmt {
    let program = result.program.as_ref().expect("program");
    let statement = program.statements.first().expect("entry statement");
    let StmtKind::Incipit(incipit) = &statement.kind else {
        return statement;
    };
    let IfBody::Block(block) = &incipit.body else {
        panic!("expected a block incipit body, got {:?}", incipit.body);
    };
    block.statements.first().expect("entry body statement")
}

fn entry_initializer(result: &ParseResult) -> &Expr {
    var_initializer(entry_statement(result))
}

#[test]
fn regex_type_word_literal_parses() {
    let result = parse_ok(r"incipit { sit regex r ← regex /\d+/ }");
    let init = entry_initializer(&result);
    let ExprKind::Literal(Literal::Regex(pattern)) = init.kind else {
        panic!("expected regex literal, got {:?}", init.kind);
    };
    assert_eq!(symbol_name(&result, pattern), r"\d+");
}

#[test]
fn inferred_regex_literal_parses() {
    let result = parse_ok(r"incipit { sit regex r ← _ /a/ }");
    let init = entry_initializer(&result);
    let ExprKind::Verte(verte) = &init.kind else {
        panic!("expected inferred hole, got {:?}", init.kind);
    };
    let ExprKind::Literal(Literal::Regex(pattern)) = &verte.expr.kind else {
        panic!(
            "expected regex literal inside the hole, got {:?}",
            verte.expr.kind
        );
    };
    assert_eq!(symbol_name(&result, *pattern), "a");
    assert!(matches!(verte.ty.kind, TypeExprKind::Infer));
}

#[test]
fn bare_slash_stays_division() {
    let result = parse_ok(r"incipit { sit numerus q ← 6 / 2 }");
    let init = entry_initializer(&result);
    let ExprKind::Binary(binary) = &init.kind else {
        panic!("expected binary expression, got {:?}", init.kind);
    };
    assert_eq!(binary.op, BinOp::Div);
}

#[test]
fn regex_literal_accepts_member_call_suffix() {
    let result = parse_ok(r#"incipit { nota regex /a+/.consentit("aaa") }"#);
    let statement = entry_statement(&result);
    let StmtKind::Scribe(scribe) = &statement.kind else {
        panic!("expected nota statement, got {:?}", statement.kind);
    };
    let expr = scribe.args.first().expect("nota argument");
    let ExprKind::Call(call) = &expr.kind else {
        panic!("expected call expression, got {:?}", expr.kind);
    };
    let ExprKind::Member(member) = &call.callee.kind else {
        panic!("expected member callee, got {:?}", call.callee.kind);
    };
    assert!(matches!(
        member.object.kind,
        ExprKind::Literal(Literal::Regex(_))
    ));
}
