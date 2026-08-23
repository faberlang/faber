//! AFM-1 (air-first-match) drivers + baseline pins for the dedicated
//! first-match expression head.
//!
//! AFM-2 chose the terminals (goal OQ1 default): the dedicated head is
//! `primus_quem` and the owned predicate tail is `ubi binder { predicate }` —
//! distinct from `rumpe`/`si` and from the reduce/scan `(fixum|varia)` tails.
//! The parse-admit drivers below are green since AFM-2; the behavior pins in
//! `radix-semantic/src/passes/first_match_contract_test.rs` stay red until
//! AFM-3 (base lane) and AFM-4 (AIR lane) land the admission rules.

use crate::test_support::{parse_ok, parse_program};

// ---------------------------------------------------------------------------
// Admit drivers — the dedicated first-match expression head parses into its
// own AST node (green since AFM-2; were red at the parse level before it).
// ---------------------------------------------------------------------------

#[test]
fn first_match_head_parses_into_a_dedicated_expression() {
    // Contract row: dedicated expression-valued first-match head over a
    // statically bounded source with an owned predicate tail.
    parse_ok(
        "functio t(tf32[4] a) → fractus ∪ nihil { redde primus_quem(a, ubi x { x > 0.0 }) }",
    );
}

#[test]
fn first_match_head_supports_filum_bounded_sources() {
    // Contract row: bounded-source routes include per-axis (`apud [i]`)
    // filum forms, mirroring the `summa ex … apud …` shape.
    parse_ok(
        "functio t(tf32[2, 2] a) → fractus ∪ nihil { redde primus_quem(a apud [i], ubi x { x > 0.0 }) }",
    );
}

// ---------------------------------------------------------------------------
// Baseline pins — grammar facts the new surface must not disturb.
// ---------------------------------------------------------------------------

#[test]
fn first_match_head_without_parens_stays_an_ordinary_identifier() {
    // The head spelling is contextual: it claims only when immediately
    // followed by `(`. Elsewhere `primus_quem` remains an ordinary
    // identifier — here a parameter name read as a value (SFR-1 `summa`
    // disambiguation precedent). Binding-target position keeps the
    // standing registry-keyword policy shared by every contextual word.
    parse_ok("functio t(numerus primus_quem) → numerus { redde primus_quem }");
}

#[test]
fn first_match_head_rejects_a_shared_reduce_tail() {
    // Owned-tail law (goal OQ1): the predicate tail must be `ubi`, never the
    // reduce/scan `(fixum|varia) binder block` tail. A `fixum` tail
    // parse-rejects inside the first-match head.
    let result = parse_program(
        "functio t(tf32[4] a) → fractus ∪ nihil { redde primus_quem(a, fixum x { x > 0.0 }) }",
    );
    assert!(
        !result.success(),
        "first-match head must reject the shared reduce tail; use the owned 'ubi' tail"
    );
}

#[test]
fn primus_method_receiver_stays_ordinary() {
    // `primus` is the existing Lista intrinsic method (`registry.rs` Lista
    // row). The dedicated head must not capture or retire this ordinary
    // method spelling (SFR-1 `summa` disambiguation precedent).
    parse_ok("functio t(tf32[4] a) → fractus ∪ nihil { redde a.primus() }");
}

#[test]
fn nullable_union_return_type_parses_today() {
    // The `T ∪ nihil` result shape is already grammar (EBNF: `T ∪ nihil` is
    // the canonical nullable form). Pinned so the admit drivers above are
    // attributable to the head spelling, not the result type.
    parse_ok("functio t(numerus a) → numerus ∪ nihil { redde nihil }");
}
