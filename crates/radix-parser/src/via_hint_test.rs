//! D11.9 `via` convert-hint clause on a `↦` conversion.
//!
//! Contract: `conversio_expr ::= '↦' type via_clause? inline_default?` with
//! `via_clause ::= 'via' IDENTIFIER`. The clause binds tighter than the `⊥`
//! default, chains left to right, and `via` stays an ordinary identifier
//! everywhere the clause cannot start (it is contextual, never reserved).

use crate::test_support::{assert_parse_error_issue, parse_ok};
use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{ConversioExpr, Expr, ExprKind, Literal, StmtKind};

fn initializer(result: &ParseResult, index: usize) -> &Expr {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[index].kind else {
        panic!("expected variable declaration at statement {index}");
    };
    var.init.as_deref().expect("initializer")
}

fn conversio(expr: &Expr) -> &ConversioExpr {
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    conversio
}

fn via_name(result: &ParseResult, conversio: &ConversioExpr) -> Option<String> {
    conversio
        .via
        .as_ref()
        .map(|via| result.interner.resolve(via.name).to_owned())
}

#[test]
fn via_clause_carries_its_hint_on_the_conversion() {
    for hint in ["Hex", "Bin", "Oct", "Be", "Le", "Bits", "Code"] {
        let result = parse_ok(&format!("sit _ n ← s ↦ u32 via {hint}"));
        let conversio = conversio(initializer(&result, 0));
        assert_eq!(via_name(&result, conversio).as_deref(), Some(hint));
        assert!(conversio.recovery.is_none());
    }
}

#[test]
fn conversion_without_via_has_no_hint() {
    let result = parse_ok("sit _ n ← s ↦ u32");
    let conversio = conversio(initializer(&result, 0));
    assert!(conversio.via.is_none());
}

#[test]
fn a_hint_as_a_further_type_argument_is_rejected() {
    for source in [
        "sit _ n ← s ↦ ascii<8, Hex>",
        "sit _ n ← s ↦ ascii<_, Bin>",
        "sit _ n ← s ↦ littera<Code>",
        "sit _ n ← s ↦ ascii<8, Hex> ⊥ 0",
    ] {
        assert_parse_error_issue(source, "conversio_hint_type_argument");
    }
}

#[test]
fn a_wrapped_numeric_target_with_a_hint_is_rejected_as_the_wrapper() {
    // N7d: the wrapped numeric spelling is retired whole, so the wrapper
    // diagnostic fires first; no hint diagnostic follows on the recovered type.
    for source in [
        "sit _ n ← s ↦ numerus<u32, Hex>",
        "sit _ n ← s ↦ numerus<u32, Bits>",
        "sit _ n ← s ↦ fractus<f64, Bits>",
        "sit _ n ← s ↦ numerus<u32, Hex> ⊥ 0",
        "sit _ n ← s ↦ numerus<u32, _>",
    ] {
        assert_parse_error_issue(source, "numeric_wrapper_retired");
        let result = crate::test_support::parse_program(source);
        assert!(
            !result.errors.iter().any(|err| crate::test_support::has_arg(
                err,
                "issue",
                "conversio_hint_type_argument"
            )),
            "{source}: wrapper rejection must not cascade into a hint error"
        );
    }
}

#[test]
fn a_bracketed_hint_tail_after_the_target_is_rejected() {
    for source in [
        "sit _ n ← s ↦ lista<b8, magnitudo 16><Le>",
        "sit _ n ← s ↦ vector<u32, magnitudo 4><Be>",
        "sit _ n ← s ↦ ascii<8><Le>",
    ] {
        assert_parse_error_issue(source, "conversio_hint_tail_argument");
    }
}

#[test]
fn a_hole_and_a_via_clause_stay_accepted() {
    for source in [
        "sit _ n ← s ↦ ascii<_> via Hex",
        "sit _ n ← s ↦ ascii<8> via Hex",
        "sit _ n ← s ↦ lista<b8, magnitudo 16> via Le",
        "sit _ n ← s ↦ vector<u32, magnitudo 4> via Be",
    ] {
        let _ = parse_ok(source);
    }
}

#[test]
fn a_user_type_named_like_a_hint_is_still_a_type_argument() {
    // The retired-form check reads scalar heads only: `Code` and `Bits` are
    // ordinary nominal types as arguments of a collection target.
    let _ = parse_ok("sit _ n ← s ↦ lista<Code>");
    let _ = parse_ok("sit _ n ← s ↦ tabula<textus, Bits>");
}

#[test]
fn via_binds_tighter_than_the_up_tack_default() {
    // `x ↦ u32 via Hex ⊥ 0` is `(x ↦ u32 via Hex) ⊥ 0`: one conversion whose
    // clause is finished before the default fills the recovery slot.
    let result = parse_ok(r#"sit _ n ← "ff" ↦ u32 via Hex ⊥ 0"#);
    let conversio = conversio(initializer(&result, 0));
    assert_eq!(via_name(&result, conversio).as_deref(), Some("Hex"));
    let recovery = conversio.recovery.as_deref().expect("recovery expression");
    assert!(matches!(recovery.kind, ExprKind::Literal(Literal::Int(0))));
}

#[test]
fn chained_conversions_each_carry_their_own_via() {
    let result = parse_ok("sit _ x ← buf[0‥2] ↦ u16 via Le ↦ f16 via Bits ↦ f32");
    let outer = conversio(initializer(&result, 0));
    assert!(outer.via.is_none(), "the last hop has no clause");
    let middle = conversio(&outer.expr);
    assert_eq!(via_name(&result, middle).as_deref(), Some("Bits"));
    let inner = conversio(&middle.expr);
    assert_eq!(via_name(&result, inner).as_deref(), Some("Le"));
    assert!(matches!(inner.expr.kind, ExprKind::Index(_)));
}

#[test]
fn via_after_a_type_argument_target_keeps_the_type_arguments() {
    let result = parse_ok("sit _ n ← b ↦ ascii<8> via Hex");
    let conversio = conversio(initializer(&result, 0));
    assert_eq!(via_name(&result, conversio).as_deref(), Some("Hex"));
}

#[test]
fn via_stays_an_identifier_outside_the_clause() {
    let result = parse_ok(
        "sit _ via ← 4\nsit _ next ← via + 1\nfunctio f(numerus via) → numerus {\n    redde via\n}\n",
    );
    let next = initializer(&result, 1);
    let ExprKind::Binary(binary) = &next.kind else {
        panic!("expected binary, got {:?}", next.kind);
    };
    assert!(matches!(binary.lhs.kind, ExprKind::Ident(_)));
}

#[test]
fn a_statement_starting_with_via_after_a_conversion_is_a_new_statement() {
    // The clause is claimed only on the conversion's own line, so a following
    // line that begins with the identifier `via` does not join the conversion.
    let result = parse_ok("sit _ k ← s ↦ numerus\nsit _ via ← k\n");
    let conversio = conversio(initializer(&result, 0));
    assert!(conversio.via.is_none());
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);
}

#[test]
fn a_via_call_after_a_conversion_line_is_a_new_statement() {
    let result = parse_ok("sit _ k ← s ↦ numerus\nvia(k)\n");
    let conversio = conversio(initializer(&result, 0));
    assert!(conversio.via.is_none());
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);
}

#[test]
fn via_without_a_hint_is_a_structured_error() {
    assert_parse_error_issue("sit _ n ← s ↦ u32 via", "conversio_via_missing_hint");
    assert_parse_error_issue("sit _ n ← s ↦ u32 via ⊥ 0", "conversio_via_missing_hint");
    assert_parse_error_issue(
        "sit _ n ← s ↦ u32 via\nsit _ m ← 1\n",
        "conversio_via_missing_hint",
    );
}

#[test]
fn a_second_via_clause_is_not_part_of_the_conversion() {
    // One clause per conversion: the second `via` cannot continue it, so it
    // is left over as ordinary identifier statements (semantic analysis then
    // reports the unknown names).
    let result = parse_ok("sit _ n ← s ↦ u32 via Hex via Bin");
    let conversio = conversio(initializer(&result, 0));
    assert_eq!(via_name(&result, conversio).as_deref(), Some("Hex"));
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 3);
}

/// Stub of a translated pack that spells the clause word `عبر` and leaves
/// every other keyword unclaimed (only the clause word is under test).
struct ArabicClausePack;

impl KeywordPack for ArabicClausePack {
    fn pack_id(&self) -> &'static str {
        "ar-stub"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "عبر" => Some("via"),
            "ثابت" => Some("fixum"),
            "ليكن" => Some("sit"),
            "ليكن" => Some("sit"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "via" => Some("عبر"),
            "fixum" => Some("ثابت"),
            "sit" => Some("ليكن"),
            "sit" => Some("ليكن"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

#[test]
fn a_translated_pack_chooses_its_own_clause_word() {
    let pack = ArabicClausePack;
    let result = parse(lex_with_locale_pack(
        "ليكن _ n ← s ↦ u32 عبر Hex ⊥ 0",
        &pack,
    ));
    assert!(result.success(), "errors: {:?}", result.errors);
    let conversio = conversio(initializer(&result, 0));
    assert_eq!(via_name(&result, conversio).as_deref(), Some("Hex"));
    assert!(conversio.recovery.is_some());
}

#[test]
fn a_translated_pack_does_not_claim_the_latin_spelling() {
    // Under a pack that spells the clause `عبر`, the Latin `via` is not the
    // clause word: the conversion ends at its target and `via Hex` is left
    // over as ordinary identifier statements.
    let pack = ArabicClausePack;
    let result = parse(lex_with_locale_pack("ليكن _ n ← s ↦ u32 via Hex", &pack));
    let conversio = conversio(initializer(&result, 0));
    assert!(
        conversio.via.is_none(),
        "Latin via is not the clause word here"
    );
}
