//! Producers of interval facts (need, slice 3): integer guards, range
//! loops, list and tensor coordinates, reduction axes, `let` bindings, and the
//! expression ranges (`⤒`, `⤓`, `%`, declared types) they draw on.
//!
//! Every fact is observed through the test-only read observer: the interval the
//! checker holds for a binding at each read of it. Nothing consumes the facts
//! yet. Fixtures use latin spellings (`itera ab`, `sit`, `varia`, `et`).

use super::flow_range::{AxisIdentity, BoundKey, Endpoint, IntRange};
use super::flow_range_probe::{clear_reads, reads_of};
use crate::test_support;

fn c(value: i128) -> BoundKey {
    BoundKey::Const(value)
}

fn at_least(value: i128) -> IntRange {
    IntRange::from_lo(c(value), false)
}

fn at_most(value: i128) -> IntRange {
    IntRange::from_hi(c(value), false)
}

/// The closed interval `[lo, hi]`.
fn within(lo: i128, hi: i128) -> IntRange {
    at_least(lo).meet(&at_most(hi))
}

fn reads(source: &str, name: &str) -> Vec<Option<IntRange>> {
    clear_reads();
    let result = test_support::analyze_source(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    reads_of(name)
}

/// Like [`reads`] for a fixture the checker rejects (a shape or arity error).
fn reads_rejected(source: &str, name: &str) -> Vec<Option<IntRange>> {
    clear_reads();
    let result = test_support::analyze_source(source);
    assert!(result.is_err(), "the fixture should be rejected");
    reads_of(name)
}

fn lower_const(range: &Option<IntRange>) -> Option<i128> {
    match range.as_ref()?.lo.as_ref()? {
        Endpoint {
            key: BoundKey::Const(value),
            strict: false,
        } => Some(*value),
        _ => None,
    }
}

fn upper_key(range: &Option<IntRange>) -> Option<(BoundKey, bool)> {
    let hi = range.as_ref()?.hi.as_ref()?;
    Some((hi.key.clone(), hi.strict))
}

/// The upper bound that relates the value to a name, if any (a bare literal
/// ceiling such as an integer type's maximum is not one).
fn relational_upper(range: &Option<IntRange>) -> Option<BoundKey> {
    upper_key(range)
        .map(|(key, _)| key)
        .filter(|key| !matches!(key, BoundKey::Const(_)))
}

fn is_live_length(key: &BoundKey) -> bool {
    matches!(key, BoundKey::LiveLength { .. })
}

fn is_axis(key: &BoundKey) -> bool {
    matches!(key, BoundKey::Axis(AxisIdentity::Param(_)))
}

fn int_fn(params: &str, body: &str) -> String {
    format!(
        "functio g(numerus a) → numerus {{ redde a }}\n\
         functio f({params}) → numerus {{\n    varia numerus n ← 0\n{body}\n    redde n\n}}"
    )
}

// ── guards ────────────────────────────────────────────────────────────────

#[test]
fn guards_normalise_integer_strictness_in_both_operand_orders() {
    let strict_lo = int_fn("numerus x", "    si x ≻ 5 { n ← x }");
    assert_eq!(
        reads(&strict_lo, "x")[1],
        Some(at_least(6)),
        "`> 5` is `≥ 6`"
    );
    let flipped = int_fn("numerus x", "    si 10 ≻ x { n ← x }");
    assert_eq!(
        reads(&flipped, "x")[1],
        Some(at_most(9)),
        "`10 > x` is `x ≤ 9`"
    );
    let inclusive = int_fn("numerus x", "    si x ≤ 5 { n ← x } secus { n ← x }");
    assert_eq!(
        reads(&inclusive, "x"),
        vec![None, Some(at_most(5)), Some(at_least(6))]
    );
}

#[test]
fn equality_gives_a_point_and_inequality_gives_it_on_the_else_edge() {
    let eq = int_fn("numerus x", "    si x ≡ 5 { n ← x } secus { n ← x }");
    assert_eq!(reads(&eq, "x"), vec![None, Some(within(5, 5)), None]);
    let ne = int_fn("numerus x", "    si x ≢ 5 { n ← x } secus { n ← x }");
    assert_eq!(reads(&ne, "x"), vec![None, None, Some(within(5, 5))]);
}

#[test]
fn equality_transfers_the_other_operands_interval() {
    let source = int_fn("u8 x, u8 y", "    si x ≡ y { n ← x }");
    let seen = reads(&source, "x");
    assert_eq!(
        lower_const(&seen[1]),
        Some(0),
        "the unsigned cell's floor: {seen:?}"
    );
    assert!(
        matches!(upper_key(&seen[1]), Some((BoundKey::Binding(_), false))),
        "x ≤ y: {seen:?}"
    );
}

#[test]
fn a_guard_alone_never_proves_a_lower_bound() {
    // `i ≺ n` bounds `i` above only. A bound against an unsigned `n` is a
    // relation, not a floor: `i` could still be negative.
    let source = int_fn("numerus i, u32 len", "    si i ≺ len { n ← i }");
    let seen = reads(&source, "i");
    let fact = seen[1].clone().expect("an upper bound");
    assert!(fact.lo.is_none(), "{fact:?}");
    assert!(matches!(
        upper_key(&seen[1]),
        Some((BoundKey::Binding(_), true))
    ));
}

#[test]
fn the_negated_edge_inherits_the_other_operands_origin_floor() {
    // `i ≥ len` with an unsigned `len` does put `i` at or above zero: the floor
    // comes from the unsigned cell, not from the guard.
    let source = int_fn(
        "numerus i, u32 len",
        "    si i ≺ len { n ← 1 } secus { n ← i }",
    );
    assert_eq!(lower_const(&reads(&source, "i")[1]), Some(0));
}

#[test]
fn a_mutable_or_unbounded_operand_gives_no_fact() {
    let mutable = int_fn("numerus x, varia numerus m", "    si x ≺ m { n ← x }");
    assert_eq!(reads(&mutable, "x")[1], None, "nothing is known about `m`");
    let local = int_fn(
        "numerus x",
        "    varia numerus m ← 3\n    si x ≺ m { n ← x }",
    );
    assert_eq!(reads(&local, "x")[1], None);
}

#[test]
fn a_float_comparison_produces_nothing() {
    let source = "functio f(fractus y) → numerus {\n    varia numerus n ← 0\n    si y ≺ 1.5 { n ← 1 } secus { n ← 2 }\n    redde n\n}";
    assert!(reads(source, "y").iter().all(Option::is_none));
}

#[test]
fn a_comparison_of_two_bindings_bounds_the_left_one_against_an_immutable_right() {
    let source = int_fn("numerus i, numerus end", "    si i ≺ end { n ← i }");
    assert!(matches!(
        upper_key(&reads(&source, "i")[1]),
        Some((BoundKey::Binding(_), true))
    ));
}

// ── range loops ───────────────────────────────────────────────────────────

fn range_loop(range: &str) -> String {
    int_fn(
        "numerus x",
        &format!("    itera ab {range} sit i {{ n ← i }}"),
    )
}

#[test]
fn an_ascending_exclusive_loop_is_bounded_by_its_captured_endpoints() {
    assert_eq!(reads(&range_loop("0‥10"), "i"), vec![Some(within(0, 9))]);
    assert_eq!(
        reads(&range_loop("3‥10 per 2"), "i"),
        vec![Some(within(3, 9))]
    );
}

#[test]
fn an_inclusive_loop_includes_its_end() {
    assert_eq!(reads(&range_loop("0…10"), "i"), vec![Some(within(0, 10))]);
    assert_eq!(
        reads(&range_loop("0 usque 4"), "i"),
        vec![Some(within(0, 4))]
    );
}

#[test]
fn a_descending_loop_runs_from_start_down_to_end() {
    assert_eq!(
        reads(&range_loop("10‥0"), "i"),
        vec![Some(within(1, 10))],
        "exclusive: the end is not visited"
    );
    assert_eq!(reads(&range_loop("10…0"), "i"), vec![Some(within(0, 10))]);
}

#[test]
fn an_equal_endpoint_range_ascends() {
    assert_eq!(reads(&range_loop("4…4"), "i"), vec![Some(within(4, 4))]);
}

#[test]
fn a_loop_to_a_signed_endpoint_of_unknown_sign_proves_nothing() {
    // `0‥x` descends when `x` is negative, so neither side holds.
    let source = int_fn("numerus x", "    itera ab 0‥x sit i { n ← i }");
    assert_eq!(reads(&source, "i"), vec![None]);
}

#[test]
fn a_loop_to_an_unsigned_endpoint_is_bounded_by_it() {
    let source = int_fn("u32 len", "    itera ab 0‥len sit i { n ← i }");
    let seen = reads(&source, "i");
    assert_eq!(lower_const(&seen[0]), Some(0));
    assert!(matches!(
        upper_key(&seen[0]),
        Some((BoundKey::Binding(_), true))
    ));
}

#[test]
fn a_loop_over_a_lists_length_is_bounded_by_the_live_length() {
    let source = int_fn(
        "varia lista<numerus> s",
        "    itera ab 0‥s.longitudo() sit i { n ← s[i] }",
    );
    let seen = reads(&source, "i");
    assert_eq!(lower_const(&seen[0]), Some(0));
    let (key, strict) = upper_key(&seen[0]).expect("an upper bound");
    assert!(is_live_length(&key) && strict, "{seen:?}");
}

#[test]
fn a_body_that_may_write_the_list_drops_its_length_bound_at_entry() {
    let source = int_fn(
        "varia lista<numerus> s",
        "    itera ab 0‥s.longitudo() sit i {\n        s[0] ← 9\n        n ← i\n    }",
    );
    let seen = reads(&source, "i");
    assert_eq!(lower_const(&seen[0]), Some(0), "the floor needs no list");
    assert_eq!(
        upper_key(&seen[0]),
        None,
        "the length may have changed: {seen:?}"
    );
}

#[test]
fn an_immutable_list_loses_its_length_bound_through_calls_in_the_body() {
    // Was `..._keeps_...`: an unmarked parameter may alias a list a call writes.
    let source = int_fn(
        "lista<numerus> s",
        "    itera ab 0‥s.longitudo() sit i {\n        n ← g(i)\n        n ← i\n    }",
    );
    let seen = reads(&source, "i");
    assert_eq!(upper_key(&seen[1]), None, "{seen:?}");
}

#[test]
fn an_immutable_text_keeps_its_length_bound_through_calls_in_the_body() {
    let source = int_fn(
        "textus s",
        "    itera ab 0‥s.longitudo() sit i {\n        n ← g(i)\n        n ← i\n    }",
    );
    let seen = reads(&source, "i");
    assert!(upper_key(&seen[1]).is_some_and(|(key, _)| is_live_length(&key)));
}

#[test]
fn a_mutable_endpoint_is_not_a_bound() {
    // The endpoint is read once at entry, but the name can change, so it cannot
    // be the bound.
    let source = int_fn(
        "numerus x",
        "    varia u32 m ← 5\n    itera ab 0‥m sit i { n ← i }",
    );
    let seen = reads(&source, "i");
    assert_eq!(lower_const(&seen[0]), Some(0));
    assert_eq!(relational_upper(&seen[0]), None);
}

#[test]
fn a_binder_the_body_assigns_loses_its_fact() {
    let source = "functio f(numerus x) → numerus {\n    varia numerus n ← 0\n    itera ab 0‥10 varia i {\n        n ← i\n        i ← 3\n    }\n    redde n\n}";
    let mut seen = reads(source, "i");
    assert_eq!(
        seen.remove(0),
        None,
        "an assigned binder is dropped for the whole body"
    );
}

#[test]
fn a_range_product_bounds_each_binder_by_its_own_range() {
    let source = int_fn(
        "numerus x",
        "    itera ab 0‥3, 1…5 sit i, j { n ← i\n        n ← j }",
    );
    assert_eq!(reads(&source, "i"), vec![Some(within(0, 2))]);
    assert_eq!(reads(&source, "j"), vec![Some(within(1, 5))]);
}

#[test]
fn a_loop_binders_fact_does_not_outlive_the_loop() {
    let source = int_fn("numerus x", "    itera ab 0‥10 sit i { n ← i }\n    n ← x");
    assert_eq!(reads(&source, "x"), vec![None]);
}

#[test]
fn a_mutable_loop_counter_loses_its_fact_at_a_call() {
    let source = int_fn(
        "numerus x",
        "    varia numerus k ← 0\n    dum k ≺ 10 {\n        n ← g(k)\n        n ← k\n        k ← k + 1\n    }",
    );
    let seen = reads(&source, "k");
    // cond, the call argument (still bounded), after the call (killed), `k + 1`.
    assert_eq!(seen[1], Some(at_most(9)), "{seen:?}");
    assert_eq!(seen[2], None, "{seen:?}");
}

// ── lists and text ────────────────────────────────────────────────────────

#[test]
fn a_list_coordinate_is_below_the_lists_live_length() {
    let source = int_fn(
        "lista<numerus> xs",
        "    itera ex xs apud [i] sit v { n ← i }",
    );
    let seen = reads(&source, "i");
    assert_eq!(lower_const(&seen[0]), Some(0));
    let (key, strict) = upper_key(&seen[0]).expect("an upper bound");
    assert!(is_live_length(&key) && strict);
}

#[test]
fn a_list_coordinate_over_an_unnamed_source_has_a_floor_only() {
    let source = "functio make() → lista<numerus> { redde [1, 2] }\nfunctio f() → numerus {\n    varia numerus n ← 0\n    itera ex make() apud [i] sit v { n ← i }\n    redde n\n}";
    let seen = reads(source, "i");
    assert_eq!(lower_const(&seen[0]), Some(0));
    assert_eq!(upper_key(&seen[0]), None, "no stable source to name");
}

#[test]
fn a_lockstep_coordinate_is_below_the_first_sources_length() {
    let source = int_fn(
        "lista<numerus> xs, lista<numerus> ys",
        "    itera ex xs, ys apud [i] sit a, b { n ← i }",
    );
    let seen = reads(&source, "i");
    assert_eq!(lower_const(&seen[0]), Some(0));
    assert!(upper_key(&seen[0]).is_some_and(|(key, _)| is_live_length(&key)));
}

// ── tensors and reductions ────────────────────────────────────────────────

#[test]
fn a_tensor_coordinate_is_bounded_per_axis_by_its_resolved_extent() {
    let source = "functio f<magnitudo N, magnitudo M>(tensor<f32, [N, M]> t) → numerus {\n    varia numerus n ← 0\n    itera ex t apud [i, j] sit v { n ← i\n        n ← j }\n    redde n\n}";
    let (i, j) = (reads(source, "i"), reads(source, "j"));
    let (ki, si) = upper_key(&i[0]).expect("i bounded");
    let (kj, sj) = upper_key(&j[0]).expect("j bounded");
    assert!(is_axis(&ki) && is_axis(&kj) && si && sj);
    assert_ne!(ki, kj, "axis 0 is N, axis 1 is M");
    assert_eq!((lower_const(&i[0]), lower_const(&j[0])), (Some(0), Some(0)));
}

#[test]
fn a_literal_extent_gives_a_literal_bound() {
    let source = "functio f(tensor<f32, [4]> t) → numerus {\n    varia numerus n ← 0\n    itera ex t apud [i] sit v { n ← i }\n    redde n\n}";
    assert_eq!(reads(source, "i"), vec![Some(within(0, 3))]);
}

#[test]
fn equal_extents_give_equal_bounds_and_unrelated_ones_do_not() {
    let source = "functio f<magnitudo N, magnitudo K>(tensor<f32, [N]> t, tensor<f32, [N]> u, tensor<f32, [K]> w) → numerus {\n    varia numerus n ← 0\n    itera ex t apud [i] sit v { n ← i }\n    itera ex u apud [p] sit v { n ← p }\n    itera ex w apud [q] sit v { n ← q }\n    redde n\n}";
    let key = |name| upper_key(&reads(source, name)[0]).expect("bounded").0;
    assert_eq!(key("i"), key("p"), "the same size parameter on two tensors");
    assert_ne!(key("i"), key("q"), "another parameter is another extent");
}

#[test]
fn a_mismatched_coordinate_count_proves_nothing() {
    let source = "functio f(tensor<f32, [4, 5]> t) → numerus {\n    varia numerus n ← 0\n    itera ex t apud [i] sit v { n ← i }\n    redde n\n}";
    assert_eq!(reads_rejected(source, "i"), vec![None]);
}

#[test]
fn a_reduction_coordinate_is_bounded_by_its_axis() {
    let source = "functio f(tensor<f32, [6]> xs) → f32 {\n    redde reducta via summa ex xs apud [i] sit v {\n        sit numerus c ← i\n        redde v\n    }\n}";
    assert_eq!(reads(source, "i"), vec![Some(within(0, 5))]);
}

#[test]
fn a_shifted_coordinate_is_not_proven_from_the_coordinate_alone() {
    // The c4-index-fault shape: `j` is in range of its axis, `j + shift` is not
    // known to be.
    let source = "functio f<magnitudo M>(tensor<f32, [M]> t, numerus shift) → numerus {\n    varia numerus n ← 0\n    itera ex t apud [j] sit v {\n        sit numerus plain ← j\n        sit numerus moved ← j + shift\n        n ← plain\n        n ← moved\n    }\n    redde n\n}";
    let plain = reads(source, "plain");
    let moved = reads(source, "moved");
    assert!(upper_key(&plain[0]).is_some_and(|(key, _)| is_axis(&key)));
    assert_eq!(moved, vec![None], "no fact for `j + shift`");
}

// ── lets, declared types, extrema and remainder ──────────────────────────

fn let_fn(params: &str, init: &str) -> String {
    int_fn(params, &format!("    sit numerus m ← {init}\n    n ← m"))
}

#[test]
fn an_unsigned_cell_is_its_own_lower_bound_origin() {
    let seen = reads(&let_fn("u8 x", "x"), "m");
    assert_eq!(lower_const(&seen[0]), Some(0), "{seen:?}");
}

#[test]
fn a_ranged_cell_reads_inside_its_declared_range() {
    let seen = reads(&let_fn("exactus<i32, 0‥200> r", "r"), "m");
    assert_eq!(lower_const(&seen[0]), Some(0), "{seen:?}");
    assert!(
        seen[0].as_ref().is_some_and(|range| range.hi.is_some()),
        "{seen:?}"
    );
}

#[test]
fn a_mutable_let_keeps_no_initializer_fact() {
    let source = int_fn("numerus x", "    varia numerus m ← 4\n    n ← m");
    assert_eq!(reads(&source, "m"), vec![None]);
}

#[test]
fn max_gives_only_a_lower_bound_and_min_only_an_upper_one() {
    assert_eq!(
        reads(&let_fn("numerus x", "x ⤒ 0"), "m"),
        vec![Some(at_least(0))]
    );
    assert_eq!(
        reads(&let_fn("numerus x", "x ⤓ 9"), "m"),
        vec![Some(at_most(9))]
    );
}

#[test]
fn a_clamp_composes_both_bounds() {
    assert_eq!(
        reads(&let_fn("numerus x", "(x ⤒ 0) ⤓ 9"), "m"),
        vec![Some(within(0, 9))]
    );
}

#[test]
fn max_of_bounded_operands_is_bounded_above_by_the_larger_ceiling() {
    let seen = reads(&let_fn("u8 x, u8 y", "x ⤒ y"), "m");
    assert_eq!(lower_const(&seen[0]), Some(0));
}

#[test]
fn a_remainder_by_a_positive_constant_is_below_it() {
    assert_eq!(
        reads(&let_fn("numerus x", "x % 8"), "m"),
        vec![Some(within(0, 7))]
    );
}

#[test]
fn a_remainder_by_a_divisor_of_unknown_sign_proves_nothing() {
    assert_eq!(
        reads(&let_fn("numerus x, numerus d", "x % d"), "m"),
        vec![None]
    );
    assert_eq!(
        reads(
            &let_fn("numerus x, lista<numerus> s", "x % s.longitudo()"),
            "m"
        ),
        vec![None],
        "a length can be zero"
    );
}

#[test]
fn a_remainder_by_a_guarded_positive_divisor_is_below_the_divisor() {
    let source = int_fn(
        "numerus x, numerus d",
        "    si d ≥ 1 {\n        sit numerus m ← x % d\n        n ← m\n    }",
    );
    let seen = reads(&source, "m");
    assert_eq!(lower_const(&seen[0]), Some(0));
    assert!(matches!(
        upper_key(&seen[0]),
        Some((BoundKey::Binding(_), true))
    ));
}

#[test]
fn a_size_parameter_copied_into_a_let_is_that_extent() {
    // `let i32 w ← W` makes `w` the extent `W`, both ways.
    let source = "functio f<magnitudo W>(tensor<f32, [W]> t) → numerus {\n    varia numerus n ← 0\n    sit i32 w ← W\n    n ← w\n    redde n\n}";
    let seen = reads(source, "w");
    let fact = seen[0].clone().expect("w holds the extent");
    let (lo, hi) = (fact.lo.expect("lo"), fact.hi.expect("hi"));
    assert!(is_axis(&hi.key) && !hi.strict);
    assert_eq!(lo.key, c(0), "an extent is at least zero");
}

#[test]
fn a_clamp_against_an_extent_proves_the_ceiling_only_when_the_extent_is_known_positive() {
    // `(x ⤒ 0) ⤓ ((w ⤒ 1) - 1)`: with `w` unknown the result may be 0 for an
    // empty axis, so `< w` is not proven; guarded by `w ≥ 1` it is.
    let clamp = |guard: &str| {
        format!(
            "functio f<magnitudo W>(tensor<f32, [W]> t, i32 x) → numerus {{\n    varia numerus n ← 0\n    sit i32 w ← W\n{guard}\n    redde n\n}}"
        )
    };
    let unguarded = clamp("    sit numerus cx ← (x ⤒ 0) ⤓ ((w ⤒ 1) - 1)\n    n ← cx");
    let seen = reads(&unguarded, "cx");
    assert_eq!(
        lower_const(&seen[0]),
        Some(0),
        "`⤒ 0` is the floor: {seen:?}"
    );
    assert_eq!(
        relational_upper(&seen[0]),
        None,
        "empty extent keeps the check: {seen:?}"
    );

    let guarded = clamp(
        "    si w ≥ 1 {\n        sit numerus cx ← (x ⤒ 0) ⤓ ((w ⤒ 1) - 1)\n        n ← cx\n    }",
    );
    let seen = reads(&guarded, "cx");
    assert_eq!(lower_const(&seen[0]), Some(0));
    let (key, strict) = upper_key(&seen[0]).expect("bounded by the extent");
    assert!(is_axis(&key) && strict, "{seen:?}");
}

// ── the read-only length call ─────────────────────────────────────────────

#[test]
fn a_length_read_does_not_end_facts_but_another_method_does() {
    let source = int_fn(
        "varia lista<numerus> s, numerus x",
        "    si x ≺ s.longitudo() {\n        n ← x\n        n ← s.longitudo()\n        n ← x\n        s.appende(1)\n        n ← x\n    }",
    );
    let seen = reads(&source, "x");
    assert!(upper_key(&seen[1]).is_some(), "{seen:?}");
    assert!(
        upper_key(&seen[2]).is_some(),
        "`longitudo` only reads: {seen:?}"
    );
    assert_eq!(seen[3], None, "`appende` writes: {seen:?}");
}
