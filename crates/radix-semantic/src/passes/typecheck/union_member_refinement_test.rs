//! Union member rule + is-refinement tests (type-algebra U1R).
//!
//! The permissive baseline this unit flips: dot-access on `∪` receivers used
//! to fall through `check_field_from_type` to a silent `Type::Error` sink —
//! sound but loose, no member rule. The common-member rule makes those sites
//! real signatures or attributed `union_member_not_common` errors, and
//! `est M` becomes the narrowing recovery path (`si` branches, `non est`
//! mirror, `adfirma` assert-remainder).
//!
//! Fixtures use latin spellings (`genus`, `functio`, `textus`, `redde`,
//! `si`, `secus`, `est`, `non est`, `adfirma`) because the radix unit-test
//! harness compiles bare source without the en-locale lex pack.

use crate::test_support;

/// Two nominal constituents sharing no members: `nuntius` vs `fructus`.
fn errata_bonum(body: &str) -> String {
    let decls = "\
genus Errata {
    varia textus nuntius
}

genus Bonum {
    varia textus fructus
}

";
    format!("{decls}{body}")
}

// ---------------------------------------------------------------------------
// Permissive-baseline flip: union member access
// ---------------------------------------------------------------------------

#[test]
fn union_field_miss_is_attributed_error() {
    // The declared flip: member access on a `∪` receiver was
    // permissive-ignotum (green, sink-typed); it is now the common-member
    // rule. `magnitudo` is no constituent's field, so the error fires with
    // the lacking constituents named.
    test_support::assert_rejects_with_issue(
        "functio f(numerus ∪ textus u) { nota u.magnitudo }",
        "union_member_not_common",
    );
}

#[test]
fn union_common_field_has_real_signature() {
    // Both constituents expose `nuntius : textus` — a common member with a
    // real signature, not ignotum.
    test_support::assert_accepts(concat!(
        "genus Errata {\n    varia textus nuntius\n}\n\n",
        "genus Signum {\n    varia textus nuntius\n    varia textus nota\n}\n\n",
        "functio f(Errata ∪ Signum r) → textus { redde r.nuntius }",
    ));
}

#[test]
fn union_field_type_disagreement_is_not_common() {
    // Same field name, different types across constituents: not a common
    // member; D9.2 names each constituent's type.
    test_support::assert_rejects_with_issue(
        concat!(
            "genus A {\n    varia textus commune\n}\n\n",
            "genus B {\n    varia numerus commune\n}\n\n",
            "functio f(A ∪ B r) → textus { redde r.commune }",
        ),
        "union_member_differs",
    );
}

#[test]
fn union_common_method_has_real_signature() {
    // Same-name methods with agreeing signatures across constituents are
    // callable with a real signature.
    test_support::assert_accepts(concat!(
        "genus A {\n    functio code() → numerus { redde 1 }\n}\n\n",
        "genus B {\n    functio code() → numerus { redde 2 }\n}\n\n",
        "functio f(A ∪ B r) → numerus { redde r.code() }",
    ));
}

#[test]
fn union_shadowed_method_is_not_common() {
    // Incompatible same-name signatures remove the method from the common
    // set; the call rejects naming each constituent's signature (D9.2).
    test_support::assert_rejects_with_issue(
        concat!(
            "genus A {\n    functio code(numerus n) → numerus { redde n }\n}\n\n",
            "genus B {\n    functio code(textus s) → numerus { redde 1 }\n}\n\n",
            "functio f(A ∪ B r) → numerus { redde r.code(5) }",
        ),
        "union_member_differs",
    );
}

#[test]
fn union_alias_receiver_follows_member_rule() {
    // `typus U = A ∪ B` is the same type as its unfolded union (BO-S-4 alias
    // transparency); the member rule sees through the alias.
    test_support::assert_rejects_with_issue(
        concat!(
            "genus A {\n    varia textus commune\n}\n\n",
            "genus B {\n    varia textus commune\n}\n\n",
            "typus U = A ∪ B\n\n",
            "functio f(U r) → textus { redde r.absens }",
        ),
        "union_member_not_common",
    );
}

// ---------------------------------------------------------------------------
// si refinement: `est M` / `non est M`
// ---------------------------------------------------------------------------

#[test]
fn si_est_narrows_then_branch() {
    // Fails without the TypeCheck arm (nuntius is not a common member); the
    // taken branch narrows `r` to Errata.
    test_support::assert_accepts(&errata_bonum(
        "functio f(Errata ∪ Bonum r) → textus {\n    si r est Errata { redde r.nuntius }\n    redde \"\"\n}",
    ));
}

#[test]
fn si_est_else_branch_is_union_minus_member() {
    // The fallthrough of `r est Errata` keeps every member except Errata —
    // here exactly Bonum, so the Bonum-only member is available there.
    test_support::assert_accepts(&errata_bonum(
        "functio f(Errata ∪ Bonum r) → textus {\n    si r est Errata {\n        redde r.nuntius\n    } secus {\n        redde r.fructus\n    }\n}",
    ));
}

#[test]
fn si_non_est_mirrors() {
    // `r non est Bonum` narrows the taken branch to the union minus Bonum
    // (Errata) and the fallthrough to Bonum.
    test_support::assert_accepts(&errata_bonum(
        "functio f(Errata ∪ Bonum r) → textus {\n    si r non est Bonum {\n        redde r.nuntius\n    } secus {\n        redde r.fructus\n    }\n}",
    ));
}

#[test]
fn si_refinement_does_not_leak_past_normal_join() {
    // Both edges continue, so a branch-only refinement must not escape the
    // `si`; the incoming union remains the only sound continuation type.
    test_support::assert_rejects_with_issue(
        &errata_bonum(
            "functio f(Errata ∪ Bonum r) → textus {\n    si r est Errata { nota r.nuntius }\n    redde r.nuntius\n}",
        ),
        "union_member_not_common",
    );
}

#[test]
fn si_est_on_primitive_union_narrows() {
    // Today's-union sanity: primitive unions narrow the same way — the taken
    // branch is `numerus`, the fallthrough `textus`.
    test_support::assert_accepts(
        "functio f(numerus ∪ textus u) → numerus {\n\
             si u est numerus { redde u + 1 }\n\
             redde 0\n\
         }",
    );
    test_support::assert_accepts(
        "functio f(numerus ∪ textus u) → numerus {\n\
             si u est textus { redde 0 } secus { redde u + 1 }\n\
         }",
    );
}

#[test]
fn si_est_null_narrowing_stays_green() {
    // The nn-u1 null seam keeps its behavior through the generalized peel.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus { si x non est nihil { redde x.longitudo() } redde 0 }",
    );
}

#[test]
fn si_est_compound_conjunction_narrows_taken_branch_only() {
    // nn-u2 conservatism carries to member tests: `r est Errata et cond`
    // narrows the taken branch; the fallthrough stays un-narrowed.
    test_support::assert_accepts(&errata_bonum(
        "functio f(Errata ∪ Bonum r, bivalens c) → textus {\n    si r est Errata et c { redde r.nuntius }\n    redde \"\"\n}",
    ));
    test_support::assert_rejects_with_issue(
        &errata_bonum(
            "functio f(Errata ∪ Bonum r, bivalens c) → textus {\n    si r est Errata et c { redde \"\" } secus { redde r.nuntius }\n}",
        ),
        "union_member_not_common",
    );
}

#[test]
fn varia_binding_is_not_refined() {
    // Immutable-first (open row 2): a mutable binding must not narrow — its
    // cell can be reassigned inside the branch, so the refinement would lie.
    // (A `varia` local carries the union annotation; the mutable-parameter
    // spelling does not compose with union annotations in the grammar.)
    test_support::assert_rejects_with_issue(
        &errata_bonum(
            "functio f(Errata ∪ Bonum seed) {\n    varia Errata ∪ Bonum r ← seed\n    si r est Errata { nota r.nuntius }\n}",
        ),
        "union_member_not_common",
    );
}

// ---------------------------------------------------------------------------
// adfirma assert-remainder refinement
// ---------------------------------------------------------------------------

#[test]
fn adfirma_est_narrows_rest_of_block() {
    // The adopted two-line idiom: `assert r is Errata` then `r.nuntius`.
    // Provable before UT on a hand-annotated union.
    test_support::assert_accepts(&errata_bonum(
        "functio f(Errata ∪ Bonum r) → textus {\n    adfirma r est Errata\n    adfirma r.nuntius ≡ \"boom\"\n    redde r.nuntius\n}",
    ));
}

#[test]
fn adfirma_refinement_restored_at_block_exit() {
    // The override lives exactly for the rest of its block; after the block,
    // `r` is the full union again.
    test_support::assert_rejects_with_issue(
        &errata_bonum(
            "functio f(Errata ∪ Bonum r) → textus {\n    {\n        adfirma r est Errata\n        adfirma r.nuntius ≡ \"boom\"\n    }\n    redde r.nuntius\n}",
        ),
        "union_member_not_common",
    );
}

#[test]
fn adfirma_non_est_narrows_to_union_minus_member() {
    test_support::assert_accepts(&errata_bonum(
        "functio f(Errata ∪ Bonum r) → textus {\n    adfirma r non est Errata\n    redde r.fructus\n}",
    ));
}

// ---------------------------------------------------------------------------
// Dead-test warning + value-equality preservation
// ---------------------------------------------------------------------------

#[test]
fn est_member_absent_from_union_warns() {
    // `est M` where M is not a union member is a statically dead test — a
    // WARN, never an error, so value-equality Binary(Is) stays untouched.
    test_support::assert_warns_with_issue(
        &errata_bonum("functio f(Errata ∪ Bonum r) → bivalens { redde r est numerus }"),
        "type_test_member_absent",
    );
}

#[test]
fn est_value_rhs_is_rejected() {
    // `est` tests a type (D23.1): a right-hand side that names a value is
    // rejected by name, never compared.
    test_support::assert_rejects_with_issue(
        "functio f(numerus x, numerus aliud) → bivalens { redde x est aliud }",
        "est_value_rhs",
    );
}

// ---------------------------------------------------------------------------
// ∪-of-nominal guardrails (1, 3, 4 at annotation sites)
// ---------------------------------------------------------------------------

#[test]
fn empty_common_member_union_warns_store_only() {
    // Guardrail 3: constituents sharing no members are store-only.
    test_support::assert_warns_with_issue(
        &errata_bonum("functio f(Errata ∪ Bonum r) → textus { redde \"\" }"),
        "union_no_common_members",
    );
}

#[test]
fn shared_field_union_is_quiet() {
    // Common members exist, so guardrail 3 stays quiet, and the retired
    // guardrail 1 (WARN024) no longer teaches narrowing (D9.2).
    let result = test_support::analyze_and_get_diagnostics(concat!(
        "genus A {\n    varia textus commune\n}\n\n",
        "genus B {\n    varia textus commune\n}\n\n",
        "functio f(A ∪ B r) → textus { redde r.commune }",
    ));
    assert!(!result
        .iter()
        .any(|err| !err.is_error() && test_support::issue(err) == Some("union_nominal_members")));
    assert!(
        !result
            .iter()
            .any(|err| !err.is_error()
                && test_support::issue(err) == Some("union_no_common_members"))
    );
}

#[test]
fn payload_free_enum_annotation_draws_no_union_warning() {
    // A declared enum is not a written `∪`: neither the bare annotation nor
    // `E ∪ nihil` draws a union guardrail warning.
    let result = test_support::analyze_and_get_diagnostics(concat!(
        "ordo Kind { plain, fizz }\n\n",
        "functio f(Kind k, Kind ∪ nihil m) → numerus {\n",
        "    sit Kind j ← Kind.fizz\n",
        "    redde 1\n",
        "}",
    ));
    assert!(!result.iter().any(|err| !err.is_error()
        && matches!(
            test_support::issue(err),
            Some("union_no_common_members" | "union_method_signature_shadowed")
        )));
}

#[test]
fn shadowed_method_union_warns_at_annotation() {
    // Guardrail 4: same-name method with incompatible signatures names the
    // collision at the `∪` site.
    test_support::assert_warns_with_issue(
        concat!(
            "genus A {\n    functio code(numerus n) → numerus { redde n }\n}\n\n",
            "genus B {\n    functio code(textus s) → numerus { redde 1 }\n}\n\n",
            "functio f(A ∪ B r) → numerus { redde 1 }",
        ),
        "union_method_signature_shadowed",
    );
}

#[test]
fn primitive_union_annotation_does_not_warn() {
    // Guardrails 3/4 are the ∪-of-nominal family: a primitive union has no
    // nominal members and no declared-member view, so nothing fires.
    let result =
        test_support::analyze_and_get_diagnostics("functio f(numerus ∪ textus u) { nota u }");
    assert!(!result
        .iter()
        .any(|err| !err.is_error() && test_support::issue(err) == Some("union_nominal_members")));
}

// ---------------------------------------------------------------------------
// Closed enum variant members (N0)
// ---------------------------------------------------------------------------

const CARRIER: &str = r"genus ErrorDetail {
    varia textus body
}

discretio Carrier {
    DetailCarrier { ErrorDetail detail },
    TextCarrier { textus text }
}

";

#[test]
fn enum_variant_refinement_reaches_return_funnel() {
    test_support::assert_accepts(&format!(
        "{CARRIER}functio probe(Carrier r) → textus {{\n    adfirma r est DetailCarrier\n    redde r.detail.body\n}}"
    ));
}

#[test]
fn enum_variant_refinement_reaches_initializer_funnel() {
    test_support::assert_accepts(&format!(
        "{CARRIER}functio probe(Carrier r) → textus {{\n    adfirma r est DetailCarrier\n    sit textus body ← r.detail.body\n    redde body\n}}"
    ));
}
