//! A non-dynamic (`json`) source converting into a genus applies the declared
//! field defaults of a nested genus, not the zero value of its field type.

use crate::host::BufferHost;
use crate::value::MatrixValue;
use crate::{MapKey, Runner, RunnerError, Value};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::DefId;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirConstant, MirConversion, MirConversionFlavor, MirNamedOperand, MirOperand, MirProgram,
    MirStructFieldDefaults, MirType, MirValidationContext,
};
use radix_types::{IndexExpr, IndexId, NumericWidth, Primitive, Type, TypeId, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn nested_genus_field_absent_from_json_takes_its_declared_default() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let inner_field = interner.intern("inner");
    let count_field = interner.intern("count");
    let inner_def = DefId(41);
    let outer_def = DefId(42);
    let json = types.primitive(Primitive::Json);
    let numerus = types.primitive(Primitive::Numerus);
    let inner_ty = types.intern(Type::Struct(inner_def));
    let outer_ty = types.intern(Type::Struct(outer_def));
    let mut context = MirValidationContext::new(&types);
    context.struct_fields.insert(
        inner_def,
        [(count_field, MirType::semantic(numerus))]
            .into_iter()
            .collect(),
    );
    context.struct_fields.insert(
        outer_def,
        [(inner_field, MirType::semantic(inner_ty))]
            .into_iter()
            .collect(),
    );
    let program = MirProgram::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);

    let empty_inner = Value::Map(Rc::new(RefCell::new(FxHashMap::default())));
    let mut object = FxHashMap::default();
    object.insert(MapKey::Str("inner".to_owned()), empty_inner);
    let source = Value::Map(Rc::new(RefCell::new(object)));

    let converted = runner
        .eval_runtime_convert(
            &MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty: MirType::semantic(json),
                target_ty: MirType::semantic(outer_ty),
                params: vec![],
                recovery: None,
                struct_defaults: vec![MirStructFieldDefaults {
                    def_id: inner_def,
                    fields: vec![MirNamedOperand {
                        name: count_field,
                        value: MirOperand::Constant(MirConstant::Int(5)),
                    }],
                }],
            },
            source,
            Span::default(),
        )
        .expect("json to genus conversion");

    let Value::Struct(def, outer) = converted else {
        panic!("expected a genus value, got {converted:?}");
    };
    assert_eq!(def, outer_def);
    let Some(Value::Struct(_, inner)) = outer.borrow().get(&inner_field).cloned() else {
        panic!("expected the nested genus value");
    };
    assert!(
        matches!(inner.borrow().get(&count_field), Some(Value::Int(5))),
        "nested field: {:?}",
        inner.borrow().get(&count_field)
    );
}

/// Run one `↦` conversion through the target-directed runner seam.
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

fn f32_element(types: &mut TypeTable) -> TypeId {
    types.sized_numeric(Primitive::Fractus, NumericWidth::F32)
}

fn literal_shape(types: &mut TypeTable, dims: &[u64]) -> IndexId {
    let dims = dims
        .iter()
        .map(|dim| types.intern_index(IndexExpr::Literal(*dim)))
        .collect();
    types.intern_index(IndexExpr::Tuple(dims))
}

fn matrix_2x2(values: [f64; 4]) -> Value {
    Value::Matrix(Rc::new(RefCell::new(MatrixValue {
        rows: 2,
        columns: 2,
        data: values.into_iter().map(Value::Float).collect(),
    })))
}

fn tensor_shape_and_values(value: Value) -> (Vec<i64>, Vec<Value>) {
    let Value::Tensor(tensor) = value else {
        panic!("expected a tensor, got {value:?}");
    };
    let tensor = tensor.borrow();
    let values = tensor
        .logical_values()
        .unwrap_or_else(|| panic!("tensor must expose logical values"));
    (tensor.shape.clone(), values)
}

#[test]
fn matrix_to_tensor_keeps_row_major_shape_and_values() {
    let mut types = TypeTable::new();
    let element = f32_element(&mut types);
    let shape = literal_shape(&mut types, &[2, 2]);
    let matrix_ty = types.matrix_with_shape(element, shape);
    let tensor_ty = types.tensor_with_shape(element, shape);
    let context = MirValidationContext::new(&types);

    let converted = convert_register(
        &types,
        &context,
        matrix_ty,
        tensor_ty,
        matrix_2x2([1.0, 2.0, 3.0, 4.0]),
    )
    .expect("matrix<f32, [2, 2]> to tensor<f32, [2, 2]>");

    let (shape, values) = tensor_shape_and_values(converted);
    assert_eq!(shape, vec![2, 2]);
    assert_eq!(
        values,
        vec![
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
        ]
    );
}

#[test]
fn vector_to_tensor_keeps_lanes_in_order() {
    let mut types = TypeTable::new();
    let element = f32_element(&mut types);
    let width = types.intern_index(IndexExpr::Literal(3));
    let vector_ty = types.vector_with_width(element, width);
    let shape = literal_shape(&mut types, &[3]);
    let tensor_ty = types.tensor_with_shape(element, shape);
    let context = MirValidationContext::new(&types);

    let converted = convert_register(
        &types,
        &context,
        vector_ty,
        tensor_ty,
        Value::Vector(Rc::new(RefCell::new(vec![
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
        ]))),
    )
    .expect("vector<f32, size 3> to tensor<f32, [3]>");

    let (shape, values) = tensor_shape_and_values(converted);
    assert_eq!(shape, vec![3]);
    assert_eq!(
        values,
        vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)]
    );
}

#[test]
fn matrix_to_lista_flattens_row_major() {
    let mut types = TypeTable::new();
    let element = f32_element(&mut types);
    let shape = literal_shape(&mut types, &[2, 2]);
    let matrix_ty = types.matrix_with_shape(element, shape);
    let lista_ty = types.intern(Type::Array(element));
    let context = MirValidationContext::new(&types);

    let converted = convert_register(
        &types,
        &context,
        matrix_ty,
        lista_ty,
        matrix_2x2([1.0, 2.0, 3.0, 4.0]),
    )
    .expect("matrix<f32, [2, 2]> to lista<f32>");

    let Value::Array(items) = converted else {
        panic!("expected a lista, got {converted:?}");
    };
    assert_eq!(
        items.borrow().as_slice(),
        &[
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
        ]
    );
}

#[test]
fn matrix_to_tensor_of_mismatched_literal_shape_fails_without_recovery() {
    let mut types = TypeTable::new();
    let element = f32_element(&mut types);
    let matrix_shape = literal_shape(&mut types, &[2, 2]);
    let target_shape = literal_shape(&mut types, &[5]);
    let matrix_ty = types.matrix_with_shape(element, matrix_shape);
    let tensor_ty = types.tensor_with_shape(element, target_shape);
    let context = MirValidationContext::new(&types);

    let result = convert_register(
        &types,
        &context,
        matrix_ty,
        tensor_ty,
        matrix_2x2([1.0, 2.0, 3.0, 4.0]),
    );

    assert!(
        result.is_err(),
        "a 2x2 matrix must not become a five-cell rank-1 tensor: {result:?}"
    );
}

#[test]
fn lista_conversio_to_queue_and_stack_preserves_order() {
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Numerus);
    let lista_ty = types.intern(Type::Array(element));
    let queue_ty = types.intern(Type::Queue(element));
    let stack_ty = types.intern(Type::Stack(element));
    let copia_ty = types.intern(Type::Set(element));
    let context = MirValidationContext::new(&types);

    let items = || {
        Value::Array(Rc::new(RefCell::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ])))
    };

    // queue FIFO and stack top-last both keep the lista carrier order; the
    // runtime carrier is the shared array cell either way.
    for target in [queue_ty, stack_ty] {
        let converted = convert_register(&types, &context, lista_ty, target, items())
            .expect("lista conversio into queue/stack");
        let Value::Array(out) = converted else {
            panic!("expected an array carrier, got {converted:?}");
        };
        assert_eq!(
            out.borrow().as_slice(),
            &[Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    let from_copia = convert_register(
        &types,
        &context,
        copia_ty,
        queue_ty,
        Value::Set(Rc::new(RefCell::new(FxHashSet::default()))),
    );
    assert!(
        from_copia.is_err(),
        "a copia source must keep failing closed: {from_copia:?}"
    );
}
