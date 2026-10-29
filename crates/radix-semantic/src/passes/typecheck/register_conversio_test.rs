//! CTFP-1 pins: register-class conversio rows (`vector`/`octeti` windows in,
//! register-out to `lista`/`octeti`, element bridges, bounded-lista
//! materialization into registers).

use crate::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

// --- register-in: octeti<N> ↦ vector<T, N/width> windows -------------------

#[test]
fn octeti_window_16_to_vu32x4_be_accepts() {
    assert_accepts_la(
        "functio win(lista<b8> corpus) → vector<u32, magnitudo 4> {
    redde corpus[0‥16] ↦ vector<u32, magnitudo 4> via Be
}",
    );
}

#[test]
fn octeti_window_16_to_vu16x8_le_accepts() {
    assert_accepts_la(
        "functio win(lista<b8> corpus) → vector<u16, magnitudo 8> {
    redde corpus[0‥16] ↦ vector<u16, magnitudo 8> via Le
}",
    );
}

#[test]
fn octeti_window_8_to_vu32x2_be_accepts() {
    assert_accepts_la(
        "functio win(lista<b8> corpus) → vector<u32, magnitudo 2> {
    redde corpus[0‥8] ↦ vector<u32, magnitudo 2> via Be
}",
    );
}

#[test]
fn octeti_literal_16_to_vu32x4_le_accepts() {
    assert_accepts_la(
        "incipit {
    sit _ v ← |de ad be ef fe dc ba 98 76 54 32 10 01 23 45 67| ↦ vector<u32, magnitudo 4> via Le
}",
    );
}

#[test]
fn octeti_window_without_endian_rejects() {
    assert_rejects_with_issue_la(
        "functio win(lista<b8> corpus) → vector<u32, magnitudo 4> {
    redde corpus[0‥16] ↦ vector<u32, magnitudo 4>
}",
        "octeti_to_vector_unspecified",
    );
}

#[test]
fn octeti_window_wrong_width_rejects() {
    assert_rejects_with_issue_la(
        "functio win(lista<b8> corpus) → vector<u32, magnitudo 4> {
    redde corpus[0‥12] ↦ vector<u32, magnitudo 4> via Be
}",
        "register_window_width_mismatch",
    );
}

#[test]
fn octeti_window_u8_element_rejects() {
    assert_rejects_with_issue_la(
        "functio win(lista<b8> corpus) → vector<u8, magnitudo 16> {
    redde corpus[0‥16] ↦ vector<u8, magnitudo 16> via Be
}",
        "register_window_width_mismatch",
    );
}

// --- register-out: vector<T, size N> ↦ lista<T> / octeti<N> --------------------

#[test]
fn vector_u32x4_to_lista_accepts() {
    assert_accepts_la(
        "functio out(vector<u32, magnitudo 4> v) → lista<u32> {
    redde v ↦ lista<u32>
}",
    );
}

#[test]
fn vector_u32x4_to_octeti_16_le_accepts() {
    assert_accepts_la(
        "functio out(vector<u32, magnitudo 4> v) → lista<b8, magnitudo 16> {
    redde v ↦ lista<b8, magnitudo 16> via Le
}",
    );
}

#[test]
fn vector_u32x4_to_octeti_16_be_accepts() {
    assert_accepts_la(
        "functio out(vector<u32, magnitudo 4> v) → lista<b8, magnitudo 16> {
    redde v ↦ lista<b8, magnitudo 16> via Be
}",
    );
}

#[test]
fn vector_to_octeti_without_endian_rejects() {
    assert_rejects_with_issue_la(
        "functio out(vector<u32, magnitudo 4> v) → lista<b8, magnitudo 16> {
    redde v ↦ lista<b8, magnitudo 16>
}",
        "vector_to_octeti_unspecified",
    );
}

#[test]
fn vector_to_octeti_wrong_capacity_rejects() {
    assert_rejects_with_issue_la(
        "functio out(vector<u32, magnitudo 4> v) → lista<b8, magnitudo 8> {
    redde v ↦ lista<b8, magnitudo 8> via Le
}",
        "register_window_width_mismatch",
    );
}

#[test]
fn vector_to_lista_element_mismatch_rejects() {
    assert_rejects_with_issue_la(
        "functio out(vector<u32, magnitudo 4> v) → lista<u16> {
    redde v ↦ lista<u16>
}",
        "register_out_element_mismatch",
    );
}

// --- bridges: vector<T, size N> ↦ vector<U, size N> ----------------------------------

#[test]
fn vector_u32x4_to_f32x4_bridge_accepts() {
    assert_accepts_la(
        "functio br(vector<u32, magnitudo 4> v) → vector<f32, magnitudo 4> {
    redde v ↦ vector<f32, magnitudo 4>
}",
    );
}

#[test]
fn vector_f32x4_to_f64x4_bridge_accepts() {
    assert_accepts_la(
        "functio br(vector<f32, magnitudo 4> v) → vector<f64, magnitudo 4> {
    redde v ↦ vector<f64, magnitudo 4>
}",
    );
}

#[test]
fn vector_bridge_width_mismatch_rejects() {
    assert_rejects_with_issue_la(
        "functio br(vector<u32, magnitudo 4> v) → vector<u32, magnitudo 2> {
    redde v ↦ vector<u32, magnitudo 2>
}",
        "register_bridge_width_mismatch",
    );
}

#[test]
fn vector_bridge_bivalens_out_of_set_rejects() {
    assert_rejects_with_issue_la(
        "functio br(vector<u32, magnitudo 4> v) → vector<bivalens, magnitudo 4> {
    redde v ↦ vector<bivalens, magnitudo 4>
}",
        "register_bridge_element_unsupported",
    );
}

// --- bounded lista<T, magnitudo N> ↦ vector<T, size N> (non-literal) ----------------------

#[test]
fn bounded_lista_u32x4_to_vector_accepts() {
    assert_accepts_la(
        "functio mat(lista<u32, magnitudo 4> xs) → vector<u32, magnitudo 4> {
    redde xs ↦ vector<u32, magnitudo 4>
}",
    );
}

#[test]
fn bounded_lista_element_mismatch_to_vector_rejects() {
    assert_rejects_with_issue_la(
        "functio mat(lista<u16, magnitudo 4> xs) → vector<u32, magnitudo 4> {
    redde xs ↦ vector<u32, magnitudo 4>
}",
        "lista_to_vector_element_mismatch",
    );
}

#[test]
fn bounded_lista_capacity_mismatch_to_vector_rejects() {
    assert_rejects_with_issue_la(
        "functio mat(lista<u32, magnitudo 8> xs) → vector<u32, magnitudo 4> {
    redde xs ↦ vector<u32, magnitudo 4>
}",
        "lista_to_vector_literal_width",
    );
}
