//! RD-4 construct ids for the runner's regex validator (RX-2b).
//!
//! Every assertion reads the payload's leading id and never the engine's
//! prose, which belongs to the `regex` crate version.

use super::RegexProgram;

fn rejected_id(pattern: &str) -> String {
    let payload = RegexProgram::compile(pattern)
        .err()
        .unwrap_or_else(|| panic!("{pattern:?} should be rejected"));
    let (id, _) = payload
        .split_once(": ")
        .unwrap_or_else(|| panic!("payload has no id separator: {payload:?}"));
    id.to_owned()
}

#[test]
fn rejected_constructs_carry_their_stable_id() {
    let cells = [
        ("(", "syntax"),
        ("a)", "syntax"),
        ("[", "syntax"),
        ("a{2,1}", "syntax"),
        ("*a", "syntax"),
        ("a**", "syntax"),
        ("\\", "syntax"),
        ("\\Z", "syntax"),
        ("\\Qa.b\\E", "syntax"),
        ("\\p{Foo}", "syntax"),
        ("(?'n'a)", "syntax"),
        ("(?#c)a", "syntax"),
        ("[a[bc]]", "syntax"),
        ("[a-z&&[^m]]", "syntax"),
        ("\\<", "syntax"),
        ("\\b{start}", "syntax"),
        ("\\u0041", "syntax"),
        ("\\u{41}", "syntax"),
        ("a{1001}", "syntax"),
        ("a(?=b)", "lookahead"),
        ("a(?!b)", "lookahead"),
        ("(?<=a)b", "lookbehind"),
        ("(?<!a)b", "lookbehind"),
        ("(a)\\1", "backreference"),
        ("(?P<n>a)\\k<n>", "backreference"),
        ("(?P<n>a)(?P=n)", "backreference"),
        ("\\0", "backreference"),
        ("(?g)a", "unsupported_flag"),
        ("(?y)a", "unsupported_flag"),
        ("(?U)a+", "unsupported_flag"),
        ("(?u)a", "unsupported_flag"),
        ("(?-u)a", "unsupported_flag"),
        ("(?>a)", "atomic_group"),
        ("a++", "possessive"),
        ("a*+", "possessive"),
        ("a?+", "possessive"),
        ("(?R)", "recursion"),
        ("(a)(?1)", "recursion"),
        ("(?(1)a|b)", "conditional"),
    ];
    for (pattern, id) in cells {
        assert_eq!(rejected_id(pattern), id, "pattern {pattern:?}");
    }
}

#[test]
fn accepted_patterns_compile_and_match() {
    let cells = [
        ("a+", "caab", true),
        ("(?i)[a-z]+", "ABC", true),
        ("\\d+", "x7", true),
        ("(?P<n>a)(?:b|c)", "ab", true),
        ("(?<n>a)", "a", true),
        ("[[:alpha:]]+", "x", true),
        ("a{1000}", "a", false),
        ("^\\bé", "é", true),
        ("\\x41", "A", true),
        ("(?i)a(?-i)b", "Ab", true),
        ("a+?", "a", true),
        ("", "", true),
    ];
    for (pattern, text, expected) in cells {
        let program = RegexProgram::compile(pattern)
            .unwrap_or_else(|payload| panic!("{pattern:?} rejected: {payload}"));
        assert_eq!(program.pattern(), pattern);
        assert_eq!(program.is_match(text), expected, "pattern {pattern:?}");
    }
}

#[test]
fn find_reports_code_point_offsets_not_bytes() {
    let program = RegexProgram::compile("[a-z]+").expect("valid pattern");
    let found = program.find("😀é abc").expect("a match");
    assert_eq!(found.text, "abc");
    assert_eq!((found.start, found.end), (3, 6));
    assert!(program.find("123").is_none());
}

#[test]
fn find_groups_are_indexed_named_and_optional() {
    let program = RegexProgram::compile("(a)|(?P<second>b)").expect("valid pattern");
    let found = program.find("xb").expect("a match");
    assert_eq!(found.group(0), Some("b"));
    assert_eq!(found.group(1), None);
    assert_eq!(found.group(2), Some("b"));
    assert_eq!(found.group(9), None);
    assert_eq!(found.named("second"), Some("b"));
    assert_eq!(found.named("missing"), None);
}

fn spans(pattern: &str, text: &str) -> Vec<(usize, usize)> {
    RegexProgram::compile(pattern)
        .expect("valid pattern")
        .find_all(text)
        .iter()
        .map(|found| (found.start, found.end))
        .collect()
}

#[test]
fn find_all_follows_the_empty_match_rule_in_code_points() {
    assert_eq!(spans("a*", "baaac"), [(0, 0), (1, 4), (5, 5)]);
    assert_eq!(spans("a*", "aa"), [(0, 2)]);
    assert_eq!(spans("", "ab"), [(0, 0), (1, 1), (2, 2)]);
    assert_eq!(spans("a*", "😀aa😀"), [(0, 0), (1, 3), (4, 4)]);
    assert_eq!(spans("[a-z]+", "😀é abc d"), [(3, 6), (7, 8)]);
    assert!(spans("x", "abc").is_empty());
}

#[test]
fn find_all_keeps_groups_and_names_per_match() {
    let program = RegexProgram::compile("(?P<k>[a-z])(\\d)?").expect("valid pattern");
    let found = program.find_all("a1 b");
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].named("k"), Some("a"));
    assert_eq!(found[0].group(2), Some("1"));
    assert_eq!(found[1].named("k"), Some("b"));
    assert_eq!(found[1].group(2), None);
}

#[test]
fn split_keeps_edges_and_never_splices_groups() {
    let pieces = |pattern: &str, text: &str| {
        RegexProgram::compile(pattern)
            .expect("valid pattern")
            .split(text)
    };
    assert_eq!(pieces(",", ",a,"), ["", "a", ""]);
    assert_eq!(pieces(",", "a,,b"), ["a", "", "b"]);
    assert_eq!(pieces("(,)", "a,b"), ["a", "b"]);
    assert_eq!(pieces("", "abc"), ["", "a", "b", "c", ""]);
    assert_eq!(pieces("a*", "baaac"), ["", "b", "c", ""]);
    assert_eq!(pieces(",", ""), [""]);
    assert_eq!(pieces("x", "abc"), ["abc"]);
}

#[test]
fn replace_all_literal_never_expands_the_replacement() {
    let replace = |pattern: &str, text: &str, with: &str| {
        RegexProgram::compile(pattern)
            .expect("valid pattern")
            .replace_all_literal(text, with)
    };
    assert_eq!(replace("a+", "caab aa", "x"), "cxb x");
    assert_eq!(replace("(?P<n>[a-z])(\\d)", "a1 b2", "$1"), "$1 $1");
    assert_eq!(replace("(?P<n>[a-z])(\\d)", "a1", "${n}-$2"), "${n}-$2");
    assert_eq!(replace("(?P<n>[a-z])(\\d)", "a1", "\\1\\2"), "\\1\\2");
    assert_eq!(replace("a*", "baaac", "-"), "-b-c-");
    assert_eq!(replace("", "ab", "-"), "-a-b-");
    assert_eq!(replace("x", "abc", "#"), "abc");
    assert_eq!(replace("[0-9]+", "a12b3", ""), "ab");
}

#[test]
fn replace_each_joins_one_result_per_match_literally() {
    let join = |pattern: &str, text: &str, results: &[&str]| {
        let results: Vec<String> = results.iter().map(|piece| (*piece).to_owned()).collect();
        RegexProgram::compile(pattern)
            .expect("valid pattern")
            .replace_each(text, &results)
    };
    assert_eq!(join("a+", "caab aa", &["X", "Y"]).as_deref(), Some("cXb Y"));
    // A result is never expanded or searched again.
    assert_eq!(
        join("a", "banana", &["aa", "$1", "a"]).as_deref(),
        Some("baan$1na")
    );
    // The same empty-match rule as `find_all`: spans (0,0) (1,4) (5,5).
    assert_eq!(
        join("a*", "baaac", &["<", "-", ">"]).as_deref(),
        Some("<b-c>")
    );
    assert_eq!(join("x", "abc", &[]).as_deref(), Some("abc"));
    assert_eq!(join("é", "é😀é", &["e", "E"]).as_deref(), Some("e😀E"));
    // One result per match: a count mismatch is refused, never padded.
    assert_eq!(join("a", "aa", &["x"]), None);
    assert_eq!(join("a", "aa", &["x", "y", "z"]), None);
}

#[test]
fn escape_covers_exactly_the_metacharacters_and_round_trips() {
    let metacharacters = "\\.+*?()|[]{}^$#&-~";
    for meta in metacharacters.chars() {
        assert_eq!(super::escape(&meta.to_string()), format!("\\{meta}"));
    }
    assert_eq!(
        super::escape("é😀 plain 日本語\t\n"),
        "é😀 plain 日本語\t\n"
    );
    let probes = [
        "",
        "a.b",
        "x+y*z",
        "(a|b)",
        "[a-z]{2}",
        "^start$",
        "a\\d",
        "naïve café 😀",
        metacharacters,
    ];
    for probe in probes {
        let program = RegexProgram::compile(&format!("^{}$", super::escape(probe)))
            .expect("an escaped pattern is accepted by the dialect");
        let found = program.find(probe).expect("the escaped pattern matches");
        assert_eq!(found.text, probe);
        assert_eq!(found.start, 0);
        assert_eq!(found.end, probe.chars().count());
    }
}
