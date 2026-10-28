//! Ranged store policies: a range is its own generic-argument
//! slot, `saturatus<u32, 0‥8>`. Both ends are always written, `∞` / `-∞` mean
//! "no bound on that side", and a one-sided range is refused by name.

use crate::test_support::{assert_parse_error_issue, parse_ok};
use radix_syntax::{GenericArgExpr, RangeBoundExpr, RangeKind, StmtKind, TypeExprKind};

fn range_of(source: &str) -> (RangeBoundExpr, RangeBoundExpr, RangeKind) {
    let result = parse_ok(source);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected a binding: {:?}", program.statements[0].kind);
    };
    let ty = decl.ty.as_ref().expect("annotation");
    let TypeExprKind::Named(_, args) = &ty.kind else {
        panic!("expected a named type: {:?}", ty.kind);
    };
    assert_eq!(args.len(), 2, "width then range: {args:?}");
    assert!(matches!(args[0], GenericArgExpr::Type(_)), "{:?}", args[0]);
    let GenericArgExpr::Range(range) = &args[1] else {
        panic!("expected a range slot: {:?}", args[1]);
    };
    (range.lo.clone(), range.hi.clone(), range.kind)
}

fn literal(bound: &RangeBoundExpr) -> i128 {
    match bound {
        RangeBoundExpr::Literal { value, .. } => *value,
        other => panic!("expected a literal bound: {other:?}"),
    }
}

#[test]
fn the_half_open_glyph_is_an_exclusive_range_argument() {
    let (lo, hi, kind) = range_of("sit saturatus<u32, 0‥8> x ← 1\n");
    assert_eq!(literal(&lo), 0);
    assert_eq!(literal(&hi), 8);
    assert!(matches!(kind, RangeKind::Exclusive));
}

#[test]
fn the_inclusive_glyph_is_an_inclusive_range_argument() {
    let (lo, hi, kind) = range_of("sit exactus<u8, 1…5> x ← 1\n");
    assert_eq!(literal(&lo), 1);
    assert_eq!(literal(&hi), 5);
    assert!(matches!(kind, RangeKind::Inclusive));
}

#[test]
fn negative_bounds_parse_as_signed_literals() {
    let (lo, hi, _) = range_of("sit exactus<i32, -2‥3> x ← 1\n");
    assert_eq!(literal(&lo), -2);
    assert_eq!(literal(&hi), 3);
}

#[test]
fn infinity_is_an_unbounded_end_with_its_sign() {
    let (lo, hi, _) = range_of("sit saturatus<i32, -∞‥∞> x ← 1\n");
    assert!(matches!(
        lo,
        RangeBoundExpr::Unbounded { negative: true, .. }
    ));
    assert!(matches!(
        hi,
        RangeBoundExpr::Unbounded {
            negative: false,
            ..
        }
    ));
}

#[test]
fn a_name_is_a_named_bound_left_to_the_checker() {
    let (lo, hi, _) = range_of("sit modulus<u32, 0‥W> x ← 1\n");
    assert_eq!(literal(&lo), 0);
    assert!(matches!(hi, RangeBoundExpr::Name { lifted: false, .. }));
}

#[test]
fn a_size_lift_is_a_lifted_named_bound() {
    let (_, hi, _) = range_of("sit modulus<u32, 0‥magnitudo W> x ← 1\n");
    assert!(matches!(hi, RangeBoundExpr::Name { lifted: true, .. }));
}

#[test]
fn a_one_sided_range_is_refused_by_name() {
    assert_parse_error_issue("sit saturatus<u32, 0…> x ← 1\n", "range_bound_required");
}

#[test]
fn a_range_missing_its_low_end_is_a_parse_error() {
    let result = crate::test_support::parse_program("sit saturatus<u32, ‥8> x ← 1\n");
    assert!(!result.success(), "a low end is always written");
}

#[test]
fn a_range_does_not_displace_the_size_argument_of_other_heads() {
    // `list<int, 4>` style arguments stay types and sizes; only `lo‥hi` is a
    // range slot.
    let result = parse_ok("sit lista<numerus> xs ← [1, 2]\n");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected a binding");
    };
    let ty = decl.ty.as_ref().expect("annotation");
    let TypeExprKind::Named(_, args) = &ty.kind else {
        panic!("expected a named type: {:?}", ty.kind);
    };
    assert!(
        args.iter()
            .all(|arg| !matches!(arg, GenericArgExpr::Range(_)))
    );
}
