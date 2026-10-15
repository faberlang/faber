//! Parser tests for the binding words: `let`/`sit` (typed run-time name),
//! `const`/`fixum` (compile-time only), `mut`/`varia` (writable name) and the
//! `ref`/`de` local.
//!
//! Contract: a run-time `const T x ← e` and the old English `var` spelling are
//! parsed whole and reported as PARSE010 `runtime_const_retired` and
//! `var_retired` (args `keyword` as written, `name`). A typed `let T x ← e`
//! and the inferred `let x ← e` both parse as an immutable local. A `ref` local
//! is a writable-name cell read through the `ref` access view; a missing
//! initializer, a `copy`/`own` marker and a pattern each have a named issue.
//! Tests identify diagnostics by issue and args, never by message text.

use crate::test_support::{has_arg, parse_ok, parse_program};
use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{AstMutability as Mutability, StmtKind, TypeMode};

/// English surface for the words these tests use.
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
            "ref" => Some("de"),
            "copy" => Some("copy"),
            "own" => Some("own"),
            "fn" => Some("functio"),
            "class" => Some("genus"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        (text == "int").then_some("numerus")
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnBindingPack))
}

fn body(statement: &str) -> String {
    format!("fn f() → int {{\n    {statement}\n    return 0\n}}")
}

/// The single error with this issue, asserted for `keyword` and `name`.
fn assert_issue(result: &ParseResult, issue: &str, keyword: &str, name: Option<&str>) {
    let matching = result
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", issue))
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "expected one {issue}: {:?}",
        result.errors
    );
    assert!(has_arg(matching[0], "keyword", keyword), "{matching:?}");
    if let Some(name) = name {
        assert!(has_arg(matching[0], "name", name), "{matching:?}");
    }
}

fn first_local(result: &ParseResult) -> &radix_syntax::VarDecl {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected a function, got {:?}", program.statements[0].kind);
    };
    let block = func.body.as_ref().expect("body");
    match &block.statements[0].kind {
        StmtKind::Var(decl) => decl,
        other => panic!("expected a local declaration, got {other:?}"),
    }
}

#[test]
fn runtime_const_is_retired_in_latin() {
    let result = parse_program(&body("fixum numerus x ← 5"));
    assert_issue(&result, "runtime_const_retired", "fixum", Some("x"));
}

#[test]
fn runtime_const_is_retired_in_english_and_names_the_pack_spelling() {
    let result = parse_en(&body("const int x ← 5"));
    assert_issue(&result, "runtime_const_retired", "const", Some("x"));
}

#[test]
fn deferred_runtime_const_is_retired_too() {
    let result = parse_en(&body("const int x"));
    assert_issue(&result, "runtime_const_retired", "const", Some("x"));
}

#[test]
fn compile_time_const_still_parses() {
    let _ = parse_ok("fixum numerus LIMES = 4 * 1024");
    let result = parse_en(&body("const int LIMIT = 10"));
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn typed_let_is_an_immutable_local_with_a_type() {
    let result = parse_en(&body("let int x ← 5"));
    assert!(result.success(), "{:?}", result.errors);
    let decl = first_local(&result);
    assert_eq!(decl.mutability, Mutability::Immutable);
    assert!(decl.ty.is_some(), "a typed let carries its type");
}

#[test]
fn inferred_let_keeps_its_shorthand() {
    let result = parse_en(&body("let x ← 5"));
    assert!(result.success(), "{:?}", result.errors);
    let decl = first_local(&result);
    assert_eq!(decl.mutability, Mutability::Immutable);
}

#[test]
fn typed_let_may_defer_its_initializer() {
    let result = parse_en(&body("let int x"));
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn a_let_named_like_a_mode_word_stays_a_name() {
    let result = parse_en(&body("let copy ← 1"));
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn old_var_spelling_is_retired_and_parsed_as_mut() {
    let result = parse_en(&body("var int x ← 5"));
    assert_issue(&result, "var_retired", "var", Some("x"));
    assert_eq!(first_local(&result).mutability, Mutability::Mutable);
}

#[test]
fn mut_is_the_writable_local() {
    let result = parse_en(&body("mut int x ← 5"));
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(first_local(&result).mutability, Mutability::Mutable);
}

#[test]
fn ref_local_reads_through_a_place() {
    let result = parse_en(&body("ref int cur ← head"));
    assert!(result.success(), "{:?}", result.errors);
    let decl = first_local(&result);
    assert_eq!(decl.mutability, Mutability::Mutable, "the name re-points");
    let ty = decl.ty.as_ref().expect("ref local type");
    assert_eq!(ty.mode, Some(TypeMode::De));
}

#[test]
fn a_lone_ref_stays_an_identifier() {
    let result = parse_en(&body("ref ← 1"));
    assert!(
        !result
            .errors
            .iter()
            .any(|err| has_arg(err, "issue", "ref_local_requires_initializer")),
        "{:?}",
        result.errors
    );
}

#[test]
fn ref_local_without_an_initializer_is_named() {
    let result = parse_en(&body("ref int cur"));
    assert_issue(&result, "ref_local_requires_initializer", "ref", None);
}

#[test]
fn ref_local_with_a_value_marker_is_named() {
    let result = parse_en(&body("ref int cur ← copy head"));
    assert_issue(&result, "ref_local_value_marker", "ref", None);
}

#[test]
fn ref_local_cannot_destructure() {
    let result = parse_en(&body("ref int [a, b] ← head"));
    assert_issue(&result, "ref_local_requires_name", "ref", None);
}

// BINDER POSITIONS
//
// Iteration binders, `ex` extraction, match-arm bindings, sort/reduce bodies
// and destructuring heads take `let`/`mut`. The retired `const` binder is
// `binder_fixum_retired`, and `ref` is `ref_binder_refused`; both parse whole
// as an immutable binder so recovery resumes after the head.

fn binder_body(statement: &str) -> String {
    body(&format!("{statement}\n    tacet"))
}

/// Latin body: `body` spells the English surface.
fn binder_body_la(statement: &str) -> String {
    format!("functio f() → numerus {{\n    {statement}\n    tacet\n    redde 0\n}}")
}

#[test]
fn let_and_mut_binders_parse_in_every_binder_position() {
    for source in [
        "itera ex xs sit x { tacet }",
        "itera ex xs varia x { tacet }",
        "itera ab 0‥3 sit i { tacet }",
        "ex punctum sit x, y",
        "ex punctum varia x, y",
        "sit [a, b] ← pair",
        "varia {a, b} ← pair",
        "sit lista<numerus> ys ← ordinata via ascendens ex xs sit p { redde [p] }",
        "sit numerus n ← reducta via summa ex xs apud [i] sit s { redde s }",
    ] {
        let result = parse_program(&binder_body_la(source));
        assert!(result.success(), "{source}: {:?}", result.errors);
    }
}

#[test]
fn a_const_binder_is_retired_in_each_binder_position() {
    for source in [
        "itera ex xs fixum x { tacet }",
        "ex punctum fixum x, y",
        "fixum [a, b] ← pair",
        "fixum {a, b} ← pair",
        "sit lista<numerus> ys ← ordinata via ascendens ex xs fixum p { redde [p] }",
        "sit numerus n ← reducta via summa ex xs apud [i] fixum s { redde s }",
    ] {
        let result = parse_program(&binder_body_la(source));
        assert_issue(&result, "binder_fixum_retired", "fixum", None);
    }
}

#[test]
fn a_const_binder_in_a_match_arm_is_retired() {
    let source = "discerne x {\n    casu Punctum fixum a, b { tacet }\n}";
    let result = parse_program(&binder_body_la(source));
    assert_issue(&result, "binder_fixum_retired", "fixum", None);
}

#[test]
fn a_const_binder_is_retired_in_english() {
    let result = parse_en(&binder_body("const [a, b] ← pair"));
    assert_issue(&result, "binder_fixum_retired", "const", None);
}

#[test]
fn a_ref_binder_is_refused() {
    for source in ["itera ex xs de x { tacet }", "ex punctum de x, y"] {
        let result = parse_program(&binder_body_la(source));
        assert_issue(&result, "ref_binder_refused", "de", None);
    }
}

#[test]
fn a_let_binder_pattern_list_keeps_the_et_separator_law() {
    let source = "discerne x {\n    casu Circulus sit r et Quadratum sit a, b { tacet }\n}";
    let result = parse_program(&binder_body_la(source));
    assert!(result.success(), "{:?}", result.errors);
    let comma = "discerne x {\n    casu Circulus sit r, Quadratum sit a { tacet }\n}";
    let result = parse_program(&binder_body_la(comma));
    assert!(
        result
            .errors
            .iter()
            .any(|err| has_arg(err, "issue", "discerne_pattern_comma_retired")),
        "{:?}",
        result.errors
    );
}
