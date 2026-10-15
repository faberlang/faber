//! Parser tests for the local compile-time constant (DECISIONS D5.10).
//!
//! Contract: `fixum T x = e` parses to an immutable `VarDecl` with
//! `compile_time` set (the `_` inferred slot included); `←` keeps its runtime
//! meaning with `compile_time` clear. `varia … = e` and a destructuring
//! pattern with `=` are PARSE010 errors with named issues. The English
//! spelling (`const T x = e`) parses to the same tree.

use crate::test_support::{has_arg, parse_ok, parse_program};
use crate::{ParseErrorKind, ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{AstMutability, BindingPattern, StmtKind, VarDecl};

/// English surface for the binding words only: `const`/`var` resolve to the
/// `fixum`/`varia` identities, `int` to `numerus`.
struct EnBindingPack;

impl KeywordPack for EnBindingPack {
    fn pack_id(&self) -> &'static str {
        "en-binding-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "mut" => Some("varia"),
            "fn" => Some("functio"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "int" => Some("numerus"),
            "void" => Some("vacuum"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnBindingPack))
}

/// The first local declaration inside `fn f() { … }`.
fn first_local(result: &ParseResult) -> &VarDecl {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Var(decl) = &body.statements[0].kind else {
        panic!(
            "expected local declaration, got {:?}",
            body.statements[0].kind
        );
    };
    decl
}

fn single_issue(result: &ParseResult, issue: &str) {
    let matching = result
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", issue))
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "expected one {issue} error, got {:?}",
        result.errors
    );
    assert_eq!(matching[0].kind, ParseErrorKind::DeclarationInvalid);
}

#[test]
fn latin_fixum_with_equals_is_a_compile_time_local() {
    let result = parse_ok("functio f() → vacuum { fixum numerus n = 4 * 8 }");
    let decl = first_local(&result);
    assert!(decl.compile_time);
    assert_eq!(decl.mutability, AstMutability::Immutable);
    assert!(decl.ty.is_some());
    assert!(decl.init.is_some());
    assert!(matches!(decl.binding, BindingPattern::Ident(_)));
}

#[test]
fn english_const_with_equals_is_a_compile_time_local() {
    let result = parse_en("fn f() → void { const int n = 4 * 8 }");
    assert!(result.success(), "parse errors: {:?}", result.errors);
    let decl = first_local(&result);
    assert!(decl.compile_time);
    assert_eq!(decl.mutability, AstMutability::Immutable);
}

#[test]
fn inferred_slot_is_accepted() {
    let result = parse_ok("functio f() → vacuum { fixum _ n = 7 }");
    assert!(first_local(&result).compile_time);
}

#[test]
fn arrow_initializer_stays_a_runtime_store() {
    let result = parse_ok("functio f() → vacuum { sit numerus n ← 7 }");
    assert!(!first_local(&result).compile_time);
}

#[test]
fn varia_with_equals_is_rejected() {
    let latin = parse_program("functio f() → vacuum { varia numerus n = 7 }");
    single_issue(&latin, "varia_compile_time_initializer");
    let english = parse_en("fn f() → void { mut int n = 7 }");
    single_issue(&english, "varia_compile_time_initializer");
}

#[test]
fn destructuring_with_equals_is_rejected() {
    let result = parse_program("functio f(lista<numerus> xs) → vacuum { fixum [a, b] = xs }");
    single_issue(&result, "compile_time_requires_name");
}
