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

use crate::test_support;

// ---------------------------------------------------------------------------
// D1 repro: generic union construction (done_when a)
// ---------------------------------------------------------------------------

#[test]
fn generic_union_variant_construction_with_applied_ascription_typechecks() {
    // `variant Plena { res = 1 } ∷ Cista<numerus>` — the applied ascription
    // drives `T ↦ numerus` before the `res` payload is checked.
    test_support::assert_accepts_la(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = 1 } ∷ Cista<numerus>
}",
    );
}

#[test]
fn generic_union_variant_construction_bare_ascription_with_expected_type_typechecks() {
    // Bare-name ascription against a generic expected type (done_when b):
    // `∷ Cista` adopts the enclosing return type's witnesses.
    test_support::assert_accepts_la(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = 1 } ∷ Cista
}",
    );
}

#[test]
fn generic_union_variant_construction_return_directed_typechecks() {
    // Return-type-directed `variant Plena { res = 1 }` (no ascription at all).
    test_support::assert_accepts_la(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = 1 }
}",
    );
}

#[test]
fn generic_union_variant_construction_binds_to_applied_expected_type() {
    // The construction result is the applied union type: it binds into an
    // `Cista<numerus>` slot.
    test_support::assert_accepts_la(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    sit Cista<numerus> x ← finge Plena { res = 1 }
    redde x
}",
    );
}

#[test]
fn generic_union_variant_construction_rejects_mismatched_field_witness() {
    // The witness substitution is real: a `textus` payload cannot flow into
    // `T ↦ numerus`.
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_rejects_with_issue_la(
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
    sit Cista<textus> x ← wrong(facere())
    nota x
}"#,
        "argument_type_mismatch",
    );
}

// ---------------------------------------------------------------------------
// Generic class construction (done_when c)
// ---------------------------------------------------------------------------

#[test]
fn generic_class_construction_with_applied_ascription_typechecks() {
    // `(C { value = 1 }) ∷ C<numerus>` — the applied target substitutes `T ↦
    // numerus` into the field before the initializer is checked.
    test_support::assert_accepts_la(
        r"genus C<T> {
    varia T value
}

functio facere() → C<numerus> {
    redde (C { value = 1 }) ∷ C<numerus>
}",
    );
}

#[test]
fn generic_class_construction_return_directed_typechecks() {
    // Return-type-directed `C { value = 1 }` (no ascription).
    test_support::assert_accepts_la(
        r"genus C<T> {
    varia T value
}

functio facere() → C<numerus> {
    redde C { value = 1 }
}",
    );
}

#[test]
fn generic_class_construction_rejects_mismatched_field_witness() {
    test_support::assert_rejects_with_issue_la(
        r#"genus C<T> {
    varia T value
}

functio facere() → C<numerus> {
    redde (C { value = "x" }) ∷ C<numerus>
}"#,
        "field_initializer_type_mismatch",
    );
}

#[test]
fn generic_constructor_type_args_standalone_constructs() {
    // S0-G1: applied heads construct — `Something<numerus> { }` is the
    // construction idiom now, not a `name_not_value` miss. The pre-S0-G1
    // pin rejected this shape; the applied-construction landing flips it.
    test_support::assert_accepts_la(
        r"genus Something<A> {
}

incipit {
    Something<numerus> { }
}",
    );
}

#[test]
fn generic_constructor_type_args_annotated_binding_is_expected_expression() {
    // Direct annotated binding with extra constructor type-args is a parse
    // miss today (PARSE030), not a semantic arity error.
    test_support::assert_rejects_with_issue_la(
        r"genus Something<A> {
}

incipit {
    Something<numerus, textus> s ← Something { }
}",
        "expected_expression",
    );
}

// ---------------------------------------------------------------------------
// Applied-generic nominal record construction (S0-G1) — magnitudo witnesses
// ---------------------------------------------------------------------------

#[test]
fn applied_generic_class_construction_with_size_argument_typechecks() {
    // `Gen<3> { lanes = … }` — the applied size argument reaches the field
    // type before the initializer is checked (S0-G1 done_when 1).
    test_support::assert_accepts_la(
        r"genus Gen<magnitudo N> {
    varia vector<f32, magnitudo N> lanes
}

functio facere(vector<f32, magnitudo 3> lanes) → Gen<3> {
    redde Gen<3> { lanes = lanes }
}",
    );
}

#[test]
fn bare_generic_class_construction_adopts_contextual_size_witness() {
    // Context-instantiated `Gen { lanes = … }` under `→ Gen<3>`: the
    // annotated return type supplies the magnitudo witness (S0-G1
    // done_when 3) — never a silent wrong-arity acceptance.
    test_support::assert_accepts_la(
        r"genus Gen<magnitudo N> {
    varia vector<f32, magnitudo N> lanes
}

functio facere(vector<f32, magnitudo 3> lanes) → Gen<3> {
    redde Gen { lanes = lanes }
}",
    );
}

#[test]
fn applied_generic_class_construction_rejects_wrong_width_initializer() {
    // A wrong-width field initializer on an applied nominal keeps the
    // structured SEM010 identity — negatives posture preserved (S0-G1
    // done_when 2).
    test_support::assert_rejects_with_issue_la(
        r"genus Gen<magnitudo N> {
    varia vector<f32, magnitudo N> lanes
}

functio facere(vector<f32, magnitudo 4> wide) → Gen<3> {
    redde Gen<3> { lanes = wide }
}",
        "field_initializer_type_mismatch",
    );
}

#[test]
fn applied_generic_class_construction_rejects_kind_mismatch() {
    // A type argument in a magnitudo slot keeps the structured SEM008
    // identity at the applied head.
    test_support::assert_rejects_with_issue_la(
        r"genus Gen<magnitudo N> {
    varia vector<f32, magnitudo N> lanes
}

functio facere(vector<f32, magnitudo 3> lanes) → Gen<3> {
    redde Gen<textus> { lanes = lanes }
}",
        "generic_argument_kind_mismatch",
    );
}

#[test]
fn applied_generic_class_construction_rejects_wrong_arity() {
    // Two arguments on a one-parameter nominal keep the structured arity
    // identity at the applied head.
    test_support::assert_rejects_with_issue_la(
        r"genus Gen<magnitudo N> {
    varia vector<f32, magnitudo N> lanes
}

functio facere(vector<f32, magnitudo 3> lanes) → Gen<3> {
    redde Gen<3, 4> { lanes = lanes }
}",
        "generic_argument_arity_mismatch",
    );
}

#[test]
fn type_only_generic_class_construction_with_applied_head_typechecks() {
    // Type-only generic nominals construct with an applied head too
    // (S0-G1 done_when 4) — no type-argument regression.
    test_support::assert_accepts_la(
        r"genus Capsula<T> {
    varia T payload
}

functio facere() → Capsula<numerus> {
    redde Capsula<numerus> { payload = 1 }
}",
    );
}

// ---------------------------------------------------------------------------
// Non-generic regression surface (no substitution, no applied result)
// ---------------------------------------------------------------------------

#[test]
fn non_generic_variant_construction_unchanged() {
    test_support::assert_accepts_la(
        r"discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}

functio facere() → Eventum {
    redde finge Ictus { vis = 3 } ∷ Eventum
}

incipit {
    sit Eventum a ← finge Ictus { vis = 4 } ∷ Eventum
    sit Eventum b ← finge Eventum.Sonitus { } ∷ Eventum
    nota a
    nota b
}",
    );
}

#[test]
fn non_generic_class_construction_unchanged() {
    test_support::assert_accepts_la(
        r"genus Plana {
    varia numerus latitudo
}

functio facere() → Plana {
    redde (Plana { latitudo = 3 }) ∷ Plana
}",
    );
}

#[test]
fn non_generic_variant_construction_wrong_payload_still_rejected() {
    test_support::assert_rejects_with_issue_la(
        r#"discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}

functio facere() → Eventum {
    redde finge Ictus { vis = "x" } ∷ Eventum
}"#,
        "argument_type_mismatch",
    );
}

// --- S0-G8: applied-generic field projection ---

#[test]
fn applied_generic_field_read_substitutes_size_witness() {
    test_support::assert_accepts_la(
        r"genus Capsa<magnitudo N> {
    varia vector<f32, magnitudo N> minima
}

functio minimum(Capsa<3> capsam) → vector<f32, magnitudo 3> {
    sit vector<f32, magnitudo 3> lo ← capsam.minima
    redde lo
}",
    );
}

#[test]
fn applied_generic_field_store_substitutes_size_witness() {
    test_support::assert_accepts_la(
        r"genus Capsa<magnitudo N> {
    varia vector<f32, magnitudo N> minima
}

functio scribe(Capsa<3> capsam, vector<f32, magnitudo 3> valor) → vector<f32, magnitudo 3> {
    varia Capsa<3> c ← capsam
    c.minima ← valor
    redde c.minima
}",
    );
}

// --- explicit variant type arguments ---

#[test]
fn explicit_variant_type_args_build_the_applied_union_type() {
    // `Plena<numerus>(1)` names the one witness; it must not be padded with
    // extra inference variables that make the result differ from
    // `Cista<numerus>`.
    test_support::assert_accepts_la(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde Plena<numerus>(1)
}",
    );
}
