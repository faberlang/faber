//! A static (`generis`) genus field reads through the type name; a static read
//! through an instance and an instance field read through the type name are
//! named compile errors. Inside the genus's own members the path `Tab` is also
//! the receiver (`ego` lowers to the same path), so the receiver keeps reading.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support;

const TAB: &str = "genus Tab {\n    generis numerus Z = 3\n    varia numerus w = 1\n    functio f() → numerus { redde ego.w + ego.Z }\n}\n";

#[test]
fn static_field_read_through_type_name_is_accepted() {
    test_support::assert_accepts(&format!("{TAB}functio g() → numerus {{ redde Tab.Z + 1 }}"));
}

#[test]
fn static_field_read_through_instance_is_named() {
    test_support::assert_rejects_with_issue(
        &format!("{TAB}functio g(Tab t) → numerus {{ redde t.Z }}"),
        "static_field_read_through_instance",
    );
}

#[test]
fn static_field_read_of_instance_field_through_type_is_named() {
    test_support::assert_rejects_with_issue(
        &format!("{TAB}functio g() → numerus {{ redde Tab.w }}"),
        "instance_field_read_through_type",
    );
}

#[test]
fn static_field_read_of_unknown_field_through_type_is_unknown_struct_field() {
    test_support::assert_rejects_with_issue(
        &format!("{TAB}functio g() → numerus {{ redde Tab.nope }}"),
        "unknown_struct_field",
    );
}

#[test]
fn static_field_read_call_through_type_name_keeps_name_not_value() {
    test_support::assert_rejects_with_issue(
        &format!("{TAB}functio g() → numerus {{ redde Tab.f() }}"),
        "name_not_value",
    );
}
