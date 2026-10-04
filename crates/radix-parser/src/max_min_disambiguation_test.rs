//! K3 parser pins that the reduction operator words remain ordinary names
//! outside the owned `reducta via` expression form.

use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex, lex_with_locale_pack};

/// Minimal pack-authoritative English surface for the reduction grammar.
///
/// The real `en` pack also maps the declaration and type vocabulary; these
/// parser-only fixtures need only the words participating in the expression.
struct EnReductionPack;

impl KeywordPack for EnReductionPack {
    fn pack_id(&self) -> &'static str {
        "en"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "max" => Some("maxima"),
            "min" => Some("minima"),
            "from" => Some("ex"),
            "at" => Some("apud"),
            "coalesce" => Some("vel"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "maxima" => Some("max"),
            "minima" => Some("min"),
            "ex" => Some("from"),
            "apud" => Some("at"),
            "vel" => Some("coalesce"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnReductionPack))
}

fn assert_parse_ok(result: &ParseResult, source: &str) {
    assert!(
        result.success(),
        "expected parse success for {source:?}, got {:?}",
        result.errors
    );
}

#[test]
fn extrema_head_call_receiver_and_mid_expression_stay_ordinary() {
    // Latin operator words remain ordinary outside `reducta via`.
    for source in [
        "maxima(2, 3)",
        "minima(2, 3)",
        "x.maxima()",
        "x.minima()",
        "maxima + 1",
        "minima + 1",
    ] {
        assert_parse_ok(&parse(lex(source)), source);
    }

    // The English operator words have the same ordinary-name behavior.
    for source in ["max(2, 3)", "min(2, 3)", "x.max()", "x.min()", "max + 1"] {
        assert_parse_ok(&parse_en(source), source);
    }
}
