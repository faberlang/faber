//! Regression matrix for the `<` divert in expression position
//! (operator need, handle).
//!
//! Contextual-keyword law under test: an identifier stays ordinary unless
//! grammatically in position. A conversio-type name followed by `<` is a
//! type-argument tail only when the tail is well-formed (`<T,…>` or
//! `<label: T,…>` for `ratio`) and followed by `(`; otherwise the `<` is a
//! comparison operator.
//!
//! Matrix (operator-verified rows):
//! | cell | expectation |
//! | --- | --- |
//! | `val < 5` | parses (ordinary identifier, comparison) |
//! | `tuple < 5` | parses (ordinary identifier, comparison) |
//! | `string < 5` (en pack) | parses (mapped spelling, malformed tail restores) |
//! | `textus > 5` | parses (comparison) |
//! | `textus < 5` | parses as comparison (was PARSE001) |
//! | `ratio < 5` | parses as comparison (was `ratio_type_arg_label_required`) |
//! | `iuncta` as declaration name | stays rejected (lexer keyword) |
//! | `sit ratio<g: f32> r ← ratio<g: f32> { g = 1.0 }` (la) | keeps parsing |
//! | `sit record<g: int> r ← record<g: int> { g = 1 }` (en pack) | constructs; head canonicalizes to `ratio` |
//! | `record < 5` (la) | parses (unmapped spelling, comparison) |
//! | `textus(5)` | parses; check-phase name resolution rejects the type-name callee |
//! | `string(5)` (en pack) | parses; check-phase name resolution rejects the type-name callee |
//! | undefined ordinary identifier + `<` | parses; definedness is a check-phase matter |
//!
//! Type-name call spellings parse as ordinary calls; the conversio surface is
//! `value ↦ Type`, and check-phase name resolution rejects a type-name callee
//! under every locale spelling.

use crate::parse;
use crate::test_support::{has_arg, parse_ok, parse_recovering_ascii_comparison};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{ExprKind, IfBody, StmtKind, TypeExprKind};

/// Minimal en-like reader pack: no keyword rows, only the [types] rows the
/// matrix exercises (spellings from `locale/en/pack.toml`).
struct EnLikeTestPack;

impl KeywordPack for EnLikeTestPack {
    fn pack_id(&self) -> &'static str {
        "en-like-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        // Identity rows so the latin keyword spellings the fixtures use still
        // claim under a non-`la` pack (pack-authoritative surface mode).
        match text {
            "incipit" => Some("incipit"),
            "fixum" => Some("fixum"),
            "sit" => Some("sit"),
            "sit" => Some("sit"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "bivalens" => Some("bivalens"),
            "string" => Some("textus"),
            "value" => Some("valor"),
            "map" => Some("tabula"),
            "list" => Some("lista"),
            "record" => Some("ratio"),
            "int" => Some("numerus"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_with_en_like_pack(source: &str) -> crate::ParseResult {
    parse(lex_with_locale_pack(source, &EnLikeTestPack))
}

fn parse_ok_with_en_like_pack(source: &str) -> crate::ParseResult {
    let result = parse_with_en_like_pack(source);
    assert!(
        result.success(),
        "expected parse success under en-like pack, got errors: {:?}",
        result.errors
    );
    result
}

/// Extract the initializer of the first `fixum`/`varia` declaration inside
/// `incipit { … }` (each matrix cell is planted as `sit bivalens ok ← <cell>`).
fn incipit_init_expr(result: &crate::ParseResult) -> &radix_syntax::Expr {
    let program = result.program.as_ref().expect("program");
    let stmt = program.statements.last().expect("statement");
    let StmtKind::Incipit(entry) = &stmt.kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(block) = &entry.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected var decl");
    };
    var.init.as_ref().expect("initializer")
}

fn assert_init_is_binary(cell: &str) {
    let source = format!("incipit {{\n    sit bivalens ok ← {cell}\n}}");
    let result = parse_recovering_ascii_comparison(&source);
    let init = incipit_init_expr(&result);
    assert!(
        matches!(init.kind, ExprKind::Binary(_)),
        "expected binary comparison for {cell:?}, got {:?}",
        init.kind
    );
}

#[test]
fn ordinary_identifier_less_than_literal_parses() {
    assert_init_is_binary("val < 5");
    assert_init_is_binary("tuple < 5");
    assert_init_is_binary("undefined_binding < 5");
}

#[test]
fn conversio_type_name_greater_than_parses_as_comparison() {
    assert_init_is_binary("textus > 5");
}

#[test]
fn conversio_type_name_less_than_parses_as_comparison() {
    // The retired-angle guard's speculative type-arg parse fails on the
    // malformed tail (`5` with no `>`); the error must restore the
    // checkpoint, not escape as PARSE001.
    assert_init_is_binary("textus < 5");
}

#[test]
fn ratio_identifier_less_than_parses_as_comparison() {
    // Both intercepts: the primary divert now requires a labeled head
    // (`IDENT/Tag` then `:`), and the postfix retired-angle guard restores on
    // the malformed tail.
    assert_init_is_binary("ratio < 5");
}

#[test]
fn en_like_pack_mapped_spelling_less_than_parses_as_comparison() {
    for cell in ["string < 5", "value < 5", "record < 5"] {
        let source = format!("incipit {{\n    sit bivalens ok ← {cell}\n}}");
        let result = parse_with_en_like_pack(&source);
        assert!(
            result
                .errors
                .iter()
                .all(|err| has_arg(err, "issue", "comparison_ascii_retired")),
            "{cell}: {:?}",
            result.errors
        );
        assert!(matches!(
            incipit_init_expr(&result).kind,
            ExprKind::Binary(_)
        ));
    }
}

#[test]
fn la_ratio_declaration_and_constructor_keep_parsing() {
    let _ = parse_ok("sit ratio<g: f32> r ← ratio<g: f32> { g = 1.0 }");
}

#[test]
fn en_record_declaration_and_constructor_parse() {
    // Locale completeness (unit 047eef65): under a reader pack the ratio
    // type surface (`record`) must claim the constructor slot too, mirroring
    // the `iuncta` identity claim — same-locale surface, no mixed
    // `record<g: int>` annotation + `ratio<g: int>` initializer.
    let result = parse_ok_with_en_like_pack(
        "incipit {\n    sit record<g: int> r ← record<g: int> { g = 1 }\n}",
    );
    let init = incipit_init_expr(&result);
    let ExprKind::Verte(verte) = &init.kind else {
        panic!("expected Verte constructor, got {:?}", init.kind);
    };
    let TypeExprKind::Named(head, args) = &verte.ty.kind else {
        panic!("expected Named type head, got {:?}", verte.ty.kind);
    };
    let resolved = result.interner.resolve(head.name);
    assert_eq!(
        resolved, "ratio",
        "constructor head must canonicalize to the latin identity"
    );
    assert_eq!(args.len(), 1, "one labeled type argument");
}

#[test]
fn unmapped_record_stays_ordinary_on_latin_surface() {
    // On the plain Latin surface `record` is unmapped: `record < 5` stays a
    // comparison and `record<g: int> { … }` is not a ratio constructor.
    assert_init_is_binary("record < 5");
}

#[test]
fn iuncta_stays_rejected_as_declaration_name() {
    let result = crate::test_support::parse_program("varia iuncta ← 5");
    assert!(
        !result.success(),
        "`iuncta` is a lexer keyword and must stay rejected as a declaration name"
    );
}

#[test]
fn unmapped_string_call_stays_ordinary_on_latin_surface() {
    // On the plain Latin surface `string` is unmapped: an ordinary call that
    // fails in check (SEM), never in parse.
    let _ = parse_ok("incipit {\n    string(5)\n}");
}
