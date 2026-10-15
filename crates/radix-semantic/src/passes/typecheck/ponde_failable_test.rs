//! K11-a: Tensor `ponde` (the checked store) declares `⇥`, so a call whose
//! index is not proven in range needs a handler, a `⊥` default or an enclosing
//! `⇥` outside `main` (D17.8). A literal index proven within a static shape
//! cannot fail and needs none; a literal provably out of range is the
//! compile-time `index_provably_out_of_bounds`, as the index form gives.
//! Fixtures use latin spellings (bare source, no en lex pack).

use super::test_support;

const PARAMS: &str = "varia tensor<f32, [4]> t ← vacua\n";

#[test]
fn ponde_runtime_index_outside_main_without_a_handler_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        &format!("functio put(numerus i) → vacuum {{\n    {PARAMS}    t.ponde([i], 2.0)\n}}\n"),
        "failable_call_requires_handler",
    );
}

#[test]
fn ponde_runtime_index_in_main_may_abort_unhandled() {
    let _ = test_support::analyze_ok_la(&format!(
        "incipit {{\n    {PARAMS}    sit numerus i ← 1\n    t.ponde([i], 2.0)\n}}\n"
    ));
}

#[test]
fn ponde_runtime_index_in_a_function_declaring_error_exit_checks() {
    let _ = test_support::analyze_ok_la(&format!(
        "functio put(numerus i) → vacuum ⇥ textus {{\n    {PARAMS}    t.ponde([i], 2.0)\n}}\n"
    ));
}

#[test]
fn ponde_runtime_index_reaches_a_cape_handler() {
    let _ = test_support::analyze_ok_la(&format!(
        "functio put(numerus i) → vacuum {{\n    {PARAMS}    fac {{\n        t.ponde([i], 2.0)\n    }} cape err {{\n        nota err\n    }}\n}}\n"
    ));
}

#[test]
fn ponde_literal_in_range_needs_no_handler() {
    let _ = test_support::analyze_ok_la(&format!(
        "functio put() → vacuum {{\n    {PARAMS}    t.ponde([1], 2.0)\n    t.ponde([3], 2.0)\n}}\n"
    ));
}

#[test]
fn ponde_literal_in_range_on_a_rank_two_shape_needs_no_handler() {
    let _ = test_support::analyze_ok_la(
        "functio put() → vacuum {\n    varia tensor<f32, [2, 3]> t ← vacua\n    t.ponde([1, 2], 2.0)\n}\n",
    );
}

#[test]
fn ponde_literal_out_of_range_is_rejected_as_the_index_form_is() {
    test_support::assert_rejects_with_issue_la(
        &format!("functio put() → vacuum {{\n    {PARAMS}    t.ponde([4], 2.0)\n}}\n"),
        "index_provably_out_of_bounds",
    );
}

#[test]
fn ponde_literal_out_of_range_is_rejected_even_inside_main() {
    test_support::assert_rejects_with_issue_la(
        &format!("incipit {{\n    {PARAMS}    t.ponde([9], 2.0)\n}}\n"),
        "index_provably_out_of_bounds",
    );
}

#[test]
fn ponde_literal_out_of_range_does_not_also_demand_a_handler() {
    // The compile-time rejection replaces the handler requirement: a handler
    // cannot make a provably bad index acceptable.
    test_support::assert_rejects_with_issue_la(
        &format!(
            "functio put() → vacuum {{\n    {PARAMS}    fac {{\n        t.ponde([9], 2.0)\n    }} cape err {{\n        nota err\n    }}\n}}\n"
        ),
        "index_provably_out_of_bounds",
    );
}

#[test]
fn ponde_list_variable_index_stays_failable() {
    test_support::assert_rejects_with_issue_la(
        &format!(
            "functio put() → vacuum {{\n    {PARAMS}    sit lista<numerus> idx ← [1]\n    t.ponde(idx, 2.0)\n}}\n"
        ),
        "failable_call_requires_handler",
    );
}

#[test]
fn generic_failable_call_in_main_may_abort_unhandled() {
    let _ = test_support::analyze_ok_la(
        "functio lege<T>(textus p) → T ⇥ textus {\n    iace p\n}\nincipit {\n    nota lege<textus>(\"x\")\n}\n",
    );
}

#[test]
fn generic_failable_call_in_helper_without_handler_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        "functio lege<T>(textus p) → T ⇥ textus {\n    iace p\n}\nfunctio help() → vacuum {\n    nota lege<textus>(\"x\")\n}\n",
        "failable_call_requires_handler",
    );
}
