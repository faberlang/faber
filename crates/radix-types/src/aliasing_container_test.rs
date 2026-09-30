//! al2: the shared aliasing-container classification (R1: an unmarked bind
//! aliases its source). Register-class `vector`/`matrix` alias like every
//! other aggregate (advisor ruling 2026-09-30 Q1), and an optional container
//! aliases through the option (Q2).

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
fn optional_vector_aliases_through_the_option() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim4 = types.intern_index(IndexExpr::Literal(4));
    let vector = types.vector_with_width(f32_ty, dim4);
    let optional_vector = types.option(vector);
    assert!(types.is_aliasing_container_through_option(optional_vector));
}
