//! `faber::Regex` carrier tests — the compiled-package match surface.

use crate::Regex;

fn rejected(pattern: &str) -> crate::regex::RegexError {
    Regex::new(pattern).expect_err("pattern must be rejected")
}

#[test]
fn consentit_matches_like_rust_regex_is_match() {
    let pattern = Regex::new("\\d+").expect("valid pattern");
    assert!(pattern.consentit("abc123".to_owned()));
    assert!(!pattern.consentit("abc".to_owned()));

    let anchored = Regex::new("^Roma").expect("valid pattern");
    assert!(anchored.consentit("Romae".to_owned()));
    assert!(!anchored.consentit("in Roma".to_owned()));
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
