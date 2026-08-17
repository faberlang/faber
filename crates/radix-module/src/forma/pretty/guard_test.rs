//! Leaf-owned tests for the line-sensitive grammar guard.

use super::{find_guard_hits, token_is_preservable, GuardKind};
use crate::lexer;
use crate::lexer::{Symbol, TokenKind};
use crate::parser;

/// Parse a Latin-spelling source and run the guard.
fn guard_hits(source: &str) -> Vec<super::GuardReport> {
    let lex = lexer::lex(source);
    assert!(lex.success(), "lex errors: {:?}", lex.errors);
    let parse = parser::parse(lex);
    assert!(parse.success(), "parse errors: {:?}", parse.errors);
    let program = parse.program.expect("program");
    find_guard_hits(&program)
}

#[test]
fn simple_ergo_body_passes() {
    let hits = guard_hits("functio f() → numerus {\n    si x ≻ 0 ergo redde 1\n    redde 0\n}\n");
    assert!(hits.is_empty(), "unexpected guard hits: {hits:?}");
}

#[test]
fn ergo_chain_passes() {
    let hits = guard_hits(
        "functio f() → numerus {\n    si x ≡ 1 ergo redde 1\n    sin x ≡ 2 ergo redde 2\n    secus ergo redde 3\n}\n",
    );
    assert!(hits.is_empty(), "unexpected guard hits: {hits:?}");
}

#[test]
fn ergo_over_block_owning_statement_is_flagged() {
    let hits = guard_hits("functio f() {\n    si a ergo si b { scribe \"x\" }\n}\n");
    assert_eq!(hits.len(), 1, "expected exactly one guard hit: {hits:?}");
    assert_eq!(hits[0].kind, GuardKind::ErgoBlockNesting);
}

#[test]
fn ergo_over_fac_is_flagged() {
    let hits = guard_hits("functio f() {\n    si a ergo fac { scribe \"x\" }\n}\n");
    assert_eq!(hits.len(), 1, "expected exactly one guard hit: {hits:?}");
    assert_eq!(hits[0].kind, GuardKind::ErgoBlockNesting);
}

#[test]
fn ergo_inside_nested_block_is_visited() {
    let hits = guard_hits("functio f() {\n    si a {\n        si b ergo dum c { perge }\n    }\n}\n");
    assert_eq!(hits.len(), 1, "expected one guard hit: {hits:?}");
    assert_eq!(hits[0].kind, GuardKind::ErgoBlockNesting);
}

#[test]
fn structured_annotations_are_confident() {
    let hits = guard_hits("functio f() {\n}\n@ publica\nfunctio g() → numerus {\n    redde 1\n}\n");
    assert!(hits.is_empty(), "unexpected guard hits: {hits:?}");
}

#[test]
fn annotation_tokens_are_preservable_or_not() {
    assert!(token_is_preservable(&TokenKind::Ident(Symbol(0))));
    assert!(token_is_preservable(&TokenKind::String(Symbol(0))));
    assert!(token_is_preservable(&TokenKind::Integer(1)));
    assert!(token_is_preservable(&TokenKind::Comma));
    assert!(!token_is_preservable(&TokenKind::Newline));
    assert!(!token_is_preservable(&TokenKind::Eof));
    assert!(!token_is_preservable(&TokenKind::Error));
}
