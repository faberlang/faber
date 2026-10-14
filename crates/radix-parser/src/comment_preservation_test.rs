//! Tests for comment and trivia preservation in parsed syntax trees.
//!
//! Contract: leading `#` comments and blank lines attach as `Trivia` on the following statement;
//! comments immediately before a block's `}` or before EOF attach to the block/program end boundary.

use crate::ParseErrorKind;
use crate::test_support::parse_program_la;
use radix_syntax::{CommentLine, SecusClause, StmtKind, Trivia};

fn parse_source(source: &str) -> crate::ParseResult {
    parse_program_la(source)
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
    // The top comment attaches as leading trivia; the between-annotation
    // comment is at an unnamed position and must fail with the structured
    // invalid-placement diagnostic instead of being silently discarded.
    assert!(!result.success());
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
    assert_eq!(
        result.errors[0].kind,
        ParseErrorKind::CommentPlacementInvalid,
        "{:?}",
        result.errors
    );
    assert!(
        result.errors[0]
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "invalid_comment_placement"),
        "{:?}",
        result.errors[0].args
    );

    let stmt = &result.program.as_ref().unwrap().statements[0];
    assert_eq!(stmt.annotations.len(), 2);
    let comments = comment_lines(&stmt.leading_trivia);
    // Only the top comment attaches as leading trivia.
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " top");
}

// ── End boundaries ──

#[test]
fn block_end_comment_survives_on_block_boundary() {
    let source = "functio f() → vacuum {\n  tacet\n  # end note\n}\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let body = match &result.program.as_ref().unwrap().statements[0].kind {
        StmtKind::Func(func) => func.body.as_ref().expect("body"),
        other => panic!("expected Func, got {other:?}"),
    };
    let comments = comment_lines(&body.end_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " end note");
    // The block-end run is ordered trivia: statement-terminating newline,
    // comment line, then its trailing newline.
    assert_eq!(newline_count(&body.end_trivia), 2);
}

#[test]
fn comment_only_block_body_survives_on_block_boundary() {
    let source = "functio f() → vacuum {\n  # only a comment\n}\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let body = match &result.program.as_ref().unwrap().statements[0].kind {
        StmtKind::Func(func) => func.body.as_ref().expect("body"),
        other => panic!("expected Func, got {other:?}"),
    };
    assert!(body.statements.is_empty());
    let comments = comment_lines(&body.end_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " only a comment");
}

#[test]
fn comment_only_file_preserves_comment_on_program_end() {
    let source = "# just a comment\n";
    let result = parse_source(source);
    // A file with only a comment still parses with the comment retained on the
    // program end — no statement-expected error and no invalid placement.
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    assert!(program.statements.is_empty());
    let comments = comment_lines(&program.end_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " just a comment");
}

#[test]
fn comment_only_file_with_blank_lines_preserves_order() {
    let source = "# one\n\n# two\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let comments = comment_lines(&program.end_trivia);
    assert_eq!(comments.len(), 2);
    assert_eq!(result.interner.resolve(comments[0].text), " one");
    assert_eq!(result.interner.resolve(comments[1].text), " two");
    // blank line (2 newlines) between comments + trailing newline
    assert_eq!(newline_count(&program.end_trivia), 3);
}

#[test]
fn trailing_comment_before_eof_survives_on_program_end() {
    let source = "functio f() → vacuum { }\n# trailing\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    assert_eq!(program.statements.len(), 1);
    let comments = comment_lines(&program.end_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " trailing");
}

// ── Branch links ──

#[test]
fn comment_between_sin_branches_preserved_as_branch_link_trivia() {
    // A section-divider comment between braced `sin` branches of one si-chain
    // sits at the ordinary statement-level divider position and must be
    // claimed as branch-link leading trivia, not rejected as invalid placement.
    let source = "\
si verum {
  tacet
}
sin falsum {
  tacet
}
# -- identity commands --
sin verum {
  tacet
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Si(si) = &program.statements[0].kind else {
        panic!("expected si statement");
    };

    let SecusClause::Sin {
        stmt: falsum,
        leading_trivia,
    } = si.else_.as_ref().expect("first sin branch")
    else {
        panic!("expected sin clause");
    };
    assert!(comment_lines(leading_trivia).is_empty());

    let SecusClause::Sin {
        stmt: _,
        leading_trivia,
    } = falsum.else_.as_ref().expect("second sin branch")
    else {
        panic!("expected sin clause");
    };
    let comments = comment_lines(leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(
        result.interner.resolve(comments[0].text),
        " -- identity commands --"
    );
    // branch-terminating newline + newline after the comment line
    assert_eq!(newline_count(leading_trivia), 2);
}

#[test]
fn comment_before_secus_else_preserved_as_branch_link_trivia() {
    let source = "\
si verum {
  tacet
}
# -- fallback --
secus {
  tacet
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Si(si) = &program.statements[0].kind else {
        panic!("expected si statement");
    };

    let SecusClause::Block { leading_trivia, .. } = si.else_.as_ref().expect("secus branch") else {
        panic!("expected secus block");
    };
    let comments = comment_lines(leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " -- fallback --");
    assert_eq!(newline_count(leading_trivia), 2);
}

// ── Genus (class) block-end and initializer-entangled boundaries (MT-U2) ──

#[test]
fn genus_block_end_comment_attaches_to_class_end_trivia() {
    let source = "genus Canis {\n  varia textus nomen\n  # before close\n}\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus declaration");
    };
    let comments = comment_lines(&class.end_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " before close");
}

#[test]
fn comment_after_field_with_initializer_attaches_to_next_member() {
    let source = "genus Thing {\n  varia numerus count = 42\n  # doc\n  varia bivalens flag\n}\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus declaration");
    };
    assert_eq!(class.members.len(), 2);
    let comments = comment_lines(&class.members[1].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " doc");
    // initializer-terminating newline + comment line's terminating newline
    assert_eq!(newline_count(&class.members[1].leading_trivia), 2);
}

#[test]
fn reader_state_shaped_genus_fixture_parses_green() {
    // The driving fixture from the goal: fields documented in place, the last
    // one preceded by a full-line comment after a plain field.
    let source = "\
genus ReaderState {
    varia tabula<textus, textus> reader_keyword_latin_keys
    varia tabula<textus, textus> reader_type_latin_keys
    varia numerus current_line
    # True once any non-trivia token has been emitted on the current line.
    varia bivalens line_has_code
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus declaration");
    };
    assert_eq!(class.members.len(), 4);
    let comments = comment_lines(&class.members[3].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(
        result.interner.resolve(comments[0].text),
        " True once any non-trivia token has been emitted on the current line."
    );
}
