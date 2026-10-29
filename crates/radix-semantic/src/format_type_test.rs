//! Tests for Faber type formatting and rendering.
//!
//! These tests prove that `format_faber_type` produces correct surface syntax
//! for tensor, indexed, numeric, and collection types.

use super::format_faber_type;
use crate::{NumericWidth, Primitive, Type, TypeTable, index::IndexExpr};
use radix_lexer::{Interner, lex};
use radix_parser::parse;

#[test]
fn format_faber_type_renders_indexed_tensor() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let rank0 = types.rank0_shape();
    let tensor = types.tensor_with_shape(elem, rank0);

    let name = |_| "unused".to_owned();
    let rendered = format_faber_type(tensor, &types, &interner, &name);
    assert_eq!(rendered, "tensor<f32, []>");
}

#[test]
fn format_faber_type_renders_literal_tuple_shape() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let elem = types.primitive(Primitive::Numerus);
    let d2 = types.intern_index(IndexExpr::Literal(2));
    let d3 = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![d2, d3]));
    let tensor = types.tensor_with_shape(elem, shape);

    let name = |_| "unused".to_owned();
    let rendered = format_faber_type(tensor, &types, &interner, &name);
    assert_eq!(rendered, "tensor<numerus, [2, 3]>");
}

#[test]
fn format_faber_type_renders_matrix_shape() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let d4 = types.intern_index(IndexExpr::Literal(4));
    let shape = types.intern_index(IndexExpr::Tuple(vec![d4, d4]));
    let matrix = types.matrix_with_shape(elem, shape);

    let name = |_| "unused".to_owned();
    let rendered = format_faber_type(matrix, &types, &interner, &name);
    assert_eq!(rendered, "matrix<f32, [4, 4]>");
}

#[test]
fn format_faber_type_renders_atomic_element() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let atomic = types.atomic(elem);

    let name = |_| "unused".to_owned();
    let rendered = format_faber_type(atomic, &types, &interner, &name);
    assert_eq!(rendered, "atomic<i32>");
}

#[test]
fn format_faber_type_renders_failable_promissum_channels() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let future = types.promissum_failable(numerus, textus);

    let name = |_| "unused".to_owned();
    let rendered = format_faber_type(future, &types, &interner, &name);

    assert_eq!(rendered, "promissum<numerus ⇥ textus>");
}

#[test]
fn format_faber_type_renders_record_as_sorted_ratio() {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let w = interner.intern("w");
    let g = interner.intern("g");
    let fractus_f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let numerus = types.primitive(Primitive::Numerus);
    // Insertion order is w then g; canonical render is sorted by label.
    let record = types.record([(w, numerus), (g, fractus_f32)]);

    let name = |_| "unused".to_owned();
    let rendered = format_faber_type(record, &types, &interner, &name);
    assert_eq!(rendered, "ratio<g: f32, w: numerus>");
}

#[test]
fn format_faber_type_record_render_round_trips_through_parser() {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let w = interner.intern("w");
    let g = interner.intern("g");
    let fractus_f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let numerus = types.primitive(Primitive::Numerus);
    let record = types.record([(w, numerus), (g, fractus_f32)]);

    let name = |_| "unused".to_owned();
    let rendered = format_faber_type(record, &types, &interner, &name);
    assert_eq!(rendered, "ratio<g: f32, w: numerus>");
    let source = format!("functio f({rendered} value) → vacuum {{}}");
    let parsed = parse(lex(&source));
    assert!(
        parsed.success(),
        "ratio diagnostic form must re-parse, got {rendered:?}: {:?}",
        parsed.errors
    );
}

/// N7E-1: the policy-float cell prints with the Latin policy word and its
/// float width marker.
#[test]
fn policy_float_cells_format_with_policy_word_and_width() {
    use radix_types::{FloatPolicy, Type};

    let interner = Interner::new();
    let mut types = TypeTable::new();
    let trapping = types.intern(Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F32));
    let saturating = types.intern(Type::PolicyFloat(
        FloatPolicy::Saturating,
        NumericWidth::F16,
    ));

    let name = |_| "unused".to_owned();
    assert_eq!(
        format_faber_type(trapping, &types, &interner, &name),
        "exactus<f32>"
    );
    assert_eq!(
        format_faber_type(saturating, &types, &interner, &name),
        "saturatus<f16>"
    );
}

/// Need-2.2: an interval's compile-time step prints after the bound.
#[test]
fn format_faber_type_renders_intervallum_step_suffix() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let stepped = types.intervallum_with_step(numerus, lit2);
    let plain = types.intervallum(numerus);

    let name = |_| "unused".to_owned();
    assert_eq!(
        format_faber_type(stepped, &types, &interner, &name),
        "intervallum<numerus, 2>"
    );
    assert_eq!(
        format_faber_type(plain, &types, &interner, &name),
        "intervallum<numerus>"
    );
}

/// Need-2.2: only a literal step other than 1 prints; step 1 and
/// symbolic steps keep the bare `intervallum<T>` spelling byte for byte.
#[test]
fn format_faber_type_renders_intervallum_step_one_stays_bare() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let lit1 = types.intern_index(IndexExpr::Literal(1));
    let canonical = types.intervallum_with_step(numerus, lit1);
    let plain = types.intervallum(numerus);
    // `intervallum_with_step` canonicalizes `Literal(1)`, so intern the raw
    // step directly to prove the formatter itself keeps step 1 bare.
    let raw_lit1 = types.intern(Type::Intervallum(numerus, lit1));
    let affine = types.intern_index(IndexExpr::Affine {
        base: lit1,
        offset: 0,
    });
    let symbolic = types.intern(Type::Intervallum(numerus, affine));

    let name = |_| "unused".to_owned();
    let canonical = format_faber_type(canonical, &types, &interner, &name);
    assert_eq!(
        canonical,
        format_faber_type(plain, &types, &interner, &name)
    );
    assert_eq!(canonical, "intervallum<numerus>");
    assert_eq!(
        format_faber_type(raw_lit1, &types, &interner, &name),
        "intervallum<numerus>"
    );
    assert_eq!(
        format_faber_type(symbolic, &types, &interner, &name),
        "intervallum<numerus>"
    );
}

#[test]
fn format_faber_type_spells_a_bounded_capacity_after_size() {
    // Need: a printed type name carries the `size` marker the source
    // must write (`ascii<size 1>` is the type of `'x'`); the `_` hole stays bare.
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let four = types.intern_index(IndexExpr::Literal(4));
    let one = types.intern_index(IndexExpr::Literal(1));
    let numerus = types.primitive(Primitive::Numerus);
    let name = |_| "unused".to_owned();

    let queue = types.bounded_queue(numerus, four);
    let stack = types.bounded_stack(numerus, four);
    let text = types.bounded_textus(four);
    let ascii = types.bounded_ascii(one);
    assert_eq!(
        format_faber_type(queue, &types, &interner, &name),
        "queue<numerus, size 4>"
    );
    assert_eq!(
        format_faber_type(stack, &types, &interner, &name),
        "stack<numerus, size 4>"
    );
    assert_eq!(
        format_faber_type(text, &types, &interner, &name),
        "textus<size 4>"
    );
    assert_eq!(
        format_faber_type(ascii, &types, &interner, &name),
        "ascii<size 1>"
    );
}
