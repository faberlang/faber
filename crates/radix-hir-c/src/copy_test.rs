//! The `copy` marker (R7) and the `own` marker.

use crate::test_support::emitted;

#[test]
fn a_copy_bind_deep_duplicates_a_list() {
    let code = emitted(
        "main {\n    let list<int> a ← [1]\n    mut list<int> b ← copy a\n    print b.length()\n}",
    );
    assert!(code.contains("faber_list_i64_copy(faber_a, l_a)"), "{code}");
}

#[test]
fn a_copy_argument_deep_duplicates_before_the_call() {
    let code = emitted(
        "fn take(copy list<int> items) → int {\n    return items.length()\n}\nmain {\n    let list<int> a ← [1]\n    print take(copy a)\n}",
    );
    assert!(
        code.contains("f_take(faber_a, faber_list_i64_copy(faber_a, l_a))"),
        "{code}"
    );
}

#[test]
fn a_copy_of_a_genus_generates_and_calls_its_clone() {
    let code = emitted(
        "class Holder {\n    var int n = 0\n    var list<int> marks = []\n}\nmain {\n    let Holder a ← Holder { n = 1 }\n    mut Holder b ← copy a\n    print b.n\n}",
    );
    assert!(code.contains("g_Holder_copy(faber_a, l_a)"), "{code}");
    assert!(
        code.contains("g_Holder_copy(faber_arena *faber_a, const g_Holder *faber_self)"),
        "{code}"
    );
    // The list field is deep-duplicated, not shared.
    assert!(
        code.contains("faber_list_i64_copy(faber_a, faber_self->m_marks)"),
        "{code}"
    );
}

#[test]
fn a_clone_is_emitted_once_even_when_asked_twice() {
    let code = emitted(
        "class Counter {\n    var int n = 0\n}\nmain {\n    let Counter a ← Counter { n = 1 }\n    mut Counter b ← copy a\n    mut Counter c ← copy a\n    print b.n, c.n\n}",
    );
    // One definition (the prototype is the same text without the body).
    assert_eq!(
        code.matches("g_Counter_copy(faber_arena *faber_a, const g_Counter *faber_self) {")
            .count(),
        1,
        "{code}"
    );
}

#[test]
fn an_own_marker_is_a_transfer_not_a_copy() {
    let code = emitted(
        "fn eat(own list<int> items) → int {\n    return items.length()\n}\nmain {\n    mut list<int> a ← [1]\n    print eat(own a)\n}",
    );
    assert!(!code.contains("_copy("), "{code}");
}
