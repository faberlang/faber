//! `call ⊥ default` tests (D17.7).
//!
//! The call-site default channel supplies a value when a failable call fails
//! through its `⇥` channel. The expression has the call's success type; the
//! default must type-check as that type; `⊥` on a call that cannot fail is an
//! error. Fixtures use latin spellings (bare source, no en lex pack).

use super::test_support::{self, entry_local_tys};
use crate::types::{Primitive, Type};

const FAILABLE: &str = "functio f(numerus x) → numerus ⇥ textus {\n    redde x\n}\n\n";

#[test]
fn call_default_has_the_success_type() {
    let result = test_support::analyze_ok(&format!(
        "{FAILABLE}incipit {{\n    fixum _ n ← f(1) ⊥ 0\n    nota n\n}}\n"
    ));
    let tys = entry_local_tys(&result);
    let [local_ty] = tys[..] else {
        panic!("expected one incipit local, got {tys:?}");
    };
    assert!(matches!(
        result.types.get(local_ty),
        Type::Primitive(Primitive::Numerus)
    ));
}

#[test]
fn call_default_satisfies_the_handler_rule_outside_a_failable_function() {
    // The default is the handler: no `⇥`, `fac/cape` or `capta` needed.
    let _ = test_support::analyze_ok(&format!(
        "{FAILABLE}functio g() → numerus {{\n    redde f(1) ⊥ 0\n}}\n"
    ));
}

#[test]
fn call_default_on_method_call_checks() {
    let _ = test_support::analyze_ok(concat!(
        "genus Fons {\n",
        "    functio lege(numerus x) → numerus ⇥ textus {\n        redde x\n    }\n",
        "}\n\n",
        "functio g(Fons s) → numerus {\n    redde s.lege(1) ⊥ 0\n}\n",
    ));
}

#[test]
fn call_default_type_must_match_the_success_type() {
    test_support::assert_rejects_with_issue(
        &format!("{FAILABLE}incipit {{\n    fixum _ n ← f(1) ⊥ \"zero\"\n    nota n\n}}\n"),
        "call_default_type_mismatch",
    );
}

#[test]
fn call_default_on_an_infallible_call_is_rejected() {
    test_support::assert_rejects_with_issue(
        concat!(
            "functio g() → numerus {\n    redde 1\n}\n\n",
            "incipit {\n    fixum _ n ← g() ⊥ 0\n    nota n\n}\n",
        ),
        "call_default_on_infallible_call",
    );
}

#[test]
fn call_default_does_not_contain_a_failable_default() {
    // The default is evaluated outside the boundary: a failable call there
    // still needs its own handler.
    test_support::assert_rejects_with_issue(
        &format!("{FAILABLE}functio g() → numerus {{\n    redde f(1) ⊥ f(2)\n}}\n"),
        "failable_call_requires_handler",
    );
}
