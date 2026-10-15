//! A list literal stored into an optional or union slot takes its element
//! type from the slot's single list member, exactly as it does for a plain
//! `lista<T>` slot (opt-lit). The empty literal `vacua` and a typed tuple
//! constructor land in an optional slot the same way.

use crate::test_support::{analyze_source_la, assert_accepts_la};

fn rejected(source: &str) -> bool {
    !analyze_source_la(source).is_ok()
}

#[test]
fn list_literal_checks_against_optional_list_slot() {
    assert_accepts_la(
        r"incipit {
    sit lista<numerus> ∪ nihil a ← [1, 2]
    varia lista<numerus> ∪ nihil b ← nulla
    b ← [3]
    nota a
    nota b
}",
    );
}

#[test]
fn optional_list_slot_elements_are_stores_into_the_element_type() {
    // The element is range-checked against `i8` through the
    // optional slot, as it is for the plain slot.
    let plain = "incipit { sit lista<i8> a ← [300] nota a }";
    let optional = "incipit { sit lista<i8> ∪ nihil a ← [300] nota a }";
    assert!(
        rejected(plain),
        "plain slot rejects an out-of-range element"
    );
    assert!(
        rejected(optional),
        "optional slot rejects an out-of-range element"
    );
    assert_accepts_la("incipit { sit lista<i8> ∪ nihil a ← [100] nota a }");
}

#[test]
fn vacua_stores_into_optional_collection_slots() {
    assert_accepts_la(
        r"incipit {
    sit lista<numerus> ∪ nihil a ← vacua
    varia lista<numerus> ∪ nihil b ← nulla
    b ← vacua
    sit tabula<textus, numerus> ∪ nihil c ← vacua
    nota a
    nota b
    nota c
}",
    );
}

#[test]
fn vacua_still_needs_exactly_one_collection_member() {
    // No collection member, or two (the literal's shape is then ambiguous).
    assert!(rejected("incipit { sit numerus ∪ nihil a ← vacua nota a }"));
    assert!(rejected(
        "incipit { sit lista<numerus> ∪ tabula<textus, numerus> a ← vacua nota a }"
    ));
}

#[test]
fn tuple_constructor_stores_into_optional_tuple_slot() {
    // The constructor's own type arguments carry the element types, so an
    // optional slot needs no help from the slot: the element is still a store
    // into `i8`.
    assert_accepts_la(
        r"incipit {
    sit iuncta<i8, numerus> ∪ nihil a ← iuncta<i8, numerus> [100, 2]
    nota a
}",
    );
    assert!(rejected(
        "incipit { sit iuncta<i8, numerus> ∪ nihil a ← iuncta<i8, numerus> [300, 2] nota a }"
    ));
}
