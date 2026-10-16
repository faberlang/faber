//! U-04 — mixed-source join under a union-hole (`∪`) expectation.
//!
//! Arrays, `si`/`secus` (and sibling `casu`/match branch joins) accumulate
//! member types when the expected type is a union hole, instead of going
//! through the monomorphic `common_type` join (which would reject
//! heterogeneous branches). Monomorphic and concrete expectations keep
//! today's `common_type` behavior byte-for-byte — no silent union widen.
//!
//! Closeout filter: `cargo test -p radix --lib type_hole_union_join`.

use super::*;
use crate::passes::typecheck::test_support;
use radix_target::Target;

// NOTE: `TypeChecker::new` borrows the `TypeTable` for the checker lifetime,
// so tests drive everything through `checker.types` (the same arena) and
// never keep a second handle to the table.

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
// Checker-level join primitives
// ---------------------------------------------------------------------------

#[test]
fn common_type_accumulates_into_union_hole() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let textus = checker.types.primitive(Primitive::Textus);
    let var = InferVar(8101);
    let hole = fresh_union_hole(&mut checker, var.0);

    // Hole on the left: the other side becomes a member, the hole stays open.
    let result = checker.common_type(hole, numerus, span);
    assert_eq!(result, hole, "common_type must keep the hole open");
    assert_eq!(members_of(&checker, var), vec![numerus]);

    // Hole on the right: same accumulation, no error.
    let result = checker.common_type(textus, hole, span);
    assert_eq!(result, hole);
    let got = members_of(&checker, var);
    assert_eq!(got.len(), 2);
    assert!(got.contains(&numerus) && got.contains(&textus));

    // Hole against itself is a no-op.
    let result = checker.common_type(hole, hole, span);
    assert_eq!(result, hole);
    assert!(
        checker.errors.is_empty(),
        "union-hole joins must not diagnose"
    );
}

#[test]
fn common_type_union_hole_flattens_explicit_union_and_merges() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let textus = checker.types.primitive(Primitive::Textus);
    let var = InferVar(8102);
    let hole = fresh_union_hole(&mut checker, var.0);

    // An explicit union value joins by contributing each member.
    let union_ty = checker.types.intern(Type::Union(vec![numerus, textus]));
    let result = checker.common_type(union_ty, hole, span);
    assert_eq!(result, hole);
    let got = members_of(&checker, var);
    assert_eq!(got.len(), 2);
    assert!(got.contains(&numerus) && got.contains(&textus));

    // A second union hole merges its member set into this one (copy).
    let var_b = InferVar(8103);
    let hole_b = fresh_union_hole(&mut checker, var_b.0);
    checker.unify_assign_issue(hole_b, numerus, span, "test");
    let result = checker.common_type(hole, hole_b, span);
    assert_eq!(result, hole);
    assert_eq!(
        members_of(&checker, var).len(),
        2,
        "merge must dedup shared members"
    );
    assert!(checker.errors.is_empty());
}

#[test]
fn common_type_monomorphic_infer_does_not_accumulate() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();

    // `common_type` is not the monomorphic bind path: a `_` hole passed to it
    // keeps today's behavior (no union accumulation, no silent widen).
    let mono = checker.fresh_infer();
    checker.common_type(mono, numerus, span);
    assert!(
        checker.union_members.borrow().is_empty(),
        "monomorphic join must not touch union accumulation"
    );
}

#[test]
fn common_type_heterogeneous_monomorphic_still_mismatches() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();
    let textus = checker.types.primitive(Primitive::Textus);

    // No silent union widen: numerus vs textus without a hole still errors.
    let result = checker.common_type(numerus, textus, span);
    assert_eq!(result, checker.error_type);
    assert!(
        !checker.errors.is_empty(),
        "heterogeneous monomorphic join must diagnose"
    );
    assert!(checker.union_members.borrow().is_empty());
}

#[test]
fn common_type_concrete_join_unchanged() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();
    let numerus = checker.numerus_type();

    let result = checker.common_type(numerus, numerus, span);
    assert_eq!(checker.resolve_inferred_type(result), numerus);
    assert!(checker.errors.is_empty());
}

// ---------------------------------------------------------------------------
// Source-level (analysis) acceptance / rejection
// ---------------------------------------------------------------------------

// NOTE: the `si`/`secus` statement form is statement-only in the parser; the
// expression-level branch join is the `sic`/`secus` ternary, which lowers to
// the same `HirExpressionKind::Si` → `check_if` path as a `si` statement.

#[test]
fn source_branch_join_accumulates_into_union_hole() {
    test_support::assert_analysis_accepts_la(
        r#"
incipit {
    sit numerus x ← 1
    sit ∪ either ← x ≻ 0 ✓ 1 ✗ "x"
    nota either
}
"#,
    );
}

#[test]
fn source_branch_join_homogeneous_under_concrete_expectation_unchanged() {
    test_support::assert_analysis_accepts_la(
        r"
incipit {
    sit numerus x ← 1
    sit numerus y ← x ≻ 0 ✓ 1 ✗ 2
    nota y
}
",
    );
}

#[test]
fn source_array_accumulates_into_union_hole_elements() {
    test_support::assert_analysis_accepts_la(
        r#"
incipit {
    sit lista<∪> xs ← [1, "x"]
    nota xs
}
"#,
    );
}

#[test]
fn source_array_accumulates_into_union_hole_with_spread() {
    test_support::assert_analysis_accepts_la(
        r"
incipit {
    sit lista<numerus> ys ← [1, 2]
    sit lista<∪> xs ← [1, sparge ys]
    nota xs
}
",
    );
}

#[test]
fn source_array_underscore_hole_still_rejects_heterogeneous() {
    test_support::assert_analysis_rejects_with_issue_la(
        r#"
incipit {
    sit lista<_> xs ← [1, "x"]
    nota xs
}
"#,
        "common_type_mismatch",
    );
}

#[test]
fn source_array_concrete_expected_rejects_heterogeneous() {
    test_support::assert_analysis_rejects_with_issue_la(
        r#"
incipit {
    sit lista<numerus> xs ← [1, "x"]
    nota xs
}
"#,
        "common_type_mismatch",
    );
}

#[test]
fn source_array_concrete_expected_accepts_homogeneous() {
    test_support::assert_analysis_accepts_la(
        r"
incipit {
    sit lista<numerus> xs ← [1, 2]
    nota xs
}
",
    );
}

#[test]
fn source_branch_join_heterogeneous_without_hole_still_mismatches() {
    test_support::assert_analysis_rejects_with_issue_la(
        r#"
incipit {
    sit numerus x ← 1
    sit _ y ← x ≻ 0 ✓ 1 ✗ "x"
    nota y
}
"#,
        "common_type_mismatch",
    );
}
