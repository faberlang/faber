//! Tests for index expression unification.
//!
//! These tests prove that `IndexUnifier` correctly unifies index expressions
//! (literals, variables, tuples) and reports unification failures with the
//! expected `IndexUnifyError` variants.

use super::{IndexUnifier, IndexUnifyError};
use crate::TypeTable;
use crate::index::{IndexExpr, IndexVar};
use handroll::FxHashMap;
use radix_lexer::Symbol;

fn lit(types: &mut TypeTable, n: u64) -> crate::IndexId {
    types.intern_index(IndexExpr::Literal(n))
}

fn param(types: &mut TypeTable, name: u32) -> crate::IndexId {
    types.intern_index(IndexExpr::Param(Symbol(name)))
}

fn infer(types: &mut TypeTable, id: u32) -> crate::IndexId {
    types.intern_index(IndexExpr::Infer(IndexVar(id)))
}

fn tuple(types: &mut TypeTable, dims: &[crate::IndexId]) -> crate::IndexId {
    types.intern_index(IndexExpr::Tuple(dims.to_vec()))
}

#[test]
fn strict_unify_literals_and_params() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let two_a = lit(&mut types, 2);
    let two_b = lit(&mut types, 2);
    let three = lit(&mut types, 3);
    let n = param(&mut types, 1);
    let n_copy = param(&mut types, 1);
    let m = param(&mut types, 2);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    assert!(unifier.unify(two_a, two_b).is_ok());
    assert_eq!(unifier.unify(two_a, three), Err(IndexUnifyError::Mismatch));
    assert!(unifier.unify(n, n_copy).is_ok());
    assert_eq!(unifier.unify(n, m), Err(IndexUnifyError::Mismatch));
}

#[test]
fn strict_unify_binds_infer_vars() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let hole = infer(&mut types, 0);
    let two = lit(&mut types, 2);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    let unified = unifier.unify(hole, two).expect("bind infer");
    let recorded = subs.get(&IndexVar(0)).copied();

    assert!(matches!(types.get_index(unified), IndexExpr::Literal(2)));
    assert_eq!(recorded, Some(two));
}

#[test]
fn strict_unify_tuple_requires_matching_rank() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let d2 = lit(&mut types, 2);
    let d3 = lit(&mut types, 3);
    let a = tuple(&mut types, &[d2, d3]);
    let b = tuple(&mut types, &[d2, d3]);
    let c = tuple(&mut types, &[d2]);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    assert!(unifier.unify(a, b).is_ok());
    assert_eq!(unifier.unify(a, c), Err(IndexUnifyError::Mismatch));
}

#[test]
fn broadcast_unifies_n_with_n1_to_n_n() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let n = param(&mut types, 1);
    let one = lit(&mut types, 1);
    let left = tuple(&mut types, &[n]);
    let right = tuple(&mut types, &[n, one]);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    let result = unifier.unify_broadcast(left, right).expect("broadcast");
    let IndexExpr::Tuple(dims) = types.get_index(result) else {
        panic!("expected tuple result");
    };
    assert_eq!(dims.len(), 2);
    assert!(matches!(types.get_index(dims[0]), IndexExpr::Param(s) if *s == Symbol(1)));
    assert!(matches!(types.get_index(dims[1]), IndexExpr::Param(s) if *s == Symbol(1)));
}

#[test]
fn broadcast_stretches_literal_one() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let d2a = lit(&mut types, 2);
    let d1 = lit(&mut types, 1);
    let d2b = lit(&mut types, 2);
    let d3 = lit(&mut types, 3);
    let left = tuple(&mut types, &[d2a, d1]);
    let right = tuple(&mut types, &[d2b, d3]);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    let result = unifier.unify_broadcast(left, right).expect("broadcast");
    let IndexExpr::Tuple(dims) = types.get_index(result) else {
        panic!("expected tuple result");
    };
    assert!(matches!(types.get_index(dims[0]), IndexExpr::Literal(2)));
    assert!(matches!(types.get_index(dims[1]), IndexExpr::Literal(3)));
}

#[test]
fn broadcast_unspecified_bridge_absorbs_peer() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let d2 = lit(&mut types, 2);
    let d3 = lit(&mut types, 3);
    let unspecified = types.unspecified_shape();
    let shape = tuple(&mut types, &[d2, d3]);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    let result = unifier.unify_broadcast(unspecified, shape).expect("bridge");
    assert_eq!(result, shape);
}

#[test]
fn occurs_check_rejects_cyclic_tuple_binding() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let hole = infer(&mut types, 0);
    let cyclic = tuple(&mut types, &[hole]);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    assert_eq!(unifier.unify(hole, cyclic), Err(IndexUnifyError::Occurs));
}

#[test]
fn affine_and_exact_division_unification_invert_only_infer_bases() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let affine_base = infer(&mut types, 10);
    let affine =
        radix_types::add_index_constant(&mut types, affine_base, -1).expect("symbolic affine term");
    let four = lit(&mut types, 4);
    let divided_base = infer(&mut types, 11);
    let divided = radix_types::divide_index_exact(&mut types, divided_base, 2)
        .expect("symbolic exact division");
    let three = lit(&mut types, 3);

    {
        let mut unifier = IndexUnifier::new(&mut types, &mut subs);
        assert!(unifier.unify(affine, four).is_ok());
        assert!(unifier.unify(divided, three).is_ok());
    }
    assert!(matches!(
        types.get_index(subs[&IndexVar(10)]),
        IndexExpr::Literal(5)
    ));
    assert!(matches!(
        types.get_index(subs[&IndexVar(11)]),
        IndexExpr::Literal(6)
    ));
}

#[test]
fn runtime_shape_unification_inverts_affine_infer_without_erasing_runtime_identity() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let hole = infer(&mut types, 12);
    let expected =
        radix_types::add_index_constant(&mut types, hole, -1).expect("symbolic affine term");
    let runtime = types.intern_index(IndexExpr::Runtime(
        radix_types::RuntimeExtentId::FunctionParameterAxis {
            parameter: 0,
            axis: 0,
        },
    ));

    {
        let mut unifier = IndexUnifier::new(&mut types, &mut subs);
        assert!(unifier.unify(expected, runtime).is_ok());
    }
    let inferred = subs[&IndexVar(12)];
    assert!(matches!(
        types.get_index(inferred),
        IndexExpr::Affine { base, offset: 1 } if *base == runtime
    ));
    assert_eq!(
        radix_types::evaluate_index_dimension(inferred, &types),
        Ok(None)
    );
}

#[test]
fn unification_rejects_invalid_exact_division_even_when_identical() {
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let five = lit(&mut types, 5);
    let invalid = types.intern_index(IndexExpr::Divide {
        numerator: five,
        divisor: 2,
    });
    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    assert_eq!(
        unifier.unify(invalid, invalid),
        Err(IndexUnifyError::Mismatch)
    );
}

#[test]
fn broadcast_unifies_b1_with_bd_to_bd() {
    // The Phase-2 broadcast pin at the unifier level: [B,1] + [B,D] → [B,D].
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let b = param(&mut types, 1);
    let d = param(&mut types, 2);
    let one = lit(&mut types, 1);
    let left = tuple(&mut types, &[b, one]);
    let right = tuple(&mut types, &[b, d]);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    let result = unifier.unify_broadcast(left, right).expect("broadcast");
    let IndexExpr::Tuple(dims) = types.get_index(result) else {
        panic!("expected tuple result");
    };
    assert_eq!(dims.len(), 2);
    assert!(matches!(types.get_index(dims[0]), IndexExpr::Param(s) if *s == Symbol(1)));
    assert!(matches!(types.get_index(dims[1]), IndexExpr::Param(s) if *s == Symbol(2)));
}

#[test]
fn strict_unify_tuple_params_match_and_distinct_reject() {
    // Same-symbol param tuples unify; a distinct param in one dim does not
    // (D5-conservative: distinct param symbols never silently merge).
    let mut types = TypeTable::new();
    let mut subs = FxHashMap::default();
    let b = param(&mut types, 1);
    let d = param(&mut types, 2);
    let m = param(&mut types, 3);
    let left = tuple(&mut types, &[b, d]);
    let same = tuple(&mut types, &[b, d]);
    let different = tuple(&mut types, &[m, d]);

    let mut unifier = IndexUnifier::new(&mut types, &mut subs);
    assert!(unifier.unify(left, same).is_ok());
    assert_eq!(
        unifier.unify(left, different),
        Err(IndexUnifyError::Mismatch)
    );
}
