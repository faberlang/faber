use crate::host::BufferHost;
use crate::value::MatrixValue;
use crate::{Runner, RunnerError, Value};
use radix_lexer::{Interner, Span};
use radix_mir::{MirConversion, MirConversionFlavor, MirProgram, MirType, MirValidationContext};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

fn convert_register(
    types: &TypeTable,
    context: &MirValidationContext<'_>,
    source_ty: TypeId,
    target_ty: TypeId,
    source: Value,
) -> Result<Value, RunnerError> {
    let interner = Interner::new();
    let program = MirProgram::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, context, types, &interner, &mut host);
    runner.eval_runtime_convert(
        &MirConversion {
            store: None,
            flavor: MirConversionFlavor::Runtime,
            source_ty: MirType::semantic(source_ty),
            target_ty: MirType::semantic(target_ty),
            params: vec![],
            recovery: None,
            struct_defaults: vec![],
        },
        source,
        Span::default(),
    )
}

fn matrix_shape(types: &mut TypeTable) -> radix_types::IndexId {
    let rows = types.intern_index(IndexExpr::Literal(2));
    let columns = types.intern_index(IndexExpr::Literal(2));
    types.intern_index(IndexExpr::Tuple(vec![rows, columns]))
}

fn f32_element(types: &mut TypeTable) -> TypeId {
    types.sized_numeric(Primitive::Fractus, NumericWidth::F32)
}

fn assert_matrix_values(value: Value) {
    let Value::Matrix(matrix) = value else {
        panic!("expected a matrix, got {value:?}");
    };
    assert_eq!(
        *matrix.borrow(),
        MatrixValue {
            rows: 2,
            columns: 2,
            data: vec![
                Value::Float(1.0),
                Value::Float(2.0),
                Value::Float(3.0),
                Value::Float(4.0),
            ],
        }
    );
}

fn flat_values() -> Value {
    Value::Array(Rc::new(RefCell::new(vec![
        Value::Float(1.0),
        Value::Float(2.0),
        Value::Float(3.0),
        Value::Float(4.0),
    ])))
}

#[test]
fn flat_lista_converts_to_matrix_in_row_major_order() {
    let mut types = TypeTable::new();
    let element = f32_element(&mut types);
    let source_ty = types.intern(Type::Array(element));
    let shape = matrix_shape(&mut types);
    let target_ty = types.matrix_with_shape(element, shape);
    let context = MirValidationContext::new(&types);

    let converted = convert_register(&types, &context, source_ty, target_ty, flat_values())
        .expect("lista<f32> to matrix<f32, [2, 2]>");

    assert_matrix_values(converted);
}

#[test]
fn nested_lista_converts_to_matrix_in_row_major_order() {
    let mut types = TypeTable::new();
    let element = f32_element(&mut types);
    let row_ty = types.intern(Type::Array(element));
    let source_ty = types.intern(Type::Array(row_ty));
    let shape = matrix_shape(&mut types);
    let target_ty = types.matrix_with_shape(element, shape);
    let context = MirValidationContext::new(&types);
    let source = Value::Array(Rc::new(RefCell::new(vec![
        Value::Array(Rc::new(RefCell::new(vec![
            Value::Float(1.0),
            Value::Float(2.0),
        ]))),
        Value::Array(Rc::new(RefCell::new(vec![
            Value::Float(3.0),
            Value::Float(4.0),
        ]))),
    ])));

    let converted = convert_register(&types, &context, source_ty, target_ty, source)
        .expect("lista<lista<f32>> to matrix<f32, [2, 2]>");

    assert_matrix_values(converted);
}

#[test]
fn flat_lista_with_wrong_length_fails_matrix_conversion_without_recovery() {
    let mut types = TypeTable::new();
    let element = f32_element(&mut types);
    let source_ty = types.intern(Type::Array(element));
    let shape = matrix_shape(&mut types);
    let target_ty = types.matrix_with_shape(element, shape);
    let context = MirValidationContext::new(&types);
    let source = Value::Array(Rc::new(RefCell::new(vec![
        Value::Float(1.0),
        Value::Float(2.0),
        Value::Float(3.0),
    ])));

    let result = convert_register(&types, &context, source_ty, target_ty, source);

    assert!(
        result.is_err(),
        "three cells cannot fill a 2x2 matrix: {result:?}"
    );
}
