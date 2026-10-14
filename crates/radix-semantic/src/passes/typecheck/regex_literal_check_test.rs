//! Inline regex literals are validated at check time (ruling d381ae56 point 3).
//!
//! `regex /pattern/` is infallible: its raw pattern compiles at `faber check`
//! through the same engine family as the Rust runtime `regex` crate, so a
//! malformed pattern fails with `SEM016` `regex_literal_invalid`. The failable
//! `↦ regex` conversion for dynamic patterns is unchanged. Assertions are
//! structured (issue names), never message text.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

const ISSUE: &str = "regex_literal_invalid";

#[test]
fn malformed_literal_pattern_fails_check() {
    assert_rejects_with_issue(r"incipit { fixum regex r ← regex /(/ }", ISSUE);
}

#[test]
fn valid_literal_pattern_passes_check() {
    assert_accepts(r"incipit { fixum regex r ← regex /a+(b|c)?/ }");
}

#[test]
fn inline_flag_literal_pattern_passes_check() {
    assert_accepts(r"incipit { fixum regex r ← regex /(?i)\d+/ }");
}

#[test]
fn dynamic_conversion_stays_failable_not_check_time() {
    assert_accepts(r#"incipit { fixum regex r ← "(" ↦ regex }"#);
}
