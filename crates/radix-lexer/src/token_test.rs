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

// =============================================================================
// CDA-U2 — ↤ (U+21A4) conversion-directed assignment: red token tests
// =============================================================================

#[test]
fn conversio_assign_glyph_pins_exact_scalar_mapping() {
    // CDA-U2: the conversion-directed assignment glyph is exactly U+21A4, and
    // the confusable neighbor glyphs are distinct scalars. Visual similarity is
    // never permission to normalize one glyph into another (goal decision 7).
    assert_eq!('\u{21A4}', '↤');
    assert_ne!('\u{21A4}', '\u{2190}'); // ← ordinary assignment
    assert_ne!('\u{21A4}', '\u{21A6}'); // ↦ explicit conversion
    assert_ne!('\u{21A4}', '\u{21E5}'); // ⇥ inline recovery
    assert_ne!('\u{21A4}', '\u{21C7}'); // ⇇ planned exact-output-transfer
}

#[test]
fn conversio_assign_glyph_lexes_without_error() {
    // RED (CDA-U2): U+21A4 must lex as its own token kind. Today the glyph is
    // unknown → unexpected_character, so this fails for the expected
    // missing-feature reason. U3 adds the token and this turns green.
    let result = crate::lex("\u{21A4}");
    assert!(
        result.errors.is_empty(),
        "U+21A4 must lex as a dedicated token; errors: {:?}",
        result.errors
    );
}
