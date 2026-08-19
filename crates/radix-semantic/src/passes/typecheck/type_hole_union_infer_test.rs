//! U-02 — inference accumulation for the union hole (`Type::InferUnion`).
//!
//! The standalone `∪` hole accumulates member types from direct flows into the
//! slot instead of binding a single inhabitant the way the monomorphic `_`
//! hole does. These tests lock the accumulation primitives in `infer.rs`
//! (unify arms, `accumulate_into_union_hole`, occurs guard) and the
//! `union_members` state on `TypeChecker`.
//!
//! Closeout filter: `cargo test -p radix --lib type_hole_union_infer`.

use super::*;
use crate::passes::typecheck::test_support;
use radix_target::Target;

// NOTE: `TypeChecker::new` borrows the `TypeTable` for the checker lifetime, so
// tests drive everything through `checker.types` (the same arena) and never
// keep a second handle to the table.

fn fresh_union_hole(checker: &mut TypeChecker<'_>, var: u32) -> TypeId {
    checker.types.intern(Type::InferUnion(InferVar(var)))
}

fn members_of(checker: &TypeChecker<'_>, var: InferVar) -> Vec<TypeId> {
    checker
        .union_members
        .borrow()
        .get(&var)
        .cloned()
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Checker-level accumulation primitives
// ---------------------------------------------------------------------------

#[test]
fn union_hole_accumulates_direct_flows_and_dedups() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let textus = checker.types.primitive(Primitive::Textus);
    let var = InferVar(7001);
    let hole = fresh_union_hole(&mut checker, var.0);

    // First direct flow: the hole stays open and the member accumulates.
    let result = checker.unify_assign_issue(hole, numerus, span, "test");
    assert_eq!(result, hole, "union hole must stay open after accumulation");
    assert_eq!(members_of(&checker, var), vec![numerus]);

    // A second distinct flow accumulates both members.
    checker.unify_assign_issue(hole, textus, span, "test");
    let got = members_of(&checker, var);
    assert_eq!(got.len(), 2, "two direct flows must accumulate both");
    assert!(got.contains(&numerus) && got.contains(&textus));

    // A duplicate flow dedups through structural `equals`.
    checker.unify_assign_issue(hole, numerus, span, "test");
    let got = members_of(&checker, var);
    assert_eq!(got.len(), 2, "duplicate flows must dedup");
    assert!(checker.errors.is_empty(), "accumulation must not error");
}

#[test]
fn union_hole_accumulates_under_join_policy_too() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let var = InferVar(7002);
    let hole = fresh_union_hole(&mut checker, var.0);

    let result = checker.unify_join(hole, numerus, span, "test");
    assert_eq!(result, hole);
    assert_eq!(members_of(&checker, var), vec![numerus]);
}

#[test]
fn union_hole_merges_member_sets_without_moving() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let textus = checker.types.primitive(Primitive::Textus);
    let var_a = InferVar(7101);
    let var_b = InferVar(7102);
    let hole_a = fresh_union_hole(&mut checker, var_a.0);
    let hole_b = fresh_union_hole(&mut checker, var_b.0);

    checker.unify_assign_issue(hole_a, numerus, span, "test");
    checker.unify_assign_issue(hole_b, textus, span, "test");

    // InferUnion ∪ InferUnion merges (copies) the other hole's set.
    let result = checker.unify_assign_issue(hole_a, hole_b, span, "test");
    assert_eq!(result, hole_a, "the accumulating hole stays open");
    let got = members_of(&checker, var_a);
    assert_eq!(got.len(), 2, "merge must union member sets");
    assert!(got.contains(&numerus) && got.contains(&textus));

    // The source hole keeps its own members (copy, not move).
    assert_eq!(members_of(&checker, var_b), vec![textus]);

    // Reverse argument order also merges, deduping shared members.
    checker.unify_assign_issue(hole_b, hole_a, span, "test");
    let got = members_of(&checker, var_b);
    assert_eq!(got.len(), 2, "reverse merge must also union sets");
    assert!(got.contains(&numerus) && got.contains(&textus));
}

#[test]
fn union_hole_merging_same_hole_is_noop() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let var = InferVar(7103);
    let hole = fresh_union_hole(&mut checker, var.0);

    let result = checker.unify_assign_issue(hole, hole, span, "test");
    assert_eq!(result, hole);
    assert!(members_of(&checker, var).is_empty());
    assert!(checker.errors.is_empty());
}

#[test]
fn union_hole_flattens_explicit_union_members() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let textus = checker.types.primitive(Primitive::Textus);
    let var = InferVar(7201);
    let hole = fresh_union_hole(&mut checker, var.0);

    // An explicit `A ∪ B` value flowing into the hole contributes each member.
    let union_ty = checker.types.intern(Type::Union(vec![numerus, textus]));
    let result = checker.unify_assign_issue(hole, union_ty, span, "test");
    assert_eq!(result, hole);
    let got = members_of(&checker, var);
    assert_eq!(got.len(), 2);
    assert!(got.contains(&numerus) && got.contains(&textus));

    // Flattening an already-present member dedups.
    checker.unify_assign_issue(hole, union_ty, span, "test");
    assert_eq!(members_of(&checker, var).len(), 2);
}

#[test]
fn union_hole_occurs_check_rejects_recursive_member() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let var = InferVar(7301);
    let hole = fresh_union_hole(&mut checker, var.0);

    // Accumulating `lista<∪>` into `∪` would form a recursive semantic type.
    let array_of_hole = checker.types.array(hole);
    let result = checker.unify_assign_issue(array_of_hole, hole, span, "recursive");
    assert_eq!(result, checker.error_type);
    assert!(
        !checker.errors.is_empty(),
        "occurs violation must be diagnosed"
    );
    assert!(
        members_of(&checker, var).is_empty(),
        "no recursive member stored"
    );
}

#[test]
fn union_hole_occurs_check_allows_self_slot_flow() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let var = InferVar(7302);
    let hole = fresh_union_hole(&mut checker, var.0);

    let result = checker.unify_assign_issue(hole, hole, span, "self");
    assert_eq!(result, hole);
    assert!(checker.errors.is_empty());
    assert!(members_of(&checker, var).is_empty());
}

#[test]
fn union_hole_keeps_pending_monomorphic_member_and_later_resolution() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let var = InferVar(7401);
    let hole = fresh_union_hole(&mut checker, var.0);

    // Cross-shape `_` → `∪`: an unbound monomorphic hole is kept as a pending
    // member (finalize resolves it through substitutions).
    let mono = checker.fresh_infer();
    checker.unify_assign_issue(hole, mono, span, "test");
    let got = members_of(&checker, var);
    assert!(
        got.contains(&mono),
        "pending `_` hole must be kept as a member"
    );

    // Once the monomorphic hole binds, a later flow contributes its resolution.
    checker.unify_assign_issue(mono, numerus, span, "test");
    checker.unify_assign_issue(hole, mono, span, "test");
    let got = members_of(&checker, var);
    assert!(
        got.contains(&numerus),
        "bound `_` resolution must accumulate"
    );
    assert!(checker.errors.is_empty());
}

#[test]
fn resolve_inferred_type_leaves_union_hole_visible() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let var = InferVar(7501);
    let hole = fresh_union_hole(&mut checker, var.0);

    // The hole resolves to itself (never collapsed to its member set) so
    // later `unify_with_policy` accumulation arms can still see it.
    assert_eq!(checker.resolve_inferred_type(hole), hole);

    // Accumulation must not create a substitution for the hole.
    checker.unify_assign_issue(hole, numerus, span, "test");
    assert_eq!(checker.resolve_inferred_type(hole), hole);
    assert!(
        checker.substitutions.borrow().get(&var).is_none(),
        "union holes must never enter the substitution map"
    );
}

#[test]
fn monomorphic_infer_path_is_unchanged() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();

    // `_` holes still bind a single inhabitant through substitutions.
    let mono = checker.fresh_infer();
    let result = checker.unify_assign_issue(mono, numerus, span, "test");
    assert_eq!(checker.resolve_inferred_type(result), numerus);
    assert_eq!(checker.resolve_inferred_type(mono), numerus);

    // No union accumulation happened anywhere.
    assert!(
        checker.union_members.borrow().is_empty(),
        "monomorphic `_` path must not touch union accumulation"
    );
}

// ---------------------------------------------------------------------------
// Source-level (analysis) acceptance
// ---------------------------------------------------------------------------

#[test]
fn source_union_hole_accepts_direct_flows() {
    test_support::assert_analysis_accepts(
        r"
incipit {
    fixum ∪ x ← 42
    varia ∪ y ← 1
    y ← 2
    nota x
    nota y
}
",
    );
}

#[test]
fn source_union_hole_accepts_heterogeneous_direct_flows() {
    test_support::assert_analysis_accepts(
        r#"
incipit {
    varia ∪ x ← 1
    x ← 2
    x ← "two"
    nota x
}
"#,
    );
}

#[test]
fn source_union_hole_accepts_hole_to_hole_merge() {
    // THU-U-05 hard gate: an unsolved `∪` hole is now a
    // `cannot_infer_union_type` error, so every hole here must accumulate.
    // Under the U-02 accumulation direction the flow target's hole absorbs the
    // flow source's member set (left absorbs right), so the declared slot gets
    // its own direct flow before the hole-to-hole merge keeps both solved.
    test_support::assert_analysis_accepts(
        r#"
incipit {
    fixum ∪ a ← 1
    fixum ∪ b ← "s"
    varia ∪ c ← 1
    c ← a
    nota a
    nota b
    nota c
}
"#,
    );
}

#[test]
fn source_underscore_hole_path_is_unchanged() {
    test_support::assert_analysis_accepts(
        r"
incipit {
    fixum _ r ← 1
    fixum _ s ← r
    nota s
}
",
    );
}
