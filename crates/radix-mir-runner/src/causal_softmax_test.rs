//! `TensorCausalMaskedSoftmax`: row `i` is a stable softmax over columns
//! `0..=i` of a rank-2 causal score matrix; entries above the diagonal are
//! exactly zero. No mask tensor input (GI3-1) — the causal structure is the
//! recipe's semantics.

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

fn causal_softmax(data: &[f64], shape: &[i64]) -> Result<(Vec<f64>, Vec<i64>), RunnerError> {
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
            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCausalMaskedSoftmax),
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

#[test]
fn causal_softmax_3x3_ones_masks_above_diagonal() {
    let (out, shape) = causal_softmax(&[1.0; 9], &[3, 3]).expect("3x3 causal softmax");
    assert_eq!(shape, vec![3, 3]);
    let expected = [
        1.0,
        0.0,
        0.0,
        0.5,
        0.5,
        0.0,
        1.0 / 3.0,
        1.0 / 3.0,
        1.0 / 3.0,
    ];
    assert_eq!(out.len(), expected.len());
    for (i, (actual, want)) in out.iter().zip(expected).enumerate() {
        assert!(
            (actual - want).abs() < 1e-12,
            "element {i}: {actual} vs {want}"
        );
    }
    assert_eq!(out[1], 0.0, "above-diagonal entry must be exactly zero");
    assert_eq!(out[2], 0.0, "above-diagonal entry must be exactly zero");
    assert_eq!(out[5], 0.0, "above-diagonal entry must be exactly zero");
}

#[test]
fn causal_softmax_2x3_non_uniform_rows() {
    let (out, shape) =
        causal_softmax(&[1.0, 2.0, 3.0, 0.0, 5.0, -1.0], &[2, 3]).expect("2x3 causal softmax");
    assert_eq!(shape, vec![2, 3]);
    let e5 = 5.0_f64.exp();
    let expected = [1.0, 0.0, 0.0, 1.0 / (1.0 + e5), e5 / (1.0 + e5), 0.0];
    assert_eq!(out.len(), expected.len());
    for (i, (actual, want)) in out.iter().zip(expected).enumerate() {
        assert!(
            (actual - want).abs() < 1e-12,
            "element {i}: {actual} vs {want}"
        );
    }
    assert_eq!(out[1], 0.0, "above-diagonal entry must be exactly zero");
    assert_eq!(out[2], 0.0, "above-diagonal entry must be exactly zero");
    assert_eq!(out[5], 0.0, "above-diagonal entry must be exactly zero");
}

#[test]
fn causal_softmax_rank1_rejected() {
    let result = causal_softmax(&[1.0, 2.0, 3.0], &[3]);
    assert!(result.is_err());
}
