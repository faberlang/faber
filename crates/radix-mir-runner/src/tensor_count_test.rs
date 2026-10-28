//! Runner pins for operator ruling 2026-10-15, Q-X1: `length` is the element
//! count on a tensor and a matrix, `TensorRank` is the number of extents and
//! `TensorNorm` the Euclidean norm. The three are distinct operations.

use super::lib_test::evaluate_collection_call;
use crate::Value;
use crate::value::{MatrixValue, TensorValue};
use radix_mir::{MirCollectionOp, MirIntrinsic, MirOperand, MirRuntimeCall, MirType, MirValueId};
use radix_types::TypeId;
use std::cell::RefCell;
use std::rc::Rc;

fn run(op: MirCollectionOp, receiver: Value) -> Value {
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(op),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    evaluate_collection_call(&call, vec![(MirValueId(0), receiver)]).expect("collection call")
}

fn tensor(data: Vec<f64>, shape: Vec<i64>) -> Value {
    let cells = data.into_iter().map(Value::Float).collect();
    Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(cells, shape))))
}

#[test]
fn tensor_length_is_the_element_count_and_rank_is_the_extent_count() {
    let grid = || tensor(vec![3.0, 4.0, 0.0, 0.0, 0.0, 0.0], vec![2, 3]);
    assert_eq!(run(MirCollectionOp::Length, grid()), Value::Int(6));
    assert_eq!(run(MirCollectionOp::TensorRank, grid()), Value::Int(2));

    let cube = || tensor(vec![0.0; 6], vec![1, 2, 3]);
    assert_eq!(run(MirCollectionOp::Length, cube()), Value::Int(6));
    assert_eq!(run(MirCollectionOp::TensorRank, cube()), Value::Int(3));
}

#[test]
fn rank_zero_tensor_holds_one_element() {
    let scalar = || tensor(vec![7.0], Vec::new());
    assert_eq!(run(MirCollectionOp::Length, scalar()), Value::Int(1));
    assert_eq!(run(MirCollectionOp::TensorRank, scalar()), Value::Int(0));
}

#[test]
fn empty_axis_tensor_has_no_elements_but_keeps_its_rank() {
    let hollow = || tensor(Vec::new(), vec![2, 0, 3]);
    assert_eq!(run(MirCollectionOp::Length, hollow()), Value::Int(0));
    assert_eq!(run(MirCollectionOp::TensorRank, hollow()), Value::Int(3));
}

#[test]
fn tensor_norm_is_the_euclidean_norm_of_the_logical_elements() {
    let grid = tensor(vec![3.0, 4.0, 0.0, 0.0, 0.0, 0.0], vec![2, 3]);
    assert_eq!(run(MirCollectionOp::TensorNorm, grid), Value::Float(5.0));
}

#[test]
fn matrix_length_is_rows_times_columns() {
    let matrix = Value::Matrix(Rc::new(RefCell::new(MatrixValue {
        rows: 2,
        columns: 3,
        data: (0..6).map(|n| Value::Float(f64::from(n))).collect(),
    })));
    assert_eq!(run(MirCollectionOp::Length, matrix), Value::Int(6));
}
