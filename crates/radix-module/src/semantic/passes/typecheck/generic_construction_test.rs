//! Generic user-type construction instantiates type parameters (U-D1).
//!
//! Mirrors `tela/spike/defects/d1-generic-construction.fab` in current
//! language spelling (`discretio` + `functio` + `finge`): a variant whose
//! payload references the union's type parameter must typecheck against the
//! substituted witness, and the construction yields the applied union type.
//! Generic class construction (`genus C<T>` + `C { ... } ∷ C<numerus>`) follows
//! the same rule.
//!
//! The field-substitution + applied-result behavior must not regress
//! non-generic variant/class construction; the negative cases prove wrong
//! witnesses still error at the field.

use super::test_support;

// ---------------------------------------------------------------------------
// D1 repro: generic union construction (done_when a)
// ---------------------------------------------------------------------------

#[cfg(feature = "hir-rust")]
#[test]
fn generic_union_variant_construction_with_applied_ascription_typechecks() {
    // `variant Plena { res = 1 } ∷ Cista<numerus>` — the applied ascription
    // drives `T ↦ numerus` before the `res` payload is checked.
    test_support::assert_accepts(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = 1 } ∷ Cista<numerus>
}",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn generic_union_variant_construction_bare_ascription_with_expected_type_typechecks() {
    // Bare-name ascription against a generic expected type (done_when b):
    // `∷ Cista` adopts the enclosing return type's witnesses.
    test_support::assert_accepts(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = 1 } ∷ Cista
}",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn generic_union_variant_construction_return_directed_typechecks() {
    // Return-type-directed `variant Plena { res = 1 }` (no ascription at all).
    test_support::assert_accepts(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = 1 }
}",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn generic_union_variant_construction_binds_to_applied_expected_type() {
    // The construction result is the applied union type: it binds into an
    // `Cista<numerus>` slot.
    test_support::assert_accepts(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    fixum Cista<numerus> x ← finge Plena { res = 1 }
    redde x
}",
    );
}

#[test]
fn generic_union_variant_construction_rejects_mismatched_field_witness() {
    // The witness substitution is real: a `textus` payload cannot flow into
    // `T ↦ numerus`.
    test_support::assert_rejects_with_issue(
        r#"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = "x" } ∷ Cista<numerus>
}"#,
        "argument_type_mismatch",
    );
}

#[test]
fn generic_union_variant_construction_result_keeps_applied_identity() {
    // The applied witness survives the construction: the value built with
    // `Cista<numerus>` must not bind into a differently-applied slot.
    test_support::assert_rejects_with_issue(
        r#"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = 1 } ∷ Cista<numerus>
}

functio wrong(Cista<textus> value) → textus {
    redde "x"
}

incipit {
    fixum Cista<textus> x ← wrong(facere())
    nota x
}"#,
        "argument_type_mismatch",
    );
}

// ---------------------------------------------------------------------------
// Generic class construction (done_when c)
// ---------------------------------------------------------------------------

#[cfg(feature = "hir-rust")]
#[test]
fn generic_class_construction_with_applied_ascription_typechecks() {
    // `(C { value = 1 }) ∷ C<numerus>` — the applied target substitutes `T ↦
    // numerus` into the field before the initializer is checked.
    test_support::assert_accepts(
        r"genus C<T> {
    T value
}

functio facere() → C<numerus> {
    redde (C { value = 1 }) ∷ C<numerus>
}",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn generic_class_construction_return_directed_typechecks() {
    // Return-type-directed `C { value = 1 }` (no ascription).
    test_support::assert_accepts(
        r"genus C<T> {
    T value
}

functio facere() → C<numerus> {
    redde C { value = 1 }
}",
    );
}

#[test]
fn generic_class_construction_rejects_mismatched_field_witness() {
    test_support::assert_rejects_with_issue(
        r#"genus C<T> {
    T value
}

functio facere() → C<numerus> {
    redde (C { value = "x" }) ∷ C<numerus>
}"#,
        "field_initializer_type_mismatch",
    );
}

// ---------------------------------------------------------------------------
// Non-generic regression surface (no substitution, no applied result)
// ---------------------------------------------------------------------------

#[cfg(feature = "hir-rust")]
#[test]
fn non_generic_variant_construction_unchanged() {
    test_support::assert_accepts(
        r"discretio Eventum {
    Ictus { numerus vis }
    Sonitus
}

functio facere() → Eventum {
    redde finge Ictus { vis = 3 } ∷ Eventum
}

incipit {
    fixum Eventum a ← finge Ictus { vis = 4 } ∷ Eventum
    fixum Eventum b ← finge Eventum.Sonitus { } ∷ Eventum
    nota a
    nota b
}",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn non_generic_class_construction_unchanged() {
    test_support::assert_accepts(
        r"genus Plana {
    numerus latitudo
}

functio facere() → Plana {
    redde (Plana { latitudo = 3 }) ∷ Plana
}",
    );
}

#[test]
fn non_generic_variant_construction_wrong_payload_still_rejected() {
    test_support::assert_rejects_with_issue(
        r#"discretio Eventum {
    Ictus { numerus vis }
    Sonitus
}

functio facere() → Eventum {
    redde finge Ictus { vis = "x" } ∷ Eventum
}"#,
        "argument_type_mismatch",
    );
}
