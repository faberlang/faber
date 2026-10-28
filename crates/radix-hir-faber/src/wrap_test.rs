//! Tests for [`crate::wrap_faber_source`]: opt-in wrapping at legal token
//! breaks only. Sources are written in the Latin surface (`lex` reads it
//! without a pack); the parser-level round trip over real fixtures lives in
//! `faber`'s `format_corpus_test`.

use crate::wrap_faber_source;
use radix_lexer::{TokenKind, lex};

/// Token texts with newline tokens dropped: wrapping may only add newlines.
fn token_texts(source: &str) -> Vec<String> {
    let lexed = lex(source);
    assert!(lexed.success(), "source must lex cleanly: {source}");
    lexed
        .tokens
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Newline | TokenKind::Eof))
        .map(|token| source[token.span.start as usize..token.span.end as usize].to_owned())
        .collect()
}

fn wrap(source: &str, width: usize) -> String {
    wrap_faber_source(source, width, None)
}

fn assert_same_tokens(before: &str, after: &str) {
    assert_eq!(
        token_texts(before),
        token_texts(after),
        "wrapping must not change any token\nbefore:\n{before}\nafter:\n{after}"
    );
}

const LONG_CALL: &str = "functio f() {\n    varia textus x ← g(alpha, beta, gamma, delta, epsilon, zeta, eta, theta)\n}\n";

#[test]
fn wrap_faber_source_breaks_after_commas_inside_brackets() {
    let wrapped = wrap(LONG_CALL, 40);
    assert_same_tokens(LONG_CALL, &wrapped);
    assert_ne!(wrapped, LONG_CALL, "a 40-column limit must wrap this line");
    for line in wrapped.lines() {
        assert!(
            line.chars().count() <= 40,
            "line over width though breakable: {line:?}"
        );
    }
    // Continuation lines are indented one level past the statement.
    assert!(
        wrapped
            .lines()
            .skip(2)
            .any(|line| line.starts_with("        ")),
        "{wrapped}"
    );
}

#[test]
fn wrap_faber_source_zero_width_and_wide_enough_change_nothing() {
    assert_eq!(wrap(LONG_CALL, 0), LONG_CALL);
    assert_eq!(wrap(LONG_CALL, 500), LONG_CALL);
}

#[test]
fn wrap_faber_source_is_idempotent() {
    let once = wrap(LONG_CALL, 40);
    assert_eq!(wrap(&once, 40), once);
}

#[test]
fn wrap_faber_source_keeps_unbreakable_text_whole_and_overflows() {
    let source = "functio f() {\n    varia textus s ← \"a very long string literal that cannot be split anywhere at all\"\n}\n";
    let wrapped = wrap(source, 20);
    assert_same_tokens(source, &wrapped);
    assert!(
        wrapped.lines().any(|line| line
            .contains("\"a very long string literal that cannot be split anywhere at all\"")),
        "the string literal stays on one line: {wrapped}"
    );
}

#[test]
fn wrap_faber_source_counts_display_columns_not_chars() {
    // Each CJK glyph is two columns: six of them fill twelve columns.
    let source = "functio f() {\n    g(日本語日本語, bb)\n}\n";
    let wrapped = wrap(source, 18);
    assert_same_tokens(source, &wrapped);
    assert!(wrapped.contains("日本語日本語,\n"), "{wrapped}");
    let narrow = wrap("functio f() {\n    g(aaaaaa, bb)\n}\n", 18);
    assert!(narrow.contains("g(aaaaaa, bb)"), "{narrow}");
}

#[test]
fn wrap_faber_source_leaves_comment_lines_in_place() {
    // Inline (trailing) comments do not lex, so a comment is always a line of
    // its own; the code around it wraps and the comment is copied verbatim.
    let source = "functio f() {\n    # a long remark that is wider than the limit\n    g(alpha, beta, gamma, delta)\n}\n";
    let wrapped = wrap(source, 20);
    assert_same_tokens(source, &wrapped);
    assert!(
        wrapped.contains("\n    # a long remark that is wider than the limit\n"),
        "{wrapped}"
    );
}

#[test]
fn wrap_faber_source_keeps_frontmatter_free_blank_lines() {
    let source = "functio f() {\n\n    g(alpha, beta, gamma, delta)\n\n}\n";
    let wrapped = wrap(source, 20);
    assert_same_tokens(source, &wrapped);
    assert!(wrapped.starts_with("functio f() {\n\n"), "{wrapped}");
    assert!(wrapped.ends_with("\n\n}\n"), "{wrapped}");
}

// The six exceptions of need, plus the parser's other same-line
// sites found while proving it. Each case is wrapped far below its length;
// every line-sensitive span must stay on one line.

#[test]
fn wrap_faber_source_keeps_line_comments_whole() {
    let source =
        "# one two three four five six seven eight nine ten eleven twelve\nfunctio f() {\n}\n";
    assert_eq!(wrap(source, 10), source);
}

#[test]
fn wrap_faber_source_keeps_annotation_sugar_on_one_line() {
    for annotation in [
        "@ nucleum fragment",
        // ALLOW_DIAGNOSTIC_RENDER_TEXT: Faber source input for the wrapper, not diagnostic text.
        "@ radix lane \"air\" contract \"c\" backward \"b\" extra",
        "@ ad 'some/path/to/a/target/file.ext' more tokens here ok",
        "@ nondum one two three four five six seven eight nine ten",
        "@ publica alpha beta gamma delta epsilon zeta eta theta iota",
    ] {
        let source = format!("{annotation}\nfunctio f() {{\n}}\n");
        let wrapped = wrap(&source, 8);
        assert_eq!(
            wrapped, source,
            "annotation sugar must not wrap: {annotation}"
        );
    }
}

#[test]
fn wrap_faber_source_keeps_schema_columns_on_their_own_lines() {
    let source = "schema Persona {\n    columna textus nomen : alias_longum_nomen\n    columna numerus aetas : alias_aetatis\n}\n";
    let wrapped = wrap(source, 12);
    assert_eq!(wrapped, source);
}

#[test]
fn wrap_faber_source_keeps_guard_word_on_the_value_line() {
    for keyword in ["iace", "mori"] {
        let source = format!(
            "functio f() {{\n    {keyword} \"a fairly long message value\" si alpha ≡ beta et gamma ≡ delta\n}}\n"
        );
        let wrapped = wrap(&source, 12);
        assert_same_tokens(&source, &wrapped);
        let guard = wrapped
            .lines()
            .find(|line| line.contains(keyword))
            .expect("the guarded statement line");
        assert!(
            guard.contains("\" si"),
            "`si` stays on the value's line for {keyword}: {wrapped}"
        );
    }
}

#[test]
fn wrap_faber_source_keeps_copy_with_changes_source_on_the_brace_line() {
    let source = "functio f() {\n    varia Punctum q ← Punctum { x: 1, y: 2, z: 3 } ex origo\n}\n";
    let wrapped = wrap(source, 16);
    assert_same_tokens(source, &wrapped);
    assert!(
        wrapped.lines().any(|line| line.contains("} ex")),
        "`ex` stays on the closing brace's line: {wrapped}"
    );
    assert!(
        !wrapped
            .lines()
            .any(|line| line.trim_start().starts_with("ex ")),
        "no line may begin with `ex`: {wrapped}"
    );
}

#[test]
fn wrap_faber_source_keeps_return_value_on_the_redde_line() {
    let source = "functio f() {\n    redde alpha + beta + gamma + delta + epsilon\n}\n";
    let wrapped = wrap(source, 14);
    assert_same_tokens(source, &wrapped);
    assert!(
        wrapped
            .lines()
            .any(|line| line.trim_start().starts_with("redde alpha")),
        "the value starts on the `redde` line: {wrapped}"
    );
}

#[test]
fn wrap_faber_source_keeps_statement_lists_on_one_line() {
    // A newline ends `scribe`'s argument list, so its commas never break;
    // commas inside a call within it still may.
    let source = "functio f() {\n    scribe alpha, beta, gamma, delta, epsilon\n}\n";
    assert_eq!(wrap(source, 12), source);
    let extract = "functio f() {\n    ex punctum fixum alpha, beta, gamma, delta\n}\n";
    let wrapped = wrap(extract, 12);
    assert_same_tokens(extract, &wrapped);
    assert!(
        wrapped.lines().all(|line| !line.trim_end().ends_with(',')),
        "no extraction field list may break at a comma: {wrapped}"
    );
}

#[test]
fn wrap_faber_source_keeps_the_conversion_hint_on_its_line() {
    let source = "functio f() {\n    varia numerus n ← valor_longus_nomine ↦ u32 via Hex\n}\n";
    let wrapped = wrap(source, 10);
    assert_same_tokens(source, &wrapped);
    assert!(
        wrapped.contains("↦ u32 via Hex") || wrapped.contains("via Hex"),
        "{wrapped}"
    );
    assert!(
        !wrapped
            .lines()
            .any(|line| line.trim_start().starts_with("via")),
        "{wrapped}"
    );
}

#[test]
fn wrap_faber_source_keeps_the_first_three_tokens_of_a_line_together() {
    let source = "functio f() {\n    varia mappa<textus, numerus> nomen ← novum\n}\n";
    let wrapped = wrap(source, 10);
    assert_same_tokens(source, &wrapped);
    assert!(
        wrapped
            .lines()
            .any(|line| line.trim_start().starts_with("varia mappa<")),
        "the binding word, type and its first bracket stay together: {wrapped}"
    );
}

#[test]
fn wrap_faber_source_keeps_a_regex_literal_beside_its_type_word() {
    let source = "functio f() {\n    varia regex pattern ← regex /ab+c/\n    varia regex other ← _ /x*y/\n}\n";
    let wrapped = wrap(source, 10);
    assert_same_tokens(source, &wrapped);
    assert!(wrapped.contains("regex /ab+c/"), "{wrapped}");
    assert!(wrapped.contains("_ /x*y/"), "{wrapped}");
}
