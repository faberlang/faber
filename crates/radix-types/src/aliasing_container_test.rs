//! al2: the shared aliasing-container classification (R1: an unmarked bind
//! aliases its source), and the bind predicate built on it.
//!
//! Register-class `vector`/`matrix` are containers — they carry lanes of their
//! own — but an unmarked bind of one COPIES (operator ruling 7b25889f R1,
//! which retires the 2026-09-30 Q1 aliasing posture for them), so
//! [`TypeTable::binds_by_sharing`] says no. An optional container aliases
//! through the option (Q2).

use crate::index::IndexExpr;
use crate::{NumericWidth, Primitive, TypeTable};

#[test]
fn vector_and_matrix_are_aliasing_containers() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim4 = types.intern_index(IndexExpr::Literal(4));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim2, dim2]));
    let vector = types.vector_with_width(f32_ty, dim4);
    let matrix = types.matrix_with_shape(f32_ty, shape);
    assert!(types.is_aliasing_container(vector));
    assert!(types.is_aliasing_container(matrix));
}

#[test]
fn register_class_values_copy_on_an_unmarked_bind() {
    // Operator ruling 7b25889f R1: `b ← a` copies for a vector or matrix, as
    // for a scalar, so neither is a sharing bind and neither aliases through
    // an option.
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let int = types.primitive(Primitive::Numerus);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim4 = types.intern_index(IndexExpr::Literal(4));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim2, dim2]));
    let vector = types.vector_with_width(f32_ty, dim4);
    let matrix = types.matrix_with_shape(f32_ty, shape);
    let optional_vector = types.option(vector);
    let optional_matrix = types.option(matrix);
    let list = types.array(int);
    let optional_list = types.option(list);
    assert!(!types.binds_by_sharing(vector));
    assert!(!types.binds_by_sharing(matrix));
    assert!(!types.binds_by_sharing_through_option(vector));
    assert!(!types.binds_by_sharing_through_option(optional_vector));
    assert!(!types.binds_by_sharing_through_option(optional_matrix));
    // Referential containers still share on an unmarked bind.
    assert!(types.binds_by_sharing(list));
    assert!(types.binds_by_sharing_through_option(optional_list));
    assert!(!types.binds_by_sharing(int));
}

#[test]
fn scalars_and_text_are_not_aliasing_containers() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let text = types.primitive(Primitive::Textus);
    let optional_int = types.option(int);
    assert!(!types.is_aliasing_container(int));
    assert!(!types.is_aliasing_container(text));
    assert!(!types.is_aliasing_container_through_option(int));
    assert!(!types.is_aliasing_container_through_option(text));
    assert!(!types.is_aliasing_container_through_option(optional_int));
}

#[test]
fn option_peels_only_in_the_through_option_form() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let list = types.array(int);
    let optional_list = types.option(list);
    assert!(types.is_aliasing_container(list));
    assert!(!types.is_aliasing_container(optional_list));
    assert!(types.is_aliasing_container_through_option(list));
    assert!(types.is_aliasing_container_through_option(optional_list));
}

#[test]
fn optional_vector_peels_as_a_container_but_copies_on_bind() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim4 = types.intern_index(IndexExpr::Literal(4));
    let vector = types.vector_with_width(f32_ty, dim4);
    let optional_vector = types.option(vector);
    assert!(types.is_aliasing_container_through_option(optional_vector));
    assert!(!types.binds_by_sharing_through_option(optional_vector));
}
