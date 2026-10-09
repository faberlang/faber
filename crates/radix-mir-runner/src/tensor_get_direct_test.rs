//! Runner pins for `tensor[[i, j, ..]]` reads: the dense row-major direct read
//! returns what the generic (strided view) read returns, and every failure
//! keeps its message and code.

use super::*;
use crate::value::TensorValue;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirValidationContext, MirValueId,
};
use radix_types::{Primitive, TypeTable};

const INVALID_INDEX: &str = "tensor accipe invalid index";

fn tensor_get(receiver: Value, index: Value, checked: bool) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let numerus = type_table.primitive(Primitive::Numerus);
    let return_ty = if checked {
        numerus
    } else {
        type_table.option(numerus)
    };
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
        return_ty: MirType::semantic(return_ty),
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
    frame.values.insert(MirValueId(0), receiver);
    frame.values.insert(MirValueId(1), index);
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGet),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(return_ty),
    };
    runner.eval_runtime_call(&call, Span::default())
}

fn ints(values: impl IntoIterator<Item = i64>) -> Vec<Value> {
    values.into_iter().map(Value::Int).collect()
}

fn index(coordinates: &[i64]) -> Value {
    Value::Array(Rc::new(RefCell::new(ints(coordinates.iter().copied()))))
}

fn dense(count: i64, shape: &[i64]) -> TensorValue {
    TensorValue::contiguous(ints(0..count), shape.to_vec())
}

fn tensor(value: TensorValue) -> Value {
    Value::Tensor(Rc::new(RefCell::new(value)))
}

fn get(value: &Value, coordinates: &[i64]) -> Result<Value, RunnerError> {
    tensor_get(value.clone(), index(coordinates), true)
}

#[test]
fn dense_rank_one_two_three_reads_match_row_major_order() {
    let rank1 = tensor(dense(5, &[5]));
    for i in 0..5 {
        assert_eq!(get(&rank1, &[i]).unwrap(), Value::Int(i));
    }
    let rank2 = tensor(dense(6, &[2, 3]));
    for i in 0..2 {
        for j in 0..3 {
            assert_eq!(get(&rank2, &[i, j]).unwrap(), Value::Int(i * 3 + j));
        }
    }
    let rank3 = tensor(dense(24, &[2, 3, 4]));
    for i in 0..2 {
        for j in 0..3 {
            for k in 0..4 {
                assert_eq!(
                    get(&rank3, &[i, j, k]).unwrap(),
                    Value::Int(i * 12 + j * 4 + k)
                );
            }
        }
    }
}

#[test]
fn dense_reads_agree_with_the_generic_strided_view_read() {
    // A transposed view of a [2, 3] tensor is not dense row-major, so it takes
    // the generic path; its reads must agree with the dense tensor's reads at
    // the swapped coordinates.
    let base = dense(6, &[2, 3]);
    let transposed = base
        .view(vec![3, 2], vec![1, 3], 0)
        .expect("transposed view");
    assert!(!transposed.is_dense_row_major());
    let dense_value = tensor(base);
    let view_value = tensor(transposed);
    for i in 0..2 {
        for j in 0..3 {
            assert_eq!(
                get(&dense_value, &[i, j]).unwrap(),
                get(&view_value, &[j, i]).unwrap()
            );
        }
    }
}

#[test]
fn typed_dense_storage_reads_the_same_cells() {
    let storage = crate::dense::DenseStorage::F32(vec![0.5, 1.5, 2.5, 3.5]);
    let typed = TensorValue::try_contiguous_typed(storage, vec![2, 2]).expect("typed tensor");
    let value = tensor(typed);
    let expected = [0.5, 1.5, 2.5, 3.5];
    for i in 0..2_i64 {
        for j in 0..2_i64 {
            let read = get(&value, &[i, j]).unwrap();
            assert_eq!(read, Value::Float(expected[(i * 2 + j) as usize]));
        }
    }
}

#[test]
fn out_of_range_and_wrong_rank_keep_the_error_text_and_code() {
    let value = tensor(dense(6, &[2, 3]));
    for coordinates in [&[2, 0][..], &[0, 3], &[5, 5], &[0], &[0, 0, 0], &[]] {
        let error = get(&value, coordinates).unwrap_err();
        assert_eq!(error.message, INVALID_INDEX, "{coordinates:?}");
        assert_eq!(error.code, "runner_internal");
        assert_eq!(error.issue, "runner_tensor_accipe_invalid_index");
        // The optional form answers a miss with none.
        assert_eq!(
            tensor_get(value.clone(), index(coordinates), false).unwrap(),
            Value::Nil,
            "{coordinates:?}"
        );
    }
}

#[test]
fn negative_and_non_numerus_index_elements_keep_their_errors() {
    let value = tensor(dense(6, &[2, 3]));
    let error = get(&value, &[-1, 0]).unwrap_err();
    assert_eq!(error.message, INVALID_INDEX);
    assert_eq!(error.code, "runner_internal");
    // A bad element is reported even when the rank is wrong too.
    let error = get(&value, &[-1]).unwrap_err();
    assert_eq!(error.message, INVALID_INDEX);

    let bad = Value::Array(Rc::new(RefCell::new(vec![
        Value::Int(0),
        Value::Str("x".into()),
    ])));
    let error = tensor_get(value.clone(), bad, true).unwrap_err();
    assert_eq!(
        error.message,
        "tensor shape element is not numerus: Str(\"x\")"
    );
    assert_eq!(error.code, "runner_internal");
}

#[test]
fn wrong_typed_arguments_keep_their_errors() {
    let value = tensor(dense(6, &[2, 3]));
    let error = tensor_get(value.clone(), Value::Int(1), true).unwrap_err();
    assert_eq!(error.message, "tensor lista argument type mismatch: Int(1)");
    let error = tensor_get(Value::Int(1), index(&[0]), true).unwrap_err();
    assert_eq!(error.message, "tensor receiver type mismatch: Int(1)");
}
