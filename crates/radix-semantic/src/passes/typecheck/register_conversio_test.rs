//! CTFP-1 pins: register-class conversio rows (`vector`/`octeti` windows in,
//! register-out to `lista`/`octeti`, element bridges, bounded-lista
//! materialization into registers).

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

// --- register-in: octeti<N> ↦ vector<T, N/width> windows -------------------

#[test]
fn octeti_window_16_to_vu32x4_be_accepts() {
    assert_accepts(
        "functio win(octeti corpus) → vector<numerus<u32>, 4> {
    redde corpus[0‥16] ↦ vector<numerus<u32>, 4><Be>
}",
    );
}

#[test]
fn octeti_window_16_to_vu16x8_le_accepts() {
    assert_accepts(
        "functio win(octeti corpus) → vector<numerus<u16>, 8> {
    redde corpus[0‥16] ↦ vector<numerus<u16>, 8><Le>
}",
    );
}

#[test]
fn octeti_window_8_to_vu32x2_be_accepts() {
    assert_accepts(
        "functio win(octeti corpus) → vector<numerus<u32>, 2> {
    redde corpus[0‥8] ↦ vector<numerus<u32>, 2><Be>
}",
    );
}

#[test]
fn octeti_literal_16_to_vu32x4_le_accepts() {
    assert_accepts(
        "incipit {
    fixum _ v ← |de ad be ef fe dc ba 98 76 54 32 10 01 23 45 67| ↦ vector<numerus<u32>, 4><Le>
}",
    );
}

#[test]
fn octeti_window_without_endian_rejects() {
    assert_rejects_with_issue(
        "functio win(octeti corpus) → vector<numerus<u32>, 4> {
    redde corpus[0‥16] ↦ vector<numerus<u32>, 4>
}",
        "octeti_to_vector_unspecified",
    );
}

#[test]
fn octeti_window_wrong_width_rejects() {
    assert_rejects_with_issue(
        "functio win(octeti corpus) → vector<numerus<u32>, 4> {
    redde corpus[0‥12] ↦ vector<numerus<u32>, 4><Be>
}",
        "register_window_width_mismatch",
    );
}

#[test]
fn octeti_window_u8_element_rejects() {
    assert_rejects_with_issue(
        "functio win(octeti corpus) → vector<numerus<u8>, 16> {
    redde corpus[0‥16] ↦ vector<numerus<u8>, 16><Be>
}",
        "register_window_width_mismatch",
    );
}

// --- register-out: vector<T, N> ↦ lista<T> / octeti<N> --------------------

#[test]
fn vector_u32x4_to_lista_accepts() {
    assert_accepts(
        "functio out(vector<numerus<u32>, 4> v) → lista<numerus<u32>> {
    redde v ↦ lista<numerus<u32>>
}",
    );
}

#[test]
fn vector_u32x4_to_octeti_16_le_accepts() {
    assert_accepts(
        "functio out(vector<numerus<u32>, 4> v) → octeti<16> {
    redde v ↦ octeti<16><Le>
}",
    );
}

#[test]
fn vector_u32x4_to_octeti_16_be_accepts() {
    assert_accepts(
        "functio out(vector<numerus<u32>, 4> v) → octeti<16> {
    redde v ↦ octeti<16><Be>
}",
    );
}

#[test]
fn vector_to_octeti_without_endian_rejects() {
    assert_rejects_with_issue(
        "functio out(vector<numerus<u32>, 4> v) → octeti<16> {
    redde v ↦ octeti<16>
}",
        "vector_to_octeti_unspecified",
    );
}

#[test]
fn vector_to_octeti_wrong_capacity_rejects() {
    assert_rejects_with_issue(
        "functio out(vector<numerus<u32>, 4> v) → octeti<8> {
    redde v ↦ octeti<8><Le>
}",
        "register_window_width_mismatch",
    );
}

#[test]
fn vector_to_lista_element_mismatch_rejects() {
    assert_rejects_with_issue(
        "functio out(vector<numerus<u32>, 4> v) → lista<numerus<u16>> {
    redde v ↦ lista<numerus<u16>>
}",
        "register_out_element_mismatch",
    );
}

// --- bridges: vector<T, N> ↦ vector<U, N> ----------------------------------

#[test]
fn vector_u32x4_to_f32x4_bridge_accepts() {
    assert_accepts(
        "functio br(vector<numerus<u32>, 4> v) → vector<fractus<f32>, 4> {
    redde v ↦ vector<fractus<f32>, 4>
}",
    );
}

#[test]
fn vector_f32x4_to_f64x4_bridge_accepts() {
    assert_accepts(
        "functio br(vector<fractus<f32>, 4> v) → vector<fractus<f64>, 4> {
    redde v ↦ vector<fractus<f64>, 4>
}",
    );
}

#[test]
fn vector_bridge_width_mismatch_rejects() {
    assert_rejects_with_issue(
        "functio br(vector<numerus<u32>, 4> v) → vector<numerus<u32>, 2> {
    redde v ↦ vector<numerus<u32>, 2>
}",
        "register_bridge_width_mismatch",
    );
}

#[test]
fn vector_bridge_bivalens_out_of_set_rejects() {
    assert_rejects_with_issue(
        "functio br(vector<numerus<u32>, 4> v) → vector<bivalens, 4> {
    redde v ↦ vector<bivalens, 4>
}",
        "register_bridge_element_unsupported",
    );
}

// --- bounded lista<T, N> ↦ vector<T, N> (non-literal) ----------------------

#[test]
fn bounded_lista_u32x4_to_vector_accepts() {
    assert_accepts(
        "functio mat(lista<numerus<u32>, 4> xs) → vector<numerus<u32>, 4> {
    redde xs ↦ vector<numerus<u32>, 4>
}",
    );
}

#[test]
fn bounded_lista_element_mismatch_to_vector_rejects() {
    assert_rejects_with_issue(
        "functio mat(lista<numerus<u16>, 4> xs) → vector<numerus<u32>, 4> {
    redde xs ↦ vector<numerus<u32>, 4>
}",
        "lista_to_vector_element_mismatch",
    );
}

#[test]
fn bounded_lista_capacity_mismatch_to_vector_rejects() {
    assert_rejects_with_issue(
        "functio mat(lista<numerus<u32>, 8> xs) → vector<numerus<u32>, 4> {
    redde xs ↦ vector<numerus<u32>, 4>
}",
        "lista_to_vector_literal_width",
    );
}
