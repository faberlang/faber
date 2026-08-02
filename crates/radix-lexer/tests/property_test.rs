//! Property-based tests for the radix-lexer.
//!
//! Generates random-ish Faber source fragments and verifies the lexer never
//! panics and always produces a well-formed token stream.

use proptest::prelude::*;

// =============================================================================
// STRATEGIES
// =============================================================================

/// Strategy that generates a single "word"-like token: a lowercase ASCII
/// identifier that could be a keyword or an identifier.
fn word_strategy() -> impl Strategy<Value = String> {
    "[a-z]{1,12}".prop_map(Into::into)
}

/// Strategy that generates an integer literal (decimal).
fn integer_strategy() -> impl Strategy<Value = String> {
    "[0-9]{1,10}".prop_map(Into::into)
}

/// Strategy that generates a single-character operator or punctuation token.
fn operator_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("+".to_string()),
        Just("-".to_string()),
        Just("*".to_string()),
        Just("/".to_string()),
        Just("%".to_string()),
        Just("=".to_string()),
        Just("<".to_string()),
        Just(">".to_string()),
        Just("!".to_string()),
        Just("?".to_string()),
        Just(".".to_string()),
        Just(",".to_string()),
        Just(":".to_string()),
        Just(";".to_string()),
        Just("(".to_string()),
        Just(")".to_string()),
        Just("{".to_string()),
        Just("}".to_string()),
        Just("[".to_string()),
        Just("]".to_string()),
        Just("@".to_string()),
        Just("|".to_string()),
    ]
}

/// Strategy that generates a C++-style `//` comment (invalid in Faber, to
/// exercise error recovery).
///
/// Weighted low so the stream stays mostly valid-ish.
fn cpp_comment_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("//[^\n]{0,20}").unwrap()
}

/// Strategy that generates a single whitespace character (space or tab).
fn whitespace_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(" ".to_string()),
        Just("  ".to_string()),
        Just("\t".to_string()),
        Just(" \t".to_string()),
    ]
}

/// Strategy that generates a newline (possibly preceded by a carriage return).
fn newline_strategy() -> impl Strategy<Value = String> {
    prop_oneof![Just("\n".to_string()), Just("\r\n".to_string()),]
}

/// Strategy that generates a single Faber token-like fragment.
fn fragment_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => word_strategy(),
        2 => integer_strategy(),
        2 => operator_strategy(),
        2 => whitespace_strategy(),
        1 => newline_strategy(),
        1 => cpp_comment_strategy(),
    ]
}

/// Strategy that generates a full source fragment composed of many smaller
/// token-like pieces, sized from empty up to ~200 bytes.
fn source_fragment_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(fragment_strategy(), 0..40).prop_map(|fragments| fragments.concat())
}

// =============================================================================
// PROPERTY TESTS
// =============================================================================

proptest! {
    /// The lexer must never panic on any input, always produce a valid token
    /// stream, and satisfy basic structural invariants.
    #[test]
    fn lexer_produces_well_formed_token_stream(
        source in source_fragment_strategy(),
    ) {
        let result = radix_lexer::lex(&source);
        let tokens = &result.tokens;

        // === INVARIANT 1: every token span is within source bounds ===
        let source_len = u32::try_from(source.len()).expect("source longer than u32::MAX");
        for token in tokens {
            let span = token.span;
            prop_assert!(
                span.start <= span.end,
                "token {:?} has degenerate span start={} end={} in source of len {}",
                token.kind, span.start, span.end, source_len,
            );
            prop_assert!(
                span.end <= source_len,
                "token {:?} span end={} exceeds source length={}",
                token.kind, span.end, source_len,
            );
        }

        // === INVARIANT 2: tokens appear in source order ===
        for window in tokens.windows(2) {
            let prev = window[0].span;
            let next = window[1].span;
            prop_assert!(
                prev.start <= next.start,
                "token {:?} at start={} appears after token {:?} at start={}",
                window[0].kind, prev.start, window[1].kind, next.start,
            );
        }

        // === INVARIANT 3: the final token is always Eof ===
        if let Some(last) = tokens.last() {
            prop_assert_eq!(
                &last.kind,
                &radix_lexer::TokenKind::Eof,
                "last token must be Eof, got {:?} at {:?}",
                last.kind,
                last.span,
            );
            // Eof span end must point to the end of the source.
            prop_assert!(
                last.span.end >= source_len,
                "Eof span end={} should be >= source length={}",
                last.span.end, source_len,
            );
        }
    }
}

/// Deterministic fuzz-style test that exercises known edge cases explicitly.
#[test]
fn edge_case_inputs_do_not_panic() {
    let edge_cases: &[&str] = &[
        "",
        " ",
        "\t",
        "\n",
        "\r\n",
        "//",
        "/*",
        "/* hello",
        "#",
        "##",
        "# comment\n# comment",
        "0x",
        "0b",
        "0o",
        "\"",
        "'",
        "`",
        "\u{00AB}", // « alone
        "|",
        "0x_",
        "0b_",
        // Mixed whitespace
        "  \t  \n  \r\n  ",
        // Just operators
        "+ - * / % = < > ! ? . , : ; ( ) { } [ ] @",
        // Unicode identifiers
        "\u{53D8}\u{91CF}", // 变量
        "caf\u{00E9}",
        // Unterminated strings
        "\"hello",
        "'hello",
        "`hello",
        "\u{00AB}hello", // «hello
        // NFKC edge case (full-width letters)
        "\u{FF4E}\u{FF4F}\u{FF4D}\u{FF45}\u{FF4E}",
        // Lone surrogates / non-XID-start
        "\u{0301}abc",
        // Bidi controls
        "\u{202E}abc",
    ];

    for &source in edge_cases {
        let result = radix_lexer::lex(source);
        // Must not panic; must produce at least the Eof token.
        assert!(
            !result.tokens.is_empty(),
            "edge case {source:?} produced zero tokens"
        );
        assert_eq!(
            &result.tokens.last().unwrap().kind,
            &radix_lexer::TokenKind::Eof,
            "edge case {source:?} must end with Eof",
        );
    }
}
