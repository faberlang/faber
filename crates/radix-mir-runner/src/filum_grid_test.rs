//! CPU-device meaning for grouped grid `FilumReduce` nodes.

use super::*;
use crate::host::BufferHost;
use crate::value::TensorValue;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlock, MirBlockId, MirFilumCoordinate, MirFilumForm, MirFilumGroupKey, MirFilumGroupOp,
    MirFilumReduceOp, MirFunction, MirFunctionId, MirIntrinsic, MirLocalId, MirOperand, MirParam,
    MirParamMode, MirPlace, MirPlaceBase, MirProgram, MirProjection, MirReduceOp, MirRuntimeCall,
    MirTerminator, MirTerminatorKind, MirType, MirValidationContext, MirValueId,
};
use radix_runtime_contract::tensor::ERR_ACCIPE_INVALID_INDEX;
use radix_types::{IndexExpr, NumericWidth, Primitive, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

fn tensor_type(types: &mut TypeTable, element: radix_types::TypeId, extent: u64) -> MirType {
    let axis = types.intern_index(IndexExpr::Literal(extent));
    let shape = types.intern_index(IndexExpr::Tuple(vec![axis]));
    MirType::semantic(types.tensor_with_shape(element, shape))
}

fn parameter(local: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(local),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: Span::default(),
    }
}

fn function(
    id: u32,
    params: Vec<MirParam>,
    return_ty: MirType,
    returned: MirOperand,
) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params,
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(returned)),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn run_grouped(
    op: MirReduceOp,
    source_values: Vec<Value>,
    key_values: Vec<Value>,
) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let i32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let bool_ty = types.primitive(Primitive::Bivalens);
    let source_element = if op == MirReduceOp::Count {
        bool_ty
    } else {
        f32_ty
    };
    let result_element = if op == MirReduceOp::Count {
        i32_ty
    } else {
        f32_ty
    };
    let _source_tensor_ty = tensor_type(&mut types, source_element, 5);
    let key_tensor_ty = tensor_type(&mut types, i32_ty, 5);
    let result_tensor_ty = tensor_type(&mut types, result_element, 4);
    let source_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        source_values,
        vec![5],
    ))));
    let key_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        key_values,
        vec![5],
    ))));
    let coordinate_ty = MirType::semantic(i32_ty);

    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty: result_tensor_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let has_term_function = op == MirReduceOp::Sum;
    let term_function_id = MirFunctionId(1);
    let key_function_id = MirFunctionId(if has_term_function { 2 } else { 1 });
    if has_term_function {
        program.functions.push(function(
            term_function_id.0,
            vec![
                parameter(0, coordinate_ty),
                parameter(1, MirType::semantic(f32_ty)),
            ],
            MirType::semantic(f32_ty),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ));
    }
    program.functions.push(function(
        key_function_id.0,
        vec![parameter(0, key_tensor_ty), parameter(1, coordinate_ty)],
        coordinate_ty,
        MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(0)),
            projections: vec![MirProjection::Index(MirOperand::Place(MirPlace::local(
                MirLocalId(1),
            )))],
        }),
    ));

    let term_ty = if op == MirReduceOp::Count {
        MirType::semantic(bool_ty)
    } else {
        MirType::semantic(f32_ty)
    };
    let filum = MirFilumReduceOp {
        op,
        form: MirFilumForm::Grid,
        filum_binder: None,
        term_binder: MirLocalId(1),
        term_binder_name: None,
        coordinates: vec![MirFilumCoordinate {
            local: MirLocalId(0),
            name: None,
        }],
        lane_width: radix_mir::kernel_plan::GRID_REDUCE_WORKGROUP,
        term_body: Vec::new(),
        term_captures: has_term_function.then_some(0),
        term_result: MirLocalId(1),
        term_ty,
        group: Some(MirFilumGroupOp {
            dimensions: vec![4],
            result_element_ty: MirType::semantic(result_element),
            keys: vec![MirFilumGroupKey {
                captures: 1,
                key_ty: coordinate_ty,
            }],
        }),
    };
    let mut args = vec![
        MirOperand::Value(MirValueId(0)),
        MirOperand::Constant(radix_mir::MirConstant::Int(5)),
    ];
    if has_term_function {
        args.push(MirOperand::Constant(radix_mir::MirConstant::Function(
            term_function_id,
        )));
    }
    args.push(MirOperand::Value(MirValueId(1)));
    args.push(MirOperand::Constant(radix_mir::MirConstant::Function(
        key_function_id,
    )));
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::FilumReduce(filum),
        args,
        return_ty: result_tensor_ty,
    };

    let context = MirValidationContext::new(&types);
    let interner = Interner::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().expect("grouped reducer frame");
    frame.values.insert(MirValueId(0), source_tensor);
    frame.values.insert(MirValueId(1), key_tensor);
    runner.eval_runtime_call(&call, Span::default())
}

fn tensor_values(value: Value) -> Vec<Value> {
    let Value::Tensor(tensor) = value else {
        panic!("grouped reduction result is not a tensor");
    };
    let count = tensor.borrow().logical_len().expect("tensor length");
    (0..count)
        .map(|index| tensor.borrow().value_at(index).expect("tensor cell"))
        .collect()
}

#[test]
fn grouped_sum_returns_typed_buckets_and_keeps_empty_bucket_identity() {
    let result = run_grouped(
        MirReduceOp::Sum,
        [2.0, 3.0, 4.0, 5.0, 6.0]
            .into_iter()
            .map(Value::Float)
            .collect(),
        [1, 0, 1, 2, 1].into_iter().map(Value::Int).collect(),
    )
    .expect("grouped sum");
    assert_eq!(
        tensor_values(result),
        [3.0, 12.0, 5.0, 0.0]
            .into_iter()
            .map(Value::Float)
            .collect::<Vec<_>>()
    );
}

#[test]
fn grouped_count_returns_bucket_counts_and_empty_identities() {
    let result = run_grouped(
        MirReduceOp::Count,
        [false, true, true, true, false]
            .into_iter()
            .map(Value::Bool)
            .collect(),
        [1, 0, 1, 2, 1].into_iter().map(Value::Int).collect(),
    )
    .expect("grouped count");
    assert_eq!(
        tensor_values(result),
        [1, 1, 1, 0].into_iter().map(Value::Int).collect::<Vec<_>>()
    );
}

#[test]
fn grouped_key_out_of_range_uses_the_fatal_tensor_index_error() {
    let error = run_grouped(
        MirReduceOp::Sum,
        [2.0, 3.0, 4.0, 5.0, 6.0]
            .into_iter()
            .map(Value::Float)
            .collect(),
        [1, 4, 1, 2, 1].into_iter().map(Value::Int).collect(),
    )
    .expect_err("group key 4 is outside [0,4)");
    assert_eq!(error.code, crate::RUNNER_CODE_INTERNAL);
    assert_eq!(error.issue, crate::runner_issue(ERR_ACCIPE_INVALID_INDEX));
}
