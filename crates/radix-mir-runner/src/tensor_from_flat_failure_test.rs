//! A flat `lista` whose length does not fill the target tensor shape is a
//! failed conversion, not a run-aborting internal error.
//!
//! `ys ↦ tensor<int, [4]>` and `tensor.construct` lower to `TensorFromFlat`.
//! Its count mismatch carries the conversion code, so a `⇥` channel, a
//! `fac … cape` handler (through the helper frame the lowering builds) and an
//! unhandled run all treat it like every other `↦` failure.

use super::lib_test::evaluate_collection_call;
use super::*;
use crate::host::BufferHost;
use radix_lexer::Interner;
use radix_mir::{
    MirBlock, MirCollectionOp, MirConstant, MirIntrinsic, MirLocal, MirParam, MirParamMode,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, MirValueId,
};
use radix_types::{Primitive, TypeId, TypeTable};

const MISMATCH_TEXT: &str = "tensor conversion expected 4 elements, got 1";

fn int_list(items: &[i64]) -> Value {
    Value::Array(Rc::new(RefCell::new(
        items.iter().copied().map(Value::Int).collect(),
    )))
}

/// `TensorFromFlat(receiver, flat, shape)` over the three operands.
fn from_flat_call() -> MirRuntimeCall {
    MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
            MirOperand::Value(MirValueId(2)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    }
}

#[test]
fn count_mismatch_is_a_recoverable_conversion_failure_with_counts() {
    let error = evaluate_collection_call(
        &from_flat_call(),
        vec![
            (MirValueId(0), Value::Unit),
            (MirValueId(1), int_list(&[7])),
            (MirValueId(2), int_list(&[4])),
        ],
    )
    .expect_err("one item does not fill a four-cell shape");

    assert_eq!(error.code, RUNNER_CODE_CONVERSION);
    assert!(error.is_conversion_failure());
    assert_eq!(error.issue, RUNNER_CONVERSION_LENGTH_MISMATCH);
    assert_eq!(error.message, MISMATCH_TEXT);
    assert_eq!(error.arg("expected"), Some("4"));
    assert_eq!(error.arg("actual"), Some("1"));
    // The class comes from the failure kind, not the wording: a conversion
    // failure exits 65 (EX_DATAERR) when nothing handles it.
    assert_eq!(error.exit_status(), 65);
}

#[test]
fn a_list_longer_than_the_shape_is_the_same_failure() {
    let error = evaluate_collection_call(
        &from_flat_call(),
        vec![
            (MirValueId(0), Value::Unit),
            (MirValueId(1), int_list(&[1, 2, 3, 4, 5])),
            (MirValueId(2), int_list(&[4])),
        ],
    )
    .expect_err("five items do not fill a four-cell shape");

    assert_eq!(error.issue, RUNNER_CONVERSION_LENGTH_MISMATCH);
    assert_eq!(error.arg("expected"), Some("4"));
    assert_eq!(error.arg("actual"), Some("5"));
}

#[test]
fn an_exact_count_still_builds_the_tensor() {
    let value = evaluate_collection_call(
        &from_flat_call(),
        vec![
            (MirValueId(0), Value::Unit),
            (MirValueId(1), int_list(&[1, 2, 3, 4])),
            (MirValueId(2), int_list(&[4])),
        ],
    )
    .expect("four items fill a four-cell shape");
    let Value::Tensor(tensor) = value else {
        panic!("expected a tensor, got {value:?}");
    };
    assert_eq!(tensor.borrow().shape, vec![4]);
}

/// The two-parameter `(flat, shape)` function the lowering builds around the
/// conversion: one `TensorFromFlat`, then its result.
fn from_flat_function(
    id: u32,
    list: TypeId,
    error_ty: Option<TypeId>,
    text: TypeId,
) -> MirFunction {
    let span = Span::default();
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: [0, 1]
            .map(|local| MirParam {
                local: MirLocalId(local),
                name: None,
                ty: MirType::semantic(list),
                mode: MirParamMode::Owned,
                span,
            })
            .to_vec(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
                        args: vec![
                            MirOperand::Constant(MirConstant::Unit),
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: MirType::semantic(TypeId(0)),
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span,
            },
            span,
        }],
        return_ty: MirType::semantic(text),
        error_ty: error_ty.map(MirType::semantic),
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn the_failure_leaves_a_failable_function_as_its_text_error() {
    // Caller `try`-calls the `(flat, shape) ⇥ textus` function: the mismatch is
    // the callee's error, the caller's error block runs, and nothing aborts.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let list = types.array(numerus);
    let text = types.primitive(Primitive::Textus);
    let span = Span::default();
    let args = vec![
        MirOperand::Place(MirPlace::local(MirLocalId(0))),
        MirOperand::Place(MirPlace::local(MirLocalId(1))),
    ];
    let local = |id: u32, ty: TypeId| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty: MirType::semantic(ty),
        mutable: false,
        span,
    };
    let caller = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![
            local(0, list),
            local(1, list),
            local(2, text),
            local(3, text),
        ],
        temps: Vec::new(),
        blocks: vec![
            MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::TryCall {
                        destination: Some(MirPlace::local(MirLocalId(3))),
                        callee: MirCallee::Function(MirFunctionId(1)),
                        args,
                        ok_block: MirBlockId(1),
                        error_place: MirPlace::local(MirLocalId(2)),
                        error_block: MirBlockId(2),
                    },
                    span,
                },
                span,
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(3),
                    )))),
                    span,
                },
                span,
            },
            MirBlock {
                id: MirBlockId(2),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(2),
                    )))),
                    span,
                },
                span,
            },
        ],
        return_ty: MirType::semantic(text),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    };
    let program = MirProgram {
        functions: vec![caller, from_flat_function(1, list, Some(text), text)],
    };
    let context = MirValidationContext::new(&types);
    let interner = Interner::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().expect("caller frame");
    frame.seed_local(MirLocalId(0), int_list(&[7]));
    frame.seed_local(MirLocalId(1), int_list(&[4]));

    runner.run_until_depth(0);

    assert!(
        runner.errors.is_empty(),
        "the call boundary handles the failure: {:?}",
        runner.errors
    );
    assert_eq!(
        runner.return_value,
        Some(Value::Str(MISMATCH_TEXT.to_owned()))
    );
}

#[test]
fn an_unhandled_failure_ends_the_run_with_the_conversion_class() {
    // The same function run with no error channel and no handler above it: the
    // run ends with the one failure, classed as a conversion (65), not as an
    // internal program-logic error (70).
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let list = types.array(numerus);
    let text = types.primitive(Primitive::Textus);
    let program = MirProgram {
        functions: vec![from_flat_function(0, list, None, text)],
    };
    let context = MirValidationContext::new(&types);
    let interner = Interner::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().expect("function frame");
    frame.seed_local(MirLocalId(0), int_list(&[7]));
    frame.seed_local(MirLocalId(1), int_list(&[4]));

    runner.run_until_depth(0);

    let [error] = runner.errors.as_slice() else {
        panic!("expected exactly one failure, got {:?}", runner.errors);
    };
    assert_eq!(error.issue, RUNNER_CONVERSION_LENGTH_MISMATCH);
    assert_eq!(error.message, MISMATCH_TEXT);
    assert_eq!(error.exit_status(), 65);
}
