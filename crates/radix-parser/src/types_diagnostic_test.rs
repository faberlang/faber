//! Structured token-contract regressions for the type-position parser.
//!
//! Grammar issues, not English sentences, must cross the parser boundary.

use super::Parser;
use crate::{ParseError, ParseErrorKind, ParseOptions};
use radix_diagnostics::DiagnosticArg;
use radix_lexer::{TokenKind, lex};

fn parser(source: &str) -> Parser {
    let lexed = lex(source);
    assert!(lexed.errors.is_empty(), "{:?}", lexed.errors);
    Parser::new(lexed.tokens, lexed.interner, ParseOptions::default())
}

fn assert_issue(error: &ParseError, issue: &str) {
    assert_eq!(error.kind, ParseErrorKind::Expected);
    assert_eq!(error.args, vec![DiagnosticArg::new("issue", issue)]);
}

#[test]
fn generic_type_closers_emit_the_type_contract_issue() {
    for source in [
        "atomic<numerus",
        "promissum<numerus ⇥ textus",
        "promissum<numerus",
        "tensor<numerus, [2]",
        "lista<numerus",
        "numerus<u8",
    ] {
        let mut parser = parser(source);
        let error = parser.parse_type().expect_err(source);
        assert_issue(&error, "type_arguments_end");
        assert_eq!(error.span, parser.current_span());
    }
}

#[test]
fn construction_type_closer_uses_the_same_issue() {
    let mut parser = parser("numerus");
    let error = parser
        .finish_iuncta_type_args_list()
        .expect_err("missing >");
    assert_issue(&error, "type_arguments_end");
}

#[test]
fn function_type_boundaries_emit_distinct_issues() {
    for (source, issue) in [
        ("numerus", "function_type_start"),
        ("(numerus", "function_type_parameters_end"),
        ("() numerus", "function_type_return_arrow"),
    ] {
        let error = parser(source).parse_func_type().expect_err(source);
        assert_issue(&error, issue);
    }
}

#[test]
fn shape_closers_preserve_their_grammar_context() {
    for (source, issue) in [
        ("[2", "shape_dimensions_end"),
        ("(2", "shape_parenthesis_end"),
    ] {
        let error = parser(source).parse_figura().expect_err(source);
        assert_issue(&error, issue);
    }
}

#[test]
fn issue_expectation_does_not_recover_facts_from_prose() {
    // Some contracts are protected by caller lookahead. Exercise their missing
    // token boundary directly without weakening that grammar guard.
    for (kind, issue) in [
        (TokenKind::LtAscii, "numeric_wrapper_start"),
        (TokenKind::Colon, "tuple_type_label_separator"),
        (TokenKind::Colon, "ratio_type_label_separator"),
    ] {
        let mut parser = parser("+");
        let span = parser.current_span();
        let error = parser
            .expect_issue(&kind, issue)
            .expect_err("missing token");
        assert_issue(&error, issue);
        assert_eq!(error.span, span);
        assert_eq!(parser.current_span(), span, "failure must not consume");
    }
}

#[test]
fn issue_expectation_still_consumes_exactly_one_matching_token() {
    let mut parser = parser("< >");
    let span = parser.current_span();
    let token = parser
        .expect_issue(&TokenKind::LtAscii, "numeric_wrapper_start")
        .expect("opening delimiter");
    assert_eq!(token.span, span);
    assert!(parser.check(&TokenKind::GtAscii));
}
