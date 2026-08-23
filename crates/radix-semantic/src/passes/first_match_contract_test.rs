//! AFM-1 (air-first-match) red drivers: first-match observable contract.
//!
//! These drivers pin the TARGET contract of the dedicated first-match head
//! (delivery `radix/docs/factory/air-first-match/delivery.md` AFM-1): total
//! evaluation, `T ∪ nihil` for no-match and empty input, predicate-error
//! propagation, AIR admission, and the `filtrata(pred).primus()` not-AIR
//! differentiation pin.
//!
//! The head spelling `primus_quem(<source>, fixum <binder> { <predicate> })` is a PLACEHOLDER
//! (terminal choice is AFM-2's). Until AFM-2 lands, the placeholder-driving
//! tests fail at the parse level — the pinned red reason. The
//! `filtrata+primus` composition drivers pin live composition facts and do
//! not depend on the placeholder spelling.

use crate::{PassConfig, SemanticError, SemanticErrorKind};
use radix_target::Target;

fn analyze(source: &str) -> Vec<SemanticError> {
    let lex = radix_lexer::lex(source);
    let mut parse = radix_parser::parse(lex);
    assert!(
        parse.success(),
        "AFM-1 pinned red: the placeholder first-match head spelling must \
         parse once AFM-2 lands the dedicated expression head; parse errors: \
         {:?}",
        parse.errors
    );
    let program = parse
        .program
        .expect("successful parse yields a program");
    let config = PassConfig::for_target(Target::MirWasm);
    crate::analyze(&program, &config, &mut parse.interner).diagnostics
}

fn air_errors(source: &str) -> Vec<SemanticError> {
    analyze(source)
        .into_iter()
        .filter(|err| err.kind == SemanticErrorKind::AirPurityViolation)
        .collect()
}

// ---------------------------------------------------------------------------
// Red drivers — placeholder-spelling behavior pins. RED today at the parse
// level (spelling admitted by AFM-2); AFM-3 must make each pass on the base
// lane, AFM-4 on the AIR lane.
// ---------------------------------------------------------------------------

#[test]
fn first_match_total_evaluation_observes_post_match_predicate_error() {
    // Contract row: the predicate is evaluated for EVERY candidate lane,
    // including lanes after the first live match (post-match trap is
    // observable) — dataflow first-live selection, no early exit.
    // RED today: parse-level rejection of the placeholder head spelling.
    let errors = analyze(
        r#"
functio t(tf32[4] a) → fractus ∪ nihil {
    redde primus_quem(a, fixum x { (x ut u8) > 0 })
}
"#,
    );
    assert!(errors.is_empty(), "unexpected diagnostics: {errors:?}");
}

#[test]
fn first_match_no_match_yields_nihil() {
    // Contract row: no live match → `nihil`, never an error.
    // RED today: parse-level rejection of the placeholder head spelling.
    let errors = analyze(
        r#"
functio t(tf32[4] a) → fractus ∪ nihil {
    redde primus_quem(a, fixum x { x < 0.0 })
}
"#,
    );
    assert!(errors.is_empty(), "unexpected diagnostics: {errors:?}");
}

#[test]
fn first_match_empty_source_yields_nihil() {
    // Contract row: empty (statically zero-length) source → `nihil`.
    // RED today: parse-level rejection of the placeholder head spelling.
    let errors = analyze(
        r#"
functio t(tf32[0] a) → fractus ∪ nihil {
    redde primus_quem(a, fixum x { x > 0.0 })
}
"#,
    );
    assert!(errors.is_empty(), "unexpected diagnostics: {errors:?}");
}

#[test]
fn first_match_predicate_error_propagates_never_becomes_nihil() {
    // Contract row: a predicate ERROR rides the error channel; it is never
    // converted to a `nihil` result. The nullable result covers only
    // no-match/empty, not predicate failure.
    // RED today: parse-level rejection of the placeholder head spelling.
    let errors = analyze(
        r#"
functio t(tf32[4] a) → fractus ∪ nihil {
    redde primus_quem(a, fixum x { (x ut u8) > 100 })
}
"#,
    );
    assert!(errors.is_empty(), "unexpected diagnostics: {errors:?}");
}

#[test]
fn air_lane_admits_dedicated_first_match_head() {
    // Contract row: the dedicated head (with a named AIR predicate) is
    // AIR-admitted over a bounded tensor source with a first-live mask.
    // RED today: parse-level rejection of the placeholder head spelling.
    // (AFM-4 must make this pass without any method-allowlist row — the
    // head is not a method.)
    let errors = air_errors(
        r#"
@ radix lane "air"
functio keep(fractus x) → bivalens {
    redde x > 0.0
}

@ radix lane "air"
functio pick(tf32[2, 2] x) → fractus ∪ nihil {
    redde primus_quem(x, fixum v { keep(v) })
}
"#,
    );
    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

// ---------------------------------------------------------------------------
// Differentiation pin — the live comparator composition. This does NOT use
// the placeholder spelling: it pins the ruling-required fact (ruling
// `94786e02`) that `filtrata(pred).primus()` is NOT AIR-admitted
// end-to-end today (`filtrata` is allowlisted ARC-3; `.primus()` is not in
// the AIR method allowlist).
// ---------------------------------------------------------------------------

#[test]
fn filtrata_primus_composition_is_not_air_admitted_end_to_end() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio keep(fractus x) → bivalens {
    redde x > 0.0
}

@ radix lane "air"
functio pick(tf32[2, 2] x) → fractus ∪ nihil {
    redde x.filtrata(keep).primus()
}
"#,
    );
    assert!(
        errors.iter().any(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "method" && arg.value == "primus")
        }),
        "filtrata(pred).primus() must stay AIR-declined on the `.primus()` \
         method row (ruling 94786e02 differentiation); got: {errors:?}"
    );
}
