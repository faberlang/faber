//! Parser expectation sites name a locale `issue` instead of an English prose
//! fragment (S14-6). These pin the identifier and string-literal sites of
//! `lib.rs` plus the converted `decl.rs` / `stmt.rs` sites by structured issue
//! only, never by message text.

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

#[test]
fn destructuring_before_await_assign_reports_await_binding_requires_name() {
    assert_parse_error_matches(
        "functio f() fiet { fixum [x] ↢ future }",
        ParseExpectation::Issue("await_binding_requires_name"),
    );
}

#[test]
fn non_literal_lane_value_reports_expected_lane_value() {
    assert_parse_error_matches(
        "@ radix lane 5\nfunctio f() → vacuum {}",
        ParseExpectation::Issue("expected_lane_value"),
    );
}

#[test]
fn enum_negative_missing_number_reports_enum_member_value_number() {
    assert_parse_error_matches(
        "ordo Color { Red = - }",
        ParseExpectation::Issue("enum_member_value_number"),
    );
}

#[test]
fn enum_missing_value_reports_enum_member_value() {
    assert_parse_error_matches(
        "ordo Color { Red = }",
        ParseExpectation::Issue("enum_member_value"),
    );
}
