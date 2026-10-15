//! One const/var rule for field writes and mutating method calls.
//!
//! A class method that changes its receiver in place (directly, through a
//! sibling method, or through an in-place collection verb on a field) is a
//! write to the receiver binding. An immutable binding (`fixum` local or a
//! non-`in` parameter, `copy` parameters included) rejects it exactly as it
//! rejects a field write; a `varia` local and an `in` parameter accept both.
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support::{analyze_ok, assert_rejects_with_issue};

const MUTATING_METHOD_ISSUE: &str = "imperativus_operator_method_requires_varia_receiver";

const COUNTER: &str = r"
genus Contador {
    varia numerus n
    functio cresce() → vacuum { ego.n ← ego.n + 1 }
    functio bis() → vacuum {
        ego.cresce()
        ego.cresce()
    }
    functio lege() → numerus { redde ego.n }
}
";

fn with_counter(rest: &str) -> String {
    format!("{COUNTER}{rest}")
}

#[test]
fn const_binding_rejects_mutating_method_call() {
    assert_rejects_with_issue(
        &with_counter("incipit {\n sit Contador k ← Contador { n = 1 }\n k.cresce()\n}\n"),
        MUTATING_METHOD_ISSUE,
    );
}

#[test]
fn const_binding_rejects_method_that_mutates_through_a_sibling() {
    assert_rejects_with_issue(
        &with_counter("incipit {\n sit Contador k ← Contador { n = 1 }\n k.bis()\n}\n"),
        MUTATING_METHOD_ISSUE,
    );
}

#[test]
fn plain_parameter_rejects_mutating_method_call() {
    assert_rejects_with_issue(
        &with_counter("functio f(Contador c) → vacuum { c.cresce() }\nincipit { }\n"),
        MUTATING_METHOD_ISSUE,
    );
}

#[test]
fn plain_parameter_rejects_field_write() {
    assert_rejects_with_issue(
        &with_counter("functio f(Contador c) → vacuum { c.n ← 1 }\nincipit { }\n"),
        "assign_to_de_parameter",
    );
}

#[test]
fn copy_parameter_accepts_mutating_method_call_and_field_write() {
    // OWN-A (2026-10-08): `copy` and `own` parameters write the callee's own
    // value, so the call and the field store are legal.
    let _ = analyze_ok(&with_counter(
        "functio f(copy Contador c) → vacuum { c.cresce() }\nincipit { }\n",
    ));
    let _ = analyze_ok(&with_counter(
        "functio f(copy Contador c) → vacuum { c.n ← 1 }\nincipit { }\n",
    ));
}

#[test]
fn var_binding_accepts_mutating_method_call_and_field_write() {
    let _ = analyze_ok(&with_counter(
        "incipit {\n varia Contador k ← Contador { n = 1 }\n k.cresce()\n k.n ← 5\n}\n",
    ));
}

#[test]
fn copy_parameter_rebound_to_var_accepts_mutation() {
    let _ = analyze_ok(&with_counter(
        "functio f(copy Contador c) → numerus {\n varia Contador mine ← c\n mine.cresce()\n redde mine.lege()\n}\nincipit { }\n",
    ));
}

#[test]
fn mut_parameter_accepts_mutating_method_call() {
    let _ = analyze_ok(&with_counter(
        "functio f(in Contador c) → vacuum { c.cresce() }\nincipit { }\n",
    ));
}

#[test]
fn const_binding_accepts_non_mutating_method_call() {
    let _ = analyze_ok(&with_counter(
        "incipit {\n sit Contador k ← Contador { n = 1 }\n nota k.lege()\n}\n",
    ));
}

#[test]
fn method_that_mutates_a_field_collection_in_place_is_mutating() {
    assert_rejects_with_issue(
        r"
genus Sacculus {
    varia lista<numerus> xs
    functio adde(numerus x) → vacuum { ego.xs.appende(x) }
}
incipit {
    sit Sacculus s ← Sacculus { xs = [] }
    s.adde(1)
}
",
        MUTATING_METHOD_ISSUE,
    );
}
