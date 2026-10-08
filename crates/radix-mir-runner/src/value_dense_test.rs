//! Tests for `TensorValue` over shared dense storage (unit U1).

use crate::dense::{DenseCell, DenseKind, DenseStorage};
use crate::value::{TensorValue, Value};
use std::sync::Arc;

fn ints(values: &[i64]) -> Vec<Value> {
    values.iter().map(|v| Value::Int(*v)).collect()
}

#[test]
fn views_share_one_storage_and_writes_are_visible_through_either() {
    let base = TensorValue::contiguous(ints(&[1, 2, 3, 4]), vec![2, 2]);
    let view = base.view(vec![1, 2], vec![2, 1], 2).expect("second row");
    assert!(Arc::ptr_eq(&base.data, &view.data));

    view.set_value_at(0, Value::Int(30))
        .expect("write via view");
    assert_eq!(base.value_at(2), Some(Value::Int(30)));
    base.set_value_at(3, Value::Int(40))
        .expect("write via base");
    assert_eq!(view.logical_values(), Some(ints(&[30, 40])));
}

#[test]
fn deep_copy_produces_independent_storage() {
    let source = Value::Tensor(std::rc::Rc::new(std::cell::RefCell::new(
        TensorValue::contiguous(ints(&[1, 2]), vec![2]),
    )));
    let Value::Tensor(copy) = source.deep_copy() else {
        panic!("tensor deep copy stays a tensor");
    };
    let Value::Tensor(source) = &source else {
        unreachable!()
    };
    assert!(!Arc::ptr_eq(&source.borrow().data, &copy.borrow().data));
    copy.borrow().set_value_at(0, Value::Int(9)).expect("write");
    assert_eq!(source.borrow().value_at(0), Some(Value::Int(1)));
    assert_eq!(copy.borrow().value_at(0), Some(Value::Int(9)));
}

#[test]
fn typed_storage_deep_copies_through_the_typed_clone() {
    let source = TensorValue::try_contiguous_typed(DenseStorage::F32(vec![1.5, 2.5]), vec![2])
        .expect("typed tensor");
    let Value::Tensor(copy) =
        Value::Tensor(std::rc::Rc::new(std::cell::RefCell::new(source))).deep_copy()
    else {
        panic!("tensor deep copy stays a tensor");
    };
    let copy = copy.borrow();
    assert_eq!(copy.data.kind(), Some(DenseKind::F32));
    assert_eq!(
        copy.logical_values(),
        Some(vec![Value::Float(1.5), Value::Float(2.5)])
    );
}

#[test]
fn tensors_born_from_values_use_values_storage() {
    let tensor = TensorValue::contiguous(ints(&[1, 2]), vec![2]);
    assert_eq!(tensor.data.kind(), None);
    assert_eq!(tensor.data.to_values(), ints(&[1, 2]));
}

#[test]
fn equality_is_by_content_not_identity() {
    let a = TensorValue::contiguous(ints(&[1, 2]), vec![2]);
    let b = TensorValue::contiguous(ints(&[1, 2]), vec![2]);
    let c = TensorValue::contiguous(ints(&[1, 3]), vec![2]);
    assert!(!Arc::ptr_eq(&a.data, &b.data));
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn the_shared_cell_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Arc<DenseCell>>();
}
