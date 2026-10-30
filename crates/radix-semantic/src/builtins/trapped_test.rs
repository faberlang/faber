//! F9D8A611: the builtin generic genus `trapped<R, T>`.
//!
//! `trapped<R, T>` resolves as a two-variant generic union carrying the
//! `trap { … }` outcome. These tests pin how the builtin's variants match:
//! coverage over the two variants, payload bindings at the applied witnesses,
//! and contextual (unreserved) variant names. The `trap` expression re-typing
//! itself lives in `passes::typecheck::trap_test`.

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
        "functio f(trapped<numerus, textus> t) → numerus {\n    discerne t {\n        casu returned sit value { redde 0 }\n    }\n    redde 0\n}\n",
        "missing_variants",
    );
    assert_accepts(
        "functio f(trapped<numerus, textus> t) → numerus {\n    discerne t {\n        casu returned sit value { redde 0 }\n        casu thrown sit error { redde 0 }\n    }\n}\n",
    );
}

// The arm-binding payload-type gap that deferred a test here is closed:
// `check_pattern` substitutes the scrutinee's applied witnesses into the
// variant payload fields (U-D1 pattern twin), so `casu returned sit value`
// over `trapped<numerus, textus>` binds `value : numerus`.

#[test]
fn trapped_variant_arms_bind_applied_payload_witnesses() {
    // The payload bindings carry the application's witnesses, never the
    // declared `R`/`T` parameters: `value` is a `numerus` and `error` a
    // `textus` at this application, so each arm body may use them as such.
    assert_accepts(
        "functio f(trapped<numerus, textus> t) → numerus {\n    discerne t {\n        casu returned sit value { redde value + 1 }\n        casu thrown sit error { redde error.longitudo() }\n    }\n}\n",
    );
    // And the witness is real: a textus-only member access inside the
    // `returned` arm is a typed error rather than an open hole that defers
    // to MIR validation.
    assert_rejects_with_issue(
        "functio f(trapped<numerus, textus> t) → vacuum {\n    discerne t {\n        casu returned sit value { nota value.longitudo() }\n        casu thrown sit error { nota error }\n    }\n}\n",
        "unknown_method",
    );
}

#[test]
fn trapped_variant_names_stay_contextual() {
    assert_accepts(
        "functio f() → numerus {\n    sit numerus returned ← 1\n    redde returned\n}\n",
    );
}
