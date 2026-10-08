//! K11-b: the failable tensor intrinsics `accipe` (checked read) and `ponde`
//! (checked store) report on `⇥ textus`. Inside a function that declares a
//! typed `⇥ E` they follow the user-function CALL behaviour in three layers:
//!
//! 1. no handler and no `@ conversio` text → `E`: `failable_call_conversion_absent`;
//! 2. a `@ conversio` row from text to `E`: it type-checks (the index failure
//!    arrives as the conversion's case);
//! 3. the row AND a spot handler (`⊥` default for `accipe`, which yields a
//!    value; `fac`/`cape` for both — a vacuum `ponde` has no default value):
//!    it type-checks, the spot wins.
//!
//! Fixtures use latin spellings (bare source, no en lex pack).

use super::test_support;

const ERROR_TYPE: &str = "discretio Erratum { Indicis }\n";
const CONVERSION: &str = "\
@ conversio
functio ex_textus(textus err) → Erratum { redde finge Indicis {} ∷ Erratum }
";
const TENSOR: &str = "varia tensor<f32, [4]> t ← vacua\n";

fn read(body: &str) -> String {
    format!("functio lege(numerus i) → fractus ⇥ Erratum {{\n    {TENSOR}    {body}\n}}\n")
}

fn store(body: &str) -> String {
    format!("functio pone(numerus i) → vacuum ⇥ Erratum {{\n    {TENSOR}    {body}\n}}\n")
}

fn without_conversion(function: &str) -> String {
    format!("{ERROR_TYPE}{function}")
}

fn with_conversion(function: &str) -> String {
    format!("{ERROR_TYPE}{CONVERSION}{function}")
}

// --- layer 1: no handler, no conversion -----------------------------------

#[test]
fn accipe_in_a_typed_error_function_without_a_conversion_is_rejected() {
    test_support::assert_rejects_with_issue(
        &without_conversion(&read("redde t.accipe([i])")),
        "failable_call_conversion_absent",
    );
}

#[test]
fn ponde_in_a_typed_error_function_without_a_conversion_is_rejected() {
    test_support::assert_rejects_with_issue(
        &without_conversion(&store("t.ponde([i], 2.0)")),
        "failable_call_conversion_absent",
    );
}

// --- layer 2: a text → E conversion row ------------------------------------

#[test]
fn accipe_in_a_typed_error_function_with_a_conversion_checks() {
    let _ = test_support::analyze_ok(&with_conversion(&read("redde t.accipe([i])")));
}

#[test]
fn ponde_in_a_typed_error_function_with_a_conversion_checks() {
    let _ = test_support::analyze_ok(&with_conversion(&store("t.ponde([i], 2.0)")));
}

// --- layer 3: the row and a spot handler; the spot wins ---------------------

#[test]
fn accipe_default_at_the_spot_checks_with_a_conversion_declared() {
    let _ = test_support::analyze_ok(&with_conversion(&read("redde t.accipe([i]) ⊥ 0.0")));
}

#[test]
fn accipe_default_at_the_spot_checks_without_a_conversion() {
    let _ = test_support::analyze_ok(&without_conversion(&read("redde t.accipe([i]) ⊥ 0.0")));
}

#[test]
fn ponde_cape_at_the_spot_checks_with_a_conversion_declared() {
    let _ = test_support::analyze_ok(&with_conversion(&store(
        "fac {\n        t.ponde([i], 2.0)\n    } cape err {\n        nota err\n    }",
    )));
}

#[test]
fn accipe_cape_at_the_spot_checks_without_a_conversion() {
    let _ = test_support::analyze_ok(&without_conversion(&read(
        "fac {\n        nota t.accipe([i])\n    } cape err {\n        nota err\n    }\n    redde 0.0",
    )));
}

// --- accipe mirrors ponde's failability rule (K11-b) ------------------------

#[test]
fn accipe_runtime_index_outside_main_without_a_handler_is_rejected() {
    test_support::assert_rejects_with_issue(
        &format!("functio lege(numerus i) → fractus {{\n    {TENSOR}    redde t.accipe([i])\n}}\n"),
        "failable_call_requires_handler",
    );
}

#[test]
fn accipe_literal_in_range_needs_no_handler_and_rejects_a_default() {
    let _ = test_support::analyze_ok(&format!(
        "functio lege() → fractus {{\n    {TENSOR}    redde t.accipe([3])\n}}\n"
    ));
    test_support::assert_rejects_with_issue(
        &format!("functio lege() → fractus {{\n    {TENSOR}    redde t.accipe([3]) ⊥ 0.0\n}}\n"),
        "call_default_on_infallible_call",
    );
}

#[test]
fn accipe_literal_out_of_range_is_rejected_once_at_compile_time() {
    test_support::assert_rejects_with_issue(
        &format!("functio lege() → fractus {{\n    {TENSOR}    redde t.accipe([4])\n}}\n"),
        "index_provably_out_of_bounds",
    );
}
