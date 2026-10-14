//! PH-2/PH-3 pins: the policy-word and `instans` marker holes as
//! family-preserving holes. The `numerus<_>` / `fractus<_>` holes retired with
//! the wrapped numeric spelling (N7d, DB-3); the checker harness tests below
//! drive the `Modulus` family through hand-built `MarkerHole` types.

use super::*;
use crate::passes::collect;
use crate::passes::resolve::resolve;
use crate::test_support;
use crate::{InstansPrecision, MarkerFamily, NumericWidth, Resolver, TypeTable};
use radix_hir::{HirItemKind, HirStatementKind};
use radix_lexer::Interner;
use radix_parser::parse;
use radix_target::Target;

fn latin_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn resolve_alias(source: &str) -> (TypeTable, crate::TypeId) {
    let mut parsed = parse(radix_lexer::lex_with_locale_pack(source, &latin_pack()));
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
fn the_numeric_marker_holes_are_retired_with_the_wrapper() {
    for source in [
        "incipit {\n    fixum numerus<_> n ← 1\n    nota n\n}",
        "incipit {\n    fixum fractus<_> x ← 1.0\n    nota x\n}",
        "functio k() → numerus<_> {\n    tacet\n}",
    ] {
        test_support::assert_rejects_with_issue_la(source, "numeric_wrapper_retired");
    }
}

#[test]
fn bare_numerus_still_accepts_i32_via_default_path() {
    test_support::assert_accepts_la(
        r"incipit {
    fixum numerus n ← 1 ∷ i32
    nota n
}",
    );
}

#[test]
fn modulus_hole_solves_exact_width() {
    test_support::assert_analysis_accepts_la(
        r"incipit {
    fixum modulus<_> m ← 1 ∷ modulus<u32>
    fixum modulus<u32> n ← m
    nota n
}",
    );
}

#[test]
fn instans_hole_solves_exact_precision() {
    test_support::assert_analysis_accepts_la(
        r"functio pin(instans<ms> t) → instans<ms> {
    fixum instans<_> hole ← t
    redde hole
}",
    );
}

#[test]
fn modulus_hole_interns_u32() {
    let unit = test_support::analyze_ok_la(
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
    test_support::assert_analysis_rejects_with_issue_la(
        r"incipit {
    fixum modulus<_> m ← 1 ∷ i32
    nota m
}",
        "expression_type_mismatch",
    );
}

#[test]
fn instans_hole_interns_millis() {
    let unit = test_support::analyze_ok_la(
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
    test_support::assert_rejects_with_issue_la(
        r"incipit {
    fixum instans<_> t ← 1
    nota t
}",
        "expression_type_mismatch",
    );
}

#[test]
fn instans_hole_from_bare_stays_seconds() {
    let unit = test_support::analyze_ok_la(
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
fn unsolved_modulus_hole_errors() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"functio k() → modulus<_> {
    tacet
}",
        "marker_width_unsolved",
    );
}

#[test]
fn harness_exact_u32_does_not_join_u64() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let hole = checker.types.intern(Type::MarkerHole {
        family: MarkerFamily::Modulus,
        var: InferVar(1),
    });
    let u32_ty = checker.types.intern(Type::ModularWord(NumericWidth::U32));
    let u64_ty = checker.types.intern(Type::ModularWord(NumericWidth::U64));
    let span = radix_lexer::Span::default();
    checker.unify_assign_issue(u32_ty, hole, span, "expression_type_mismatch");
    assert!(checker.errors.is_empty(), "{:?}", checker.errors);
    checker.unify_assign_issue(u64_ty, hole, span, "expression_type_mismatch");
    assert!(
        !checker.errors.is_empty(),
        "modulus<_> solved to u32 must not then accept u64"
    );
}

#[test]
fn harness_family_mismatch_does_not_bind() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let hole = checker.types.intern(Type::MarkerHole {
        family: MarkerFamily::Modulus,
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
        family: MarkerFamily::Modulus,
        var: InferVar(1),
    });
    let u32_ty = checker.types.intern(Type::ModularWord(NumericWidth::U32));
    let span = radix_lexer::Span::default();
    let unified = checker.unify_assign_issue(u32_ty, hole, span, "expression_type_mismatch");
    assert!(
        checker.errors.is_empty(),
        "u32 → modulus<_> must unify: {:?}",
        checker.errors
    );
    assert!(
        matches!(
            checker.types.get(checker.resolve_inferred_type(unified)),
            Type::ModularWord(NumericWidth::U32)
        ),
        "exact marker, not widened, got {:?}",
        checker.types.get(checker.resolve_inferred_type(unified))
    );
}
