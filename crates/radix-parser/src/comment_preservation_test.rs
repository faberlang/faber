//! Tests for comment and trivia preservation in parsed syntax trees.
//!
//! Contract: leading `#` comments and blank lines attach as `Trivia` on the following statement.

use crate::parse;
use radix_lexer::lex;
use radix_syntax::{CommentLine, StmtKind, Trivia};

fn parse_source(source: &str) -> crate::ParseResult {
    parse(lex(source))
}

fn comment_lines(trivia: &[Trivia]) -> Vec<&CommentLine> {
    trivia
        .iter()
        .filter_map(|item| match item {
            Trivia::CommentLine(comment) => Some(comment),
            Trivia::Newline(_) => None,
        })
        .collect()
}

fn newline_count(trivia: &[Trivia]) -> usize {
    trivia
        .iter()
        .filter(|item| matches!(item, Trivia::Newline(_)))
        .count()
}

#[test]
fn three_leading_comments_before_functio() {
    let source = "# one\n# two\n# three\nfunctio f() → vacuum { }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    assert_eq!(program.statements.len(), 1);

    let stmt = &program.statements[0];
    let comments = comment_lines(&stmt.leading_trivia);
    assert_eq!(comments.len(), 3);
    for (comment, expected) in comments.iter().zip([" one", " two", " three"]) {
        assert_eq!(result.interner.resolve(comment.text), expected);
    }
    assert_eq!(newline_count(&stmt.leading_trivia), 3);
}

#[test]
fn blank_line_between_decls_appears_as_newline_trivia() {
    let source = "functio a() → vacuum { }\n\nfunctio b() → vacuum { }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let second = &result.program.as_ref().unwrap().statements[1];
    assert_eq!(newline_count(&second.leading_trivia), 2);
    assert!(comment_lines(&second.leading_trivia).is_empty());
}

#[test]
fn comment_and_blank_line_before_second_decl() {
    let source = "functio a() → vacuum { }\n\n# trailing section\nfunctio b() → vacuum { }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let second = &result.program.as_ref().unwrap().statements[1];
    // blank line (2 newlines) + newline after the comment line
    assert_eq!(newline_count(&second.leading_trivia), 3);
    let comments = comment_lines(&second.leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(
        result.interner.resolve(comments[0].text),
        " trailing section"
    );
}

#[test]
fn comments_before_annotations_on_same_decl() {
    let source = "# lead\n@ publica\nfunctio f() → vacuum { }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let stmt = &result.program.as_ref().unwrap().statements[0];
    let comments = comment_lines(&stmt.leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " lead");
    assert_eq!(stmt.annotations.len(), 1);
}

#[test]
fn incipit_with_leading_comment() {
    let source = "# entry note\nincipit { }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let stmt = &result.program.as_ref().unwrap().statements[0];
    assert!(matches!(stmt.kind, StmtKind::Incipit(_)));
    let comments = comment_lines(&stmt.leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " entry note");
}

#[test]
fn inner_block_stmt_collects_leading_comment() {
    let source = "functio f() → vacuum {\n  # inner\n  tacet\n}\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let body = match &result.program.as_ref().unwrap().statements[0].kind {
        StmtKind::Func(func) => func.body.as_ref().expect("body"),
        other => panic!("expected Func, got {other:?}"),
    };
    assert_eq!(body.statements.len(), 1);
    let comments = comment_lines(&body.statements[0].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " inner");
}

// ── Edge cases ──

#[test]
fn first_decl_no_leading_comment_has_empty_trivia() {
    let source = "functio f() → vacuum { }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let stmt = &result.program.as_ref().unwrap().statements[0];
    assert!(stmt.leading_trivia.is_empty());
}

#[test]
fn three_blank_lines_between_decls_produces_four_newlines() {
    let source = "functio a() → vacuum { }\n\n\n\nfunctio b() → vacuum { }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let second = &result.program.as_ref().unwrap().statements[1];
    // 3 blank lines = 4 newlines (each blank line is "\n\n")
    assert_eq!(newline_count(&second.leading_trivia), 4);
}

#[test]
fn comment_after_last_decl_has_no_following_statement() {
    let source = "functio f() → vacuum { }\n# trailing\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let stmt = &result.program.as_ref().unwrap().statements[0];
    assert!(comment_lines(&stmt.leading_trivia).is_empty());
}

#[test]
fn comment_only_source_parses_with_empty_program() {
    let source = "# just a comment\n";
    let result = parse_source(source);
    // A file with only a comment should still parse (no errors).
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn comment_between_annotation_lines() {
    let source = "# top\n@ publica\n# between\n@ cli \"tool\"\nfunctio f() → vacuum { }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let stmt = &result.program.as_ref().unwrap().statements[0];
    assert_eq!(stmt.annotations.len(), 2);
    let comments = comment_lines(&stmt.leading_trivia);
    // Only the top comment attaches as leading trivia; the between comment
    // attaches to the following annotation.
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " top");
}
