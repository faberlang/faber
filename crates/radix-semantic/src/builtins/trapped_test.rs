//! F9D8A611: the builtin generic genus `trapped<R, T>`.
//!
//! `trapped<R, T>` resolves as a two-variant generic union carrying the
//! `trap { … }` outcome. These tests pin how the builtin's variants match:
//! coverage over the two variants, and contextual (unreserved) variant names.
//! The `trap` expression itself lands in later units.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

#[test]
fn builtin_registration_keeps_user_ids_at_base() {
    let mut types = crate::TypeTable::new();
    let mut resolver = crate::Resolver::new();
    let mut interner = radix_lexer::Interner::new();
    crate::builtins::register(&mut resolver, &mut types, &mut interner);
    assert_eq!(
        resolver.fresh_def_id(),
        crate::DefId(crate::scope::USER_DEF_ID_BASE),
        "builtin registration must not consume user DefIds"
    );
}

#[test]
fn trapped_resolves_with_two_type_arguments() {
    assert_accepts("functio f(trapped<numerus, textus> t) → numerus {\n    redde 0\n}\n");
}

#[test]
fn trapped_match_is_coverage_checked() {
    assert_rejects_with_issue(
        "functio f(trapped<numerus, textus> t) → numerus {\n    discerne t {\n        casu returned fixum value { redde 0 }\n    }\n    redde 0\n}\n",
        "missing_variants",
    );
    assert_accepts(
        "functio f(trapped<numerus, textus> t) → numerus {\n    discerne t {\n        casu returned fixum value { redde 0 }\n        casu thrown fixum error { redde 0 }\n    }\n}\n",
    );
}

// DEBT(f9d8a611-1.2): the arm-binding payload-type test is deferred. Pattern
// payload bindings over a generic `discretio` (and this builtin genus) are
// bound to the declared field's raw `Type::Param` by
// `passes::typecheck::pattern::check_pattern`, which never substitutes the
// scrutinee's applied witnesses. The same source fails identically with a user
// `discretio Cista<T> { Plena { T res } }`, so this is a typecheck-pass gap,
// not `trapped` wiring, and it is outside this unit's write scope.

#[test]
fn trapped_variant_names_stay_contextual() {
    assert_accepts(
        "functio f() → numerus {\n    fixum numerus returned ← 1\n    redde returned\n}\n",
    );
}
