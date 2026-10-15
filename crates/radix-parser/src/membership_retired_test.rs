//! The retired membership words (membership-glyph unit 1c).
//!
//! `intra` / `inter` and every reader pack's spelling of them are no longer
//! operators: `∈` / `∉` are the only membership spellings. A retired word in
//! operator position records `PARSE001 membership_keyword_retired` with args
//! `keyword` (as written) and `glyph` (`∈`), and the operand still parses as
//! `Member` so recovery continues. Everywhere else the words stay ordinary
//! identifiers.
//!
//! Tests assert code (the error kind), issue and args only, never prose.

use crate::test_support::{has_arg, parse_ok, parse_program};
use crate::{ParseErrorKind, ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{BinOp, ExprKind, StmtKind};

const ISSUE: &str = "membership_keyword_retired";

/// A reader surface that declares the retired words' rows (en and zh-Hans)
/// beside the `const` introducer, the way the installed packs keep them.
struct RetiredWordsPack {
    id: &'static str,
}

impl KeywordPack for RetiredWordsPack {
    fn pack_id(&self) -> &str {
        self.id
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "between" | "间" => Some("inter"),
            "within" | "内" => Some("intra"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_with(id: &'static str, source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &RetiredWordsPack { id }))
}

/// The single retirement error, asserted for `keyword`, and the binary the
/// recovery still builds for the initializer.
fn assert_retired(result: &ParseResult, keyword: &str) -> BinOp {
    let found: Vec<_> = result
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", ISSUE))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected one {ISSUE} error, got {:?}",
        result.errors
    );
    assert_eq!(result.errors.len(), 1, "no cascade: {:?}", result.errors);
    let err = found[0];
    assert_eq!(err.kind, ParseErrorKind::Expected);
    assert!(has_arg(err, "keyword", keyword), "keyword: {:?}", err.args);
    assert!(has_arg(err, "glyph", "∈"), "glyph: {:?}", err.args);
    let program = result.program.as_ref().expect("recovery keeps the program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected a binding");
    };
    let init = decl.init.as_ref().expect("initializer");
    let ExprKind::Binary(bin) = &init.kind else {
        panic!("recovery keeps the operand as a binary expression");
    };
    bin.op
}

#[test]
fn latin_inter_and_intra_are_retired() {
    let result = parse_program("sit v ← x inter xs");
    assert!(matches!(assert_retired(&result, "inter"), BinOp::Member));
    let result = parse_program("sit v ← n intra 0‥10");
    assert!(matches!(assert_retired(&result, "intra"), BinOp::Member));
}

#[test]
fn english_between_and_within_are_retired() {
    let result = parse_with("en", "let v ← x between xs");
    assert!(matches!(assert_retired(&result, "between"), BinOp::Member));
    let result = parse_with("en", "let v ← n within 0‥10");
    assert!(matches!(assert_retired(&result, "within"), BinOp::Member));
}

#[test]
fn a_non_latin_pack_word_is_retired() {
    let result = parse_with("zh-Hans", "let v ← x 间 xs");
    assert!(matches!(assert_retired(&result, "间"), BinOp::Member));
    let result = parse_with("zh-Hans", "let v ← n 内 0‥10");
    assert!(matches!(assert_retired(&result, "内"), BinOp::Member));
}

#[test]
fn recovery_continues_past_the_retired_word() {
    // The operand parses as `Member`, so the logical tier above it still
    // builds and the one diagnostic is not followed by a cascade.
    let result = parse_program("sit v ← x inter xs et y");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
    assert!(has_arg(&result.errors[0], "issue", ISSUE));
}

#[test]
fn the_words_stay_ordinary_identifiers() {
    let _ = parse_ok("sit numerus inter ← 3\nsit numerus intra ← inter + 1");
    let _ = parse_ok("sit v ← blk.get(inter)\nsit w ← f(intra, inter)");
    let result = parse_with("en", "let int between ← 3\nlet int within ← between + 1");
    assert!(
        result
            .errors
            .iter()
            .all(|err| !has_arg(err, "issue", ISSUE)),
        "{:?}",
        result.errors
    );
}

#[test]
fn glyph_membership_is_unaffected() {
    let result = parse_ok("sit v ← x ∈ xs");
    assert!(result.errors.is_empty());
}
