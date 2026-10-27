//! D10.6: `littera` means a one-letter string, so its conversions are text
//! conversions. A number converted to any text type is its digits; the code
//! point is reached only through the `Code` hint. Failable rows follow D17
//! (`⇥` propagation or `⊥` default): a bare failable row inside a function
//! whose channel is not text is rejected with the conversio channel issue,
//! while an infallible row is accepted there. Assertions are structured.

use crate::test_support;

const UNSUPPORTED: &str = "unsupported_conversio";
const CHANNEL_TEXTUS: &str = "conversio_failure_channel_textus_required";
const CODE_PAIR: &str = "code_hint_pair_mismatch";

// --- n ↦ ascii: digits, never fails ---------------------------------------

#[test]
fn numerus_to_ascii_accepts() {
    test_support::assert_accepts("functio f(numerus n) → ascii { redde n ↦ ascii }");
}

#[test]
fn numerus_to_ascii_is_infallible() {
    test_support::assert_accepts("functio f(numerus n) → ascii ⇥ numerus { redde n ↦ ascii }");
}

#[test]
fn sized_numerus_to_ascii_accepts() {
    test_support::assert_accepts("functio f(u8 n) → ascii { redde n ↦ ascii }");
}

// --- n ↦ littera: one digit, fails outside 0–9 ----------------------------

#[test]
fn numerus_to_littera_accepts_with_default() {
    test_support::assert_accepts("functio f(numerus n) → littera { redde n ↦ littera ⊥ '?' }");
}

#[test]
fn numerus_to_littera_propagates_through_text_channel() {
    test_support::assert_accepts("functio f(numerus n) → littera ⇥ textus { redde n ↦ littera }");
}

#[test]
fn numerus_to_littera_is_failable() {
    test_support::assert_rejects_with_issue(
        "functio f(numerus n) → littera ⇥ numerus { redde n ↦ littera }",
        CHANNEL_TEXTUS,
    );
}

/// A `u8` number converts by the digits rule (the scalar byte word is
/// retired; a raw byte is `b8`, a number is `u8`).
#[test]
fn u8_to_littera_is_digits_rule() {
    test_support::assert_accepts("functio f(u8 b) → littera { redde b ↦ littera ⊥ '?' }");
}

#[test]
fn fractus_to_littera_rejects() {
    test_support::assert_rejects_with_issue(
        "functio f(fractus x) → littera { redde x ↦ littera ⊥ '?' }",
        UNSUPPORTED,
    );
}

// --- littera ↦ numerus: parse the digit, fails otherwise -------------------

#[test]
fn littera_to_numerus_accepts_with_default() {
    test_support::assert_accepts("functio f(littera c) → numerus { redde c ↦ numerus ⊥ 0 }");
}

#[test]
fn littera_to_sized_numerus_accepts() {
    test_support::assert_accepts("functio f(littera c) → u8 { redde c ↦ u8 ⊥ 0 }");
}

#[test]
fn littera_to_numerus_is_failable() {
    test_support::assert_rejects_with_issue(
        "functio f(littera c) → numerus ⇥ numerus { redde c ↦ numerus }",
        CHANNEL_TEXTUS,
    );
}

#[test]
fn littera_to_fractus_rejects() {
    test_support::assert_rejects_with_issue(
        "functio f(littera c) → fractus { redde c ↦ fractus ⊥ 0.0 }",
        UNSUPPORTED,
    );
}

// --- littera ↦ textus: one-letter string, never fails ----------------------

#[test]
fn littera_to_textus_is_infallible() {
    test_support::assert_accepts("functio f(littera c) → textus ⇥ numerus { redde c ↦ textus }");
}

// --- textus ↦ littera: the only letter, fails unless exactly one -----------

#[test]
fn textus_to_littera_accepts_with_default() {
    test_support::assert_accepts("functio f(textus s) → littera { redde s ↦ littera ⊥ '?' }");
}

#[test]
fn textus_to_littera_is_failable() {
    test_support::assert_rejects_with_issue(
        "functio f(textus s) → littera ⇥ numerus { redde s ↦ littera }",
        CHANNEL_TEXTUS,
    );
}

// --- octeti ↦ textus / ascii: failable decode (already shipped) ------------

#[test]
fn octeti_window_to_ascii_accepts() {
    test_support::assert_accepts(
        "functio f(lista<b8> b) → ascii { redde b[0‥1] ↦ ascii ⊥ 'nihil' }",
    );
}

// --- Code hint: code points --------------------------------------------------

#[test]
fn littera_to_u32_code_accepts() {
    test_support::assert_accepts("functio f(littera c) → u32 { redde c ↦ u32 via Code }");
}

#[test]
fn littera_to_u32_code_is_infallible() {
    test_support::assert_accepts("functio f(littera c) → u32 ⇥ numerus { redde c ↦ u32 via Code }");
}

#[test]
fn numerus_to_littera_code_accepts_with_default() {
    test_support::assert_accepts(
        "functio f(numerus n) → littera { redde n ↦ littera via Code ⊥ '?' }",
    );
}

#[test]
fn numerus_to_littera_code_is_failable() {
    test_support::assert_rejects_with_issue(
        "functio f(numerus n) → littera ⇥ numerus { redde n ↦ littera via Code }",
        CHANNEL_TEXTUS,
    );
}

#[test]
fn code_hint_on_other_width_rejects() {
    test_support::assert_rejects_with_issue(
        "functio f(littera c) → i64 { redde c ↦ i64 via Code }",
        CODE_PAIR,
    );
}

#[test]
fn code_hint_on_text_source_rejects() {
    test_support::assert_rejects_with_issue(
        "functio f(textus s) → u32 { redde s ↦ u32 via Code ⊥ 0 }",
        CODE_PAIR,
    );
}

#[test]
fn code_hint_on_textus_to_littera_rejects() {
    test_support::assert_rejects_with_issue(
        "functio f(textus s) → littera { redde s ↦ littera via Code ⊥ '?' }",
        CODE_PAIR,
    );
}

/// `Code` is a `via` hint word only (D11.9): it names no type, so outside a
/// `↦` conversion it is an unknown type name, bare or as a type argument.
#[test]
fn code_is_not_a_declaration_type() {
    test_support::assert_rejects_with_issue(
        "functio f(Code c) → vacuum { nota c }",
        "unknown_type",
    );
    test_support::assert_rejects_with_issue(
        "functio f(littera<Code> c) → vacuum { nota c }",
        "unknown_type",
    );
}
