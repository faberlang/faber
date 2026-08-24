//! M1 (reduction-verbs-minmax) parser drivers and contextual-disambiguation pins.
//!
//! The admit assertions are intentionally red until the maxima/minima grammar
//! claims its contextual head. Ordinary calls, receivers, and mid-expression
//! names must remain ordinary identifiers, matching the summa precedent.

use crate::{parse, ParseResult};
use radix_lexer::{lex, lex_with_locale_pack, KeywordPack, TokenKind};
use radix_syntax::{ExprKind, StmtKind};

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

fn assert_extremum_head(result: &ParseResult, source: &str, is_maximum: bool, has_identity: bool) {
    let statement = &result.program.as_ref().expect("parsed program").statements[0];
    let StmtKind::Expr(statement) = &statement.kind else {
        panic!(
            "expected expression statement for {source:?}: {:?}",
            statement.kind
        );
    };
    match (&statement.expr.kind, is_maximum) {
        (ExprKind::Maxima(extremum), true) | (ExprKind::Minima(extremum), false) => {
            let expected_apud = usize::from(source.contains(" apud ") || source.contains(" at "));
            assert_eq!(
                extremum.apud.len(),
                expected_apud,
                "apud coordinates for {source:?}"
            );
            assert_eq!(
                extremum.coalesce.is_some(),
                has_identity,
                "identity for {source:?}"
            );
        }
        (kind, _) => panic!("wrong contextual head for {source:?}: {kind:?}"),
    }
}

#[test]
fn maxima_and_minima_heads_parse_in_latin_and_english_surfaces() {
    for (source, is_maximum, has_identity) in [
        ("maxima ex x", true, false),
        ("maxima ex x apud [i]", true, false),
        ("minima ex x apud [i] vel identity", false, true),
    ] {
        let result = parse(lex(source));
        assert_parse_ok(&result, source);
        assert_extremum_head(&result, source, is_maximum, has_identity);
    }

    for (source, is_maximum, has_identity) in [
        ("max from x", true, false),
        ("max from x at [i]", true, false),
        ("min from x at [i] coalesce identity", false, true),
    ] {
        let result = parse_en(source);
        assert_parse_ok(&result, source);
        assert_extremum_head(&result, source, is_maximum, has_identity);
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
    let latin_result = parse(lex("max from x at [i]"));
    assert_parse_ok(&latin_result, "max from x at [i]");
    let first = &latin_result
        .program
        .as_ref()
        .expect("ordinary Latin parse")
        .statements[0];
    assert!(
        matches!(&first.kind, StmtKind::Expr(statement) if matches!(statement.expr.kind, ExprKind::Ident(_))),
        "English max must remain an ordinary Latin identifier: {:?}",
        first.kind
    );
    let english_result = parse_en("maxima ex x apud [i]");
    assert_parse_ok(&english_result, "maxima ex x apud [i]");
    let first = &english_result
        .program
        .as_ref()
        .expect("ordinary English parse")
        .statements[0];
    assert!(
        matches!(&first.kind, StmtKind::Expr(statement) if matches!(statement.expr.kind, ExprKind::Ident(_))),
        "Latin maxima must remain an ordinary English identifier: {:?}",
        first.kind
    );
}
