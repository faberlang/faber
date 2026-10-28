//! Contracts for the list join `extende` (en `extend`): one list of the
//! receiver's element type appended in place to a plain `lista<T>` receiver.

use crate::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

#[test]
fn extende_accepts_a_plain_list_of_the_same_element_type() {
    assert_accepts_la(
        r"incipit {
    varia lista<numerus> xs ← vacua
    sit lista<numerus> ys ← vacua
    xs.extende(ys)
    xs.extende([1, 2])
    xs.extende(xs)
}",
    );
}

#[test]
fn extende_accepts_a_bounded_byte_buffer_into_a_byte_list() {
    // A fixed-capacity `lista<b8, N>` is not assignable to `lista<b8>`, but it
    // joins one: that is how per-element sized encodes become one byte string.
    assert_accepts_la(
        r"functio join(lista<b8, magnitudo 4> part) → lista<b8> {
    varia lista<b8> out ← vacua
    out.extende(part)
    redde out
}",
    );
}

#[test]
fn extende_accepts_a_bounded_list_of_the_same_element_type() {
    assert_accepts_la(
        r"functio join(lista<numerus, magnitudo 4> part) → lista<numerus> {
    varia lista<numerus> out ← vacua
    out.extende(part)
    redde out
}",
    );
}

#[test]
fn extende_rejects_a_different_element_type() {
    assert_rejects_with_issue_la(
        r"functio bad(lista<textus> names) → lista<numerus> {
    varia lista<numerus> out ← vacua
    out.extende(names)
    redde out
}",
        "argument_type_mismatch",
    );
    // A bounded byte buffer carries raw bytes (`b8`), not the numeric `u8`.
    assert_rejects_with_issue_la(
        r"functio bad(lista<b8, magnitudo 4> part) → lista<u8> {
    varia lista<u8> out ← vacua
    out.extende(part)
    redde out
}",
        "argument_type_mismatch",
    );
}

#[test]
fn extende_rejects_an_argument_that_is_not_a_list() {
    assert_rejects_with_issue_la(
        r"incipit {
    varia lista<numerus> xs ← vacua
    xs.extende(3)
}",
        "argument_type_mismatch",
    );
}

#[test]
fn extende_needs_a_mutable_receiver() {
    assert_rejects_with_issue_la(
        r"incipit {
    sit lista<numerus> xs ← vacua
    sit lista<numerus> ys ← vacua
    xs.extende(ys)
}",
        "imperativus_operator_method_requires_varia_receiver",
    );
}

#[test]
fn extende_refuses_a_bounded_receiver_by_name() {
    assert_rejects_with_issue_la(
        r"functio bad(lista<numerus, magnitudo 4> xs, lista<numerus> ys) → lista<numerus, magnitudo 4> {
    xs.extende(ys)
    redde xs
}",
        "extend_requires_plain_list",
    );
}

#[test]
fn extende_inside_a_loop_over_its_receiver_is_a_shape_change() {
    assert_rejects_with_issue_la(
        r"incipit {
    varia lista<numerus> xs ← vacua
    sit lista<numerus> ys ← vacua
    itera ex xs sit x {
        xs.extende(ys)
    }
}",
        "loop_collection_shape_change",
    );
}
