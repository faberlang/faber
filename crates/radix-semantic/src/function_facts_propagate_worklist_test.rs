use crate::TypeId;
use crate::function_facts::{RawFnDeps, propagate_failable};
use handroll::FxHashMap;
use radix_hir::DefId;

fn deps_calls(callees: &[u32]) -> RawFnDeps {
    RawFnDeps {
        calls: callees.iter().copied().map(DefId).collect(),
        ..RawFnDeps::default()
    }
}

fn deps_throw(callees: &[u32]) -> RawFnDeps {
    RawFnDeps {
        direct_throw: true,
        calls: callees.iter().copied().map(DefId).collect(),
        ..RawFnDeps::default()
    }
}

fn deps_ad() -> RawFnDeps {
    RawFnDeps {
        direct_ad: true,
        ..RawFnDeps::default()
    }
}

#[test]
fn chain_propagates_from_leaf_to_root() {
    // f0 → f1 → f2 → f3(throw)
    let mut raw = FxHashMap::default();
    raw.insert(DefId(0), deps_calls(&[1]));
    raw.insert(DefId(1), deps_calls(&[2]));
    raw.insert(DefId(2), deps_calls(&[3]));
    raw.insert(DefId(3), deps_throw(&[]));
    let declared = FxHashMap::default();
    let f = propagate_failable(&raw, &declared);
    assert_eq!(f.len(), 4);
    for i in 0..4 {
        assert!(f.contains(&DefId(i)), "expected DefId({i}) failable");
    }
}

#[test]
fn declared_err_seeds_without_throw() {
    let mut raw = FxHashMap::default();
    raw.insert(DefId(0), deps_calls(&[1]));
    raw.insert(DefId(1), deps_calls(&[]));
    let mut declared = FxHashMap::default();
    declared.insert(DefId(1), TypeId(0));
    let f = propagate_failable(&raw, &declared);
    assert!(f.contains(&DefId(0)));
    assert!(f.contains(&DefId(1)));
}

#[test]
fn direct_ad_does_not_propagate() {
    let mut raw = FxHashMap::default();
    raw.insert(DefId(0), deps_calls(&[1]));
    raw.insert(DefId(1), deps_ad());
    let f = propagate_failable(&raw, &FxHashMap::default());
    assert!(!f.contains(&DefId(0)));
    assert!(f.contains(&DefId(1)));
}

#[test]
fn cycle_marks_all_members_once() {
    // 0 → 1 → 2 → 0, with 2 throwing
    let mut raw = FxHashMap::default();
    raw.insert(DefId(0), deps_calls(&[1]));
    raw.insert(DefId(1), deps_calls(&[2]));
    raw.insert(DefId(2), deps_throw(&[0]));
    let f = propagate_failable(&raw, &FxHashMap::default());
    assert_eq!(f.len(), 3);
}
