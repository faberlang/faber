//! inf track U3 parser pins: an integer literal beyond `u64` parses to
//! `Literal::BigInt` (expression and pattern position) and stays out of JSON.

use crate::test_support::parse_ok;
use radix_syntax::{ExprKind, Literal, Pattern, StmtKind};

fn first_initializer_literal(source: &str) -> (radix_lexer::Interner, Literal) {
    let result = parse_ok(source);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected a binding: {:?}", program.statements[0].kind);
    };
    let init = decl.init.as_ref().expect("initializer");
    let ExprKind::Literal(literal) = &init.kind else {
        panic!("expected a literal: {:?}", init.kind);
    };
    let literal = match literal {
        Literal::Int(value) => Literal::Int(*value),
        Literal::BigInt(text) => Literal::BigInt(*text),
        other => panic!("unexpected literal {other:?}"),
    };
    (result.interner, literal)
}

#[test]
fn a_big_literal_in_expression_position_is_a_big_int_literal() {
    let (interner, literal) = first_initializer_literal("fixum inf x ← 18446744073709551616\n");
    let Literal::BigInt(text) = literal else {
        panic!("expected a big literal");
    };
    assert_eq!(interner.resolve(text), "18446744073709551616");
}

#[test]
fn a_u64_max_literal_stays_an_ordinary_int() {
    let (_, literal) = first_initializer_literal("fixum numerus<u64> x ← 18446744073709551615\n");
    assert!(matches!(literal, Literal::Int(18_446_744_073_709_551_615)));
}

#[test]
fn a_big_literal_in_a_discerne_pattern_is_a_big_int_pattern() {
    let result = parse_ok(
        r"
discerne x {
    casu 0x10000000000000000 ergo redde 1
    casu -18446744073709551616 ergo redde 2
}
",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne: {:?}", program.statements[0].kind);
    };
    assert!(matches!(
        stmt.arms[0].patterns[0],
        Pattern::Literal(Literal::BigInt(_), _)
    ));
    assert!(matches!(
        stmt.arms[1].patterns[0],
        Pattern::NegatedLiteral(Literal::BigInt(_), _)
    ));
}
