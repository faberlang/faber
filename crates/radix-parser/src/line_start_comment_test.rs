//! Line-start `#` comment policy: lexer rejects mid-line `#`; parser surfaces LEX007.

use crate::test_support::args_have_no_legacy_message;
use crate::{ParseErrorKind, parse};
use radix_lexer::{INLINE_COMMENT_ISSUE, LexErrorKind, lex};

const BRACE_INLINE_FIXTURE: &str = "functio f() → vacuum { sit numerus x ← 1  # inline }\n";

fn inline_hash_offset(source: &str) -> u32 {
    u32::try_from(source.find('#').expect("# in fixture"))
        .expect("test fixture byte offset fits u32")
}

fn assert_lex_inline_comment(source: &str) {
    let lex_result = lex(source);
    assert!(!lex_result.success());
    let err = lex_result
        .errors
        .iter()
        .find(|err| err.kind == LexErrorKind::InlineComment)
        .expect("inline comment diagnostic");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == INLINE_COMMENT_ISSUE)
    );
    assert_eq!(err.span.start, inline_hash_offset(source));
}

#[test]
fn brace_swallow_reports_inline_comment_not_missing_brace() {
    let hash_offset = inline_hash_offset(BRACE_INLINE_FIXTURE);

    assert_lex_inline_comment(BRACE_INLINE_FIXTURE);

    let parse_result = parse(lex(BRACE_INLINE_FIXTURE));
    assert!(!parse_result.success());
    assert_eq!(parse_result.errors.len(), 1, "{:?}", parse_result.errors);
    assert_eq!(parse_result.errors[0].kind, ParseErrorKind::LexError);
    assert!(
        parse_result.errors[0]
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == INLINE_COMMENT_ISSUE),
        "parser lex passthrough should preserve structured lexer args: {:?}",
        parse_result.errors[0].args
    );
    assert!(
        args_have_no_legacy_message(&parse_result.errors[0].args),
        "parser lex passthrough should not preserve lexer prose: {:?}",
        parse_result.errors[0].args
    );
    assert_eq!(parse_result.errors[0].span.start, hash_offset);
    assert!(
        !parse_result
            .errors
            .iter()
            .any(|err| err.kind != ParseErrorKind::LexError),
        "parse errors should surface the lexer diagnostic, not a missing brace: {:?}",
        parse_result.errors
    );
}

#[test]
fn indented_line_start_comment_before_stmt_still_parses() {
    let result = parse(lex("  # indented comment\nnota 0\n"));
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn leading_comment_before_functio_still_parses() {
    let result = parse(lex("# lead\nfunctio f() → vacuum { }\n"));
    assert!(result.success(), "{:?}", result.errors);
}
