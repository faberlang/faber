//! `capta` capture-boundary tests (type-algebra UT).
//!
//! `capta { ... }` is the third legal answer to a failable call: it reifies
//! the error channel into a value union (`Success ∪ Errata`). Typing is all
//! reuse — the U-02 union-hole accumulation arms, the existing finalize
//! solve (WARN017 for the degenerate single-member case), and the existing
//! normalization law for the synthesized union.
//!
//! Fixtures use latin spellings (`functio`, `fixum`, `capta`, `iace`,
//! `redde`, `nota`) because the radix unit-test harness compiles bare
//! source without the en-locale lex pack.

use super::test_support::{self, entry_local_tys};
use crate::types::{Primitive, Type};

// ---------------------------------------------------------------------------
// The third answer: trap accepts a failable call
// ---------------------------------------------------------------------------

#[test]
fn trap_binds_inference_hole_to_solved_union() {
    // The canonical idiom: `sit r ← capta { f() }` checks green with
    // `r : numerus ∪ textus`. The `_` hole binds to the trap hole, so the
    // binding's finalized type is the solved two-member union.
    let result = test_support::analyze_ok(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "incipit {\n    sit r ← capta { f() }\n    nota r\n}\n",
    ));
    let tys = entry_local_tys(&result);
    let [local_ty] = tys[..] else {
        panic!("expected exactly one incipit local, got {tys:?}");
    };
    let members = union_members(&result, local_ty);
    assert_eq!(members.len(), 2, "trap result must be a two-member union");
    assert!(
        members
            .iter()
            .any(|ty| is_primitive(&result, *ty, Primitive::Numerus))
    );
    assert!(
        members
            .iter()
            .any(|ty| is_primitive(&result, *ty, Primitive::Textus))
    );
}

#[test]
fn trap_with_union_hole_annotation_owns_accumulation() {
    // `sit ∪ r ← capta { f() }`: the annotation hole IS the trap hole, so
    // the solved members land on the binding without an inference round trip.
    let result = test_support::analyze_ok(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "incipit {\n    sit ∪ r ← capta { f() }\n    nota r\n}\n",
    ));
    let tys = entry_local_tys(&result);
    assert_eq!(union_members(&result, tys[0]).len(), 2);
}

#[test]
fn trap_of_vacuum_success_is_distinguishable_two_member_union() {
    // `capta` over `→ vacuum ⇥ E`: vacuum stays an explicit member (only
    // nihil is extracted by the normalization), so the union is two members
    // and success/failure stay distinguishable.
    let result = test_support::analyze_ok(concat!(
        "functio f() → vacuum ⇥ textus {\n    redde\n}\n\n",
        "incipit {\n    sit ∪ r ← capta { f() }\n    nota r\n}\n",
    ));
    let tys = entry_local_tys(&result);
    let members = union_members(&result, tys[0]);
    assert_eq!(
        members.len(),
        2,
        "vacuum success must stay an explicit member"
    );
    assert!(
        members
            .iter()
            .any(|ty| is_primitive(&result, *ty, Primitive::Vacuum))
    );
    assert!(
        members
            .iter()
            .any(|ty| is_primitive(&result, *ty, Primitive::Textus))
    );
}

#[test]
fn trap_supports_explicit_union_annotation_and_narrowing() {
    // The adopted two-line idiom over a trap: an explicit `T ∪ E`
    // annotation, then `est` narrowing (U1R is the recovery path).
    test_support::assert_accepts(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "incipit {\n",
        "    sit numerus ∪ textus r ← capta { f() }\n",
        "    si r est numerus {\n        nota r\n    } secus {\n",
        "        nota r.longitudo()\n    }\n",
        "}\n",
    ));
}

#[test]
fn trap_inferred_hole_narrows_under_is_test() {
    // The same idiom over a `_` binding (no `T ∪ E` annotation): the closed
    // trap hole supplies the members, so the taken branch is `numerus` and the
    // fallthrough is `textus` — `textus` members are reachable only there.
    test_support::assert_accepts(concat!(
        "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
        "incipit {\n",
        "    sit r ← capta { f() }\n",
        "    si r est numerus {\n        nota r\n    } secus {\n",
        "        nota r.longitudo()\n    }\n",
        "}\n",
    ));
}

#[test]
fn trap_inferred_hole_narrowed_branch_is_the_tested_member() {
    // Under narrowing the taken branch of `r est numerus` is `numerus`, so a
    // textus-only member access there is a typed error rather than an open hole
    // that defers to MIR validation.
    test_support::assert_rejects_with_issue(
        concat!(
            "functio f() → numerus ⇥ textus {\n    redde 42\n}\n\n",
            "incipit {\n",
            "    sit r ← capta { f() }\n",
            "    si r est numerus {\n        nota r.longitudo()\n    }\n",
            "}\n",
        ),
        "unknown_method",
    );
}

#[test]
fn trap_captures_iace_payload() {
    // An explicit `iace` inside the body is an alternate exit toward the
    // same trap hole — no channel-specific typing.
    test_support::assert_accepts(concat!(
        "functio f() → numerus ⇥ textus {\n    iace \"boom\"\n}\n\n",
        "incipit {\n    sit ∪ r ← capta { f() }\n    nota r\n}\n",
    ));
    let result = test_support::analyze_ok(concat!(
        "functio f() → numerus ⇥ textus {\n    iace \"boom\"\n}\n\n",
        "incipit {\n    sit ∪ r ← capta { f() }\n    nota r\n}\n",
    ));
    let tys = entry_local_tys(&result);
    assert_eq!(union_members(&result, tys[0]).len(), 2);
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
            "    sit ∪ r ← capta { f() }\n",
            "    nota r\n",
            "    f()\n",
            "}\n",
        ),
        "failable_call_requires_handler",
    );
}

// ---------------------------------------------------------------------------
// T ≡ E collapse: fail-closed with a dedicated diagnostic
// ---------------------------------------------------------------------------

#[test]
fn trap_union_collapse_is_rejected_fail_closed() {
    // Success and error are the same type: the collapsed value cannot
    // distinguish success from failure, the trap contract is unstateable.
    test_support::assert_rejects_with_issue(
        concat!(
            "genus Pondero {\n    varia textus nuntius\n}\n\n",
            "functio f() → Pondero ⇥ Pondero {\n    redde Pondero { nuntius = \"x\" }\n}\n\n",
            "incipit {\n    sit ∪ r ← capta { f() }\n    nota r\n}\n",
        ),
        "trap_union_collapsed",
    );
}

#[test]
fn trap_partial_overlap_stays_legal() {
    // T and E share a member but the synthesized union keeps ≥2 members:
    // legal. Narrowing keeps it sound; the shared-member ambiguity is the
    // shape the programmer declared.
    test_support::assert_accepts(concat!(
        "functio f() → numerus ⇥ numerus ∪ textus {\n    redde 1\n}\n\n",
        "incipit {\n    sit ∪ r ← capta { f() }\n    nota r\n}\n",
    ));
}

// ---------------------------------------------------------------------------
// Degenerate solve: WARN017, not an error
// ---------------------------------------------------------------------------

#[test]
fn trap_of_infallible_body_solves_degenerately_with_warn017() {
    // No error flow at all: the trap hole solves to one member and the
    // existing WARN017 (`union_hole_solved_monomorphically`) fires — the
    // T ≡ E guard does not (nothing collapsed).
    let result = test_support::analyze_source(concat!(
        "functio f() → numerus {\n    redde 42\n}\n\n",
        "incipit {\n    sit ∪ r ← capta { f() }\n    nota r\n}\n",
    ));
    assert!(
        result.diagnostics.iter().any(|diag| !diag.is_error()
            && test_support::issue(diag) == Some("union_hole_solved_monomorphically")),
        "expected WARN017 for the degenerate trap solve, got: {:?}",
        result.diagnostics
    );
    assert!(
        !result.diagnostics.iter().any(
            |diag| diag.is_error() && test_support::issue(diag) == Some("trap_union_collapsed")
        ),
        "no error flow means no T ≡ E collapse: {:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn union_members(result: &test_support::AnalyzedFixture, ty: crate::TypeId) -> Vec<crate::TypeId> {
    match result.types.get(ty) {
        Type::Union(members) => members.clone(),
        other => panic!("expected a union type, got {other:?}"),
    }
}

fn is_primitive(
    result: &test_support::AnalyzedFixture,
    ty: crate::TypeId,
    primitive: Primitive,
) -> bool {
    matches!(result.types.get(ty), Type::Primitive(found) if *found == primitive)
}
