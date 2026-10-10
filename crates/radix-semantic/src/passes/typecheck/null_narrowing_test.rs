//! Flow-refinement tests for null checks (nn-u1 refinement core).
//!
//! The assignment-path probe is recorded here first, fixture before claim:
//! `const T y ← x` compiles **outside** any null-check block because the
//! source spelling `T ∪ null` lowers to `Type::Option(T)` (hir/lower/types.rs
//! `lower_union_type`) and `TypeTable::is_assignable` permits `Option(T) → T`
//! assignment generally (implicit unwrap; Rust codegen unwraps). Since need
//! f1e76e37 a declaration or assignment store of an optional into a plain
//! cell is `optional_into_plain_cell`; narrowing is the way to a plain `T`.
//!
//! What IS block-sensitive today is *use*: a method call or `+` concat on an
//! `Option(T)` receiver/operand rejects (SEM004 / SEM011) even inside
//! `if x not is null { … }`. nn-u1 adds the flow-refinement record that
//! narrows the binding to `T` for the taken branch only.
//!
//! Scope hygiene is asserted explicitly: the narrowed type must not leak past
//! its branch block (a narrowed type escaping its block would be a soundness
//! defect).
//!
//! NOTE: fixtures use the latin spellings (`functio`, `textus`, `nihil`,
//! `redde`, `si`, `non est`, `est`, `secus`) because the radix unit-test
//! harness compiles bare source without the en-locale lex pack
//! (`fn`/`string`/`null`/`return`/`if`/`is`/`not` are en-pack spellings —
//! the corpus pipeline activates them via frontmatter).

use crate::test_support;

// ---------------------------------------------------------------------------
// Declaration and assignment stores do not unwrap an optional (f1e76e37)
// ---------------------------------------------------------------------------

#[test]
fn assignment_of_option_outside_null_check_is_rejected() {
    // Recorded 2026-08-10 as accepted (`TypeTable::is_assignable` unwraps
    // `Option(T)` into `T`); need closed that for declaration and
    // assignment stores: `fixum textus y ← x` with `x : textus ∪ nihil` and no
    // null check is `SEM010 optional_into_plain_cell`.
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → textus {\n    fixum textus y ← x\n    redde y\n}",
        "optional_into_plain_cell",
    );
}

#[test]
fn probe_capture_idiom_compiles_inside_block() {
    // The shipping capture idiom (tela browser.fab, rivus pilot) stays green.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → textus {\n    si x non est nihil {\n        fixum textus y ← x\n        redde y\n    }\n    redde \"\"\n}",
    );
}

// ---------------------------------------------------------------------------
// Refinement: `not is null` records x : T inside the taken branch
// ---------------------------------------------------------------------------

#[test]
fn non_null_member_call_uses_method_lookup() {
    // `x!.longitudo()` lowers as a direct call whose callee is the non-null
    // member chain. It must use method lookup, not field lookup followed by a
    // callable check.
    test_support::assert_accepts("functio f(textus ∪ nihil x) → numerus { redde x!.longitudo() }");
}

#[test]
fn not_is_null_narrows_method_call() {
    // Fails without the record (SEM004 optional_receiver_requires_chain on the
    // `Option` receiver); passes once the binding is recorded as `textus`.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus { si x non est nihil { redde x.longitudo() } redde 0 }",
    );
}

#[test]
fn not_is_null_narrows_concat() {
    // Fails without the record (SEM011 string_operands_required on the
    // `Option` operand); passes once the binding is recorded as `textus`.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → textus { si x non est nihil { redde x + \"!\" } redde \"\" }",
    );
}

// ---------------------------------------------------------------------------
// `is null` keeps the binding nullable (no spurious narrowing)
// ---------------------------------------------------------------------------

#[test]
fn is_null_keeps_method_call_rejected() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → numerus { si x est nihil { redde x.longitudo() } redde 0 }",
        "optional_receiver_requires_chain",
    );
}

#[test]
fn is_null_keeps_concat_rejected() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → textus { si x est nihil { redde x + \"!\" } redde \"\" }",
        "string_operands_required",
    );
}

// ---------------------------------------------------------------------------
// Fallthrough / else keeps T ∪ null; the else of `x is null` narrows (Q3)
// ---------------------------------------------------------------------------

#[test]
fn not_is_null_else_keeps_binding_nullable() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → numerus { si x non est nihil { redde 0 } secus { redde x.longitudo() } }",
        "optional_receiver_requires_chain",
    );
}

#[test]
fn is_null_else_narrows() {
    // Q3 (accepted): the else-branch of `x is null` narrows identically.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus { si x est nihil { redde 0 } secus { redde x.longitudo() } }",
    );
}

// ---------------------------------------------------------------------------
// Scope hygiene: the refinement must not escape its branch block
// ---------------------------------------------------------------------------

#[test]
fn refinement_does_not_leak_outside_block() {
    // After the narrowed `not is null` block, `x` is nullable again: a method
    // call in the fallthrough must still reject.
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → numerus { si x non est nihil { redde x.longitudo() } redde x.longitudo() }",
        "optional_receiver_requires_chain",
    );
}

#[test]
fn refinement_restored_after_if_statement() {
    // `x is null { } secus { }` narrows only the else; after the whole if
    // statement, `x` is nullable again.
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → numerus { si x est nihil { redde 0 } secus { redde x.longitudo() } redde x.longitudo() }",
        "optional_receiver_requires_chain",
    );
}

// ---------------------------------------------------------------------------
// nn-u2: use-site threading — argument passing, then-form, compound conditions
// ---------------------------------------------------------------------------

#[test]
fn narrowed_binding_passes_as_non_null_argument() {
    // done_when (b): inside the narrowed block, `x` flows to a non-null
    // parameter without a capture binding. The binding override threads the
    // refined type through the call argument path (`check_call_args`
    // unification against the parameter contract).
    test_support::assert_accepts(
        "functio g(textus y) → textus { redde y }\n\
         functio f(textus ∪ nihil x) → textus {\n\
             si x non est nihil { redde g(x) }\n\
             redde \"\"\n\
         }",
    );
}

#[test]
fn then_form_narrows_identically() {
    // done_when (c): the single-statement `ergo` (`then`) body lowers to the
    // same `si` block surface, so the flow refinement applies identically to
    // the block form.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → bivalens { si x non est nihil ergo redde x.longitudo() ≻ 0 redde falsum }",
    );
}

#[test]
fn or_condition_does_not_narrow_success_path() {
    // done_when (d): `x non est nihil aut …` must not narrow `x` on the
    // success path — the right disjunct alone can satisfy the condition, so
    // the binding keeps `T ∪ null` inside the taken branch.
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, bivalens y) → numerus { si x non est nihil aut y { redde x.longitudo() } redde 0 }",
        "optional_receiver_requires_chain",
    );
}

#[test]
fn and_condition_narrows_after_the_check() {
    // Delivery outcome: `x non est nihil et …` refines `x` only after the
    // check — the whole conjunction being true requires the left null-check
    // conjunct to hold, so the taken branch narrows.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, bivalens y) → numerus { si x non est nihil et y { redde x.longitudo() } redde 0 }",
    );
}

#[test]
fn and_condition_else_stays_unnarrowed() {
    // Soundness: the falsity of `x non est nihil et y` does not force `x` to
    // be null, so the else-branch must not narrow (no spurious refinement on
    // the `and` fallthrough).
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, bivalens y) → numerus { si x non est nihil et y { redde 0 } secus { redde x.longitudo() } }",
        "optional_receiver_requires_chain",
    );
}

// ---------------------------------------------------------------------------
// nn-u3: consumer-shape oracle fixtures — the rivus `code2`/`iss2` captures
// and the tela `certain ← nodes` copy are no longer required
// ---------------------------------------------------------------------------

#[test]
fn rivus_lookup_key_direct_form_compiles_without_captures() {
    // done_when (a): the rivus `lookup_key` body's direct form —
    // `redde code + ":" + iss` inside the narrowed block — compiles with NO
    // `code2`/`iss2` capture bindings. Mirrors the live rivus shape
    // (src/diagnostics/diagnostic.fab): the nested `not is null` checks are
    // the capture sites the predicate will revert.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil code, textus ∪ nihil iss) → textus ∪ nihil {\n\
             si code non est nihil {\n\
                 si iss non est nihil {\n\
                     redde code + \":\" + iss\n\
                 }\n\
                 redde code\n\
             }\n\
             redde nulla\n\
         }",
    );
}

#[test]
fn rivus_diagnostic_identity_direct_method_compiles_without_captures() {
    // done_when (a): the rivus `diagnostic_identity` body's direct form —
    // `iss.longitudo() > 0` inside the narrowed block — compiles with NO
    // `iss2` capture. Mirrors the live rivus shape
    // (src/diagnostics/diagnostic.fab): the `iss2` capture site the
    // predicate will revert.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil iss) → textus {\n\
             si iss non est nihil {\n\
                 si iss.longitudo() ≻ 0 {\n\
                     redde iss\n\
                 }\n\
             }\n\
             redde \"\"\n\
         }",
    );
}

#[test]
fn tela_shape_uses_narrowed_list_directly_without_copy() {
    // done_when (b): the tela `identities_from_nodes` body's direct form —
    // method + index use on `nodes` inside `if nodes not is null { … }` —
    // compiles with NO `certain ← nodes` copy. Mirrors the live tela shape
    // (src/browser.fab): the copy (`certain`) is the workaround the
    // predicate will revert.
    test_support::assert_accepts(
        "functio f(lista<textus> ∪ nihil nodes) → numerus {\n\
             si nodes non est nihil {\n\
                 redde nodes.longitudo() + nodes[0].longitudo()\n\
             }\n\
             redde 0\n\
         }",
    );
}

#[test]
fn tela_shape_narrowed_list_members_compile_directly() {
    // done_when (b), extension: member access on a narrowed list element
    // (`nodes[i].identitas` in tela) also compiles directly inside the
    // block — the full reversed loop body needs no `certain` carrier.
    test_support::assert_accepts(
        "functio f(lista<textus> ∪ nihil nodes) → numerus {\n\
             varia numerus total ← 0\n\
             varia numerus i ← 0\n\
             si nodes non est nihil {\n\
                 dum i ≺ nodes.longitudo() {\n\
                     total ← total + nodes[i].longitudo()\n\
                     i ← i + 1\n\
                 }\n\
             }\n\
             redde total\n\
         }",
    );
}
