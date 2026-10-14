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
    let result = test_support::analyze_ok_la(&format!(
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
    let _ = test_support::analyze_ok_la(&format!(
        "{FAILABLE}functio g() → numerus {{\n    redde f(1) ⊥ 0\n}}\n"
    ));
}

#[test]
fn call_default_on_method_call_checks() {
    let _ = test_support::analyze_ok_la(concat!(
        "genus Fons {\n",
        "    functio lege(numerus x) → numerus ⇥ textus {\n        redde x\n    }\n",
        "}\n\n",
        "functio g(Fons s) → numerus {\n    redde s.lege(1) ⊥ 0\n}\n",
    ));
}

#[test]
fn call_default_type_must_match_the_success_type() {
    test_support::assert_rejects_with_issue_la(
        &format!("{FAILABLE}incipit {{\n    fixum _ n ← f(1) ⊥ \"zero\"\n    nota n\n}}\n"),
        "call_default_type_mismatch",
    );
}

#[test]
fn call_default_on_an_infallible_call_is_rejected() {
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_rejects_with_issue_la(
        &format!("{FAILABLE}functio g() → numerus {{\n    redde f(1) ⊥ f(2)\n}}\n"),
        "failable_call_requires_handler",
    );
}

// --- `vel` (coalesce) on a failable call: use `⊥` -----------------------------

#[test]
fn vel_on_a_failable_call_is_rejected_toward_the_call_default() {
    test_support::assert_rejects_with_issue_la(
        &format!("{FAILABLE}functio g() → numerus ⇥ textus {{\n    redde f(1) vel 0\n}}\n"),
        "coalesce_on_failable_call",
    );
}

#[test]
fn vel_on_a_failable_tensor_accipe_is_rejected_even_in_main() {
    test_support::assert_rejects_with_issue_la(
        concat!(
            "functio idx() → numerus {\n    redde 2\n}\n\n",
            "incipit {\n    varia tensor<f32, [4]> t ← vacua\n    fixum numerus i ← idx()\n",
            "    nota t.accipe([i]) vel 0.0\n}\n",
        ),
        "coalesce_on_failable_call",
    );
}

#[test]
fn vel_on_an_infallible_or_optional_call_is_unchanged() {
    // An infallible call returning an optional keeps `vel`.
    let _ = test_support::analyze_ok_la(concat!(
        "functio g(numerus x) → numerus ∪ nihil {\n    redde x\n}\n\n",
        "functio h() → numerus {\n    redde g(1) vel 0\n}\n",
    ));
    // A list element read (`T ∪ nihil`, not failable) keeps `vel`.
    let _ = test_support::analyze_ok_la(concat!(
        "functio h(lista<numerus> xs) → numerus {\n    redde xs.accipe(1) vel 0\n}\n",
    ));
}

// --- zero-argument directional view methods ---------------------------------

#[test]
fn directional_view_method_rejects_a_written_argument() {
    // `fini` takes no arguments: the argument is still typechecked, then the
    // shared arity rejection yields the error type.
    test_support::assert_rejects_with_issue_la(
        concat!(
            "@ ad 'x:y'\n",
            "functio f(textus n) → numerus { redde 1 }\n\n",
            "incipit {\n",
            "    fixum _ s ← ad 'x:y' (\"a\")\n",
            "    fixum _ t ← s.tuus<numerus>()\n",
            "    nota t.fini(1)\n",
            "}\n",
        ),
        "method_accepts_no_arguments",
    );
}
