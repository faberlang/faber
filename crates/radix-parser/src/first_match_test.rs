//! AFM-1 (air-first-match) red drivers + baseline pins for the dedicated
//! first-match expression head.
//!
//! The head spelling used here (`primus_quem(<source>, fixum <binder> { <predicate> })`) is a
//! PLACEHOLDER — terminal choice is AFM-2's (goal OQ1 default: one dedicated
//! head + one owned predicate tail, morphologia vocabulary, distinct from
//! `rumpe`/`si`/reduce-scan tails). AFM-2 inverts the reject-pin into an
//! admit and retargets the placeholder to the chosen terminals.

use crate::test_support::{parse_ok, parse_program};

// ---------------------------------------------------------------------------
// Red drivers — target contract, failing today at the parse level because
// no first-match head spelling exists in the grammar (delivery §3 baseline:
// no `first_match` in AST/HIR/parser/MirIntrinsic/EBNF).
// ---------------------------------------------------------------------------

#[test]
fn first_match_head_parses_into_a_dedicated_expression() {
    // Contract row: dedicated expression-valued first-match head over a
    // statically bounded source with an owned predicate tail.
    // RED today: the placeholder head spelling is parse-rejected (no such
    // production exists yet). AFM-2's terminal choice replaces the spelling.
    parse_ok("functio t(tf32[4] a) → fractus ∪ nihil { redde primus_quem(a, fixum x { x > 0.0 }) }");
}

#[test]
fn first_match_head_supports_filum_bounded_sources() {
    // Contract row: bounded-source routes include per-axis (`apud [i]`)
    // filum forms, mirroring the `summa ex … apud …` shape.
    // RED today: parse-level rejection of the placeholder head spelling.
    parse_ok(
        "functio t(tf32[2, 2] a) → fractus ∪ nihil { redde primus_quem(a apud [i], fixum x { x > 0.0 }) }",
    );
}

// ---------------------------------------------------------------------------
// Baseline pins — today's grammar facts the new surface must not disturb.
// ---------------------------------------------------------------------------

#[test]
fn first_match_placeholder_spelling_parse_rejects_today() {
    // AFM-1 pin: the placeholder head spelling is NOT in the grammar today.
    // This is the documented starting red for the admit drivers above and
    // the disambiguation anchor AFM-2 inverts.
    let result = parse_program(
        "functio t(tf32[4] a) → fractus ∪ nihil { redde primus_quem(a, fixum x { x > 0.0 }) }",
    );
    assert!(
        !result.success(),
        "placeholder first-match head must parse-reject before AFM-2"
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
    // the canonical nullable form). Pinned so the admit-driver reds above
    // are attributable to the missing head spelling, not the result type.
    parse_ok("functio t(numerus a) → numerus ∪ nihil { redde nihil }");
}
