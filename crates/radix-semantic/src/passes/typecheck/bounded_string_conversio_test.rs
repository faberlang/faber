//! CTFP-2 pins: bounded string conversio rows — `ascii<N> ↦ textus`,
//! `textus<N> ↦ textus`, `ascii<N> ↔ ascii<M>` (D7 resize: copy initialized
//! prefix, fail-closed overflow). The landed AEU-5 `lista<T,magnitudo N> ↦ tensor<T,[N]>`
//! row is pinned by `lista_tensor_conversio` tests — cited, not duplicated.

use crate::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

// --- ascii<N> ↦ textus ------------------------------------------------------

#[test]
fn ascii8_to_textus_accepts() {
    assert_accepts_la(
        "functio s(ascii<magnitudo 8> b) → textus {
    redde b ↦ textus
}",
    );
}

// --- textus<N> ↦ textus -----------------------------------------------------

#[test]
fn bounded_textus_to_textus_accepts() {
    assert_accepts_la(
        "functio s(textus<magnitudo 4> t) → textus {
    redde t ↦ textus
}",
    );
}

// --- ascii<N> ↔ ascii<M> (resize: copy initialized prefix) -----------------

#[test]
fn ascii4_to_ascii8_resize_grow_accepts() {
    assert_accepts_la(
        "functio s(ascii<magnitudo 4> b) → ascii<magnitudo 8> {
    redde b ↦ ascii<magnitudo 8>
}",
    );
}

#[test]
fn ascii8_to_ascii4_resize_shrink_accepts() {
    assert_accepts_la(
        "functio s(ascii<magnitudo 8> b) → ascii<magnitudo 4> {
    redde b ↦ ascii<magnitudo 4>
}",
    );
}

#[test]
fn ascii8_to_ascii8_identity_accepts() {
    assert_accepts_la(
        "functio s(ascii<magnitudo 8> b) → ascii<magnitudo 8> {
    redde b ↦ ascii<magnitudo 8>
}",
    );
}

// --- fail-closed overflow (D7 pattern) --------------------------------------

#[test]
fn ascii_literal_over_capacity_rejects() {
    assert_rejects_with_issue_la(
        "incipit {
    sit _ s ← 'alpha' ↦ ascii<magnitudo 4>
}",
        "ascii_literal_too_long",
    );
}

#[test]
fn textus_literal_over_capacity_rejects() {
    assert_rejects_with_issue_la(
        "incipit {
    sit _ t ← \"abcdef\" ↦ textus<magnitudo 4>
}",
        "textus_literal_too_long",
    );
}

#[test]
fn probe_ascii_literal_to_bounded() {}
