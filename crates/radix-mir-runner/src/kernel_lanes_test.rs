//! Device-form kernels run lane by lane on the runner (gpu-reset F1-1).
//!
//! The runner crate cannot depend on `radix-module`, so the two kernels are
//! hand-built to match, statement for statement, the device-form MIR that
//! `faber mir` prints for them:
//!
//! ```text
//! fn add(tf32[4] a, tf32[4] b, mut tf32[4] out, u32 id)
//!   %0 = _0[_3] + _1[_3];  _2[_3] = %0
//! fn scale(tf32[2,3] input, mut tf32[2,3] output, u32 y, u32 x)
//!   %0 = extent _1 axis 1; %1 = _2 * %0; %2 = %1 + _3
//!   %3 = extent _0 axis 1; %4 = _2 * %3; %5 = %4 + _3
//!   %6 = const float 2.0;  %7 = _0[%5] * %6;  _1[%2] = %7
//! ```
//!
//! The expected tensors are the host-form meaning of each kernel (the same
//! element expression applied at every index).

use super::kernel_lanes::{
    run_device_form_call_prepared, run_device_form_lane, run_device_form_lanes,
    run_device_form_lanes_prepared,
};
use super::*;
use crate::host::BufferHost;
use crate::value::TensorValue;
use radix_mir::{
    MirBlock, MirKernelBuiltin, MirKernelBuiltinInput, MirLocal, MirParam, MirStatement, MirTemp,
    MirTerminator, MirValue,
};
use radix_types::{IndexExpr, NumericWidth};

fn span() -> Span {
    Span::default()
}

fn assign(place: MirPlace, id: u32, kind: MirValueKind, ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place,
            value: MirValue {
                id: MirValueId(id),
                kind,
                ty,
                span: span(),
            },
        },
        span: span(),
    }
}

fn indexed(local: u32, index: MirOperand) -> MirPlace {
    MirPlace {
        base: MirPlaceBase::Local(MirLocalId(local)),
        projections: vec![MirProjection::Index(index)],
    }
}

fn binary(op: MirBinOp, lhs: MirOperand, rhs: MirOperand) -> MirValueKind {
    MirValueKind::Binary { op, lhs, rhs }
}

fn temp(id: u32) -> MirOperand {
    MirOperand::Temp(MirTempId(id))
}

fn local(id: u32) -> MirOperand {
    MirOperand::Place(MirPlace::local(MirLocalId(id)))
}

struct Fixture {
    types: TypeTable,
    interner: Interner,
    program: MirProgram,
}

impl Fixture {
    fn validated(&self) -> ValidatedMir<'_> {
        let mut context = MirValidationContext::new(&self.types);
        context.interner = radix_mir::MirInterner::Borrowed(&self.interner);
        ValidatedMir::new(self.program.clone(), context).expect("device-form fixture validates")
    }
}

/// Build a one-function program from tensor shape `dims`, `scalar_params`
/// trailing index parameters, and the kernel body.
fn fixture(
    dims: &[u64],
    scalar_params: u32,
    temp_floats: &[u32],
    temp_ints: &[u32],
    build: impl FnOnce(MirType, MirType) -> Vec<MirStatement>,
) -> Fixture {
    let mut types = TypeTable::new();
    let float = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let int = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let extents: Vec<_> = dims
        .iter()
        .map(|dim| types.intern_index(IndexExpr::Literal(*dim)))
        .collect();
    let shape = types.intern_index(IndexExpr::Tuple(extents));
    let tensor = types.tensor_with_shape(float, shape);
    let (float, int, tensor, vacuum) = (
        MirType::semantic(float),
        MirType::semantic(int),
        MirType::semantic(tensor),
        MirType::semantic(vacuum),
    );
    let statements = build(int, float);
    let param = |id: u32, ty: MirType, mode: MirParamMode| MirParam {
        local: MirLocalId(id),
        name: None,
        ty,
        mode,
        span: span(),
    };
    let local_of = |id: u32, ty: MirType, mutable: bool| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable,
        span: span(),
    };
    let mut params = vec![
        param(0, tensor, MirParamMode::Ref),
        param(1, tensor, MirParamMode::MutRef),
    ];
    let mut locals = vec![local_of(0, tensor, false), local_of(1, tensor, true)];
    // `add` has a second input tensor ahead of `out`; `scale` does not.
    let first_scalar = if scalar_params == 1 {
        params.insert(1, param(1, tensor, MirParamMode::Ref));
        params[2] = param(2, tensor, MirParamMode::MutRef);
        locals = vec![
            local_of(0, tensor, false),
            local_of(1, tensor, false),
            local_of(2, tensor, true),
        ];
        3
    } else {
        2
    };
    for index in 0..scalar_params {
        let id = first_scalar + index;
        params.push(param(id, int, MirParamMode::Owned));
        locals.push(local_of(id, int, false));
    }
    let mut temps = Vec::new();
    for id in temp_ints {
        temps.push(MirTemp {
            id: MirTempId(*id),
            ty: int,
            span: span(),
        });
    }
    for id in temp_floats {
        temps.push(MirTemp {
            id: MirTempId(*id),
            ty: float,
            span: span(),
        });
    }
    temps.sort_by_key(|temp| temp.id.0);
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params,
        locals,
        temps,
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: span(),
            },
            span: span(),
        }],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: span(),
    };
    Fixture {
        types,
        interner: Interner::new(),
        program: MirProgram {
            functions: vec![function],
        },
    }
}

fn tensor(values: &[f64], shape: &[i64]) -> Value {
    let data = values.iter().map(|v| Value::Float(*v)).collect();
    Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        data,
        shape.to_vec(),
    ))))
}

fn contents(value: &Value) -> Vec<f64> {
    let Value::Tensor(tensor) = value else {
        panic!("not a tensor")
    };
    let tensor = tensor.borrow();
    tensor
        .data
        .to_values()
        .iter()
        .map(|v| match v {
            Value::Float(f) => *f,
            other => panic!("non-float element {other:?}"),
        })
        .collect()
}

fn builtin(local: u32, which: MirKernelBuiltin) -> MirKernelBuiltinInput {
    MirKernelBuiltinInput {
        local: MirLocalId(local),
        source_name: None,
        builtin: which,
        ty: MirType::semantic(TypeId(0)),
    }
}

#[test]
fn elementwise_add_runs_per_lane_and_matches_host_form() {
    let fx = fixture(&[4], 1, &[0], &[], |_, float| {
        vec![
            assign(
                MirPlace::temp(MirTempId(0)),
                0,
                binary(
                    MirBinOp::Add,
                    MirOperand::Place(indexed(0, local(3))),
                    MirOperand::Place(indexed(1, local(3))),
                ),
                float,
            ),
            assign(
                indexed(2, local(3)),
                1,
                MirValueKind::Operand(temp(0)),
                float,
            ),
        ]
    });
    let validated = fx.validated();
    let a = [1.0, 2.0, 3.0, 4.0];
    let b = [0.5, 0.25, 8.0, -1.0];
    let (ta, tb, out) = (tensor(&a, &[4]), tensor(&b, &[4]), tensor(&[0.0; 4], &[4]));
    let mut host = BufferHost::default();
    for lane in 0..4 {
        run_device_form_lane(
            &validated,
            MirFunctionId(0),
            &mut host,
            &[ta.clone(), tb.clone(), out.clone()],
            &[builtin(3, MirKernelBuiltin::GlobalInvocationX)],
            [lane, 0, 0],
        )
        .unwrap_or_else(|errors| panic!("lane {lane}: {errors:?}"));
    }
    let expected: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
    assert_eq!(contents(&out), expected);
}

#[test]
fn rank2_linearized_index_with_tensor_extent_matches_host_form() {
    let body = |int: MirType, float: MirType| {
        vec![
            assign(
                MirPlace::temp(MirTempId(0)),
                0,
                MirValueKind::TensorExtent {
                    tensor: local(1),
                    axis: 1,
                },
                int,
            ),
            assign(
                MirPlace::temp(MirTempId(1)),
                1,
                binary(MirBinOp::Mul, local(2), temp(0)),
                int,
            ),
            assign(
                MirPlace::temp(MirTempId(2)),
                2,
                binary(MirBinOp::Add, temp(1), local(3)),
                int,
            ),
            assign(
                MirPlace::temp(MirTempId(3)),
                3,
                MirValueKind::TensorExtent {
                    tensor: local(0),
                    axis: 1,
                },
                int,
            ),
            assign(
                MirPlace::temp(MirTempId(4)),
                4,
                binary(MirBinOp::Mul, local(2), temp(3)),
                int,
            ),
            assign(
                MirPlace::temp(MirTempId(5)),
                5,
                binary(MirBinOp::Add, temp(4), local(3)),
                int,
            ),
            assign(
                MirPlace::temp(MirTempId(6)),
                6,
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(2.0))),
                float,
            ),
            assign(
                MirPlace::temp(MirTempId(7)),
                7,
                binary(
                    MirBinOp::Mul,
                    MirOperand::Place(indexed(0, temp(5))),
                    temp(6),
                ),
                float,
            ),
            assign(
                indexed(1, temp(2)),
                8,
                MirValueKind::Operand(temp(7)),
                float,
            ),
        ]
    };
    let mut fx = fixture(&[2, 3], 2, &[6, 7], &[0, 1, 2, 3, 4, 5], body);
    // The first scalar parameter is `y` (local 2), the second `x` (local 3).
    fx.program.functions[0].params.truncate(4);
    let validated = fx.validated();
    let input = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let (tin, out) = (tensor(&input, &[2, 3]), tensor(&[0.0; 6], &[2, 3]));
    let mut host = BufferHost::default();
    for y in 0..2 {
        for x in 0..3 {
            run_device_form_lane(
                &validated,
                MirFunctionId(0),
                &mut host,
                &[tin.clone(), out.clone()],
                &[
                    builtin(2, MirKernelBuiltin::GlobalInvocationY),
                    builtin(3, MirKernelBuiltin::GlobalInvocationX),
                ],
                [x, y, 0],
            )
            .unwrap_or_else(|errors| panic!("lane ({y},{x}): {errors:?}"));
        }
    }
    let expected: Vec<f64> = input.iter().map(|v| v * 2.0).collect();
    assert_eq!(contents(&out), expected);
}

/// The `add` kernel of [`elementwise_add_runs_per_lane_and_matches_host_form`].
fn add_fixture() -> Fixture {
    fixture(&[4], 1, &[0], &[], |_, float| {
        vec![
            assign(
                MirPlace::temp(MirTempId(0)),
                0,
                binary(
                    MirBinOp::Add,
                    MirOperand::Place(indexed(0, local(3))),
                    MirOperand::Place(indexed(1, local(3))),
                ),
                float,
            ),
            assign(
                indexed(2, local(3)),
                1,
                MirValueKind::Operand(temp(0)),
                float,
            ),
        ]
    })
}

#[test]
fn one_runner_over_every_lane_matches_one_lane_runs() {
    let fx = add_fixture();
    let validated = fx.validated();
    let a = [1.0, 2.0, 3.0, 4.0];
    let b = [0.5, 0.25, 8.0, -1.0];
    let (ta, tb) = (tensor(&a, &[4]), tensor(&b, &[4]));
    let builtins = [builtin(3, MirKernelBuiltin::GlobalInvocationX)];
    let mut host = BufferHost::default();

    let single = tensor(&[0.0; 4], &[4]);
    for lane in 0..4 {
        run_device_form_lane(
            &validated,
            MirFunctionId(0),
            &mut host,
            &[ta.clone(), tb.clone(), single.clone()],
            &builtins,
            [lane, 0, 0],
        )
        .unwrap_or_else(|errors| panic!("lane {lane}: {errors:?}"));
    }
    let ranged = tensor(&[0.0; 4], &[4]);
    run_device_form_lanes(
        &validated,
        MirFunctionId(0),
        &mut host,
        &[ta, tb, ranged.clone()],
        &builtins,
        (0..4).map(|lane| [lane, 0, 0]),
    )
    .unwrap_or_else(|failure| panic!("ranged run: {failure:?}"));

    assert_eq!(contents(&ranged), contents(&single));
    let expected: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
    assert_eq!(contents(&ranged), expected);
}

#[test]
fn a_failing_lane_stops_the_run_and_is_named() {
    let fx = add_fixture();
    let validated = fx.validated();
    let out = tensor(&[0.0; 4], &[4]);
    let mut host = BufferHost::default();
    // Lane 4 indexes past the 4-element tensors.
    let failure = run_device_form_lanes(
        &validated,
        MirFunctionId(0),
        &mut host,
        &[
            tensor(&[1.0, 2.0, 3.0, 4.0], &[4]),
            tensor(&[1.0; 4], &[4]),
            out.clone(),
        ],
        &[builtin(3, MirKernelBuiltin::GlobalInvocationX)],
        (0..6).map(|lane| [lane, 0, 0]),
    )
    .expect_err("lane 4 is out of range");
    assert_eq!(failure.lane, Some([4, 0, 0]));
    assert_ne!(
        failure.errors.len(),
        0,
        "the failing lane carries its errors"
    );
    // Lanes before the failure ran and wrote.
    assert_eq!(contents(&out), vec![2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn a_setup_failure_names_no_lane() {
    let fx = add_fixture();
    let validated = fx.validated();
    let mut host = BufferHost::default();
    let failure = run_device_form_lanes(
        &validated,
        MirFunctionId(0),
        &mut host,
        &[tensor(&[1.0; 4], &[4])],
        &[builtin(3, MirKernelBuiltin::GlobalInvocationX)],
        (0..4).map(|lane| [lane, 0, 0]),
    )
    .expect_err("two of three tensor arguments are missing");
    assert_eq!(failure.lane, None);
}

/// Run the `add` fixture over the given inputs through the prepared entry and
/// return the output tensor's contents.
fn add_prepared(
    prepared: &RunnerPrepared,
    validated: &ValidatedMir<'_>,
    host: &mut BufferHost,
    a: &[f64],
    b: &[f64],
) -> Vec<f64> {
    let out = tensor(&[0.0; 4], &[4]);
    run_device_form_lanes_prepared(
        prepared,
        validated,
        MirFunctionId(0),
        host,
        &[tensor(a, &[4]), tensor(b, &[4]), out.clone()],
        &[builtin(3, MirKernelBuiltin::GlobalInvocationX)],
        (0..4).map(|lane| [lane, 0, 0]),
    )
    .unwrap_or_else(|failure| panic!("prepared run: {failure:?}"));
    contents(&out)
}

#[test]
fn one_prepared_value_serves_repeated_runs_like_the_unprepared_entry() {
    let fx = add_fixture();
    let validated = fx.validated();
    let context = validated.validation();
    let prepared = RunnerPrepared::new(validated.program(), context, context.types.as_table());
    let mut host = BufferHost::default();
    let cases = [
        ([1.0, 2.0, 3.0, 4.0], [0.5, 0.25, 8.0, -1.0]),
        ([10.0, 20.0, 30.0, 40.0], [-1.0, -2.0, -3.0, -4.0]),
    ];
    for (a, b) in cases {
        let expected: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let prepared_out = add_prepared(&prepared, &validated, &mut host, &a, &b);
        let unprepared = tensor(&[0.0; 4], &[4]);
        run_device_form_lanes(
            &validated,
            MirFunctionId(0),
            &mut host,
            &[tensor(&a, &[4]), tensor(&b, &[4]), unprepared.clone()],
            &[builtin(3, MirKernelBuiltin::GlobalInvocationX)],
            (0..4).map(|lane| [lane, 0, 0]),
        )
        .unwrap_or_else(|failure| panic!("unprepared run: {failure:?}"));
        assert_eq!(prepared_out, expected);
        assert_eq!(contents(&unprepared), prepared_out);
    }
}

/// Per-call setup cost, prepared versus rebuilt, on a program padded with many
/// copies of the kernel. Timing only; run with `--ignored --nocapture`.
#[test]
#[ignore = "timing report, not a correctness check"]
fn prepared_setup_is_far_cheaper_than_rebuilding_a_runner() {
    use std::time::Instant;

    let mut fx = add_fixture();
    let template = fx.program.functions[0].clone();
    for id in 1..2000 {
        let mut copy = template.clone();
        copy.id = MirFunctionId(id);
        fx.program.functions.push(copy);
    }
    let validated = fx.validated();
    let context = validated.validation();
    let a = [1.0, 2.0, 3.0, 4.0];
    let b = [0.5, 0.25, 8.0, -1.0];
    let mut host = BufferHost::default();
    let rounds = 50;

    let started = Instant::now();
    let prepared = RunnerPrepared::new(validated.program(), context, context.types.as_table());
    let build_once = started.elapsed();

    let started = Instant::now();
    for _ in 0..rounds {
        add_prepared(&prepared, &validated, &mut host, &a, &b);
    }
    let prepared_per_call = started.elapsed() / rounds;

    let started = Instant::now();
    for _ in 0..rounds {
        let out = tensor(&[0.0; 4], &[4]);
        run_device_form_lanes(
            &validated,
            MirFunctionId(0),
            &mut host,
            &[tensor(&a, &[4]), tensor(&b, &[4]), out],
            &[builtin(3, MirKernelBuiltin::GlobalInvocationX)],
            (0..4).map(|lane| [lane, 0, 0]),
        )
        .unwrap_or_else(|failure| panic!("unprepared run: {failure:?}"));
    }
    let rebuilt_per_call = started.elapsed() / rounds;
    eprintln!(
        "2000 functions: prepare once {build_once:?}; per call prepared {prepared_per_call:?}, rebuilt {rebuilt_per_call:?}"
    );
    assert!(prepared_per_call * 5 < rebuilt_per_call);
}

/// The whole-call form of a kernel with no lane builtin returns the value the
/// body returns (a returned kernel result is the call's output).
#[test]
fn a_whole_call_returns_the_kernel_value() {
    let types = TypeTable::new();
    let int = MirType::semantic(types.primitive(Primitive::Numerus));
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: int,
            mode: MirParamMode::Owned,
            span: span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: int,
            mutable: false,
            span: span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(local(0))),
                span: span(),
            },
            span: span(),
        }],
        return_ty: int,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: span(),
    };
    let fixture = Fixture {
        types,
        interner: Interner::new(),
        program: MirProgram {
            functions: vec![function],
        },
    };
    let validated = fixture.validated();
    let prepared = RunnerPrepared::new(
        validated.program(),
        validated.validation(),
        validated.validation().types.as_table(),
    );
    let mut host = BufferHost::default();
    let value = run_device_form_call_prepared(
        &prepared,
        &validated,
        MirFunctionId(0),
        &mut host,
        &mut [Value::Int(7)],
        &[],
    )
    .expect("the whole call runs");
    assert!(matches!(value, Some(Value::Int(7))), "{value:?}");
}

/// A whole-call kernel `fn k(mut int o)` whose body stores 41 into `o`, then
/// returns, or throws the constant text `boom` when `throws`.
fn rebinding_fixture(throws: bool) -> Fixture {
    let types = TypeTable::new();
    let int = MirType::semantic(types.primitive(Primitive::Numerus));
    let text = MirType::semantic(types.primitive(Primitive::Textus));
    let mut interner = Interner::new();
    let boom = interner.intern("boom");
    let terminator = if throws {
        MirTerminatorKind::ReturnError(MirOperand::Constant(MirConstant::String(boom)))
    } else {
        MirTerminatorKind::Return(None)
    };
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: int,
            mode: MirParamMode::MutRef,
            span: span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: int,
            mutable: true,
            span: span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![assign(
                MirPlace::local(MirLocalId(0)),
                0,
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(41))),
                int,
            )],
            terminator: MirTerminator {
                kind: terminator,
                span: span(),
            },
            span: span(),
        }],
        return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
        error_ty: throws.then_some(text),
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: span(),
    };
    Fixture {
        types,
        interner,
        program: MirProgram {
            functions: vec![function],
        },
    }
}

/// A `mut` parameter is a place of the caller: a body that rebinds it (as
/// `o ⇇ a · b` lowers) leaves the new value in the argument slot.
#[test]
fn a_whole_call_leaves_a_rebound_mut_parameter_in_its_argument() {
    let fixture = rebinding_fixture(false);
    let validated = fixture.validated();
    let prepared = RunnerPrepared::new(
        validated.program(),
        validated.validation(),
        validated.validation().types.as_table(),
    );
    let mut host = BufferHost::default();
    let mut args = [Value::Int(0)];
    run_device_form_call_prepared(
        &prepared,
        &validated,
        MirFunctionId(0),
        &mut host,
        &mut args,
        &[],
    )
    .unwrap_or_else(|failure| panic!("the whole call runs: {failure:?}"));
    assert!(matches!(args[0], Value::Int(41)), "{:?}", args[0]);
}

/// A thrown value is carried structurally for a fault channel to route, and
/// what the body wrote before the throw stays written, as the sequential call
/// leaves it.
#[test]
fn a_thrown_value_is_carried_and_earlier_writes_stay() {
    let fixture = rebinding_fixture(true);
    let validated = fixture.validated();
    let prepared = RunnerPrepared::new(
        validated.program(),
        validated.validation(),
        validated.validation().types.as_table(),
    );
    let mut host = BufferHost::default();
    let mut args = [Value::Int(0)];
    let failure = run_device_form_call_prepared(
        &prepared,
        &validated,
        MirFunctionId(0),
        &mut host,
        &mut args,
        &[],
    )
    .expect_err("the body throws");
    assert_eq!(failure.lane, Some([0, 0, 0]));
    assert!(
        matches!(&failure.raised, Some(Value::Str(text)) if text == "boom"),
        "{:?}",
        failure.raised
    );
    assert!(matches!(args[0], Value::Int(41)), "{:?}", args[0]);
}
