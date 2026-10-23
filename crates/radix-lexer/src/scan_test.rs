//! Integration-level lexer coverage: token recognition, error recovery, NFKC
//! normalization, and `#` comment policy.
//!
//! These tests exercise the public `lex()` path over operators, keywords,
//! literals, retired syntax, and diagnostic emission. They do not depend on
//! parser or semantic layers.

use super::{HOMOGLYPH_ISSUE, INLINE_COMMENT_ISSUE, Interner};
use crate::{
    KeywordPack, KeywordScope, LexErrorKind, LexResult, TokenKind, keyword_specs, lex,
    lex_with_locale_pack, lookup_keyword_spec, textus_literal::decode_textus_literal,
};

fn has_arg(err: &crate::LexError, name: &str, value: &str) -> bool {
    err.args
        .iter()
        .any(|arg| arg.name == name && arg.value == value)
}

// =============================================================================
// 3.1a — Empty input
// =============================================================================

#[test]
fn empty_input_produces_only_eof() {
    let result = lex("");
    assert!(
        result.errors.is_empty(),
        "empty input should produce no errors"
    );
    assert_eq!(
        result.tokens.len(),
        1,
        "empty input should yield exactly one token (EOF)"
    );
    assert_eq!(result.tokens[0].kind, TokenKind::Eof);
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 0));
}

// =============================================================================
// 3.1b — Whitespace-only input
// =============================================================================

#[test]
fn whitespace_only_input_produces_only_eof() {
    let result = lex("  \t  ");
    assert!(
        result.errors.is_empty(),
        "whitespace-only input should produce no errors"
    );
    assert_eq!(
        result.tokens.len(),
        1,
        "whitespace-only should yield exactly one token (EOF)"
    );
    assert_eq!(result.tokens[0].kind, TokenKind::Eof);
    assert_eq!(result.tokens[0].span, crate::Span::new(5, 5));
}

#[test]
fn lexes_unicode_identifiers() {
    let result = lex("fixum 变量 = 1");
    assert!(result.errors.is_empty());
    assert!(
        result
            .tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Ident(_)))
    );
    // 3.1c — span: fixum at 0..5
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 5));
    // `fixum` is tokenless since Batch E; it lexes as an identifier.
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
}

#[test]
fn lexes_therefore_glyph_as_therefore_token() {
    let result = lex("numerus x ∴ x");
    assert!(result.errors.is_empty());
    assert!(
        result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Therefore)
    );
    // 3.1c — span: numerus ident at 0..7, ∴ glyph at 10..13
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 7));
    let therefore = result
        .tokens
        .iter()
        .find(|t| t.kind == TokenKind::Therefore)
        .expect("∴ token must exist");
    assert_eq!(therefore.span, crate::Span::new(10, 13));
}

#[test]
fn lexes_control_flow_keywords_as_identifiers_after_tokenless_migration() {
    let result = lex("si verum ergo");
    assert!(result.errors.is_empty());
    // None of the migrated control-flow/literal words is `Therefore`.
    assert!(
        !result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Therefore)
    );
    for token in result.tokens.iter().filter(|t| t.kind != TokenKind::Eof) {
        assert!(
            matches!(token.kind, TokenKind::Ident(_)),
            "control-flow keyword should lex as an identifier, got {:?}",
            token.kind
        );
    }
    // 3.1c — spans unchanged: si at 0..2, ergo at 9..13
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 2));
    assert_eq!(result.tokens[2].span, crate::Span::new(9, 13));
}

#[test]
fn lexes_modulus_as_identifier_after_tokenless_migration() {
    let result = lex("modulus chorda {");
    assert!(result.errors.is_empty());
    // Every keyword lexes as an identifier; the parser claims it by identity
    // in type position.
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    let spec = lookup_keyword_spec("modulus").expect("modulus should be registered");
    assert_eq!(spec.token_kind, Some(TokenKind::Modulus));
    // 3.1c — span: modulus at 0..7
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 7));
}

#[test]
fn keyword_registry_specs_lex_to_current_tokens() {
    for spec in keyword_specs() {
        let result = lex(spec.text);
        assert!(
            result.errors.is_empty(),
            "failed to lex keyword spec '{}'",
            spec.text
        );
        // Keywords lex as identifiers; the identity resolves through the
        // recorded latin-key map, so the parser can claim any of them.
        assert!(
            matches!(result.tokens[0].kind, TokenKind::Ident(_)),
            "keyword '{}' must lex as an identifier",
            spec.text
        );
        if spec.token_kind.is_some() {
            // Identity-backed rows record their Latin spelling so the parser
            // can claim the identifier by the registry token identity.
            assert_eq!(
                result.reader_keyword_latin_keys.get(spec.text),
                Some(&spec.text.to_string()),
                "latin key not recorded for token-backed keyword '{}'",
                spec.text
            );
        } else {
            // Tokenless rows are ordinary identifiers. Their parser-owned
            // contextual scope is the lookup path; they have no token
            // identity and therefore must not acquire a Latin-key entry.
            assert!(
                !result.reader_keyword_latin_keys.contains_key(spec.text),
                "tokenless identifier '{}' must not record a token latin key",
                spec.text
            );
            assert!(
                matches!(spec.scope, KeywordScope::Contextual(owners) if !owners.is_empty()),
                "tokenless identifier '{}' must retain a contextual parser owner",
                spec.text
            );
        }
    }
}

#[test]
fn keyword_lookup_uses_nfkc_normalized_identifier_text() {
    let result = lex("ｃａｐｅ textus name ← \"ok\"");

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    // Keywords lex as identifiers; the NFKC-normalized spelling resolves to
    // the latin identity through the recorded key map.
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    assert_eq!(
        result.reader_keyword_latin_keys.get("cape"),
        Some(&"cape".to_string())
    );
}

#[test]
fn infinity_glyph_lexes_as_an_infinite_float() {
    let result = lex("∞ -∞");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(
        matches!(result.tokens[0].kind, TokenKind::Float(value) if value.is_infinite() && value.is_sign_positive())
    );
    assert_eq!(result.tokens[1].kind, TokenKind::Minus);
    assert!(
        matches!(result.tokens[2].kind, TokenKind::Float(value) if value.is_infinite() && value.is_sign_positive())
    );
}

#[test]
fn nonnumerus_is_a_tokenless_literal_registry_row() {
    let spec = lookup_keyword_spec("nonnumerus").expect("nonnumerus should be registered");
    assert!(spec.token_kind.is_none());
    assert!(
        matches!(spec.scope, KeywordScope::Contextual(owners) if owners.contains(&crate::KeywordOwner::Literal))
    );

    let result = lex("nonnumerus");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    assert!(!result.reader_keyword_latin_keys.contains_key("nonnumerus"));
}

#[test]
fn allocator_kind_names_are_not_keywords() {
    assert!(lookup_keyword_spec("arena").is_none());
    assert!(lookup_keyword_spec("page").is_none());

    let result = lex("sit _ arena ← page");
    let idents = result
        .tokens
        .iter()
        .filter(|token| matches!(token.kind, TokenKind::Ident(_)))
        .count();
    // `fixum` is tokenless since Batch E, so it also counts as an identifier.
    assert_eq!(idents, 3);
}

#[test]
fn keyword_registry_alias_entries_preserve_source_text() {
    let spec =
        lookup_keyword_spec("scribe").expect("scribe compatibility spelling should be registered");
    assert_eq!(spec.text, "scribe");

    match spec.scope {
        KeywordScope::Alias { canonical } => assert_eq!(canonical, "nota"),
        other => panic!("scribe should be recorded as an alias, got {other:?}"),
    }
}

#[test]
fn keyword_registry_texts_are_unique() {
    let mut texts = std::collections::BTreeSet::new();
    for spec in keyword_specs() {
        assert!(
            texts.insert(spec.text),
            "duplicate KeywordSpec for '{}'",
            spec.text
        );
    }
}

#[test]
fn rejects_line_start_section_directive() {
    let result = lex("§ sectio \"smoke\"\n");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::CharacterUnexpected
            && has_arg(err, "issue", "unexpected_character")
    }));
}

#[test]
fn rejects_c_style_line_comment() {
    let result = lex("incipit {\n    // not a Faber comment\n}\n");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::CommentSyntaxInvalid
            && has_arg(err, "issue", "c_style_line_comment")
    }));
    assert!(
        !result
            .tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Slash))
    );
}

#[test]
fn rejects_c_style_block_comment() {
    let result = lex("incipit { /* block */ }\n");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::CommentSyntaxInvalid
            && has_arg(err, "issue", "c_style_block_comment")
    }));
}

#[test]
fn rejects_unterminated_c_style_block_comment() {
    let result = lex("incipit { /* open\n}\n");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::CommentSyntaxInvalid
            && has_arg(err, "issue", "unterminated_c_style_block_comment")
    }));
}

#[test]
fn hash_after_c_block_comment_with_newline_is_line_comment() {
    // The newline token after `*/` resets line-start state, so `#` on the
    // following line is trivia rather than an inline-comment error.
    let result = lex("/*\n*/\n# good\n");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::CommentSyntaxInvalid
            && has_arg(err, "issue", "c_style_block_comment")
    }));
    assert!(
        result
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::LineComment(_))),
        "expected LineComment on the line after /* */"
    );
}

#[test]
fn hash_after_terminated_c_block_comment_with_newline_is_line_comment() {
    let result = lex("/*\n*/\n# next\n");
    assert!(!result.success());
    assert!(
        result
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::LineComment(_))),
        "expected LineComment after terminated /* */ with interior newline"
    );
}

// ---------------------------------------------------------------------------

#[test]
fn division_slash_remains_valid_operator() {
    let result = lex("sit numerus x ← 10 / 2");
    assert!(result.success(), "errors: {:?}", result.errors);
    assert!(
        result
            .tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Slash))
    );
    // 3.1c — span: sit at 0..3, slash at 21..22
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 3));
    // `sit` is tokenless since Batch E; it lexes as an identifier.
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    let slash = result
        .tokens
        .iter()
        .find(|t| matches!(t.kind, TokenKind::Slash))
        .expect("slash token must exist");
    assert_eq!(slash.span, crate::Span::new(21, 22));
}

#[test]
fn string_template_section_holes_remain_valid() {
    let result = lex(r#"sit _ greeting ← "Salve, §!"(name)"#);
    assert!(result.success(), "errors: {:?}", result.errors);
}

#[test]
fn rejects_non_xid_identifier_start() {
    let result = lex("\u{0301}abc");
    assert!(!result.errors.is_empty());
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::CharacterUnexpected)
    );
}

#[test]
fn digit_run_then_letters_lex_as_integer_then_identifier() {
    // "1abc" is not a single invalid identifier: the number scanner consumes `1`,
    // then `abc` is a separate Ident. No UnexpectedCharacter diagnostic.
    let result = lex("1abc");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(
        matches!(result.tokens[0].kind, TokenKind::Integer(1)),
        "got {:?}",
        result.tokens[0].kind
    );
    assert!(
        matches!(result.tokens[1].kind, TokenKind::Ident(_)),
        "got {:?}",
        result.tokens[1].kind
    );
}

#[test]
fn rejects_null_byte_in_source() {
    let result = lex("abc\0def");
    assert!(!result.errors.is_empty());
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::CharacterUnexpected)
    );
}

#[test]
fn empty_line_comment_produces_comment_token() {
    let result = lex("#\n");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(matches!(result.tokens[0].kind, TokenKind::LineComment(_)));
}

#[test]
fn unexpected_bidi_control_diagnostic_uses_codepoint_not_raw_control() {
    let raw = '\u{202E}';
    let result = lex(&format!("{raw}abc"));
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == LexErrorKind::CharacterUnexpected)
        .expect("unexpected bidi control diagnostic");

    assert!(has_arg(err, "char", "U+202E"), "{:?}", err.args);
    assert!(
        !err.args.iter().any(|arg| arg.value.contains(raw)),
        "{:?}",
        err.args
    );
}

#[test]
fn mixed_script_identifier_advises_and_still_lexes_as_ident() {
    // Cyrillic `а` beside Latin `dd`: the defined-confusable shape that was
    // silent before the S-A5 screen. Advisory only — no error token.
    let result = lex("аdd");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(
        result.mixed_script_advisories.len(),
        1,
        "expected one mixed-script advisory"
    );
    let advisory = &result.mixed_script_advisories[0];
    assert_eq!(advisory.spelling, "аdd");
    assert_eq!(
        advisory.scripts,
        vec!["Cyrillic".to_string(), "Latin".to_string()],
        "scripts sorted, Common/Inherited excluded"
    );
    assert_eq!(advisory.span, crate::Span::new(0, 4));
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
}

#[test]
fn single_script_identifiers_yield_no_mixed_script_advisory() {
    // Pure Latin, pure Cyrillic, pure Greek: one visible script each.
    for source in ["add", "значення", "μεταβλητη"] {
        let result = lex(source);
        assert!(
            result.mixed_script_advisories.is_empty(),
            "{source} should not advise: {:?}",
            result.mixed_script_advisories
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }
}

#[test]
fn mixed_script_with_common_characters_yields_no_advisory() {
    // Digits and `_` are Common: a Greek name with ASCII digits/underscore
    // stays single-script under the S-A5 screen.
    for source in ["α_1", "β12", "_x"] {
        let result = lex(source);
        assert!(
            result.mixed_script_advisories.is_empty(),
            "{source} should not advise: {:?}",
            result.mixed_script_advisories
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }
}

#[test]
fn normalizes_interned_identifiers_to_nfkc() {
    // Canonical NFC equivalence (composed vs decomposed) still holds under NFKC.
    let mut interner = Interner::new();
    let composed = interner.intern("café");
    let decomposed = interner.intern("cafe\u{301}");

    assert_eq!(composed, decomposed);
    assert_eq!(interner.resolve(composed), "café");

    // NFKC additionally applies compatibility decompositions.
    // Full-width Latin letters normalize to their ASCII equivalents.
    let half_width = interner.intern("nomen");
    let full_width = interner.intern("\u{FF4E}\u{FF4F}\u{FF4D}\u{FF45}\u{FF4E}");
    assert_eq!(
        half_width, full_width,
        "full-width identifiers must normalize to ASCII under NFKC"
    );
}

#[test]
fn lookup_finds_interned_string_without_appending() {
    // Reverse lookup reads the existing table: byte-exact keys, no
    // normalization, never an insertion.
    let mut interner = Interner::new();
    let sym = interner.intern_raw("sunt");

    assert_eq!(interner.lookup("sunt"), Some(sym));
    assert_eq!(interner.lookup("numquam"), None);
    assert_eq!(interner.strings().len(), 1, "lookup must never append");
}

#[test]
fn string_literal_payloads_preserve_source_bytes() {
    // PRP-013 R3: full-width parens inside a string literal must not collapse
    // via NFKC. Literal payloads preserve source bytes; only identifiers use NFKC.
    let result = lex("\"hello\u{FF08}world\u{FF09}\"");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let str_token = result
        .tokens
        .iter()
        .find(|t| matches!(t.kind, TokenKind::String(_)));
    let str_token = str_token.expect("expected string literal token");
    if let TokenKind::String(sym) = str_token.kind {
        let content = result.interner.resolve(sym);
        assert_eq!(
            content, "hello\u{FF08}world\u{FF09}",
            "string literal payloads must preserve full-width source bytes"
        );
        assert_ne!(
            content, "hello(world)",
            "string literal payloads must not be NFKC-collapsed"
        );
    } else {
        panic!("token kind mismatch");
    }
}

#[test]
fn comment_payloads_preserve_source_bytes() {
    // PRP-013: LineComment payload is not NFKC-normalized.
    let result = lex("# full-width \u{FF08}\n");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let comment = result
        .tokens
        .iter()
        .find(|t| matches!(t.kind, TokenKind::LineComment(_)))
        .expect("expected LineComment token");
    if let TokenKind::LineComment(sym) = comment.kind {
        let content = result.interner.resolve(sym);
        assert_eq!(
            content, " full-width \u{FF08}",
            "comment payload must preserve full-width source bytes"
        );
    } else {
        panic!("token kind mismatch");
    }
}

#[test]
fn string_literal_preserves_distinct_unicode_forms() {
    // Identifiers NFKC-merge café forms; string literals must stay distinct.
    let composed = lex("\"café\"");
    let decomposed = lex("\"cafe\u{0301}\"");
    assert!(composed.errors.is_empty() && decomposed.errors.is_empty());

    let c_sym = match &composed.tokens[0].kind {
        TokenKind::String(s) => *s,
        other => panic!("expected String, got {other:?}"),
    };
    let d_sym = match &decomposed.tokens[0].kind {
        TokenKind::String(s) => *s,
        other => panic!("expected String, got {other:?}"),
    };
    assert_eq!(composed.interner.resolve(c_sym), "café");
    assert_eq!(decomposed.interner.resolve(d_sym), "cafe\u{0301}");
}

#[test]
fn nfkc_does_not_reject_ideographic_punctuation() {
    // Ideographic 。 (U+3002) has no NFKC compatibility decomposition.
    // It remains a lex error (unexpected character) just as under NFC.
    let result = lex("。test");
    assert!(
        !result.errors.is_empty(),
        "ideographic punctuation remains a lex error under NFKC"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e.kind, LexErrorKind::CharacterUnexpected)),
        "expected UnexpectedCharacter error, got {:?}",
        result.errors
    );
}

#[test]
fn lexer_interns_equivalent_unicode_forms_as_one_symbol() {
    let result = lex("cape café = 1\ncape cafe\u{301} = 2");
    assert!(result.errors.is_empty());

    let idents: Vec<_> = result
        .tokens
        .iter()
        .filter_map(|token| match token.kind {
            TokenKind::Ident(sym) => Some(sym),
            _ => None,
        })
        .collect();

    // `cape` now lexes as an identifier too; both spellings of `cape` and both
    // spellings of `café` must intern to one symbol each (NFKC equivalence).
    assert_eq!(idents[0], idents[2]);
    assert_eq!(idents[1], idents[3]);
}

// ---------------------------------------------------------------------------
// Operator glyph groups (split from one mega-test for fast diagnostics)
// ---------------------------------------------------------------------------

#[test]
fn lexes_arithmetic_operators() {
    let result = lex("+ - * / %");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_glyph_product_operators() {
    let result = lex("· × ⊗ ⊙ ⊘");
    assert!(
        result.errors.is_empty(),
        "glyph products must lex cleanly, errors: {:?}",
        result.errors
    );
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::MiddleDot,
            TokenKind::Cross,
            TokenKind::CircledTimes,
            TokenKind::CircledDot,
            TokenKind::CircledDivide,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_obelus_true_division() {
    let result = lex("a ÷ b");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(kinds[1], TokenKind::Obelus);
}

#[test]
fn lexes_glyph_products_mixed_with_arithmetic_chain() {
    let result = lex("a · b × c");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert!(matches!(kinds[0], TokenKind::Ident(_)));
    assert_eq!(kinds[1], TokenKind::MiddleDot);
    assert!(matches!(kinds[2], TokenKind::Ident(_)));
    assert_eq!(kinds[3], TokenKind::Cross);
    assert!(matches!(kinds[4], TokenKind::Ident(_)));
    assert_eq!(kinds[5], TokenKind::Eof);
}

#[test]
fn lexes_minmax_infix_glyphs() {
    let result = lex("⤒ ⤓");
    assert!(
        result.errors.is_empty(),
        "min/max glyphs must lex cleanly: {:?}",
        result.errors
    );
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![TokenKind::Maximum, TokenKind::Minimum, TokenKind::Eof]
    );
}

#[test]
fn minmax_confusable_arrows_suggest_canonical_glyphs() {
    // Live ↑/↓ remain PostInc/PostDec; the rejected neighbors below must teach
    // authors the new to-bar glyph instead of silently accepting a lookalike.
    for (source, suggestion) in [
        ("⇡", "⤒"), // U+21E1 UPWARDS DASHED ARROW
        ("⇣", "⤓"), // U+21E3 DOWNWARDS DASHED ARROW
        ("⇑", "⤒"), // U+21D1 UPWARDS DOUBLE ARROW
        ("⇓", "⤓"), // U+21D3 DOWNWARDS DOUBLE ARROW
        ("↥", "⤒"), // U+21A5 UPWARDS ARROW FROM BAR
        ("↧", "⤓"), // U+21A7 DOWNWARDS ARROW FROM BAR
        ("⤈", "⤓"), // U+2908 DOWNWARDS ARROW WITH HORIZONTAL STROKE
        ("⤉", "⤒"), // U+2909 UPWARDS ARROW WITH HORIZONTAL STROKE
        ("⤊", "⤒"), // U+290A UPWARDS TRIPLE ARROW
        ("⤋", "⤓"), // U+290B DOWNWARDS TRIPLE ARROW
    ] {
        assert_homoglyph_did_you_mean(source, source, suggestion);
    }

    let upward = lex("↑");
    assert_eq!(
        upward.tokens[0].kind,
        TokenKind::PostInc,
        "the existing postfix ↑ token is not a rejected confusable"
    );
    assert!(upward.errors.is_empty());
    let downward = lex("↓");
    assert_eq!(downward.tokens[0].kind, TokenKind::PostDec);
    assert!(downward.errors.is_empty());
}

#[test]
fn glyph_product_glyph_codepoints_are_distinct() {
    // Visual similarity is never permission to normalize a neighbor glyph.
    assert_eq!('\u{00B7}', '·'); // middle dot
    assert_eq!('\u{00D7}', '×'); // multiplication sign
    assert_eq!('\u{2297}', '⊗'); // circled times
    assert_eq!('\u{2299}', '⊙'); // circled dot
    assert_eq!('\u{2298}', '⊘'); // circled division slash
    assert_ne!('\u{00B7}', '*'); // not ASCII star
    assert_ne!('\u{00D7}', 'x'); // not ASCII x
    assert_ne!('\u{2297}', '\u{00D7}'); // not the multiplication sign
    assert_ne!('\u{2299}', '\u{00B7}'); // not the middle dot
}

fn assert_homoglyph_did_you_mean(source: &str, lookalike: &str, suggestion: &str) {
    let result = lex(source);
    let error = result
        .errors
        .iter()
        .find(|error| has_arg(error, "issue", HOMOGLYPH_ISSUE))
        .unwrap_or_else(|| {
            panic!(
                "expected homoglyph diagnostic for {lookalike}: {:?}",
                result.errors
            )
        });

    assert_eq!(error.kind, LexErrorKind::CharacterUnexpected);
    assert!(has_arg(error, "char", lookalike), "{:?}", error.args);
    assert!(has_arg(error, "suggestion", suggestion), "{:?}", error.args);
    assert!(
        result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Error)
    );
    assert!(result.reader_keyword_latin_keys.is_empty());
}

#[test]
fn circled_ring_homoglyph_suggests_hadamard_product() {
    assert_homoglyph_did_you_mean("∘", "∘", "⊙");
}

#[test]
fn norm_homoglyphs_suggest_method_shaped_norms() {
    assert_homoglyph_did_you_mean("‖", "‖", ".rms_norm(...)");
    assert_homoglyph_did_you_mean("∥", "∥", ".rms_norm(...)");
}

#[test]
fn transpose_lookalikes_get_the_canonical_suffix_advice() {
    let transpose = lex("ᵀ");
    assert!(transpose.errors.is_empty(), "ᵀ is the admitted token");
    assert!(
        transpose
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Transpose)
    );

    assert_homoglyph_did_you_mean("ᵗ", "ᵗ", "ᵀ");

    // Plain `T` remains a legal identifier: the language cannot reject it at
    // lex time without breaking generic type parameters and ordinary names.
    let plain_t = lex("T");
    assert!(plain_t.errors.is_empty(), "plain T remains an identifier");
    assert!(plain_t.reader_keyword_latin_keys.is_empty());

    for glyph in ["⊚", "⊛", "⊜", "⊝"] {
        let result = lex(glyph);
        assert!(
            result.errors.iter().any(|error| {
                error.kind == LexErrorKind::CharacterUnexpected
                    && has_arg(error, "issue", "unexpected_character")
                    && !error.args.iter().any(|arg| arg.name == "suggestion")
            }),
            "{glyph} must not receive a map-correct glyph suggestion: {:?}",
            result.errors
        );
    }
}

#[test]
fn lexes_postfix_inc_dec_operators() {
    let result = lex("↑ ↓");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![TokenKind::PostInc, TokenKind::PostDec, TokenKind::Eof,]
    );
}

#[test]
fn lexes_equality_comparison_operators() {
    let result = lex("≡ ≢");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![TokenKind::EqEq, TokenKind::BangEq, TokenKind::Eof,]
    );
}

/// U+2260 is retired (ruling 6b5c1c615): it lexes to its own token, never to
/// `BangEq`, so the parser can name `≢` in the diagnostic.
#[test]
fn retired_not_equal_glyph_lexes_to_its_own_token() {
    let result = lex("≡ ≠");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![TokenKind::EqEq, TokenKind::BangEqRetired, TokenKind::Eof,]
    );
}

#[test]
fn lexes_equality_ladder_to_split_tokens() {
    let result = lex("≡ ≢ ≅ ≇ ≈ ≉");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::EqEq,
            TokenKind::BangEq,
            TokenKind::ApproxEq,
            TokenKind::ApproxNotEq,
            TokenKind::FuzzyEq,
            TokenKind::FuzzyNotEq,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_relational_operators() {
    let result = lex("< ≤ ≺ > ≥ ≻");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::LtAscii,
            TokenKind::LtEq,
            TokenKind::Lt,
            TokenKind::GtAscii,
            TokenKind::GtEq,
            TokenKind::Gt,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_logical_bitwise_operators() {
    let result = lex("⊻ ¬ ∧ ∨");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::BitwiseXor,
            TokenKind::BitwiseNot,
            TokenKind::BitwiseAnd,
            TokenKind::BitwiseOr,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_arrow_shift_operators() {
    let result = lex("→ ⇥ ⇐ ⇒ ↦ ∷");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Arrow,
            TokenKind::ExitArrow,
            TokenKind::BitwiseShl,
            TokenKind::BitwiseShr,
            TokenKind::Conversio,
            TokenKind::Verte,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_bang_prefix_operators() {
    let result = lex("!. ![ !(");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::BangDot,
            TokenKind::BangBracket,
            TokenKind::BangParen,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_null_conditional_operators() {
    let result = lex("?. ?[ ?( ??");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::QuestionDot,
            TokenKind::QuestionBracket,
            TokenKind::QuestionParen,
            TokenKind::Question,
            TokenKind::Question,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_assignment_operators() {
    let result = lex("= ←");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![TokenKind::Eq, TokenKind::Assign, TokenKind::Eof,]
    );
}

#[test]
fn rejects_old_ascii_compound_operators() {
    // Explicit negative tests proving the clean break.
    // Old ASCII compounds no longer produce dedicated compound TokenKinds.
    let cases: &[(&str, TokenKind)] = &[
        ("==", TokenKind::EqEq),
        ("!=", TokenKind::BangEq),
        ("<=", TokenKind::LtEq),
        (">=", TokenKind::GtEq),
        ("->", TokenKind::Arrow),
    ];
    for &(op, ref compound) in cases {
        let src = format!("fixum _ x = 1 {op} 2");
        let result = lex(&src);
        let emits_compound = result
            .tokens
            .iter()
            .any(|t| std::mem::discriminant(&t.kind) == std::mem::discriminant(compound));
        assert!(
            !emits_compound,
            "legacy operator {op} must not produce {compound:?} token after clean break",
        );
    }

    // ASCII += and friends lex as separate tokens, never as postfix inc/dec glyphs.
    for op in ["+=", "-=", "*=", "/="] {
        let result = lex(&format!("varia numerus x ← 0 x {op} 1"));
        assert!(
            !result
                .tokens
                .iter()
                .any(|t| matches!(t.kind, TokenKind::PostInc | TokenKind::PostDec)),
            "legacy operator {op} must not produce postfix inc/dec tokens",
        );
    }

    // Triple forms also gone (no EqEqEq etc from source).
    let triples = lex("1 === 2");
    assert!(
        !triples
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::EqEqEq | TokenKind::BangEqEq))
    );
}

#[test]
fn lexes_hexadecimal_number() {
    let result = lex("0xCAFE_F00D");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![TokenKind::Integer(3_405_705_229), TokenKind::Eof]
    );
}

#[test]
fn lexes_binary_number() {
    let result = lex("0b1010_1111");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(kinds, vec![TokenKind::Integer(175), TokenKind::Eof]);
}

#[test]
fn lexes_octal_number() {
    let result = lex("0o755");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(kinds, vec![TokenKind::Integer(493), TokenKind::Eof]);
}

// ---------------------------------------------------------------------------
// F-057: Float underflow must emit diagnostic
// ---------------------------------------------------------------------------

#[test]
fn rejects_float_underflow() {
    // 1e-400 underflows to 0.0 — must produce an error, not silent Float(0.0)
    let result = lex("1e-400");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::NumberInvalid && has_arg(err, "issue", "float_underflow")
    }));
    assert!(
        result.tokens.iter().any(|t| t.kind == TokenKind::Error),
        "expected Error token for float underflow"
    );
}

#[test]
fn zero_float_underflow_does_not_false_positive() {
    // True zero must not trigger underflow diagnostic
    let result = lex("0e-400");
    assert!(
        result.success(),
        "true zero should not underflow: {:?}",
        result.errors
    );
}

#[test]
fn zero_dot_zero_is_not_underflow() {
    let result = lex("0.0");
    assert!(
        result.success(),
        "0.0 is a valid float: {:?}",
        result.errors
    );
}

// ---------------------------------------------------------------------------
// F-002: Oversized float must not become Inf silently
// ---------------------------------------------------------------------------

#[test]
fn rejects_float_overflow_to_infinity() {
    // Magnitude beyond f64::MAX parses as Inf — must diagnostic + Error token.
    let result = lex("1e400");
    assert!(
        !result.success(),
        "1e400 must not lex as a successful float"
    );
    assert!(
        result.errors.iter().any(|err| {
            err.kind == LexErrorKind::NumberInvalid && has_arg(err, "issue", "float_overflow")
        }),
        "expected float_overflow diagnostic, got {:?}",
        result.errors
    );
    assert!(
        result.tokens.iter().any(|t| t.kind == TokenKind::Error),
        "expected Error recovery token for float overflow"
    );
    assert!(
        !result
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::Float(n) if n.is_infinite())),
        "must not emit Float(+Inf) for overflow literal"
    );
}

// ---------------------------------------------------------------------------
// F-001: invalid number paths must emit Error tokens (not diagnostic-only)
// ---------------------------------------------------------------------------

#[test]
fn rejects_invalid_hexadecimal_literal() {
    let result = lex("0x");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::NumberInvalid
            && has_arg(err, "issue", "invalid_hexadecimal_number")
    }));
    assert!(
        result.tokens.iter().any(|t| t.kind == TokenKind::Error),
        "F-001: empty 0x must emit Error token for parser recovery"
    );
}

#[test]
fn rejects_invalid_binary_literal() {
    let result = lex("0b");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::NumberInvalid && has_arg(err, "issue", "invalid_binary_number")
    }));
}

#[test]
fn rejects_invalid_octal_literal() {
    let result = lex("0o");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::NumberInvalid && has_arg(err, "issue", "invalid_octal_number")
    }));
}

// ---------------------------------------------------------------------------
// F-056: Invalid trailing digit after radix prefix must not split silently
// ---------------------------------------------------------------------------

#[test]
fn rejects_invalid_trailing_digit_in_octal() {
    // 0o18 must not produce Integer(1) + Integer(8)
    let result = lex("0o18");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::NumberInvalid && has_arg(err, "issue", "invalid_octal_number")
    }));
    // The whole span from 0 to after the bad digit is consumed as Error.
    assert!(
        result.tokens.iter().any(|t| t.kind == TokenKind::Error),
        "expected Error token for invalid octal"
    );
    // No Integer(8) as a separate token.
    let ints: Vec<&u64> = result
        .tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Integer(n) => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(ints.len(), 0, "no integer tokens should leak from 0o18");
}

#[test]
fn rejects_invalid_trailing_digit_in_binary() {
    // 0b12 must not produce Integer(1) + Integer(2)
    let result = lex("0b12");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::NumberInvalid && has_arg(err, "issue", "invalid_binary_number")
    }));
    let ints: Vec<&u64> = result
        .tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Integer(n) => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(ints.len(), 0);
}

#[test]
fn rejects_invalid_trailing_digit_in_hexadecimal() {
    // 0x1G must not produce Integer(1) + G-as-something
    let result = lex("0x1G");
    assert!(!result.success());
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::NumberInvalid
            && has_arg(err, "issue", "invalid_hexadecimal_number")
    }));
}

#[test]
fn unterminated_string_at_eof_preserves_payload() {
    let result = lex("\"salve");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString)
    );

    assert_eq!(first_string_payload(&result), "salve");
}

#[test]
fn unterminated_string_at_newline_preserves_payload_without_newline() {
    let result = lex("\"salve\nsit _ x ← 1");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString)
    );

    assert_eq!(first_string_payload(&result), "salve");
}

fn first_string_payload(result: &LexResult) -> &str {
    let TokenKind::String(sym) = result.tokens[0].kind else {
        panic!(
            "expected recovered string token, got {:?}",
            result.tokens[0].kind
        );
    };
    result.interner.resolve(sym)
}

fn first_ascii_string_payload(result: &LexResult) -> &str {
    let TokenKind::AsciiString(sym) = result.tokens[0].kind else {
        panic!(
            "expected recovered AsciiString token, got {:?}",
            result.tokens[0].kind
        );
    };
    result.interner.resolve(sym)
}

// =============================================================================
// PRP-014 — Escaped newlines in single-line literals
// =============================================================================

#[test]
fn escaped_newline_in_string_literal_rejected() {
    let result = lex("\"salve\\\nsit _ x ← 1");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString
                && has_arg(err, "issue", "newline_in_string_literal"))
    );
    // Payload should include the `\` before the newline
    assert_eq!(first_string_payload(&result), "salve\\");
}

#[test]
fn escaped_newline_in_ascii_literal_rejected() {
    let result = lex("'salve\\\nsit _ x ← 1");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString
                && has_arg(err, "issue", "newline_in_ascii_literal"))
    );
    // Payload should include the `\` before the newline
    assert_eq!(first_ascii_string_payload(&result), "salve\\");
}

// =============================================================================
// PRP-078 — Bare carriage return in single-line literals
// =============================================================================

#[test]
fn bare_cr_in_string_literal_rejected() {
    let result = lex("\"salve\rfixum x ← 1");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString
                && has_arg(err, "issue", "newline_in_string_literal"))
    );
    assert_eq!(first_string_payload(&result), "salve");
}

#[test]
fn bare_cr_in_ascii_literal_rejected() {
    let result = lex("'salve\rfixum x ← 1");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString
                && has_arg(err, "issue", "newline_in_ascii_literal"))
    );
    assert_eq!(first_ascii_string_payload(&result), "salve");
}

// =============================================================================
// CDA-U2 — ↤ (U+21A4) conversion-directed assignment: red lexer tests
// =============================================================================
// These tests encode the U+21A4 lexer contract before the token exists (U3).
// The positive assertions FAIL today with the expected missing-feature reason:
// the glyph is unknown and lexes as `unexpected_character`. The confusion
// tests pin scalar distinctness against the neighbor glyphs ← ↦ ⇥ ⇇ ⇜ and must
// stay green (U+21C7 is copy-into; U+21DC is sink-into).

#[test]
fn lexes_conversio_assign_glyph_as_dedicated_token() {
    // RED (CDA-U2): `↤` must lex as its own token kind. Today the glyph is an
    // unknown character → UnexpectedCharacter, so this test fails for the
    // expected missing-feature reason. U3 adds the token and this turns green.
    let result = lex("x ↤ y");
    assert!(
        result.errors.is_empty(),
        "U+21A4 must lex as a dedicated token; errors: {:?}",
        result.errors
    );
    assert!(
        !result.tokens.iter().any(|t| t.kind == TokenKind::Error),
        "U+21A4 must not produce an Error recovery token"
    );
}

#[test]
fn conversio_assign_glyph_does_not_scan_as_existing_arrow() {
    // RED (CDA-U2): the raw ↤ scalar must scan as its own token kind, never
    // as `←` / `↦` / `⇥` / `→`. Today the glyph is unknown, so the failure is
    // the expected unexpected_character (the glyph is not aliased).
    let result = lex("\u{21A4}");
    assert!(
        result.errors.is_empty(),
        "U+21A4 must not be rejected or aliased; errors: {:?}",
        result.errors
    );
    for token in result.tokens.iter().filter(|t| t.kind != TokenKind::Eof) {
        assert!(
            !matches!(
                token.kind,
                TokenKind::Assign | TokenKind::Conversio | TokenKind::ExitArrow | TokenKind::Arrow
            ),
            "U+21A4 must not alias an existing arrow token, got {:?}",
            token.kind
        );
    }
}

#[test]
fn leftward_arrow_never_scans_as_conversio_assign() {
    // Confusion (protection): `←` (U+2190) remains the ordinary Assign token;
    // it is never the future ↤ token.
    let result = lex("←");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.tokens[0].kind, TokenKind::Assign);
}

#[test]
fn conversio_arrow_never_scans_as_conversio_assign() {
    // Confusion (protection): `↦` (U+21A6) remains the Conversio token.
    let result = lex("↦");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.tokens[0].kind, TokenKind::Conversio);
}

#[test]
fn exit_arrow_never_scans_as_conversio_assign() {
    // Confusion (protection): `⇥` (U+21E5) remains the ExitArrow token.
    let result = lex("⇥");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.tokens[0].kind, TokenKind::ExitArrow);
}

#[test]
fn leftwards_pair_arrow_never_scans_as_conversio_assign() {
    // Confusion (EOT-3L pin): `⇇` (U+21C7) is the dedicated
    // copy-into token. It must never be aliased into the ↤ token
    // (or any other arrow); it lexes as its own `Transfer` token.
    let result = lex("\u{21C7}");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(
        result.tokens[0].kind,
        TokenKind::Transfer,
        "U+21C7 must lex as the dedicated Transfer token"
    );
    for token in result.tokens.iter().filter(|t| t.kind != TokenKind::Eof) {
        assert!(
            !matches!(
                token.kind,
                TokenKind::Assign | TokenKind::Conversio | TokenKind::ExitArrow
            ),
            "U+21C7 must not alias an arrow token, got {:?}",
            token.kind
        );
    }
}

// =============================================================================
// CDA-U3 — ↤ (U+21A4): dedicated token kind (U2 red tests turn green)
// =============================================================================

#[test]
fn conversio_assign_lexes_as_dedicated_token_kind() {
    // U3: the U+21A4 scalar scans as its own `ConversioAssign` token kind —
    // exact scalar mapping, no aliases, no normalization into ←/↦/⇥/→.
    let result = lex("x ↤ y");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    assert_eq!(result.tokens[1].kind, TokenKind::ConversioAssign);
    assert!(matches!(result.tokens[2].kind, TokenKind::Ident(_)));
    assert_eq!(result.tokens[3].kind, TokenKind::Eof);
    // 3.1c — span: x at 0..1, ↤ at 2..5 (three UTF-8 bytes)
    assert_eq!(result.tokens[1].span, crate::Span::new(2, 5));
}

#[test]
fn conversio_assign_does_not_normalize_neighbor_glyphs() {
    // The confusion cluster above pins scalar distinctness; this test proves
    // the dedicated token does not absorb or normalize any neighbor glyph.
    for (src, expected) in [
        ("←", TokenKind::Assign),
        ("↦", TokenKind::Conversio),
        ("⇥", TokenKind::ExitArrow),
        ("→", TokenKind::Arrow),
    ] {
        let result = lex(src);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(
            result.tokens[0].kind, expected,
            "{src} must stay {expected:?}"
        );
    }
}

#[test]
fn escaped_cr_in_string_literal_rejected() {
    let result = lex("\"salve\\\rfixum x ← 1");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString
                && has_arg(err, "issue", "newline_in_string_literal"))
    );
    // Payload should include the `\` before the CR
    assert_eq!(first_string_payload(&result), "salve\\");
}

#[test]
fn escaped_cr_in_ascii_literal_rejected() {
    let result = lex("'salve\\\rfixum x ← 1");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString
                && has_arg(err, "issue", "newline_in_ascii_literal"))
    );
    // Payload should include the `\` before the CR
    assert_eq!(first_ascii_string_payload(&result), "salve\\");
}

#[test]
fn normal_escaped_chars_in_string_literal_still_work() {
    let result = lex(r#""salve\"mundus""#);
    assert!(
        result.errors.is_empty(),
        "normal escaped chars should not produce errors"
    );
    // The `\"` is consumed as an escape; the payload is the text between the
    // outer quotes with the escape consumed (the `\"` is not a terminator).
    assert_eq!(first_string_payload(&result), r#"salve\"mundus"#);
}

#[test]
fn normal_escaped_chars_in_ascii_literal_still_work() {
    let result = lex(r"'salve\'mundus'");
    assert!(
        result.errors.is_empty(),
        "normal escaped chars should not produce errors"
    );
    assert_eq!(first_ascii_string_payload(&result), r"salve\'mundus");
}

#[test]
fn lexes_unicode_range_operators() {
    let result = lex("0‥5 0…5 0 ante 5 0 usque 5");
    assert!(result.errors.is_empty());

    // 3.1c — span: first 0 at 0..1, ‥ at 1..4, second 5 at 4..5
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 1));
    assert_eq!(result.tokens[1].span, crate::Span::new(1, 4));
    assert_eq!(result.tokens[2].span, crate::Span::new(4, 5));

    let kinds: Vec<&str> = result
        .tokens
        .iter()
        .map(|token| match &token.kind {
            TokenKind::Integer(_) => "Int",
            TokenKind::DotDot => "DotDot",
            TokenKind::Ellipsis => "Ellipsis",
            // `ante`/`usque` are tokenless since Batch G.
            TokenKind::Ident(_) => "Ident",
            TokenKind::Eof => "Eof",
            other => panic!("unexpected token {other:?}"),
        })
        .collect();
    let expected = vec![
        "Int", "DotDot", "Int", "Int", "Ellipsis", "Int", "Int", "Ident", "Int", "Int", "Ident",
        "Int", "Eof",
    ];

    assert_eq!(kinds, expected);
}

#[test]
fn lexes_conversio_glyph() {
    let result = lex("x ↦ numerus");
    assert!(result.errors.is_empty());
    assert!(
        matches!(result.tokens[1].kind, TokenKind::Conversio),
        "'↦' should lex as TokenKind::Conversio"
    );
    // 3.1c — span: x ident at 0..1, ↦ at 2..5
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 1));
    assert_eq!(result.tokens[1].span, crate::Span::new(2, 5));
}

#[test]
fn lexes_shift_arrow_glyphs() {
    let result = lex("1 ⇐ 4 16 ⇒ 2");
    assert!(result.errors.is_empty());
    assert!(matches!(result.tokens[1].kind, TokenKind::BitwiseShl));
    assert!(matches!(result.tokens[4].kind, TokenKind::BitwiseShr));
    assert!(
        !result
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::Conversio)),
        "'⇒' must lex as BitwiseShr, not Conversio"
    );
    // 3.1c — span: first integer at 0..1, ⇐ at 2..5, ⇒ at 11..14
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 1));
    assert_eq!(result.tokens[1].span, crate::Span::new(2, 5));
    assert_eq!(result.tokens[4].span, crate::Span::new(11, 14));
}

#[test]
fn rejects_shift_glyphs() {
    for src in ["1 ≪ 4", "16 ≫ 2"] {
        let result = lex(src);
        assert!(
            result
                .errors
                .iter()
                .any(|err| err.kind == LexErrorKind::CharacterUnexpected),
            "shift glyph in {src:?} should be a scan error"
        );
    }
}

#[test]
fn rejects_verte_arrow_glyph() {
    let result = lex("x ⇢ textus");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::CharacterUnexpected),
        "the ⇢ glyph must not lex as a type-ascription operator"
    );
}

#[test]
fn verte_alias_spellings_lex_as_identifiers() {
    // Only the ∷ glyph produces TokenKind::Verte.
    for word in &["qua", "innatum", "novum"] {
        let result = lex(&format!("x {word} textus"));
        assert!(result.errors.is_empty(), "failed for word: {word}");
        assert!(
            matches!(&result.tokens[1].kind, TokenKind::Ident(_)),
            "'{word}' should lex as Ident",
        );
    }
}

// ---------------------------------------------------------------------------
// Proba modifier keyword groups (split from one mega-test for speed)
// ---------------------------------------------------------------------------

#[test]
fn lexes_proba_header_keywords() {
    let result = lex(r#"proba "case" omitte "blocked""#);
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert!(matches!(kinds[0], TokenKind::Ident(_)));
    assert!(matches!(kinds[1], TokenKind::String(_)));
    assert!(matches!(kinds[2], TokenKind::Ident(_)));
    assert!(matches!(kinds[3], TokenKind::String(_)));
    assert!(matches!(kinds[4], TokenKind::Eof));
}

#[test]
fn lexes_proba_scheduling_keywords() {
    let result = lex(r#"futurum "later" solum tag "focus" temporis 5"#);
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert!(matches!(kinds[0], TokenKind::Ident(_)));
    assert!(matches!(kinds[1], TokenKind::String(_)));
    assert!(matches!(kinds[2], TokenKind::Ident(_)));
    assert!(matches!(kinds[3], TokenKind::Ident(_)));
    assert!(matches!(kinds[4], TokenKind::String(_)));
    assert!(matches!(kinds[5], TokenKind::Ident(_)));
    assert!(matches!(kinds[6], TokenKind::Integer(5)));
    assert!(matches!(kinds[7], TokenKind::Eof));
}

#[test]
fn lexes_proba_metrics_keywords() {
    let result = lex("metior repete 2 fragilis 1");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert!(matches!(kinds[0], TokenKind::Ident(_)));
    assert!(matches!(kinds[1], TokenKind::Ident(_)));
    assert!(matches!(kinds[2], TokenKind::Integer(2)));
    assert!(matches!(kinds[3], TokenKind::Ident(_)));
    assert!(matches!(kinds[4], TokenKind::Integer(1)));
    assert!(matches!(kinds[5], TokenKind::Eof));
}

#[test]
fn lexes_requirit_statement_keyword() {
    // `requirit` is a statement keyword (error-handling identity), not the
    // retired proba modifier. It lexes as an identifier whose latin key the
    // parser claims as `TokenKind::Requirit`.
    let result = lex("requirit verum secus \"bad\"");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert!(matches!(kinds[0], TokenKind::Ident(_)));
    assert!(matches!(kinds[1], TokenKind::Ident(_)));
    assert!(matches!(kinds[2], TokenKind::Ident(_)));
    assert!(matches!(kinds[3], TokenKind::String(_)));
    assert!(matches!(kinds[4], TokenKind::Eof));
    assert_eq!(
        result.reader_keyword_latin_keys.get("requirit"),
        Some(&"requirit".to_owned()),
        "requirit must record its latin identity for parser claims"
    );
    let spec = lookup_keyword_spec("requirit").expect("requirit should be registered");
    assert_eq!(spec.token_kind, Some(TokenKind::Requirit));
    assert_eq!(
        spec.category,
        crate::KeywordKind::ErrorHandling,
        "requirit belongs to the error-handling category"
    );
}

#[test]
fn lexes_reice_statement_keyword() {
    // `reice` is the reject statement keyword (error-handling identity), the
    // boolean opposite of `requirit`. It lexes as an identifier whose latin
    // key the parser claims as `TokenKind::Reice`.
    let result = lex("reice verum iace \"bad\"");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert!(matches!(kinds[0], TokenKind::Ident(_)));
    assert!(matches!(kinds[1], TokenKind::Ident(_)));
    assert!(matches!(kinds[2], TokenKind::Ident(_)));
    assert!(matches!(kinds[3], TokenKind::String(_)));
    assert!(matches!(kinds[4], TokenKind::Eof));
    assert_eq!(
        result.reader_keyword_latin_keys.get("reice"),
        Some(&"reice".to_owned()),
        "reice must record its latin identity for parser claims"
    );
    let spec = lookup_keyword_spec("reice").expect("reice should be registered");
    assert_eq!(spec.token_kind, Some(TokenKind::Reice));
    assert_eq!(
        spec.category,
        crate::KeywordKind::ErrorHandling,
        "reice belongs to the error-handling category"
    );
}

#[test]
fn lexes_proba_block_braces() {
    let result = lex("{}");
    assert!(result.errors.is_empty());
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert!(matches!(kinds[0], TokenKind::LBrace));
    assert!(matches!(kinds[1], TokenKind::RBrace));
    assert!(matches!(kinds[2], TokenKind::Eof));
}

#[test]
fn guillemet_block_string_allows_embedded_quotes_and_newlines() {
    let result = lex("«he said \"salve\"»");
    assert!(result.errors.is_empty());
    assert_eq!(first_string_payload(&result), "he said \"salve\"");
    // 3.1c — span: «…» block string at 0..19
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 19));

    let multiline = lex("«line one\nstill line one»");
    assert!(multiline.errors.is_empty());
    assert_eq!(first_string_payload(&multiline), "line one\nstill line one");
}

#[test]
fn backtick_template_lexes_multiline_payload() {
    let result = lex("`select §\nfrom t`");
    assert!(result.success());
    let TokenKind::BacktickString(sym) = result.tokens[0].kind else {
        panic!("expected BacktickString, got {:?}", result.tokens[0].kind);
    };
    assert_eq!(result.interner.resolve(sym), "select §\nfrom t");
}

#[test]
fn octeti_literal_lexes_hex_payload_and_empty_form() {
    let spaced = lex("|48 65 6c 6c 6f|");
    assert!(spaced.errors.is_empty());
    let TokenKind::OctetiString(sym) = spaced.tokens[0].kind else {
        panic!("expected OctetiString, got {:?}", spaced.tokens[0].kind);
    };
    assert_eq!(spaced.interner.resolve(sym), "48 65 6c 6c 6f");

    let empty = lex("||");
    assert!(empty.errors.is_empty());
    let TokenKind::OctetiString(empty_sym) = empty.tokens[0].kind else {
        panic!("expected OctetiString, got {:?}", empty.tokens[0].kind);
    };
    assert_eq!(empty.interner.resolve(empty_sym), "");
}

#[test]
fn unterminated_octeti_literal_emits_diagnostic() {
    let result = lex("|deadbeef");
    assert!(
        result
            .errors
            .iter()
            .any(|err| has_arg(err, "issue", "unterminated_octeti_literal"))
    );
}

#[test]
fn ascii_literal_lexes_single_quoted_payload() {
    let result = lex("'pg:query'");
    assert!(result.errors.is_empty());
    let TokenKind::AsciiString(sym) = result.tokens[0].kind else {
        panic!(
            "expected ascii literal token, got {:?}",
            result.tokens[0].kind
        );
    };
    assert_eq!(result.interner.resolve(sym), "pg:query");
    // 3.1c — span: 'pg:query' at 0..10
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 10));
}

#[test]
fn empty_string_literal() {
    let result = lex(r#""""#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let TokenKind::String(sym) = result.tokens[0].kind else {
        panic!("expected String token, got {:?}", result.tokens[0].kind);
    };
    assert_eq!(result.interner.resolve(sym), "");
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 2));
}

#[test]
fn empty_ascii_literal() {
    let result = lex("''");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let TokenKind::AsciiString(sym) = result.tokens[0].kind else {
        panic!(
            "expected AsciiString token, got {:?}",
            result.tokens[0].kind
        );
    };
    assert_eq!(result.interner.resolve(sym), "");
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 2));
}

// =============================================================================
// 3.1d — Unterminated quote recovery (ascii, backtick, guillemet)
// =============================================================================

#[test]
fn unterminated_ascii_literal_at_eof_recovers() {
    let result = lex("'hello world");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString)
    );
    // Must produce at least an EOF token without panicking.
    assert!(!result.tokens.is_empty());
    assert_eq!(result.tokens.last().unwrap().kind, TokenKind::Eof);
    // The unterminated ascii payload should be recovered.
    let TokenKind::AsciiString(sym) = result.tokens[0].kind else {
        panic!(
            "expected recovered AsciiString token, got {:?}",
            result.tokens[0].kind
        );
    };
    assert_eq!(result.interner.resolve(sym), "hello world");
}

#[test]
fn unterminated_backtick_literal_at_eof_recovers() {
    let result = lex("`hello world");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString)
    );
    // Must produce at least an EOF token without panicking.
    assert!(!result.tokens.is_empty());
    assert_eq!(result.tokens.last().unwrap().kind, TokenKind::Eof);
    // The unterminated backtick payload should be recovered.
    let TokenKind::BacktickString(sym) = result.tokens[0].kind else {
        panic!(
            "expected recovered BacktickString token, got {:?}",
            result.tokens[0].kind
        );
    };
    assert_eq!(result.interner.resolve(sym), "hello world");
}

#[test]
fn unterminated_guillemet_literal_at_eof_recovers() {
    let result = lex("«hello world");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString)
    );
    // Must produce at least an EOF token without panicking.
    assert!(!result.tokens.is_empty());
    assert_eq!(result.tokens.last().unwrap().kind, TokenKind::Eof);
    // The unterminated guillemet payload should be recovered.
    let TokenKind::String(sym) = result.tokens[0].kind else {
        panic!(
            "expected recovered String token, got {:?}",
            result.tokens[0].kind
        );
    };
    assert_eq!(result.interner.resolve(sym), "hello world");
}

#[test]
fn block_string_opener_glyph_is_unexpected_character() {
    // Guillemets `«...»` are the only block-string spelling; `❝` fails as a
    // plain unexpected character.
    let result = lex("❝legacy❞");
    assert!(
        result
            .errors
            .iter()
            .any(|err| has_arg(err, "issue", "unexpected_character"))
    );
    assert!(matches!(result.tokens[0].kind, TokenKind::Error));
}

#[test]
fn triple_quote_is_not_a_special_lex_surface() {
    // `"""` fails as an ordinary string lex error (unterminated at EOF).
    let result = lex("\"\"\"");
    assert!(!result.success());
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::UnterminatedString)
    );
}

#[test]
fn newline_tokens_separate_comments_and_keywords() {
    let result = lex("# foo\n\n# bar\nincipit");
    assert!(result.errors.is_empty());

    let kinds: Vec<_> = result
        .tokens
        .iter()
        .take(6)
        .map(|token| match &token.kind {
            TokenKind::LineComment(_) => "Comment",
            TokenKind::Newline => "Newline",
            TokenKind::Ident(_) => "Incipit",
            TokenKind::Eof => "Eof",
            other => panic!("unexpected token {other:?}"),
        })
        .collect();

    assert_eq!(
        kinds,
        vec![
            "Comment", "Newline", "Newline", "Comment", "Newline", "Incipit"
        ]
    );
}

#[test]
fn crlf_emits_single_newline_token() {
    let result = lex("# note\r\nincipit");
    assert!(result.errors.is_empty());

    let kinds: Vec<_> = result
        .tokens
        .iter()
        .take(3)
        .map(|token| match &token.kind {
            TokenKind::LineComment(_) => "Comment",
            TokenKind::Newline => "Newline",
            TokenKind::Ident(_) => "Incipit",
            other => panic!("unexpected token {other:?}"),
        })
        .collect();

    assert_eq!(kinds, vec!["Comment", "Newline", "Incipit"]);
    assert_eq!(result.tokens[1].span.len(), 2);
}

// =============================================================================
// PRP-030 — Trailing trivia / recovery (lexer scope)
//
// Locks Newline/Error adjacency, line_has_code reset after error recovery, and
// post-PRP-014 residual: escaped-newline rejection must not swallow the
// physical line break or corrupt the next line's comment policy.
// =============================================================================

/// Compact kind labels for adjacency assertions (non-Eof sequence).
fn kind_labels(result: &LexResult) -> Vec<&'static str> {
    result
        .tokens
        .iter()
        .filter_map(|token| match &token.kind {
            TokenKind::Newline => Some("Newline"),
            TokenKind::Error => Some("Error"),
            TokenKind::LineComment(_) => Some("Comment"),
            TokenKind::String(_) => Some("String"),
            TokenKind::AsciiString(_) => Some("AsciiString"),
            TokenKind::Ident(_) => Some("Ident"),
            TokenKind::Integer(_) => Some("Integer"),
            TokenKind::Eof => None,
            _ => Some("Other"),
        })
        .collect()
}

#[test]
fn prp030_inline_hash_error_then_newline_adjacency() {
    // R1/R3: inline-# emits Error at `#`; physical LF still yields Newline.
    let result = lex("sit _ x ← 1 # bad\n");
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::InlineComment && has_arg(err, "issue", INLINE_COMMENT_ISSUE)
    }));

    let labels = kind_labels(&result);
    let error_idx = labels
        .iter()
        .position(|k| *k == "Error")
        .expect("Error token for inline #");
    assert_eq!(
        labels.get(error_idx + 1),
        Some(&"Newline"),
        "Newline must follow Error at physical line break; got {labels:?}"
    );

    let error_token = result
        .tokens
        .iter()
        .find(|t| t.kind == TokenKind::Error)
        .expect("Error token");
    // `#` starts after "fixum x ← 1 " — ASCII offset 13 (fixum=5, space, x, spaces, ← is 3 bytes...)
    // Span start must match the diagnostic and the `#` source position.
    let hash_offset = u32::try_from("sit _ x ← 1 ".len()).expect("test source offset fits u32");
    assert_eq!(error_token.span.start, hash_offset);
    let err = result
        .errors
        .iter()
        .find(|e| e.kind == LexErrorKind::InlineComment)
        .expect("InlineComment");
    assert_eq!(err.span.start, error_token.span.start);
}

#[test]
fn prp030_line_has_code_resets_after_inline_hash_error() {
    // R2: after Error + Newline, next line-start `#` is a real LineComment.
    let result = lex("x # bad\n# good\nfunctio\n");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::InlineComment)
    );

    let labels = kind_labels(&result);
    assert_eq!(
        labels,
        vec![
            "Ident", "Error", "Newline", "Comment", "Newline", "Ident", "Newline"
        ],
        "expected Ident/Error/Newline/Comment… adjacency; got {labels:?}"
    );
    assert_eq!(
        result
            .tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::LineComment(_)))
            .count(),
        1,
        "only the second-line # should be LineComment"
    );
}

#[test]
fn prp030_crlf_after_inline_hash_emits_single_newline() {
    let result = lex("x # bad\r\n# good\r\n");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::InlineComment)
    );

    let labels = kind_labels(&result);
    assert_eq!(
        labels,
        vec!["Ident", "Error", "Newline", "Comment", "Newline"],
        "CRLF must not drop or double Newline after Error; got {labels:?}"
    );
    // Newline after Error spans both CR and LF.
    let newline_after_error = result
        .tokens
        .iter()
        .enumerate()
        .find(|(i, t)| {
            *i > 0 && t.kind == TokenKind::Newline && result.tokens[i - 1].kind == TokenKind::Error
        })
        .map(|(_, t)| t)
        .expect("Newline after Error");
    assert_eq!(newline_after_error.span.len(), 2);
}

#[test]
fn prp030_escaped_newline_string_still_emits_newline_token() {
    // Post-PRP-014 residual: `\<LF>` rejects the string but must not swallow LF.
    let result = lex("\"salve\\\nmundus\n");
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::UnterminatedString
            && has_arg(err, "issue", "newline_in_string_literal")
    }));

    let labels = kind_labels(&result);
    assert!(
        labels.windows(2).any(|w| w == ["String", "Newline"]),
        "unterminated string must be followed by Newline; got {labels:?}"
    );
    // Remaining line content after the rejected string is still lexed.
    assert!(
        labels.contains(&"Ident"),
        "expected Ident(mundus) after recovered line break; got {labels:?}"
    );
}

#[test]
fn prp030_line_start_comment_after_escaped_newline_string() {
    // line_has_code must reset at the Newline that follows the recovered string
    // so the next line may open with `#`.
    let result = lex("\"salve\\\n# next\n");
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::UnterminatedString
            && has_arg(err, "issue", "newline_in_string_literal")
    }));
    assert!(
        !result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::InlineComment),
        "line-start # after recovered newline must not be InlineComment"
    );

    let labels = kind_labels(&result);
    assert!(
        labels
            .windows(3)
            .any(|w| w == ["String", "Newline", "Comment"]),
        "expected String/Newline/Comment sequence; got {labels:?}"
    );
}

#[test]
fn prp030_escaped_newline_ascii_then_line_start_comment() {
    let result = lex("'salve\\\n# next\n");
    assert!(result.errors.iter().any(|err| {
        err.kind == LexErrorKind::UnterminatedString
            && has_arg(err, "issue", "newline_in_ascii_literal")
    }));
    assert!(
        !result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::InlineComment)
    );
    let labels = kind_labels(&result);
    assert!(
        labels
            .windows(3)
            .any(|w| w == ["AsciiString", "Newline", "Comment"]),
        "expected AsciiString/Newline/Comment; got {labels:?}"
    );
}

#[test]
fn prp030_error_token_counts_as_line_code_for_same_line_hash() {
    // UnexpectedCharacter Error sets line_has_code; mid-line # still rejected.
    let result = lex("$ # not comment\n# real\n");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::CharacterUnexpected)
    );
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == LexErrorKind::InlineComment)
    );

    let labels = kind_labels(&result);
    // `$` → Error, then `# not comment` → Error, Newline, `# real` → Comment
    assert_eq!(
        labels,
        vec!["Error", "Error", "Newline", "Comment", "Newline"],
        "got {labels:?}"
    );
}

mod line_start_comment {
    use super::super::INLINE_COMMENT_ISSUE;
    use super::*;

    fn assert_rejects_inline_comment(source: &str) {
        let result = lex(source);
        assert!(
            !result.success(),
            "expected inline # rejection for: {source:?}"
        );
        assert!(result.errors.iter().any(|err| {
            err.kind == LexErrorKind::InlineComment && has_arg(err, "issue", INLINE_COMMENT_ISSUE)
        }));
    }

    fn assert_rejects_inline_comment_at(source: &str, expected_offset: u32) {
        let result = lex(source);
        assert!(!result.success());
        let err = result
            .errors
            .iter()
            .find(|err| err.kind == LexErrorKind::InlineComment)
            .expect("inline comment diagnostic");
        assert!(has_arg(err, "issue", INLINE_COMMENT_ISSUE));
        assert_eq!(err.span.start, expected_offset);
        let error_token = result
            .tokens
            .iter()
            .find(|token| token.kind == TokenKind::Error)
            .expect("Error token for inline #");
        assert_eq!(err.span.start, error_token.span.start);
    }

    #[test]
    fn rejects_inline_hash_after_code() {
        assert_rejects_inline_comment_at("sit numerus x ← 1  # inline\n", 21);
    }

    #[test]
    fn rejects_inline_hash_in_block_not_at_brace() {
        assert_rejects_inline_comment("functio f() → vacuum { sit numerus x ← 1  # inline }\n");
    }

    #[test]
    fn allows_hash_at_column_zero() {
        let result = lex("# lead\nnota 0\n");
        assert!(result.success(), "{:?}", result.errors);
        assert!(
            result
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::LineComment(_)))
        );
    }

    #[test]
    fn allows_indented_line_start_comment() {
        let result = lex("  # indented comment\nnota 0\n");
        assert!(result.success(), "{:?}", result.errors);
    }

    #[test]
    fn hash_inside_string_literal_is_not_a_comment() {
        let result = lex(r#""hello # world""#);
        assert!(result.success(), "{:?}", result.errors);
        assert!(
            !result
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::LineComment(_)))
        );
    }

    #[test]
    fn multiple_line_start_comments_on_consecutive_lines() {
        let result = lex("# one\n# two\nnota 0\n");
        assert!(result.success(), "{:?}", result.errors);
        let comments = result
            .tokens
            .iter()
            .filter(|token| matches!(token.kind, TokenKind::LineComment(_)))
            .count();
        assert_eq!(comments, 2);
    }

    #[test]
    fn hash_inside_ascii_literal_is_not_a_comment() {
        let result = lex("'hello # world'");
        assert!(result.success(), "{:?}", result.errors);
        assert!(
            !result
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::LineComment(_)))
        );
    }

    #[test]
    fn hash_inside_backtick_forma_literal_is_not_a_comment() {
        let result = lex("`hello # world`");
        assert!(result.success(), "{:?}", result.errors);
        assert!(
            !result
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::LineComment(_)))
        );
    }

    #[test]
    fn hash_inside_guillemet_literal_is_not_a_comment() {
        let result = lex("«hello # world»");
        assert!(result.success(), "{:?}", result.errors);
        assert!(
            !result
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::LineComment(_)))
        );
    }

    #[test]
    fn hash_inside_octeti_literal_is_not_a_comment() {
        let result = lex("|68#23|");
        assert!(result.success(), "{:?}", result.errors);
        assert!(
            !result
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::LineComment(_)))
        );
    }

    #[test]
    fn rejects_inline_hash_after_string_literal() {
        assert_rejects_inline_comment(r#""done" # trailing note"#);
    }

    #[test]
    fn rejects_inline_hash_after_annotation_token() {
        assert_rejects_inline_comment("@ publica # note on same line\nfunctio f() → vacuum { }\n");
    }

    #[test]
    fn rejects_inline_hash_after_opening_brace() {
        assert_rejects_inline_comment("incipit { # inner comment on same line\n  tacet\n}\n");
    }
}

// =============================================================================
// U+21DC — exact sink-into operator token
// =============================================================================

#[test]
fn sink_into_token_scan() {
    let result = lex("output ⇜ \"salve\"");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.tokens.len(), 4);
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    assert_eq!(result.tokens[1].kind, TokenKind::SinkInto);
    assert!(matches!(result.tokens[2].kind, TokenKind::String(_)));
    assert_eq!(result.tokens[3].kind, TokenKind::Eof);
    assert_eq!(result.tokens[0].span, crate::Span::new(0, 6));
    assert_eq!(result.tokens[1].span, crate::Span::new(7, 10));
    assert_eq!(result.tokens[2].span, crate::Span::new(11, 18));
    assert_eq!(result.tokens[3].span, crate::Span::new(18, 18));
    assert_eq!('\u{21DC}', '⇜');
}

// =============================================================================
// Prefix radicals — U+221A / U+221B / U+221C exact tokens
// =============================================================================

#[test]
fn radical_glyphs_scan_as_prefix_operators() {
    let cases = [
        ("√ x", TokenKind::RadicalSqrt),
        ("∛ y", TokenKind::RadicalCbrt),
        ("∜ z", TokenKind::RadicalFourth),
    ];
    for (source, glyph) in cases {
        let result = lex(source);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.tokens.len(), 3);
        assert_eq!(result.tokens[0].kind, glyph);
        assert!(matches!(result.tokens[1].kind, TokenKind::Ident(_)));
        assert_eq!(result.tokens[2].kind, TokenKind::Eof);
    }
    assert_eq!('\u{221A}', '√');
    assert_eq!('\u{221B}', '∛');
    assert_eq!('\u{221C}', '∜');
}

// =============================================================================
// EOT-3L — U+21C7 exact-output transfer token
// =============================================================================
// U+21C7 (`⇇`) is one exact operator token, never a keyword. The scanner maps
// only that scalar; neighbor arrows and ASCII aliases must not produce it.

#[test]
fn eot_transfer_glyph_lexes_as_a_dedicated_token() {
    let result = lex("output ⇇ \"salve\\n\"");
    assert!(
        result.errors.is_empty(),
        "EOT-3L: U+21C7 must lex without errors, got {:?}",
        result.errors
    );
    assert!(
        !result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Error),
        "EOT-3L: U+21C7 must not produce an Error token"
    );
    assert!(
        result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Transfer),
        "EOT-3L: U+21C7 must lex as the dedicated Transfer token, got {:?}",
        result.tokens
    );
}

#[test]
fn eot_transfer_glyph_scalar_is_exact_and_non_aliasing() {
    let result = lex("\u{21C7}");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![TokenKind::Transfer, TokenKind::Eof],
        "the bare glyph must lex as exactly Transfer + Eof"
    );
}

#[test]
fn eot_transfer_glyph_is_distinct_from_neighbor_arrows() {
    // Visual similarity is never permission to normalize a neighbor glyph.
    assert_eq!('\u{21C7}', '⇇');
    assert_ne!('\u{21C7}', '\u{2190}'); // ← ordinary assignment
    assert_ne!('\u{21C7}', '\u{21A6}'); // ↦ explicit conversion
    assert_ne!('\u{21C7}', '\u{21E5}'); // ⇥ inline recovery
    assert_ne!('\u{21C7}', '\u{2192}'); // → arrow

    // The neighbor arrows keep their own tokens.
    let result = lex("← ↦ ⇥ →");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Assign,    // ←
            TokenKind::Conversio, // ↦
            TokenKind::ExitArrow, // ⇥
            TokenKind::Arrow,     // →
            TokenKind::Eof,
        ]
    );
}

#[test]
fn eot_transfer_ascii_alias_does_not_produce_transfer_token() {
    // `<==` (or any ASCII alias) is not the transfer glyph and must never
    // lex as the Transfer token; each ASCII char keeps its own operator.
    let result = lex("<==");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let kinds: Vec<TokenKind> = result.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::LtAscii,
            TokenKind::Eq,
            TokenKind::Eq,
            TokenKind::Eof,
        ]
    );
    assert!(
        !result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Transfer),
        "ASCII <== must never produce the Transfer token"
    );
}

// ---------------------------------------------------------------------------
// ADB-U1 — ↢ (U+21A2) await-directed binding initializer
// ---------------------------------------------------------------------------

#[test]
fn await_assign_glyph_has_a_dedicated_token() {
    let result = lex("\u{21A2}");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(
        result
            .tokens
            .iter()
            .map(|token| token.kind.clone())
            .collect::<Vec<_>>(),
        vec![TokenKind::AwaitAssign, TokenKind::Eof]
    );
}

#[test]
fn former_await_assign_glyph_is_rejected() {
    let result = lex("\u{21E4}");
    assert!(!result.errors.is_empty());
    assert!(
        !result
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::AwaitAssign),
        "the retired glyph must not remain an await-assignment alias"
    );
}

#[test]
fn await_assign_glyph_is_distinct_from_neighbor_arrows() {
    let result = lex("↢ ← ↤ ⇥ ⇇ ⇜ → ↦");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(
        result
            .tokens
            .iter()
            .map(|token| token.kind.clone())
            .collect::<Vec<_>>(),
        vec![
            TokenKind::AwaitAssign,
            TokenKind::Assign,
            TokenKind::ConversioAssign,
            TokenKind::ExitArrow,
            TokenKind::Transfer,
            TokenKind::SinkInto,
            TokenKind::Arrow,
            TokenKind::Conversio,
            TokenKind::Eof,
        ]
    );
}

// ---------------------------------------------------------------------------
// decimal-widths D1: decimal literals preserve exact digit text (R0 fix)
// ---------------------------------------------------------------------------

#[test]
fn decimal_literal_carries_exact_digit_text() {
    let result = lex("4.2");
    assert!(result.errors.is_empty());
    match &result.tokens[0].kind {
        TokenKind::FloatText { value, text } => {
            assert_eq!(result.interner.resolve(*text), "4.2");
            assert!((*value - 4.2_f64).abs() < f64::EPSILON);
        }
        other => panic!("expected FloatText, got {other:?}"),
    }
}

#[test]
fn decimal_literal_with_exponent_carries_text() {
    let result = lex("1e-3");
    assert!(result.errors.is_empty());
    match &result.tokens[0].kind {
        TokenKind::FloatText { text, .. } => {
            assert_eq!(result.interner.resolve(*text), "1e-3");
        }
        other => panic!("expected FloatText, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Keyword identities are locale-sealed at the reader-key recording step.
// Pack spellings resolve to their Latin identity; spellings the surface
// does not declare record nothing and cannot claim a slot later.
// ---------------------------------------------------------------------------

/// English-style surface: declares only the `kernel` spelling for the
/// `nucleum` annotation identity.
struct EnModifierSurfacePack;

impl KeywordPack for EnModifierSurfacePack {
    fn pack_id(&self) -> &'static str {
        "en-modifier-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "kernel" => Some("nucleum"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

#[test]
fn en_surface_records_only_pack_declared_keyword_spellings() {
    let result = lex_with_locale_pack("kernel nucleum", &EnModifierSurfacePack);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    for token in result.tokens.iter().take(2) {
        assert!(
            matches!(token.kind, TokenKind::Ident(_)),
            "keyword spellings must lex as identifiers"
        );
    }
    assert_eq!(
        result.reader_keyword_latin_keys.get("kernel"),
        Some(&"nucleum".to_owned()),
        "en `kernel` must resolve to the nucleum identity"
    );
    assert!(
        !result.reader_keyword_latin_keys.contains_key("nucleum"),
        "undeclared Latin `nucleum` must not claim the identity on the en surface"
    );
}

#[test]
fn latin_surface_records_only_latin_keyword_identities() {
    // Registry-direct (`la`/no-pack) surface: `nucleum` is token-backed and
    // records its Latin key; the English spelling is an ordinary identifier
    // with no identity.
    let result = lex("kernel nucleum");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(
        result.reader_keyword_latin_keys.get("nucleum"),
        Some(&"nucleum".to_owned()),
        "la `nucleum` must record its Latin identity"
    );
    assert!(
        !result.reader_keyword_latin_keys.contains_key("kernel"),
        "`kernel` must not claim the nucleum identity on the la surface"
    );
}

#[test]
fn lexes_up_tack_default_glyph() {
    // D17.7: `⊥` (U+22A5) is the default channel; one glyph in every locale.
    let result = lex("⊥");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.tokens[0].kind, TokenKind::UpTack);
    assert!(result.reader_keyword_latin_keys.is_empty());
}

#[test]
fn perpendicular_lookalike_suggests_up_tack() {
    // D17.7: `⟂` (U+27C2 PERPENDICULAR) is rejected with "did you mean `⊥`?".
    assert_homoglyph_did_you_mean("⟂", "⟂", "⊥");
}

#[test]
fn lexes_check_and_ballot_conditional_glyphs() {
    // D4.5: `c ✓ a ✗ b` uses U+2713 CHECK MARK and U+2717 BALLOT X; one
    // spelling in every locale, no word twin.
    let result = lex("✓ ✗");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.tokens[0].kind, TokenKind::Check);
    assert_eq!(result.tokens[1].kind, TokenKind::Ballot);
    assert!(result.reader_keyword_latin_keys.is_empty());
}

#[test]
fn heavy_check_lookalike_suggests_check_mark() {
    // D4.7: `✔` (U+2714 HEAVY CHECK MARK) gets "did you mean `✓`?".
    assert_homoglyph_did_you_mean("✔", "✔", "✓");
}

#[test]
fn heavy_ballot_lookalike_suggests_ballot_x() {
    // D4.7: `✘` (U+2718 HEAVY BALLOT X) gets "did you mean `✗`?".
    assert_homoglyph_did_you_mean("✘", "✘", "✗");
}

#[test]
fn lexes_pilcrow_format_operator() {
    // D2.1: `value ¶ "spec"` uses U+00B6 PILCROW SIGN; one spelling in every
    // locale, no word twin.
    let result = lex("x ¶ \".2\"");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.tokens[1].kind, TokenKind::Pilcrow);
    assert!(result.reader_keyword_latin_keys.is_empty());
}

#[test]
fn lexes_element_of_membership_glyph() {
    // `x ∈ xs` uses U+2208; one spelling in every locale, no word twin.
    let result = lex("x ∈ xs");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.tokens[1].kind, TokenKind::ElementOf);
    assert!(result.reader_keyword_latin_keys.is_empty());
}

#[test]
fn lexes_not_element_of_membership_glyph() {
    // `x ∉ xs` uses U+2209; one spelling in every locale, no word twin.
    let result = lex("x ∉ xs");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.tokens[1].kind, TokenKind::NotElementOf);
    assert!(result.reader_keyword_latin_keys.is_empty());
}

#[test]
fn membership_glyphs_in_comment_and_string_are_not_scanned() {
    let result = lex("# a ∈ b ∉ c\n\"∈ ∉\"");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(
        result
            .tokens
            .iter()
            .all(|t| !matches!(t.kind, TokenKind::ElementOf | TokenKind::NotElementOf)),
        "{:?}",
        result.tokens
    );
}

// ---------------------------------------------------------------------------
// Inline regex literals (ruling d381ae56): `regex /pattern/` and `_ /pattern/`
// open a raw body; a bare `/` in operand position stays division.
// ---------------------------------------------------------------------------

#[test]
fn regex_literal_after_type_word_lexes_raw_body() {
    let result = lex("regex /a+/");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    let TokenKind::RegexString(sym) = result.tokens[1].kind else {
        panic!("expected RegexString, got {:?}", result.tokens[1].kind);
    };
    assert_eq!(result.interner.resolve(sym), "a+");
}

#[test]
fn regex_literal_after_underscore_lexes_raw_body() {
    let result = lex("_ /a+/");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(matches!(result.tokens[0].kind, TokenKind::Underscore(_)));
    let TokenKind::RegexString(sym) = result.tokens[1].kind else {
        panic!("expected RegexString, got {:?}", result.tokens[1].kind);
    };
    assert_eq!(result.interner.resolve(sym), "a+");
}

#[test]
fn regex_literal_does_not_claim_bare_division() {
    for source in ["x / 2", "n / regex_count"] {
        let result = lex(source);
        assert!(result.errors.is_empty(), "{source}: {:?}", result.errors);
        assert!(
            result.tokens.iter().any(|t| t.kind == TokenKind::Slash),
            "{source}: expected a Slash token"
        );
        assert!(
            !result
                .tokens
                .iter()
                .any(|t| matches!(t.kind, TokenKind::RegexString(_))),
            "{source}: no RegexString token expected"
        );
    }
}

#[test]
fn regex_literal_keeps_escaped_slash_in_payload() {
    let result = lex("regex /a\\/b/");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let TokenKind::RegexString(sym) = result.tokens[1].kind else {
        panic!("expected RegexString, got {:?}", result.tokens[1].kind);
    };
    assert_eq!(result.interner.resolve(sym), "a\\/b");
}

#[test]
fn regex_literal_unterminated_reports_issue() {
    let result = lex("regex /abc");
    assert!(
        result.errors.iter().any(|err| {
            err.kind == LexErrorKind::UnterminatedString
                && has_arg(err, "issue", "unterminated_regex_literal")
        }),
        "{:?}",
        result.errors
    );
    assert!(
        result
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::RegexString(_))),
        "an unterminated regex body still emits its token: {:?}",
        result.tokens
    );
}

#[test]
fn regex_literal_empty_reports_issue() {
    let result = lex("regex //");
    assert!(
        result.errors.iter().any(|err| {
            err.kind == LexErrorKind::UnterminatedString
                && has_arg(err, "issue", "empty_regex_literal")
        }),
        "{:?}",
        result.errors
    );
}

#[test]
fn regex_literal_member_identifier_stays_division() {
    let result = lex("self.regex / 2");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(
        result.tokens.iter().any(|t| t.kind == TokenKind::Slash),
        "{:?}",
        result.tokens
    );
    assert!(
        !result
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::RegexString(_))),
        "{:?}",
        result.tokens
    );
}

#[test]
fn regex_literal_class_slash_requires_escape() {
    let result = lex(r"regex /[\/]/");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let TokenKind::RegexString(sym) = result.tokens[1].kind else {
        panic!("expected RegexString, got {:?}", result.tokens[1].kind);
    };
    assert_eq!(result.interner.resolve(sym), r"[\/]");
}

#[test]
fn regex_literal_unescaped_class_slash_ends_body() {
    let result = lex(r"regex /[/]/");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let TokenKind::RegexString(sym) = result.tokens[1].kind else {
        panic!("expected RegexString, got {:?}", result.tokens[1].kind);
    };
    assert_eq!(result.interner.resolve(sym), "[");
    assert!(
        result.tokens.iter().any(|t| t.kind == TokenKind::RBracket),
        "{:?}",
        result.tokens
    );
    assert!(
        result.tokens.iter().any(|t| t.kind == TokenKind::Slash),
        "{:?}",
        result.tokens
    );
}

#[test]
fn regex_literal_parenthesized_binding_stays_division() {
    let result = lex("(regex) / 2");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(
        result.tokens.iter().any(|t| t.kind == TokenKind::Slash),
        "{:?}",
        result.tokens
    );
    assert!(
        !result
            .tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::RegexString(_))),
        "{:?}",
        result.tokens
    );
}

/// Minimal pack: the localized regex type word is `expressio`.
struct EnRegexTypePack;

impl KeywordPack for EnRegexTypePack {
    fn pack_id(&self) -> &'static str {
        "en-regex-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        (text == "expressio").then_some("regex")
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

#[test]
fn regex_literal_locale_pack_type_word_opens_body() {
    let result = lex_with_locale_pack("expressio /a+/", &EnRegexTypePack);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let TokenKind::RegexString(sym) = result.tokens[1].kind else {
        panic!("expected RegexString, got {:?}", result.tokens[1].kind);
    };
    assert_eq!(result.interner.resolve(sym), "a+");
}

// =============================================================================
// LEX003 — Unknown escapes in delimited literals
// =============================================================================

#[test]
fn unknown_escape_quote_emits_lex003() {
    let result = lex(r#""a\qb""#);
    assert_eq!(result.errors.len(), 1, "errors: {:?}", result.errors);
    assert_eq!(result.errors[0].kind, LexErrorKind::EscapeInvalid);
    assert!(has_arg(&result.errors[0], "issue", "unknown_escape"));
    assert!(matches!(result.tokens[0].kind, TokenKind::String(_)));
}

#[test]
fn unknown_escape_guillemet_emits_lex003() {
    let result = lex(r#"«a\"b»"#);
    assert_eq!(result.errors.len(), 1, "errors: {:?}", result.errors);
    assert_eq!(result.errors[0].kind, LexErrorKind::EscapeInvalid);
    assert!(has_arg(&result.errors[0], "issue", "unknown_escape"));
    assert!(matches!(result.tokens[0].kind, TokenKind::String(_)));
}

#[test]
fn unknown_escape_backtick_emits_lex003() {
    let result = lex(r#"`a\"b`"#);
    assert_eq!(result.errors.len(), 1, "errors: {:?}", result.errors);
    assert_eq!(result.errors[0].kind, LexErrorKind::EscapeInvalid);
    assert!(has_arg(&result.errors[0], "issue", "unknown_escape"));
    assert!(matches!(
        result.tokens[0].kind,
        TokenKind::BacktickString(_)
    ));
}

#[test]
fn unknown_escape_guillemet_own_delimiter_is_valid() {
    let result = lex(r#"«a\»b»"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(decode_textus_literal(first_string_payload(&result)), "a»b");
}

#[test]
fn unknown_escape_quote_own_delimiter_is_valid() {
    let result = lex(r#""a\"b""#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(decode_textus_literal(first_string_payload(&result)), "a\"b");
}
