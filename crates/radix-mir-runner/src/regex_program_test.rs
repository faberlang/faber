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
