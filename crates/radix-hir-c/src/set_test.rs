//! Sets: the list storage under the set contract.

use crate::test_support::emitted;

#[test]
fn set_basics_use_the_set_helpers() {
    let code = emitted(
        "main {\n    mut set<int> s ← empty\n    s.add(1)\n    print s.has(1), s.delete(1), s.length(), s.is_empty()\n}",
    );
    assert!(code.contains("faber_set_i64_add(faber_a,"), "{code}");
    assert!(code.contains("faber_list_i64_contains("), "{code}");
    assert!(code.contains("faber_set_i64_delete("), "{code}");
}

#[test]
fn set_algebra_returns_a_new_set() {
    let code = emitted(
        "main {\n    let set<int> a ← empty\n    let set<int> b ← empty\n    print a.union(b).length(), a.intersection(b).length()\n    print a.is_subset(b), a.is_superset(b)\n}",
    );
    assert!(code.contains("faber_set_i64_union(faber_a,"), "{code}");
    assert!(code.contains("faber_set_i64_intersection(faber_a,"), "{code}");
    assert!(code.contains("faber_set_i64_is_subset("), "{code}");
}

#[test]
fn list_and_set_conversions() {
    let code = emitted(
        "main {\n    let list<int> xs ← [1, 2, 2]\n    let set<int> s ← xs ↦ set<int>\n    let list<int> back ← s ↦ list<int>\n    print back.length()\n}",
    );
    assert!(code.contains("faber_set_i64_of(faber_a,"), "{code}");
    assert!(code.contains("faber_list_i64_copy(faber_a,"), "{code}");
}
