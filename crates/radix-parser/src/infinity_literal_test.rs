//! IL-1 parser pins for the contextual infinity and NaN literals.

use crate::{parse, ParseResult};
use radix_lexer::{lex, lex_with_locale_pack, KeywordPack, TokenKind};
use radix_syntax::{ExprKind, StmtKind};

struct EnglishNanPack;

impl KeywordPack for EnglishNanPack {
    fn pack_id(&self) -> &'static str {
        "en"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        (text == "nan").then_some("nonnumerus")
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        (latin_key == "nonnumerus").then_some("nan")
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

struct LatinNanPack;

impl KeywordPack for LatinNanPack {
    fn pack_id(&self) -> &'static str {
        "la"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        (text == "nonnumerus").then_some("nonnumerus")
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        (latin_key == "nonnumerus").then_some("nonnumerus")
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn first_expr(result: &ParseResult) -> &ExprKind {
    let statement = &result.program.as_ref().expect("program").statements[0];
    let StmtKind::Expr(statement) = &statement.kind else {
        panic!("expected expression statement: {:?}", statement.kind);
    };
    &statement.expr.kind
}

#[test]
fn infinity_glyph_is_a_float_literal_in_the_primary_slot() {
    let positive = parse(lex("∞"));
    assert!(positive.success(), "parse errors: {:?}", positive.errors);
    assert!(
        matches!(first_expr(&positive), ExprKind::Literal(radix_syntax::Literal::Float(value)) if value.is_infinite())
    );

    let negative = parse(lex("-∞"));
    assert!(negative.success(), "parse errors: {:?}", negative.errors);
    assert!(
        matches!(first_expr(&negative), ExprKind::Unary(unary) if matches!(unary.op, radix_syntax::UnOp::Neg))
    );
}

#[test]
fn nonnumerus_and_english_nan_claim_only_literal_slots() {
    let latin = parse(lex("nonnumerus + 1.0"));
    assert!(latin.success(), "parse errors: {:?}", latin.errors);
    assert!(matches!(first_expr(&latin), ExprKind::Binary(_)));

    let english = parse(lex_with_locale_pack("nan + 1.0", &EnglishNanPack));
    assert!(english.success(), "parse errors: {:?}", english.errors);
    assert!(matches!(first_expr(&english), ExprKind::Binary(_)));

    // A call head stays an ordinary identifier rather than becoming a NaN
    // value. Member names are likewise parsed by the member-name slot.
    let call = parse(lex("nonnumerus(1.0)"));
    assert!(call.success(), "parse errors: {:?}", call.errors);
    assert!(matches!(first_expr(&call), ExprKind::Call(_)));
    let member = parse(lex("value.nonnumerus()"));
    assert!(member.success(), "parse errors: {:?}", member.errors);
    assert!(matches!(first_expr(&member), ExprKind::Call(_)));

    let declaration = parse(lex("functio nan(numerus x) → numerus { redde x }"));
    assert!(
        declaration.success(),
        "parse errors: {:?}",
        declaration.errors
    );
}

#[test]
fn nan_is_sealed_against_the_latin_reader_surface() {
    let result = parse(lex_with_locale_pack("nan", &LatinNanPack));
    assert!(result.success(), "parse errors: {:?}", result.errors);
    assert!(matches!(first_expr(&result), ExprKind::Ident(_)));
}
