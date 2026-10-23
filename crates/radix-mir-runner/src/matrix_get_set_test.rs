//! Runner pins for `matrix[[i, j]]` coordinate reads/writes (ruling
//! 2026-10-15): the matrix receiver shares the tensor coordinate contract —
//! row-major flat offset `row * columns + column`, the same index validation,
//! the same optional-vs-bare miss posture, and the same alias-through-`Rc`
//! store semantics.

use super::*;
use crate::value::MatrixValue;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirValidationContext, MirValueId,
};
use radix_types::{NumericWidth, Primitive, TypeId, TypeTable};

const ACCIPE_INVALID_INDEX_ISSUE: &str = "runner_tensor_accipe_invalid_index";
const PONDE_INVALID_INDEX_ISSUE: &str = "runner_tensor_ponde_invalid_index";

/// A `2x2` row-major matrix over `f32` cells.
fn matrix(data: [f64; 4]) -> Value {
    Value::Matrix(Rc::new(RefCell::new(MatrixValue {
        rows: 2,
        columns: 2,
        data: data.into_iter().map(Value::Float).collect(),
    })))
}

fn index(coordinates: &[i64]) -> Value {
    Value::Array(Rc::new(RefCell::new(
        coordinates.iter().copied().map(Value::Int).collect(),
    )))
}

fn f32_element(types: &mut TypeTable) -> TypeId {
    types.sized_numeric(Primitive::Fractus, NumericWidth::F32)
}

/// Evaluate one collection runtime call against the given operands through a
/// synthetic single-function runner. `return_ty` is built by `build_return`
/// and read back through the call's `MirType`.
fn eval_call(
    intrinsic: MirCollectionOp,
    operands: Vec<Value>,
    build_return: impl FnOnce(&mut TypeTable) -> TypeId,
) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let return_ty = MirType::semantic(build_return(&mut type_table));
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    let args = operands
        .into_iter()
        .enumerate()
        .map(|(id, value)| {
            let id = MirValueId(u32::try_from(id).expect("operand index fits"));
            frame.values.insert(id, value);
            MirOperand::Value(id)
        })
        .collect();
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(intrinsic),
        args,
        return_ty,
    };
    runner.eval_runtime_call(&call, Span::default())
}

fn get(receiver: Value, coordinates: &[i64], optional: bool) -> Result<Value, RunnerError> {
    let index = index(coordinates);
    eval_call(
        MirCollectionOp::TensorGet,
        vec![receiver, index],
        move |types| {
            let element = f32_element(types);
            if optional {
                types.option(element)
            } else {
                element
            }
        },
    )
}

fn set(receiver: Value, coordinates: &[i64], value: Value) -> Result<Value, RunnerError> {
    let index = index(coordinates);
    eval_call(
        MirCollectionOp::TensorSet,
        vec![receiver, index, value],
        |types| types.primitive(Primitive::Vacuum),
    )
}

fn shape(receiver: Value) -> Result<Value, RunnerError> {
    eval_call(MirCollectionOp::TensorShape, vec![receiver], |types| {
        let numerus = types.primitive(Primitive::Numerus);
        types.ensure_array(numerus)
    })
}

#[test]
fn matrix_shape_answers_rows_then_columns() {
    let Value::Array(cells) = shape(matrix([1.0, 2.0, 3.0, 4.0])).unwrap() else {
        panic!("matrix shape is not an array");
    };
    assert_eq!(*cells.borrow(), vec![Value::Int(2), Value::Int(2)]);
}

#[test]
fn matrix_read_is_row_major_cell() {
    let value = matrix([1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        get(value.clone(), &[0, 0], true).unwrap(),
        Value::Float(1.0)
    );
    assert_eq!(
        get(value.clone(), &[0, 1], true).unwrap(),
        Value::Float(2.0)
    );
    assert_eq!(
        get(value.clone(), &[1, 0], true).unwrap(),
        Value::Float(3.0)
    );
    assert_eq!(get(value, &[1, 1], true).unwrap(), Value::Float(4.0));
}

#[test]
fn matrix_option_read_answers_miss_with_nil() {
    let value = matrix([1.0, 2.0, 3.0, 4.0]);
    // Out of range on either axis, and every wrong rank.
    for coordinates in [&[2, 0][..], &[0, 2], &[5, 5], &[0], &[0, 0, 0], &[]] {
        assert_eq!(
            get(value.clone(), coordinates, true).unwrap(),
            Value::Nil,
            "{coordinates:?}"
        );
    }
}

#[test]
fn matrix_bare_read_errors_on_miss() {
    let value = matrix([1.0, 2.0, 3.0, 4.0]);
    for coordinates in [&[2, 0][..], &[0], &[0, 0, 0]] {
        let error = get(value.clone(), coordinates, false).unwrap_err();
        assert_eq!(error.code, "runner_internal");
        assert_eq!(error.issue, ACCIPE_INVALID_INDEX_ISSUE, "{coordinates:?}");
    }
}

#[test]
fn matrix_store_writes_and_read_observes_alias() {
    let value = matrix([1.0, 2.0, 3.0, 4.0]);
    // The second read goes through the same `Rc` handle, so it observes the
    // write the first call performed.
    set(value.clone(), &[1, 0], Value::Float(9.5)).unwrap();
    assert_eq!(get(value, &[1, 0], true).unwrap(), Value::Float(9.5));
}

#[test]
fn matrix_store_out_of_range_errors() {
    let value = matrix([1.0, 2.0, 3.0, 4.0]);
    for coordinates in [&[2, 0][..], &[0, 2], &[0], &[0, 0, 0], &[]] {
        let error = set(value.clone(), coordinates, Value::Float(9.5)).unwrap_err();
        assert_eq!(error.code, "runner_internal");
        assert_eq!(error.issue, PONDE_INVALID_INDEX_ISSUE, "{coordinates:?}");
    }
}
