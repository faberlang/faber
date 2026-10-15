//! Parser tests for the retired wrapped numeric spelling (N7d-lock).
//!
//! Contract: `numerus<W>` / `fractus<W>` (every locale word of the type pack)
//! is rejected whole, with PARSE040 `numeric_wrapper_retired` carrying the form
//! as written (`form`) and the bare replacement (`bare`). The wrapper covers
//! family-correct forms, wrong-family forms, `d64`/`inf`, the removed `d32`,
//! the marker holes and every extra-argument form. The parse recovers (the
//! wrapper is consumed, the type becomes the bare marker or bare word), so
//! exactly one diagnostic is reported per wrapper. The policy words keep their
//! `<W>` argument and the bare markers parse as before.

use crate::test_support::has_arg;
use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{StmtKind, TypeExprKind};

const ISSUE: &str = "numeric_wrapper_retired";

/// Type words of every locale pack for the integer and float families.
const INT_WORDS: [&str; 7] = ["numerus", "int", "整数", "عدد", "संख्या", "จํานวน", "số"];
const FLOAT_WORDS: [&str; 7] = [
    "fractus",
    "float",
    "小数",
    "كسر",
    "भिन्न",
    "เศษ",
    "thập_phân",
];

/// A reader surface that knows every locale word of the two numeric families.
struct AllNumericWords;

impl KeywordPack for AllNumericWords {
    fn pack_id(&self) -> &'static str {
        "all-numeric-words-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "fn" => Some("functio"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        if INT_WORDS.contains(&text) {
            Some("numerus")
        } else if FLOAT_WORDS.contains(&text) {
            Some("fractus")
        } else {
            // The pack also spells the Latin words it uses as its own types.
            [
                "lista",
                "tabula",
                "tensor",
                "vector",
                "matrix",
                "modulus",
                "saturatus",
                "exactus",
                "octetus",
            ]
            .into_iter()
            .find(|word| *word == text)
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_words(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &AllNumericWords))
}

fn parse_clean(source: &str) -> ParseResult {
    let result = parse_words(source);
    assert!(result.success(), "{source}: {:?}", result.errors);
    result
}

fn retired(result: &ParseResult) -> Vec<&crate::ParseError> {
    result
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", ISSUE))
        .collect()
}

/// Assert `source` yields exactly one wrapper diagnostic, and nothing else.
fn assert_one_wrapper(source: &str, form: &str, bare: &str) {
    let result = parse_words(source);
    let found = retired(&result);
    assert_eq!(
        found.len(),
        1,
        "{source}: expected one {ISSUE}, got {:?}",
        result.errors
    );
    assert_eq!(
        result.errors.len(),
        1,
        "{source}: the rejection must not cascade, got {:?}",
        result.errors
    );
    let err = found[0];
    assert!(has_arg(err, "form", form), "{source}: form {:?}", err.args);
    assert!(has_arg(err, "bare", bare), "{source}: bare {:?}", err.args);
}

#[test]
fn every_family_correct_wrapper_is_rejected_with_its_bare_marker() {
    for (word, width) in [
        ("numerus", "i8"),
        ("numerus", "i16"),
        ("numerus", "i32"),
        ("numerus", "i64"),
        ("numerus", "u8"),
        ("numerus", "u16"),
        ("numerus", "u32"),
        ("numerus", "u64"),
        ("numerus", "d64"),
        ("numerus", "inf"),
        ("fractus", "f16"),
        ("fractus", "f32"),
        ("fractus", "f64"),
        ("fractus", "bf16"),
    ] {
        assert_one_wrapper(
            &format!("let {word}<{width}> x ← 1"),
            &format!("{word}<{width}>"),
            width,
        );
    }
}

#[test]
fn every_locale_word_of_the_type_pack_is_rejected() {
    for word in INT_WORDS {
        assert_one_wrapper(
            &format!("let {word}<i32> x ← 1"),
            &format!("{word}<i32>"),
            "i32",
        );
        assert_one_wrapper(&format!("let {word}<_> x ← 1"), &format!("{word}<_>"), word);
    }
    for word in FLOAT_WORDS {
        assert_one_wrapper(
            &format!("let {word}<f32> x ← 1.0"),
            &format!("{word}<f32>"),
            "f32",
        );
        assert_one_wrapper(
            &format!("let {word}<_> x ← 1.0"),
            &format!("{word}<_>"),
            word,
        );
    }
}

#[test]
fn wrong_family_wrappers_are_rejected_as_the_wrapper() {
    assert_one_wrapper("let numerus<f32> x ← 1", "numerus<f32>", "f32");
    assert_one_wrapper("let fractus<i32> x ← 1", "fractus<i32>", "i32");
    assert_one_wrapper("let int<f32> x ← 1", "int<f32>", "f32");
}

#[test]
fn the_marker_holes_are_rejected_and_name_the_bare_word() {
    assert_one_wrapper("let numerus<_> x ← 1", "numerus<_>", "numerus");
    assert_one_wrapper("let fractus<_> x ← 1", "fractus<_>", "fractus");
}

#[test]
fn the_removed_d32_and_extra_arguments_are_rejected_as_the_wrapper() {
    assert_one_wrapper("let numerus<d32> x ← 1", "numerus<d32>", "numerus");
    assert_one_wrapper(
        "let numerus<u32, Foo> x ← 1",
        "numerus<u32, Foo>",
        "numerus",
    );
    assert_one_wrapper("let numerus<W> x ← 1", "numerus<W>", "numerus");
}

#[test]
fn a_wrapper_is_rejected_in_every_type_position() {
    for (source, form, bare) in [
        ("let lista<numerus<i32>> xs ← vacua", "numerus<i32>", "i32"),
        (
            "let tensor<fractus<f32>, [2, 3]> t ← vacua",
            "fractus<f32>",
            "f32",
        ),
        ("let n ← s ↦ numerus<u32>", "numerus<u32>", "u32"),
        ("let n ← x ↦ numerus<i32> ⊥ 0", "numerus<i32>", "i32"),
        ("let lista<numerus<u64>> ids ← vacua", "numerus<u64>", "u64"),
    ] {
        assert_one_wrapper(source, form, bare);
    }
    // Parameter and return positions report one diagnostic each.
    let result = parse_words("fn f(numerus<i32> a) → fractus<f64> { redde 1.0 }");
    assert_eq!(retired(&result).len(), 2, "{:?}", result.errors);
}

#[test]
fn the_rejection_recovers_so_later_diagnostics_still_appear() {
    let result = parse_words("let numerus<i32> a ← 1\nlet b ← )\nlet numerus<u8> c ← 3");
    assert_eq!(retired(&result).len(), 2, "{:?}", result.errors);
    let program = result.program.as_ref().expect("recovered program");
    // The first declaration keeps its annotation as the bare marker.
    let StmtKind::Var(first) = &program.statements[0].kind else {
        panic!("expected var, got {:?}", program.statements[0].kind);
    };
    let TypeExprKind::Named(name, args) = &first.ty.as_ref().expect("annotation").kind else {
        panic!("expected a named type");
    };
    assert_eq!(result.interner.resolve(name.name), "i32");
    assert!(args.is_empty());
    // The unrelated error on the second line is not swallowed.
    assert!(
        result.errors.len() >= 3,
        "a later, unrelated error must still be reported: {:?}",
        result.errors
    );
}

#[test]
fn the_bare_markers_and_bare_words_parse() {
    for source in [
        "let i32 x ← 1",
        "let u8 x ← 1",
        "let d64 x ← 1",
        "let inf x ← 1",
        "let f32 x ← 1.0",
        "let bf16 x ← 1.0",
        "let numerus x ← 1",
        "let fractus x ← 1.0",
        "let lista<u32> xs ← vacua",
        "let tensor<f32, [2, 3]> t ← vacua",
        "let n ← s ↦ u32",
    ] {
        let _ = parse_clean(source);
    }
}

#[test]
fn the_policy_words_keep_their_width_argument() {
    for source in [
        "let saturatus<i32> x ← 1",
        "let modulus<u32> x ← 1",
        "let exactus<u16> x ← 1",
        "let saturatus<inf> x ← 1",
        "let modulus<u8> x ← 1",
    ] {
        let _ = parse_clean(source);
    }
}
