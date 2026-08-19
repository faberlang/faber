//! PH-2/PH-3 pins: `numerus<_>` and marker twins as family-preserving holes.

use super::test_support;
use super::*;
use crate::codegen::Target;
use crate::lexer::{lex, Interner};
use crate::parser::parse;
use crate::semantic::passes::collect;
use crate::semantic::passes::resolve::resolve;
use crate::semantic::{MarkerFamily, NumericWidth, Resolver, TypeTable};

fn resolve_alias(source: &str) -> (TypeTable, crate::semantic::TypeId) {
    let mut parsed = parse(lex(source));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
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
        matches!(types.get(ty), Type::MarkerHole { family: MarkerFamily::Numerus, .. }),
        "numerus<_> must intern MarkerHole, got {:?}",
        types.get(ty)
    );

    let (types, ty) = resolve_alias("typus X = fractus<_>");
    assert!(
        matches!(types.get(ty), Type::MarkerHole { family: MarkerFamily::Fractus, .. }),
        "fractus<_>"
    );

    let (types, ty) = resolve_alias("typus X = modulus<_>");
    assert!(
        matches!(types.get(ty), Type::MarkerHole { family: MarkerFamily::Modulus, .. }),
        "modulus<_>"
    );

    let (types, ty) = resolve_alias("typus X = instans<_>");
    assert!(
        matches!(types.get(ty), Type::MarkerHole { family: MarkerFamily::Instans, .. }),
        "instans<_>"
    );
}

#[test]
fn resolve_rejects_convert_hint_hole() {
    let parsed = parse(lex("typus X = numerus<u32, _>"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    let result = resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(result.is_err(), "numerus<u32, _> must not intern a marker hole");
}

#[cfg(feature = "hir-rust")]
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

#[cfg(feature = "hir-rust")]
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

#[cfg(feature = "hir-rust")]
#[test]
fn bare_numerus_still_accepts_i32_via_default_path() {
    test_support::assert_accepts(
        r"incipit {
    fixum numerus n ← 1 ∷ numerus<i32>
    nota n
}",
    );
}

#[cfg(feature = "hir-rust")]
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

#[cfg(feature = "hir-rust")]
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

#[cfg(feature = "hir-rust")]
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

#[cfg(feature = "hir-rust")]
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
fn harness_exact_marker_does_not_widen() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let hole = checker
        .types
        .intern(Type::MarkerHole { family: MarkerFamily::Numerus, var: InferVar(1) });
    let i32_ty = checker
        .types
        .sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let span = crate::lexer::Span::default();
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
