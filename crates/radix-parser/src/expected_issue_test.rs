//! Parser expectation sites name a locale `issue` instead of an English prose
//! fragment (S14-6). These pin the identifier and string-literal sites of
//! `lib.rs` by structured issue only, never by message text.

use crate::test_support::{ParseExpectation, assert_parse_error_matches};

#[test]
fn non_identifier_binding_name_reports_expected_identifier() {
    assert_parse_error_matches(
        "fixum magnitudo 5 = 1",
        ParseExpectation::Issue("expected_identifier"),
    );
}

#[test]
fn non_string_scriptum_template_reports_expected_string_literal() {
    assert_parse_error_matches(
        "functio f() { scriptum(5) }",
        ParseExpectation::Issue("expected_string_literal"),
    );
}
