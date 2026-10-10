//! M3 runner proofs for the TensorMax/TensorMin collection operations.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use super::lib_test::evaluate_collection_call;
use crate::host::BufferHost;
use crate::value::TensorValue;
use radix_lexer::Interner;
use radix_mir::{MirCollectionOp, MirConstant, MirFunction, MirIntrinsic, MirRuntimeCall, MirType};
use radix_types::TypeTable;

fn reduction_function() -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(radix_types::TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: radix_lexer::Span::default(),
    }
}

fn eval_extremum(
    op: MirCollectionOp,
    data: Vec<Value>,
    identity: Option<f64>,
) -> Result<Value, RunnerError> {
    let mut host = BufferHost::default();
    let types = TypeTable::default();
    let interner = Interner::new();
    let context = radix_mir::MirValidationContext::new(&types);
    let mut program = radix_mir::MirProgram::new();
    program.functions.push(reduction_function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner
        .frames
        .last_mut()
        .expect("reduction frame")
        .values
        .insert(
            MirValueId(0),
            Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
                data,
                vec![3],
            )))),
        );

    let mut args = vec![MirOperand::Value(MirValueId(0))];
    if let Some(identity) = identity {
        args.push(MirOperand::Constant(MirConstant::Float(identity)));
    }
    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(op),
            args,
            return_ty: MirType::semantic(radix_types::TypeId(0)),
        },
        radix_lexer::Span::default(),
    )
}

#[test]
fn tensor_extrema_collection_ops_have_distinct_identities() {
    assert_ne!(MirCollectionOp::TensorMax, MirCollectionOp::TensorMin);
    assert_eq!(format!("{:?}", MirCollectionOp::TensorMax), "TensorMax");
    assert_eq!(format!("{:?}", MirCollectionOp::TensorMin), "TensorMin");
}

#[test]
fn tensor_max_and_min_fold_f64_values() {
    let data = vec![Value::Float(-3.0), Value::Float(4.0), Value::Float(1.0)];
    assert_eq!(
        eval_extremum(MirCollectionOp::TensorMax, data.clone(), None).expect("max runtime call"),
        Value::Float(4.0)
    );
    assert_eq!(
        eval_extremum(MirCollectionOp::TensorMin, data, None).expect("min runtime call"),
        Value::Float(-3.0)
    );
}

#[test]
fn tensor_extrema_coalesce_empty_runtime_tensors_with_identity() {
    assert_eq!(
        eval_extremum(MirCollectionOp::TensorMax, Vec::new(), Some(-10.0))
            .expect("coalesced empty max"),
        Value::Float(-10.0)
    );
    assert_eq!(
        eval_extremum(MirCollectionOp::TensorMin, Vec::new(), Some(10.0))
            .expect("coalesced empty min"),
        Value::Float(10.0)
    );
}

#[test]
fn tensor_extrema_return_nihil_for_empty_runtime_tensors_without_identity() {
    assert_eq!(
        eval_extremum(MirCollectionOp::TensorMax, Vec::new(), None)
            .expect("uncoalesced empty max returns nihil"),
        Value::Nil
    );
    assert_eq!(
        eval_extremum(MirCollectionOp::TensorMin, Vec::new(), None)
            .expect("uncoalesced empty min returns nihil"),
        Value::Nil
    );
}

fn eval_scalar_extremum(
    op: MirCollectionOp,
    left: Value,
    right: Value,
) -> Result<Value, RunnerError> {
    evaluate_collection_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(op),
            args: vec![
                MirOperand::Value(MirValueId(0)),
                MirOperand::Value(MirValueId(1)),
            ],
            return_ty: MirType::semantic(radix_types::TypeId(0)),
        },
        vec![(MirValueId(0), left), (MirValueId(1), right)],
    )
}

#[test]
fn scalar_minimum_and_maximum_stay_exact_beyond_the_f64_integers() {
    // 2^53 + 1 and 2^53 + 3 are not f64-representable; the pair must come back
    // as one of the operands, not as a rounded neighbour.
    let low = (1_i64 << 53) + 1;
    let high = (1_i64 << 53) + 3;
    assert_eq!(
        eval_scalar_extremum(MirCollectionOp::Minimum, Value::Int(low), Value::Int(high))
            .expect("scalar minimum"),
        Value::Int(low)
    );
    assert_eq!(
        eval_scalar_extremum(MirCollectionOp::Maximum, Value::Int(low), Value::Int(high))
            .expect("scalar maximum"),
        Value::Int(high)
    );
}

#[test]
fn scalar_minimum_and_maximum_accept_unsigned_pairs() {
    let low = u64::MAX - 2;
    let high = u64::MAX - 1;
    assert_eq!(
        eval_scalar_extremum(
            MirCollectionOp::Minimum,
            Value::UInt(low),
            Value::UInt(high)
        )
        .expect("unsigned minimum"),
        Value::UInt(low)
    );
    assert_eq!(
        eval_scalar_extremum(
            MirCollectionOp::Maximum,
            Value::UInt(low),
            Value::UInt(high)
        )
        .expect("unsigned maximum"),
        Value::UInt(high)
    );
}
