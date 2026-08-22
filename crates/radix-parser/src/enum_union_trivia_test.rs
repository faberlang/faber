//! Tests for member-leading trivia on enum (`ordo`) and union (`discretio`)
//! declarations.
//!
//! Contract: a full-line `#` comment between member declarations attaches as
//! the following member's `leading_trivia`, in both the comma-separated member
//! layout and the per-line member layout. The claim walker claims those spans,
//! so no `PARSE060 invalid_comment_placement` is reported for these positions.
//!
//! Inline comments (a `#` after code on the same line, e.g. `Red, # doc`)
//! stay forbidden by the lexer (`LEX007`) — the fixtures below place every
//! comment at the start of its own line. Block-end comments (before the
//! closing `}`) remain invalid placement — out of scope for this unit.

use crate::{parse, ParseErrorKind};
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

// ── Comma-separated layout (done_when 1) ──

#[test]
fn union_comma_layout_comment_attaches_to_next_variant() {
    let source = "\
discretio Color {
    Red,
    # doc
    Green
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Union(decl) = &program.statements[0].kind else {
        panic!("expected union declaration");
    };
    assert_eq!(decl.variants.len(), 2);

    // The comment attaches to the following variant's leading trivia.
    let comments = comment_lines(&decl.variants[1].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " doc");
    // member-terminating newline + comment line + its terminating newline
    assert_eq!(newline_count(&decl.variants[1].leading_trivia), 2);

    // The first member has no comment; only a following member carries trivia.
    assert!(comment_lines(&decl.variants[0].leading_trivia).is_empty());
}

// ── Per-line layout (done_when 2) ──

#[test]
fn enum_per_line_comment_attaches_to_next_member() {
    let source = "\
ordo Color {
    Red,
    # doc
    Green
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Enum(decl) = &program.statements[0].kind else {
        panic!("expected enum declaration");
    };
    assert_eq!(decl.members.len(), 2);

    let comments = comment_lines(&decl.members[1].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " doc");
    // member-terminating newline + comment line + its terminating newline
    assert_eq!(newline_count(&decl.members[1].leading_trivia), 2);
}

#[test]
fn enum_comma_layout_comment_attaches_to_next_member() {
    let source = "\
ordo Color {
    Red,
    # doc
    Green
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Enum(decl) = &program.statements[0].kind else {
        panic!("expected enum declaration");
    };
    assert_eq!(decl.members.len(), 2);

    let comments = comment_lines(&decl.members[1].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " doc");
    assert_eq!(newline_count(&decl.members[1].leading_trivia), 2);
}

// ── Union variants (done_when 3) ──

#[test]
fn union_per_line_variants_attach_leading_trivia() {
    let source = "\
discretio Resultatum {
    Bene,
    # success payload
    Error
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Union(decl) = &program.statements[0].kind else {
        panic!("expected union declaration");
    };
    assert_eq!(decl.variants.len(), 2);

    let comments = comment_lines(&decl.variants[1].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(
        result.interner.resolve(comments[0].text),
        " success payload"
    );
    assert_eq!(newline_count(&decl.variants[1].leading_trivia), 2);
}

#[test]
fn union_variant_with_fields_attaches_leading_trivia() {
    let source = "\
discretio Resultatum {
    Bene { textus x },
    # error case
    Error { textus y }
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Union(decl) = &program.statements[0].kind else {
        panic!("expected union declaration");
    };
    assert_eq!(decl.variants.len(), 2);
    assert_eq!(decl.variants[1].fields.len(), 1);

    let comments = comment_lines(&decl.variants[1].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " error case");
}

// ── Attachment boundaries ──

#[test]
fn comment_after_open_brace_attaches_to_first_member() {
    let source = "\
ordo Color {
    # first member doc
    Red,
    Green
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Enum(decl) = &program.statements[0].kind else {
        panic!("expected enum declaration");
    };
    let comments = comment_lines(&decl.members[0].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(
        result.interner.resolve(comments[0].text),
        " first member doc"
    );
}

#[test]
fn enum_member_with_value_attaches_leading_trivia() {
    let source = "\
ordo Color {
    Red = 1,
    # doc
    Green = 2
}
";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Enum(decl) = &program.statements[0].kind else {
        panic!("expected enum declaration");
    };
    assert!(decl.members[0].value.is_some());
    assert!(decl.members[1].value.is_some());

    let comments = comment_lines(&decl.members[1].leading_trivia);
    assert_eq!(comments.len(), 1);
    assert_eq!(result.interner.resolve(comments[0].text), " doc");
}

#[test]
fn member_without_preceding_comment_has_empty_leading_trivia() {
    let source = "ordo Color { Red, Green }\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().unwrap();
    let StmtKind::Enum(decl) = &program.statements[0].kind else {
        panic!("expected enum declaration");
    };
    assert!(decl.members[0].leading_trivia.is_empty());
    assert!(decl.members[1].leading_trivia.is_empty());
}

// ── Out-of-scope boundary: block-end stays invalid placement ──

#[test]
fn enum_block_end_comment_stays_invalid_placement() {
    // A comment between the last member and the closing `}` is the block-end
    // position — out of scope for this unit; it must still be reported as
    // PARSE060 invalid_comment_placement, never silently discarded.
    let source = "\
ordo Color {
    Red
    # doc
}
";
    let result = parse_source(source);
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
}
