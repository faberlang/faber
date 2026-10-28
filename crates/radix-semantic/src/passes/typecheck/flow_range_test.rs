//! Interval flow facts (need, slices 1-2): the fact value, branch
//! scoping, short-circuit composition, the normal-edge hull, symbolic bound
//! identity and the closed invalidation rule.
//!
//! Nothing in the checker produces interval facts yet. The source-level tests
//! drive the machinery through the test-only probe in `flow_range_probe.rs`
//! (producer `x <op> bound`, observer = the interval held at each read of a
//! binding). Fixtures use latin spellings because the unit-test harness
//! compiles bare source without the en-locale lex pack.

use super::flow_range::{AxisIdentity, BoundKey, IntRange, RangeFlow, axis_bound};
use super::flow_range_probe::{clear_reads, reads_of};
use crate::test_support;
use crate::{IndexExpr, IndexInferState, RuntimeExtentId, TypeTable};
use radix_hir::DefId;

fn c(value: i128) -> BoundKey {
    BoundKey::Const(value)
}

fn below(value: i128) -> IntRange {
    IntRange::from_hi(c(value), true)
}

fn at_least(value: i128) -> IntRange {
    IntRange::from_lo(c(value), false)
}

fn above(value: i128) -> IntRange {
    IntRange::from_lo(c(value), true)
}

/// `< value` as a guard on an integer produces it: the adjacent non-strict
/// integer.
fn lt(value: i128) -> IntRange {
    IntRange::from_hi(c(value - 1), false)
}

/// `[lo, hi)` as guards on an integer produce it.
fn band(lo: i128, hi: i128) -> IntRange {
    at_least(lo).meet(&lt(hi))
}

/// Check `source`, require it to be accepted, and return the interval held at
/// each read of `name`, in source order.
fn reads(source: &str, name: &str) -> Vec<Option<IntRange>> {
    clear_reads();
    let result = test_support::analyze_source(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    reads_of(name)
}

fn int_fn(body: &str) -> String {
    format!(
        "functio g(numerus a) → numerus {{ redde a }}\n\
         functio f(numerus x) → numerus {{\n    varia numerus n ← 0\n{body}\n    redde n\n}}"
    )
}

// ── slice 1: then/else scoping ────────────────────────────────────────────

#[test]
fn then_and_else_edges_carry_opposite_bounds() {
    let source = int_fn("    si x ≺ 10 { n ← x } secus { n ← x }\n    n ← x");
    assert_eq!(
        reads(&source, "x"),
        vec![None, Some(lt(10)), Some(at_least(10)), None],
        "the facts neither cross into the other edge nor escape the join"
    );
}

#[test]
fn a_fact_does_not_escape_a_branch_without_else() {
    let source = int_fn("    si x ≺ 10 { n ← x }\n    n ← x");
    assert_eq!(reads(&source, "x"), vec![None, Some(lt(10)), None]);
}

#[test]
fn a_diverging_edge_contributes_nothing_to_the_continuation() {
    let source = "functio f(numerus x) → numerus {\n    si x ≺ 10 { redde 0 }\n    redde x\n}";
    assert_eq!(reads(source, "x"), vec![None, Some(at_least(10))]);
}

// ── slice 1: short-circuit and / or / not ─────────────────────────────────

#[test]
fn and_holds_the_left_fact_while_the_right_conjunct_is_checked() {
    let source = int_fn("    si x ≥ 0 et x ≺ 10 { n ← x }\n    n ← x");
    assert_eq!(
        reads(&source, "x"),
        vec![None, Some(at_least(0)), Some(band(0, 10)), None],
        "right conjunct sees the left fact; the body sees both; neither escapes"
    );
}

#[test]
fn and_learns_nothing_on_the_else_edge() {
    let source = int_fn("    si x ≥ 0 et x ≺ 10 { n ← 1 } secus { n ← x }");
    assert_eq!(
        reads(&source, "x"),
        vec![None, Some(at_least(0)), None],
        "the falsity of a conjunction forces neither conjunct"
    );
}

#[test]
fn diverging_or_leaves_both_negations_for_the_rest_of_the_block() {
    let source =
        "functio f(numerus x) → numerus {\n    si x ≺ 0 aut x ≥ 10 { redde 0 }\n    redde x\n}";
    assert_eq!(
        reads(source, "x"),
        vec![None, None, Some(band(0, 10))],
        "`aut` has never narrowed its own right operand; the fallthrough has both"
    );
}

#[test]
fn not_swaps_the_edges() {
    let source = int_fn("    si non (x ≺ 10) { n ← x } secus { n ← x }");
    assert_eq!(
        reads(&source, "x"),
        vec![None, Some(at_least(10)), Some(lt(10))]
    );
}

// ── slice 1: normal-edge joins ────────────────────────────────────────────

#[test]
fn joined_edges_keep_the_hull_of_each_side() {
    let source =
        int_fn("    si x ≥ 0 {\n        si x ≺ 5 { n ← 1 } secus { n ← 2 }\n        n ← x\n    }");
    assert_eq!(
        reads(&source, "x"),
        vec![None, Some(at_least(0)), Some(at_least(0))],
        "inner then-edge is [0,5), else-edge is [5,∞): lower min(0,5)=0, upper \
         is bounded on one edge only so it is dropped"
    );
}

#[test]
fn edges_that_agree_keep_the_fact_after_the_join() {
    let source =
        int_fn("    si x ≺ 10 {\n        si x ≥ 0 { n ← 1 } secus { n ← 2 }\n        n ← x\n    }");
    assert_eq!(
        reads(&source, "x"),
        vec![None, Some(lt(10)), Some(lt(10))],
        "inner edges are [0,10) and (-∞,0): the upper hull is the weaker `< 10`, \
         and the outer fact is on both edges"
    );
}

#[test]
fn a_rebind_inside_one_edge_drops_the_fact_at_the_join() {
    let source = "functio f(varia numerus x) → numerus {\n    varia numerus n ← 0\n    si x ≺ 10 {\n        si n ≺ 1 { x ← 3 } secus { n ← 1 }\n        n ← x\n    }\n    redde n\n}";
    let after_join = reads(source, "x").last().cloned().expect("a read");
    assert_eq!(after_join, None);
}

// ── slice 1: interval algebra ─────────────────────────────────────────────

#[test]
fn hull_takes_the_weaker_endpoint_and_drops_incomparable_sides() {
    // lower: weaker is the smaller value; equal values keep strictness only
    // when both are strict.
    assert_eq!(at_least(0).hull(&at_least(5)), Some(at_least(0)));
    assert_eq!(above(3).hull(&at_least(3)), Some(at_least(3)));
    assert_eq!(above(3).hull(&above(3)), Some(above(3)));
    // upper: weaker is the larger value.
    assert_eq!(below(4).hull(&below(9)), Some(below(9)));
    // a side bounded on one edge only is dropped; nothing left means no fact.
    assert_eq!(at_least(0).hull(&below(9)), None);
    let live = BoundKey::LiveLength {
        source: DefId(7),
        version: 0,
    };
    assert_eq!(
        IntRange::from_hi(live, true).hull(&below(9)),
        None,
        "a literal and a symbolic bound cannot be compared"
    );
}

#[test]
fn meet_keeps_the_stronger_endpoint() {
    assert_eq!(at_least(0).meet(&at_least(5)), at_least(5));
    assert_eq!(above(5).meet(&at_least(5)), above(5));
    assert_eq!(below(4).meet(&below(9)), below(4));
}

#[test]
fn interval_queries_follow_strictness() {
    let range = at_least(0).meet(&below(10));
    assert!(range.proves_at_least(&c(0), false));
    assert!(!range.proves_at_least(&c(0), true));
    assert!(range.proves_at_most(&c(10), true));
    assert!(range.proves_at_most(&c(11), true));
    assert!(!range.proves_at_most(&c(9), false));
}

// ── slice 2: stable bound identities ──────────────────────────────────────

fn axis(types: &mut TypeTable, infer: &mut IndexInferState, expr: IndexExpr) -> Option<BoundKey> {
    let id = types.intern_index(expr);
    axis_bound(types, infer, id)
}

#[test]
fn equal_axis_sources_give_equal_bounds_even_from_distinct_index_ids() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let width = radix_lexer::Symbol(1);
    let height = radix_lexer::Symbol(2);
    let first = axis(&mut types, &mut infer, IndexExpr::Param(width));
    let second = axis(&mut types, &mut infer, IndexExpr::Param(width));
    assert!(first.is_some());
    assert_eq!(first, second, "same size parameter, distinct arena ids");
    assert_ne!(
        first,
        axis(&mut types, &mut infer, IndexExpr::Param(height))
    );

    let origin =
        |binding| IndexExpr::Runtime(RuntimeExtentId::TensorBindingAxis { binding, axis: 0 });
    assert_eq!(
        axis(&mut types, &mut infer, origin(DefId(5))),
        axis(&mut types, &mut infer, origin(DefId(5)))
    );
    assert_ne!(
        axis(&mut types, &mut infer, origin(DefId(5))),
        axis(&mut types, &mut infer, origin(DefId(6))),
        "another tensor's axis is not this tensor's axis"
    );
    assert_ne!(
        axis(&mut types, &mut infer, origin(DefId(5))),
        axis(
            &mut types,
            &mut infer,
            IndexExpr::Runtime(RuntimeExtentId::TensorBindingAxis {
                binding: DefId(5),
                axis: 1
            })
        ),
        "another axis of the same tensor"
    );
}

#[test]
fn literal_and_affine_axes_fold_and_unresolved_holes_prove_nothing() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let seven = types.intern_index(IndexExpr::Literal(7));
    let shifted = axis(
        &mut types,
        &mut infer,
        IndexExpr::Affine {
            base: seven,
            offset: 1,
        },
    );
    assert_eq!(shifted, Some(c(8)));
    assert_eq!(axis(&mut types, &mut infer, IndexExpr::Literal(8)), shifted);

    let symbolic = types.intern_index(IndexExpr::Param(radix_lexer::Symbol(1)));
    let plus_one = axis(
        &mut types,
        &mut infer,
        IndexExpr::Affine {
            base: symbolic,
            offset: 1,
        },
    );
    assert!(matches!(
        plus_one,
        Some(BoundKey::Axis(AxisIdentity::Affine { offset: 1, .. }))
    ));
    assert_ne!(
        plus_one,
        axis(
            &mut types,
            &mut infer,
            IndexExpr::Param(radix_lexer::Symbol(1))
        ),
        "N + 1 is not N"
    );

    let hole = infer.fresh_hole(&mut types);
    assert_eq!(axis_bound(&mut types, &mut infer, hole), None);
    assert_eq!(axis(&mut types, &mut infer, IndexExpr::Unspecified), None);
}

#[test]
fn an_axis_bound_proves_an_access_against_the_same_extent_only() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let width = radix_lexer::Symbol(1);
    let bound = |types: &mut TypeTable, infer: &mut IndexInferState, name| {
        axis(types, infer, IndexExpr::Param(name)).expect("param axis")
    };
    let index_fact = IntRange::from_hi(bound(&mut types, &mut infer, width), true);
    assert!(index_fact.proves_at_most(&bound(&mut types, &mut infer, width), true));
    assert!(
        !index_fact.proves_at_most(&bound(&mut types, &mut infer, radix_lexer::Symbol(2)), true)
    );
}

#[test]
fn a_live_length_identity_is_tied_to_the_source_and_its_version() {
    let mut flow = RangeFlow::default();
    let s = DefId(100);
    let t = DefId(101);
    let first = flow.live_length(s);
    assert_eq!(first, flow.live_length(s));
    assert_ne!(first, flow.live_length(t), "another source");
    flow.drop_subject(s);
    assert_eq!(
        flow.live_length(s),
        first,
        "a store to a source that no fact is bounded by moves nothing"
    );
    let fact = IntRange::from_hi(first.clone(), true);
    flow.assume(DefId(1), &fact);
    flow.drop_subject(s);
    assert_ne!(
        flow.live_length(s),
        first,
        "the old length identity is gone"
    );
    assert!(flow.get(DefId(1)).is_none(), "the fact bounded by it died");
}

#[test]
fn a_stale_length_never_survives_a_kill_even_across_a_restore_join() {
    let mut flow = RangeFlow::default();
    let s = DefId(100);
    let x = DefId(1);
    flow.assume(x, &IntRange::from_hi(flow.live_length(s), true));
    let incoming = flow.snapshot();
    // One edge mutates `s` (kill), the other does not.
    flow.kill_mutable_sources(|_| true);
    let mutated = flow.snapshot();
    flow.restore(&incoming);
    let untouched = flow.snapshot();
    let joined = flow.join(&incoming, &[&mutated, &untouched]);
    flow.restore(&joined);
    assert!(flow.get(x).is_none(), "the mutated edge has no fact");
    assert_ne!(
        flow.live_length(s),
        incoming_length(&incoming, s),
        "the versions differed across edges, so the join minted a fresh one"
    );
}

fn incoming_length(snapshot: &super::flow_range::RangeSnapshot, source: DefId) -> BoundKey {
    let mut flow = RangeFlow::default();
    flow.restore(snapshot);
    flow.live_length(source)
}

// ── slice 2: invalidation (closed rule) ───────────────────────────────────

const LIST: &str = "functio g(numerus a) → numerus { redde a }\n\
functio h(varia lista<numerus> l) → vacuum { l.appende(1) }\n\
functio eat(own lista<numerus> l) → vacuum { nota l.longitudo() }\n\
functio peek(lista<numerus> l) → vacuum { nota l.longitudo() }\n";

fn list_fn(body: &str) -> String {
    format!(
        "{LIST}functio f(varia lista<numerus> s, numerus x) → numerus {{\n    varia numerus n ← 0\n{body}\n    redde n\n}}"
    )
}

fn is_live_length_bound(range: &Option<IntRange>) -> bool {
    matches!(
        range,
        Some(IntRange {
            hi: Some(super::flow_range::Endpoint {
                key: BoundKey::LiveLength { .. },
                strict: true
            }),
            lo: None
        })
    )
}

fn list_fn_with(params: &str, body: &str) -> String {
    format!(
        "{LIST}functio f({params}, numerus x) → numerus {{\n    varia numerus n ← 0\n{body}\n    redde n\n}}"
    )
}

/// `effect` runs between two reads of `x` inside `si x ≺ s.longitudo()`: the
/// first read must see the live-length bound, the second must see nothing.
fn assert_effect_kills_length_bound(params: &str, effect: &str) {
    let source = list_fn_with(
        params,
        &format!(
            "    si x ≺ s.longitudo() {{\n        n ← x\n        {effect}\n        n ← x\n    }}"
        ),
    );
    let seen = reads(&source, "x");
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert!(
        is_live_length_bound(&seen[1]),
        "before the effect: {seen:?}"
    );
    assert_eq!(seen[2], None, "after `{effect}`: {seen:?}");
}

#[test]
fn a_mutable_lists_length_bound_is_usable_until_the_list_is_touched() {
    let source = list_fn("    si x ≺ s.longitudo() { n ← x }");
    let seen = reads(&source, "x");
    assert!(is_live_length_bound(&seen[1]), "{seen:?}");
}

#[test]
fn a_mutating_intrinsic_kills_the_length_bound() {
    assert_effect_kills_length_bound("varia lista<numerus> s", "s.appende(1)");
}

#[test]
fn a_projected_store_kills_the_length_bound() {
    assert_effect_kills_length_bound("varia lista<numerus> s", "s[0] ← 5");
}

#[test]
fn a_field_store_kills_the_length_bound() {
    // A store through a projection may reach any mutable source through an
    // alias, even one that shares nothing with the bound's list.
    let source = format!(
        "genus Cella {{\n    varia numerus k\n}}\n{LIST}functio f(varia lista<numerus> s, varia Cella p, numerus x) → numerus {{\n    varia numerus n ← 0\n    si x ≺ s.longitudo() {{\n        n ← x\n        p.k ← 5\n        n ← x\n    }}\n    redde n\n}}"
    );
    let seen = reads(&source, "x");
    assert!(is_live_length_bound(&seen[1]), "{seen:?}");
    assert_eq!(seen[2], None, "{seen:?}");
}

#[test]
fn a_mutref_call_kills_the_length_bound() {
    assert_effect_kills_length_bound("varia lista<numerus> s", "h(varia s)");
}

#[test]
fn a_move_call_kills_the_length_bound() {
    assert_effect_kills_length_bound("own lista<numerus> s", "eat(own s)");
}

#[test]
fn even_a_read_only_call_kills_in_version_one() {
    // Closed rule: there is no purity contract yet, so a call that takes the
    // list read-only still ends facts about it.
    assert_effect_kills_length_bound("varia lista<numerus> s", "peek(s)");
}

#[test]
fn an_unknown_indirect_call_kills_the_length_bound() {
    assert_effect_kills_length_bound(
        "varia lista<numerus> s, (numerus) → numerus unknown",
        "n ← unknown(1)",
    );
}

#[test]
fn a_write_through_a_mutable_alias_kills_the_length_bound() {
    // The bound names `s`; the write goes through the alias `r`.
    let source = list_fn(
        "    varia lista<numerus> r ← s\n    si x ≺ s.longitudo() {\n        n ← x\n        r.appende(1)\n        n ← x\n    }",
    );
    let seen = reads(&source, "x");
    assert!(is_live_length_bound(&seen[1]), "{seen:?}");
    assert_eq!(seen[2], None, "{seen:?}");
}

#[test]
fn a_closure_that_writes_a_captured_mut_kills_facts_when_it_is_called() {
    let source = "functio f(numerus x) → numerus {\n    varia numerus c ← 0\n    sit () → numerus bump ← () → numerus ∴ fac {\n        c ← c + 1\n        redde c\n    }\n    si c ≺ 10 {\n        bump()\n        redde c\n    }\n    redde 0\n}";
    let seen = reads(source, "c");
    let after_call = seen.iter().rev().nth(1).cloned().expect("reads of c");
    assert_eq!(
        after_call, None,
        "`c` is mutable and the closure call may have written it: {seen:?}"
    );
}

#[test]
fn a_call_kills_a_mutable_binding_fact_but_not_a_let_fact() {
    let source = "functio g(numerus a) → numerus { redde a }\nfunctio f(varia numerus m, numerus x) → numerus {\n    varia numerus n ← 0\n    si m ≺ 10 et x ≺ 10 {\n        n ← g(1)\n        n ← m + x\n    }\n    redde n\n}";
    let m = reads(source, "m");
    let x = reads(source, "x");
    assert_eq!(m.last().cloned().flatten(), None, "mutable subject: killed");
    assert_eq!(
        x.last().cloned().flatten(),
        Some(lt(10)),
        "an immutable parameter cannot be written by the call: its fact survives"
    );
}

#[test]
fn a_captured_immutable_endpoint_survives_calls() {
    let source = "functio g(numerus a) → numerus { redde a }\nfunctio f(varia lista<numerus> s, numerus x, numerus end) → numerus {\n    varia numerus n ← 0\n    si x ≺ end {\n        s.appende(1)\n        n ← g(1)\n        n ← x\n    }\n    redde n\n}";
    let seen = reads(source, "x");
    assert!(
        matches!(
            seen.last(),
            Some(Some(IntRange {
                hi: Some(super::flow_range::Endpoint {
                    key: BoundKey::Binding(_),
                    strict: true
                }),
                lo: None
            }))
        ),
        "`x ≺ end` is about two immutable bindings: {seen:?}"
    );
}

#[test]
fn an_immutable_list_keeps_its_length_bound_across_calls() {
    let source = "functio g(numerus a) → numerus { redde a }\nfunctio f(lista<numerus> s, numerus x) → numerus {\n    varia numerus n ← 0\n    si x ≺ s.longitudo() {\n        n ← g(1)\n        n ← x\n    }\n    redde n\n}";
    let seen = reads(source, "x");
    assert!(
        is_live_length_bound(seen.last().expect("a read")),
        "{seen:?}"
    );
}

#[test]
fn a_rebound_binding_loses_its_fact_and_a_loop_that_calls_drops_entry_facts() {
    let source = "functio g(numerus a) → numerus { redde a }\nfunctio f(varia numerus m) → numerus {\n    varia numerus n ← 0\n    si m ≺ 10 {\n        dum n ≺ 3 {\n            n ← g(n)\n            n ← m\n        }\n    }\n    redde n\n}";
    let seen = reads(source, "m");
    assert_eq!(
        seen.last().cloned().flatten(),
        None,
        "the loop body calls: the pre-loop fact on mutable `m` does not enter it"
    );
}
