//! `faber::Regex` carrier tests — the compiled-package match surface.

use crate::Regex;

#[test]
fn consentit_matches_like_rust_regex_is_match() {
    let pattern = Regex::new("\\d+");
    assert!(pattern.consentit("abc123".to_owned()));
    assert!(!pattern.consentit("abc".to_owned()));

    let anchored = Regex::new("^Roma");
    assert!(anchored.consentit("Romae".to_owned()));
    assert!(!anchored.consentit("in Roma".to_owned()));
}

#[test]
fn invalid_pattern_deterministically_matches_nothing() {
    // Patterns are compiler-validated literals in practice; an invalid
    // pattern must not panic the compiled runtime.
    let broken = Regex::new("(unclosed");
    assert!(!broken.consentit("anything".to_owned()));
}
