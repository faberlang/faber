//! Need: a float or decimal does not convert to an integer bare.
//! The rounding is named with `via Floor|Ceil|HalfEven|Trunc`; the hint is
//! refused on every other pairing.

use crate::test_support;

const REQUIRES: &str = "float_to_integer_requires_rounding";
const UNKNOWN_HINT: &str = "conversio_via_unknown_hint";

const INTEGER_TARGETS: [&str; 11] = [
    "numerus",
    "i8",
    "i16",
    "i32",
    "i64",
    "u8",
    "u16",
    "u32",
    "u64",
    "modulus<u8>",
    "saturatus<u8>",
];

#[test]
fn bare_float_to_every_integer_target_is_the_named_error() {
    for source in ["f64", "f32", "fractus"] {
        for target in INTEGER_TARGETS {
            test_support::assert_rejects_with_issue(
                &format!("functio r({source} x) → {target} {{ redde x ↦ {target} }}"),
                REQUIRES,
            );
        }
    }
}

#[test]
fn bare_decimal_to_every_integer_target_is_the_named_error() {
    for target in INTEGER_TARGETS {
        test_support::assert_rejects_with_issue(
            &format!("functio r(d64 x) → {target} {{ redde x ↦ {target} }}"),
            REQUIRES,
        );
    }
}

#[test]
fn bare_float_or_decimal_to_inf_is_the_named_error() {
    test_support::assert_rejects_with_issue("functio r(f64 x) → inf { redde x ↦ inf }", REQUIRES);
    test_support::assert_rejects_with_issue("functio r(d64 x) → inf { redde x ↦ inf }", REQUIRES);
}

#[test]
fn float_literal_to_integer_is_the_named_error() {
    test_support::assert_rejects_with_issue(
        "functio r() → numerus ⇥ textus { redde 3.7 ↦ numerus }",
        REQUIRES,
    );
}

#[test]
fn named_error_help_names_the_glyphs_and_the_via_forms() {
    let help = crate::catalog_semantic::semantic_help(
        crate::error::SemanticErrorKind::ConversionInvalid,
        &[radix_diagnostics::DiagnosticArg::new("issue", REQUIRES)],
    )
    .expect("catalog carries help for the rounding error");
    for needle in [
        "⌊x⌋",
        "⌈x⌉",
        "⌊x⌉",
        "via Floor",
        "via Ceil",
        "via HalfEven",
        "via Trunc",
    ] {
        assert!(help.contains(needle), "help must name {needle}: {help}");
    }
}

#[test]
fn each_rounding_hint_is_accepted_on_float_to_integer() {
    for hint in ["Floor", "Ceil", "HalfEven", "Trunc"] {
        for source in ["f64", "f32", "d64"] {
            test_support::assert_accepts(&format!(
                "functio r({source} x) → i64 ⇥ textus {{ redde x ↦ i64 via {hint} }}"
            ));
        }
    }
}

#[test]
fn rounding_hint_is_accepted_on_every_integer_target() {
    for target in INTEGER_TARGETS {
        test_support::assert_accepts(&format!(
            "functio r(f64 x) → {target} ⇥ textus {{ redde x ↦ {target} via Floor }}"
        ));
        test_support::assert_accepts(&format!(
            "functio r(d64 x) → {target} ⇥ textus {{ redde x ↦ {target} via HalfEven }}"
        ));
    }
}

#[test]
fn rounding_hint_is_accepted_on_inf() {
    test_support::assert_accepts("functio r(f64 x) → inf ⇥ textus { redde x ↦ inf via Trunc }");
    test_support::assert_accepts("functio r(d64 x) → inf ⇥ textus { redde x ↦ inf via Ceil }");
}

#[test]
fn rounded_conversion_keeps_its_failure_channel() {
    // The checked form is failable: no `⇥` channel and no `⊥` default is the
    // same error a bare failable conversion reported before the ruling.
    test_support::assert_accepts("functio r(f64 x) → i32 { redde x ↦ i32 via Floor ⊥ 0 }");
    test_support::assert_rejects_with_issue(
        "functio r(f64 x) → i32 { redde x ↦ i32 via Floor }",
        "failable_conversio_requires_handler",
    );
}

#[test]
fn rounding_hint_is_refused_off_the_float_to_integer_pairing() {
    for (param, target) in [
        ("i64 n", "i32"),
        ("textus s", "i64"),
        ("textus s", "f64"),
        ("i64 n", "f64"),
        ("f64 x", "f32"),
        ("d64 x", "f64"),
        ("f64 x", "d64"),
        ("i64 n", "d64"),
    ] {
        test_support::assert_rejects_with_issue(
            &format!(
                "functio r({param}) → {target} ⇥ textus {{ redde {} ↦ {target} via Floor }}",
                param.split(' ').nth(1).unwrap()
            ),
            UNKNOWN_HINT,
        );
    }
}

#[test]
fn integer_and_float_to_float_and_decimal_conversions_are_unchanged() {
    test_support::assert_accepts("functio r(i32 n) → i64 { redde n ↦ i64 }");
    test_support::assert_accepts("functio r(i64 n) → f64 { redde n ↦ f64 }");
    test_support::assert_accepts("functio r(f32 x) → f64 { redde x ↦ f64 }");
    test_support::assert_accepts("functio r(d64 x) → d64 { redde x ↦ d64 }");
}
