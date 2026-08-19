//! PH-2/PH-3 pins: `numerus<_>` and marker twins as family-preserving holes.

use super::*;
use crate::passes::collect;
use crate::passes::resolve::resolve;
use crate::test_support;
use crate::{InstansPrecision, MarkerFamily, NumericWidth, Resolver, TypeTable};
use radix_hir::{HirItemKind, HirStatementKind};
use radix_lexer::{lex, Interner};
use radix_parser::parse;
use radix_target::Target;

fn resolve_alias(source: &str) -> (TypeTable, crate::TypeId) {
    let mut parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    resolve(program, &mut resolver, &parsed.interner, &mut types).expect("resolve");
    let alias = parsed.interner.intern("X");
    let def_id = resolver.lookup(alias).expect("alias def");
    let ty = resolver
        .get_symbol(def_id)
        .and_then(|s| s.ty)
        .expect("alias type");
    (types, ty)
}

#[test]
fn resolve_interns_marker_holes() {
    let (types, ty) = resolve_alias("typus X = numerus<_>");
    assert!(
        matches!(
            types.get(ty),
            Type::MarkerHole {
                family: MarkerFamily::Numerus,
                ..
            }
        ),
        "numerus<_> must intern MarkerHole, got {:?}",
        types.get(ty)
    );

    let (types, ty) = resolve_alias("typus X = fractus<_>");
    assert!(
        matches!(
            types.get(ty),
            Type::MarkerHole {
                family: MarkerFamily::Fractus,
                ..
            }
        ),
        "fractus<_>"
    );

    let (types, ty) = resolve_alias("typus X = modulus<_>");
    assert!(
        matches!(
            types.get(ty),
            Type::MarkerHole {
                family: MarkerFamily::Modulus,
                ..
            }
        ),
        "modulus<_>"
    );

    let (types, ty) = resolve_alias("typus X = instans<_>");
    assert!(
        matches!(
            types.get(ty),
            Type::MarkerHole {
                family: MarkerFamily::Instans,
                ..
            }
        ),
        "instans<_>"
    );
}

#[test]
fn resolve_rejects_convert_hint_hole() {
    let parsed = parse(lex("typus X = numerus<u32, _>"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    let result = resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(
        result.is_err(),
        "numerus<u32, _> must not intern a marker hole"
    );
}

#[test]
fn numerus_hole_solves_exact_sized_witness() {
    test_support::assert_accepts(
        r"incipit {
    fixum numerus<_> n ← 1 ∷ numerus<i32>
    fixum numerus<i32> m ← n
    nota m
}",
    );
}

#[test]
fn numerus_hole_materializes_bare_witness_to_i64() {
    test_support::assert_accepts(
        r"incipit {
    fixum numerus<_> n ← 42
    fixum numerus<i64> m ← n
    nota m
}",
    );
}

#[test]
fn bare_numerus_still_accepts_i32_via_default_path() {
    test_support::assert_accepts(
        r"incipit {
    fixum numerus n ← 1 ∷ numerus<i32>
    nota n
}",
    );
}

#[test]
fn numerus_hole_rejects_fractus_witness() {
    test_support::assert_rejects_with_issue(
        r"incipit {
    fixum numerus<_> n ← 1.0 ∷ fractus<f32>
    nota n
}",
        "expression_type_mismatch",
    );
}

#[test]
fn fractus_hole_solves_exact_sized_witness() {
    test_support::assert_accepts(
        r"incipit {
    fixum fractus<_> x ← 1.0 ∷ fractus<f32>
    fixum fractus<f32> y ← x
    nota y
}",
    );
}

#[test]
fn modulus_hole_solves_exact_width() {
    test_support::assert_analysis_accepts(
        r"incipit {
    fixum modulus<_> m ← 1 ∷ modulus<u32>
    fixum modulus<u32> n ← m
    nota n
}",
    );
}

#[test]
fn instans_hole_solves_exact_precision() {
    test_support::assert_analysis_accepts(
        r"functio pin(instans<ms> t) → instans<ms> {
    fixum instans<_> hole ← t
    redde hole
}",
    );
}

#[test]
fn unsolved_numerus_hole_errors() {
    test_support::assert_analysis_rejects_with_issue(
        r"functio k() → numerus<_> {
    tacet
}",
        "marker_width_unsolved",
    );
}

#[test]
fn numerus_hole_interns_sized_i32_not_i64() {
    let unit = test_support::analyze_ok(
        r"incipit {
    fixum numerus<_> n ← 1 ∷ numerus<i32>
    nota n
}",
    );
    let tys = test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 1);
    assert!(
        matches!(
            unit.types.get(tys[0]),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::I32)
        ),
        "numerus<_> ← i32 must intern sized i32, got {:?}",
        unit.types.get(tys[0])
    );
}

#[test]
fn numerus_hole_and_bare_are_two_interned_types() {
    let unit = test_support::analyze_ok(
        r"incipit {
    fixum numerus a ← 42
    fixum numerus<_> b ← 42
    nota a
    nota b
}",
    );
    let tys = test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 2);
    assert!(
        matches!(unit.types.get(tys[0]), Type::Primitive(Primitive::Numerus)),
        "bare numerus stays Primitive, got {:?}",
        unit.types.get(tys[0])
    );
    assert!(
        matches!(
            unit.types.get(tys[1]),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64)
        ),
        "numerus<_> from 42 materializes sized i64, got {:?}",
        unit.types.get(tys[1])
    );
    assert_ne!(tys[0], tys[1], "same width, two interned types");
    assert!(
        unit.types.equals(tys[0], tys[1]),
        "bare numerus still equals sized i64"
    );
}

#[test]
fn fractus_hole_interns_sized_f32() {
    let unit = test_support::analyze_ok(
        r"incipit {
    fixum fractus<_> x ← 1.0 ∷ fractus<f32>
    nota x
}",
    );
    let tys = test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 1);
    assert!(
        matches!(
            unit.types.get(tys[0]),
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
        ),
        "got {:?}",
        unit.types.get(tys[0])
    );
}

#[test]
fn fractus_hole_materializes_bare_witness_to_f64() {
    let unit = test_support::analyze_ok(
        r"incipit {
    fixum fractus<_> x ← 1.0
    nota x
}",
    );
    let tys = test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 1);
    assert!(
        matches!(
            unit.types.get(tys[0]),
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64)
        ),
        "fractus<_> from 1.0 must materialize sized f64, got {:?}",
        unit.types.get(tys[0])
    );
}

#[test]
fn fractus_hole_rejects_numerus_witness() {
    test_support::assert_rejects_with_issue(
        r"incipit {
    fixum fractus<_> x ← 1 ∷ numerus<i32>
    nota x
}",
        "expression_type_mismatch",
    );
}

#[test]
fn modulus_hole_interns_u32() {
    let unit = test_support::analyze_ok(
        r"incipit {
    fixum modulus<_> m ← 1 ∷ modulus<u32>
    nota m
}",
    );
    let tys = test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 1);
    assert!(
        matches!(unit.types.get(tys[0]), Type::ModularWord(NumericWidth::U32)),
        "got {:?}",
        unit.types.get(tys[0])
    );
}

#[test]
fn modulus_hole_rejects_signed_numerus() {
    test_support::assert_analysis_rejects_with_issue(
        r"incipit {
    fixum modulus<_> m ← 1 ∷ numerus<i32>
    nota m
}",
        "expression_type_mismatch",
    );
}

#[test]
fn instans_hole_interns_millis() {
    let unit = test_support::analyze_ok(
        r"functio pin(instans<ms> t) → vacuum {
    fixum instans<_> hole ← t
    nota hole
}",
    );
    let HirItemKind::Function(function) = &unit.hir.as_ref().expect("hir").items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("body");
    let HirStatementKind::Local(local) = &body.statements[0].kind else {
        panic!("expected local");
    };
    let ty = local.ty.expect("hole type");
    assert!(
        matches!(
            unit.types.get(ty),
            Type::SizedInstans(InstansPrecision::Millis)
        ),
        "got {:?}",
        unit.types.get(ty)
    );
}

#[test]
fn instans_hole_rejects_numerus_witness() {
    test_support::assert_rejects_with_issue(
        r"incipit {
    fixum instans<_> t ← 1
    nota t
}",
        "expression_type_mismatch",
    );
}

#[test]
fn instans_hole_from_bare_stays_seconds() {
    let unit = test_support::analyze_ok(
        r"functio pin(instans t) → vacuum {
    fixum instans<_> hole ← t
    nota hole
}",
    );
    let HirItemKind::Function(function) = &unit.hir.as_ref().expect("hir").items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("body");
    let HirStatementKind::Local(local) = &body.statements[0].kind else {
        panic!("expected local");
    };
    let ty = local.ty.expect("hole type");
    assert!(
        matches!(unit.types.get(ty), Type::Primitive(Primitive::Instans)),
        "bare instans witness keeps seconds, got {:?}",
        unit.types.get(ty)
    );
}

#[test]
fn unsolved_numerus_binding_errors() {
    test_support::assert_analysis_rejects_with_issue(
        r"functio k() → vacuum {
    fixum numerus<_> n
    tacet
}",
        "marker_width_unsolved",
    );
}

#[test]
fn unsolved_fractus_and_modulus_holes_error() {
    test_support::assert_analysis_rejects_with_issue(
        r"functio k() → fractus<_> {
    tacet
}",
        "marker_width_unsolved",
    );
    test_support::assert_analysis_rejects_with_issue(
        r"functio k() → modulus<_> {
    tacet
}",
        "marker_width_unsolved",
    );
}

#[test]
fn convert_hint_hole_rejected_as_binding() {
    test_support::assert_analysis_rejects_with_issue(
        r"incipit {
    fixum numerus<u32, _> n ← 1
    nota n
}",
        "numerus_width_params",
    );
}

#[test]
fn harness_exact_i32_does_not_join_i64() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let hole = checker.types.intern(Type::MarkerHole {
        family: MarkerFamily::Numerus,
        var: InferVar(1),
    });
    let i32_ty = checker
        .types
        .sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64_ty = checker
        .types
        .sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let span = radix_lexer::Span::default();
    checker.unify_assign_issue(i32_ty, hole, span, "expression_type_mismatch");
    assert!(checker.errors.is_empty(), "{:?}", checker.errors);
    checker.unify_assign_issue(i64_ty, hole, span, "expression_type_mismatch");
    assert!(
        !checker.errors.is_empty(),
        "numerus<_> solved to i32 must not then accept i64"
    );
}

#[test]
fn harness_family_mismatch_does_not_bind() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let hole = checker.types.intern(Type::MarkerHole {
        family: MarkerFamily::Numerus,
        var: InferVar(2),
    });
    let f32_ty = checker
        .types
        .sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let span = radix_lexer::Span::default();
    let unified = checker.unify_assign_issue(f32_ty, hole, span, "expression_type_mismatch");
    assert!(!checker.errors.is_empty());
    assert_eq!(unified, checker.error_type);
}

#[test]
fn harness_exact_marker_does_not_widen() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let hole = checker.types.intern(Type::MarkerHole {
        family: MarkerFamily::Numerus,
        var: InferVar(1),
    });
    let i32_ty = checker
        .types
        .sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let span = radix_lexer::Span::default();
    let unified = checker.unify_assign_issue(i32_ty, hole, span, "expression_type_mismatch");
    assert!(
        checker.errors.is_empty(),
        "numerus<i32> → numerus<_> must unify: {:?}",
        checker.errors
    );
    assert!(
        matches!(
            checker.types.get(checker.resolve_inferred_type(unified)),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::I32)
        ),
        "exact marker, not widened, got {:?}",
        checker.types.get(checker.resolve_inferred_type(unified))
    );
}
