//! List methods beyond length, append and index.

use crate::test_support::emitted;

#[test]
fn an_end_read_is_an_optional() {
    let code = emitted(
        "main {\n    let list<int> xs ← [1, 2] ∷ list<int>\n    print xs.first(), xs.last()\n}",
    );
    assert!(code.contains("faber_list_i64_get("), "{code}");
    assert!(code.contains("faber_opt_i64"), "{code}");
}

#[test]
fn removing_an_end_is_an_effect_so_siblings_keep_their_order() {
    let code = emitted(
        "main {\n    mut list<int> v ← [1] ∷ list<int>\n    print v.remove_last(), v.length()\n}",
    );
    // Both operands are bound in source order before the print.
    let remove = code.find("faber_list_i64_remove_last").expect("remove");
    let length = code.find("->len").expect("length");
    assert!(remove < length, "{code}");
}

#[test]
fn copies_and_ordering_allocate_in_the_arena() {
    let code = emitted(
        "main {\n    let list<int> xs ← [3, 1] ∷ list<int>\n    print xs.sorted(), xs.invert(), xs.added(9)\n    print sort via desc from xs\n}",
    );
    for call in [
        "faber_list_i64_sorted(faber_a, l_xs)",
        "faber_list_i64_inverted(faber_a, l_xs)",
        "faber_list_i64_added(faber_a, l_xs, INT64_C(9))",
    ] {
        assert!(code.contains(call), "{call:?} missing in {code}");
    }
}

#[test]
fn integer_lists_sum_exactly() {
    let code = emitted("main {\n    let list<int> xs ← [1, 2] ∷ list<int>\n    print xs.sum()\n}");
    assert!(code.contains("faber_list_i64_sum(l_xs)"), "{code}");
}

#[test]
fn in_place_sort_uses_the_sort_helper() {
    let code = emitted("main {\n    mut list<int> v ← [2, 1]\n    v.sort()\n    print v\n}");
    assert!(code.contains("faber_list_i64_sort(l_v)"), "{code}");
}

#[test]
fn a_spread_literal_pushes_each_element_in_order() {
    let code = emitted(
        "main {\n    let list<int> xs ← [2]\n    let list<int> ys ← [1, spread xs, 3]\n    print ys\n}",
    );
    assert!(
        code.contains("faber_list_i64_push_all(faber_a, faber_t0, l_xs)"),
        "{code}"
    );
    assert!(code.contains("faber_list_i64_new(faber_a, 0)"), "{code}");
}

#[test]
fn a_modular_list_uses_its_width_family() {
    let code = emitted(
        "main {\n    let list<wrapping<u32>> xs ← [1, 4294967295]\n    print xs\n    print xs.sorted()\n}",
    );
    assert!(code.contains("faber_list_u32_of(faber_a, 2"), "{code}");
    assert!(code.contains("faber_list_u32_show(faber_a, l_xs)"), "{code}");
    assert!(
        code.contains("faber_list_u32_sorted(faber_a, l_xs)"),
        "{code}"
    );
}

#[test]
fn a_modular_list_element_reduces_at_its_store() {
    // `300` is not a `u8`; the element store reduces it modulo 2^8.
    let code = emitted(
        "main {\n    let list<wrapping<u8>> xs ← [300]\n    print xs\n}",
    );
    assert!(code.contains("faber_store_wrapping_u8("), "{code}");
}

#[test]
fn a_u64_list_element_reads_back_exact() {
    let code = emitted(
        "main {\n    let list<wrapping<u64>> xs ← [18446744073709551615]\n    print xs.get(0) coalesce 0 ∷ wrapping<u64>\n}",
    );
    assert!(code.contains("faber_list_u64_get("), "{code}");
    assert!(code.contains("faber_opt_u64"), "{code}");
}

#[test]
fn membership_is_the_list_family_contains() {
    let code = emitted(
        "main {\n    let string c ← \"agens\"\n    if c ∈ [\"pendens\", \"agens\"] {\n        print \"in\"\n    }\n    if 21 ∉ [18, 21] {\n        print \"out\"\n    }\n}",
    );
    assert!(code.contains("faber_list_text_contains("), "{code}");
    assert!(code.contains("faber_list_i64_contains("), "{code}");
    assert!(code.contains("(!(faber_list_i64_contains("), "{code}");
}
