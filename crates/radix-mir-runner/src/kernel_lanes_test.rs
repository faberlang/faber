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

use super::kernel_lanes::run_device_form_lane;
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
