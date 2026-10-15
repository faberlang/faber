//! inf track U3 parser pins: an integer literal beyond `u64` parses to
//! `Literal::BigInt` (expression and pattern position) and stays out of JSON.

use crate::test_support::{
    ParseExpectation, assert_parse_error_issue, assert_parse_error_matches, parse_ok,
};
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
    let (interner, literal) = first_initializer_literal("sit inf x ← 18446744073709551616\n");
    let Literal::BigInt(text) = literal else {
        panic!("expected a big literal");
    };
    assert_eq!(interner.resolve(text), "18446744073709551616");
}

#[test]
fn a_u64_max_literal_stays_an_ordinary_int() {
    let (_, literal) = first_initializer_literal("sit u64 x ← 18446744073709551615\n");
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

// The Earley reference grammar (`scripta/grammar-earley.py`) reads NUMBER as
// legal in the two positions below; these are the parser half of the
// BigInteger restriction recorded there.
#[test]
fn a_big_literal_in_a_json_literal_is_rejected() {
    for source in [
        "sit json l ← { \"a\": 18446744073709551616 }\n",
        "sit json l ← { \"a\": [1, 123456789012345678901234567890] }\n",
        // A u64 above i64::MAX is an `Integer` token and fails the same checked boundary.
        "sit json l ← { \"a\": 9223372036854775808 }\n",
    ] {
        assert_parse_error_issue(source, "json_integer_overflow");
    }
}

#[test]
fn a_big_literal_as_an_enum_member_value_is_rejected() {
    assert_parse_error_matches(
        "ordo E { a = 18446744073709551616 }\n",
        ParseExpectation::Args(&[("expected", "value")]),
    );
    assert_parse_error_matches(
        "ordo E { a = -18446744073709551616 }\n",
        ParseExpectation::Args(&[("expected", "number")]),
    );
}
