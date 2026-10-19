//! Span merge and `TokenKind` classification contract (keyword, comment,
//! trivia).

use super::{Span, Symbol, TokenKind};

#[test]
fn span_merge_covers_both_inputs() {
    assert_eq!(Span::new(10, 12).merge(Span::new(2, 4)), Span::new(2, 12));
}

#[test]
fn unregistered_statement_spellings_are_plain_identifiers() {
    use crate::{lex, lookup_keyword_spec};

    // tempta / demum / emitte have no keyword spec behind them; each must lex
    // as an ordinary identifier.
    for spelling in ["tempta", "demum", "emitte"] {
        assert!(
            lookup_keyword_spec(spelling).is_none(),
            "{spelling} must not be a registered keyword"
        );
        let result = lex(spelling);
        assert!(result.errors.is_empty(), "{spelling} must lex cleanly");
        assert!(
            matches!(result.tokens[0].kind, TokenKind::Ident(_)),
            "{spelling} must lex as an identifier, got {:?}",
            result.tokens[0].kind
        );
    }

    let ident = lex("emitte_suffix");
    assert!(matches!(ident.tokens[0].kind, TokenKind::Ident(_)));
}

#[test]
fn pro_is_a_tokenless_contextual_row_that_lexes_as_an_identifier() {
    use crate::{KeywordOwner, KeywordScope, lex, lookup_keyword_spec};

    let spec = lookup_keyword_spec("pro").expect("pro has a registry row (K3 grouping word)");
    assert!(spec.token_kind.is_none(), "pro must stay tokenless");
    assert!(matches!(
        spec.scope,
        KeywordScope::Contextual(owners) if owners == [KeywordOwner::ReductionGroup]
    ));

    let result = lex("pro");
    assert!(result.errors.is_empty(), "pro must lex cleanly");
    assert!(
        matches!(result.tokens[0].kind, TokenKind::Ident(_)),
        "pro must lex as an identifier, got {:?}",
        result.tokens[0].kind
    );
}

#[test]
fn identifiers_and_comments_have_no_keyword_registry_spelling() {
    use crate::keyword_spelling_for_token;

    assert_eq!(
        keyword_spelling_for_token(&TokenKind::Ident(Symbol(0))),
        None
    );
    assert_eq!(
        keyword_spelling_for_token(&TokenKind::Underscore(Symbol(1))),
        None
    );
    assert_eq!(
        keyword_spelling_for_token(&TokenKind::LineComment(Symbol(2))),
        None
    );
    assert!(TokenKind::LineComment(Symbol(2)).is_comment());
    assert!(TokenKind::LineComment(Symbol(2)).is_trivia());
    assert!(TokenKind::Newline.is_trivia());
    assert!(!TokenKind::Newline.is_comment());
}

#[test]
fn conversio_assign_glyph_pins_exact_scalar_mapping() {
    // Visual similarity is never permission to normalize a neighbor glyph.
    assert_eq!('\u{21A4}', '↤');
    assert_ne!('\u{21A4}', '\u{2190}'); // ← ordinary assignment
    assert_ne!('\u{21A4}', '\u{21A6}'); // ↦ explicit conversion
    assert_ne!('\u{21A4}', '\u{21E5}'); // ⇥ inline recovery
    assert_ne!('\u{21A4}', '\u{21C7}'); // ⇇ copy-into
    assert_ne!('\u{21DC}', '\u{21C7}'); // ⇜ sink-into vs ⇇ copy-into
}

#[test]
fn conversio_assign_glyph_lexes_without_error() {
    let result = crate::lex("\u{21A4}");
    assert!(
        result.errors.is_empty(),
        "U+21A4 must lex as a dedicated token; errors: {:?}",
        result.errors
    );
}

#[test]
fn glyph_product_tokens_are_operators_not_registered_keywords() {
    use crate::keyword_spelling_for_token;

    for kind in [
        TokenKind::MiddleDot,
        TokenKind::Cross,
        TokenKind::CircledTimes,
        TokenKind::CircledDot,
        TokenKind::CircledDivide,
        TokenKind::Maximum,
        TokenKind::Minimum,
        TokenKind::Transpose,
        TokenKind::Nabla,
    ] {
        assert_eq!(keyword_spelling_for_token(&kind), None, "{kind:?}");
        assert!(!kind.is_comment(), "{kind:?} must not classify as trivia");
        assert!(!kind.is_trivia(), "{kind:?} must not classify as trivia");
    }
}

#[test]
fn transpose_lexes_as_dedicated_postfix_operator() {
    let result = crate::lex("xᵀ");
    assert!(
        result.errors.is_empty(),
        "U+1D40 must lex cleanly: {:?}",
        result.errors
    );
    assert!(
        result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Transpose)
    );
}

#[test]
fn nabla_lexes_as_dedicated_gradient_operator() {
    let result = crate::lex("loss(x) ∇");
    assert!(
        result.errors.is_empty(),
        "U+2207 must lex cleanly; errors: {:?}",
        result.errors
    );
    assert!(
        result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Nabla),
        "∇ must produce TokenKind::Nabla, got {:?}",
        result.tokens
    );
}

// EOT-3L: U+21C7 is one exact operator token, never a keyword identity, and
// stays distinct from the live arrow family.
#[test]
fn eot_transfer_glyph_is_a_dedicated_non_keyword_token() {
    let result = crate::lex("\u{21C7}");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(
        result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Transfer),
        "U+21C7 must lex as the dedicated Transfer token, got {:?}",
        result.tokens
    );
    assert!(
        !result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Error),
        "U+21C7 must not produce the Error recovery token"
    );
    // Distinct from every live arrow-family token.
    assert_ne!(TokenKind::Transfer, TokenKind::Assign); // ←
    assert_ne!(TokenKind::Transfer, TokenKind::Conversio); // ↦
    assert_ne!(TokenKind::Transfer, TokenKind::ExitArrow); // ⇥
    assert_ne!(TokenKind::Transfer, TokenKind::Arrow); // →
    assert_ne!(TokenKind::Transfer, TokenKind::BitwiseShl); // ⇐
    assert_ne!(TokenKind::Transfer, TokenKind::BitwiseShr); // ⇒
}

#[test]
fn radical_glyphs_pin_exact_scalars() {
    // Visual similarity is never permission to normalize a neighbor glyph.
    assert_eq!('\u{221A}', '√');
    assert_eq!('\u{221B}', '∛');
    assert_eq!('\u{221C}', '∜');
}

#[test]
fn radical_glyphs_are_distinct_kinds() {
    // Each prefix radical is its own token kind, never a spelling of another.
    assert_ne!(TokenKind::RadicalSqrt, TokenKind::RadicalCbrt); // √ vs ∛
    assert_ne!(TokenKind::RadicalSqrt, TokenKind::RadicalFourth); // √ vs ∜
    assert_ne!(TokenKind::RadicalCbrt, TokenKind::RadicalFourth); // ∛ vs ∜
    // Visual similarity to negation never merges a radical with ¬ or -.
    assert_ne!(TokenKind::RadicalSqrt, TokenKind::BitwiseNot); // √ vs ¬
    assert_ne!(TokenKind::RadicalSqrt, TokenKind::Minus); // √ vs -
    assert_ne!(TokenKind::RadicalCbrt, TokenKind::BitwiseNot); // ∛ vs ¬
    assert_ne!(TokenKind::RadicalCbrt, TokenKind::Minus); // ∛ vs -
    assert_ne!(TokenKind::RadicalFourth, TokenKind::BitwiseNot); // ∜ vs ¬
    assert_ne!(TokenKind::RadicalFourth, TokenKind::Minus); // ∜ vs -
}
