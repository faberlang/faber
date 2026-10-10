//! Parser coverage for value-side `copy`/`own` markers (operator ruling
//! 2026-10-08): `var T b ← copy a`, `var T b ← own a`, `b ← copy c`, and the
//! retired binding-side spelling (`var copy T b ← a`, need), which is
//! PARSE010 `binding_marker_moved_to_value`.

use crate::test_support::{has_arg, parse_ok, parse_program};
use crate::{ParseErrorKind, ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{AssignOp, ExprKind, IfBody, StmtKind, TypeMode};

/// Surface words for the en (`const`/`var`) and la (`exemplum`/`penes`)
/// reader spellings of the declaration and marker keywords.
struct MarkerPack;

impl KeywordPack for MarkerPack {
    fn pack_id(&self) -> &'static str {
        "marker-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "const" => Some("fixum"),
            "var" => Some("varia"),
            "copy" | "exemplum" => Some("copy"),
            "own" | "penes" => Some("own"),
            "fixum" => Some("fixum"),
            "varia" => Some("varia"),
            _ => None,
        }
    }

    fn type_latin_key(&self, _text: &str) -> Option<&str> {
        None
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_surface(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &MarkerPack))
}

/// The single retired-binding-marker error, asserted for its args.
fn assert_binding_marker_retired(result: &ParseResult, keyword: &str, marker: &str, name: &str) {
    let retired: Vec<_> = result
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", "binding_marker_moved_to_value"))
        .collect();
    assert_eq!(retired.len(), 1, "{:?}", result.errors);
    let err = retired[0];
    assert!(matches!(err.kind, ParseErrorKind::DeclarationInvalid));
    assert!(has_arg(err, "keyword", keyword), "{:?}", err.args);
    assert!(has_arg(err, "marker", marker), "{:?}", err.args);
    assert!(has_arg(err, "name", name), "{:?}", err.args);
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

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

#[test]
fn binding_marker_copy_is_retired_in_the_latin_surface() {
    let result = parse_program("incipit {\n    fixum copy lista<numerus> b ← a\n}");
    assert_binding_marker_retired(&result, "fixum", "copy", "b");
}

#[test]
fn binding_marker_own_is_retired_in_the_latin_surface() {
    let result = parse_program("incipit {\n    varia own lista<numerus> b ← a\n}");
    assert_binding_marker_retired(&result, "varia", "own", "b");
}

#[test]
fn binding_marker_copy_is_retired_in_the_en_surface() {
    let result = parse_surface("incipit {\n    var copy Punctum b ← a\n}");
    assert_binding_marker_retired(&result, "var", "copy", "b");
}

#[test]
fn binding_marker_own_is_retired_in_the_en_surface() {
    let result = parse_surface("incipit {\n    const own Punctum b ← a\n}");
    assert_binding_marker_retired(&result, "const", "own", "b");
}

#[test]
fn binding_marker_is_retired_in_the_la_surface_spellings() {
    let result = parse_surface("incipit {\n    fixum exemplum Punctum b ← a\n}");
    assert_binding_marker_retired(&result, "fixum", "exemplum", "b");
    let result = parse_surface("incipit {\n    varia penes Punctum b ← a\n}");
    assert_binding_marker_retired(&result, "varia", "penes", "b");
}

#[test]
fn binding_marker_retirement_leaves_parameter_markers_alone() {
    let _ = parse_ok("functio f(copy lista<numerus> xs, own lista<numerus> ys) → vacuum {}");
}

#[test]
fn binding_marker_retirement_leaves_the_value_side_form_clean() {
    let result = parse_surface("incipit {\n    var Punctum b ← copy a\n}");
    assert!(result.success(), "{:?}", result.errors);
}
