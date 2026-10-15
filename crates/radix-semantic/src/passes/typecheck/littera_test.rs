//! D10.1–D10.5: `littera` (one Unicode scalar) and the retired byte word.
//!
//! `ascii[i]`, text iteration, and text cursors bind `littera`; `textus[i]`
//! keeps its `ascii<1>` row while operator fork F5 is held. `ascii<1> →
//! littera → textus` widen implicitly, the scalar `octetus` is retired
//! (`retired_byte_word`), and `octeti` is exactly `lista<b8>`. Assertions are structured (issue identity),
//! never rendered prose.

use crate::test_support;

const INIT_MISMATCH: &str = "initializer_annotation_mismatch";

#[test]
fn ascii_scalar_index_binds_littera() {
    test_support::assert_accepts_la(
        "functio f(ascii s) → vacuum {
    sit littera c ← s[0]
    nota c
}",
    );
}

#[test]
fn ascii_scalar_index_is_not_ascii_one() {
    test_support::assert_rejects_with_issue_la(
        "functio f(ascii s) → vacuum {
    sit ascii<1> c ← s[0]
    nota c
}",
        INIT_MISMATCH,
    );
}

#[test]
fn bounded_ascii_scalar_index_binds_littera() {
    test_support::assert_accepts_la(
        "functio f(ascii<4> s) → vacuum {
    sit littera c ← s[1]
    nota c
}",
    );
}

/// F5 held: `textus[i]` keeps today's `ascii<1>` row.
#[test]
fn textus_scalar_index_keeps_ascii_one() {
    test_support::assert_accepts_la(
        "functio f(textus s) → vacuum {
    sit ascii<1> c ← s[0]
    nota c
}",
    );
}

#[test]
fn itera_ex_textus_binds_littera() {
    test_support::assert_accepts_la(
        "functio f(textus s) → vacuum {
    itera ex s fixum c {
        sit littera x ← c
        nota x
    }
}",
    );
}

#[test]
fn itera_ex_textus_element_is_not_ascii_one() {
    test_support::assert_rejects_with_issue_la(
        "functio f(textus s) → vacuum {
    itera ex s fixum c {
        sit ascii<1> x ← c
        nota x
    }
}",
        INIT_MISMATCH,
    );
}

#[test]
fn itera_ex_ascii_binds_littera() {
    test_support::assert_accepts_la(
        "functio f(ascii s) → vacuum {
    itera ex s fixum c {
        sit littera x ← c
        nota x
    }
}",
    );
}

#[test]
fn textus_cursor_yields_littera() {
    test_support::assert_accepts_la(
        "functio f(textus s) → cursor<littera> {
    redde s.cursor()
}",
    );
}

#[test]
fn ascii_cursor_yields_littera() {
    test_support::assert_accepts_la(
        "functio f(ascii s) → cursor<littera> {
    redde s.cursor()
}",
    );
}

#[test]
fn littera_equals_char_literal_either_side() {
    test_support::assert_accepts_la(
        "functio f(textus s) → numerus {
    varia numerus n ← 0
    itera ex s fixum c {
        si c ≡ 'o' { n ← n + 1 }
        si 'a' ≡ c { n ← n + 1 }
        si c ≢ 'z' { n ← n + 1 }
    }
    redde n
}",
    );
}

#[test]
fn littera_orders_against_char_literal() {
    test_support::assert_accepts_la(
        "functio f(littera c) → bivalens {
    redde c ≥ 'a' et c ≤ 'z'
}",
    );
}

#[test]
fn littera_equality_with_textus_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio f(littera c, textus t) → bivalens {
    redde c ≡ t
}",
        "equality_exact_types_required",
    );
}

#[test]
fn ascii_one_widens_to_littera() {
    test_support::assert_accepts_la(
        "incipit {
    sit littera c ← 'x'
    nota c
}",
    );
}

#[test]
fn multi_char_ascii_does_not_widen_to_littera() {
    test_support::assert_rejects_with_issue_la(
        "incipit {
    sit littera c ← 'xy'
    nota c
}",
        INIT_MISMATCH,
    );
}

#[test]
fn littera_widens_to_textus() {
    test_support::assert_accepts_la(
        "functio f(littera c) → textus {
    sit textus t ← c
    redde t
}",
    );
}

#[test]
fn textus_does_not_narrow_to_littera() {
    test_support::assert_rejects_with_issue_la(
        "functio f(textus t) → vacuum {
    sit littera c ← t
    nota c
}",
        INIT_MISMATCH,
    );
}

#[test]
fn littera_converts_to_textus() {
    test_support::assert_accepts_la(
        "functio f(littera c) → textus {
    redde c ↦ textus
}",
    );
}

/// Need (ruling B8): the byte scalar word is retired; the issue
/// names both replacements (`b8` raw bits, `u8` number) in every pack.
#[test]
fn octetus_is_retired_with_named_issue() {
    test_support::assert_rejects_with_issue_la(
        "functio f(octetus b) → vacuum {
    nota b
}",
        "retired_byte_word",
    );
}

/// Need (ruling B9): the `octeti` word resolves to `lista<b8>`.
#[test]
fn octeti_resolves_to_lista_b8() {
    test_support::assert_accepts_la(
        "functio f(octeti b) → vacuum {
    sit lista<b8> c ← b
    nota c
}",
    );
}

#[test]
fn octeti_elements_are_b8() {
    test_support::assert_accepts_la(
        "functio f(octeti b) → vacuum {
    itera ex b fixum x {
        sit b8 y ← x
        nota y
    }
}",
    );
}

/// Identity, not mutual assignability — exact equality accepts the pair
/// because they are one type (`octeti` is exactly `lista<b8>`).
#[test]
fn octeti_is_exactly_lista_b8() {
    test_support::assert_accepts_la(
        "functio f(octeti a, lista<b8> b) → bivalens {
    redde a ≡ b
}",
    );
}

#[test]
fn textus_cursor_is_not_cursor_of_ascii_one() {
    test_support::assert_rejects_with_issue_la(
        "functio f(textus s) → vacuum {
    sit cursor<ascii<1>> k ← s.cursor()
    nota k
}",
        INIT_MISMATCH,
    );
}
