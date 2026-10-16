//! Tests for the in-process MIR runner.
//!
//! Proves tensor/vector/matrix elementwise dispatch, host fail-closed
//! defaults, runner issue encoding, regex patterns, constant/value
//! equality, and the F-0xx regression behaviors.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    // Bit-pattern fixtures (f64 bit images) read better unseparated.
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::approx_constant,
    clippy::cast_possible_wrap,
    clippy::needless_pass_by_value,
    clippy::float_cmp
)]
#![expect(
    clippy::cast_precision_loss,
    reason = "test helpers promote Int elements to f64 for float comparison"
)]

use super::*;

use radix_mir::MirTerminator;
use radix_mir::MirType;
use radix_mir::{
    MirCollectionOp, MirDiagnosticKind, MirIntrinsic, MirLiftOp, MirProvider, MirRuntimeCall,
};
use radix_types::TypeId;
use std::sync::Arc;

/// Canonical test program shape: one empty function with no params, locals, or blocks.
fn make_function() -> MirFunction {
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

#[test]
fn scalar_mut_param_propagates_through_runner_frames() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirLocal, MirParam, MirParamMode, MirStatement, MirStatementKind, MirTerminator,
        MirTerminatorKind, MirValue, MirValueKind,
    };
    use radix_types::{Primitive, TypeTable};

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let scalar = MirType::semantic(numerus);
    let span = Span::default();
    let mut interner = Interner::new();
    let mut program = MirProgram::new();
    let mutabile = interner.intern("mutabile");
    let salve = interner.intern("salve");
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: scalar,
            mutable: true,
            span,
        }],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    });
    program.functions.push(MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(1),
            name: None,
            ty: scalar,
            mode: MirParamMode::MutRef,
            span,
        }],
        locals: vec![MirLocal {
            id: MirLocalId(1),
            name: None,
            ty: scalar,
            mutable: true,
            span,
        }],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(1)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Binary {
                            op: MirBinOp::Mul,
                            lhs: MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            rhs: MirOperand::Constant(MirConstant::Int(2)),
                        },
                        ty: scalar,
                        span,
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
        return_ty: MirType::semantic(vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    });

    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner
        .frames
        .last_mut()
        .expect("caller frame")
        .seed_local(MirLocalId(0), Value::Int(3));

    runner
        .invoke_call(
            &MirCallee::Function(MirFunctionId(1)),
            &[MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            span,
        )
        .expect("mut call should execute");
    assert_eq!(
        runner
            .read_local(MirLocalId(0), span)
            .expect("caller value"),
        Value::Int(6)
    );
    let Err(error) = runner.invoke_call(
        &MirCallee::Function(MirFunctionId(1)),
        &[MirOperand::Constant(MirConstant::Int(3))],
        span,
    ) else {
        panic!("in parameter must reject a non-place argument");
    };
    assert_eq!(
        error.issue,
        "runner_mutable_parameter_requires_a_place_argument"
    );

    for operand in [
        MirOperand::Constant(MirConstant::String(mutabile)),
        MirOperand::Place(MirPlace::local(MirLocalId(0))),
        MirOperand::Constant(MirConstant::String(salve)),
    ] {
        runner
            .eval_runtime_call(
                &MirRuntimeCall {
                    intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
                    args: vec![operand],
                    return_ty: MirType::semantic(vacuum),
                },
                span,
            )
            .expect("nota should execute");
    }
    drop(runner);
    assert_eq!(host.stdout_lines, ["mutabile", "6", "salve"]);
}

#[test]
fn scalar_mut_param_field_projection_writes_back_through_runner_frames() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirLocal, MirParam, MirParamMode, MirStatement, MirStatementKind, MirTerminator,
        MirTerminatorKind, MirValue, MirValueKind,
    };
    use radix_types::{Primitive, TypeTable};
    use rustc_hash::FxHashMap;
    use std::cell::RefCell;
    use std::rc::Rc;

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let scalar = MirType::semantic(numerus);
    let span = Span::default();
    let mut interner = Interner::new();
    let mut program = MirProgram::new();
    let count = interner.intern("count");
    let label = interner.intern("label");
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: scalar,
            mutable: true,
            span,
        }],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    });
    program.functions.push(MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(1),
            name: None,
            ty: scalar,
            mode: MirParamMode::MutRef,
            span,
        }],
        locals: vec![MirLocal {
            id: MirLocalId(1),
            name: None,
            ty: scalar,
            mutable: true,
            span,
        }],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(1)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(7))),
                        ty: scalar,
                        span,
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
        return_ty: MirType::semantic(vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    });

    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let fields = FxHashMap::from_iter([
        (count, Value::Int(3)),
        (label, Value::Str("kept".to_owned())),
    ]);
    runner.frames.last_mut().expect("caller frame").seed_local(
        MirLocalId(0),
        Value::Record(Rc::new(RefCell::new(fields)), Rc::from([count, label])),
    );

    // Before the write-back ruling this call copied the record field into
    // the callee cell and left the caller's `count` at 3.
    let outcome = runner
        .invoke_call(
            &MirCallee::Function(MirFunctionId(1)),
            &[MirOperand::Place(MirPlace {
                base: MirPlaceBase::Local(MirLocalId(0)),
                projections: vec![MirProjection::Field(count)],
            })],
            span,
        )
        .expect("mut call through a projected place should execute");
    assert!(matches!(outcome, CallOutcome::Ok(Value::Unit)));

    // Surgical write-back: only `count` moves, the record itself survives.
    let Value::Record(fields, order) = runner
        .read_local(MirLocalId(0), span)
        .expect("caller value")
    else {
        panic!("caller local must still hold the record");
    };
    assert_eq!(fields.borrow().get(&count), Some(&Value::Int(7)));
    assert_eq!(
        fields.borrow().get(&label),
        Some(&Value::Str("kept".to_owned()))
    );
    assert_eq!(order.as_ref(), &[count, label]);
}

#[test]
fn closure_capture_projection_write_updates_environment_slot() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirCaptureMode, MirClosureCapture, MirClosureEnvironment, MirClosureEnvironmentId,
        MirLocal, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind,
        MirValidationContext, MirValue, MirValueKind,
    };
    use radix_types::{Primitive, TypeTable};
    use rustc_hash::FxHashMap;
    use std::cell::RefCell;
    use std::rc::Rc;

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let scalar = MirType::semantic(numerus);
    let span = Span::default();
    let mut interner = Interner::new();
    let name = interner.intern("c");
    let environment = MirClosureEnvironmentId(0);
    let source = radix_hir::DefId(7);
    let capture_place = MirPlace {
        base: MirPlaceBase::Local(MirLocalId(0)),
        projections: vec![MirProjection::ClosureCapture {
            environment,
            source,
        }],
    };
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: scalar,
                mutable: true,
                span,
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: capture_place.clone(),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Constant(
                                    MirConstant::Int(7),
                                )),
                                ty: scalar,
                                span,
                            },
                        },
                        span,
                    },
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: None,
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
                                args: vec![MirOperand::Place(capture_place)],
                                return_ty: MirType::semantic(vacuum),
                            },
                        },
                        span,
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }],
            return_ty: MirType::semantic(vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }],
    };
    let mut context = MirValidationContext::new(&types);
    context.closure_environments.insert(
        environment,
        MirClosureEnvironment {
            id: environment,
            function: MirFunctionId(0),
            value_ty: scalar,
            environment_local: MirLocalId(0),
            captures: vec![MirClosureCapture {
                source,
                name,
                ty: scalar,
                source_place: MirPlace::local(MirLocalId(9)),
                mode: MirCaptureMode::Value,
            }],
        },
    );
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.frames.last_mut().expect("closure frame").seed_local(
        MirLocalId(0),
        Value::ClosureEnvironment(
            environment,
            Rc::new(RefCell::new(FxHashMap::from_iter([(name, Value::Int(1))]))),
        ),
    );

    runner.run_until_depth(0);

    assert!(runner.errors.is_empty(), "capture write should execute");
    drop(runner);
    assert_eq!(host.stdout_lines, ["7"]);
}

#[test]
fn vide_writes_stderr_and_nota_writes_stdout() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{Primitive, TypeTable};

    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let span = Span::default();
    let mut interner = Interner::new();
    let trace = interner.intern("trace");
    let shown = interner.intern("shown");
    let program = MirProgram::new();
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    for (kind, text) in [
        (MirDiagnosticKind::Vide, trace),
        (MirDiagnosticKind::Nota, shown),
    ] {
        runner
            .eval_runtime_call(
                &MirRuntimeCall {
                    intrinsic: MirIntrinsic::Diagnostic(kind),
                    args: vec![MirOperand::Constant(MirConstant::String(text))],
                    return_ty: MirType::semantic(vacuum),
                },
                span,
            )
            .expect("diagnostic should execute");
    }
    drop(runner);
    assert_eq!(host.stderr_lines, ["trace"]);
    assert_eq!(host.stdout_lines, ["shown"]);
}

#[test]
fn repeated_numbered_hole_reads_one_argument() {
    // Operator ruling 2026-10-13: `"§0, §0, §0, and §1"("her", "me")`
    // renders `her, her, her, and me` — a numbered hole selects its argument
    // by number, and the same number may appear any number of times. The
    // template carries more holes than values; nothing may panic and no
    // value may be consumed positionally.
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{Primitive, TypeTable};

    let types = TypeTable::new();
    let textus = MirType::semantic(types.primitive(Primitive::Textus));
    let span = Span::default();
    let mut interner = Interner::new();
    let template = interner.intern("§0, §0, §0, and §1");
    let her = interner.intern("her");
    let me = interner.intern("me");
    let program = MirProgram::new();
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let rendered = runner
        .eval_runtime_call(
            &MirRuntimeCall {
                intrinsic: MirIntrinsic::FormatString { template },
                args: vec![
                    MirOperand::Constant(MirConstant::String(her)),
                    MirOperand::Constant(MirConstant::String(me)),
                ],
                return_ty: textus,
            },
            span,
        )
        .expect("format call should execute");
    drop(runner);
    assert_eq!(rendered, Value::Str("her, her, her, and me".to_owned()));
}

#[test]
fn template_text_decodes_escapes_like_plain_strings() {
    // Template text is raw source: its escapes decode exactly as a plain
    // string constant's do, while `§` hole parsing is unchanged.
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{Primitive, TypeTable};

    let types = TypeTable::new();
    let textus = MirType::semantic(types.primitive(Primitive::Textus));
    let span = Span::default();
    let mut interner = Interner::new();
    let template = interner.intern(r"a\tb\nc\\d\u{41} §");
    let x = interner.intern("x");
    let program = MirProgram::new();
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let rendered = runner
        .eval_runtime_call(
            &MirRuntimeCall {
                intrinsic: MirIntrinsic::FormatString { template },
                args: vec![MirOperand::Constant(MirConstant::String(x))],
                return_ty: textus,
            },
            span,
        )
        .expect("format call should execute");
    drop(runner);
    assert_eq!(rendered, Value::Str("a\tb\nc\\dA x".to_owned()));
}

#[test]
fn converts_numeric_array_to_octeti_in_one_step() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let program = MirProgram::new();
    let runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let input = Value::Array(Rc::new(RefCell::new(vec![
        Value::Int(0),
        Value::Int(127),
        Value::UInt(255),
    ])));

    let output = runner
        .convert_to_octeti(input, Span::default())
        .expect("byte-valued numeric lista should convert to octeti");

    assert_eq!(output, Value::Octeti(vec![0, 127, 255]));
}

#[test]
fn rounding_ops_return_numerus_so_bound_compares_do_not_mismatch() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{Primitive, TypeTable};

    let mut host = BufferHost::default();
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let fractus = types.primitive(Primitive::Fractus);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    let round = |value: f64| MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Round),
        args: vec![MirOperand::Constant(MirConstant::Float(value))],
        return_ty: MirType::semantic(numerus),
    };
    assert_eq!(
        runner
            .eval_runtime_call(&round(1.0), radix_lexer::Span::default())
            .expect("round 1.0"),
        Value::Int(1)
    );
    // Nearest rounding ties to even on the runner (sb260 b3410a4863, need
    // ea7cfde1): 0.5 -> 0, 1.5 -> 2, 2.5 -> 2, -0.5 -> 0; never ties-away.
    assert_eq!(
        runner
            .eval_runtime_call(&round(0.5), radix_lexer::Span::default())
            .expect("round 0.5"),
        Value::Int(0)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&round(1.5), radix_lexer::Span::default())
            .expect("round 1.5"),
        Value::Int(2)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&round(2.5), radix_lexer::Span::default())
            .expect("round 2.5"),
        Value::Int(2)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&round(0.2), radix_lexer::Span::default())
            .expect("round 0.2"),
        Value::Int(0)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&round(-0.5), radix_lexer::Span::default())
            .expect("round -0.5"),
        Value::Int(0)
    );

    let abs_float = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Absolute),
        args: vec![MirOperand::Constant(MirConstant::Float(-3.5))],
        return_ty: MirType::semantic(fractus),
    };
    assert_eq!(
        runner
            .eval_runtime_call(&abs_float, radix_lexer::Span::default())
            .expect("abs keeps fractus"),
        Value::Float(3.5)
    );
}

#[test]
fn tensor_layernorm_rank2_no_affine() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Build 2×3 input tensor: [[1, 2, 3], [4, 5, 6]] ---
    let input_data = vec![
        Value::Float(1.0),
        Value::Float(2.0),
        Value::Float(3.0),
        Value::Float(4.0),
        Value::Float(5.0),
        Value::Float(6.0),
    ];
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![2, 3],
    ))));

    // --- 2. Build minimal runner context ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    // Push a frame and insert the input tensor in value slot 0.
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    // --- 3. Build the layernorm call (axis=1, epsilon=1e-5, no affine) ---
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorLayerNorm),
        args: vec![
            MirOperand::Value(MirValueId(0)),               // input tensor
            MirOperand::Constant(MirConstant::Int(1)),      // axis
            MirOperand::Constant(MirConstant::Float(1e-5)), // epsilon
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    // --- 4. Evaluate ---
    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("layernorm dispatch should succeed");

    // --- 5. Verify ---
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![2, 3], "output shape must match input shape");

    // Row 0: mean=2.0, var = ((1-2)²+(2-2)²+(3-2)²)/3 = 0.666..., inv_std ≈ 1.22474
    // Row 1: mean=5.0, var = ((4-5)²+(5-5)²+(6-5)²)/3 = 0.666..., same inv_std
    // Normalized row 0: [-1.2247, 0.0, 1.2247]
    // Normalized row 1: [-1.2247, 0.0, 1.2247]
    #[allow(clippy::float_cmp)]
    let expected: [(usize, f64); 6] = [
        (0, -1.224_744_871_391_589),
        (1, 0.0),
        (2, 1.224_744_871_391_589),
        (3, -1.224_744_871_391_589),
        (4, 0.0),
        (5, 1.224_744_871_391_589),
    ];

    for (idx, expected_val) in &expected {
        let actual = match &out.logical_values().expect("valid tensor layout")[*idx] {
            Value::Float(f) => *f,
            Value::Int(i) => *i as f64,
            other => panic!("item {idx}: expected numeric, got {other:?}"),
        };
        let diff = (actual - expected_val).abs();
        assert!(
            diff < 1e-4,
            "item {idx}: actual={actual}, expected={expected_val}, diff={diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// TensorSoftmax runner dispatch smoke test
// ---------------------------------------------------------------------------

#[test]
fn tensor_softmax_rank1() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Build rank-1 input tensor: [1.0, 2.0, 3.0] ---
    let input_data = vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)];
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![3],
    ))));

    // --- 2. Build minimal runner context ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    // Push a frame and insert the input tensor in value slot 0.
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    // --- 3. Build the softmax call ---
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSoftmax),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };

    // --- 4. Evaluate ---
    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("softmax dispatch should succeed");

    // --- 5. Verify ---
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![3], "output shape must match input shape");
    let out_values = out.logical_values().expect("valid tensor layout");

    // softmax([1.0, 2.0, 3.0])
    // max=3.0, exp(1-3)=e^-2≈0.1353, exp(2-3)=e^-1≈0.3679, exp(3-3)=1.0
    // sum≈1.5032, normalized≈[0.0900, 0.2447, 0.6652]
    let sum: f64 = out_values
        .iter()
        .map(|v| match v {
            Value::Float(f) => *f,
            Value::Int(i) => *i as f64,
            _ => 0.0,
        })
        .sum();
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "softmax sum must be 1.0, got {sum}"
    );

    let expected: [(usize, f64); 3] = [
        (0, 0.090_030_573_170_380_46),
        (1, 0.244_728_471_054_797_67),
        (2, 0.665_240_955_774_821_9),
    ];
    for (idx, expected_val) in &expected {
        let actual = match &out.logical_values().expect("valid tensor layout")[*idx] {
            Value::Float(f) => *f,
            Value::Int(i) => *i as f64,
            other => panic!("item {idx}: expected numeric, got {other:?}"),
        };
        let diff = (actual - expected_val).abs();
        assert!(
            diff < 1e-6,
            "item {idx}: actual={actual}, expected={expected_val}, diff={diff:.6e}"
        );
    }
}

#[test]
fn tensor_softmax_rank2() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Build rank-2 input tensor: [[1, 2, 3], [1, 2, 3]] ---
    let input_data = vec![
        Value::Float(1.0),
        Value::Float(2.0),
        Value::Float(3.0),
        Value::Float(1.0),
        Value::Float(2.0),
        Value::Float(3.0),
    ];
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![2, 3],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSoftmax),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("softmax rank-2 dispatch should succeed");

    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![2, 3], "output shape must match input shape");

    // Each row sums to 1.0.
    let row0_sum: f64 = out.logical_values().expect("valid tensor layout")[0..3]
        .iter()
        .map(|v| match v {
            Value::Float(f) => *f,
            _ => 0.0,
        })
        .sum();
    let row1_sum: f64 = out.logical_values().expect("valid tensor layout")[3..6]
        .iter()
        .map(|v| match v {
            Value::Float(f) => *f,
            _ => 0.0,
        })
        .sum();
    assert!(
        (row0_sum - 1.0).abs() < 1e-6,
        "row 0 sum must be 1.0, got {row0_sum}"
    );
    assert!(
        (row1_sum - 1.0).abs() < 1e-6,
        "row 1 sum must be 1.0, got {row1_sum}"
    );

    // Both rows are identical since input rows are identical.
    for i in 0..3 {
        let diff = match (
            &out.logical_values().expect("valid tensor layout")[i],
            &out.logical_values().expect("valid tensor layout")[i + 3],
        ) {
            (Value::Float(a), Value::Float(b)) => (a - b).abs(),
            _ => 1.0,
        };
        assert!(
            diff < 1e-10,
            "rows must be identical; item {i} differs by {diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// TensorGelu runner dispatch smoke test
// ---------------------------------------------------------------------------

#[test]
fn tensor_gelu_rank1() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Build rank-1 input tensor: [-3.0, -1.0, 0.0, 1.0, 3.0] ---
    let inputs = [-3.0, -1.0, 0.0, 1.0, 3.0];
    let input_data: Vec<Value> = inputs.iter().map(|v| Value::Float(*v)).collect();
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![5],
    ))));

    // --- 2. Build minimal runner context ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    // --- 3. Build the tensor_gelu call ---
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGelu),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };

    // --- 4. Evaluate ---
    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("tensor gelu dispatch should succeed");

    // --- 5. Verify ---
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![5], "output shape must match input shape");
    assert_eq!(
        out.logical_values().expect("valid tensor layout").len(),
        5,
        "output data length must match input"
    );

    for (idx, input) in inputs.iter().enumerate() {
        let expected = gelu_f64(*input);
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        let diff = (actual - expected).abs();
        assert!(
            diff < 1e-12,
            "tensor_gelu item {idx}: input={input}, actual={actual}, expected={expected}, diff={diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// TensorRelu runner dispatch smoke test
// ---------------------------------------------------------------------------

#[test]
fn tensor_relu_rank1() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Mixed-sign input proves ReLU is not identity: negatives clamp to 0.0.
    let inputs = [-3.0, -1.0, 0.0, 1.0, 3.0];
    let input_data: Vec<Value> = inputs.iter().map(|v| Value::Float(*v)).collect();
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![5],
    ))));

    // --- 2. Build minimal runner context ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    // --- 3. Build the tensor_relu call ---
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRelu),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };

    // --- 4. Evaluate ---
    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("tensor relu dispatch should succeed");

    // --- 5. Verify ---
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![5], "output shape must match input shape");
    assert_eq!(
        out.logical_values().expect("valid tensor layout").len(),
        5,
        "output data length must match input"
    );

    for (idx, input) in inputs.iter().enumerate() {
        let expected = input.max(0.0);
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        assert!(
            (actual - expected).abs() < 1e-9,
            "tensor_relu item {idx}: input={input}, actual={actual}, expected={expected}"
        );
    }
}

// ---------------------------------------------------------------------------
// MirUnOp::Gelu tensor dispatch test
// ---------------------------------------------------------------------------

#[test]
fn mir_unop_gelu_tensor() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let inputs = [-2.0, -0.5, 0.0, 0.5, 2.0];
    let input_data: Vec<Value> = inputs.iter().map(|v| Value::Float(*v)).collect();
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![5],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    // --- 3. Call eval_unop with MirUnOp::Gelu on the tensor operand ---
    let result = runner
        .eval_unop(
            MirUnOp::Gelu,
            &MirOperand::Value(MirValueId(0)),
            MirType::semantic(TypeId(0)),
            radix_lexer::Span::default(),
        )
        .expect("MirUnOp::Gelu tensor dispatch should succeed");

    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![5], "output shape must match");
    assert_eq!(out.logical_values().expect("valid tensor layout").len(), 5);

    for (idx, input) in inputs.iter().enumerate() {
        let expected = gelu_f64(*input);
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        let diff = (actual - expected).abs();
        assert!(
            diff < 1e-12,
            "MirUnOp::Gelu tensor item {idx}: input={input}, actual={actual}, expected={expected}, diff={diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// MirUnOp::Tanh tensor dispatch test
// ---------------------------------------------------------------------------

#[test]
fn mir_unop_tanh_tensor() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let inputs = [-2.0, -1.0, 0.0, 1.0, 2.0];
    let input_data: Vec<Value> = inputs.iter().map(|v| Value::Float(*v)).collect();
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![5],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    let result = runner
        .eval_unop(
            MirUnOp::Tanh,
            &MirOperand::Value(MirValueId(0)),
            MirType::semantic(TypeId(0)),
            radix_lexer::Span::default(),
        )
        .expect("MirUnOp::Tanh tensor dispatch should succeed");

    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![5], "shape preserved");

    for (idx, input) in inputs.iter().enumerate() {
        let expected = input.tanh();
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        let diff = (actual - expected).abs();
        assert!(
            diff < 1e-15,
            "MirUnOp::Tanh tensor item {idx}: input={input}, actual={actual}, expected={expected}, diff={diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// MirUnOp::Exp tensor dispatch test (undocumented scope from original commit)
// ---------------------------------------------------------------------------

#[test]
fn mir_unop_exp_tensor() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let inputs = [-1.0, 0.0, 1.0, 2.0, 3.0];
    let input_data: Vec<Value> = inputs.iter().map(|v| Value::Float(*v)).collect();
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![5],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    let result = runner
        .eval_unop(
            MirUnOp::Exp,
            &MirOperand::Value(MirValueId(0)),
            MirType::semantic(TypeId(0)),
            radix_lexer::Span::default(),
        )
        .expect("MirUnOp::Exp tensor dispatch should succeed");

    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![5], "shape preserved");

    for (idx, input) in inputs.iter().enumerate() {
        let expected = input.exp();
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        let diff = (actual - expected).abs();
        let tolerance = 1e-15;
        assert!(
            diff < tolerance,
            "MirUnOp::Exp tensor item {idx}: input={input}, actual={actual}, expected={expected}, diff={diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// tensor_elementwise_or_scalar broadcast test
// ---------------------------------------------------------------------------

#[test]
fn tensor_elementwise_or_scalar_broadcast() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Tensor: [1.0, 2.0, 3.0], scalar: 2.0 → expected: [2.0, 4.0, 6.0]
    let input_data: Vec<Value> = vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)];
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![3],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), Value::Float(2.0));

    // --- 3. Build TensorMul call with (tensor, scalar) ---
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMul),
        args: vec![
            MirOperand::Value(MirValueId(0)), // tensor
            MirOperand::Value(MirValueId(1)), // scalar
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("tensor mul scalar broadcast should succeed");

    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![3], "shape preserved");

    let expected_values = [2.0, 4.0, 6.0];
    for (idx, expected) in expected_values.iter().enumerate() {
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        assert!(
            (actual - expected).abs() < 1e-15,
            "broadcast mul item {idx}: actual={actual}, expected={expected}"
        );
    }
}

// ---------------------------------------------------------------------------
// (scalar, Tensor) Div: 1.0 / [2.0, 4.0, 8.0] → [0.5, 0.25, 0.125]
// Exercises the (scalar, Tensor(cell)) arm added for non-commutative op
// operand-order correctness (fixed in 73f3db582).
// ---------------------------------------------------------------------------

#[test]
fn tensor_elementwise_or_scalar_broadcast_div_scalar_tensor() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Scalar: 1.0, Tensor: [2.0, 4.0, 8.0] → expected: [0.5, 0.25, 0.125]
    let input_data: Vec<Value> = vec![Value::Float(2.0), Value::Float(4.0), Value::Float(8.0)];
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![3],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), Value::Float(1.0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), input_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorDiv),
        args: vec![
            MirOperand::Value(MirValueId(0)), // scalar: 1.0
            MirOperand::Value(MirValueId(1)), // tensor: [2.0, 4.0, 8.0]
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("scalar div tensor broadcast should succeed");

    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![3], "shape preserved");

    // 1.0 / [2.0, 4.0, 8.0] = [0.5, 0.25, 0.125]
    let expected_values = [0.5, 0.25, 0.125];
    for (idx, expected) in expected_values.iter().enumerate() {
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        assert!(
            (actual - expected).abs() < 1e-15,
            "scalar/tensor div item {idx}: actual={actual}, expected={expected}"
        );
    }
}

// ---------------------------------------------------------------------------
// (scalar, Tensor) Sub: 10.0 - [1.0, 2.0, 3.0] → [9.0, 8.0, 7.0]
// Exercises the (scalar, Tensor(cell)) arm for non-commutative operand-order
// correctness (fixed in 73f3db582).
// ---------------------------------------------------------------------------

#[test]
fn tensor_elementwise_or_scalar_broadcast_sub_scalar_tensor() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Scalar: 10.0, Tensor: [1.0, 2.0, 3.0] → expected: [9.0, 8.0, 7.0]
    let input_data: Vec<Value> = vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)];
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![3],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), Value::Float(10.0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), input_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSub),
        args: vec![
            MirOperand::Value(MirValueId(0)), // scalar: 10.0
            MirOperand::Value(MirValueId(1)), // tensor: [1.0, 2.0, 3.0]
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("scalar sub tensor broadcast should succeed");

    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![3], "shape preserved");

    // 10.0 - [1.0, 2.0, 3.0] = [9.0, 8.0, 7.0]
    let expected_values = [9.0, 8.0, 7.0];
    for (idx, expected) in expected_values.iter().enumerate() {
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        assert!(
            (actual - expected).abs() < 1e-15,
            "scalar/tensor sub item {idx}: actual={actual}, expected={expected}"
        );
    }
}

// ---------------------------------------------------------------------------
// MirUnOp::Sqrt on Value::Tensor unit test
// ---------------------------------------------------------------------------

#[test]
fn unop_sqrt_tensor() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Build 2×2 input tensor: [[4.0, 9.0], [16.0, 25.0]] ---
    let input_data = vec![
        Value::Float(4.0),
        Value::Float(9.0),
        Value::Float(16.0),
        Value::Float(25.0),
    ];
    let input_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        input_data,
        vec![2, 2],
    ))));

    // --- 2. Build minimal runner context ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input_tensor);

    // --- 3. Build MirValueKind::Unary with Sqrt ---
    let mir_value = MirValue {
        id: MirValueId(42),
        kind: MirValueKind::Unary {
            op: MirUnOp::Sqrt,
            operand: MirOperand::Value(MirValueId(0)),
        },
        ty: MirType::semantic(TypeId(0)),
        span: radix_lexer::Span::default(),
    };

    // --- 4. Evaluate ---
    let result = runner
        .eval_value(&mir_value, radix_lexer::Span::default())
        .expect("sqrt tensor should succeed");

    // --- 5. Verify ---
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![2, 2], "output shape must match input shape");

    let expected = [2.0, 3.0, 4.0, 5.0];
    for (idx, expected_val) in expected.iter().enumerate() {
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        let diff = (actual - expected_val).abs();
        assert!(
            diff < 1e-15,
            "sqrt tensor item {idx}: actual={actual}, expected={expected_val}, diff={diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// TensorFill prefix broadcast [B]→[B,D] unit test
// ---------------------------------------------------------------------------

#[test]
fn tensor_fill_prefix_broadcast() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Create receiver [2,3] tensor and fill [2] tensor ---
    let receiver_data = vec![Value::Float(0.0); 6];
    let receiver_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        receiver_data,
        vec![2, 3],
    ))));

    // Fill tensor shape [2] — prefix broadcast: each element repeats 3 times.
    let fill_data = vec![Value::Float(10.0), Value::Float(20.0)];
    let fill_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        fill_data,
        vec![2],
    ))));

    // --- 2. Build minimal runner context ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), receiver_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), fill_tensor);

    // --- 3. Build TensorFill call ---
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFill),
        args: vec![
            MirOperand::Value(MirValueId(0)), // receiver tensor [2,3]
            MirOperand::Value(MirValueId(1)), // fill tensor [2]
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    // --- 4. Evaluate ---
    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("tensor fill prefix broadcast should succeed");

    // --- 5. Verify: [2,3] filled from [2] prefix → [10, 10, 10, 20, 20, 20] ---
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![2, 3], "output shape must be [2, 3]");

    let expected = [10.0, 10.0, 10.0, 20.0, 20.0, 20.0];
    for (idx, expected_val) in expected.iter().enumerate() {
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        let diff = (actual - expected_val).abs();
        assert!(
            diff < 1e-15,
            "fill prefix item {idx}: actual={actual}, expected={expected_val}, diff={diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// TensorFill suffix broadcast [D]→[B,D] unit test
// ---------------------------------------------------------------------------

#[test]
fn tensor_fill_suffix_broadcast() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Create receiver [2,3] tensor and fill [3] tensor ---
    let receiver_data = vec![Value::Float(0.0); 6];
    let receiver_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        receiver_data,
        vec![2, 3],
    ))));

    // Fill tensor shape [3] — suffix broadcast: cycle the block 2 times.
    let fill_data = vec![
        Value::Float(100.0),
        Value::Float(200.0),
        Value::Float(300.0),
    ];
    let fill_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        fill_data,
        vec![3],
    ))));

    // --- 2. Build minimal runner context ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), receiver_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), fill_tensor);

    // --- 3. Build TensorFill call ---
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFill),
        args: vec![
            MirOperand::Value(MirValueId(0)), // receiver tensor [2,3]
            MirOperand::Value(MirValueId(1)), // fill tensor [3]
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    // --- 4. Evaluate ---
    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("tensor fill suffix broadcast should succeed");

    // --- 5. Verify: [2,3] filled from [3] suffix → [100, 200, 300, 100, 200, 300] ---
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![2, 3], "output shape must be [2, 3]");

    let expected = [100.0, 200.0, 300.0, 100.0, 200.0, 300.0];
    for (idx, expected_val) in expected.iter().enumerate() {
        let actual = match &out.logical_values().expect("valid tensor layout")[idx] {
            Value::Float(f) => *f,
            other => panic!("item {idx}: expected float, got {other:?}"),
        };
        let diff = (actual - expected_val).abs();
        assert!(
            diff < 1e-15,
            "fill suffix item {idx}: actual={actual}, expected={expected_val}, diff={diff:.6e}"
        );
    }
}

// ---------------------------------------------------------------------------
// TensorCruxEntropia runner dispatch smoke test
// ---------------------------------------------------------------------------

#[test]
fn tensor_crux_entropia_rank2() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Build rank-2 logits: [[2.0, 1.0, 0.0], [1.0, 2.0, 1.0]] ---
    let logits_data = vec![
        Value::Float(2.0),
        Value::Float(1.0),
        Value::Float(0.0),
        Value::Float(1.0),
        Value::Float(2.0),
        Value::Float(1.0),
    ];
    let logits_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        logits_data,
        vec![2, 3],
    ))));

    // --- 2. Build rank-2 targets: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] ---
    let targets_data = vec![
        Value::Float(1.0),
        Value::Float(0.0),
        Value::Float(0.0),
        Value::Float(0.0),
        Value::Float(1.0),
        Value::Float(0.0),
    ];
    let targets_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        targets_data,
        vec![2, 3],
    ))));

    // --- 3. Build minimal runner context ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), logits_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), targets_tensor);

    // --- 4. Build the crux_entropia call ---
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCruxEntropia),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    // --- 5. Evaluate ---
    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("crux_entropia dispatch should succeed");

    // --- 6. Verify ---
    // softmax row0 = [0.6652, 0.2447, 0.0900], row1 = [0.2119, 0.5761, 0.2119]
    // CE: row0 = -log(0.6652+ε) ≈ 0.4076, row1 = -log(0.5761+ε) ≈ 0.5514
    // Loss = (0.4076 + 0.5514) / 2 (batch rows) ≈ 0.4795
    let Value::Float(loss) = result else {
        panic!("expected float loss, got {result:?}");
    };
    let expected = 0.4795_f64;
    let diff = (loss - expected).abs();
    assert!(
        diff < 1e-3,
        "crux_entropia loss: actual={loss}, expected={expected}, diff={diff:.6e}"
    );
}

/// XENTDIV: the loss is the sum over rows divided by the batch row count.
/// Uniform logits over [2, 3] give a per-row loss of ln 3, so the mean is
/// ln 3 (about 1.0986); dividing the sum by the class count 3 would give
/// 2 ln 3 / 3 (about 0.7324).
#[test]
fn tensor_crux_entropia_rank2_divides_by_batch_rows() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let tensor = |values: [f64; 6]| {
        Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
            values.into_iter().map(Value::Float).collect(),
            vec![2, 3],
        ))))
    };
    let logits_tensor = tensor([0.0; 6]);
    let targets_tensor = tensor([1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    frame.values.insert(MirValueId(0), logits_tensor);
    frame.values.insert(MirValueId(1), targets_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCruxEntropia),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let result = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("crux_entropia dispatch should succeed");
    let Value::Float(loss) = result else {
        panic!("expected float loss, got {result:?}");
    };
    let expected = 3.0_f64.ln();
    assert!(
        (loss - expected).abs() < 1e-5,
        "crux_entropia loss: actual={loss}, expected={expected}"
    );
}

// ---------------------------------------------------------------------------
// Host default fail-closed smoketests (PRP-001, PRP-002)
// ---------------------------------------------------------------------------

struct MinimalHost;

impl Host for MinimalHost {
    fn scribe(&mut self, _kind: MirDiagnosticKind, _text: &str) -> Result<(), RunnerError> {
        Ok(())
    }
    fn read_line(&mut self) -> Result<Option<String>, RunnerError> {
        Ok(None)
    }
    fn abort(&mut self, reason: &str) -> ! {
        panic!("runner abort: {reason}")
    }
    fn provider(&mut self, _provider: &MirProvider) -> Result<Value, RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "provider"))
    }
    fn exit(&mut self, code: i32) -> ! {
        panic!("runner exit: {code}")
    }
}

#[test]
fn provider_failure_uses_stable_identity() {
    let mut interner = Interner::new();
    let module = interner.intern("camera");
    let item = interner.intern("include");
    let provider = MirProvider {
        kind: radix_mir::MirProviderKind::Package,
        module: vec![module],
        name: item,
    };
    let identity = super::host::provider_identity(&provider, &interner);
    let error =
        RunnerError::unsupported(Span::default(), "provider").with_provider_identity(&identity);

    assert_eq!(identity, "package:camera::include");
    assert_eq!(
        error.issue,
        "runner_faberscript_unsupported_provider_package_camera_include"
    );
}

#[test]
fn provider_identity_rewrite_ignores_prose_matches() {
    // WHY: the rewrite keys on the structured code + `feature=provider` arg.
    // A diagnostic that merely mentions the MIR dump spelling (`provider sym#`)
    // must keep its own message and issue.
    let error = RunnerError::internal(Span::default(), "runtime provider sym#30::sym#31")
        .with_provider_identity("package:camera::include");

    assert_eq!(error.issue, "runner_runtime_provider_sym_30_sym_31");
    assert!(error.args.iter().all(|(key, _)| key != "identity"));
}

#[test]
fn provider_identity_keeps_declared_package_route() {
    let mut interner = Interner::new();
    let provider = MirProvider {
        kind: radix_mir::MirProviderKind::Package,
        module: vec![interner.intern("geom"), interner.intern("Mesh")],
        name: interner.intern("layout_count"),
    };

    assert_eq!(
        super::host::provider_identity(&provider, &interner),
        "package:geom::Mesh::layout_count"
    );
}

#[test]
fn host_provider_failure_does_not_expose_symbol_index() {
    let mut host = BufferHost::default();
    let provider = MirProvider {
        kind: radix_mir::MirProviderKind::Package,
        module: vec![radix_lexer::Symbol(30)],
        name: radix_lexer::Symbol(31),
    };
    let error = host
        .provider(&provider)
        .expect_err("provider must fail closed");

    assert_eq!(error.issue, "runner_faberscript_unsupported_provider");
}

#[test]
fn package_provider_routes_fail_closed() {
    use radix_lexer::Interner;
    use radix_mir::{MirOperand, MirProviderKind, MirValidationContext};
    use radix_types::TypeTable;

    let mut interner = Interner::new();
    let module = interner.intern("geomlib:data");
    let item = interner.intern("data");
    let name = interner.intern("layout_count");
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Provider(MirProvider {
            kind: MirProviderKind::Package,
            module: vec![module, item],
            name,
        }),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let error = runner
        .eval_runtime_call(&call, Span::default())
        .expect_err("package providers have no runner host bridge");
    assert_eq!(
        error.issue,
        "runner_faberscript_unsupported_provider_package_geomlib_data_data_layout_count"
    );
}

#[test]
fn host_argumenta_default_rejects() {
    let host = MinimalHost;
    let result = host.argumenta();
    assert!(result.is_err(), "default argumenta must reject");
}

#[test]
fn host_pid_default_rejects() {
    let host = MinimalHost;
    let result = host.pid();
    assert!(result.is_err(), "default pid must reject");
}

#[test]
fn host_env_get_default_rejects() {
    let host = MinimalHost;
    let result = host.env_get("PATH");
    assert!(result.is_err(), "default env_get must reject");
}

#[test]
fn host_env_set_default_rejects() {
    let mut host = MinimalHost;
    let result = host.env_set("PATH", "/usr/bin");
    assert!(result.is_err(), "default env_set must reject");
}

#[test]
fn host_cwd_default_rejects() {
    let host = MinimalHost;
    let result = host.cwd();
    assert!(result.is_err(), "default cwd must reject");
}

#[test]
fn host_set_cwd_default_rejects() {
    let mut host = MinimalHost;
    let result = host.set_cwd("/tmp");
    assert!(result.is_err(), "default set_cwd must reject");
}

// ---------------------------------------------------------------------------
// Tensor shape-mismatch rejection (PRP-011)
// ---------------------------------------------------------------------------

#[test]
fn tensor_elementwise_shape_mismatch_rejected() {
    use crate::Value;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // [2,3] tensor (6 elements)
    let left_data: Vec<Value> = (1..=6).map(Value::Int).collect();
    let left_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        left_data,
        vec![2, 3],
    ))));

    // [6] tensor (6 elements — same count, different shape)
    let right_data: Vec<Value> = (10..=15).map(Value::Int).collect();
    let right_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        right_data,
        vec![6],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), left_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), right_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner.eval_runtime_call(&call, Span::default());
    assert!(result.is_err(), "[2,3] + [6] must reject on shape mismatch");
    let err = result.unwrap_err();
    // S6-C2: typed issue for non-same-shape / non-rank-extension pairs (not
    // the old generic "shape mismatch" issue). Assert issue id only —
    // hygiene forbids message/help prose asserts outside renderers.
    assert_eq!(
        err.issue, "runner_tensor_tensor_broadcast_unsupported",
        "error issue must name the broadcast rejection, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// PRP-077 — Backing-length mismatch rejection
// ---------------------------------------------------------------------------

#[test]
fn tensor_elementwise_backing_length_mismatch_rejected() {
    use crate::Value;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Both tensors have shape [2,2] but different backing lengths.
    let left_data: Vec<Value> = (1..=4).map(Value::Int).collect();
    let left_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        left_data,
        vec![2, 2],
    ))));

    // Same shape [2,2], but only 3 elements (should be 4).
    let right_data: Vec<Value> = (10..=12).map(Value::Int).collect();
    let right_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        right_data,
        vec![2, 2],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), left_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), right_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner.eval_runtime_call(&call, Span::default());
    assert!(
        result.is_err(),
        "[2,2] + [2,2] with mismatched backing length must reject"
    );
    let err = result.unwrap_err();
    assert!(
        err.issue.contains("backing_length_mismatch"),
        "error issue must mention backing-length mismatch, got: {err:?}"
    );
}

#[test]
fn tensor_elementwise_same_shape_succeeds() {
    use crate::Value;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // [2,3] + [2,3] → must succeed
    let left_data = vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
        Value::Int(4),
        Value::Int(5),
        Value::Int(6),
    ];
    let left_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        left_data,
        vec![2, 3],
    ))));

    let right_data = vec![
        Value::Int(10),
        Value::Int(20),
        Value::Int(30),
        Value::Int(40),
        Value::Int(50),
        Value::Int(60),
    ];
    let right_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        right_data,
        vec![2, 3],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), left_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), right_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner
        .eval_runtime_call(&call, Span::default())
        .expect("[2,3] + [2,3] must succeed");

    let tensor_value = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor, got {other:?}"),
    };
    let tensor = tensor_value.borrow();
    assert_eq!(tensor.shape, vec![2, 3]);
    assert_eq!(
        tensor.logical_values().expect("valid tensor layout").len(),
        6
    );
    // 1+10=11, 2+20=22, 3+30=33, 4+40=44, 5+50=55, 6+60=66
    let expected: [i64; 6] = [11, 22, 33, 44, 55, 66];
    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(
            tensor.logical_values().expect("valid tensor layout")[i],
            Value::Int(exp),
            "element {i} mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// Zero-width shape rejection (PRP-012)
// ---------------------------------------------------------------------------

#[test]
fn tensor_softmax_zero_width_rejected() {
    use crate::Value;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Shape [0] — zero-width rank-1 tensor
    let input = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![],
        vec![0],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSoftmax),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner.eval_runtime_call(&call, Span::default());
    assert!(
        result.is_err(),
        "softmax on shape [0] must reject zero-width dimension"
    );
    let err = result.unwrap_err();
    assert!(
        err.issue.contains("positive"),
        "error issue must mention positive dimensions, got: {err:?}"
    );
}

#[test]
fn tensor_softmax_rank2_zero_width_rejected() {
    use crate::Value;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Shape [2, 0] — rank-2 with zero last dim
    let input = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![],
        vec![2, 0],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), input);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSoftmax),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner.eval_runtime_call(&call, Span::default());
    assert!(
        result.is_err(),
        "softmax on shape [2,0] must reject zero-width dimension"
    );
}

#[test]
fn tensor_crux_entropia_zero_width_rejected() {
    use crate::Value;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Shape [3, 0] — rank-2 with zero last dim, both logits and targets
    let logits = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![],
        vec![3, 0],
    ))));
    let targets = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![],
        vec![3, 0],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), logits);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), targets);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCruxEntropia),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner.eval_runtime_call(&call, Span::default());
    assert!(
        result.is_err(),
        "cross-entropy on shape [3,0] must reject zero-width dimension"
    );
}

// ---------------------------------------------------------------------------
// Utility function unit tests: runner_issue
// ---------------------------------------------------------------------------

#[test]
fn runner_issue_simple_message() {
    let issue = runner_issue("hello world");
    assert_eq!(issue, "runner_hello_world");
}

#[test]
fn runner_issue_alphanumeric_only() {
    let issue = runner_issue("convert! fallback?");
    assert_eq!(issue, "runner_convert_fallback");
}

#[test]
fn runner_issue_special_chars_collapsed() {
    let issue = runner_issue("foo---bar___baz");
    assert_eq!(issue, "runner_foo_bar_baz");
}

#[test]
fn runner_issue_trailing_underscore_trimmed() {
    let issue = runner_issue("shape mismatch!!! ");
    assert_eq!(issue, "runner_shape_mismatch");
}

#[test]
fn runner_issue_empty_input() {
    let issue = runner_issue("");
    assert_eq!(issue, "runner");
}

// ---------------------------------------------------------------------------
// Utility function unit tests: regex_pattern
// ---------------------------------------------------------------------------

#[test]
fn regex_pattern_no_flags() {
    let result = regex_pattern("^foo$", None);
    assert_eq!(result, "^foo$");
}

#[test]
fn regex_pattern_with_flag() {
    let result = regex_pattern("^foo$", Some("i"));
    assert_eq!(result, "(?i)^foo$");
}

#[test]
fn regex_pattern_multiple_flags() {
    let result = regex_pattern("foo", Some("imsUx"));
    assert_eq!(result, "(?imsUx)foo");
}

#[test]
fn regex_pattern_rejects_unknown_flags() {
    let result = regex_pattern("foo", Some("izwx"));
    // Only 'i' should be kept; 'z', 'w', 'x' are unknown but 'x' is valid per spec
    assert_eq!(result, "(?ix)foo");
}

#[test]
fn regex_pattern_only_unknown_flags() {
    let result = regex_pattern("foo", Some("zwq"));
    assert_eq!(result, "foo");
}

// ---------------------------------------------------------------------------
// Utility function unit tests: constant_equals_value
// ---------------------------------------------------------------------------

#[test]
fn constant_equals_value_int_equal() {
    let konst = MirConstant::Int(42);
    let value = Value::Int(42);
    let interner = Interner::new();
    assert!(constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_int_not_equal() {
    let konst = MirConstant::Int(42);
    let value = Value::Int(7);
    let interner = Interner::new();
    assert!(!constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_uint_equal() {
    let konst = MirConstant::UInt(100);
    let value = Value::UInt(100);
    let interner = Interner::new();
    assert!(constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_uint_not_equal() {
    let konst = MirConstant::UInt(100);
    let value = Value::UInt(50);
    let interner = Interner::new();
    assert!(!constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_int_uint_cross_equal() {
    let konst = MirConstant::Int(5);
    let value = Value::UInt(5);
    let interner = Interner::new();
    assert!(constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_int_uint_cross_negative() {
    let konst = MirConstant::Int(-1);
    let value = Value::UInt(0);
    let interner = Interner::new();
    assert!(!constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_bool_equal() {
    let konst = MirConstant::Bool(true);
    let value = Value::Bool(true);
    let interner = Interner::new();
    assert!(constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_bool_not_equal() {
    let konst = MirConstant::Bool(true);
    let value = Value::Bool(false);
    let interner = Interner::new();
    assert!(!constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_nil() {
    let konst = MirConstant::Nil;
    let value = Value::Nil;
    let interner = Interner::new();
    assert!(constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_unit() {
    let konst = MirConstant::Unit;
    let value = Value::Unit;
    let interner = Interner::new();
    assert!(constant_equals_value(&konst, &value, &interner));
}

#[test]
fn constant_equals_value_type_mismatch() {
    let konst = MirConstant::Int(42);
    let value = Value::Bool(false);
    let interner = Interner::new();
    assert!(!constant_equals_value(&konst, &value, &interner));
}

// ---------------------------------------------------------------------------
// F-029: tensor ops width-checked arithmetic — red tests
// ---------------------------------------------------------------------------

/// F-029 verify: `TensorAdd` near u8 width max must overflow.
#[test]
fn f029_tensor_add_u8_overflow_rejected() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{NumericWidth, Primitive, Type, TypeTable};

    // Register numerus<u8> type so width checking applies.
    let mut type_table = TypeTable::new();
    let u8_ty = type_table.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8));
    let shape = type_table.rank0_shape();
    let tensor_ty = type_table.tensor_with_shape(u8_ty, shape);

    // 200 + 100 = 300 > u8::MAX=255 → must overflow.
    let left_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Int(200)],
        vec![1],
    ))));
    let right_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Int(100)],
        vec![1],
    ))));

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), left_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), right_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(tensor_ty),
    };

    let result = runner.eval_runtime_call(&call, Span::default());
    assert!(
        result.is_err(),
        "F-029: u8 overflow must be rejected, got {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.issue == crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP,
        "F-029: the element store must trap, got: {}",
        err.issue
    );
}

/// F-029 verify: `UInt` tensor elementwise add must succeed (not rejected).
#[test]
fn f029_uint_tensor_add_succeeds() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{NumericWidth, Primitive, Type, TypeTable};

    // Register numerus<u64> type for UInt tensors.
    let mut type_table = TypeTable::new();
    let u64_ty = type_table.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::U64));

    let left_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::UInt(100), Value::UInt(200)],
        vec![2],
    ))));
    let right_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::UInt(50), Value::UInt(25)],
        vec![2],
    ))));

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), left_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), right_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(u64_ty),
    };

    let result = runner
        .eval_runtime_call(&call, Span::default())
        .expect("F-029: UInt tensor add must succeed");
    let tensor_value = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor, got {other:?}"),
    };
    let tensor = tensor_value.borrow();
    assert_eq!(
        tensor.logical_values().expect("valid tensor layout").len(),
        2
    );
    // 100+50=150, 200+25=225
    assert_eq!(
        tensor.logical_values().expect("valid tensor layout")[0],
        Value::UInt(150)
    );
    assert_eq!(
        tensor.logical_values().expect("valid tensor layout")[1],
        Value::UInt(225)
    );
}

/// F-029 verify: `TensorDiv` by zero must be rejected.
#[test]
fn f029_tensor_div_zero_rejected() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{NumericWidth, Primitive, Type, TypeTable};

    // Register numerus<i32> type.
    let mut type_table = TypeTable::new();
    let i32_ty = type_table.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::I32));

    let left_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Int(42)],
        vec![1],
    ))));
    // Right tensor contains zero → division by zero.
    let right_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Int(0)],
        vec![1],
    ))));

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), left_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), right_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorDiv),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(i32_ty),
    };

    let result = runner.eval_runtime_call(&call, Span::default());
    assert!(
        result.is_err(),
        "F-029: division by zero must be rejected, got {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.issue.contains("division") || err.issue.contains("numerus"),
        "F-029: error must mention division, got: {}",
        err.issue
    );
}

/// F-029 verify: float `TensorDiv` by zero is still allowed (floats are width-agnostic).
#[test]
fn f029_float_tensor_div_zero_allowed() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{NumericWidth, Primitive, Type, TypeTable};

    let mut type_table = TypeTable::new();
    let f32_ty = type_table.intern(Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32));

    let left_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Float(42.0)],
        vec![1],
    ))));
    let right_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Float(0.0)],
        vec![1],
    ))));

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), left_tensor);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), right_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorDiv),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(f32_ty),
    };

    let result = runner
        .eval_runtime_call(&call, Span::default())
        .expect("F-029: float tensor div by zero must succeed (IEEE 754)");
    let tensor_value = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor, got {other:?}"),
    };
    let tensor = tensor_value.borrow();
    let val = match &tensor.logical_values().expect("valid tensor layout")[0] {
        Value::Float(f) => *f,
        other => panic!("expected float, got {other:?}"),
    };
    assert!(
        val.is_infinite(),
        "F-029: float division by zero should produce infinity, got {val}"
    );
}

/// Direct f32 materialization occupies f32 width: a bound literal, a mul-by-one
/// product, and a list element of the same decimal compare equal.
#[test]
fn f32_const_bind_matches_mul_by_one_and_list_element() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlockId,
        MirOperand, MirProgram, MirType, MirValidationContext, MirValueId,
    };
    use radix_types::{NumericWidth, Primitive, Type, TypeTable};

    let decimal = 0.70710678_f64;
    let f32_bits = f64::from(decimal as f32);
    assert_ne!(
        decimal, f32_bits,
        "probe decimal must keep extra f64 fraction so the store-width gap is observable"
    );

    let mut type_table = TypeTable::new();
    let f32_ty = type_table.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f64_ty = type_table.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let list_f32_ty = type_table.intern(Type::Array(f32_ty));
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    let span = Span::default();
    let expected = Value::Float(f32_bits);

    let literal = runner
        .eval_value(
            &MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(decimal))),
                ty: MirType::semantic(f32_ty),
                span,
            },
            span,
        )
        .expect("f32 const bind");
    let product = runner
        .eval_value(
            &MirValue {
                id: MirValueId(1),
                kind: MirValueKind::Binary {
                    op: MirBinOp::Mul,
                    lhs: MirOperand::Constant(MirConstant::Float(decimal)),
                    rhs: MirOperand::Constant(MirConstant::Float(1.0)),
                },
                ty: MirType::semantic(f32_ty),
                span,
            },
            span,
        )
        .expect("f32 mul-by-one");
    let wide = runner
        .eval_value(
            &MirValue {
                id: MirValueId(2),
                kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(decimal))),
                ty: MirType::semantic(f64_ty),
                span,
            },
            span,
        )
        .expect("f64 const bind");
    let list = runner
        .eval_construct(
            &MirAggregate {
                kind: MirAggregateKind::Array,
                ty: MirType::semantic(list_f32_ty),
                fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(
                    MirOperand::Constant(MirConstant::Float(decimal)),
                )]),
            },
            span,
        )
        .expect("f32 list construct");

    assert_eq!(literal, expected);
    assert_eq!(product, expected);
    assert!(values_equal(&literal, &product));
    assert_eq!(wide, Value::Float(decimal));
    assert!(!values_equal(&wide, &literal));
    let Value::Array(items) = list else {
        panic!("expected array, got {list:?}");
    };
    assert_eq!(items.borrow().as_slice(), [expected]);
}

// ---------------------------------------------------------------------------
// F-082…F-087: red-green tests
// ---------------------------------------------------------------------------

/// F-082: reversed intervallum bounds must be rejected at construction.
#[test]
fn f082_reversed_interval_bounds_rejected() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlockId,
        MirOperand, MirProgram, MirType, MirValidationContext, MirValueId,
    };
    use radix_types::{TypeId, TypeTable};

    let type_table = TypeTable::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // Store start=5, end=1 (reversed) as MIR values.
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), crate::Value::Int(5));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), crate::Value::Int(1));

    let aggregate = MirAggregate {
        kind: MirAggregateKind::Interval { inclusive: false },
        ty: MirType::semantic(TypeId(0)),
        fields: MirAggregateFields::Ordered(vec![
            MirAggregateItem::Operand(MirOperand::Value(MirValueId(0))),
            MirAggregateItem::Operand(MirOperand::Value(MirValueId(1))),
        ]),
    };

    let result = runner.eval_construct(&aggregate, radix_lexer::Span::default());
    assert!(
        result.is_err(),
        "F-082: reversed interval bounds must be rejected, got {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.issue.contains("monotonic"),
        "F-082: error issue must mention monotonic, got: {}",
        err.issue
    );
}

/// F-082: valid (monotonic) intervallum bounds must succeed.
#[test]
fn f082_valid_interval_bounds_accepted() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlockId,
        MirOperand, MirProgram, MirType, MirValidationContext, MirValueId,
    };
    use radix_types::{TypeId, TypeTable};

    let type_table = TypeTable::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), crate::Value::Int(1));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), crate::Value::Int(5));

    let aggregate = MirAggregate {
        kind: MirAggregateKind::Interval { inclusive: true },
        ty: MirType::semantic(TypeId(0)),
        fields: MirAggregateFields::Ordered(vec![
            MirAggregateItem::Operand(MirOperand::Value(MirValueId(0))),
            MirAggregateItem::Operand(MirOperand::Value(MirValueId(1))),
        ]),
    };

    let result = runner.eval_construct(&aggregate, radix_lexer::Span::default());
    assert!(
        result.is_ok(),
        "F-082: valid interval bounds must succeed, got {result:?}"
    );
    assert_eq!(
        result.unwrap(),
        crate::Value::Tuple(vec![
            crate::Value::Int(1),
            crate::Value::Int(5),
            crate::Value::Bool(true),
        ])
    );
}

/// F-088: interval clamp on a legal empty exclusive interval `[5, 5)` must
/// not panic — the clamp target has no valid output, so the source value is
/// returned unchanged (identity on the empty set).
#[test]
fn f088_interval_clamp_empty_exclusive_interval_no_panic() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{TypeId, TypeTable};

    let type_table = TypeTable::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // Clamp a value against the empty exclusive interval `[5, 5)`.
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), crate::Value::Int(3));
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(1),
        crate::Value::Tuple(vec![
            crate::Value::Int(5),
            crate::Value::Int(5),
            crate::Value::Bool(false),
        ]),
    );

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::IntervalClamp),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    assert_eq!(
        result.expect("empty exclusive clamp must not panic"),
        crate::Value::Int(3),
        "clamping to an empty exclusive interval returns the source unchanged"
    );
}

/// F-088 companion: a non-empty exclusive interval still clamps the source
/// into `[start, end)`.
#[test]
fn f088_interval_clamp_exclusive_interval_clamps_into_range() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{TypeId, TypeTable};

    let type_table = TypeTable::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // Clamp 10 into `[5, 10)` → 9 (exclusive upper bound), and 3 → 5 (floor).
    for (source, expected) in [(10, 9), (3, 5), (7, 7)] {
        runner
            .frames
            .last_mut()
            .unwrap()
            .values
            .insert(MirValueId(0), crate::Value::Int(source));
        runner.frames.last_mut().unwrap().values.insert(
            MirValueId(1),
            crate::Value::Tuple(vec![
                crate::Value::Int(5),
                crate::Value::Int(10),
                crate::Value::Bool(false),
            ]),
        );
        let call = MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(MirCollectionOp::IntervalClamp),
            args: vec![
                MirOperand::Value(MirValueId(0)),
                MirOperand::Value(MirValueId(1)),
            ],
            return_ty: MirType::semantic(TypeId(0)),
        };
        let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
        assert_eq!(
            result.expect("exclusive clamp must not panic"),
            crate::Value::Int(expected),
            "clamp of {source} into [5, 10)"
        );
    }
}

/// The integer Sum path applies the same width checks as other integer ops:
/// a `numerus<u8>` sum that overflows u8 rejects instead of wrapping.
#[test]
fn sum_numeric_u8_overflow_rejects() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{NumericWidth, Primitive, TypeTable};
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut type_table = TypeTable::new();
    let u8_ty = type_table.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        crate::Value::Array(Rc::new(RefCell::new(vec![
            crate::Value::Int(200),
            crate::Value::Int(100),
        ]))),
    );

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Sum),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(u8_ty),
    };

    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    let err = result.expect_err("u8 sum of 200 + 100 must overflow");
    assert!(
        err.issue.contains("overflow"),
        "expected a numerus overflow issue, got {err:?}"
    );
}

/// A sum that stays within the declared width succeeds, and a `numerus<u64>`
/// sum accumulates unsigned magnitudes without sign-encoding.
#[test]
fn sum_numeric_width_boundary_succeeds() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{NumericWidth, Primitive, TypeTable};
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut type_table = TypeTable::new();
    let u8_ty = type_table.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let u64_ty = type_table.sized_numeric(Primitive::Numerus, NumericWidth::U64);
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // u8 sum within the width: 50 + 100 = 150.
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        crate::Value::Array(Rc::new(RefCell::new(vec![
            crate::Value::Int(50),
            crate::Value::Int(100),
        ]))),
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Sum),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(u8_ty),
    };
    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    assert_eq!(
        result.expect("u8 sum within width must succeed"),
        crate::Value::Int(150)
    );

    // u64 sum keeps upper-half magnitudes as UInt: u64::MAX + 0.
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        crate::Value::Array(Rc::new(RefCell::new(vec![
            crate::Value::UInt(u64::MAX),
            crate::Value::UInt(0),
        ]))),
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Sum),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(u64_ty),
    };
    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    assert_eq!(
        result.expect("u64 sum must succeed"),
        crate::Value::UInt(u64::MAX),
        "upper-half u64 magnitudes must not sign-encode"
    );
}

/// The flat sum hands the exact total to the store: a partial sum that exceeds
/// `i64` mid-fold but fits at the end returns the exact `Int`, and a total that
/// exceeds `i64` only at the end returns the exact `UInt` an `inf` destination
/// converts (d11-6 ruling 17 residual: only a total beyond `u64::MAX` traps).
#[test]
fn sum_numeric_i64_exact_total_flows_to_the_store() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{NumericWidth, Primitive, TypeTable};
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut type_table = TypeTable::new();
    let i64_ty = type_table.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // Exceeds i64 mid-fold, fits at the end: MAX + MAX - MAX = MAX.
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        crate::Value::Array(Rc::new(RefCell::new(vec![
            crate::Value::Int(i64::MAX),
            crate::Value::Int(i64::MAX),
            crate::Value::Int(-i64::MAX),
        ]))),
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Sum),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(i64_ty),
    };
    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    assert_eq!(
        result.expect("a total that fits i64 at the end must succeed"),
        crate::Value::Int(i64::MAX)
    );

    // Exceeds i64 at the end: MAX + MAX = 2^64 - 2, still inside the exact
    // carrier and exactly what an `inf` store converts.
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        crate::Value::Array(Rc::new(RefCell::new(vec![
            crate::Value::Int(i64::MAX),
            crate::Value::Int(i64::MAX),
        ]))),
    );
    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    assert_eq!(
        result.expect("a total past i64 but inside u64 must stay exact, not trap"),
        crate::Value::UInt(18_446_744_073_709_551_614)
    );
}

/// `check_int_width` must not wrap `numerus<u64>` results above `i64::MAX` into
/// a negative i64 — the i64 carrier cannot encode them, so the add fails
/// closed exactly like every other out-of-width result.
#[test]
fn width_checked_add_u64_above_i64_max_fails_closed() {
    use radix_types::NumericWidth;

    let span = radix_lexer::Span::default();
    let at_boundary = crate::value::width_checked_add(i64::MAX - 1, 1, NumericWidth::U64, span);
    assert_eq!(
        at_boundary.expect("u64 add at i64::MAX boundary fits"),
        i64::MAX
    );
    let past_boundary = crate::value::width_checked_add(i64::MAX - 1, 2, NumericWidth::U64, span);
    let err = past_boundary.expect_err("u64 add past i64::MAX must fail closed, not wrap");
    assert!(
        err.issue.contains("overflow"),
        "expected a numerus overflow issue, got {err:?}"
    );
}

/// F-083: optional enum variant field projection must return Nil, not error.
#[test]
fn f083_optional_variant_field_nil_fallback() {
    use crate::host::BufferHost;
    use radix_hir::DefId;
    use radix_lexer::{Interner, Symbol};
    use radix_mir::{MirBlockId, MirProgram, MirValidationContext};
    use radix_types::TypeTable;
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::rc::Rc;

    let variant_def = DefId(100);
    let optional_field = Symbol(42);

    let type_table = TypeTable::new();
    let mut context = MirValidationContext::new(&type_table);
    context
        .optional_struct_fields
        .insert(variant_def, FxHashSet::from_iter([optional_field]));

    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // EnumVariant with empty field map — the optional field is not present.
    let variant_value =
        crate::Value::EnumVariant(variant_def, Rc::new(RefCell::new(FxHashMap::default())));

    let result = runner.project_variant_field(
        variant_value,
        variant_def,
        optional_field,
        radix_lexer::Span::default(),
    );

    assert!(
        result.is_ok(),
        "F-083: optional variant field must return Nil, not error, got {result:?}"
    );
    assert_eq!(
        result.unwrap(),
        crate::Value::Nil,
        "F-083: optional variant field must return Nil"
    );
}

/// F-083: missing non-optional variant field must still error.
#[test]
fn f083_required_variant_field_errors() {
    use crate::host::BufferHost;
    use radix_hir::DefId;
    use radix_lexer::{Interner, Symbol};
    use radix_mir::{MirBlockId, MirProgram, MirValidationContext};
    use radix_types::TypeTable;
    use rustc_hash::FxHashMap;
    use std::cell::RefCell;
    use std::rc::Rc;

    let variant_def = DefId(100);
    let missing_field = Symbol(99);

    let type_table = TypeTable::new();
    let context = MirValidationContext::new(&type_table);

    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    let variant_value =
        crate::Value::EnumVariant(variant_def, Rc::new(RefCell::new(FxHashMap::default())));

    let result = runner.project_variant_field(
        variant_value,
        variant_def,
        missing_field,
        radix_lexer::Span::default(),
    );

    assert!(
        result.is_err(),
        "F-083: non-optional variant field must error, got {result:?}"
    );
}

/// F-086: Octeti append on non-Place must return the modified Octeti.
#[test]
fn f086_octeti_append_non_place_returns_modified() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{TypeId, TypeTable};

    let type_table = TypeTable::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // Create empty octeti as a plain Value (not a Place).
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), crate::Value::Octeti(vec![]));
    // Byte to append: 42.
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), crate::Value::Int(42));

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Append),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    assert!(
        result.is_ok(),
        "F-086: octeti append must succeed, got {result:?}"
    );
    assert_eq!(
        result.unwrap(),
        crate::Value::Octeti(vec![42]),
        "F-086: octeti append must return modified octeti"
    );
}

/// F-087: inclusive range materialize must include `i64::MAX`.
#[test]
fn f087_inclusive_range_materialize_i64max() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{Type, TypeId, TypeTable};

    let mut type_table = TypeTable::new();
    let lista_ty = type_table.intern(Type::Array(TypeId(0)));

    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // Inclusive interval [i64::MAX, i64::MAX] — should produce one element.
    let interval = crate::Value::Tuple(vec![
        crate::Value::Int(i64::MAX),
        crate::Value::Int(i64::MAX),
        crate::Value::Bool(true),
    ]);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), interval);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::IntervalMaterialize),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(lista_ty),
    };

    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    assert!(
        result.is_ok(),
        "F-087: interval materialize must succeed, got {result:?}"
    );
    let array_value = result.unwrap();
    let Value::Array(array) = array_value else {
        panic!("F-087: expected array, got {array_value:?}");
    };
    let items = array.borrow();
    assert_eq!(items.len(), 1, "F-087: must produce exactly 1 element");
    assert_eq!(
        items[0],
        crate::Value::Int(i64::MAX),
        "F-087: element must be i64::MAX"
    );
}

/// F-087: exclusive range with `i64::MAX` must still work (materialize empty).
#[test]
fn f087_exclusive_range_i64max_empty() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{Type, TypeId, TypeTable};

    let mut type_table = TypeTable::new();
    let lista_ty = type_table.intern(Type::Array(TypeId(0)));

    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));

    // Exclusive interval [i64::MAX, i64::MAX) — empty.
    let interval = crate::Value::Tuple(vec![
        crate::Value::Int(i64::MAX),
        crate::Value::Int(i64::MAX),
        crate::Value::Bool(false),
    ]);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), interval);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::IntervalMaterialize),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(lista_ty),
    };

    let result = runner.eval_runtime_call(&call, radix_lexer::Span::default());
    assert!(
        result.is_ok(),
        "F-087: exclusive interval i64::MAX must succeed, got {result:?}"
    );
    let array_value = result.unwrap();
    let Value::Array(array) = array_value else {
        panic!("F-087: expected array, got {array_value:?}");
    };
    assert!(
        array.borrow().is_empty(),
        "F-087: exclusive [MAX, MAX) must be empty"
    );
}

// ── S6-C2: tensor-tensor rank-extension broadcast (the [8]-bias add) ─────

/// Helper: build a `[2,8]` left tensor, a `[8]` bias tensor, and a `TensorAdd`
/// runtime call, then evaluate it through the runner.
fn eval_tensor_add_bias(x_data: Vec<f64>, b_data: Vec<f64>) -> Value {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let x_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        x_data.into_iter().map(Value::Float).collect(),
        vec![2, 8],
    ))));
    let b_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        b_data.into_iter().map(Value::Float).collect(),
        vec![8],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    frame.values.insert(MirValueId(0), x_tensor);
    frame.values.insert(MirValueId(1), b_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect("tensor-tensor rank-extension add should succeed")
}

/// S6-C2: the `[2,8]` + `[8]` tensor-tensor add broadcasts the bias across
/// the leading axis (row b added to every row of x).
#[test]
fn tensor_add_rank_extension_broadcasts_8_bias_over_2_rows() {
    let result = eval_tensor_add_bias(
        vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0,
        ],
        vec![100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0],
    );

    let Value::Tensor(cell) = result else {
        panic!("expected tensor result");
    };
    let out = cell.borrow();
    assert_eq!(out.shape, vec![2, 8], "result keeps the higher-rank shape");
    let expected = [
        101.0, 202.0, 303.0, 404.0, 505.0, 606.0, 707.0, 808.0, // row 0 + b
        110.0, 220.0, 330.0, 440.0, 550.0, 660.0, 770.0, 880.0, // row 1 + b
    ];
    for (index, (actual, expected)) in out
        .logical_values()
        .expect("valid tensor layout")
        .iter()
        .zip(expected)
        .enumerate()
    {
        let Value::Float(actual) = actual else {
            panic!("expected float element at {index}");
        };
        assert!(
            (actual - expected).abs() < 1e-9,
            "element {index}: expected {expected}, got {actual}"
        );
    }
}

/// S6-C2: a tensor-tensor pair that is neither same-shape nor a valid
/// rank-extension broadcast fails closed with the typed diagnostic.
#[test]
fn tensor_add_invalid_broadcast_pair_fails_closed() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // Left `[2,8]`, right `[2]` — the bias dims are not the trailing dims.
    let x_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Float(1.0); 16],
        vec![2, 8],
    ))));
    let wrong_tensor = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Float(2.0); 2],
        vec![2],
    ))));

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    frame.values.insert(MirValueId(0), x_tensor);
    frame.values.insert(MirValueId(1), wrong_tensor);

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };

    let error = runner
        .eval_runtime_call(&call, radix_lexer::Span::default())
        .expect_err("an invalid broadcast pair must fail closed");
    assert_eq!(error.issue, "runner_tensor_tensor_broadcast_unsupported");
}

// ---------------------------------------------------------------------------
// CCI-1: TensorRmsNorm / TensorRope / TensorGather runner dispatch
// ---------------------------------------------------------------------------

/// Evaluate a single collection runtime call with the given input value
/// slots (mirrors the inline harness of the LayerNorm/Softmax smoke tests).
pub(super) fn evaluate_collection_call(
    call: &MirRuntimeCall,
    inputs: Vec<(MirValueId, Value)>,
) -> Result<Value, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    for (id, value) in inputs {
        runner.frames.last_mut().unwrap().values.insert(id, value);
    }
    runner.eval_runtime_call(call, Span::default())
}

fn tensor_value(data: Vec<Value>, shape: Vec<i64>) -> Value {
    use crate::value::TensorValue;
    Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(data, shape))))
}

fn assert_numeric_close(out: &[Value], expected: &[f64], tolerance: f64) {
    assert_eq!(out.len(), expected.len(), "output length mismatch");
    for (idx, (actual, expected_val)) in out.iter().zip(expected).enumerate() {
        let actual = match actual {
            Value::Float(f) => *f,
            Value::Int(i) => *i as f64,
            other => panic!("item {idx}: expected numeric, got {other:?}"),
        };
        let diff = (actual - expected_val).abs();
        assert!(
            diff < tolerance,
            "item {idx}: actual={actual}, expected={expected_val}, diff={diff:.6e}"
        );
    }
}

/// CCI-1: `rms_norm` follows the decoder oracle order (f64 sum of squares, f32
/// mean, f32 scale, fused x·scale·gamma) with no mean subtraction.
#[test]
fn tensor_rms_norm_rank2_with_gamma() {
    let input = tensor_value(
        vec![
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
            Value::Float(5.0),
            Value::Float(6.0),
        ],
        vec![2, 3],
    );
    let gamma = tensor_value(
        vec![Value::Float(2.0), Value::Float(1.0), Value::Float(0.5)],
        vec![3],
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRmsNorm),
        args: vec![
            MirOperand::Value(MirValueId(0)),               // input tensor
            MirOperand::Constant(MirConstant::Int(1)),      // axis (last)
            MirOperand::Constant(MirConstant::Float(1e-5)), // epsilon
            MirOperand::Value(MirValueId(1)),               // gamma peer
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let result =
        evaluate_collection_call(&call, vec![(MirValueId(0), input), (MirValueId(1), gamma)])
            .expect("rms_norm dispatch should succeed");
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![2, 3], "output shape must match input shape");
    // Row 0 [1,2,3]: sum_sq=14, mean=14/3, scale=1/sqrt(mean+1e-5)≈0.462906.
    // Row 1 [4,5,6]: sum_sq=77, mean=77/3, scale=1/sqrt(mean+1e-5)≈0.197367.
    // gamma = [2, 1, 0.5].
    let row0_scale = 1.0_f64 / (14.0_f64 / 3.0_f64 + 1e-5).sqrt();
    let row1_scale = 1.0_f64 / (77.0_f64 / 3.0_f64 + 1e-5).sqrt();
    assert_numeric_close(
        &out.logical_values().expect("valid tensor layout"),
        &[
            1.0 * row0_scale * 2.0,
            2.0 * row0_scale * 1.0,
            3.0 * row0_scale * 0.5,
            4.0 * row1_scale * 2.0,
            5.0 * row1_scale * 1.0,
            6.0 * row1_scale * 0.5,
        ],
        1e-4,
    );
}

/// CCI-1: `rope_norm` applies the supplied table's NORM consecutive-pair
/// rotation and preserves no trailing remainder when dim == width.
#[test]
fn tensor_rope_rank1_norm_pairs() {
    let input = tensor_value(
        vec![
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
        ],
        vec![4],
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRope),
        args: vec![
            MirOperand::Value(MirValueId(0)),          // input tensor
            MirOperand::Constant(MirConstant::Int(1)), // pos
            MirOperand::Constant(MirConstant::Int(4)), // dim
            MirOperand::Value(MirValueId(1)),          // [rows, dim/2, 3] table
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let table = tensor_value(
        vec![
            Value::Float(11.0),
            Value::Float(0.0),
            Value::Float(1.0),
            Value::Float(13.0),
            Value::Float(1.0),
            Value::Float(0.0),
        ],
        vec![1, 2, 3],
    );
    let result =
        evaluate_collection_call(&call, vec![(MirValueId(0), input), (MirValueId(1), table)])
            .expect("rope dispatch should succeed");
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![4], "output shape must match input shape");
    // The supplied table rotates the first pair by π/2 and leaves the second.
    let expected = [-2.0, 1.0, 3.0, 4.0];
    assert_numeric_close(
        &out.logical_values().expect("valid tensor layout"),
        &expected,
        1e-4,
    );
}

/// CCI-1: `rope_norm` preserves elements beyond `dim` on the last axis.
#[test]
fn tensor_rope_preserves_remainder_beyond_dim() {
    let input = tensor_value(
        vec![
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
            Value::Float(5.0),
            Value::Float(6.0),
        ],
        vec![6],
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRope),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Int(2)), // rotate only the first pair
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let table = tensor_value(
        vec![Value::Float(17.0), Value::Float(0.0), Value::Float(1.0)],
        vec![1, 1, 3],
    );
    let result =
        evaluate_collection_call(&call, vec![(MirValueId(0), input), (MirValueId(1), table)])
            .expect("rope dispatch should succeed");
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![6]);
    let expected = [-2.0, 1.0, 3.0, 4.0, 5.0, 6.0];
    assert_numeric_close(
        &out.logical_values().expect("valid tensor layout"),
        &expected,
        1e-4,
    );
}

/// Qwen/NeoX `rotate_half`: pairs `(x[k], x[k+dim/2])` with the same
/// `theta[k]` as llama pair-rotate. A unit vector at x[0] lands in x[2]
/// for dim=4 (not x[1]).
#[test]
fn tensor_rope_half_rank1_rotate_half_pairs() {
    let input = tensor_value(
        vec![
            Value::Float(1.0),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(0.0),
        ],
        vec![4],
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRopeHalf),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Int(4)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let table = tensor_value(
        vec![
            Value::Float(19.0),
            Value::Float(0.0),
            Value::Float(1.0),
            Value::Float(23.0),
            Value::Float(1.0),
            Value::Float(0.0),
        ],
        vec![1, 2, 3],
    );
    let result =
        evaluate_collection_call(&call, vec![(MirValueId(0), input), (MirValueId(1), table)])
            .expect("rope_half dispatch should succeed");
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![4]);
    let expected = [0.0, 0.0, 1.0, 0.0];
    assert_numeric_close(
        &out.logical_values().expect("valid tensor layout"),
        &expected,
        1e-4,
    );
}

/// CCI-1: gather selects table rows by id, preserving repeated/out-of-order
/// ids, and fails closed on an out-of-range id.
#[test]
fn tensor_gather_selects_rows_and_fails_closed_on_bad_id() {
    // Table [3,2]: rows [1,2], [3,4], [5,6].
    let table = tensor_value(
        vec![
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
            Value::Float(5.0),
            Value::Float(6.0),
        ],
        vec![3, 2],
    );
    // K7c: the ids are a rank-1 `tensor<u32, [n]>`, never a list.
    let ids = tensor_value(vec![Value::Int(2), Value::Int(0), Value::Int(2)], vec![3]);
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGather),
        args: vec![
            MirOperand::Value(MirValueId(0)), // table
            MirOperand::Value(MirValueId(1)), // ids
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let result = evaluate_collection_call(
        &call,
        vec![(MirValueId(0), table.clone()), (MirValueId(1), ids)],
    )
    .expect("gather dispatch should succeed");
    let output = match result {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected tensor output, got {other:?}"),
    };
    let out = output.borrow();
    assert_eq!(out.shape, vec![3, 2], "result shape is [T, D]");
    assert_numeric_close(
        &out.logical_values().expect("valid tensor layout"),
        &[5.0, 6.0, 1.0, 2.0, 5.0, 6.0],
        1e-9,
    );

    // Out-of-range id (>= table rows) fails closed.
    let bad_ids = tensor_value(vec![Value::Int(0), Value::Int(3)], vec![2]);
    let bad_call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGather),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let err = evaluate_collection_call(
        &bad_call,
        vec![(MirValueId(0), table.clone()), (MirValueId(1), bad_ids)],
    )
    .expect_err("out-of-range id must fail closed");
    assert!(err.issue.contains("out_of_range"), "got: {err:?}");

    // A list of ids (the retired operand form) and a rank-2 ids tensor fail
    // closed: the ids are exactly a rank-1 tensor.
    for bad in [
        Value::Array(Rc::new(RefCell::new(vec![Value::Int(0), Value::Int(1)]))),
        tensor_value(vec![Value::Int(0), Value::Int(1)], vec![2, 1]),
    ] {
        let call = MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGather),
            args: vec![
                MirOperand::Value(MirValueId(0)),
                MirOperand::Value(MirValueId(1)),
            ],
            return_ty: MirType::semantic(TypeId(0)),
        };
        evaluate_collection_call(
            &call,
            vec![(MirValueId(0), table.clone()), (MirValueId(1), bad)],
        )
        .expect_err("a non-rank-1-tensor ids operand must fail closed");
    }
}

#[test]
fn tensor_gather_rejects_a_table_above_rank_two() {
    // Rows are cut at the second extent, so a rank-3 table would silently
    // lose its trailing extent instead of failing closed.
    let table = tensor_value(vec![Value::Float(1.0); 8], vec![2, 2, 2]);
    let ids = tensor_value(vec![Value::Int(1)], vec![1]);
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGather),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    evaluate_collection_call(&call, vec![(MirValueId(0), table), (MirValueId(1), ids)])
        .expect_err("a rank-3 gather table must fail closed");
}

// ---------------------------------------------------------------------------
// Gradient-handle round trip (S5-U1): Create → Accumulate → Read → Zero
// ---------------------------------------------------------------------------

#[test]
fn runner_gradient_handle_create_accumulate_read_zero_round_trip() {
    use crate::Value;
    use crate::host::BufferHost;
    use crate::value::TensorValue;
    use radix_lexer::Interner;
    use radix_mir::MirDiagnosticKind;
    use radix_mir::MirGradientOp;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    // --- 1. Build the round-trip program state ---
    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let span = radix_lexer::Span::default();

    // Slot 0: shape [2]; slot 1: rank 1; slot 2: element kind (F32).
    runner.frames.last_mut().unwrap().values.extend([
        (
            MirValueId(0),
            Value::Array(Rc::new(RefCell::new(vec![Value::Int(2)]))),
        ),
        (MirValueId(1), Value::Int(1)),
        (MirValueId(2), Value::Int(1)),
        (
            MirValueId(3),
            Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
                vec![Value::Float(1.0), Value::Float(2.0)],
                vec![2],
            )))),
        ),
        (
            MirValueId(4),
            Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
                vec![Value::Float(3.0), Value::Float(4.0)],
                vec![2],
            )))),
        ),
    ]);

    let shape = MirOperand::Value(MirValueId(0));
    let rank = MirOperand::Value(MirValueId(1));

    // --- 2. Create → opaque handle ---
    let create = MirRuntimeCall {
        intrinsic: MirIntrinsic::Gradient(MirGradientOp::Create),
        args: vec![
            shape.clone(),
            rank.clone(),
            MirOperand::Value(MirValueId(2)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let handle = runner
        .eval_runtime_call(&create, span)
        .expect("gradient create should succeed");
    let Value::GradientHandle(handle_id) = handle else {
        panic!("gradient create must return an opaque handle, got {handle:?}");
    };
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(5), Value::GradientHandle(handle_id));

    // nota handle → "gradient-handle#0"
    let nota_handle = MirRuntimeCall {
        intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
        args: vec![MirOperand::Value(MirValueId(5))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    runner
        .eval_runtime_call(&nota_handle, span)
        .expect("nota handle should succeed");

    // --- 3. Accumulate [1.0, 2.0] then [3.0, 4.0] ---
    for data_slot in [MirValueId(3), MirValueId(4)] {
        let accumulate = MirRuntimeCall {
            intrinsic: MirIntrinsic::Gradient(MirGradientOp::Accumulate),
            args: vec![
                MirOperand::Value(MirValueId(5)),
                MirOperand::Value(data_slot),
                shape.clone(),
                rank.clone(),
                MirOperand::Constant(MirConstant::Int(1)),
            ],
            return_ty: MirType::semantic(TypeId(0)),
        };
        runner
            .eval_runtime_call(&accumulate, span)
            .expect("gradient accumulate should succeed");
    }

    // --- 4. Read → nota → "[4.0, 6.0]" ---
    let read = MirRuntimeCall {
        intrinsic: MirIntrinsic::Gradient(MirGradientOp::Read),
        args: vec![MirOperand::Value(MirValueId(5))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let read_back = runner
        .eval_runtime_call(&read, span)
        .expect("gradient read should succeed");
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(6), read_back);
    let nota_read = MirRuntimeCall {
        intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
        args: vec![MirOperand::Value(MirValueId(6))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    runner
        .eval_runtime_call(&nota_read, span)
        .expect("nota read-back should succeed");

    // --- 5. Zero → read again → nota → "[0.0, 0.0]" ---
    let zero = MirRuntimeCall {
        intrinsic: MirIntrinsic::Gradient(MirGradientOp::Zero),
        args: vec![MirOperand::Value(MirValueId(5))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    runner
        .eval_runtime_call(&zero, span)
        .expect("gradient zero should succeed");
    let zeroed = runner
        .eval_runtime_call(&read, span)
        .expect("gradient read after zero should succeed");
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(7), zeroed);
    let nota_zeroed = MirRuntimeCall {
        intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
        args: vec![MirOperand::Value(MirValueId(7))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    runner
        .eval_runtime_call(&nota_zeroed, span)
        .expect("nota zeroed read-back should succeed");

    // --- 6. Exact stdout oracle ---
    assert_eq!(
        host.stdout_lines,
        vec![
            "gradient-handle#0".to_owned(),
            "[4.0, 6.0]".to_owned(),
            "[0.0, 0.0]".to_owned(),
        ]
    );
}

#[test]
fn runner_gradient_operations_fail_closed_on_unknown_or_foreign_handle() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirGradientOp;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let span = radix_lexer::Span::default();

    // Reading/zeroing an unknown handle must fail closed.
    let read = MirRuntimeCall {
        intrinsic: MirIntrinsic::Gradient(MirGradientOp::Read),
        args: vec![MirOperand::Constant(MirConstant::Int(99))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let err = runner
        .eval_runtime_call(&read, span)
        .expect_err("unknown gradient handle must fail closed");
    assert!(
        err.issue
            .starts_with("runner_gradient_operation_requires_opaque_handle"),
        "expected opaque-handle issue, got: {err:?}"
    );

    // A non-handle value (a numerus) is not a valid opaque handle.
    let zero = MirRuntimeCall {
        intrinsic: MirIntrinsic::Gradient(MirGradientOp::Zero),
        args: vec![MirOperand::Constant(MirConstant::Int(1))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let err = runner
        .eval_runtime_call(&zero, span)
        .expect_err("foreign scalar handle must fail closed");
    assert!(
        err.issue
            .starts_with("runner_gradient_operation_requires_opaque_handle"),
        "expected opaque-handle issue, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// `est <type>` variant-tag dispatch (S5-U3): positive/negative type checks
// ---------------------------------------------------------------------------

#[test]
fn runner_type_check_dispatches_on_runtime_variant_tag() {
    use crate::Value;
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirType;
    use radix_mir::MirTypeCheckOp;
    use radix_mir::MirValidationContext;
    use radix_types::Primitive;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let span = radix_lexer::Span::default();

    let textus = type_table.primitive(Primitive::Textus);
    let numerus = type_table.primitive(Primitive::Numerus);

    // Slot 0: a textus; slot 1: a numerus.
    runner.frames.last_mut().unwrap().values.extend([
        (MirValueId(0), Value::Str("faber".to_owned())),
        (MirValueId(1), Value::Int(42)),
    ]);

    let type_check = |positive: bool, ty: TypeId, slot: u32| MirRuntimeCall {
        intrinsic: MirIntrinsic::TypeCheck(MirTypeCheckOp {
            positive,
            ty: MirType::semantic(ty),
        }),
        args: vec![MirOperand::Value(MirValueId(slot))],
        return_ty: MirType::semantic(textus),
    };

    // Positive checks against the runtime tag.
    assert_eq!(
        runner
            .eval_runtime_call(&type_check(true, textus, 0), span)
            .expect("textus is textus"),
        Value::Bool(true)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&type_check(true, numerus, 0), span)
            .expect("textus is not numerus"),
        Value::Bool(false)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&type_check(true, numerus, 1), span)
            .expect("numerus is numerus"),
        Value::Bool(true)
    );

    // Negative checks (`non est`) invert the tag result.
    assert_eq!(
        runner
            .eval_runtime_call(&type_check(false, textus, 1), span)
            .expect("numerus is not textus"),
        Value::Bool(true)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&type_check(false, numerus, 1), span)
            .expect("numerus is not numerus"),
        Value::Bool(false)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&type_check(false, textus, 0), span)
            .expect("textus is not textus"),
        Value::Bool(false)
    );
}

#[test]
fn runner_type_check_dispatches_nominal_struct_and_enum_tags() {
    use crate::Value;
    use crate::host::BufferHost;
    use radix_hir::DefId;
    use radix_lexer::Interner;
    use radix_mir::MirType;
    use radix_mir::MirTypeCheckOp;
    use radix_mir::MirValidationContext;
    use radix_types::{Primitive, Type, TypeTable};

    let mut host = BufferHost::default();
    let mut type_table = TypeTable::default();
    let interner = Interner::new();
    let struct_def = DefId(10);
    let enum_def = DefId(20);
    let variant_def = DefId(21);
    let bivalens = type_table.primitive(Primitive::Bivalens);
    let struct_ty = type_table.intern(Type::Struct(struct_def));
    let enum_ty = type_table.intern(Type::Enum(enum_def));
    let mut context = MirValidationContext::new(&type_table);
    context.enum_variants.insert(enum_def, vec![variant_def]);
    context.variant_parents.insert(variant_def, enum_def);
    context.rebuild_variant_positions();
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner.frames.last_mut().unwrap().values.extend([
        (
            MirValueId(2),
            Value::Struct(struct_def, Rc::new(RefCell::new(FxHashMap::default()))),
        ),
        (
            MirValueId(3),
            Value::EnumVariant(variant_def, Rc::new(RefCell::new(FxHashMap::default()))),
        ),
    ]);

    let type_check = |ty: TypeId, slot: u32| MirRuntimeCall {
        intrinsic: MirIntrinsic::TypeCheck(MirTypeCheckOp {
            positive: true,
            ty: MirType::semantic(ty),
        }),
        args: vec![MirOperand::Value(MirValueId(slot))],
        return_ty: MirType::semantic(bivalens),
    };

    assert_eq!(
        runner
            .eval_runtime_call(&type_check(struct_ty, 2), Span::default())
            .expect("struct tag check"),
        Value::Bool(true)
    );
    assert_eq!(
        runner
            .eval_runtime_call(&type_check(enum_ty, 3), Span::default())
            .expect("enum variant tag check"),
        Value::Bool(true)
    );
}

#[test]
fn runner_type_check_accepts_union_member_tags() {
    use crate::Value;
    use crate::host::BufferHost;
    use radix_hir::DefId;
    use radix_lexer::Interner;
    use radix_mir::{MirType, MirTypeCheckOp, MirValidationContext};
    use radix_types::{Primitive, Type, TypeTable};

    let mut host = BufferHost::default();
    let mut type_table = TypeTable::default();
    let interner = Interner::new();
    let struct_def = DefId(30);
    let numerus = type_table.primitive(Primitive::Numerus);
    let struct_ty = type_table.intern(Type::Struct(struct_def));
    let union_ty = type_table.intern(Type::Union(vec![numerus, struct_ty]));
    let bivalens = type_table.primitive(Primitive::Bivalens);
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        Value::Struct(struct_def, Rc::new(RefCell::new(FxHashMap::default()))),
    );

    let check = MirRuntimeCall {
        intrinsic: MirIntrinsic::TypeCheck(MirTypeCheckOp {
            positive: true,
            ty: MirType::semantic(union_ty),
        }),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(bivalens),
    };
    assert_eq!(
        runner
            .eval_runtime_call(&check, Span::default())
            .expect("union member tag check"),
        Value::Bool(true)
    );
}

// ---------------------------------------------------------------------------
// Matrix display (runner display contract: nested rows, matching the
// compiled host route goldens)
// ---------------------------------------------------------------------------

#[test]
fn matrix_display_emits_nested_rows() {
    use crate::Value;
    use crate::value::MatrixValue;
    use radix_lexer::Interner;

    let matrix = Value::Matrix(Rc::new(RefCell::new(MatrixValue {
        rows: 3,
        columns: 2,
        data: vec![
            Value::Float(16.0),
            Value::Float(25.0),
            Value::Float(64.0),
            Value::Float(100.0),
            Value::Float(144.0),
            Value::Float(225.0),
        ],
    })));

    let interner = Interner::new();
    let text = matrix
        .display(
            &interner,
            FieldOrder::new(&FxHashMap::default(), &FxHashMap::default()),
            Span::default(),
        )
        .expect("matrix display must succeed");
    assert_eq!(text, "[[16.0, 25.0], [64.0, 100.0], [144.0, 225.0]]");
}

#[test]
fn matrix_display_rows_split_on_column_count() {
    use crate::Value;
    use crate::value::MatrixValue;
    use radix_lexer::Interner;

    let matrix = Value::Matrix(Rc::new(RefCell::new(MatrixValue {
        rows: 2,
        columns: 3,
        data: vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
            Value::Int(5),
            Value::Int(6),
        ],
    })));

    let interner = Interner::new();
    let text = matrix
        .display(
            &interner,
            FieldOrder::new(&FxHashMap::default(), &FxHashMap::default()),
            Span::default(),
        )
        .expect("matrix display must succeed");
    assert_eq!(text, "[[1, 2, 3], [4, 5, 6]]");
}

// ---------------------------------------------------------------------------
// tensor_matmul correctness + measurement (perf-A F1)
// ---------------------------------------------------------------------------

fn eval_tensor_matmul(left: Value, right: Value) -> Result<Value, RunnerError> {
    evaluate_collection_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
            args: vec![
                MirOperand::Value(MirValueId(0)),
                MirOperand::Value(MirValueId(1)),
            ],
            return_ty: MirType::semantic(TypeId(0)),
        },
        vec![(MirValueId(0), left), (MirValueId(1), right)],
    )
}

fn eval_tensor_view_op(
    op: MirCollectionOp,
    args: Vec<MirOperand>,
    inputs: Vec<(MirValueId, Value)>,
) -> Result<Value, RunnerError> {
    evaluate_collection_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(op),
            args,
            return_ty: MirType::semantic(TypeId(0)),
        },
        inputs,
    )
}

fn dense_float_tensor(rows: i64, cols: i64, fill: impl Fn(i64, i64) -> f64) -> Value {
    let mut data = Vec::with_capacity((rows * cols) as usize);
    for i in 0..rows {
        for j in 0..cols {
            data.push(Value::Float(fill(i, j)));
        }
    }
    tensor_value(data, vec![rows, cols])
}

#[test]
fn tensor_matmul_float_2x3_by_3x4() {
    // Same contraction as corpus-product/tensor-package/fmir-matmul.
    let left = dense_float_tensor(2, 3, |i, j| (i * 3 + j + 1) as f64);
    let right = dense_float_tensor(3, 4, |i, j| (i * 4 + j + 1) as f64);
    let result = eval_tensor_matmul(left, right).expect("float matmul");
    let Value::Tensor(cell) = result else {
        panic!("expected tensor");
    };
    let out = cell.borrow();
    assert_eq!(out.shape, vec![2, 4]);
    assert_numeric_close(
        &out.logical_values().expect("valid tensor layout"),
        &[38.0, 44.0, 50.0, 56.0, 83.0, 98.0, 113.0, 128.0],
        1e-9,
    );
}

#[test]
fn tensor_matmul_float_2x3_by_rank1_vector() {
    let left = dense_float_tensor(2, 3, |i, j| (i * 3 + j + 1) as f64);
    let right = tensor_value(
        vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)],
        vec![3],
    );
    let result = eval_tensor_matmul(left, right).expect("rank-1 RHS GEMV");
    let Value::Tensor(cell) = result else {
        panic!("expected tensor");
    };
    let out = cell.borrow();
    assert_eq!(out.shape, vec![2]);
    assert_numeric_close(
        &out.logical_values().expect("valid tensor layout"),
        &[14.0, 32.0],
        1e-9,
    );
}

#[test]
fn tensor_batched_matmul_rank3_and_rank4_prefix_rhs() {
    let rank3_left = tensor_value(
        (1..=8)
            .map(|value| Value::Float(f64::from(value)))
            .collect(),
        vec![2, 2, 2],
    );
    let rank2_rhs = tensor_value(vec![Value::Float(1.0), Value::Float(1.0)], vec![2, 1]);
    let rank3 = eval_tensor_matmul(rank3_left, rank2_rhs).expect("rank-3 batched matmul");
    let Value::Tensor(rank3) = rank3 else {
        panic!("expected rank-3 tensor result");
    };
    let rank3 = rank3.borrow();
    assert_eq!(rank3.shape, vec![2, 2, 1]);
    assert_numeric_close(
        &rank3.logical_values().expect("valid tensor layout"),
        &[3.0, 7.0, 11.0, 15.0],
        1e-9,
    );

    // The rank-3 RHS batch `[2]` is a prefix of the rank-4 receiver's
    // `[2,2]` batch axes, so each RHS matrix repeats across the last batch.
    let rank4_left = tensor_value(
        (1..=8)
            .map(|value| Value::Float(f64::from(value)))
            .collect(),
        vec![2, 2, 1, 2],
    );
    let rank3_rhs = tensor_value(
        vec![
            Value::Float(1.0),
            Value::Float(1.0),
            Value::Float(1.0),
            Value::Float(1.0),
        ],
        vec![2, 2, 1],
    );
    let rank4 = eval_tensor_matmul(rank4_left, rank3_rhs).expect("rank-4 batched matmul");
    let Value::Tensor(rank4) = rank4 else {
        panic!("expected rank-4 tensor result");
    };
    let rank4 = rank4.borrow();
    assert_eq!(rank4.shape, vec![2, 2, 1, 1]);
    assert_numeric_close(
        &rank4.logical_values().expect("valid tensor layout"),
        &[3.0, 7.0, 11.0, 15.0],
        1e-9,
    );
}

#[test]
fn tensor_view_composition_keeps_storage_until_materialize() {
    let base = tensor_value((0..12).map(Value::Int).collect(), vec![4, 3]);
    let sliced = eval_tensor_view_op(
        MirCollectionOp::TensorSlice,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Int(4)),
            MirOperand::Constant(MirConstant::Int(2)),
        ],
        vec![(MirValueId(0), base.clone())],
    )
    .expect("strided slice view");
    let transposed = eval_tensor_view_op(
        MirCollectionOp::TensorTranspose,
        vec![MirOperand::Value(MirValueId(0))],
        vec![(MirValueId(0), sliced)],
    )
    .expect("transpose view");
    let Value::Tensor(base_cell) = &base else {
        unreachable!();
    };
    let Value::Tensor(transposed_cell) = &transposed else {
        panic!("expected tensor view");
    };
    let base_tensor = base_cell.borrow();
    let transposed_tensor = transposed_cell.borrow();
    assert!(Arc::ptr_eq(&base_tensor.data, &transposed_tensor.data));
    assert_eq!(transposed_tensor.shape, vec![3, 2]);
    assert_eq!(transposed_tensor.strides, vec![1, 6]);
    assert_eq!(transposed_tensor.offset, 3);
    assert_eq!(
        transposed_tensor
            .logical_values()
            .expect("valid view layout"),
        vec![
            Value::Int(3),
            Value::Int(9),
            Value::Int(4),
            Value::Int(10),
            Value::Int(5),
            Value::Int(11),
        ]
    );
    drop(transposed_tensor);
    drop(base_tensor);

    let materialized = eval_tensor_view_op(
        MirCollectionOp::TensorMaterialize,
        vec![MirOperand::Value(MirValueId(0))],
        vec![(MirValueId(0), transposed)],
    )
    .expect("explicit materialize");
    let (Value::Tensor(base_cell), Value::Tensor(materialized_cell)) = (&base, &materialized)
    else {
        unreachable!();
    };
    assert!(!Arc::ptr_eq(
        &base_cell.borrow().data,
        &materialized_cell.borrow().data
    ));
    base_cell
        .borrow()
        .set_value_at(4, Value::Int(400))
        .expect("base write");
    assert_eq!(
        materialized_cell
            .borrow()
            .logical_values()
            .expect("valid materialized layout")[2],
        Value::Int(4)
    );
}

#[test]
fn tensor_expand_uses_shared_storage_and_rightmost_exact_axis_tie() {
    let source = tensor_value(
        vec![Value::Int(10), Value::Int(20), Value::Int(30)],
        vec![1, 3],
    );
    let shape = Value::Array(Rc::new(RefCell::new(vec![
        Value::Int(1),
        Value::Int(1),
        Value::Int(3),
    ])));
    let expanded = eval_tensor_view_op(
        MirCollectionOp::TensorExpand,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        vec![(MirValueId(0), source.clone()), (MirValueId(1), shape)],
    )
    .expect("expanded view");
    let (Value::Tensor(source), Value::Tensor(expanded)) = (&source, &expanded) else {
        unreachable!();
    };
    let source = source.borrow();
    let expanded = expanded.borrow();
    assert!(Arc::ptr_eq(&source.data, &expanded.data));
    assert_eq!(expanded.shape, vec![1, 1, 3]);
    assert_eq!(expanded.strides, vec![0, 3, 1]);
    assert_eq!(
        expanded.logical_values().expect("valid expanded layout"),
        vec![Value::Int(10), Value::Int(20), Value::Int(30)]
    );
}

#[test]
fn tensor_rank_zero_and_empty_axis_logical_reads_are_distinct() {
    let scalar = crate::value::TensorValue::contiguous(vec![Value::Int(42)], vec![]);
    assert_eq!(scalar.logical_len(), Some(1));
    assert_eq!(scalar.value_at(0), Some(Value::Int(42)));
    assert_eq!(scalar.logical_values(), Some(vec![Value::Int(42)]));

    let seed = crate::value::TensorValue::contiguous(vec![], vec![]);
    assert_eq!(seed.logical_len(), Some(0));
    assert_eq!(seed.logical_values(), Some(Vec::new()));

    let empty_axis = crate::value::TensorValue::contiguous(vec![], vec![2, 0, 3]);
    assert_eq!(empty_axis.logical_len(), Some(0));
    assert_eq!(empty_axis.logical_values(), Some(Vec::new()));

    assert!(crate::value::TensorValue::try_contiguous(vec![], vec![i64::MAX, 3]).is_none());
    assert!(crate::value::TensorValue::try_contiguous_strides(&[2, -1]).is_none());
}

#[test]
fn tensor_shift_limes_composes_with_views_without_materializing() {
    use radix_mir::{MirBuiltinEdgePolicy, MirEdgePolicy};

    let base = tensor_value(
        vec![Value::Int(10), Value::Int(20), Value::Int(30)],
        vec![3],
    );
    let offsets = Value::Vector(Rc::new(RefCell::new(vec![Value::Int(1)])));
    let shifted = eval_tensor_view_op(
        MirCollectionOp::TensorShift,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        vec![(MirValueId(0), base.clone()), (MirValueId(1), offsets)],
    )
    .expect("shift view");
    let wrapped = evaluate_collection_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::TensorLimes(MirEdgePolicy::Builtin(
                MirBuiltinEdgePolicy::Wrap,
            )),
            args: vec![MirOperand::Value(MirValueId(0))],
            return_ty: MirType::semantic(TypeId(0)),
        },
        vec![(MirValueId(0), shifted)],
    )
    .expect("resolve wrap policy");
    let shape = Value::Array(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(3)])));
    let reshaped = eval_tensor_view_op(
        MirCollectionOp::TensorReshape,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        vec![(MirValueId(0), wrapped), (MirValueId(1), shape)],
    )
    .expect("reshape view");
    let transposed = eval_tensor_view_op(
        MirCollectionOp::TensorTranspose,
        vec![MirOperand::Value(MirValueId(0))],
        vec![(MirValueId(0), reshaped)],
    )
    .expect("transpose view");

    let (Value::Tensor(base_cell), Value::Tensor(transposed_cell)) = (&base, &transposed) else {
        unreachable!();
    };
    {
        let base_tensor = base_cell.borrow();
        let transposed_tensor = transposed_cell.borrow();
        assert!(Arc::ptr_eq(&base_tensor.data, &transposed_tensor.data));
        assert_eq!(transposed_tensor.shape, vec![3, 1]);
        assert_eq!(
            transposed_tensor
                .logical_values()
                .expect("resolved lazy view"),
            vec![Value::Int(20), Value::Int(30), Value::Int(10)]
        );
    }

    let materialized = eval_tensor_view_op(
        MirCollectionOp::TensorMaterialize,
        vec![MirOperand::Value(MirValueId(0))],
        vec![(MirValueId(0), transposed)],
    )
    .expect("explicit materialize");
    let Value::Tensor(materialized_cell) = materialized else {
        panic!("expected a tensor result");
    };
    {
        let base_tensor = base_cell.borrow();
        let materialized_tensor = materialized_cell.borrow();
        assert!(!Arc::ptr_eq(&base_tensor.data, &materialized_tensor.data));
        base_tensor
            .set_value_at(0, Value::Int(100))
            .expect("base storage write");
        assert_eq!(
            materialized_tensor
                .logical_values()
                .expect("materialized layout"),
            vec![Value::Int(20), Value::Int(30), Value::Int(10)]
        );
    }
}

#[test]
fn tensor_shift_accepts_integer_array_offsets_and_keeps_validation() {
    let base = tensor_value(
        vec![Value::Int(10), Value::Int(20), Value::Int(30)],
        vec![3],
    );
    let offsets = Value::Array(Rc::new(RefCell::new(vec![Value::Int(1)])));
    let shifted = eval_tensor_view_op(
        MirCollectionOp::TensorShift,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        vec![(MirValueId(0), base.clone()), (MirValueId(1), offsets)],
    )
    .expect("integer array offsets form a lazy shift view");
    let (Value::Tensor(base_cell), Value::Tensor(shifted_cell)) = (&base, &shifted) else {
        panic!("expected tensor values");
    };
    assert!(Arc::ptr_eq(
        &base_cell.borrow().data,
        &shifted_cell.borrow().data
    ));
    assert_eq!(
        shifted_cell
            .borrow()
            .logical_values()
            .expect("optional shifted view"),
        vec![Value::Int(20), Value::Int(30), Value::Nil]
    );

    let wrong_rank = Value::Array(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])));
    let error = eval_tensor_view_op(
        MirCollectionOp::TensorShift,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        vec![(MirValueId(0), base.clone()), (MirValueId(1), wrong_rank)],
    )
    .expect_err("offset count must match tensor rank");
    assert_eq!(error.code, RUNNER_CODE_INTERNAL);
    assert_eq!(
        error.args,
        vec![(
            "detail".to_owned(),
            "tensor shift offset rank differs from receiver rank".to_owned()
        )]
    );

    let wrong_element = Value::Array(Rc::new(RefCell::new(vec![Value::Float(1.0)])));
    let error = eval_tensor_view_op(
        MirCollectionOp::TensorShift,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        vec![(MirValueId(0), base), (MirValueId(1), wrong_element)],
    )
    .expect_err("offset elements must be integers");
    assert_eq!(error.code, RUNNER_CODE_INTERNAL);
    assert_eq!(
        error.args,
        vec![(
            "detail".to_owned(),
            "tensor shift offset is not numerus: Float(1.0)".to_owned()
        )]
    );
}

#[test]
fn tensor_shift_coalesce_consumes_optional_holes_before_materialize() {
    let base = tensor_value(
        vec![Value::Int(10), Value::Int(20), Value::Int(30)],
        vec![3],
    );
    let offsets = Value::Vector(Rc::new(RefCell::new(vec![Value::Int(1)])));
    let shifted = eval_tensor_view_op(
        MirCollectionOp::TensorShift,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        vec![(MirValueId(0), base), (MirValueId(1), offsets)],
    )
    .expect("optional shifted view");
    let Value::Tensor(shifted_cell) = &shifted else {
        panic!("expected a tensor view");
    };
    assert_eq!(
        shifted_cell
            .borrow()
            .logical_values()
            .expect("optional holes"),
        vec![Value::Int(20), Value::Int(30), Value::Nil]
    );
    let materialize_error = eval_tensor_view_op(
        MirCollectionOp::TensorMaterialize,
        vec![MirOperand::Value(MirValueId(0))],
        vec![(MirValueId(0), shifted.clone())],
    )
    .expect_err("unresolved optional view cannot escape");
    assert_eq!(materialize_error.code, RUNNER_CODE_INTERNAL);
    assert_eq!(
        materialize_error.args,
        vec![(
            "detail".to_owned(),
            "tensor materialize cannot escape unresolved shifted edges".to_owned()
        )]
    );

    let coalesced = evaluate_collection_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::TensorLift(MirLiftOp::Coalesce),
            args: vec![
                MirOperand::Value(MirValueId(0)),
                MirOperand::Constant(MirConstant::Int(99)),
            ],
            return_ty: MirType::semantic(TypeId(0)),
        },
        vec![(MirValueId(0), shifted)],
    )
    .expect("fused optional coalesce");
    let Value::Tensor(coalesced) = coalesced else {
        panic!("expected coalesced tensor");
    };
    assert_eq!(
        coalesced.borrow().logical_values().expect("coalesced data"),
        vec![Value::Int(20), Value::Int(30), Value::Int(99)]
    );
}

#[test]
fn tensor_edge_checked_displacement_rejects_coordinate_overflow() {
    assert_eq!(
        crate::tensor_edge::shifted_coordinate(&[i128::MAX], &[1]),
        None
    );
}

#[test]
fn tensor_custom_policy_callback_runtime_failure_propagates() {
    use crate::host::BufferHost;
    use radix_hir::DefId;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_mir::{
        MirBlock, MirEdgePolicy, MirLocal, MirParam, MirParamMode, MirShapeExpr, MirStatement,
        MirStatementKind, MirTerminator, MirTerminatorKind,
    };

    let types = TypeTable::new();
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let span = Span::default();
    let callback_type = MirType::semantic(TypeId(0));
    let callback = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: (0..2)
            .map(|local| MirParam {
                local: MirLocalId(local),
                name: None,
                ty: callback_type,
                mode: MirParamMode::Owned,
                span,
            })
            .collect(),
        locals: (0..2)
            .map(|id| MirLocal {
                id: MirLocalId(id),
                name: None,
                ty: callback_type,
                mutable: false,
                span,
            })
            .collect(),
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::TensorShapeSize(MirShapeExpr::Divide {
                            numerator: Box::new(MirShapeExpr::Literal(1)),
                            divisor: 0,
                        }),
                        args: vec![],
                        return_ty: callback_type,
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
        return_ty: callback_type,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    };
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    program.functions.push(callback);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner
        .frames
        .last_mut()
        .expect("caller frame")
        .values
        .insert(
            MirValueId(0),
            tensor_value(vec![Value::Int(7), Value::Int(8)], vec![2]),
        );
    runner
        .frames
        .last_mut()
        .expect("caller frame")
        .values
        .insert(
            MirValueId(1),
            Value::Vector(Rc::new(RefCell::new(vec![Value::Int(-1)]))),
        );
    let shifted = runner
        .eval_runtime_call(
            &MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorShift),
                args: vec![
                    MirOperand::Value(MirValueId(0)),
                    MirOperand::Value(MirValueId(1)),
                ],
                return_ty: callback_type,
            },
            span,
        )
        .expect("lazy shift");
    runner
        .frames
        .last_mut()
        .expect("caller frame")
        .values
        .insert(MirValueId(2), shifted);
    let resolved = runner
        .eval_runtime_call(
            &MirRuntimeCall {
                intrinsic: MirIntrinsic::TensorLimes(MirEdgePolicy::Custom {
                    policy_def: DefId(0),
                    remap: MirFunctionId(1),
                    transform: None,
                }),
                args: vec![MirOperand::Value(MirValueId(2))],
                return_ty: callback_type,
            },
            span,
        )
        .expect("lazy custom limes");
    let Value::Tensor(resolved) = resolved else {
        panic!("expected custom edge view");
    };
    let error = runner
        .tensor_read_at(&resolved, 0, span)
        .expect_err("custom remap callback runtime failure");
    assert_eq!(error.issue, "runner_tensor_shape_invalid_extent");
}

#[test]
fn tensor_shape_check_evaluates_dynamic_axis_and_affine_terms() {
    use radix_mir::{MirShapeCheck, MirShapeExpr, MirShapeRelation};

    let tensor = tensor_value((0..12).map(Value::Int).collect(), vec![3, 4]);
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::TensorShapeCheck(MirShapeCheck {
            relations: vec![
                MirShapeRelation {
                    left: MirShapeExpr::TensorAxis {
                        argument: 0,
                        axis: 1,
                    },
                    right: MirShapeExpr::ScalarArgument { argument: 1 },
                },
                MirShapeRelation {
                    left: MirShapeExpr::TensorAxis {
                        argument: 0,
                        axis: 0,
                    },
                    right: MirShapeExpr::Affine {
                        base: Box::new(MirShapeExpr::Literal(1)),
                        offset: 2,
                    },
                },
                MirShapeRelation {
                    left: MirShapeExpr::Literal(2),
                    right: MirShapeExpr::Divide {
                        numerator: Box::new(MirShapeExpr::Literal(8)),
                        divisor: 4,
                    },
                },
            ],
            size_bindings: vec![],
        }),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    assert_eq!(
        evaluate_collection_call(
            &call,
            vec![(MirValueId(0), tensor), (MirValueId(1), Value::Int(4)),],
        )
        .expect("matching runtime shapes"),
        Value::Unit
    );
}

#[test]
fn tensor_shape_check_mismatch_has_recoverable_issue_identity() {
    use radix_mir::{MirShapeCheck, MirShapeExpr, MirShapeRelation};

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::TensorShapeCheck(MirShapeCheck {
            relations: vec![MirShapeRelation {
                left: MirShapeExpr::TensorAxis {
                    argument: 0,
                    axis: 0,
                },
                right: MirShapeExpr::Literal(3),
            }],
            size_bindings: vec![],
        }),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let error = evaluate_collection_call(
        &call,
        vec![(MirValueId(0), tensor_value(vec![Value::Int(1); 2], vec![2]))],
    )
    .expect_err("runtime extent mismatch");
    assert_eq!(error.issue, "runner_tensor_shape_mismatch");
    assert_eq!(error.code, RUNNER_CODE_INTERNAL);
}

#[test]
fn tensor_shape_guard_before_try_call_uses_the_call_error_block() {
    use radix_mir::{
        MirBlock, MirBlockId, MirLocal, MirParam, MirParamMode, MirShapeBinding, MirShapeCheck,
        MirShapeExpr, MirShapeRelation, MirStatement, MirStatementKind, MirTerminator,
        MirTerminatorKind,
    };
    use radix_types::{IndexExpr, NumericWidth, Primitive, TypeTable};

    let mut types = TypeTable::new();
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let two = types.intern_index(IndexExpr::Literal(2));
    let three = types.intern_index(IndexExpr::Literal(3));
    let first_shape = types.intern_index(IndexExpr::Tuple(vec![two]));
    let second_shape = types.intern_index(IndexExpr::Tuple(vec![three]));
    let first_tensor = MirType::semantic(types.tensor_with_shape(element, first_shape));
    let second_tensor = MirType::semantic(types.tensor_with_shape(element, second_shape));
    let text = MirType::semantic(types.primitive(Primitive::Textus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut interner = Interner::new();
    let entered = interner.intern("entered");
    let args = vec![
        MirOperand::Place(MirPlace::local(MirLocalId(0))),
        MirOperand::Place(MirPlace::local(MirLocalId(1))),
    ];
    let first_axis = MirShapeExpr::TensorAxis {
        argument: 0,
        axis: 0,
    };
    let second_axis = MirShapeExpr::TensorAxis {
        argument: 1,
        axis: 0,
    };
    let caller = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: first_tensor,
                mutable: false,
                span: Span::default(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: second_tensor,
                mutable: false,
                span: Span::default(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: text,
                mutable: false,
                span: Span::default(),
            },
            MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: text,
                mutable: false,
                span: Span::default(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![
            MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: None,
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::TensorShapeCheck(MirShapeCheck {
                                relations: vec![MirShapeRelation {
                                    left: first_axis.clone(),
                                    right: second_axis,
                                }],
                                size_bindings: vec![MirShapeBinding {
                                    parameter: 0,
                                    witness: first_axis,
                                }],
                            }),
                            args: args.clone(),
                            return_ty: vacuum,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::TryCall {
                        destination: Some(MirPlace::local(MirLocalId(3))),
                        callee: MirCallee::Function(MirFunctionId(1)),
                        args,
                        ok_block: MirBlockId(1),
                        error_place: MirPlace::local(MirLocalId(2)),
                        error_block: MirBlockId(2),
                    },
                    span: Span::default(),
                },
                span: Span::default(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(3),
                    )))),
                    span: Span::default(),
                },
                span: Span::default(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(2),
                    )))),
                    span: Span::default(),
                },
                span: Span::default(),
            },
        ],
        return_ty: text,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let callee = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: first_tensor,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: second_tensor,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::String(
                    entered,
                )))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: text,
        error_ty: Some(text),
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let program = MirProgram {
        functions: vec![caller, callee],
    };
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().expect("caller frame");
    frame.seed_local(
        MirLocalId(0),
        tensor_value(vec![Value::Int(1), Value::Int(2)], vec![2]),
    );
    frame.seed_local(
        MirLocalId(1),
        tensor_value(vec![Value::Int(1), Value::Int(2), Value::Int(3)], vec![3]),
    );

    runner.run_until_depth(0);

    assert!(
        runner.errors.is_empty(),
        "the call boundary catches its guard failure"
    );
    assert_eq!(
        runner.return_value,
        Some(Value::Str(
            "tensor runtime shape mismatch: 2 != 3".to_owned()
        ))
    );
    assert_eq!(runner.return_error, None);
}

#[test]
fn tensor_shape_check_reads_vector_axis_zero_and_rejects_other_axes() {
    use radix_mir::{MirShapeCheck, MirShapeExpr, MirShapeRelation};

    let call = |axis| MirRuntimeCall {
        intrinsic: MirIntrinsic::TensorShapeCheck(MirShapeCheck {
            relations: vec![MirShapeRelation {
                left: MirShapeExpr::TensorAxis { argument: 0, axis },
                right: MirShapeExpr::Literal(1),
            }],
            size_bindings: vec![],
        }),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let vector = Value::Vector(Rc::new(RefCell::new(vec![Value::Int(7)])));

    assert_eq!(
        evaluate_collection_call(&call(0), vec![(MirValueId(0), vector.clone())])
            .expect("vector axis zero is its lane count"),
        Value::Unit
    );

    let error = evaluate_collection_call(&call(1), vec![(MirValueId(0), vector)])
        .expect_err("vectors have no axis beyond zero");
    assert_eq!(error.issue, "runner_tensor_shape_invalid_extent");
    assert_eq!(error.code, RUNNER_CODE_INTERNAL);
}

#[test]
fn tensor_shape_check_rejects_inexact_division() {
    use radix_mir::{MirShapeCheck, MirShapeExpr, MirShapeRelation};

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::TensorShapeCheck(MirShapeCheck {
            relations: vec![MirShapeRelation {
                left: MirShapeExpr::Divide {
                    numerator: Box::new(MirShapeExpr::ScalarArgument { argument: 0 }),
                    divisor: 2,
                },
                right: MirShapeExpr::Literal(2),
            }],
            size_bindings: vec![],
        }),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let error = evaluate_collection_call(&call, vec![(MirValueId(0), Value::Int(5))])
        .expect_err("shape division must be exact");
    assert_eq!(error.code, RUNNER_CODE_INTERNAL);
    assert_eq!(error.issue, "runner_tensor_shape_invalid_extent");
}

#[test]
fn tensor_shape_check_validates_size_bindings_without_relations() {
    use radix_mir::{MirShapeBinding, MirShapeCheck, MirShapeExpr};

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::TensorShapeCheck(MirShapeCheck {
            relations: vec![],
            size_bindings: vec![MirShapeBinding {
                parameter: 17,
                witness: MirShapeExpr::ScalarArgument { argument: 0 },
            }],
        }),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let error = evaluate_collection_call(&call, vec![(MirValueId(0), Value::Int(-1))])
        .expect_err("invalid unreferenced size binding");
    // The size slot is a lift store, so a witness that fails here is the lift
    // failing its `>= 0` check, named as such since size-type Z1 (cb95f1807);
    // this pinned the generic invalid-extent issue it replaced.
    assert_eq!(error.issue, "size_lift_negative");
}

#[test]
fn tensor_shape_size_evaluates_explicit_runtime_terms() {
    use radix_mir::MirShapeExpr;

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::TensorShapeSize(MirShapeExpr::Divide {
            numerator: Box::new(MirShapeExpr::TensorAxis {
                argument: 0,
                axis: 1,
            }),
            divisor: 2,
        }),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    assert_eq!(
        evaluate_collection_call(
            &call,
            vec![(
                MirValueId(0),
                tensor_value(vec![Value::Int(0); 12], vec![3, 4]),
            )],
        )
        .expect("shape size runtime term"),
        Value::Float(2.0)
    );
}

#[test]
fn tensor_shape_size_checked_product_preserves_rank_zero_identity() {
    use radix_mir::MirShapeExpr;

    let call = |expression| MirRuntimeCall {
        intrinsic: MirIntrinsic::TensorShapeSize(expression),
        args: vec![],
        return_ty: MirType::semantic(TypeId(0)),
    };
    assert_eq!(
        evaluate_collection_call(
            &call(MirShapeExpr::Product(vec![
                MirShapeExpr::Literal(2),
                MirShapeExpr::Literal(3),
            ])),
            vec![],
        )
        .expect("shape product"),
        Value::Float(6.0)
    );
    assert_eq!(
        evaluate_collection_call(&call(MirShapeExpr::Product(vec![])), vec![])
            .expect("rank-zero empty product"),
        Value::Float(1.0)
    );

    let error = evaluate_collection_call(
        &call(MirShapeExpr::Product(vec![
            MirShapeExpr::Literal(u64::MAX),
            MirShapeExpr::Literal(2),
        ])),
        vec![],
    )
    .expect_err("shape product overflow");
    assert_eq!(error.issue, "runner_tensor_shape_invalid_extent");
}

#[test]
fn tensor_shape_size_reports_recoverable_extent_overflow() {
    use radix_mir::MirShapeExpr;

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::TensorShapeSize(MirShapeExpr::Affine {
            base: Box::new(MirShapeExpr::Literal(u64::MAX)),
            offset: i128::MAX,
        }),
        args: vec![],
        return_ty: MirType::semantic(TypeId(0)),
    };
    let error = evaluate_collection_call(&call, vec![]).expect_err("checked affine overflow");
    assert_eq!(error.code, RUNNER_CODE_INTERNAL);
    assert_eq!(error.issue, "runner_tensor_shape_invalid_extent");
}

#[test]
fn tensor_matmul_int_2x2() {
    let left = tensor_value(
        vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)],
        vec![2, 2],
    );
    let right = tensor_value(
        vec![Value::Int(5), Value::Int(6), Value::Int(7), Value::Int(8)],
        vec![2, 2],
    );
    let result = eval_tensor_matmul(left, right).expect("int matmul");
    let Value::Tensor(cell) = result else {
        panic!("expected tensor");
    };
    let out = cell.borrow();
    assert_eq!(out.shape, vec![2, 2]);
    assert_eq!(
        out.logical_values().expect("valid tensor layout"),
        vec![
            Value::Int(19),
            Value::Int(22),
            Value::Int(43),
            Value::Int(50)
        ]
    );
}

#[test]
fn tensor_matmul_mixed_promotes_to_float() {
    let left = tensor_value(vec![Value::Int(2), Value::Int(4)], vec![1, 2]);
    let right = tensor_value(vec![Value::Float(0.5), Value::Float(1.5)], vec![2, 1]);
    let result = eval_tensor_matmul(left, right).expect("mixed matmul");
    let Value::Tensor(cell) = result else {
        panic!("expected tensor");
    };
    let out = cell.borrow();
    assert_eq!(out.shape, vec![1, 1]);
    assert_numeric_close(
        &out.logical_values().expect("valid tensor layout"),
        &[7.0],
        1e-9,
    );
}

/// Evaluate a tensor collection op whose result is `tensor<element, [n]>`.
fn eval_tensor_op_typed(
    op: MirCollectionOp,
    element: radix_types::Type,
    args: Vec<MirOperand>,
    inputs: Vec<(MirValueId, Value)>,
) -> Result<Value, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let mut type_table = TypeTable::default();
    let element = type_table.intern(element);
    let shape = type_table.rank0_shape();
    let tensor = type_table.tensor_with_shape(element, shape);
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    for (id, value) in inputs {
        runner.frames.last_mut().unwrap().values.insert(id, value);
    }
    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(op),
            args,
            return_ty: MirType::semantic(tensor),
        },
        Span::default(),
    )
}

fn i8_element() -> radix_types::Type {
    radix_types::Type::SizedNumeric(
        radix_types::Primitive::Numerus,
        radix_types::NumericWidth::I8,
    )
}

fn i64_element() -> radix_types::Type {
    radix_types::Type::SizedNumeric(
        radix_types::Primitive::Numerus,
        radix_types::NumericWidth::I64,
    )
}

fn binary_operands() -> Vec<MirOperand> {
    vec![
        MirOperand::Value(MirValueId(0)),
        MirOperand::Value(MirValueId(1)),
    ]
}

fn ints(values: &[i64]) -> Value {
    tensor_value(
        values.iter().copied().map(Value::Int).collect(),
        vec![values.len() as i64],
    )
}

#[test]
fn tensor_elementwise_stores_each_element_under_the_element_width() {
    // ruling 19: 100 + 100 does not fit `i8`; the store traps.
    let err = eval_tensor_op_typed(
        MirCollectionOp::TensorAdd,
        i8_element(),
        binary_operands(),
        vec![
            (MirValueId(0), ints(&[100, 100, -100])),
            (MirValueId(1), ints(&[100, 1, -100])),
        ],
    )
    .expect_err("200 leaves i8");
    assert_eq!(err.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
    assert_eq!(err.arg("value"), Some("200"));
    assert_eq!(err.arg("type"), Some("i8"));
    // A product that fits stays; scalar broadcast is checked per element too.
    let fits = eval_tensor_op_typed(
        MirCollectionOp::TensorMul,
        i8_element(),
        binary_operands(),
        vec![
            (MirValueId(0), ints(&[100, 20, 3])),
            (MirValueId(1), Value::Int(1)),
        ],
    )
    .expect("fits");
    let Value::Tensor(cell) = fits else {
        panic!("expected tensor");
    };
    assert_eq!(
        cell.borrow().logical_values().expect("valid tensor layout"),
        vec![Value::Int(100), Value::Int(20), Value::Int(3)]
    );
    let err = eval_tensor_op_typed(
        MirCollectionOp::TensorAdd,
        i8_element(),
        binary_operands(),
        vec![
            (MirValueId(0), ints(&[100, 20])),
            (MirValueId(1), Value::Int(50)),
        ],
    )
    .expect_err("150 leaves i8");
    assert_eq!(err.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
    assert_eq!(err.arg("value"), Some("150"));
    assert_eq!(err.arg("type"), Some("i8"));
}

#[test]
fn tensor_sum_axis_partials_are_exact_and_the_result_is_stored() {
    let operands = || {
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Constant(radix_mir::MirConstant::Int(1)),
        ]
    };
    let rows = tensor_value(
        vec![Value::Int(100), Value::Int(100), Value::Int(-100)],
        vec![1, 3],
    );
    let out = eval_tensor_op_typed(
        MirCollectionOp::TensorSumAxis,
        i8_element(),
        operands(),
        vec![(MirValueId(0), rows)],
    )
    .expect("exact partial 200 is never stored");
    let Value::Tensor(cell) = out else {
        panic!("expected tensor");
    };
    assert_eq!(
        cell.borrow().logical_values().expect("valid tensor layout"),
        vec![Value::Int(100)]
    );
    let rows = tensor_value(vec![Value::Int(100), Value::Int(100)], vec![1, 2]);
    let err = eval_tensor_op_typed(
        MirCollectionOp::TensorSumAxis,
        i8_element(),
        operands(),
        vec![(MirValueId(0), rows)],
    )
    .expect_err("200 leaves i8");
    assert_eq!(err.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
    assert_eq!(err.arg("value"), Some("200"));
    assert_eq!(err.arg("type"), Some("i8"));
}

#[test]
fn tensor_sum_axis_i64_partials_exact_in_ether() {
    // Ruling 17: the sum is exact in the single 64-bit ether
    // [i64::MIN, u64::MAX]; the element width applies only at the store.
    let operands = || {
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Constant(radix_mir::MirConstant::Int(0)),
        ]
    };
    // (a) A partial at 2^63 sits inside the ether; the finished cell is 1.
    // Rank 1 sums to a scalar.
    let items = tensor_value(
        vec![Value::Int(i64::MAX), Value::Int(1), Value::Int(-i64::MAX)],
        vec![3],
    );
    let out = eval_tensor_op_typed(
        MirCollectionOp::TensorSumAxis,
        i64_element(),
        operands(),
        vec![(MirValueId(0), items)],
    )
    .expect("partial 2^63 is inside the ether and the cell 1 fits i64");
    assert_eq!(out, Value::Int(1));
    // (b) A partial at 2^64 - 2 is still inside the ether. Rank 1 sums to a
    // scalar.
    let items = tensor_value(
        vec![
            Value::Int(i64::MAX),
            Value::Int(i64::MAX),
            Value::Int(-i64::MAX),
        ],
        vec![3],
    );
    let out = eval_tensor_op_typed(
        MirCollectionOp::TensorSumAxis,
        i64_element(),
        operands(),
        vec![(MirValueId(0), items)],
    )
    .expect("partial 2^64 - 2 is inside the ether");
    assert_eq!(out, Value::Int(i64::MAX));
    // (c) A finished cell that leaves i64 traps at the store, not mid-fold.
    let items = tensor_value(vec![Value::Int(i64::MAX), Value::Int(i64::MAX)], vec![2]);
    let err = eval_tensor_op_typed(
        MirCollectionOp::TensorSumAxis,
        i64_element(),
        operands(),
        vec![(MirValueId(0), items)],
    )
    .expect_err("2^64 - 2 leaves i64 at the store");
    assert_eq!(err.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
    assert_eq!(err.arg("value"), Some("18446744073709551614"));
    // (d) A partial that leaves the single 64-bit ether is a representation
    // limit and still traps.
    let items = tensor_value(
        vec![
            Value::Int(i64::MAX),
            Value::Int(i64::MAX),
            Value::Int(i64::MAX),
        ],
        vec![3],
    );
    let err = eval_tensor_op_typed(
        MirCollectionOp::TensorSumAxis,
        i64_element(),
        operands(),
        vec![(MirValueId(0), items)],
    )
    .expect_err("3 * (2^63 - 1) leaves the ether");
    assert!(
        err.issue.contains("numerus_overflow"),
        "expected a numerus overflow issue, got {err:?}"
    );
}

#[test]
fn tensor_matmul_partials_are_exact_and_each_cell_is_stored() {
    let matrix = |values: &[i64], shape: Vec<i64>| {
        tensor_value(values.iter().copied().map(Value::Int).collect(), shape)
    };
    let run = |left: Value, right: Value| {
        eval_tensor_op_typed(
            MirCollectionOp::TensorMatMul,
            i8_element(),
            binary_operands(),
            vec![(MirValueId(0), left), (MirValueId(1), right)],
        )
    };
    // Partial 200 leaves i8, the finished cell 0 fits.
    let ok = run(
        matrix(&[100, 100, 100], vec![1, 3]),
        matrix(&[1, 1, -2], vec![3, 1]),
    )
    .expect("finished cell fits");
    let Value::Tensor(cell) = ok else {
        panic!("expected tensor");
    };
    assert_eq!(
        cell.borrow().logical_values().expect("valid tensor layout"),
        vec![Value::Int(0)]
    );
    let err = run(
        matrix(&[100, 100, 100], vec![1, 3]),
        matrix(&[1, 1, 1], vec![3, 1]),
    )
    .expect_err("300 leaves i8");
    assert_eq!(err.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
    assert_eq!(err.arg("value"), Some("300"));
    assert_eq!(err.arg("type"), Some("i8"));
}

#[test]
#[ignore = "manual before/after bench; run with --ignored"]
fn measure_tensor_matmul_128() {
    use std::time::Instant;

    let dim = 128_i64;
    let left = dense_float_tensor(dim, dim, |i, j| ((i * 17 + j) % 13) as f64);
    let right = dense_float_tensor(dim, dim, |i, j| ((i * 13 + j) % 11) as f64 * 0.5);
    // Warmup
    let _ = eval_tensor_matmul(left.clone(), right.clone()).expect("warmup");
    let mut samples = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let result = eval_tensor_matmul(left.clone(), right.clone()).expect("timed matmul");
        samples.push(start.elapsed());
        let Value::Tensor(cell) = result else {
            panic!("expected tensor");
        };
        assert_eq!(cell.borrow().shape, vec![dim, dim]);
        assert_eq!(
            cell.borrow()
                .logical_values()
                .expect("valid tensor layout")
                .len(),
            (dim * dim) as usize
        );
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    eprintln!(
        "MEASURE tensor_matmul {dim}x{dim}x{dim} n={} samples={:?} median={median:?}",
        samples.len(),
        samples
    );
}

/// AEU-5 Route C: `TensorFromFlat` with a trailing fill operand pads a shorter
/// bounded-lista snapshot to the literal shape; the fill is a real value, and a
/// flat longer than the shape still fails the exact-count contract.
#[test]
fn conversio_padded_tensor_from_flat_fills_dead_slots() {
    use crate::host::BufferHost;
    use radix_lexer::{Interner, Span};
    use radix_mir::{
        MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
        MirValidationContext, MirValueId,
    };
    use radix_types::{Primitive, TypeTable};
    use std::cell::RefCell;
    use std::rc::Rc;

    let type_table = TypeTable::new();
    let fractus = type_table.primitive(Primitive::Fractus);
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, crate::MirBlockId(0));

    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        crate::Value::Array(Rc::new(RefCell::new(vec![
            crate::Value::Float(1.0),
            crate::Value::Float(2.0),
            crate::Value::Float(3.0),
        ]))),
    );
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(1),
        crate::Value::Array(Rc::new(RefCell::new(vec![crate::Value::Int(8)]))),
    );

    let padded = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
        args: vec![
            MirOperand::Value(MirValueId(0)), // seed placeholder slot is the
            // receiver; keep flat at index 1 below by repeating the flat value.
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
            MirOperand::Constant(radix_mir::MirConstant::Float(0.0)),
        ],
        return_ty: MirType::semantic(fractus),
    };
    let result = runner
        .eval_runtime_call(&padded, Span::default())
        .expect("padded snapshot must succeed");
    let crate::Value::Tensor(tensor) = result else {
        panic!("padded snapshot must produce a tensor");
    };
    let tensor = tensor.borrow();
    assert_eq!(tensor.shape, vec![8]);
    assert_eq!(
        tensor.logical_values().expect("valid tensor layout"),
        vec![
            crate::Value::Float(1.0),
            crate::Value::Float(2.0),
            crate::Value::Float(3.0),
            crate::Value::Float(0.0),
            crate::Value::Float(0.0),
            crate::Value::Float(0.0),
            crate::Value::Float(0.0),
            crate::Value::Float(0.0),
        ]
    );

    // A flat longer than the literal shape keeps the exact-count failure.
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(2),
        crate::Value::Array(Rc::new(RefCell::new(
            (0..9).map(|_| crate::Value::Float(0.0)).collect(),
        ))),
    );
    let overlong = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
        args: vec![
            MirOperand::Value(MirValueId(2)),
            MirOperand::Value(MirValueId(2)),
            MirOperand::Value(MirValueId(1)),
            MirOperand::Constant(radix_mir::MirConstant::Float(0.0)),
        ],
        return_ty: MirType::semantic(fractus),
    };
    let err = runner
        .eval_runtime_call(&overlong, Span::default())
        .expect_err("overlong flat must fail the count contract");
    assert!(
        err.issue.contains("element_count_does_not_match"),
        "expected element-count failure, got {err:?}"
    );
}

#[test]
fn lista_elementwise_dynamic_pair_checks_length_and_broadcasts() {
    use crate::Value;
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::{Primitive, TypeTable};

    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let lista = types.array(numerus);
    let context = MirValidationContext::new(&types);
    let interner = Interner::new();
    let mut host = BufferHost::default();
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        Value::Array(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)]))),
    );
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), Value::Int(3));

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::ArrayMul),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(lista),
    };
    let result = runner
        .eval_runtime_call(&call, Span::default())
        .expect("lista scalar broadcast should succeed");
    assert_eq!(
        result,
        Value::Array(Rc::new(RefCell::new(vec![Value::Int(3), Value::Int(6)])))
    );

    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(2),
        Value::Array(Rc::new(RefCell::new(vec![Value::Int(4)]))),
    );
    let mismatch = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::ArrayAdd),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(2)),
        ],
        return_ty: MirType::semantic(lista),
    };
    let error = runner
        .eval_runtime_call(&mismatch, Span::default())
        .expect_err("dynamic lista length mismatch is a hard error");
    assert_eq!(error.issue, "runner_lista_elementwise_length_mismatch");
}

// --- Test-path limits: default step budget, wall-clock deadline, and the
// --- explicit-unbounded program-execution variant.

fn validated_loop_function<'a>(
    interner: &'a mut Interner,
    types: &'a radix_types::TypeTable,
    blocks: Vec<MirBlock>,
) -> ValidatedMir<'a> {
    let name = interner.intern("case");
    let vacuum = MirType::semantic(types.primitive(radix_types::Primitive::Vacuum));
    let mut context = MirValidationContext::new(types);
    context.interner = radix_mir::MirInterner::Borrowed(interner);
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: Some(name),
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks,
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    ValidatedMir::new(
        MirProgram {
            functions: vec![function],
        },
        context,
    )
    .expect("loop fixture validates")
}

/// Validator-legal infinite loop: `loop { if true { continue } else { return } }`
/// expressed as a branch whose condition is the constant `true`. Lowered
/// `dum verum` loops have the same shape, so the validator accepts them.
fn self_loop_blocks() -> Vec<MirBlock> {
    vec![
        MirBlock {
            id: MirBlockId(0),
            statements: vec![],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Branch {
                    condition: MirOperand::Constant(MirConstant::Bool(true)),
                    then_block: MirBlockId(0),
                    else_block: MirBlockId(1),
                },
                span: Span::default(),
            },
            span: Span::default(),
        },
        MirBlock {
            id: MirBlockId(1),
            statements: vec![],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        },
    ]
}

fn block_chain(len: u32) -> Vec<MirBlock> {
    (0..len)
        .map(|index| MirBlock {
            id: MirBlockId(index),
            statements: vec![],
            terminator: MirTerminator {
                kind: if index + 1 < len {
                    MirTerminatorKind::Goto(MirBlockId(index + 1))
                } else {
                    MirTerminatorKind::Return(None)
                },
                span: Span::default(),
            },
            span: Span::default(),
        })
        .collect()
}

#[test]
fn none_step_budget_applies_default_guard_to_infinite_loop() {
    let mut interner = Interner::new();
    let types = radix_types::TypeTable::new();
    // One block that jumps to itself: unbounded work, no host calls.
    let loop_blocks = self_loop_blocks();
    let validated = validated_loop_function(&mut interner, &types, loop_blocks);
    let mut host = BufferHost::default();
    let result = run_function_with_limits(&validated, MirFunctionId(0), &mut host, None, None);
    let errors = result.expect_err("default step budget must stop an infinite loop");
    // Issue identity only: the exhaustion identity is a named constant, so the
    // step count never enters the diagnostic identity (hygiene:
    // `non_renderer_tests_do_not_assert_diagnostic_text`).
    let exhaustion = RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED;
    assert!(
        errors.iter().any(|error| error.issue == exhaustion),
        "expected default-budget exhaustion, got: {errors:?}"
    );
}

#[test]
fn stats_entry_without_an_explicit_entry_reports_the_runner_identity() {
    let mut interner = Interner::new();
    let types = radix_types::TypeTable::new();
    // The fixture's only function is named, so there is no explicit entry.
    let validated = validated_loop_function(&mut interner, &types, self_loop_blocks());
    let mut host = BufferHost::default();
    let errors = run_entry_with_stats(&validated, &mut host)
        .expect_err("a program without an explicit entry cannot start");
    assert_eq!(errors[0].issue, "runner_no_entry_function_in_mir_program");
}

#[test]
fn explicit_unbounded_variant_completes_bounded_program() {
    let mut interner = Interner::new();
    let types = radix_types::TypeTable::new();
    // 300-block chain: far beyond a small budget, far below the default.
    let validated = validated_loop_function(&mut interner, &types, block_chain(300));
    let mut host = BufferHost::default();
    run_function_unbounded(&validated, MirFunctionId(0), &mut host)
        .expect("program execution is explicitly unbounded and completes");
    // The test path on the same shape: a small budget trips, the default
    // (None -> 1e8) does not — the variants differ.
    let validated = validated_loop_function(&mut interner, &types, block_chain(300));
    let mut host = BufferHost::default();
    assert!(
        run_function_with_limits(&validated, MirFunctionId(0), &mut host, Some(64), None).is_err(),
        "explicit small budget must trip on a 300-step chain"
    );
    let mut host = BufferHost::default();
    run_function_with_limits(&validated, MirFunctionId(0), &mut host, None, None)
        .expect("default budget (1e8) does not trip a 300-step chain");
}

#[test]
fn wall_clock_deadline_stops_infinite_loop() {
    let mut interner = Interner::new();
    let types = radix_types::TypeTable::new();
    let loop_blocks = self_loop_blocks();
    let validated = validated_loop_function(&mut interner, &types, loop_blocks);
    let mut host = BufferHost::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    let result = run_function_with_limits(
        &validated,
        MirFunctionId(0),
        &mut host,
        None,
        Some(deadline),
    );
    let errors = result.expect_err("wall-clock deadline must stop an infinite loop");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "runner_wall_clock_timeout_exceeded"),
        "expected wall-clock timeout, got: {errors:?}"
    );
}

/// Red-first twin of `none_step_budget_applies_default_guard_to_infinite_loop`:
/// the explicit-unbounded variant must NOT stop on the default guard. Runs
/// forever by design — only execute under an external timeout.
#[test]
#[ignore = "infinite by design: run under an external timeout (need evidence)"]
fn explicit_unbounded_variant_does_not_stop_on_default_guard() {
    let mut interner = Interner::new();
    let types = radix_types::TypeTable::new();
    let loop_blocks = self_loop_blocks();
    let validated = validated_loop_function(&mut interner, &types, loop_blocks);
    let mut host = BufferHost::default();
    let result = run_function_unbounded(&validated, MirFunctionId(0), &mut host);
    panic!("unbounded variant must never return from an infinite loop: {result:?}");
}

#[test]
fn direct_kernel_function_run_follows_the_kernel_policy() {
    let mut interner = Interner::new();
    let marked = interner.intern("marked");
    let types = radix_types::TypeTable::new();
    let mut context = MirValidationContext::new(&types);
    context.interner = radix_mir::MirInterner::Borrowed(&interner);
    let vacuum = MirType::semantic(types.primitive(radix_types::Primitive::Vacuum));
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: Some(marked),
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(radix_mir::MirKernelShaderStage::Compute),
        span: Span::default(),
    };
    let validated = ValidatedMir::new(
        MirProgram {
            functions: vec![function],
        },
        context,
    )
    .expect("kernel refusal fixture validates");
    let mut host = BufferHost::default();

    // Default policy: the run is refused, with the stable identity.
    let errors = run_function(&validated, MirFunctionId(0), &mut host)
        .expect_err("the runner must refuse direct kernel execution");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].issue, RUNNER_KERNEL_REFUSED_ISSUE);
    assert_eq!(RunnerKernelPolicy::default(), RunnerKernelPolicy::Refuse);
    let explicit = run_function_with_kernel_policy(
        &validated,
        MirFunctionId(0),
        &mut host,
        RunnerKernelPolicy::Refuse,
    )
    .expect_err("an explicit Refuse policy refuses like the default");
    assert_eq!(explicit[0].issue, RUNNER_KERNEL_REFUSED_ISSUE);

    // CPU policy: the same program runs to completion.
    run_function_with_kernel_policy(
        &validated,
        MirFunctionId(0),
        &mut host,
        RunnerKernelPolicy::Execute,
    )
    .expect("the Execute policy runs a compute-stage function");
}

fn eval_tensor_intrinsic(
    intrinsic: MirCollectionOp,
    inputs: Vec<Value>,
    span: Span,
) -> Result<Value, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let type_table = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    let mut runner = Runner::new(&program, &context, &type_table, &interner, &mut host);
    let function = program.functions.first().unwrap();
    runner.push_frame(function.id, MirBlockId(0));
    for (index, input) in inputs.into_iter().enumerate() {
        runner
            .frames
            .last_mut()
            .unwrap()
            .values
            .insert(MirValueId(index as u32), input);
    }
    let args = (0..runner.frames.last().unwrap().values.len())
        .map(|index| MirOperand::Value(MirValueId(index as u32)))
        .collect();
    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(intrinsic),
            args,
            return_ty: MirType::semantic(TypeId(0)),
        },
        span,
    )
}

fn tensor(data: Vec<Value>, shape: Vec<i64>) -> Value {
    Value::Tensor(Rc::new(RefCell::new(
        crate::value::TensorValue::contiguous(data, shape),
    )))
}

#[test]
fn pd25_row_softmax_preserves_values_and_errors() {
    let span = Span::new(17, 23);
    let rank1 = eval_tensor_intrinsic(
        MirCollectionOp::TensorSoftmax,
        vec![tensor(
            vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)],
            vec![3],
        )],
        span,
    )
    .expect("rank-1 softmax");
    let Value::Tensor(rank1) = rank1 else {
        panic!("rank-1 softmax must return a tensor");
    };
    let rank1 = rank1.borrow();
    assert_eq!(rank1.shape, vec![3]);
    let rank1_values: Vec<f64> = rank1
        .logical_values()
        .expect("valid tensor layout")
        .iter()
        .map(|value| match value {
            Value::Float(value) => *value,
            other => panic!("expected float, got {other:?}"),
        })
        .collect();
    assert_eq!(
        rank1_values
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>(),
        vec![
            4591151807155678170,
            4597985292698930127,
            4604167177386354576
        ],
    );

    let rank2 = eval_tensor_intrinsic(
        MirCollectionOp::TensorSoftmax,
        vec![tensor(
            vec![
                Value::Float(1.0),
                Value::Float(2.0),
                Value::Float(3.0),
                Value::Float(1.0),
                Value::Float(2.0),
                Value::Float(3.0),
            ],
            vec![2, 3],
        )],
        span,
    )
    .expect("rank-2 softmax");
    let Value::Tensor(rank2) = rank2 else {
        panic!("rank-2 softmax must return a tensor");
    };
    let rank2 = rank2.borrow();
    assert_eq!(rank2.shape, vec![2, 3]);
    assert_eq!(
        rank2
            .logical_values()
            .expect("valid tensor layout")
            .iter()
            .map(|value| match value {
                Value::Float(value) => value.to_bits(),
                other => panic!("expected float, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        vec![
            4591151807155678170,
            4597985292698930127,
            4604167177386354576,
            4591151807155678170,
            4597985292698930127,
            4604167177386354576,
        ],
    );

    let entropia = eval_tensor_intrinsic(
        MirCollectionOp::TensorCruxEntropia,
        vec![
            tensor(
                vec![
                    Value::Float(2.0),
                    Value::Float(1.0),
                    Value::Float(0.0),
                    Value::Float(1.0),
                    Value::Float(2.0),
                    Value::Float(1.0),
                ],
                vec![2, 3],
            ),
            tensor(
                vec![
                    Value::Float(1.0),
                    Value::Float(0.0),
                    Value::Float(0.0),
                    Value::Float(0.0),
                    Value::Float(1.0),
                    Value::Float(0.0),
                ],
                vec![2, 3],
            ),
        ],
        span,
    )
    .expect("entropia");
    let Value::Float(entropia) = entropia else {
        panic!("entropia must return a float");
    };
    // Cross-entropy divides by the batch row count, not the last dimension
    // (sb264 917bdd5297, XENTDIV, need item 8): the mean of the two
    // row losses ln(1 + e^-1 + e^-2) and ln(1 + 2e^-1) is 0.47952517...
    assert_eq!(entropia.to_bits(), 4602309977556029130);

    let cases = [
        (
            MirCollectionOp::TensorSoftmax,
            vec![],
            "tensor softmax arity",
        ),
        (
            MirCollectionOp::TensorSoftmax,
            vec![Value::Int(7)],
            "tensor receiver type mismatch: Int(7)",
        ),
        (
            MirCollectionOp::TensorSoftmax,
            vec![tensor(vec![], vec![])],
            "tensor softmax requires non-empty tensor",
        ),
        (
            MirCollectionOp::TensorSoftmax,
            vec![tensor(vec![], vec![2, 0])],
            "tensor softmax requires all dimensions to be positive",
        ),
        (
            MirCollectionOp::TensorSoftmax,
            vec![tensor(vec![Value::Bool(true)], vec![1])],
            "tensor numeric element type mismatch",
        ),
        (
            MirCollectionOp::TensorCruxEntropia,
            vec![],
            "tensor crux_entropia arity",
        ),
        (
            MirCollectionOp::TensorCruxEntropia,
            vec![Value::Int(7), Value::Int(8)],
            "tensor receiver type mismatch: Int(7)",
        ),
        (
            MirCollectionOp::TensorCruxEntropia,
            vec![tensor(vec![], vec![])],
            "tensor crux_entropia arity",
        ),
    ];
    for (intrinsic, inputs, detail) in cases {
        let error = eval_tensor_intrinsic(intrinsic, inputs, span).expect_err(detail);
        assert_eq!(error, RunnerError::internal(span, detail));
    }

    let error = eval_tensor_intrinsic(
        MirCollectionOp::TensorCruxEntropia,
        vec![tensor(vec![], vec![]), tensor(vec![], vec![])],
        span,
    )
    .expect_err("empty entropia tensor");
    assert_eq!(
        error,
        RunnerError::internal(span, "tensor crux_entropia requires non-empty tensor")
    );

    let error = eval_tensor_intrinsic(
        MirCollectionOp::TensorCruxEntropia,
        vec![
            tensor(vec![Value::Float(1.0)], vec![1, 1, 1]),
            tensor(vec![Value::Float(1.0)], vec![1, 1, 1]),
        ],
        span,
    )
    .expect_err("rank-three entropia tensor");
    assert_eq!(
        error,
        RunnerError::internal(
            span,
            "tensor crux_entropia requires rank-1 or rank-2 tensor"
        )
    );

    let error = eval_tensor_intrinsic(
        MirCollectionOp::TensorCruxEntropia,
        vec![tensor(vec![], vec![2, 0]), tensor(vec![], vec![2, 0])],
        span,
    )
    .expect_err("zero-width entropia tensor");
    assert_eq!(
        error,
        RunnerError::internal(
            span,
            "tensor crux_entropia requires all dimensions to be positive"
        )
    );

    let error = eval_tensor_intrinsic(
        MirCollectionOp::TensorCruxEntropia,
        vec![
            tensor(vec![Value::Bool(true)], vec![1]),
            tensor(vec![Value::Float(1.0)], vec![1]),
        ],
        span,
    )
    .expect_err("nonnumeric entropia row");
    assert_eq!(
        error,
        RunnerError::internal(span, "tensor numeric element type mismatch")
    );

    let error = eval_tensor_intrinsic(
        MirCollectionOp::TensorCruxEntropia,
        vec![
            tensor(vec![Value::Float(1.0), Value::Float(2.0)], vec![2]),
            tensor(vec![Value::Float(1.0)], vec![1]),
        ],
        span,
    )
    .expect_err("entropia shape mismatch");
    assert_eq!(
        error,
        RunnerError::internal(span, "tensor crux_entropia shape mismatch")
    );

    // A backing shorter than the shape implies is malformed: it fails closed
    // instead of yielding an empty result (a08c554b9).
    let error = eval_tensor_intrinsic(
        MirCollectionOp::TensorSoftmax,
        vec![tensor(vec![Value::Float(1.0)], vec![2])],
        span,
    )
    .expect_err("softmax short backing is a backing-length mismatch");
    assert_eq!(
        error,
        RunnerError::internal(span, "tensor backing-length mismatch")
    );

    let error = eval_tensor_intrinsic(
        MirCollectionOp::TensorCruxEntropia,
        vec![
            tensor(vec![Value::Float(1.0)], vec![2]),
            tensor(vec![Value::Float(1.0)], vec![2]),
        ],
        span,
    )
    .expect_err("entropia short backing is a backing-length mismatch");
    assert_eq!(
        error,
        RunnerError::internal(span, "tensor backing-length mismatch")
    );
}

#[test]
#[rustfmt::skip]
fn pd25_map_tensor_unary_preserves_values_and_errors
() {
    use crate::value::TensorValue;

    fn tensor(data: Vec<Value>, shape: Vec<i64>) -> Value {
        Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(data, shape))))
    }

    fn assert_tensor(result: Value, expected: Vec<Value>, shape: &[i64]) {
        let Value::Tensor(cell) = result else {
            panic!("expected tensor output");
        };
        let output = cell.borrow();
        assert_eq!(output.shape, shape);
        assert_eq!(output.logical_values().expect("valid tensor layout"), expected);
    }

    fn assert_internal(result: Result<Value, RunnerError>, message: &'static str) {
        let error = result.expect_err("malformed tensor should fail");
        assert_eq!(error.issue, super::runner_issue(message));
        assert_eq!(error.code, RUNNER_CODE_INTERNAL);
        assert_eq!(error.args, vec![("detail".to_owned(), message.to_owned())]);
        assert_eq!(error.span, Span::default());
    }

    let mut host = BufferHost::default();
    let types = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let operand = MirOperand::Value(MirValueId(0));
    let result_ty = MirType::semantic(TypeId(0));

    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        tensor(
            vec![Value::Float(1.0), Value::Float(4.0), Value::Float(9.0)],
            vec![3],
        ),
    );
    assert_tensor(
        runner
            .eval_unop(MirUnOp::Sqrt, &operand, result_ty, Span::default())
            .unwrap(),
        vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)],
        &[3],
    );

    let float_inputs = vec![Value::Float(-2.0), Value::Float(0.5), Value::Float(2.0)];
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), tensor(float_inputs.clone(), vec![3]));
    assert_tensor(
        runner
            .eval_unop(MirUnOp::Gelu, &operand, result_ty, Span::default())
            .unwrap(),
        float_inputs
            .iter()
            .map(|value| match value {
                Value::Float(value) => Value::Float(gelu_f64(*value)),
                _ => unreachable!(),
            })
            .collect(),
        &[3],
    );

    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        tensor(
            vec![Value::Float(-1.0), Value::Float(0.0), Value::Float(1.0)],
            vec![3],
        ),
    );
    assert_tensor(
        runner
            .eval_unop(MirUnOp::Tanh, &operand, result_ty, Span::default())
            .unwrap(),
        vec![
            Value::Float((-1.0_f64).tanh()),
            Value::Float(0.0),
            Value::Float(1.0_f64.tanh()),
        ],
        &[3],
    );

    runner.frames.last_mut().unwrap().values.insert(
        MirValueId(0),
        tensor(
            vec![Value::Float(-1.0), Value::Float(0.0), Value::Float(1.0)],
            vec![3],
        ),
    );
    assert_tensor(
        runner
            .eval_unop(MirUnOp::Exp, &operand, result_ty, Span::default())
            .unwrap(),
        vec![
            Value::Float((-1.0_f64).exp()),
            Value::Float(1.0),
            Value::Float(1.0_f64.exp()),
        ],
        &[3],
    );

    let f16_inputs = vec![Value::Float(-1.1), Value::Float(0.0), Value::Float(1.1)];
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), tensor(f16_inputs.clone(), vec![3]));
    assert_tensor(
        runner
            .eval_unop(MirUnOp::F16Round, &operand, result_ty, Span::default())
            .unwrap(),
        f16_inputs
            .iter()
            .map(|value| match value {
                Value::Float(value) => Value::Float(f16_round_f64(*value)),
                _ => unreachable!(),
            })
            .collect(),
        &[3],
    );

    let unary_errors = [
        (MirUnOp::Sqrt, "tensor sqrt element type mismatch"),
        (MirUnOp::Gelu, "tensor gelu element type mismatch"),
        (MirUnOp::Tanh, "tensor tanh element type mismatch"),
        (MirUnOp::Exp, "tensor exp element type mismatch"),
        (MirUnOp::F16Round, "tensor f16-round element type mismatch"),
    ];
    for &(op, message) in &unary_errors {
        runner.frames.last_mut().unwrap().values.insert(
            MirValueId(0),
            tensor(
                vec![
                    Value::Float(1.0),
                    Value::Str("bad".to_owned()),
                    Value::Float(2.0),
                ],
                vec![3],
            ),
        );
        assert_internal(
            runner.eval_unop(op, &operand, result_ty, Span::default()),
            message,
        );
    }

    let collection_cases = [
        (
            MirCollectionOp::TensorNeg,
            "tensor neg element type mismatch",
            "tensor neg arity",
            vec![Value::Int(-2), Value::Int(0), Value::Int(3)],
            vec![Value::Int(2), Value::Int(0), Value::Int(-3)],
        ),
        (
            MirCollectionOp::TensorGelu,
            "tensor gelu element type mismatch",
            "tensor gelu arity",
            vec![Value::Float(-1.0), Value::Float(0.0), Value::Float(1.0)],
            vec![
                Value::Float(gelu_f64(-1.0)),
                Value::Float(gelu_f64(0.0)),
                Value::Float(gelu_f64(1.0)),
            ],
        ),
        (
            MirCollectionOp::TensorRelu,
            "tensor relu element type mismatch",
            "tensor relu arity",
            vec![Value::Float(-1.0), Value::Float(0.0), Value::Float(2.0)],
            vec![Value::Float(0.0), Value::Float(0.0), Value::Float(2.0)],
        ),
    ];
    for &(op, element_message, arity_message, ref inputs, ref expected) in &collection_cases {
        runner
            .frames
            .last_mut()
            .unwrap()
            .values
            .insert(MirValueId(0), tensor(inputs.clone(), vec![3]));
        let call = MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(op),
            args: vec![operand.clone()],
            return_ty: result_ty,
        };
        assert_tensor(
            runner.eval_runtime_call(&call, Span::default()).unwrap(),
            expected.clone(),
            &[3],
        );

        runner.frames.last_mut().unwrap().values.insert(
            MirValueId(0),
            tensor(
                vec![Value::Float(1.0), Value::Bool(true), Value::Float(2.0)],
                vec![3],
            ),
        );
        assert_internal(
            runner.eval_runtime_call(&call, Span::default()),
            element_message,
        );

        let arity_call = MirRuntimeCall {
            args: vec![],
            ..call
        };
        assert_internal(
            runner.eval_runtime_call(&arity_call, Span::default()),
            arity_message,
        );
    }
}

#[test]
fn tensor_expand_static_tensor_witness_matches_list_shape_and_shares_storage() {
    let source = tensor_value(
        vec![Value::Int(10), Value::Int(20), Value::Int(30)],
        vec![1, 3],
    );
    let witness = tensor_value(vec![Value::Int(0); 24], vec![2, 4, 3]);
    let expanded = eval_tensor_view_op(
        MirCollectionOp::TensorExpand,
        vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        vec![(MirValueId(0), source.clone()), (MirValueId(1), witness)],
    )
    .expect("static tensor witness expansion");
    let (Value::Tensor(source), Value::Tensor(expanded)) = (&source, &expanded) else {
        unreachable!();
    };
    assert!(Arc::ptr_eq(&source.borrow().data, &expanded.borrow().data));
    assert_eq!(expanded.borrow().shape, [2, 4, 3]);
    let values = expanded.borrow().logical_values().unwrap();
    assert_eq!(
        values,
        [Value::Int(10), Value::Int(20), Value::Int(30)]
            .into_iter()
            .cycle()
            .take(24)
            .collect::<Vec<_>>()
    );
}

fn assign_statement(
    destination: MirTempId,
    value_id: MirValueId,
    operand: MirOperand,
) -> radix_mir::MirStatement {
    radix_mir::MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::temp(destination),
            value: MirValue {
                id: value_id,
                kind: MirValueKind::Operand(operand),
                ty: MirType::semantic(TypeId(0)),
                span: Span::default(),
            },
        },
        span: Span::default(),
    }
}

/// Run two `Assign` statements in one frame. The second reads the first
/// statement's value id when `read_value` is set. Returns the second temp and
/// whether the frame recorded no statement results.
fn run_two_assigns(read_value: bool) -> (Option<Value>, bool) {
    let types = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let first = assign_statement(
        MirTempId(0),
        MirValueId(0),
        MirOperand::Constant(MirConstant::Int(7)),
    );
    let second_operand = if read_value {
        MirOperand::Value(MirValueId(0))
    } else {
        MirOperand::Temp(MirTempId(0))
    };
    let second = assign_statement(MirTempId(1), MirValueId(1), second_operand);
    let mut function = make_function();
    // The slot-indexed frame sizes itself from the declared temps, so the
    // fixture declares the ids its statements write (validated MIR always
    // does).
    function.temps = [MirTempId(0), MirTempId(1)]
        .into_iter()
        .map(|id| radix_mir::MirTemp {
            id,
            ty: MirType::semantic(TypeId(0)),
            span: Span::default(),
        })
        .collect();
    function.blocks.push(MirBlock {
        id: MirBlockId(0),
        statements: vec![first.clone(), second.clone()],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    });
    let mut program = MirProgram::new();
    program.functions.push(function);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.execute_statement(&first).expect("first assign");
    runner.execute_statement(&second).expect("second assign");
    let frame = runner.frames.last().expect("frame");
    (
        frame.temps.get(1).cloned().flatten(),
        frame.values.is_empty(),
    )
}

#[test]
fn assign_results_are_readable_through_value_operands() {
    let (read, recorded_nothing) = run_two_assigns(true);
    assert_eq!(read, Some(Value::Int(7)));
    assert!(
        !recorded_nothing,
        "a value-reading function records Assign results"
    );
}

#[test]
fn functions_without_value_operands_record_no_assign_results() {
    let (read, recorded_nothing) = run_two_assigns(false);
    assert_eq!(read, Some(Value::Int(7)));
    assert!(
        recorded_nothing,
        "no Value operand, so no per-Assign value record"
    );
}

#[test]
fn fault_text_channel_receives_text() {
    let fault = Some(("NotFound".into(), "/x".into()));
    let message = "NotFound /x".to_owned();
    assert_eq!(
        failure_channel_value(&Primitive::Textus, fault.clone(), message.clone()),
        Value::Str("NotFound /x".into()),
        "a textus channel receives the failure text, not the fault map"
    );
    assert_eq!(
        failure_channel_value(&Primitive::Ignotum, fault.clone(), message.clone()),
        routes::fault_record_value("NotFound", "/x"),
        "an ignotum channel receives the fault record map"
    );
    assert_eq!(
        failure_channel_value(&Primitive::Valor, fault, message),
        routes::fault_record_value("NotFound", "/x"),
        "a valor channel receives the fault record map"
    );
    assert_eq!(
        failure_channel_value(&Primitive::Textus, None, "NotFound /x".into()),
        Value::Str("NotFound /x".into()),
        "without fault facts, a textus channel still receives the text"
    );
}
