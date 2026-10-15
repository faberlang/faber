//! Identifier policy tests.

use crate::names::Namer;

#[test]
fn keywords_and_library_names_are_renamed() {
    let mut scope = Namer::program_scope();
    assert_eq!(scope.claim("int"), "int_1");
    assert_eq!(scope.claim("bool"), "bool_1");
    assert_eq!(scope.claim("printf"), "printf_1");
    assert_eq!(scope.claim("stdout"), "stdout_1");
    assert_eq!(scope.claim("EOF"), "EOF_1");
    assert_eq!(scope.claim("main"), "main_1");
}

#[test]
fn reservation_patterns_are_rewritten_not_suffixed() {
    let mut scope = Namer::program_scope();
    assert_eq!(scope.claim("__x"), "v___x");
    assert_eq!(scope.claim("_Foo"), "v__Foo");
    assert_eq!(scope.claim("size_t"), "size_t_v");
    assert_eq!(scope.claim("INT64_MAX"), "v_INT64_MAX");
    assert_eq!(scope.claim("UINT8_C"), "v_UINT8_C");
    assert_eq!(scope.claim("PRId64"), "v_PRId64");
    // Lowercase underscore-led names are free at block scope.
    assert_eq!(scope.claim("_x"), "_x");
    // Mixed case is not a `<stdint.h>` macro spelling.
    assert_eq!(scope.claim("Interval"), "Interval");
}

#[test]
fn claims_are_unique_and_case_sensitive() {
    let mut scope = Namer::program_scope();
    assert_eq!(scope.claim("x"), "x");
    assert_eq!(scope.claim("x"), "x_1");
    assert_eq!(scope.claim("X"), "X");
    assert_eq!(scope.claim("size_t"), "size_t_v");
    assert_eq!(scope.claim("size_t"), "size_t_v_1");
}

#[test]
fn emitter_prefix_is_reserved() {
    let mut scope = Namer::program_scope();
    assert_eq!(scope.claim("faber_show"), "v_faber_show");
    assert_eq!(scope.claim("9lives"), "_9lives");
}
