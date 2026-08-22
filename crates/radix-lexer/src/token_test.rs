//! Span merge and `TokenKind` classification contract (keyword, comment,
//! trivia).

use super::{Span, Symbol, TokenKind};

#[test]
fn span_merge_covers_both_inputs() {
    assert_eq!(Span::new(10, 12).merge(Span::new(2, 4)), Span::new(2, 12));
}

#[test]
fn retired_spellings_are_plain_identifiers() {
    use crate::{lex, lookup_keyword_spec};

    // tempta / demum / emitte / pro are no longer keyword tokens; each must
    // lex as an ordinary identifier with no keyword spec behind it.
    for spelling in ["tempta", "demum", "emitte", "pro"] {
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
fn identifiers_and_comments_are_not_keywords() {
    assert!(!TokenKind::Ident(Symbol(0)).is_keyword());
    assert!(!TokenKind::Underscore(Symbol(1)).is_keyword());
    assert!(!TokenKind::LineComment(Symbol(2)).is_keyword());
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
    assert_ne!('\u{21A4}', '\u{21C7}'); // ⇇ planned exact-output-transfer
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
fn glyph_product_tokens_are_operators_not_keywords() {
    for kind in [
        TokenKind::MiddleDot,
        TokenKind::Cross,
        TokenKind::CircledTimes,
        TokenKind::CircledDot,
        TokenKind::Nabla,
    ] {
        assert!(
            !kind.is_keyword(),
            "{kind:?} is an operator token, not a keyword identity"
        );
        assert!(!kind.is_comment(), "{kind:?} must not classify as trivia");
        assert!(!kind.is_trivia(), "{kind:?} must not classify as trivia");
    }
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
    // Nabla is never a keyword identity.
    assert!(!TokenKind::Nabla.is_keyword());
}

// EOT-1 baseline: U+21C7 is not a current TokenKind and must remain distinct
// from the live arrow family until EOT-3L adds the exact transfer token.
#[test]
fn eot_transfer_glyph_is_currently_an_unexpected_character() {
    let result = crate::lex("\u{21C7}");
    let error = result
        .errors
        .iter()
        .find(|error| error.kind == crate::LexErrorKind::CharacterUnexpected)
        .expect("U+21C7 must be absent from the current token set");
    assert!(error
        .args
        .iter()
        .any(|arg| { arg.name == "issue" && arg.value == "unexpected_character" }));
    assert!(error.args.iter().any(|arg| {
        arg.name == "char" && arg.value == format!("'{}'", '\u{21C7}'.escape_default())
    }));
    assert!(
        result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Error),
        "an absent glyph must use the lexer Error recovery token"
    );
}
