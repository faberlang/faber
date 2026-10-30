//! `capta` capture-boundary tests (type-algebra UT).
//!
//! `capta { ... }` is the third legal answer to a failable call: it reifies
//! the error channel into the built-in tagged genus `trapped<R, T>` (ruling
//! f9d8a611): `R` is the body's success type, `T` the union of error types
//! that can escape the body, and the result is read through the `returned`
//! / `thrown` variant arms of an ordinary `discerne`. Typing is all reuse —
//! the U-02 union-hole accumulation arms carry the `T` slot, the existing
//! finalize solve normalizes it, and the payload bindings substitute the
//! application's witnesses (U-D1 pattern twin).
//!
//! Fixtures use latin spellings (`functio`, `fixum`, `capta`, `iace`,
//! `redde`, `nota`, `discerne`, `casu`, `sit`) because the radix unit-test
//! harness compiles bare source without the en-locale lex pack.

use super::test_support::{self, entry_local_tys};
use crate::types::{Primitive, Type};

// ---------------------------------------------------------------------------
// The third answer: trap accepts a failable call
// ---------------------------------------------------------------------------

#[test]
fn trap_binds_inference_hole_to_tagged_result() {
    // The canonical idiom: `sit _ r ← capta { f() }` checks green with
    // `r : trapped<numerus, textus>` — `R` from the body, `T` from the one
    // escaping error type. Separate slots: the variants carry the
    // discrimination, not the member set.
    let result = test_support::analyze_ok(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "incipit {\n    sit _ r ← capta { f() }\n    nota r\n}\n",
    ));
    let tys = entry_local_tys(&result);
    let [local_ty] = tys[..] else {
        panic!("expected exactly one incipit local, got {tys:?}");
    };
    let (success, error) = trapped_slots(&result, local_ty);
    assert!(
        is_primitive(&result, success, Primitive::Numerus),
        "R must be the body's success type"
    );
    assert!(
        is_primitive(&result, error, Primitive::Textus),
        "T must be the escaping error type"
    );
}

#[test]
fn trap_union_hole_annotation_solves_to_tagged_result() {
    // `sit ∪ r ← capta { f() }`: the `∪` annotation no longer owns an
    // accumulation (there is no result union to accumulate into); the tagged
    // result flows in as the hole's single member, and the ordinary WARN017
    // cross-shape warning names the `∪` that solved to one type.
    let result = test_support::analyze_source(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "incipit {\n    sit ∪ r ← capta { f() }\n    nota r\n}\n",
    ));
    assert!(
        !result.diagnostics.iter().any(|diag| diag.is_error()),
        "a ∪-annotated trap binding must check green: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diag| !diag.is_error()
            && test_support::issue(diag) == Some("union_hole_solved_monomorphically")),
        "the ∪ annotation solved to the one tagged result type: {:?}",
        result.diagnostics
    );
    let tys = entry_local_tys(&result);
    let (success, error) = trapped_slots(&result, tys[0]);
    assert!(is_primitive(&result, success, Primitive::Numerus));
    assert!(is_primitive(&result, error, Primitive::Textus));
}

#[test]
fn trap_of_vacuum_success_keeps_the_success_slot_explicit() {
    // `capta` over `→ vacuum ⇥ E`: the `R` slot stays `vacuum` (it is a slot,
    // not a union member to normalize away), so `returned { value }` and
    // `thrown { error }` stay distinguishable whatever the two types are.
    let result = test_support::analyze_ok(concat!(
        "functio f() → vacuum ⇥ textus {\n    redde\n}\n\n",
        "incipit {\n    sit _ r ← capta { f() }\n    nota r\n}\n",
    ));
    let tys = entry_local_tys(&result);
    let (success, error) = trapped_slots(&result, tys[0]);
    assert!(
        is_primitive(&result, success, Primitive::Vacuum),
        "vacuum success must stay the explicit R slot"
    );
    assert!(is_primitive(&result, error, Primitive::Textus));
}

#[test]
fn trap_supports_explicit_trapped_annotation() {
    // The ruled two-slot idiom over a trap: annotate `trapped<R, T>` and read
    // the result through the variant arms.
    test_support::assert_accepts(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "incipit {\n",
        "    sit trapped<numerus, textus> r ← capta { f() }\n",
        "    discerne r {\n",
        "        casu returned sit value { nota value }\n",
        "        casu thrown sit error { nota error }\n",
        "    }\n",
        "}\n",
    ));
}

#[test]
fn trap_explicit_union_annotation_mismatches_the_tagged_result() {
    // The retired form spelled its annotation `T ∪ E`; the tagged result is
    // not that union, so the ordinary initializer check reports the mismatch.
    test_support::assert_rejects_with_issue(
        concat!(
            "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
            "incipit {\n",
            "    sit numerus ∪ textus r ← capta { f() }\n",
            "    nota r\n",
            "}\n",
        ),
        "initializer_annotation_mismatch",
    );
}

#[test]
fn trap_variant_arms_bind_the_applied_payload_witnesses() {
    // The payload bindings carry the application's witnesses (U-D1 pattern
    // twin): `value : numerus` inside `casu returned`, `error : textus`
    // inside `casu thrown` — never the declared `R`/`T` parameters.
    test_support::assert_accepts(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "incipit {\n",
        "    sit _ r ← capta { f() }\n",
        "    discerne r {\n",
        "        casu returned sit value { nota value + 1 }\n",
        "        casu thrown sit error { nota error.longitudo() }\n",
        "    }\n",
        "}\n",
    ));
    // And the witness is real: a textus-only member access inside the
    // `returned` arm is a typed error, not an open hole.
    test_support::assert_rejects_with_issue(
        concat!(
            "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
            "incipit {\n",
            "    sit _ r ← capta { f() }\n",
            "    discerne r {\n",
            "        casu returned sit value { nota value.longitudo() }\n",
            "        casu thrown sit error { nota error }\n",
            "    }\n",
            "}\n",
        ),
        "unknown_method",
    );
}

#[test]
fn trap_captures_iace_payload() {
    // An explicit `iace` inside the body is an alternate exit toward the
    // same `T` slot — no channel-specific typing.
    let result = test_support::analyze_ok(concat!(
        "functio f() → numerus ⇥ textus {\n    iace \"boom\"\n}\n\n",
        "incipit {\n    sit _ r ← capta { f() }\n    nota r\n}\n",
    ));
    let tys = entry_local_tys(&result);
    let (success, error) = trapped_slots(&result, tys[0]);
    assert!(is_primitive(&result, success, Primitive::Numerus));
    assert!(is_primitive(&result, error, Primitive::Textus));
}

#[test]
fn trap_accumulates_each_escaping_error_type_into_the_t_slot() {
    // Two failable calls with different error channels: `T` is the union of
    // the escaping error types.
    let result = test_support::analyze_ok(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "functio g() → numerus ⇥ numerus {\n    redde 7\n}\n\n",
        "incipit {\n    sit _ r ← capta { f() }\n    sit _ s ← capta { g() }\n    nota r\n    nota s\n}\n",
    ));
    let tys = entry_local_tys(&result);
    let (_, error) = trapped_slots(&result, tys[0]);
    assert!(
        is_primitive(&result, error, Primitive::Textus),
        "one escaping error type solves to that type, not a singleton union"
    );
    // A body whose two calls throw different error types unions them.
    let result = test_support::analyze_ok(concat!(
        "functio f() → numerus ⇥ textus {\n    iace \"x\"\n}\n\n",
        "functio g() → numerus ⇥ numerus {\n    iace 7\n}\n\n",
        "incipit {\n",
        "    sit _ r ← capta {\n",
        "        si verum { f() } secus { g() }\n",
        "    }\n",
        "    nota r\n",
        "}\n",
    ));
    let tys = entry_local_tys(&result);
    let (_, error) = trapped_slots(&result, tys[0]);
    match result.types.get(error) {
        Type::Union(members) => {
            assert!(
                members
                    .iter()
                    .any(|ty| is_primitive(&result, *ty, Primitive::Textus)),
                "textus error member: {members:?}"
            );
            assert!(
                members
                    .iter()
                    .any(|ty| is_primitive(&result, *ty, Primitive::Numerus)),
                "numerus error member: {members:?}"
            );
        }
        other => panic!("expected a two-member error union, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The other two answers still stand
// ---------------------------------------------------------------------------

#[test]
fn bare_failable_call_still_requires_one_of_three_answers() {
    // Absent all three answers (⇥ declaration, fac/cape handler, capta
    // trap), a bare failable call in a non-⇥ function still fails SEM011.
    test_support::assert_rejects_with_issue(
        concat!(
            "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
            "functio g() → vacuum {\n    f()\n}\n",
        ),
        "failable_call_requires_handler",
    );
}

#[test]
fn trap_does_not_leak_past_its_boundary() {
    // A failable call after the trap body (outside the boundary) still
    // rejects: the sink is restored, not widened.
    test_support::assert_rejects_with_issue(
        concat!(
            "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
            "functio g() → vacuum {\n",
            "    sit _ r ← capta { f() }\n",
            "    nota r\n",
            "    f()\n",
            "}\n",
        ),
        "failable_call_requires_handler",
    );
}

// ---------------------------------------------------------------------------
// T ≡ E: legal under the tagged form (the union-collapse guard is retired)
// ---------------------------------------------------------------------------

#[test]
fn trap_of_same_success_and_error_type_is_legal() {
    // Ruling f9d8a611: `R` and `T` are separate slots, so a success and an
    // error of the same type stay distinguishable — the variants carry the
    // discrimination. The `trap_union_collapsed` guard retired with the
    // untagged union result.
    test_support::assert_accepts(concat!(
        "genus Pondero {\n    varia textus nuntius\n}\n\n",
        "functio f() → Pondero ⇥ Pondero {\n    redde Pondero { nuntius = \"x\" }\n}\n\n",
        "incipit {\n    sit _ r ← capta { f() }\n    nota r\n}\n",
    ));
}

#[test]
fn trap_partial_overlap_stays_legal() {
    // T and E share a member: legal, exactly as before — each slot keeps its
    // own shape and the variants keep them apart.
    test_support::assert_accepts(concat!(
        "functio f() → numerus ⇥ numerus ∪ textus {\n    redde 1\n}\n\n",
        "incipit {\n    sit _ r ← capta { f() }\n    nota r\n}\n",
    ));
}

// ---------------------------------------------------------------------------
// Infallible body: the empty error channel warns, and types as numquam
// ---------------------------------------------------------------------------

#[test]
fn trap_of_infallible_body_warns_and_empties_the_error_channel() {
    // No error flow at all: nothing can escape through the `T` slot, so it
    // types as `numquam` (the `thrown` variant is uninhabited) and the
    // dedicated `trap_error_channel_empty` warning fires instead of an
    // inference error.
    let result = test_support::analyze_source(concat!(
        "functio f() → numerus {\n    redde 42\n}\n\n",
        "incipit {\n    sit _ r ← capta { f() }\n    nota r\n}\n",
    ));
    assert!(
        !result.diagnostics.iter().any(|diag| diag.is_error()),
        "an infallible trap body is legal, only warned: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diag| !diag.is_error()
            && test_support::issue(diag) == Some("trap_error_channel_empty")),
        "expected the empty-channel warning for the infallible trap body, got: {:?}",
        result.diagnostics
    );
    let tys = entry_local_tys(&result);
    let (success, error) = trapped_slots(&result, tys[0]);
    assert!(is_primitive(&result, success, Primitive::Numerus));
    assert!(
        is_primitive(&result, error, Primitive::Numquam),
        "the empty error channel types as numquam"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Destructure `trapped<R, T>` into its two slots.
fn trapped_slots(
    result: &test_support::AnalyzedFixture,
    ty: crate::TypeId,
) -> (crate::TypeId, crate::TypeId) {
    let Type::Applied(base, args) = result.types.get(ty).clone() else {
        panic!(
            "expected the tagged trapped application, got {:?}",
            result.types.get(ty)
        );
    };
    assert!(
        matches!(result.types.get(base), Type::Enum(_)),
        "the trapped application's base must be the builtin genus"
    );
    let [
        radix_types::AppliedArg::Type(success),
        radix_types::AppliedArg::Type(error),
    ] = args[..]
    else {
        panic!("trapped takes exactly two type arguments");
    };
    (success, error)
}

fn is_primitive(
    result: &test_support::AnalyzedFixture,
    ty: crate::TypeId,
    primitive: Primitive,
) -> bool {
    matches!(result.types.get(ty), Type::Primitive(found) if *found == primitive)
}
