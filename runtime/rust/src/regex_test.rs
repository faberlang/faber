//! `faber::Regex` carrier tests — the compiled-package match surface.

use crate::Regex;
use crate::regex::{
    compile_or_trap, escape, find, find_all, literal_or_trap, match_end, match_group, match_named,
    match_start, match_text, matches, replace, replace_with, split,
};

fn rejected(pattern: &str) -> crate::regex::RegexError {
    Regex::new(pattern).expect_err("pattern must be rejected")
}

#[test]
fn matches_agrees_with_rust_regex_is_match() {
    let pattern = Regex::new("\\d+").expect("valid pattern");
    assert!(matches(&pattern, "abc123"));
    assert!(!matches(&pattern, "abc"));

    let anchored = Regex::new("^Roma").expect("valid pattern");
    assert!(matches(&anchored, "Romae"));
    assert!(!matches(&anchored, "in Roma"));
}

#[test]
fn invalid_pattern_fails_with_a_construct_id() {
    let error = rejected("(unclosed");
    assert_eq!(error.construct(), "syntax");
    assert!(error.to_string().starts_with("syntax: "));
}

#[test]
fn rejected_constructs_carry_their_stable_ids() {
    let cells = [
        ("a(?=b)", "lookahead"),
        ("a(?!b)", "lookahead"),
        ("(?<=a)b", "lookbehind"),
        ("(?<!a)b", "lookbehind"),
        ("(a)\\1", "backreference"),
        ("(?P<n>a)\\k<n>", "backreference"),
        ("(?P<n>a)(?P=n)", "backreference"),
        ("(?g)a", "unsupported_flag"),
        ("(?y)a", "unsupported_flag"),
        ("(?>a)", "atomic_group"),
        ("(a)(?1)", "recursion"),
        ("(?(1)a|b)", "conditional"),
        ("(", "syntax"),
        ("a)", "syntax"),
        ("[", "syntax"),
        ("a{2,1}", "syntax"),
        ("*a", "syntax"),
        ("\\", "syntax"),
        ("\\Z", "syntax"),
        ("\\p{Foo}", "syntax"),
        ("(?'n'a)", "syntax"),
        ("(?#c)a", "syntax"),
        // Reference extras the engine accepts but the dialect does not.
        ("(?R)", "recursion"),
        ("(?U)a+", "unsupported_flag"),
        ("(?u)a", "unsupported_flag"),
        ("a++", "possessive"),
        ("a*+", "possessive"),
        ("a**", "syntax"),
        ("a{1001}", "syntax"),
        ("\\u0041", "syntax"),
        ("[a[bc]]", "syntax"),
        ("[a-z&&[^m]]", "syntax"),
        ("\\b{start}", "syntax"),
        ("\\<", "syntax"),
    ];
    for (pattern, id) in cells {
        assert_eq!(rejected(pattern).construct(), id, "pattern {pattern:?}");
    }
}

#[test]
fn rejection_payload_leads_with_the_id_and_reports_a_code_point_offset() {
    let error = rejected("é(?=b)");
    assert_eq!(error.construct(), "lookahead");
    assert!(error.to_string().starts_with("lookahead: "));
    assert_eq!(error.offset(), 1);
}

#[test]
fn compiled_carrier_keeps_pattern_text_and_compares_by_it() {
    let a = Regex::new("a+").expect("valid pattern");
    let b = Regex::new("a+").expect("valid pattern");
    assert_eq!(a, b);
    assert_eq!(a.pattern(), "a+");
    assert_eq!(a.to_string(), "a+");
    assert_eq!(a.clone(), a);
}

fn compiled(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid pattern")
}

fn spans(pattern: &str, text: &str) -> Vec<(i64, i64)> {
    find_all(&compiled(pattern), text)
        .iter()
        .map(|found| (match_start(found), match_end(found)))
        .collect()
}

#[test]
fn find_reads_text_span_and_groups_in_code_points() {
    let pair = compiled("(?P<key>[a-z]+)=(?P<val>[0-9]+)");
    let hit = find(&pair, "x: size=42;").expect("a match");
    assert_eq!(match_text(&hit), "size=42");
    assert_eq!((match_start(&hit), match_end(&hit)), (3, 10));
    assert_eq!(match_group(&hit, 0).as_deref(), Some("size=42"));
    assert_eq!(match_group(&hit, 1).as_deref(), Some("size"));
    assert_eq!(match_named(&hit, "val").as_deref(), Some("42"));
    assert_eq!(match_named(&hit, "nope"), None);
    assert_eq!(match_group(&hit, 9), None);
    assert_eq!(match_group(&hit, -1), None);
    assert!(find(&pair, "no pairs here").is_none());

    let wide = find(&compiled("[a-z]+"), "😀é abc").expect("a match");
    assert_eq!((match_start(&wide), match_end(&wide)), (3, 6));
}

#[test]
fn a_group_that_did_not_take_part_is_none() {
    let either = compiled("(a)|(b)");
    let hit = find(&either, "xb").expect("a match");
    assert_eq!(match_group(&hit, 1), None);
    assert_eq!(match_group(&hit, 2).as_deref(), Some("b"));
}

#[test]
fn find_all_skips_an_empty_match_that_touches_the_previous_match() {
    assert_eq!(spans("a*", "baaac"), [(0, 0), (1, 4), (5, 5)]);
    assert_eq!(spans("", "ab"), [(0, 0), (1, 1), (2, 2)]);
    assert_eq!(spans("[0-9]+", "none here"), []);
    assert_eq!(spans("[a-z]+", "😀é abc d"), [(3, 6), (7, 8)]);
}

#[test]
fn split_keeps_edge_pieces_and_does_not_splice_groups() {
    let pieces = |pattern: &str, text: &str| split(&compiled(pattern), text);
    assert_eq!(pieces(",", ",a,"), ["", "a", ""]);
    assert_eq!(pieces(",", "a,,b"), ["a", "", "b"]);
    assert_eq!(pieces("(,)", "a,b"), ["a", "b"]);
    assert_eq!(pieces("x", "abc"), ["abc"]);
    assert_eq!(pieces(",", ""), [""]);
    assert_eq!(pieces("", "abc"), ["", "a", "b", "c", ""]);
}

#[test]
fn replace_takes_the_replacement_literally() {
    let pair = compiled("(?P<n>[a-z])([0-9])");
    assert_eq!(replace(&pair, "a1 b2", "$1"), "$1 $1");
    assert_eq!(replace(&pair, "a1 b2", "${n}-$2"), "${n}-$2 ${n}-$2");
    assert_eq!(replace(&pair, "a1 b2", "\\1\\2"), "\\1\\2 \\1\\2");
    assert_eq!(replace(&compiled("a*"), "baaac", "-"), "-b-c-");
    assert_eq!(replace(&compiled("[0-9]+"), "none here", "#"), "none here");
}

#[test]
fn replace_with_runs_the_closure_per_match_and_never_rescans() {
    let word = compiled("[a-z]+");
    assert_eq!(
        replace_with(&word, "ab cd e", |found| match_text(&found).to_uppercase()),
        "AB CD E"
    );
    assert_eq!(
        replace_with(&compiled("a"), "banana", |_| "aa"),
        "baanaanaa"
    );
    assert_eq!(
        replace_with(&compiled("a*"), "baaac", |found| {
            format!("<{}-{}>", match_start(&found), match_end(&found))
        }),
        "<0-0>b<1-4>c<5-5>"
    );
    assert_eq!(
        replace_with(&word, "😀é abc", |found| {
            format!("{}@{}", match_text(&found), match_start(&found))
        }),
        "😀é abc@3"
    );
}

#[test]
fn escape_backslashes_the_metacharacters_only() {
    assert_eq!(escape("a.b*c"), "a\\.b\\*c");
    assert_eq!(escape("é😀 x"), "é😀 x");
    let anchored = compiled(&format!("^{}$", escape("(1+1)=[2]{3}|?^$")));
    assert!(matches(&anchored, "(1+1)=[2]{3}|?^$"));
}

/// The panic text of a constructor that must trap.
fn trap_text(f: impl FnOnce() -> Regex) -> String {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .expect_err("a rejected pattern must trap");
    payload
        .downcast_ref::<String>()
        .cloned()
        .expect("trap payload is a String")
}

#[test]
fn compile_or_trap_returns_the_compiled_pattern() {
    let compiled = compile_or_trap("\\d+");
    assert!(matches(&compiled, "abc123"));
    assert_eq!(compiled.pattern(), "\\d+");
}

#[test]
fn compile_or_trap_fails_with_the_result_unwrap_text() {
    let expected = format!(
        "called `Result::unwrap()` on an `Err` value: {:?}",
        rejected("(unclosed")
    );
    assert_eq!(trap_text(|| compile_or_trap("(unclosed")), expected);
}

#[test]
fn literal_or_trap_returns_the_compiled_pattern() {
    let compiled = literal_or_trap("(?i)\\w+");
    assert!(matches(&compiled, "ABC"));
}

#[test]
fn literal_or_trap_fails_with_the_literal_validated_text() {
    let expected = format!("regex literal validated: {:?}", rejected("a(?=b)"));
    assert_eq!(trap_text(|| literal_or_trap("a(?=b)")), expected);
}
