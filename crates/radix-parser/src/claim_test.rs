//! Post-parse completeness: every `LineComment` token is claimed by a legal
//! syntax trivia carrier or reported with one structured invalid-placement
//! diagnostic (never silently discarded, never a generic parse error).

use crate::{parse, ParseError, ParseErrorKind};
use radix_lexer::lex;
use radix_lexer::TokenKind;

fn parse_source(source: &str) -> crate::ParseResult {
    parse(lex(source))
}

fn invalid_placement_count(result: &crate::ParseResult) -> usize {
    result
        .errors
        .iter()
        .filter(|e| e.kind == ParseErrorKind::InvalidCommentPlacement)
        .count()
}

/// Assert exactly one structured invalid-placement diagnostic with stable
/// `issue` and `comment` facts (never prose assertions).
fn assert_invalid_comment_placement(result: &crate::ParseResult, comment_text: &str) {
    assert!(!result.success(), "{:?}", result.errors);
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
    let err: &ParseError = &result.errors[0];
    assert_eq!(
        err.kind,
        ParseErrorKind::InvalidCommentPlacement,
        "{:?}",
        result.errors
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "invalid_comment_placement"),
        "expected issue fact: {:?}",
        err.args
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "comment" && arg.value == comment_text),
        "expected comment fact {:?}: {:?}",
        comment_text,
        err.args
    );
}

// ── Invalid positions ──

#[test]
fn comment_between_annotations_is_invalid() {
    let source = "@ publica\n# between\n@ cli \"tool\"\nfunctio f() → vacuum { }\n";
    let result = parse_source(source);
    assert_invalid_comment_placement(&result, " between");
}

#[test]
fn comment_before_genus_close_is_claimed() {
    // MT-U2: a full-line comment before a genus's closing `}` is legal
    // ClassDecl.end_trivia (mirrors block-end trivia), so it is claimed and no
    // invalid-placement diagnostic is reported.
    let source = "genus Canis {\n  textus nomen\n  # before close\n}\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(invalid_placement_count(&result), 0);

    use radix_syntax::{StmtKind, Trivia};
    let program = result.program.as_ref().unwrap();
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus declaration");
    };
    let comments: Vec<&str> = class
        .end_trivia
        .iter()
        .filter_map(|item| match item {
            Trivia::CommentLine(comment) => Some(result.interner.resolve(comment.text)),
            Trivia::Newline(_) => None,
        })
        .collect();
    assert_eq!(comments, vec![" before close"]);
    assert_eq!(class.members.len(), 1);
}

#[test]
fn comment_before_implendum_close_is_invalid() {
    let source = "implendum Forma {\n  functio describat() → textus\n  # before close\n}\n";
    let result = parse_source(source);
    assert_invalid_comment_placement(&result, " before close");
}

#[test]
fn comment_inside_expression_is_invalid() {
    let source = "fixum numerus x ← 1 +\n# mid\n2\n";
    let result = parse_source(source);
    assert_invalid_comment_placement(&result, " mid");
}

#[test]
fn comment_between_condition_and_ergo_is_invalid() {
    let source = "si verum\n# joint\nergo redde 1\n";
    let result = parse_source(source);
    assert_invalid_comment_placement(&result, " joint");
}

#[test]
fn comment_inside_sic_secus_ternary_is_invalid() {
    let source = "fixum numerus x ← verum sic\n# mid\n1 secus 2\n";
    let result = parse_source(source);
    assert_invalid_comment_placement(&result, " mid");
}

// ── Legal position: after last declaration / program end ──

#[test]
fn comment_after_last_declaration_is_claimed_on_program_end() {
    let source = "functio f() → vacuum { }\n# tail\n";
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(invalid_placement_count(&result), 0);
    let program = result.program.as_ref().unwrap();
    let comment = program
        .end_trivia
        .iter()
        .find_map(|item| match item {
            radix_syntax::Trivia::CommentLine(c) => Some(c),
            radix_syntax::Trivia::Newline(_) => None,
        })
        .expect("program-end comment");
    assert_eq!(result.interner.resolve(comment.text), " tail");
}

// ── Completeness matrix ──

#[test]
fn every_line_comment_token_is_claimed_or_diagnosed() {
    let source = "\
# top
@ publica
# between annotations
@ cli \"tool\"
functio f() → vacuum {
  # inner
  tacet
  # block end
}
# program end
";
    let result = parse_source(source);
    let program = result.program.as_ref().expect("program still builds");

    // Union of legal-carrier claims and invalid-placement diagnostic spans.
    let mut represented = crate::claim::claimed_comment_spans(program);
    for err in &result.errors {
        assert_eq!(
            err.kind,
            ParseErrorKind::InvalidCommentPlacement,
            "only structured invalid placement is allowed: {:?}",
            result.errors
        );
        represented.insert(err.span.start);
    }

    // Exactly the between-annotations comment is invalid; the rest are claimed.
    assert_eq!(invalid_placement_count(&result), 1, "{:?}", result.errors);

    // Every LineComment token is represented exactly once.
    let lexed = lex(source);
    let comment_tokens: Vec<_> = lexed
        .tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::LineComment(_)))
        .collect();
    assert_eq!(
        comment_tokens.len(),
        represented.len(),
        "claim + diagnostic counts must cover every comment token"
    );
    for token in &comment_tokens {
        assert!(
            represented.contains(&token.span.start),
            "unclaimed comment token at byte offset {}",
            token.span.start
        );
    }
}
