//! THU-U-05 — finalize solve + shape warnings (WARN016/WARN017).
//!
//! Locks the goal req 15 matrix: the `_` hole solved to a multi-member union
//! emits WARN016 `monomorphic_hole_solved_to_union`; the `∪` hole solved to a
//! single inhabitant emits WARN017 `union_hole_solved_monomorphically`; the
//! canonical nullable `T ∪ nihil` is a single inhabitant (never WARN016);
//! explicit `A ∪ B` annotations are never holes; named aliases of multi-member
//! unions do not hide the sum; unsolved `∪` is a `cannot_infer_union_type`
//! error; nested `lista<∪>` solved monomorphically warns; all emissions are
//! `Severity::Warning` only.
//!
//! Closeout filter: `cargo test -p radix --lib type_hole_union_warnings`.

use super::*;
use crate::SemanticError;
use crate::passes::typecheck::test_support;
use radix_target::Target;

fn count_issue(diagnostics: &[SemanticError], issue: &str) -> usize {
    diagnostics
        .iter()
        .filter(|diag| diag.issue() == Some(issue))
        .count()
}

fn has_error(diagnostics: &[SemanticError]) -> bool {
    diagnostics.iter().any(SemanticError::is_error)
}

fn count_checker_issue(checker: &TypeChecker<'_>, issue: &str) -> usize {
    checker
        .errors
        .iter()
        .filter(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .count()
}

// ---------------------------------------------------------------------------
// WARN016 — `_` hole solved to a multi-member union
// ---------------------------------------------------------------------------

#[test]
fn underscore_hole_solved_to_multi_member_union_warns_warn016() {
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r"
typus Valor = numerus ∪ textus
incipit {
    sit _ v ← 1 ∷ Valor
    nota v
}
",
    );
    assert_eq!(
        count_issue(&diagnostics, "monomorphic_hole_solved_to_union"),
        1,
        "a `_` hole solved to a multi-member union must emit WARN016 exactly once: {diagnostics:?}"
    );
    assert!(
        !has_error(&diagnostics),
        "WARN016 must be a warning, not an error: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(
            |diag| diag.issue() == Some("monomorphic_hole_solved_to_union") && !diag.is_error()
        ),
        "WARN016 diagnostic must carry Severity::Warning: {diagnostics:?}"
    );
}

#[test]
fn named_alias_of_multi_member_union_does_not_hide_warn016() {
    // Goal req 12: a named alias of a multi-member union is still multi-member
    // after alias peeling — `_` solved through the alias warns WARN016.
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r"
typus Valor = numerus ∪ textus
functio f() → Valor {
    redde 1
}
incipit {
    sit _ v ← f()
    nota v
}
",
    );
    assert_eq!(
        count_issue(&diagnostics, "monomorphic_hole_solved_to_union"),
        1,
        "alias of a multi-member union must not hide WARN016: {diagnostics:?}"
    );
    assert!(
        !has_error(&diagnostics),
        "WARN016 must not be an error: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// WARN017 — `∪` hole solved to a single inhabitant
// ---------------------------------------------------------------------------

#[test]
fn union_hole_solved_monomorphically_warns_warn017() {
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r"
incipit {
    sit ∪ only ← 1
    nota only
}
",
    );
    assert_eq!(
        count_issue(&diagnostics, "union_hole_solved_monomorphically"),
        1,
        "a `∪` hole solved to one member must emit WARN017 exactly once: {diagnostics:?}"
    );
    assert!(
        !has_error(&diagnostics),
        "WARN017 must be a warning, not an error: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(
            |diag| diag.issue() == Some("union_hole_solved_monomorphically") && !diag.is_error()
        ),
        "WARN017 diagnostic must carry Severity::Warning: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// Nullable and explicit-union exceptions
// ---------------------------------------------------------------------------

#[test]
fn nullable_union_never_fires_warn016() {
    // Goal req 10: `T ∪ nihil` (canonical `Type::Option`) counts as ONE
    // inhabitant — `_` solved to it must not warn.
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r#"
incipit {
    sit textus ∪ nihil opt ← "s"
    sit _ o ← opt
    nota o
}
"#,
    );
    assert_eq!(
        count_issue(&diagnostics, "monomorphic_hole_solved_to_union"),
        0,
        "`_` solved to `T ∪ nihil` must not emit WARN016: {diagnostics:?}"
    );
    assert_eq!(
        count_issue(&diagnostics, "union_hole_solved_monomorphically"),
        0,
        "no `∪` hole here, so WARN017 must stay silent: {diagnostics:?}"
    );
    assert!(
        !has_error(&diagnostics),
        "the nullable path must typecheck cleanly: {diagnostics:?}"
    );
}

#[test]
fn explicit_union_annotation_fires_neither_warning() {
    // Goal req 11: explicit `A ∪ B` annotations are never holes.
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r"
incipit {
    sit numerus ∪ textus named ← 1
    nota named
}
",
    );
    assert_eq!(
        count_issue(&diagnostics, "monomorphic_hole_solved_to_union"),
        0,
        "explicit union annotation must not emit WARN016: {diagnostics:?}"
    );
    assert_eq!(
        count_issue(&diagnostics, "union_hole_solved_monomorphically"),
        0,
        "explicit union annotation must not emit WARN017: {diagnostics:?}"
    );
    assert!(
        !has_error(&diagnostics),
        "explicit union must typecheck cleanly: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// Unsolved `∪` is an error, never a warning
// ---------------------------------------------------------------------------

#[test]
fn unsolved_union_hole_is_cannot_infer_union_type_error() {
    // Goal req 7: an underdetermined `∪` hole is a missing-annotation error.
    // A struct field with no flows into its `∪` hole leaves it unsolved.
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r"
genus G {
    varia ∪ x
}
",
    );
    assert_eq!(
        count_issue(&diagnostics, "cannot_infer_union_type"),
        1,
        "unsolved `∪` must report cannot_infer_union_type: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("cannot_infer_union_type") && diag.is_error()),
        "unsolved `∪` must be an error, not a warning: {diagnostics:?}"
    );
    assert_eq!(
        count_issue(&diagnostics, "union_hole_solved_monomorphically"),
        0,
        "unsolved holes must never warn WARN017: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// Warned-set guard: decl + init sharing one hole emit a single diagnostic
// ---------------------------------------------------------------------------

#[test]
fn shared_hole_across_decl_and_init_emits_single_warning() {
    // `varia ∪ b ← a`: the identifier `a` carries its still-open hole on the
    // expression node while `a`'s own declaration node carries the same hole.
    // Without the per-`InferVar` warned-set, `a` would warn WARN017 twice
    // (decl + expr) and `b` twice. Both holes are solved monomorphically →
    // exactly two WARN017 total, one per hole variable.
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r"
incipit {
    sit ∪ a ← 1
    varia ∪ b ← 0
    b ← a
    nota a
    nota b
}
",
    );
    assert_eq!(
        count_issue(&diagnostics, "union_hole_solved_monomorphically"),
        2,
        "one warning per hole variable, never per node: {diagnostics:?}"
    );
    assert!(
        !has_error(&diagnostics),
        "hole-to-hole merge must typecheck: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// Nested `lista<∪>` solved monomorphically — checker-level (robust while U-04
// mixed-join is still landing; builds the exact scenario directly)
// ---------------------------------------------------------------------------

#[test]
fn nested_union_hole_solved_monomorphically_warns_warn017() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let var = InferVar(9_000_101);
    let hole = checker.types.intern(Type::InferUnion(var));
    let list_ty = checker.types.array(hole);

    // One direct flow into the nested element hole.
    checker.unify_assign_issue(hole, numerus, span, "test");

    // Finalize classifies the annotation `lista<∪>` (the hole is nested one
    // level inside the array shape).
    checker.collect_hole_shape_warnings(list_ty, span);

    assert_eq!(
        count_checker_issue(&checker, "union_hole_solved_monomorphically"),
        1,
        "nested `lista<∪>` solved to one member must emit WARN017"
    );
    assert!(
        checker.errors.iter().all(|err| !err.is_error()),
        "WARN017 must be Severity::Warning only"
    );
}

// ---------------------------------------------------------------------------
// No-noise guard: ordinary `_` holes stay silent
// ---------------------------------------------------------------------------

#[test]
fn ordinary_underscore_holes_produce_no_hole_warnings() {
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r"
incipit {
    sit _ n ← 3
    sit lista<_> xs ← [1, 2, 3]
    nota n
    nota xs
}
",
    );
    assert_eq!(
        count_issue(&diagnostics, "monomorphic_hole_solved_to_union"),
        0,
        "monomorphic solutions must not warn WARN016: {diagnostics:?}"
    );
    assert_eq!(
        count_issue(&diagnostics, "union_hole_solved_monomorphically"),
        0,
        "no union holes here, so WARN017 must stay silent: {diagnostics:?}"
    );
    assert!(
        !has_error(&diagnostics),
        "ordinary `_` bindings must typecheck cleanly: {diagnostics:?}"
    );
}
