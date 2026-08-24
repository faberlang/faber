//! M1 (reduction-verbs-minmax) parser drivers and contextual-disambiguation pins.
//!
//! The admit assertions are intentionally red until the maxima/minima grammar
//! claims its contextual head. Ordinary calls, receivers, and mid-expression
//! names must remain ordinary identifiers, matching the summa precedent.

use crate::{parse, ParseResult};
use radix_lexer::{lex, lex_with_locale_pack, KeywordPack, TokenKind};

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
fn maxima_and_minima_heads_parse_in_latin_and_english_surfaces() {
    for source in [
        "maxima ex x apud [i]",
        "minima ex x apud [i] vel identity",
    ] {
        let result = parse(lex(source));
        assert_parse_ok(&result, source);
    }

    for source in [
        "max from x at [i]",
        "min from x at [i] coalesce identity",
    ] {
        let result = parse_en(source);
        assert_parse_ok(&result, source);
    }
}

#[test]
fn extrema_transform_operand_is_parenthesized_and_has_no_thread_body() {
    for source in [
        "maxima ex (x + 1) apud [i]",
        "min from (x * scale) at [i] coalesce fallback",
    ] {
        let result = if source.contains(" from ") {
            parse_en(source)
        } else {
            parse(lex(source))
        };
        assert_parse_ok(&result, source);
    }
}

#[test]
fn extrema_head_call_receiver_and_mid_expression_stay_ordinary() {
    // Latin source keeps the canonical head identities ordinary outside the
    // exact `head ex` contextual production.
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

    // The translated English surfaces keep the same non-head freedoms.
    for source in ["max(2, 3)", "min(2, 3)", "x.max()", "x.min()", "max + 1"] {
        assert_parse_ok(&parse_en(source), source);
    }
}

#[test]
fn extrema_heads_are_sealed_against_the_wrong_reader_surface() {
    // English words do not claim the Latin reader, and Latin words do not
    // claim the pack-authoritative English reader.
    assert!(
        !parse(lex("max from x at [i]")).success(),
        "English max/from/at must not claim the Latin reader surface"
    );
    assert!(
        !parse_en("maxima ex x apud [i]").success(),
        "Latin maxima/ex/apud must not claim the English reader surface"
    );
}
