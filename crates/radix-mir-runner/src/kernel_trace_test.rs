use super::*;
use crate::dense::DenseStorage;
use crate::value::{TensorValue, Value};

#[test]
fn f32_elements_are_float_and_exactly_widened() {
    let tensor = TensorValue::try_contiguous_typed(DenseStorage::F32(vec![1.5, 0.1]), vec![2])
        .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("tensor:0", &tensor);

    assert_eq!(traced.slot, "tensor:0");
    assert_eq!(traced.elements.len(), 2);
    assert_eq!(traced.elements[0], kernel_trace::TraceElement::Float(1.5));
    assert_eq!(
        traced.elements[1],
        kernel_trace::TraceElement::Float(f64::from(0.1_f32))
    );
}

#[test]
fn i64_elements_keep_their_twos_complement_bits() {
    let tensor = TensorValue::try_contiguous_typed(DenseStorage::I64(vec![-1, 7]), vec![2])
        .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("tensor:0", &tensor);

    assert_eq!(
        traced.elements,
        vec![
            kernel_trace::TraceElement::Exact(u64::MAX),
            kernel_trace::TraceElement::Exact(7)
        ]
    );
}

#[test]
fn bool_elements_are_exact_zero_or_one() {
    let tensor = TensorValue::try_contiguous_typed(DenseStorage::Bool(vec![true, false]), vec![2])
        .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("tensor:0", &tensor);

    assert_eq!(
        traced.elements,
        vec![
            kernel_trace::TraceElement::Exact(1),
            kernel_trace::TraceElement::Exact(0)
        ]
    );
}

#[test]
fn a_values_backing_records_floats_ints_and_text() {
    let tensor = TensorValue::try_contiguous(
        vec![
            Value::Float(2.0),
            Value::Int(-3),
            Value::Str("x".to_owned()),
        ],
        vec![3],
    )
    .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("tensor:0", &tensor);

    assert_eq!(traced.elements[0], kernel_trace::TraceElement::Float(2.0));
    assert_eq!(
        traced.elements[1],
        kernel_trace::TraceElement::Exact(u64::MAX - 2)
    );
    match &traced.elements[2] {
        kernel_trace::TraceElement::Text(text) => assert!(text.contains('x')),
        other => panic!("expected text trace element, got {other:?}"),
    }
}
