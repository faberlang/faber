//! Softmax runs over the last axis of any rank >= 1 tensor; every leading
//! axis is a batch (operator decision 2026-10-08, gpu-reset SOFTMAXN).

use super::*;

use crate::host::BufferHost;
use crate::value::TensorValue;
use radix_lexer::Interner;
use radix_mir::{MirCollectionOp, MirIntrinsic, MirRuntimeCall, MirType, MirValidationContext};
use radix_types::{TypeId, TypeTable};

fn function() -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn softmax(data: &[f64], shape: &[i64]) -> Result<(Vec<f64>, Vec<i64>), RunnerError> {
    let values = data.iter().copied().map(Value::Float).collect();
    let input = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        values,
        shape.to_vec(),
    ))));
    let mut host = BufferHost::default();
    let types = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input);
    let result = runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSoftmax),
            args: vec![MirOperand::Value(MirValueId(0))],
            return_ty: MirType::semantic(TypeId(0)),
        },
        Span::default(),
    )?;
    let Value::Tensor(cell) = result else {
        panic!("expected a tensor, got {result:?}");
    };
    let tensor = cell.borrow();
    let out = tensor
        .logical_values()
        .expect("valid tensor layout")
        .into_iter()
        .map(|v| match v {
            Value::Float(f) => f,
            other => panic!("expected float element, got {other:?}"),
        })
        .collect();
    Ok((out, tensor.shape.clone()))
}

/// Deterministic, varied, non-uniform inputs.
fn sample(len: usize) -> Vec<f64> {
    (0..len)
        .map(|i| ((i * 37 % 23) as f64) * 0.31 - 3.0)
        .collect()
}

/// Expected values: the rank-1 path applied to every last-axis row.
fn per_row_rank1(data: &[f64], last: usize) -> Vec<f64> {
    data.chunks(last)
        .flat_map(|row| softmax(row, &[last as i64]).expect("rank-1 softmax").0)
        .collect()
}

fn assert_bitwise(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a.to_bits(), e.to_bits(), "element {i}: {a} vs {e}");
    }
}

#[test]
fn softmax_rank3_matches_per_row_rank1() {
    let shape = [2, 3, 5];
    let data = sample(30);
    let (out, out_shape) = softmax(&data, &shape).expect("rank-3 softmax");
    assert_eq!(out_shape, shape.to_vec());
    assert_bitwise(&out, &per_row_rank1(&data, 5));
}

#[test]
fn softmax_rank4_matches_per_row_rank1() {
    let shape = [2, 2, 3, 4];
    let data = sample(48);
    let (out, out_shape) = softmax(&data, &shape).expect("rank-4 softmax");
    assert_eq!(out_shape, shape.to_vec());
    assert_bitwise(&out, &per_row_rank1(&data, 4));
}

#[test]
fn softmax_rank5_matches_per_row_rank1() {
    let shape = [2, 1, 2, 3, 3];
    let data = sample(36);
    let (out, out_shape) = softmax(&data, &shape).expect("rank-5 softmax");
    assert_eq!(out_shape, shape.to_vec());
    assert_bitwise(&out, &per_row_rank1(&data, 3));
}

#[test]
fn softmax_masked_attention_shape_rows_sum_to_one() {
    let shape = [5, 3, 1, 76];
    let mut data = sample(5 * 3 * 76);
    // Additive mask: the tail of every row is -infinity-like.
    for row in data.chunks_mut(76) {
        for v in &mut row[60..] {
            *v += -1.0e9;
        }
    }
    let (out, out_shape) = softmax(&data, &shape).expect("masked softmax");
    assert_eq!(out_shape, shape.to_vec());
    for (r, row) in out.chunks(76).enumerate() {
        let sum: f64 = row.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "row {r} sums to {sum}");
        assert!(row[60..].iter().all(|p| *p < 1e-12), "row {r} mask leaked");
    }
}

#[test]
fn softmax_rank1_and_rank2_unchanged() {
    let (one, _) = softmax(&[1.0, 2.0, 3.0], &[3]).expect("rank-1");
    let (two, shape) = softmax(&[1.0, 2.0, 3.0, 1.0, 2.0, 3.0], &[2, 3]).expect("rank-2");
    assert_eq!(shape, vec![2, 3]);
    assert_bitwise(&two[..3], &one);
    assert_bitwise(&two[3..], &one);
}

#[test]
fn softmax_zero_width_last_axis_still_rejected() {
    let err = softmax(&[], &[2, 3, 0]).expect_err("zero-width last axis");
    assert!(err.issue.contains("positive"), "got {err:?}");
}

#[test]
fn softmax_rank0_still_rejected() {
    let err = softmax(&[1.0], &[]).expect_err("rank 0");
    assert!(err.issue.contains("non_empty"), "got {err:?}");
}
