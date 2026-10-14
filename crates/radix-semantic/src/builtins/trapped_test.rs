//! F9D8A611: the builtin generic genus `trapped<R, T>`.
//!
//! `trapped<R, T>` resolves as a two-variant generic union carrying the
//! `trap { … }` outcome. This unit proves the type application resolves; the
//! `trap` expression itself lands in later units.

use crate::test_support::assert_accepts;

#[test]
fn trapped_resolves_with_two_type_arguments() {
    assert_accepts("functio f(trapped<numerus, textus> t) → numerus {\n    redde 0\n}\n");
}
