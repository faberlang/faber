//! Bounded lists: the same list class, marked fixed at run time.

use crate::test_support::emitted;

#[test]
fn a_literal_list_is_bounded_by_its_length() {
    let code = emitted("main {\n    let _ xs ← [1, 2, 3]\n    print xs\n}");
    assert!(
        code.contains(
            "faber_list_i64_of_fixed_capacity(faber_a, 3, (int64_t[]){INT64_C(1), INT64_C(2), INT64_C(3)}, 3)"
        ),
        "{code}"
    );
}

#[test]
fn a_short_bounded_literal_reserves_its_declared_capacity() {
    let code = emitted("main {\n    print [1, 2]\n}");
    // The inferred literal's capacity is its own length.
    assert!(
        code.contains(
            "faber_list_i64_of_fixed_capacity(faber_a, 2, (int64_t[]){INT64_C(1), INT64_C(2)}, 2)"
        ),
        "{code}"
    );
    let short = emitted("main {\n    let list<int, size 4> xs ← [1, 2]\n    print xs.length()\n}");
    assert!(
        short.contains(
            "faber_list_i64_of_fixed_capacity(faber_a, 2, (int64_t[]){INT64_C(1), INT64_C(2)}, 4)"
        ),
        "a bounded literal reserves N zeroed cells, not its length: {short}"
    );
}

#[test]
fn an_ascription_to_list_makes_it_growable() {
    let code = emitted(
        "main {\n    mut list<int> xs ← [1, 2, 3] ∷ list<int>\n    xs.append(4)\n    print xs\n}",
    );
    assert!(code.contains("faber_list_i64_of(faber_a, 3,"), "{code}");
    assert!(!code.contains("of_fixed"), "{code}");
}

#[test]
fn an_empty_bounded_list_has_its_capacity() {
    let code =
        emitted("main {\n    mut list<int, size 8> xs ← empty\n    xs.append(1)\n    print xs\n}");
    assert!(
        code.contains("faber_list_i64_new_fixed(faber_a, 8)"),
        "{code}"
    );
}
