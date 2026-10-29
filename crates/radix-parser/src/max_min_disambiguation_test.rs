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
            "reduce" => Some("reducta"),
            "via" => Some("via"),
            "max" => Some("maxima"),
            "min" => Some("minima"),
            "from" => Some("ex"),
            "at" => Some("apud"),
            "per" => Some("pro"),
            "coalesce" => Some("vel"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "reducta" => Some("reduce"),
            "via" => Some("via"),
            "maxima" => Some("max"),
            "minima" => Some("min"),
            "ex" => Some("from"),
            "apud" => Some("at"),
            "pro" => Some("per"),
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
fn bare_group_key_stops_before_reduction_coalesce_identity_in_both_locales() {
    let cases = [
        (
            "reduce via max from w at [i] per k[i] coalesce 0.5",
            parse_en("reduce via max from w at [i] per k[i] coalesce 0.5"),
        ),
        (
            "reducta via maxima ex w apud [i] pro k[i] vel 0.5",
            parse(lex("reducta via maxima ex w apud [i] pro k[i] vel 0.5")),
        ),
    ];
    for (source, result) in cases {
        assert_parse_ok(&result, source);
        let program = result.program.as_ref().expect("program");
        let radix_syntax::StmtKind::Expr(statement) = &program.statements[0].kind else {
            panic!("expected expression statement");
        };
        let radix_syntax::ExprKind::Reducta(reduction) = &statement.expr.kind else {
            panic!("expected reduction expression");
        };
        assert_eq!(reduction.op, radix_syntax::ReductaOp::Max);
        let Some(key) = reduction.group.as_deref() else {
            panic!("expected bare grouping key");
        };
        let radix_syntax::ExprKind::Index(index) = &key.kind else {
            panic!("expected indexed key, got {:?}", key.kind);
        };
        let radix_syntax::ExprKind::Ident(key_name) = &index.object.kind else {
            panic!("expected key tensor name");
        };
        assert_eq!(result.interner.resolve(key_name.name), "k");
        let Some(identity) = reduction.identity.as_deref() else {
            panic!("expected reduction identity");
        };
        assert!(matches!(
            identity.kind,
            radix_syntax::ExprKind::Literal(
                radix_syntax::Literal::Float(value)
                    | radix_syntax::Literal::FloatText { value, .. }
            ) if value == 0.5
        ));
    }
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
