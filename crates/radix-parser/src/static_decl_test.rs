//! Parser tests for the top-level static declaration (DECISIONS D5.8, D5.9).
//!
//! Contract: `generis T NAME = expr` at top level parses to a static
//! `VarDecl` (immutable, `is_static`). The initializer is spelled with `=`
//! only; `←` and a missing initializer are PARSE010 errors with named issues.
//! A static inside a block is PARSE010 `static_not_top_level`. `generis` as
//! a genus member keeps its field meaning.

use crate::ParseErrorKind;
use crate::test_support::{has_arg, parse_ok, parse_program};
use radix_syntax::{AstMutability, BindingPattern, StmtKind};

fn single_issue(source: &str, issue: &str) {
    let result = parse_program(source);
    let matching = result
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", issue))
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "expected one {issue} error for {source:?}, got {:?}",
        result.errors
    );
    assert_eq!(matching[0].kind, ParseErrorKind::DeclarationInvalid);
}

#[test]
fn top_level_static_parses_as_static_var_decl() {
    let result = parse_ok("generis numerus LIMES = 4 * 1024");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected var decl, got {:?}", program.statements[0].kind);
    };
    assert!(decl.is_static);
    assert_eq!(decl.mutability, AstMutability::Immutable);
    assert!(decl.ty.is_some());
    assert!(decl.init.is_some());
    assert!(matches!(decl.binding, BindingPattern::Ident(_)));
}

#[test]
fn annotated_top_level_static_parses() {
    let _ = parse_ok("@ publica\ngeneris textus NOMEN = \"x\"");
}

#[test]
fn ordinary_fixum_is_not_static() {
    let result = parse_ok("fixum numerus n ← 1");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected var decl");
    };
    assert!(!decl.is_static);
}

#[test]
fn static_with_arrow_initializer_is_rejected() {
    single_issue("generis numerus L ← 4", "static_initializer_eq");
}

#[test]
fn static_without_initializer_is_rejected() {
    single_issue(
        "generis numerus L\nfunctio f() → vacuum { tacet }",
        "static_initializer_required",
    );
}

#[test]
fn static_inside_a_block_is_rejected() {
    single_issue(
        "functio f() → numerus {\n    generis numerus L = 4\n    redde 1\n}",
        "static_not_top_level",
    );
}

#[test]
fn static_genus_field_keeps_member_meaning() {
    let _ = parse_ok("genus Capsa {\n    generis numerus MAXIMUM = 8\n    numerus n\n}");
}
