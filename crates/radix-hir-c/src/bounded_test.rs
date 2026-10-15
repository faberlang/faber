//! Bounded lists: the same list class, marked fixed at run time.

use crate::test_support::emitted;

#[test]
fn a_literal_list_is_bounded_by_its_length() {
    let code = emitted("main {\n    let xs ← [1, 2, 3]\n    print xs\n}");
    assert!(
        code.contains("faber_list_i64_of_fixed(faber_a, 3,"),
        "{code}"
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
        emitted("main {\n    mut list<int, 8> xs ← empty\n    xs.append(1)\n    print xs\n}");
    assert!(
        code.contains("faber_list_i64_new_fixed(faber_a, 8)"),
        "{code}"
    );
}
