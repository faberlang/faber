use super::{Intervallum, IntervallumKind, Tensor};

// ===========================================================================
// i64 — existing tests (preserved and extended)
// ===========================================================================

#[test]
fn exclusive_half_open_containment() {
    let range = Intervallum::exclusive(0_i64, 10);
    assert!(range.continet(&5));
    assert!(!range.continet(&10));
    assert!(!range.continet(&-1));
}

#[test]
fn inclusive_closed_containment() {
    let range = Intervallum::inclusive(0_i64, 10);
    assert!(range.continet(&10));
    assert!(!range.continet(&11));
}

#[test]
fn coercere_half_open_clamps_above_range() {
    let range = Intervallum::exclusive(0, 10);
    assert_eq!(range.coercere(15), 9);
    assert_eq!(range.coercere(5), 5);
    assert_eq!(range.coercere(-3), 0);
}

#[test]
fn coercere_inclusive_clamps_above_range() {
    let range = Intervallum::inclusive(0, 10);
    assert_eq!(range.coercere(15), 10);
}

#[test]
fn ad_lista_honors_inclusivity() {
    let half = Intervallum::exclusive(0, 3);
    assert_eq!(half.ad_lista(), vec![0, 1, 2]);
    let closed = Intervallum::inclusive(0, 3);
    assert_eq!(closed.ad_lista(), vec![0, 1, 2, 3]);
}

#[test]
fn ad_lista_near_max_inclusive() {
    let near_max = Intervallum::inclusive(i64::MAX - 2, i64::MAX);
    assert_eq!(
        near_max.ad_lista(),
        vec![i64::MAX - 2, i64::MAX - 1, i64::MAX]
    );
}

#[test]
fn ad_lista_near_min_inclusive() {
    let near_min = Intervallum::inclusive(i64::MIN + 2, i64::MIN);
    assert_eq!(
        near_min.ad_lista(),
        vec![i64::MIN + 2, i64::MIN + 1, i64::MIN]
    );
}

#[test]
fn ad_lista_single_max_inclusive() {
    assert_eq!(
        Intervallum::inclusive(i64::MAX, i64::MAX).ad_lista(),
        vec![i64::MAX]
    );
}

#[test]
fn ad_lista_single_min_inclusive() {
    assert_eq!(
        Intervallum::inclusive(i64::MIN, i64::MIN).ad_lista(),
        vec![i64::MIN]
    );
}

#[test]
fn coercere_intervallum_inherits_target_kind() {
    let wide = Intervallum::exclusive(0, 100);
    let target = Intervallum::inclusive(10, 50);
    let narrow = wide.coercere_intervallum(&target);
    assert_eq!(narrow.kind, IntervallumKind::Inclusive);
    assert_eq!(narrow.initium, 10);
    assert_eq!(narrow.finis, 50);
}

#[test]
fn ad_tensor_materializes_one_dimensional_half_open() {
    let range = Intervallum::exclusive(0, 3);
    let tensor: Tensor<i64> = range.ad_tensor();
    assert_eq!(tensor.magnitudines(), vec![3]);
    assert_eq!(tensor.planata(), vec![0, 1, 2]);
}

#[test]
fn inter_disjoint_returns_none() {
    let left = Intervallum::exclusive(0, 5);
    let right = Intervallum::exclusive(6, 10);
    assert!(left.inter(right).is_none());
}

#[test]
fn inter_overlapping_half_open() {
    let left = Intervallum::exclusive(0, 10);
    let right = Intervallum::exclusive(5, 15);
    let hit = left.inter(right).expect("overlap");
    assert_eq!(hit, Intervallum::inclusive(5, 9));
}

#[test]
fn union_adjacent_half_open_merges() {
    let left = Intervallum::exclusive(0, 5);
    let right = Intervallum::exclusive(5, 10);
    let merged = left.union(right).expect("adjacent");
    assert_eq!(merged, Intervallum::inclusive(0, 9));
}

#[test]
fn union_gap_returns_none() {
    let left = Intervallum::exclusive(0, 5);
    let right = Intervallum::exclusive(6, 10);
    assert!(left.union(right).is_none());
}

#[test]
fn longitudo_counts_materialized_values() {
    let half = Intervallum::exclusive(0, 10);
    assert_eq!(half.longitudo(), 10);
    let closed = Intervallum::inclusive(0, 10);
    assert_eq!(closed.longitudo(), 11);
}

#[test]
fn longitudo_ascending_exclusive() {
    let range = Intervallum {
        initium: 0,
        finis: 10,
        kind: IntervallumKind::Exclusive,
    };
    assert_eq!(range.longitudo(), range.ad_lista().len() as i64);
}

#[test]
fn longitudo_ascending_inclusive() {
    let range = Intervallum {
        initium: 0,
        finis: 10,
        kind: IntervallumKind::Inclusive,
    };
    assert_eq!(range.longitudo(), range.ad_lista().len() as i64);
}

#[test]
fn longitudo_descending_exclusive() {
    let range = Intervallum {
        initium: 10,
        finis: 0,
        kind: IntervallumKind::Exclusive,
    };
    assert_eq!(range.longitudo(), range.ad_lista().len() as i64);
}

#[test]
fn longitudo_single_point_inclusive() {
    let range = Intervallum {
        initium: 5,
        finis: 5,
        kind: IntervallumKind::Inclusive,
    };
    assert_eq!(range.longitudo(), range.ad_lista().len() as i64);
}

// --- Descending span tests (directional ranges) ---

#[test]
fn descending_exclusive_continet() {
    let range = Intervallum::exclusive(5, 0);
    assert!(range.continet(&5));
    assert!(range.continet(&4));
    assert!(range.continet(&1));
    assert!(!range.continet(&0));
    assert!(!range.continet(&6));
    assert!(!range.continet(&-1));
}

#[test]
fn descending_inclusive_continet() {
    let range = Intervallum::inclusive(5, 0);
    assert!(range.continet(&5));
    assert!(range.continet(&0));
    assert!(!range.continet(&6));
    assert!(!range.continet(&-1));
}

#[test]
fn descending_exclusive_coercere() {
    let range = Intervallum::exclusive(5, 0);
    assert_eq!(range.coercere(5), 5);
    assert_eq!(range.coercere(3), 3);
    assert_eq!(range.coercere(1), 1);
    assert_eq!(range.coercere(0), 1);
    assert_eq!(range.coercere(-1), 1);
    assert_eq!(range.coercere(6), 5);
    assert_eq!(range.coercere(10), 5);
}

#[test]
fn descending_inclusive_coercere() {
    let range = Intervallum::inclusive(5, 0);
    assert_eq!(range.coercere(5), 5);
    assert_eq!(range.coercere(0), 0);
    assert_eq!(range.coercere(-1), 0);
    assert_eq!(range.coercere(6), 5);
}

#[test]
fn descending_exclusive_ad_lista() {
    let range = Intervallum::exclusive(5, 0);
    assert_eq!(range.ad_lista(), vec![5, 4, 3, 2, 1]);
}

#[test]
fn descending_inclusive_ad_lista() {
    let range = Intervallum::inclusive(5, 0);
    assert_eq!(range.ad_lista(), vec![5, 4, 3, 2, 1, 0]);
}

#[test]
fn descending_exclusive_longitudo() {
    let range = Intervallum::exclusive(5, 0);
    assert_eq!(range.longitudo(), 5);
    assert_eq!(range.longitudo(), range.ad_lista().len() as i64);
}

#[test]
fn descending_inclusive_longitudo() {
    let range = Intervallum::inclusive(5, 0);
    assert_eq!(range.longitudo(), 6);
    assert_eq!(range.longitudo(), range.ad_lista().len() as i64);
}

#[test]
fn descending_inter_overlapping() {
    let left = Intervallum::exclusive(5, 0);
    let right = Intervallum::exclusive(7, 1);
    let hit = left.inter(right).expect("overlap");
    assert_eq!(hit, Intervallum::inclusive(5, 2));
}

#[test]
fn descending_inter_disjoint() {
    let left = Intervallum::exclusive(5, 0);
    let right = Intervallum::exclusive(10, 8);
    assert!(left.inter(right).is_none());
}

#[test]
fn descending_union_adjacent() {
    let left = Intervallum::exclusive(5, 1);
    let right = Intervallum::exclusive(1, -3);
    let merged = left.union(right).expect("adjacent");
    assert_eq!(merged, Intervallum::inclusive(5, -2));
    assert_eq!(merged.ad_lista(), vec![5, 4, 3, 2, 1, 0, -1, -2]);
}

#[test]
fn mixed_direction_inter() {
    let asc = Intervallum::exclusive(0, 5);
    let desc = Intervallum::exclusive(7, 1);
    let hit = asc.inter(desc).expect("overlap");
    assert_eq!(hit, Intervallum::inclusive(2, 4));
    assert_eq!(hit.ad_lista(), vec![2, 3, 4]);
}

#[test]
fn mixed_direction_union_touching() {
    let desc = Intervallum::exclusive(4, 1);
    let asc = Intervallum::inclusive(5, 10);
    let merged = desc.union(asc).expect("adjacent");
    assert_eq!(merged, Intervallum::inclusive(10, 2));
    assert_eq!(merged.ad_lista(), vec![10, 9, 8, 7, 6, 5, 4, 3, 2]);
}

#[test]
fn ad_tensor_descending_preserves_order() {
    let range = Intervallum::exclusive(5, 0);
    let tensor: Tensor<i64> = range.ad_tensor();
    assert_eq!(tensor.magnitudines(), vec![5]);
    assert_eq!(tensor.planata(), vec![5, 4, 3, 2, 1]);
}

// ===========================================================================
// Sized integer types — i8, i16, i32
// ===========================================================================

#[test]
fn i8_ascending_exclusive_ad_lista() {
    let r = Intervallum::exclusive(0_i8, 5);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3, 4]);
    assert_eq!(r.longitudo(), 5);
}

#[test]
fn i8_ascending_inclusive_ad_lista() {
    let r = Intervallum::inclusive(0_i8, 5);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(r.longitudo(), 6);
}

#[test]
fn i8_descending_exclusive_ad_lista() {
    let r = Intervallum::exclusive(5_i8, 0);
    assert_eq!(r.ad_lista(), vec![5, 4, 3, 2, 1]);
}

#[test]
fn i8_descending_inclusive_ad_lista() {
    let r = Intervallum::inclusive(5_i8, 0);
    assert_eq!(r.ad_lista(), vec![5, 4, 3, 2, 1, 0]);
}

#[test]
fn i8_coercere_exclusive_clamps() {
    let r = Intervallum::exclusive(0_i8, 10);
    assert_eq!(r.coercere(15), 9);
    assert_eq!(r.coercere(-5), 0);
}

#[test]
fn i8_coercere_inclusive_clamps() {
    let r = Intervallum::inclusive(0_i8, 10);
    assert_eq!(r.coercere(15), 10);
}

#[test]
fn i8_inter_adjacent_returns_none() {
    let a = Intervallum::exclusive(0_i8, 5);
    let b = Intervallum::exclusive(5_i8, 10);
    assert!(a.inter(b).is_none());
}

#[test]
fn i8_union_adjacent_merges() {
    let a = Intervallum::exclusive(0_i8, 5);
    let b = Intervallum::exclusive(5_i8, 10);
    let u = a.union(b).expect("adjacent");
    assert_eq!(u, Intervallum::inclusive(0, 9));
}

#[test]
fn i8_union_gap_returns_none() {
    let c = Intervallum::exclusive(0_i8, 5);
    let d = Intervallum::exclusive(6_i8, 10);
    assert!(c.union(d).is_none());
}

#[test]
fn i16_ascending_ad_lista() {
    let r = Intervallum::exclusive(0_i16, 4);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3]);
    assert_eq!(r.longitudo(), 4);
}

#[test]
fn i16_descending_ad_lista() {
    let r = Intervallum::exclusive(4_i16, 0);
    assert_eq!(r.ad_lista(), vec![4, 3, 2, 1]);
}

#[test]
fn i16_continet_exclusive() {
    let r = Intervallum::exclusive(0_i16, 100);
    assert!(r.continet(&50));
    assert!(!r.continet(&100));
}

#[test]
fn i16_coercere_exclusive() {
    let r = Intervallum::exclusive(0_i16, 100);
    assert_eq!(r.coercere(200), 99);
    assert_eq!(r.coercere(-1), 0);
}

#[test]
fn i16_ad_tensor() {
    let r = Intervallum::exclusive(0_i16, 4);
    let t: Tensor<i16> = r.ad_tensor();
    assert_eq!(t.planata(), vec![0, 1, 2, 3]);
}

#[test]
fn i32_ascending_ad_lista() {
    let r = Intervallum::exclusive(0_i32, 4);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3]);
}

#[test]
fn i32_descending_ad_lista() {
    let r = Intervallum::exclusive(5_i32, 0);
    assert_eq!(r.ad_lista(), vec![5, 4, 3, 2, 1]);
}

#[test]
fn i32_inter_intersection() {
    let a = Intervallum::exclusive(0_i32, 10);
    let b = Intervallum::exclusive(3_i32, 7);
    let hit = a.inter(b).expect("overlap");
    assert_eq!(hit, Intervallum::inclusive(3, 6));
}

#[test]
fn i32_longitudo_matches_list() {
    let r = Intervallum::inclusive(100_i32, 200);
    assert_eq!(r.longitudo(), 101);
    assert_eq!(r.longitudo(), r.ad_lista().len() as i64);
}

// ===========================================================================
// Sized integer types — u8, u16, u32, u64
// ===========================================================================

#[test]
fn u8_ascending_exclusive_ad_lista() {
    let r = Intervallum::exclusive(0_u8, 5);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3, 4]);
    assert_eq!(r.longitudo(), 5);
}

#[test]
fn u8_ascending_inclusive_ad_lista() {
    let r = Intervallum::inclusive(0_u8, 5);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(r.longitudo(), 6);
}

#[test]
fn u8_continet() {
    let r = Intervallum::exclusive(10_u8, 20);
    assert!(r.continet(&10));
    assert!(r.continet(&15));
    assert!(!r.continet(&20));
    assert!(!r.continet(&9));
}

#[test]
fn u8_coercere_clamps() {
    let r = Intervallum::exclusive(10_u8, 20);
    assert_eq!(r.coercere(5), 10); // below → lo
    assert_eq!(r.coercere(25), 19); // above → hi_valid (excluded finis - 1)
    assert_eq!(r.coercere(15), 15); // inside
}

#[test]
fn u8_inter_adjacent_returns_none() {
    let a = Intervallum::exclusive(0_u8, 5);
    let b = Intervallum::exclusive(5_u8, 10);
    assert!(a.inter(b).is_none());
}

#[test]
fn u8_union_adjacent_merges() {
    let a = Intervallum::exclusive(0_u8, 5);
    let b = Intervallum::exclusive(5_u8, 10);
    let u = a.union(b).expect("adjacent");
    assert_eq!(u, Intervallum::inclusive(0, 9));
}

#[test]
fn u16_ascending_ad_lista() {
    let r = Intervallum::exclusive(0_u16, 4);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3]);
}

#[test]
fn u16_longitudo_exclusive() {
    let r = Intervallum::exclusive(100_u16, 200);
    assert_eq!(r.longitudo(), 100);
}

#[test]
fn u16_longitudo_inclusive() {
    let r = Intervallum::inclusive(100_u16, 200);
    assert_eq!(r.longitudo(), 101);
}

#[test]
fn u16_coercere_intervallum() {
    let wide = Intervallum::exclusive(0_u16, 200);
    let target = Intervallum::inclusive(50_u16, 100);
    let narrow = wide.coercere_intervallum(&target);
    assert_eq!(narrow.kind, IntervallumKind::Inclusive);
    assert_eq!(narrow.initium, 50);
    assert_eq!(narrow.finis, 100);
}

#[test]
fn u32_ascending_ad_lista() {
    let r = Intervallum::exclusive(0_u32, 5);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3, 4]);
}

#[test]
fn u32_longitudo_matches_list() {
    let r = Intervallum::inclusive(1_u32, 10);
    assert_eq!(r.longitudo(), 10);
    assert_eq!(r.longitudo(), r.ad_lista().len() as i64);
}

#[test]
fn u64_exclusive_ad_lista() {
    let r = Intervallum::exclusive(0_u64, 4);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3]);
}

#[test]
fn u64_inclusive_ad_lista() {
    let r = Intervallum::inclusive(0_u64, 4);
    assert_eq!(r.ad_lista(), vec![0, 1, 2, 3, 4]);
}

#[test]
fn u64_continet() {
    let r = Intervallum::exclusive(10_u64, 20);
    assert!(r.continet(&10));
    assert!(!r.continet(&20));
    assert!(!r.continet(&200));
}

#[test]
fn u64_coercere() {
    let r = Intervallum::exclusive(5_u64, 10);
    assert_eq!(r.coercere(2), 5);
    assert_eq!(r.coercere(20), 9);
    assert_eq!(r.coercere(7), 7);
}

#[test]
fn u64_ad_tensor() {
    let r = Intervallum::exclusive(0_u64, 4);
    let t: Tensor<u64> = r.ad_tensor();
    assert_eq!(t.planata(), vec![0, 1, 2, 3]);
}

// ===========================================================================
// IntervallumWalk (ambula) — lazy iterator
// ===========================================================================

#[test]
fn ambula_ascending_exclusive() {
    let r = Intervallum::exclusive(0_i64, 5);
    let collected: Vec<i64> = r.ambula().collect();
    assert_eq!(collected, vec![0, 1, 2, 3, 4]);
}

#[test]
fn ambula_ascending_inclusive() {
    let r = Intervallum::inclusive(0_i64, 5);
    let collected: Vec<i64> = r.ambula().collect();
    assert_eq!(collected, vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn ambula_descending_exclusive() {
    let r = Intervallum::exclusive(5_i64, 0);
    let collected: Vec<i64> = r.ambula().collect();
    assert_eq!(collected, vec![5, 4, 3, 2, 1]);
}

#[test]
fn ambula_descending_inclusive() {
    let r = Intervallum::inclusive(5_i64, 0);
    let collected: Vec<i64> = r.ambula().collect();
    assert_eq!(collected, vec![5, 4, 3, 2, 1, 0]);
}

#[test]
fn ambula_matches_ad_lista_ascending_exclusive() {
    let r = Intervallum::exclusive(0_i64, 10);
    assert_eq!(r.ambula().collect::<Vec<_>>(), r.ad_lista());
}

#[test]
fn ambula_matches_ad_lista_ascending_inclusive() {
    let r = Intervallum::inclusive(0_i64, 10);
    assert_eq!(r.ambula().collect::<Vec<_>>(), r.ad_lista());
}

#[test]
fn ambula_matches_ad_lista_descending_exclusive() {
    let r = Intervallum::exclusive(10_i64, 0);
    assert_eq!(r.ambula().collect::<Vec<_>>(), r.ad_lista());
}

#[test]
fn ambula_matches_ad_lista_descending_inclusive() {
    let r = Intervallum::inclusive(10_i64, 0);
    assert_eq!(r.ambula().collect::<Vec<_>>(), r.ad_lista());
}

#[test]
fn ambula_matches_ad_lista_single_point() {
    let r = Intervallum::inclusive(5_i64, 5);
    assert_eq!(r.ambula().collect::<Vec<_>>(), r.ad_lista());
}

#[test]
fn ambula_u8_ascending() {
    let r = Intervallum::exclusive(0_u8, 5);
    let collected: Vec<u8> = r.ambula().collect();
    assert_eq!(collected, vec![0, 1, 2, 3, 4]);
}

#[test]
fn ambula_i8_descending() {
    let r = Intervallum::exclusive(5_i8, 0);
    let collected: Vec<i8> = r.ambula().collect();
    assert_eq!(collected, vec![5, 4, 3, 2, 1]);
}

#[test]
fn ambula_empty_interval() {
    // Exclusive where initium == finis → empty.
    let r = Intervallum::exclusive(0_i64, 0);
    assert!(r.ambula().next().is_none());
}

#[test]
fn ambula_single_point() {
    let r = Intervallum::inclusive(42_i64, 42);
    let collected: Vec<i64> = r.ambula().collect();
    assert_eq!(collected, vec![42]);
}

// ===========================================================================
// Edge and sad-path tests
// ===========================================================================

#[test]
fn ad_lista_empty_exclusive_range() {
    let r = Intervallum::exclusive(5_i64, 5);
    assert!(r.ad_lista().is_empty());
}

#[test]
fn ad_lista_single_point_exclusive_is_empty() {
    // Exclusive where initium == finis means no values are included.
    let r = Intervallum::exclusive(42_i64, 42);
    assert_eq!(r.ad_lista(), vec![] as Vec<i64>);
}

#[test]
fn inter_identical_ranges_exclusive() {
    let r = Intervallum::exclusive(0_i64, 10);
    let hit = r.inter(r).expect("identical overlap");
    assert_eq!(hit, Intervallum::inclusive(0, 9));
}

#[test]
fn inter_identical_ranges_inclusive() {
    let r = Intervallum::inclusive(0_i64, 10);
    let hit = r.inter(r).expect("identical overlap");
    assert_eq!(hit, r);
}

#[test]
fn inter_fully_contained() {
    let outer = Intervallum::exclusive(0_i64, 100);
    let inner = Intervallum::inclusive(30, 50);
    let hit = outer.inter(inner).expect("contained overlap");
    assert_eq!(hit, Intervallum::inclusive(30, 50));
}

#[test]
fn inter_descending_fully_contained() {
    let outer = Intervallum::exclusive(10_i64, 0);
    let inner = Intervallum::inclusive(8, 2);
    let hit = outer.inter(inner).expect("descending contained overlap");
    assert_eq!(hit, Intervallum::inclusive(8, 2));
}

#[test]
fn inter_descending_disjoint() {
    let left = Intervallum::exclusive(0_i64, 5);
    let right = Intervallum::exclusive(-3, -7);
    assert!(left.inter(right).is_none());
}

#[test]
fn union_identical_ranges_exclusive() {
    let r = Intervallum::exclusive(0_i64, 10);
    let merged = r.union(r).expect("identical union");
    assert_eq!(merged, Intervallum::inclusive(0, 9));
}

#[test]
fn union_identical_ranges_inclusive() {
    let r = Intervallum::inclusive(0_i64, 10);
    let merged = r.union(r).expect("identical union");
    assert_eq!(merged, r);
}

#[test]
fn ad_lista_u8_boundary_full_span() {
    let r = Intervallum::inclusive(u8::MIN, u8::MAX);
    assert_eq!(r.longitudo(), 256);
    assert_eq!(r.ad_lista()[0], u8::MIN);
    assert_eq!(r.ad_lista()[255], u8::MAX);
}

#[test]
fn ambula_u8_boundary_full_span() {
    let r = Intervallum::inclusive(u8::MIN, u8::MAX);
    let collected: Vec<u8> = r.ambula().collect();
    assert_eq!(collected.len(), 256);
    assert_eq!(collected[0], u8::MIN);
    assert_eq!(collected[255], u8::MAX);
}

#[test]
fn longitudo_zero_exclusive() {
    let r = Intervallum::exclusive(5_i64, 5);
    assert_eq!(r.longitudo(), 0);
}

#[test]
fn ad_lista_descending_empty_exclusive() {
    let r = Intervallum::exclusive(5_i64, 5);
    assert!(r.ad_lista().is_empty());
}

#[test]
fn ambula_descending_empty_exclusive() {
    let r = Intervallum::exclusive(5_i64, 5);
    assert!(r.ambula().next().is_none());
}
