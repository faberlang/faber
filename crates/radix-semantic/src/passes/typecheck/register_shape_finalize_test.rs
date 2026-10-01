//! VEC-2 pins: finalize canonicalizes the width/shape of register families
//! (`vector`, `matrix`, `sparsa`) so a solved index hole never survives in a
//! generic call's result type.

use super::*;
use crate::{Resolver, TypeTable};
use radix_lexer::{Interner, Span};
use radix_target::Target;

fn solved_hole(checker: &mut TypeChecker<'_>, value: u64) -> crate::IndexId {
    let hole = checker.fresh_index_hole();
    let literal = checker.types.intern_index(IndexExpr::Literal(value));
    checker.constrain_index_equal(hole, literal, Span::default());
    hole
}

#[test]
fn canonicalize_replaces_solved_hole_in_register_families() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let f32_ty = checker.types.primitive(Primitive::Fractus);
    let hole = solved_hole(&mut checker, 3);
    let literal = checker.types.intern_index(IndexExpr::Literal(3));
    let carriers = [
        checker.types.vector_with_width(f32_ty, hole),
        checker.types.matrix_with_shape(f32_ty, hole),
        checker.types.sparsa_with_shape(f32_ty, hole),
    ];
    let expected = [
        checker.types.vector_with_width(f32_ty, literal),
        checker.types.matrix_with_shape(f32_ty, literal),
        checker.types.sparsa_with_shape(f32_ty, literal),
    ];
    for (carrier, expected) in carriers.into_iter().zip(expected) {
        let canonical = checker.canonicalize_type(carrier);
        assert!(
            checker.types.equals(canonical, expected),
            "solved hole must canonicalize to the literal width, got {:?}",
            checker.types.get(canonical)
        );
    }
    assert!(checker.errors.is_empty(), "{:?}", checker.errors);
}
