//! Identifier policy tests. The Faber-level cases (a local named `char`,
//! `stdout`, ...) are in `emit_test.rs`; these check sanitizing and uniqueness
//! of already-prefixed names.

use crate::names::Namer;

#[test]
fn prefixed_names_keep_their_spelling() {
    let mut scope = Namer::program_scope();
    assert_eq!(scope.claim("l_char"), "l_char");
    assert_eq!(scope.claim("l_INT64_MAX"), "l_INT64_MAX");
    assert_eq!(scope.claim("p___x"), "p___x");
}

#[test]
fn non_ascii_is_sanitized_and_collisions_take_a_suffix() {
    let mut scope = Namer::program_scope();
    assert_eq!(scope.claim("l_caf\u{e9}"), "l_caf_");
    assert_eq!(scope.claim("l_caf\u{e8}"), "l_caf__1");
}

#[test]
fn claims_are_unique_and_case_sensitive() {
    let mut scope = Namer::program_scope();
    assert_eq!(scope.claim("l_x"), "l_x");
    assert_eq!(scope.claim("l_x"), "l_x_1");
    assert_eq!(scope.claim("l_X"), "l_X");
}
