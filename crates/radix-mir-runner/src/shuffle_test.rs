//! Unit tests for the D19.6 test-only shuffle (companion to `shuffle.rs`).
//!
//! `shuffle_unordered` reads a process-cached env var, so exercising two
//! different seeds within one test binary goes through the pure `permute`
//! helper directly rather than mutating the environment mid-run.

use super::shuffle::permute;

#[test]
fn permute_is_a_no_op_below_two_elements() {
    let mut empty: Vec<i32> = vec![];
    permute(&mut empty, 42);
    assert!(empty.is_empty());

    let mut one = vec![7];
    permute(&mut one, 42);
    assert_eq!(one, vec![7]);
}

#[test]
fn two_seeds_permute_a_map_snapshot_differently_without_sorting() {
    // Mirrors ex-tabula.fab before its D19.6 fix: three tabula values
    // materialized in their natural (unsorted) FxHashMap order.
    let natural = vec![1, 2, 3];

    let mut under_seed_1 = natural.clone();
    permute(&mut under_seed_1, 1);

    let mut under_seed_2 = natural.clone();
    permute(&mut under_seed_2, 2);

    // The whole point of D19.6: a golden that pins the natural order (or
    // any single order) cannot survive two different seeds unless the
    // corpus sorted first. At least one seed must move the sequence off
    // its natural order, and the two seeds must disagree with each other.
    assert!(
        under_seed_1 != natural || under_seed_2 != natural,
        "at least one seed must actually permute the natural order"
    );
    assert_ne!(
        under_seed_1, under_seed_2,
        "two different seeds must diverge from each other, not just from the natural order"
    );
}

#[test]
fn same_seed_is_deterministic_across_calls() {
    let natural = vec!["alpha", "beta", "gamma", "delta", "epsilon"];

    let mut first = natural.clone();
    permute(&mut first, 99);

    let mut second = natural.clone();
    permute(&mut second, 99);

    assert_eq!(
        first, second,
        "the same seed must reproduce the same permutation"
    );
}

#[test]
fn permutation_is_a_rearrangement_not_a_data_mutation() {
    let mut items = vec![10, 20, 30, 40, 50, 60, 70, 80];
    let mut expected_sorted = items.clone();
    permute(&mut items, 12345);
    let mut actually_sorted = items.clone();
    expected_sorted.sort_unstable();
    actually_sorted.sort_unstable();
    assert_eq!(
        expected_sorted, actually_sorted,
        "shuffle must permute elements, never add, drop, or alter them"
    );
}

#[test]
fn seed_resolution_unset_is_random_zero_is_off_number_is_pinned() {
    use super::shuffle::resolve_seed;
    assert_eq!(resolve_seed(Some("0")), None, "0 turns the shuffle off");
    assert_eq!(resolve_seed(Some("7")), Some(7));
    assert!(resolve_seed(None).is_some(), "unset shuffles by default");
    assert!(resolve_seed(Some("not a number")).is_some());
}

#[test]
fn default_shuffle_permutes_a_sixteen_int_set_and_seed_zero_is_natural() {
    use super::shuffle::{shuffle_unordered, with_seed};
    let natural: Vec<i64> = (0..16).collect();
    let run = |seed: Option<u64>| {
        let mut items = natural.clone();
        with_seed(seed, || shuffle_unordered(&mut items));
        items
    };
    assert_eq!(run(None), natural, "seed 0 keeps the natural order");
    let orders: Vec<Vec<i64>> = (1..=8).map(|seed| run(Some(seed))).collect();
    assert!(
        orders.iter().any(|order| *order != natural),
        "a seeded run must permute the natural order"
    );
    assert!(
        orders.iter().any(|order| *order != orders[0]),
        "different seeds must give different orders"
    );
}
