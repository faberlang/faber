//! Failable list/text `at` (`apud`) and `slice` (`sectio`) discharge their
//! handler requirement (SEM011, `failable_call_requires_handler`) only when the
//! checker's interval facts prove the call stays inside its receiver (need
//! 5d3a8afd, slice 5). `at` needs `0 ≤ i < len`; `slice` needs the whole window
//! `0 ≤ start ≤ end ≤ len`, the empty window at the end included. Everything
//! unproven keeps the handler requirement with the same diagnostic.
//!
//! Fixtures use latin spellings (`itera ab`, `sit`, `varia`, `et`).

use crate::test_support::{analyze_source, assert_rejects_with_issue};

const NEEDS_HANDLER: &str = "failable_call_requires_handler";

fn accepted(source: &str) {
    let result = analyze_source(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

fn rejected(source: &str) {
    assert_rejects_with_issue(source, NEEDS_HANDLER);
}

/// A function over `params` whose body is `body`, returning the counter `n`.
fn int_fn(params: &str, body: &str) -> String {
    format!(
        "functio g(numerus a) → numerus {{ redde a }}\n\
         functio f({params}) → numerus {{\n    varia numerus n ← 0\n{body}\n    redde n\n}}"
    )
}

// ── at ────────────────────────────────────────────────────────────────────

#[test]
fn at_inside_a_range_loop_over_the_length_is_proven() {
    accepted(&int_fn(
        "lista<numerus> xs",
        "    itera ab 0‥xs.longitudo() sit i { n ← xs.apud(i) }",
    ));
}

#[test]
fn at_inside_a_list_coordinate_loop_is_proven() {
    accepted(&int_fn(
        "lista<numerus> xs",
        "    itera ex xs apud [i] sit v { n ← xs.apud(i) }",
    ));
}

#[test]
fn text_at_inside_a_range_loop_over_the_length_is_proven() {
    accepted(
        "functio f(textus s) → textus {\n    varia textus t ← \"\"\n    itera ab 0‥s.longitudo() sit i { t ← s.apud(i) }\n    redde t\n}",
    );
}

#[test]
fn at_behind_a_two_sided_guard_is_proven() {
    accepted(&int_fn(
        "lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() { n ← xs.apud(i) }",
    ));
}

#[test]
fn at_without_a_fact_keeps_the_handler() {
    rejected(&int_fn(
        "lista<numerus> xs, numerus i",
        "    n ← xs.apud(i)",
    ));
    rejected(&int_fn("lista<numerus> xs", "    n ← xs.apud(0)"));
}

#[test]
fn at_with_half_a_proof_keeps_the_handler() {
    // No lower bound.
    rejected(&int_fn(
        "lista<numerus> xs, numerus i",
        "    si i ≺ xs.longitudo() { n ← xs.apud(i) }",
    ));
    // No upper bound.
    rejected(&int_fn(
        "lista<numerus> xs, numerus i",
        "    si i ≥ 0 { n ← xs.apud(i) }",
    ));
    // The index may equal the length.
    rejected(&int_fn(
        "lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≤ xs.longitudo() { n ← xs.apud(i) }",
    ));
}

#[test]
fn at_one_past_the_length_keeps_the_handler() {
    rejected(&int_fn(
        "lista<numerus> xs",
        "    itera ab 0‥xs.longitudo() sit i { n ← xs.apud(i + 1) }",
    ));
}

#[test]
fn at_a_length_proven_for_another_list_keeps_the_handler() {
    rejected(&int_fn(
        "lista<numerus> xs, lista<numerus> ys",
        "    itera ab 0‥xs.longitudo() sit i { n ← ys.apud(i) }",
    ));
}

#[test]
fn a_provably_out_of_range_at_keeps_the_handler() {
    rejected(&int_fn(
        "lista<numerus> xs",
        "    itera ab 1…5 sit i { n ← xs.apud(i - 10) }",
    ));
}

#[test]
fn a_default_on_a_proven_at_is_still_accepted() {
    accepted(&int_fn(
        "lista<numerus> xs",
        "    itera ab 0‥xs.longitudo() sit i { n ← xs.apud(i) ⊥ 0 }",
    ));
}

#[test]
fn a_cape_handler_on_a_proven_at_is_still_accepted() {
    accepted(&int_fn(
        "lista<numerus> xs",
        "    itera ab 0‥xs.longitudo() sit i {\n        fac {\n            n ← xs.apud(i)\n        } cape err {\n            n ← 9\n        }\n    }",
    ));
}

#[test]
fn a_proven_at_in_a_function_with_an_error_channel_is_accepted() {
    accepted(
        "functio f(lista<numerus> xs) → numerus ⇥ textus {\n    varia numerus n ← 0\n    itera ab 0‥xs.longitudo() sit i { n ← xs.apud(i) }\n    redde n\n}",
    );
}

// ── mutable lists ─────────────────────────────────────────────────────────

#[test]
fn at_on_a_mutable_list_with_a_live_fact_is_proven() {
    accepted(&int_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() { n ← xs.apud(i) }",
    ));
}

#[test]
fn appende_before_the_access_kills_the_length_fact() {
    rejected(&int_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() {\n        xs.appende(1)\n        n ← xs.apud(i)\n    }",
    ));
}

#[test]
fn a_call_before_the_access_kills_the_length_fact() {
    rejected(&int_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() {\n        n ← g(1)\n        n ← xs.apud(i)\n    }",
    ));
}

#[test]
fn a_mutating_call_inside_the_loop_body_keeps_the_handler() {
    rejected(&int_fn(
        "varia lista<numerus> xs",
        "    itera ab 0‥xs.longitudo() sit i {\n        n ← xs.apud(i)\n        xs.detrahe()\n    }",
    ));
}

// ── slice ─────────────────────────────────────────────────────────────────

#[test]
fn the_unit_slice_loop_over_a_text_is_proven() {
    accepted(
        "functio f(textus s) → textus {\n    varia textus t ← \"\"\n    itera ab 0‥s.longitudo() sit i { t ← s.sectio(i, i + 1) }\n    redde t\n}",
    );
}

#[test]
fn the_unit_slice_loop_over_a_list_is_proven() {
    accepted(&int_fn(
        "lista<numerus> xs",
        "    itera ab 0‥xs.longitudo() sit i { n ← xs.sectio(i, i + 1).longitudo() }",
    ));
}

#[test]
fn the_empty_window_at_the_end_is_proven() {
    accepted("functio f(textus s) → textus {\n    redde s.sectio(s.longitudo(), s.longitudo())\n}");
}

#[test]
fn the_whole_and_the_tail_windows_are_proven() {
    accepted("functio f(textus s) → textus {\n    redde s.sectio(0, s.longitudo())\n}");
    accepted(&int_fn(
        "lista<numerus> xs",
        "    itera ab 0…xs.longitudo() sit i { n ← xs.sectio(i, xs.longitudo()).longitudo() }",
    ));
}

#[test]
fn a_literal_window_over_an_unknown_length_keeps_the_handler() {
    rejected("functio f(textus s) → textus {\n    redde s.sectio(0, 1)\n}");
}

#[test]
fn a_window_past_the_end_keeps_the_handler() {
    rejected("functio f(textus s) → textus {\n    redde s.sectio(0, s.longitudo() + 1)\n}");
    rejected(
        "functio f(textus s) → textus {\n    varia textus t ← \"\"\n    itera ab 0‥s.longitudo() sit i { t ← s.sectio(i, i + 2) }\n    redde t\n}",
    );
}

#[test]
fn a_window_that_may_start_before_zero_keeps_the_handler() {
    rejected(
        "functio f(textus s) → textus {\n    varia textus t ← \"\"\n    itera ab 0‥s.longitudo() sit i { t ← s.sectio(i - 1, i) }\n    redde t\n}",
    );
}

#[test]
fn a_reversed_window_keeps_the_handler() {
    rejected(
        "functio f(textus s) → textus {\n    varia textus t ← \"\"\n    itera ab 0‥s.longitudo() sit i { t ← s.sectio(i + 1, i) }\n    redde t\n}",
    );
    rejected("functio f(textus s) → textus {\n    redde s.sectio(s.longitudo(), 0)\n}");
    // `len - 1` is below zero for an empty text, so `0 ≤ len - 1` is unproven.
    rejected("functio f(textus s) → textus {\n    redde s.sectio(0, s.longitudo() - 1)\n}");
}

#[test]
fn a_variable_addend_proves_nothing() {
    rejected(&int_fn(
        "textus s, numerus b",
        "    itera ab 0‥s.longitudo() sit i { n ← s.sectio(i, i + b).longitudo() }",
    ));
}

#[test]
fn a_window_between_unrelated_bindings_keeps_the_handler() {
    rejected(&int_fn(
        "textus s, numerus a, numerus b",
        "    si a ≥ 0 et b ≥ 0 et a ≺ s.longitudo() et b ≺ s.longitudo() {\n        n ← s.sectio(a, b).longitudo()\n    }",
    ));
}

#[test]
fn unknown_endpoints_keep_the_handler() {
    rejected(&int_fn(
        "lista<numerus> xs, numerus a, numerus b",
        "    n ← xs.sectio(a, b).longitudo()",
    ));
}

#[test]
fn a_slice_on_a_mutable_list_after_appende_keeps_the_handler() {
    accepted(&int_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() { n ← xs.sectio(i, i + 1).longitudo() }",
    ));
    rejected(&int_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() {\n        xs.appende(1)\n        n ← xs.sectio(i, i + 1).longitudo()\n    }",
    ));
}

#[test]
fn a_slice_over_a_mutated_text_binding_keeps_the_handler() {
    rejected(
        "functio f(varia textus s) → textus {\n    varia textus t ← \"\"\n    itera ab 0‥s.longitudo() sit i {\n        s ← s\n        t ← s.sectio(i, i + 1)\n    }\n    redde t\n}",
    );
}

#[test]
fn a_provably_out_of_range_slice_keeps_the_handler() {
    rejected(&int_fn(
        "lista<numerus> xs",
        "    itera ab 5…9 sit i { n ← xs.sectio(i, i - 3).longitudo() }",
    ));
}

// ── the map read is unchanged ─────────────────────────────────────────────

#[test]
fn a_map_at_keeps_the_handler_whatever_the_key() {
    rejected(
        "functio f(tabula<numerus, numerus> m) → numerus {\n    varia numerus n ← 0\n    itera ab 0‥3 sit i { n ← m.apud(i) }\n    redde n\n}",
    );
}
