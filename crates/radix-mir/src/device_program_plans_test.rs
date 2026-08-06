use super::*;
use crate::abi::{
    CollectionOpContract, MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature, MirKernelWorkgroupCount,
    MirWorkgroupSize,
};
use crate::device_program::{
    BufferId, BufferIdentity, BufferLifetime, BufferVersion, DeviceProgram, DeviceProgramLifetime,
    DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit, ObservationCadence,
    ResultBuffer,
};
use crate::kernel_plan::{
    MatMulShapes, MatMulSharedMemory, OobPaddingPolicy, ReduceOp, SharedMemoryLayout,
    MATMUL_SHARED_A_BUFFER_NAME, MATMUL_SHARED_B_BUFFER_NAME,
};
use crate::layout::MirTensorStorageLayout;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall, MirStatement, MirStatementKind,
    MirTerminator, MirTerminatorKind, MirType, MirUnOp, MirValue, MirValueId, MirValueKind,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};

fn test_span() -> Span {
    Span::default()
}

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn f64_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F64))
}

fn tensor_ty(types: &mut TypeTable, count: u64) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let len = types.intern_index(IndexExpr::Literal(count));
    MirType::semantic(types.intern(Type::Tensor(f32_ty, len)))
}

/// A rank-2 `tf32[M, N]` tensor type with static literal dims (the matmul
/// operand shape the contract/plan passes read).
fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)))
}

/// A rank-2 `fractus<f64>[M, N]` tensor type — the genuinely f64 operator-graph
/// element that stays fail-closed (S5A-U2: no f64 scalar admits at the device
/// boundary, whatever the tensor graph looks like).
fn rank2_f64_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f64_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f64_ty, shape)))
}

// -----------------------------------------------------------------------
// Signature fixtures (the post-contract shapes `apply_collection_op_contract`
// produces — the shape `CollectionKernelPlan::from_contract` requires).
// -----------------------------------------------------------------------

fn reduction_signature(length: u64, workgroup_x: u32, partials: u64) -> MirKernelSignature {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    MirKernelSignature {
        function: MirFunctionId(0),
        input: MirKernelResource {
            group: 0,
            binding: 0,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: length,
            source_local: Some(MirLocalId(0)),
            source_name: None,
        },
        extra_inputs: Vec::new(),
        output: MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: partials,
            source_local: None,
            source_name: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize {
            x: workgroup_x,
            y: 1,
            z: 1,
        },
        dispatch_size: MirKernelDispatchSize {
            x: length,
            y: 1,
            z: 1,
        },
    }
}

fn matmul_signature(shapes: MatMulShapes, dispatch_x: u64, dispatch_y: u64) -> MirKernelSignature {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    MirKernelSignature {
        function: MirFunctionId(0),
        input: MirKernelResource {
            group: 0,
            binding: 0,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: shapes.m * shapes.k,
            source_local: Some(MirLocalId(0)),
            source_name: None,
        },
        extra_inputs: vec![MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: shapes.k * shapes.n,
            source_local: Some(MirLocalId(1)),
            source_name: None,
        }],
        output: MirKernelResource {
            group: 0,
            binding: 2,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: shapes.m * shapes.n,
            source_local: None,
            source_name: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 8, y: 8, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: dispatch_x,
            y: dispatch_y,
            z: 1,
        },
    }
}

/// A post-contract transpose signature: workgroup
/// `(TRANSPOSE_WORKGROUP_X, 1, 1)`, dispatch `(M·N, 1, 1)`, input and output
/// each `M·N` elements — the shape `apply_collection_op_contract` produces
/// for a rank-2 `[M,N] → [N,M]` transpose kernel (S5-U1).
fn transpose_signature(m: u64, n: u64) -> MirKernelSignature {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    MirKernelSignature {
        function: MirFunctionId(0),
        input: MirKernelResource {
            group: 0,
            binding: 0,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: m * n,
            source_local: Some(MirLocalId(0)),
            source_name: None,
        },
        extra_inputs: Vec::new(),
        output: MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: m * n,
            source_local: None,
            source_name: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize {
            x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
            y: 1,
            z: 1,
        },
        dispatch_size: MirKernelDispatchSize {
            x: m * n,
            y: 1,
            z: 1,
        },
    }
}

/// A generic-1D transformer signature (S6-P1): workgroup `(1,1,1)`,
/// dispatch `(output_count, 1, 1)`, input/output at the flattened counts —
/// the shape the ABI produces for kernels without a collection-op contract
/// (the transformer recipes have none).
fn transformer_signature(input_count: u64, output_count: u64) -> MirKernelSignature {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    MirKernelSignature {
        function: MirFunctionId(0),
        input: MirKernelResource {
            group: 0,
            binding: 0,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: input_count,
            source_local: Some(MirLocalId(0)),
            source_name: None,
        },
        extra_inputs: Vec::new(),
        output: MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: output_count,
            source_local: None,
            source_name: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: output_count,
            y: 1,
            z: 1,
        },
    }
}

// -----------------------------------------------------------------------
// Function fixtures (kernel bodies the typed plan mapping scans).
// -----------------------------------------------------------------------

fn reduction_function(types: &mut TypeTable, length: u64) -> MirFunction {
    let tensor = tensor_ty(types, length);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: tensor,
            mutable: false,
            span: test_span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Sum),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: tensor,
                    },
                },
                span: test_span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

fn matmul_function(types: &mut TypeTable) -> MirFunction {
    let left = rank2_tensor_ty(types, [17, 12]);
    let right = rank2_tensor_ty(types, [12, 33]);
    let out = rank2_tensor_ty(types, [17, 33]);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: left,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: right,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: left,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: right,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(2))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: out,
                    },
                },
                span: test_span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(2),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: out,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

fn elementwise_function(types: &mut TypeTable) -> MirFunction {
    let tensor = tensor_ty(types, 4);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: tensor,
            mutable: false,
            span: test_span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(0),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// Build a kernel function whose body is exactly the given collection-op
/// statements. All params and locals share one tensor type (`length`), so
/// the ABI/plan operand scans resolve them; the function returns the last
/// statement's destination local. This is the fixture builder the multi-op
/// and fail-closed plan tests use (N3.2: a kernel body's plan is decided by
/// its complete collection-op set, never the first matching statement).
fn tensor_kernel_function(
    types: &mut TypeTable,
    length: u64,
    param_count: u32,
    ops: &[(MirCollectionOp, Vec<u32>, u32)],
) -> MirFunction {
    let tensor = tensor_ty(types, length);
    let mut max_local = param_count;
    let mut statements = Vec::with_capacity(ops.len());
    let mut return_local = MirLocalId(0);
    for (op, args, destination) in ops {
        max_local = max_local.max(*destination + 1);
        for &arg in args {
            max_local = max_local.max(arg + 1);
        }
        return_local = MirLocalId(*destination);
        statements.push(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(MirPlace::local(MirLocalId(*destination))),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Collection(*op),
                    args: args
                        .iter()
                        .map(|&local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
                        .collect(),
                    return_ty: tensor,
                },
            },
            span: test_span(),
        });
    }
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: (0..param_count)
            .map(|local| MirParam {
                local: MirLocalId(local),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span: test_span(),
            })
            .collect(),
        locals: (0..max_local)
            .map(|id| MirLocal {
                id: MirLocalId(id),
                name: None,
                ty: tensor,
                mutable: false,
                span: test_span(),
            })
            .collect(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    return_local,
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// `t = mul(x, w); s = sum(t)` — the S3-A2 forward shape (elementwise
/// transform + tree reduction in one body).
fn mul_sum_function(types: &mut TypeTable, length: u64) -> MirFunction {
    tensor_kernel_function(
        types,
        length,
        2,
        &[
            (MirCollectionOp::TensorMul, vec![0, 1], 2),
            (MirCollectionOp::Sum, vec![2], 3),
        ],
    )
}

/// `s = sum(x); m = mean(x); t = s + m; return t` — two DISTINCT live recipe
/// ops in one body (both results are consumed by the trailing add, so
/// neither is dead-code-filtered by the S3-A5 plan scan).
fn sum_mean_function(types: &mut TypeTable, length: u64) -> MirFunction {
    let tensor = tensor_ty(types, length);
    let mut function = tensor_kernel_function(
        types,
        length,
        1,
        &[
            (MirCollectionOp::Sum, vec![0], 1),
            (MirCollectionOp::TensorMean, vec![0], 2),
        ],
    );
    // `t = s + m` — both recipe results become live; return `t`.
    let add = MirValue {
        id: MirValueId(0),
        kind: MirValueKind::Binary {
            op: MirBinOp::Add,
            lhs: MirOperand::Place(MirPlace::local(MirLocalId(1))),
            rhs: MirOperand::Place(MirPlace::local(MirLocalId(2))),
        },
        ty: tensor,
        span: test_span(),
    };
    function.locals.push(MirLocal {
        id: MirLocalId(3),
        name: None,
        ty: tensor,
        mutable: false,
        span: test_span(),
    });
    let block = &mut function.blocks[0];
    block.statements.push(MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(3)),
            value: add,
        },
        span: test_span(),
    });
    block.terminator = MirTerminator {
        kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(3))))),
        span: test_span(),
    };
    function
}

/// `t = mul(x, w); u = softmax(t)` — an elementwise transform feeding the
/// admitted `RowSoftmax` recipe: the body derives the softmax plan (never a
/// silent elementwise floor).
fn mul_softmax_function(types: &mut TypeTable) -> MirFunction {
    tensor_kernel_function(
        types,
        4,
        2,
        &[
            (MirCollectionOp::TensorMul, vec![0, 1], 2),
            (MirCollectionOp::TensorSoftmax, vec![2], 3),
        ],
    )
}

/// `t = crux_entropia(x, y)` — a genuinely unplannable op (no recipe, not
/// an elementwise transform): the fail-closed example that replaces the
/// former softmax one (S6-P1 admits softmax).
fn crux_entropia_function(types: &mut TypeTable) -> MirFunction {
    tensor_kernel_function(
        types,
        4,
        2,
        &[(MirCollectionOp::TensorCruxEntropia, vec![0, 1], 1)],
    )
}

/// `t = mul(x, w); u = crux_entropia(t, x)` — an elementwise op does not
/// excuse an unplannable sibling statement.
fn mul_crux_entropia_function(types: &mut TypeTable) -> MirFunction {
    tensor_kernel_function(
        types,
        4,
        2,
        &[
            (MirCollectionOp::TensorMul, vec![0, 1], 2),
            (MirCollectionOp::TensorCruxEntropia, vec![2, 0], 3),
        ],
    )
}

/// A single transformer-op kernel over a rank-2 `[dims]` tensor with const
/// trailing operands (the S6-P1 admission fixtures): `t = op(x, consts…)`.
fn rank2_const_op_function(
    types: &mut TypeTable,
    dims: [u64; 2],
    op: MirCollectionOp,
    consts: Vec<MirConstant>,
) -> MirFunction {
    let r2 = rank2_tensor_ty(types, dims);
    let mut args = vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))];
    args.extend(consts.into_iter().map(MirOperand::Constant));
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: r2,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: r2,
            mutable: false,
            span: test_span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(op),
                        args,
                        return_ty: r2,
                    },
                },
                span: test_span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: r2,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A rank-2 softmax kernel: `t = softmax(x)` over `[4,4]`.
fn softmax_rank2_function(types: &mut TypeTable) -> MirFunction {
    rank2_const_op_function(types, [4, 4], MirCollectionOp::TensorSoftmax, vec![])
}

/// A rank-2 axis-sum kernel: `t = sum_axis(x, 1)` over `[4,4]`.
fn sum_axis_function(types: &mut TypeTable) -> MirFunction {
    rank2_const_op_function(
        types,
        [4, 4],
        MirCollectionOp::TensorSumAxis,
        vec![MirConstant::Int(1)],
    )
}

/// A rank-2 layer-norm kernel: `t = layernorm(x, 1, 1e-5)` over `[4,4]`.
fn layer_norm_function(types: &mut TypeTable) -> MirFunction {
    rank2_const_op_function(
        types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![MirConstant::Int(1), MirConstant::Float(1e-5)],
    )
}

/// `t = transpose(x)` — the S5-U1 transpose-bearing kernel shape over a
/// rank-2 `[4,4]` operand (the MLP 4×4 matmul-VJP transpose).
fn transpose_rank2_function(types: &mut TypeTable) -> MirFunction {
    let tensor = rank2_tensor_ty(types, [4, 4]);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: tensor,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorTranspose),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: tensor,
                    },
                },
                span: test_span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// `t = add(x, w); u = mul(x, w)` — elementwise-only multi-op body.
fn add_mul_function(types: &mut TypeTable) -> MirFunction {
    tensor_kernel_function(
        types,
        4,
        2,
        &[
            (MirCollectionOp::TensorAdd, vec![0, 1], 2),
            (MirCollectionOp::TensorMul, vec![0, 1], 3),
        ],
    )
}

/// The `mlp_backward` companion shape (S5-W-A1): `t = x @ w; out = t / s`
/// — a `TiledMatMul` recipe whose data-flow set carries a SCALAR upstream
/// seed (`s`) beyond the two recipe operands (`x`, `w`). The seed's type is
/// the parameter: the generated companion types the seed from the primal's
/// scalar return type (f32 in the fixed MLP fixture, f64 in the latent
/// `fractus` variant), so the emission signature must admit the kernel
/// scalar surface and fail closed outside it.
fn matmul_scalar_seed_function(types: &mut TypeTable, seed_ty: MirType) -> MirFunction {
    let tensor = rank2_tensor_ty(types, [4, 4]);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(2),
                name: None,
                ty: seed_ty,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: tensor,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: seed_ty,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                // t = x @ w — the recipe statement whose operand locals bind
                // the contract-shaped inputs (0 and 1).
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(3))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            ],
                            return_ty: tensor,
                        },
                    },
                    span: test_span(),
                },
                // out = t / s — the scalar seed flows into the elementwise
                // div as the subchain's data-flow input.
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(4))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorDiv),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(3))),
                                MirOperand::Place(MirPlace::local(MirLocalId(2))),
                            ],
                            return_ty: tensor,
                        },
                    },
                    span: test_span(),
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(4),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

fn identity(id: u32, name: &str, role: BufferRole) -> BufferIdentity {
    BufferIdentity {
        id: BufferId(id),
        name: name.to_owned(),
        role,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime: BufferLifetime::PerProgram,
    }
}

fn version(number: u32, ty: MirType, count: u64) -> BufferVersion {
    BufferVersion {
        version: number,
        element_ty: ty,
        element_count: count,
    }
}

fn resource(
    identity: BufferIdentity,
    version: BufferVersion,
    binding: u32,
    access: MirKernelResourceAccess,
) -> DeviceResource {
    DeviceResource {
        buffer: identity,
        version,
        binding: Binding { group: 0, binding },
        access,
    }
}

/// The carried plan a materializer stores on a kernel unit, built through the
/// same typed mapping the emitters cross-check against.
fn carried_reduction_plan(length: u64, workgroup_x: u32, partials: u64) -> CollectionKernelPlan {
    let signature = reduction_signature(length, workgroup_x, partials);
    let contract = CollectionOpContract::Reduction {
        length,
        workgroup_x,
        partials,
    };
    plan_for_collection_op(&signature, &contract, MirCollectionOp::Sum, None)
        .expect("Sum under a reduction contract carries a plan")
}

fn elementwise_device_program() -> DeviceProgram {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let input = identity(1, "x", BufferRole::Input);
    let output = identity(2, "y", BufferRole::Output);
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "addita".to_owned(),
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![
            resource(
                input.clone(),
                version(1, element_ty, 4),
                0,
                MirKernelResourceAccess::Read,
            ),
            resource(
                output.clone(),
                version(1, element_ty, 4),
                1,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 8, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 4, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    };
    DeviceProgram {
        kernels: vec![kernel],
        launches: vec![LaunchUnit {
            id: LaunchId(1),
            kernel_index: 0,
        }],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: vec![ResultBuffer {
            buffer: output,
            version: version(1, element_ty, 4),
            role: BufferRole::Output,
            produced_by: LaunchId(1),
            cadence: ObservationCadence::EndOfRun,
        }],
    }
}

// -----------------------------------------------------------------------
// Plan mapping (typed op → plan).
// -----------------------------------------------------------------------

#[test]
fn sum_maps_to_tree_reduction() {
    let signature = reduction_signature(1024, 256, 4);
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    let plan = plan_for_collection_op(&signature, &contract, MirCollectionOp::Sum, None)
        .expect("Sum under a reduction contract carries a plan");
    let CollectionKernelPlan::TreeReduction(reduction) = &plan else {
        panic!("Sum must map to a tree-reduction plan, got {plan:?}");
    };
    assert_eq!(reduction.op, ReduceOp::Sum);
    assert_eq!(reduction.length, 1024);
    assert_eq!(reduction.workgroup_x, 256);
    assert_eq!(reduction.partials, 4);
}

#[test]
fn tensor_mean_maps_to_tree_reduction() {
    let signature = reduction_signature(1024, 256, 4);
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    let plan = plan_for_collection_op(&signature, &contract, MirCollectionOp::TensorMean, None)
        .expect("TensorMean under a reduction contract carries a plan");
    let CollectionKernelPlan::TreeReduction(reduction) = &plan else {
        panic!("TensorMean must map to a tree-reduction plan, got {plan:?}");
    };
    assert_eq!(reduction.op, ReduceOp::Mean);
}

#[test]
fn matmul_maps_to_tiled_matmul() {
    let shapes = MatMulShapes {
        m: 17,
        k: 12,
        n: 33,
    };
    let signature = matmul_signature(shapes, 5, 3);
    let contract = CollectionOpContract::MatMul {
        dispatch_x: 5,
        dispatch_y: 3,
    };
    let plan = plan_for_collection_op(
        &signature,
        &contract,
        MirCollectionOp::TensorMatMul,
        Some(shapes),
    )
    .expect("TensorMatMul under a matmul contract carries a plan");
    let CollectionKernelPlan::TiledMatMul(matmul) = &plan else {
        panic!("TensorMatMul must map to a tiled-matmul plan, got {plan:?}");
    };
    assert_eq!(matmul.m, 17);
    assert_eq!(matmul.k, 12);
    assert_eq!(matmul.n, 33);
    assert_eq!(
        matmul.shared_memory.shared_a.buffer_name,
        MATMUL_SHARED_A_BUFFER_NAME
    );
    assert_eq!(
        matmul.shared_memory.shared_b.buffer_name,
        MATMUL_SHARED_B_BUFFER_NAME
    );
}

#[test]
fn elementwise_op_under_recipe_contract_fails_closed() {
    // N3.2: an elementwise op under a recipe (reduction) contract is a
    // dispatch/op mismatch — the op-level mapping must never fall back to a
    // silent Elementwise. Elementwise kernels are decided by the
    // function-level scan ([`kernel_plan_for_function`]) from the body's
    // complete op set instead.
    let signature = reduction_signature(1024, 256, 4);
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    let error = plan_for_collection_op(&signature, &contract, MirCollectionOp::TensorRelu, None)
        .expect_err("elementwise op under a recipe contract must fail closed");
    assert!(
        error.message.contains("no kernel plan"),
        "expected the typed plan diagnostic, got: {}",
        error.message
    );
}

// -----------------------------------------------------------------------
// Function-level plan re-derivation (the emitters' cross-check input).
// -----------------------------------------------------------------------

#[test]
fn kernel_plan_for_function_derives_reduction_plan() {
    let mut types = TypeTable::new();
    let function = reduction_function(&mut types, 1024);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("reduction function facts resolve")
        .expect("reduction function carries a recipe plan");
    // A carried plan built by the same typed mapping agrees exactly.
    let carried = carried_reduction_plan(1024, 256, 4);
    assert!(carried_plan_agrees(&carried, Some(&derived)));
    assert_eq!(carried, derived);
}

#[test]
fn kernel_plan_for_function_derives_matmul_plan() {
    let mut types = TypeTable::new();
    let function = matmul_function(&mut types);
    let shapes = MatMulShapes {
        m: 17,
        k: 12,
        n: 33,
    };
    let signature = matmul_signature(shapes, 5, 3);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("matmul function facts resolve")
        .expect("matmul function carries a recipe plan");
    assert!(matches!(derived, CollectionKernelPlan::TiledMatMul(_)));
}

#[test]
fn kernel_plan_for_function_is_none_for_elementwise_function() {
    let mut types = TypeTable::new();
    let function = elementwise_function(&mut types);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("elementwise facts resolve");
    assert!(derived.is_none());
    // An elementwise kernel unit carries the no-recipe plan; the agreement
    // predicate matches it to the `None` derivation.
    assert!(carried_plan_agrees(
        &CollectionKernelPlan::Elementwise,
        derived.as_ref()
    ));
    // A carried recipe on a function without one must fail the cross-check.
    let carried = carried_reduction_plan(1024, 256, 4);
    assert!(!carried_plan_agrees(&carried, derived.as_ref()));
}

#[test]
fn carried_plan_agrees_rejects_recipe_mismatch() {
    let mut types = TypeTable::new();
    let function = reduction_function(&mut types, 1024);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let derived =
        kernel_plan_for_function(&function, &signature, &validation).expect("facts resolve");
    // A no-recipe carried plan on a function that carries a recipe is a
    // contradiction (and vice versa).
    assert!(!carried_plan_agrees(
        &CollectionKernelPlan::Elementwise,
        derived.as_ref()
    ));
}

#[test]
fn contract_shape_signature_output_carries_produced_local() {
    // A2-2b: a recipe subchain's contract output records the local its
    // recipe statement produces. Decomposition subchains share the source
    // function's local tables, so the producer's `MirLocal` origin joins the
    // companion consumer (same function, same local) by semantic identity —
    // the forward-save value the companion reads comes from an EARLIER
    // subchain of the same decomposed function.
    let mut types = TypeTable::new();
    let reduction = reduction_function(&mut types, 1024);
    let matmul = matmul_function(&mut types);
    let transpose = transpose_rank2_function(&mut types);
    let validation = MirValidationContext::new(&types);

    let reduction_contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    let reduction_signature =
        contract_shape_signature(&reduction, &reduction_contract, &validation)
            .expect("reduction contract shape resolves");
    assert_eq!(
        reduction_signature.output.source_local,
        Some(MirLocalId(1)),
        "reduction output must carry the recipe statement's destination local"
    );

    let matmul_contract = CollectionOpContract::MatMul {
        dispatch_x: 5,
        dispatch_y: 3,
    };
    let matmul_signature = contract_shape_signature(&matmul, &matmul_contract, &validation)
        .expect("matmul contract shape resolves");
    assert_eq!(
        matmul_signature.output.source_local,
        Some(MirLocalId(2)),
        "matmul output must carry the recipe statement's destination local"
    );

    let transpose_contract = CollectionOpContract::Transpose { m: 4, n: 4 };
    let transpose_signature =
        contract_shape_signature(&transpose, &transpose_contract, &validation)
            .expect("transpose contract shape resolves");
    assert_eq!(
        transpose_signature.output.source_local,
        Some(MirLocalId(1)),
        "transpose output must carry the recipe statement's destination local"
    );
}

#[test]
fn contract_shape_signature_output_stays_synthetic_without_local_destination() {
    // A2-2b fallback: when the recipe statement's destination is not a plain
    // local (a temp), the contract output keeps `source_local: None` and the
    // Synthetic origin fallback stays honest.
    let mut types = TypeTable::new();
    let tensor = tensor_ty(&mut types, 1024);
    let validation = MirValidationContext::new(&types);
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: tensor,
            mutable: false,
            span: test_span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::temp(crate::MirTempId(0))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Sum),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: tensor,
                    },
                },
                span: test_span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(0),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    };
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    let signature = contract_shape_signature(&function, &contract, &validation)
        .expect("reduction contract shape resolves");
    assert_eq!(
        signature.output.source_local, None,
        "a non-local destination must leave the Synthetic origin fallback intact"
    );
}

// -----------------------------------------------------------------------
// Fail-closed plan surface (N3.2: no plan is an error, never Elementwise).
// -----------------------------------------------------------------------

#[test]
fn unplannable_collection_op_fails_closed_with_typed_diagnostic() {
    // S6-P1 admission test replacement: a device-routed function emitting
    // the GENUINELY unplannable TensorCruxEntropia (no recipe, not an
    // elementwise transform) must fail with the typed diagnostic, never a
    // silent Elementwise. (The former fixture used TensorSoftmax, which
    // S6-P1 promotes to the RowSoftmax recipe.)
    let mut types = TypeTable::new();
    let function = crux_entropia_function(&mut types);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let error = kernel_plan_for_function(&function, &signature, &validation)
        .expect_err("TensorCruxEntropia must fail the plan pass");
    assert!(
        error.message.contains("TensorCruxEntropia"),
        "diagnostic must name the op, got: {}",
        error.message
    );
    assert!(
        error.message.contains("no kernel plan"),
        "diagnostic must say no kernel plan, got: {}",
        error.message
    );
}

// -----------------------------------------------------------------------
// S6-P1 transformer admission (TensorSumAxis / softmax / layer-norm resolve
// a single carried recipe; re-derivation agrees with carried plans).
// -----------------------------------------------------------------------

#[test]
fn softmax_function_resolves_row_softmax_plan() {
    // A softmax-bearing body resolves the RowSoftmax recipe over the last
    // (row) axis — S6-P1 closes the N3.2 fail-closed for this op.
    let mut types = TypeTable::new();
    let function = softmax_rank2_function(&mut types);
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("softmax function facts resolve")
        .expect("softmax function carries a recipe plan");
    let CollectionKernelPlan::RowSoftmax(plan) = &derived else {
        panic!("softmax body must derive a row-softmax plan, got {derived:?}");
    };
    assert_eq!(plan.axis, 1, "row softmax normalizes over the last axis");
    derived.validate().expect("the derived plan validates");
    // A carried plan built from the same typed facts agrees exactly.
    let carried = CollectionKernelPlan::RowSoftmax(crate::kernel_plan::RowSoftmaxPlan { axis: 1 });
    assert!(carried_plan_agrees(&carried, Some(&derived)));
}

#[test]
fn sum_axis_function_resolves_axis_reduction_plan() {
    // A TensorSumAxis-bearing body resolves the AxisReduction recipe with
    // `op: Sum` and the axis from the trailing const operand.
    let mut types = TypeTable::new();
    let function = sum_axis_function(&mut types);
    let signature = transformer_signature(16, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("sum_axis function facts resolve")
        .expect("sum_axis function carries a recipe plan");
    let CollectionKernelPlan::AxisReduction(plan) = &derived else {
        panic!("sum_axis body must derive an axis-reduction plan, got {derived:?}");
    };
    assert_eq!(plan.op, crate::kernel_plan::ReduceOp::Sum);
    assert_eq!(plan.axis, 1, "axis comes from the trailing const operand");
    derived.validate().expect("the derived plan validates");
    // A carried plan built from the same typed facts agrees exactly.
    let carried = CollectionKernelPlan::AxisReduction(crate::kernel_plan::AxisReductionPlan {
        op: crate::kernel_plan::ReduceOp::Sum,
        axis: 1,
    });
    assert!(carried_plan_agrees(&carried, Some(&derived)));
}

/// The keep-dims `SumAxis` subchain shape (mirrors `bert_tiny_backward__13`):
/// `t = sum_axis(x, 1); u = t / divisor; v = fill(x, u); w = x - v;
/// q = w * w; return (t, w, q)` over `[2,8]` tensors. The recipe output is
/// the REDUCED `[2]` result while the folded elementwise tail produces the
/// full-shape `[2,8]` consumed outputs.
fn keep_dims_sum_axis_tail_function(types: &mut TypeTable) -> MirFunction {
    let tensor = rank2_tensor_ty(types, [2, 8]);
    let tuple = MirType::semantic(types.intern(Type::Tuple(vec![tensor.semantic_id(); 3])));
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(48),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(63),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: (64..69)
            .map(|id| MirLocal {
                id: MirLocalId(id),
                name: None,
                ty: tensor,
                mutable: false,
                span: test_span(),
            })
            .chain(std::iter::once(MirLocal {
                id: MirLocalId(69),
                name: None,
                ty: tuple,
                mutable: false,
                span: test_span(),
            }))
            .collect(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(64))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSumAxis),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(48))),
                                MirOperand::Constant(MirConstant::Int(1)),
                            ],
                            return_ty: tensor,
                        },
                    },
                    span: test_span(),
                },
                tensor_kernel_collection_call(MirCollectionOp::TensorDiv, &[64, 63], 65, tensor),
                tensor_kernel_collection_call(MirCollectionOp::TensorFill, &[48, 65], 66, tensor),
                tensor_kernel_collection_call(MirCollectionOp::TensorSub, &[48, 66], 67, tensor),
                tensor_kernel_collection_call(MirCollectionOp::TensorMul, &[67, 67], 68, tensor),
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::local(MirLocalId(69)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: tuple,
                            fields: MirAggregateFields::Ordered(
                                [64u32, 67, 68]
                                    .into_iter()
                                    .map(|id| {
                                        MirAggregateItem::Operand(MirOperand::Place(
                                            MirPlace::local(MirLocalId(id)),
                                        ))
                                    })
                                    .collect(),
                            ),
                        },
                    },
                    span: test_span(),
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(69),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tuple,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A local-destination collection-call statement (the test fixtures'
/// elementwise shape).
fn tensor_kernel_collection_call(
    op: MirCollectionOp,
    args: &[u32],
    destination: u32,
    ty: MirType,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(destination))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(op),
                args: args
                    .iter()
                    .map(|&local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
                    .collect(),
                return_ty: ty,
            },
        },
        span: test_span(),
    }
}

/// R6 (P1) transformer dispatch: a keep-dims `SumAxis` subchain whose folded
/// elementwise tail carries full-shape consumed outputs dispatches over the
/// UNION of the output extents — the reduced `[2]` recipe output plus the
/// `[2,8]` (16-element) tail outputs — so no tail element is left
/// unwritten. Same-shape recipes (softmax / layer-norm) keep their
/// full-shape dispatch.
#[test]
fn transformer_subchain_dispatch_covers_the_union_of_output_extents() {
    let mut types = TypeTable::new();
    let function = keep_dims_sum_axis_tail_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let signature = transformer_subchain_signature_for_emission(
        &function,
        MirCollectionOp::TensorSumAxis,
        &validation,
        &[MirLocalId(64), MirLocalId(67), MirLocalId(68)],
    )
    .expect("keep-dims SumAxis subchain signature resolves");
    assert_eq!(
        signature.dispatch_size.x, 16,
        "the SumAxis subchain must dispatch over the union (max of the \
         reduced [2] recipe output and the [2,8] tail outputs), got {:?}",
        signature.dispatch_size
    );
    assert_eq!(
        signature.output.element_count, 2,
        "the recipe output keeps the SumAxis-aware reduced count"
    );
    assert_eq!(
        signature.extra_outputs.len(),
        2,
        "the folded tail's consumed outputs stay bound as extra outputs"
    );
    for extra in &signature.extra_outputs {
        assert_eq!(
            extra.element_count, 16,
            "the tail outputs keep their full shape"
        );
    }

    // A same-shape transformer recipe (softmax) keeps its full-shape
    // dispatch — the union equals the recipe output count.
    let mut softmax_types = TypeTable::new();
    let softmax = softmax_rank2_function(&mut softmax_types);
    let softmax_validation = MirValidationContext::new(&softmax_types);
    let softmax_signature = transformer_subchain_signature_for_emission(
        &softmax,
        MirCollectionOp::TensorSoftmax,
        &softmax_validation,
        &[MirLocalId(1)],
    )
    .expect("softmax subchain signature resolves");
    assert_eq!(
        softmax_signature.dispatch_size.x, 16,
        "a same-shape recipe's union dispatch equals its full output count"
    );
}

/// The whole-function plan gate: a kernel signature whose storage-buffer
/// count exceeds [`MAX_STORAGE_BUFFERS_PER_KERNEL`] (Metal's `[[buffer(N)]]`
/// maximum, 31) is refused with a typed diagnostic so the callers fall
/// through to the decomposition split — never a silent Elementwise floor.
#[test]
fn kernel_plan_refuses_signature_over_the_buffer_binding_maximum() {
    let mut types = TypeTable::new();
    let function = elementwise_function(&mut types);
    let mut signature = reduction_signature(4, 1, 1);
    // Bloat the signature to 32 storage buffers: the input + output already
    // occupy bindings 0 and 1; 30 extra inputs push past the maximum.
    let element_ty = signature.input.element_ty;
    for binding in 2..32u32 {
        signature.extra_inputs.push(MirKernelResource {
            group: 0,
            binding,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: 4,
            source_local: None,
            source_name: None,
        });
    }
    let validation = MirValidationContext::new(&types);
    let error = kernel_plan_for_function(&function, &signature, &validation)
        .expect_err("a signature over the binding maximum must be refused");
    assert!(
        error.message.contains("binding maximum"),
        "the diagnostic must name the budget, got: {}",
        error.message
    );
}

#[test]
fn layer_norm_function_resolves_layer_normalization_plan() {
    // A layer-norm-bearing body resolves the LayerNormalization recipe with
    // the axis (and epsilon) from the const operands.
    let mut types = TypeTable::new();
    let function = layer_norm_function(&mut types);
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("layer-norm function facts resolve")
        .expect("layer-norm function carries a recipe plan");
    let CollectionKernelPlan::LayerNormalization(plan) = &derived else {
        panic!("layer-norm body must derive a layer-normalization plan, got {derived:?}");
    };
    assert_eq!(plan.axis, 1, "axis comes from the const operands");
    derived.validate().expect("the derived plan validates");
    // A carried plan built from the same typed facts agrees exactly.
    let carried =
        CollectionKernelPlan::LayerNormalization(crate::kernel_plan::LayerNormalizationPlan {
            axis: 1,
        });
    assert!(carried_plan_agrees(&carried, Some(&derived)));
}

#[test]
fn elementwise_op_feeding_softmax_derives_row_softmax_plan() {
    // N3.2: an elementwise transform (mul) feeding the admitted softmax
    // recipe still derives the softmax plan from the complete op set — the
    // former "an elementwise op does not excuse an unplannable sibling"
    // boundary is now an admission boundary.
    let mut types = TypeTable::new();
    let function = mul_softmax_function(&mut types);
    let signature = transformer_signature(4, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("mul+softmax body facts resolve")
        .expect("mul+softmax body carries the softmax recipe");
    let CollectionKernelPlan::RowSoftmax(plan) = &derived else {
        panic!("mul+softmax body must derive a row-softmax plan, got {derived:?}");
    };
    assert_eq!(plan.axis, 0, "rank-1 [4] softmax is over axis 0");
}

#[test]
fn transformer_recipe_kinds_are_typed() {
    // The RecipeKind tag mirrors the new plan variants (hosts and backends
    // switch on the tag for launch semantics).
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::AxisReduction(
            crate::kernel_plan::AxisReductionPlan {
                op: crate::kernel_plan::ReduceOp::Sum,
                axis: 1,
            }
        )),
        RecipeKind::AxisReduction
    );
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::RowSoftmax(
            crate::kernel_plan::RowSoftmaxPlan { axis: 1 }
        )),
        RecipeKind::RowSoftmax
    );
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::LayerNormalization(
            crate::kernel_plan::LayerNormalizationPlan { axis: 1 }
        )),
        RecipeKind::LayerNormalization
    );
}

#[test]
fn transformer_plans_report_zero_shared_memory() {
    // The transformer recipes declare no shared-memory layout (each row is
    // one thread's serial pass), so the launch descriptor reports a zero
    // workgroup-memory footprint.
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let input = identity(1, "x", BufferRole::Input);
    let output = identity(2, "y", BufferRole::Output);
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "softmaxa".to_owned(),
        plan: CollectionKernelPlan::RowSoftmax(crate::kernel_plan::RowSoftmaxPlan { axis: 1 }),
        resources: vec![
            resource(
                input,
                version(1, element_ty, 16),
                0,
                MirKernelResourceAccess::Read,
            ),
            resource(
                output,
                version(1, element_ty, 16),
                1,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 16, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 16, y: 1, z: 1 },
        },
    };
    let descriptor = KernelLaunchDescriptor::from_kernel_unit(&kernel, 0);
    assert_eq!(descriptor.recipe, RecipeKind::RowSoftmax);
    assert_eq!(descriptor.shared_memory_bytes, 0);
}

#[test]
fn tensor_unary_admission_declares_exp_and_sqrt() {
    // S6-P1: the MIR-level tensor-unary admission declares Exp/Sqrt (the
    // softmax exp and the LayerNorm sqrt lanes) alongside the pre-existing
    // elementwise tensor unaries — the emitter dispatch seam P2/P3 consume.
    assert!(is_admitted_tensor_unary(MirUnOp::Exp));
    assert!(is_admitted_tensor_unary(MirUnOp::Sqrt));
    assert!(is_admitted_tensor_unary(MirUnOp::Tanh));
    assert!(is_admitted_tensor_unary(MirUnOp::Relu));
    assert!(is_admitted_tensor_unary(MirUnOp::Gelu));
    // A unary outside the admitted set has no elementwise arm.
    assert!(!is_admitted_tensor_unary(MirUnOp::Log));
    assert!(!is_admitted_tensor_unary(MirUnOp::Neg));
}

#[test]
fn transformer_subchain_signature_binds_operand_and_output() {
    // The S6-P1 emission seam: a transformer subchain's emission signature
    // binds the recipe operand as the read input, sizes the output from the
    // flattened output dims, and carries the recipe destination as the
    // output source local.
    let mut types = TypeTable::new();
    let function = sum_axis_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let signature = transformer_subchain_signature_for_emission(
        &function,
        MirCollectionOp::TensorSumAxis,
        &validation,
        &[MirLocalId(1)],
    )
    .expect("sum_axis subchain emission signature resolves");
    assert_eq!(signature.input.source_local, Some(MirLocalId(0)));
    assert_eq!(signature.input.element_count, 16);
    assert_eq!(signature.output.source_local, Some(MirLocalId(1)));
    assert_eq!(
        signature.output.element_count, 4,
        "sum over axis 1 of [4,4] yields 4 elements"
    );
    assert_eq!(signature.output.binding, 1);
    assert!(signature.extra_inputs.is_empty());
    assert!(signature.extra_outputs.is_empty());
    // The transformer-shaped signature carries the recipe's counts, so the
    // shared plan pass resolves the same plan from it.
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorSumAxis,
    )
    .expect("the transformer-shaped signature resolves the carried plan");
    let carried = CollectionKernelPlan::AxisReduction(crate::kernel_plan::AxisReductionPlan {
        op: crate::kernel_plan::ReduceOp::Sum,
        axis: 1,
    });
    assert!(carried_plan_agrees(&carried, Some(&plan)));
}

#[test]
fn kernel_plan_for_function_derives_transpose_plan() {
    // S5-U1 admission: a transpose-bearing body resolves the rank-2
    // Transpose recipe over the contract-shaped signature.
    let mut types = TypeTable::new();
    let function = transpose_rank2_function(&mut types);
    let signature = transpose_signature(4, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("transpose function facts resolve")
        .expect("transpose function carries a recipe plan");
    let CollectionKernelPlan::Transpose(plan) = &derived else {
        panic!("transpose body must derive a transpose plan, got {derived:?}");
    };
    assert_eq!((plan.m, plan.n), (4, 4));
    assert_eq!(plan.dispatch_x, 16);
    derived.validate().expect("transpose plan validates");
    // A carried transpose plan built by the same typed mapping agrees.
    let carried = CollectionKernelPlan::Transpose(crate::kernel_plan::TransposePlan {
        m: 4,
        n: 4,
        workgroup_x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
        dispatch_x: 16,
    });
    assert!(carried_plan_agrees(&carried, Some(&derived)));
}

#[test]
fn transpose_plan_is_recipe_kind_transpose() {
    // The RecipeKind tag mirrors the new plan variant (hosts and backends
    // switch on the tag for launch semantics).
    let mut types = TypeTable::new();
    let function = transpose_rank2_function(&mut types);
    let signature = transpose_signature(4, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("transpose facts resolve")
        .expect("transpose carries a recipe plan");
    assert_eq!(RecipeKind::from_plan(&derived), RecipeKind::Transpose);
}

#[test]
fn multi_op_body_mixed_elementwise_and_recipe_derives_recipe_plan() {
    // N3.2: a body mixing an elementwise transform (mul) with a recipe op
    // (Sum) is scanned across ALL statements — the derived plan is the
    // recipe. The old single-op scan saw only the first statement (mul) and
    // silently floored the reduction to Elementwise; the full scan fixes
    // that false floor.
    let mut types = TypeTable::new();
    let function = mul_sum_function(&mut types, 1024);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("mul+sum body facts resolve")
        .expect("mul+sum body carries the reduction recipe");
    let CollectionKernelPlan::TreeReduction(reduction) = &derived else {
        panic!("mul+sum body must derive a tree-reduction plan, got {derived:?}");
    };
    assert_eq!(reduction.op, ReduceOp::Sum);
    assert_eq!(reduction.length, 1024);
}

#[test]
fn multi_op_body_conflicting_recipes_fail_closed() {
    // Two distinct recipe ops in one body cannot share one carried plan —
    // reject with a typed diagnostic, never guessed.
    let mut types = TypeTable::new();
    let function = sum_mean_function(&mut types, 1024);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let error = kernel_plan_for_function(&function, &signature, &validation)
        .expect_err("conflicting recipes must be rejected");
    assert!(
        error.message.contains("conflicting recipes"),
        "expected the conflicting-recipes diagnostic, got: {}",
        error.message
    );
}

#[test]
fn multi_op_body_with_unplannable_op_fails_closed() {
    // An elementwise sibling does not excuse an unplannable statement — the
    // full scan still rejects it. (The former fixture used
    // mul+softmax, which S6-P1 now admits as a softmax-bearing kernel, so
    // the genuinely-unplannable TensorCruxEntropia carries the boundary.)
    let mut types = TypeTable::new();
    let function = mul_crux_entropia_function(&mut types);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let error = kernel_plan_for_function(&function, &signature, &validation)
        .expect_err("an elementwise op does not excuse an unplannable op");
    assert!(
        error.message.contains("TensorCruxEntropia"),
        "diagnostic must name the unplannable op, got: {}",
        error.message
    );
}

#[test]
fn elementwise_only_multi_op_body_is_elementwise() {
    // A body whose ops are ALL elementwise transforms carries no recipe: the
    // scan decides `None` (elementwise), the typed decision — not a
    // fallback.
    let mut types = TypeTable::new();
    let function = add_mul_function(&mut types);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("elementwise-only body facts resolve");
    assert!(
        derived.is_none(),
        "an elementwise-only body carries no recipe"
    );
    assert!(carried_plan_agrees(
        &CollectionKernelPlan::Elementwise,
        derived.as_ref()
    ));
}

// -----------------------------------------------------------------------
// Launch-descriptor construction.
// -----------------------------------------------------------------------

#[test]
fn elementwise_kernel_descriptor_carries_typed_facts() {
    let program = elementwise_device_program();
    let descriptors = launch_descriptors(&program);
    assert_eq!(descriptors.len(), 1);
    let descriptor = &descriptors[0];
    assert_eq!(descriptor.kernel_index, 0);
    assert_eq!(descriptor.entry, "addita");
    assert_eq!(descriptor.recipe, RecipeKind::Elementwise);
    assert_eq!(descriptor.workgroup, MirWorkgroupSize { x: 8, y: 1, z: 1 });
    assert_eq!(
        descriptor.dispatch,
        MirKernelDispatchSize { x: 4, y: 1, z: 1 }
    );
    assert_eq!(
        descriptor.workgroup_count,
        MirKernelWorkgroupCount { x: 1, y: 1, z: 1 }
    );
    assert_eq!(descriptor.shared_memory_bytes, 0);
    assert_eq!(descriptor.slots.len(), 2);
    assert_eq!(
        descriptor.slots[0].binding,
        Binding {
            group: 0,
            binding: 0
        }
    );
    assert_eq!(descriptor.slots[0].element_count, 4);
    assert_eq!(descriptor.slots[0].role, BufferRole::Input);
    assert_eq!(
        descriptor.slots[1].binding,
        Binding {
            group: 0,
            binding: 1
        }
    );
    assert_eq!(descriptor.slots[1].role, BufferRole::Output);
}

#[test]
fn reduction_kernel_descriptor_reports_recipe_and_shared_memory() {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let input = identity(1, "summa_in", BufferRole::Input);
    let output = identity(2, "summa_out", BufferRole::Output);
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "summa".to_owned(),
        plan: carried_reduction_plan(1024, 256, 4),
        resources: vec![
            resource(
                input,
                version(1, element_ty, 1024),
                0,
                MirKernelResourceAccess::Read,
            ),
            resource(
                output,
                version(1, element_ty, 4),
                1,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 256, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize {
                x: 1024,
                y: 1,
                z: 1,
            },
            workgroup_count: MirKernelWorkgroupCount { x: 4, y: 1, z: 1 },
        },
    };
    let descriptor = KernelLaunchDescriptor::from_kernel_unit(&kernel, 0);
    assert_eq!(descriptor.entry, "summa");
    assert_eq!(descriptor.recipe, RecipeKind::TreeReduction);
    assert_eq!(descriptor.shared_memory_bytes, 256 * 4);
    assert_eq!(
        descriptor.workgroup_count,
        MirKernelWorkgroupCount { x: 4, y: 1, z: 1 }
    );
    assert_eq!(descriptor.slots.len(), 2);
}

#[test]
fn matmul_kernel_descriptor_reports_tiled_matmul_recipe() {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let left = identity(1, "left", BufferRole::Input);
    let right = identity(2, "right", BufferRole::Input);
    let out = identity(3, "out", BufferRole::Output);
    let tile = 8;
    let tile_slots = tile * tile;
    let matmul_plan = crate::kernel_plan::MatMulPlan {
        m: 17,
        k: 12,
        n: 33,
        tile,
        workgroup_x: tile,
        workgroup_y: tile,
        shared_memory: MatMulSharedMemory {
            shared_a: SharedMemoryLayout {
                element_byte_width: 4,
                slot_count: tile_slots,
                buffer_name: MATMUL_SHARED_A_BUFFER_NAME.to_owned(),
            },
            shared_b: SharedMemoryLayout {
                element_byte_width: 4,
                slot_count: tile_slots,
                buffer_name: MATMUL_SHARED_B_BUFFER_NAME.to_owned(),
            },
        },
        barriers: Vec::new(),
        oob_padding: OobPaddingPolicy::ZeroFill,
    };
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "multiplico".to_owned(),
        plan: CollectionKernelPlan::TiledMatMul(matmul_plan),
        resources: vec![
            resource(
                left,
                version(1, element_ty, 17 * 12),
                0,
                MirKernelResourceAccess::Read,
            ),
            resource(
                right,
                version(1, element_ty, 12 * 33),
                1,
                MirKernelResourceAccess::Read,
            ),
            resource(
                out,
                version(1, element_ty, 17 * 33),
                2,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 8, y: 8, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 5, y: 3, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 5, y: 3, z: 1 },
        },
    };
    let descriptor = KernelLaunchDescriptor::from_kernel_unit(&kernel, 0);
    assert_eq!(descriptor.recipe, RecipeKind::TiledMatMul);
    // Two `tile × tile` operand tiles: 2 × (64 slots × 4 bytes).
    assert_eq!(descriptor.shared_memory_bytes, 2 * 64 * 4);
    assert_eq!(descriptor.slots.len(), 3);
}

// -----------------------------------------------------------------------
// Subchain emission signatures (S5-W-A1: scalar data-flow admission).
// -----------------------------------------------------------------------

#[test]
fn f32_scalar_data_flow_param_resolves_in_subchain_emission_signature() {
    // S5-W-A1: the `mlp_backward` companion's recipe subchains carry a
    // SCALAR upstream seed (typed from the primal's f32 return) as a
    // data-flow param beyond the recipe operands. The emission signature
    // must bind it as a 1-element read input at the scalar's own type —
    // the pre-A1 blocker was `recipe operand requires a tensor type` on
    // EVERY non-operand param.
    let mut types = TypeTable::new();
    let seed_ty = f32_ty(&mut types);
    let function = matmul_scalar_seed_function(&mut types, seed_ty);
    let validation = MirValidationContext::new(&types);
    let contract = CollectionOpContract::MatMul {
        dispatch_x: 1,
        dispatch_y: 1,
    };

    let signature = subchain_signature_for_emission(&function, &contract, &validation, &[])
        .expect("a scalar f32 data-flow param resolves in the emission signature");

    // Recipe operands bind first (input 0 = x, extra input 1 = w).
    assert_eq!(signature.input.source_local, Some(MirLocalId(0)));
    assert_eq!(signature.extra_inputs.len(), 2, "w plus the scalar seed");
    assert_eq!(signature.extra_inputs[0].source_local, Some(MirLocalId(1)));
    // The f32 scalar seed binds as a 1-element read input at binding 2.
    let seed = &signature.extra_inputs[1];
    assert_eq!(seed.source_local, Some(MirLocalId(2)));
    assert_eq!(seed.binding, 2);
    assert_eq!(
        seed.element_ty,
        f32_ty(&mut types),
        "the scalar binds at its own f32 type"
    );
    assert_eq!(
        seed.element_count, 1,
        "a scalar data-flow input is 1 element"
    );
    assert_eq!(seed.role, MirKernelResourceRole::Input);
    assert_eq!(seed.access, MirKernelResourceAccess::Read);
    assert_eq!(
        signature.output.binding, 3,
        "the output binds after the recipe inputs and the scalar seed"
    );
}

#[test]
fn f64_scalar_data_flow_param_fails_closed_in_subchain_emission_signature() {
    // S5A-U2 (CTO correction): an f64 scalar data-flow param (a companion's
    // f64 upstream seed) over an f32 tensor graph FAILS CLOSED with the ABI's
    // typed diagnostic — the former graph-wide "all tensors are f32"
    // narrowing (D-6a) is gone, because graph shape proves nothing about an
    // individual f64 scalar's value. The MLP's scalar-return lane is f32
    // end-to-end now (the fixture returns f32), so the f64 shape is only a
    // regression boundary: no graph configuration may admit an f64 scalar.
    let mut types = TypeTable::new();
    let seed_ty = f64_ty(&mut types);
    let function = matmul_scalar_seed_function(&mut types, seed_ty);
    let validation = MirValidationContext::new(&types);
    let contract = CollectionOpContract::MatMul {
        dispatch_x: 1,
        dispatch_y: 1,
    };

    let error = subchain_signature_for_emission(&function, &contract, &validation, &[])
        .expect_err("an f64 scalar data-flow param must fail closed");
    assert!(
        error.message.contains("kernel scalar layout f64"),
        "the diagnostic must name the unsupported scalar layout, got: {}",
        error.message
    );
}

#[test]
fn f64_scalar_return_local_fails_closed_in_subchain_emission_signature() {
    // S5A-U2: a scalar-return local (the f64 loss local shape) in a
    // subchain's consumed-output set over an f32 tensor graph ALSO fails
    // closed with the typed diagnostic — no f64 scalar admits at the device
    // boundary regardless of the operator graph.
    let mut types = TypeTable::new();
    let seed_ty = f64_ty(&mut types);
    let function = matmul_scalar_seed_function(&mut types, seed_ty);
    let validation = MirValidationContext::new(&types);
    let contract = CollectionOpContract::MatMul {
        dispatch_x: 1,
        dispatch_y: 1,
    };

    // outputs = [recipe result (4), scalar-return local (2)] — the f64 scalar
    // local is an ADDITIONAL consumed output (D-2b-1 shape).
    let error = subchain_signature_for_emission(
        &function,
        &contract,
        &validation,
        &[MirLocalId(4), MirLocalId(2)],
    )
    .expect_err("an f64 scalar-return local must fail closed");

    assert!(
        error.message.contains("kernel scalar layout f64"),
        "the diagnostic must name the unsupported scalar layout, got: {}",
        error.message
    );
}

#[test]
fn f64_scalar_data_flow_param_fails_closed_for_f64_tensor_graph() {
    // The fail-closed boundary on an explicitly f64 operator graph: a
    // subchain whose recipe operand is an f64 tensor, with an f64 scalar
    // seed, keeps the existing fail-closed typed diagnostic — no f64 scalar
    // admits at the device boundary, never a silent truncation.
    let mut types = TypeTable::new();
    let seed_ty = f64_ty(&mut types);
    let function = matmul_f64_tensor_seed_function(&mut types, seed_ty);
    let validation = MirValidationContext::new(&types);
    let contract = CollectionOpContract::MatMul {
        dispatch_x: 1,
        dispatch_y: 1,
    };

    let error = subchain_signature_for_emission(&function, &contract, &validation, &[])
        .expect_err("an f64 scalar over an f64 tensor graph must fail closed");
    assert!(
        error.message.contains("kernel scalar layout f64"),
        "the diagnostic must name the unsupported scalar layout, got: {}",
        error.message
    );
}

/// The [`matmul_scalar_seed_function`] companion shape over F64 tensors — the
/// genuinely f64 graph boundary (S5A-U2: fail-closed regardless of shape).
fn matmul_f64_tensor_seed_function(types: &mut TypeTable, seed_ty: MirType) -> MirFunction {
    let tensor = rank2_f64_tensor_ty(types, [4, 4]);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(2),
                name: None,
                ty: seed_ty,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: tensor,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: seed_ty,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(3))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            ],
                            return_ty: tensor,
                        },
                    },
                    span: test_span(),
                },
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(4))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorDiv),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(3))),
                                MirOperand::Place(MirPlace::local(MirLocalId(2))),
                            ],
                            return_ty: tensor,
                        },
                    },
                    span: test_span(),
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(4),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

// -----------------------------------------------------------------------
// D-2b-1: a subchain's additional consumed outputs carry as `extra_outputs`
// in the emission signature.
// -----------------------------------------------------------------------

/// The D-2b-1 lane: `h1 = matmul(x, w1); t = h1 + h1; h2 = matmul(t, w2)`
/// over 4×4 tensors — subchain 0 folds the elementwise Add and its
/// destination `t` is read by subchain 1, so subchain 0's output set is
/// `[h1, t]` (the recipe result plus the consumed elementwise destination).
fn matmul_with_consumed_elementwise_destination(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let span = test_span();
    let collection = |op: MirCollectionOp, args: &[u32], destination: u32| MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(destination))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(op),
                args: args
                    .iter()
                    .map(|&local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
                    .collect(),
                return_ty: r2,
            },
        },
        span,
    };
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: [0u32, 1, 5]
            .into_iter()
            .map(|id| MirParam {
                local: MirLocalId(id),
                name: None,
                ty: r2,
                mode: MirParamMode::Owned,
                span,
            })
            .collect(),
        locals: [2u32, 3, 6]
            .into_iter()
            .map(|id| MirLocal {
                id: MirLocalId(id),
                name: None,
                ty: r2,
                mutable: false,
                span,
            })
            .collect(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                collection(MirCollectionOp::TensorMatMul, &[0, 1], 2),
                MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::local(MirLocalId(3)),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Binary {
                                op: MirBinOp::Add,
                                lhs: MirOperand::Place(MirPlace::local(MirLocalId(2))),
                                rhs: MirOperand::Place(MirPlace::local(MirLocalId(2))),
                            },
                            ty: r2,
                            span,
                        },
                    },
                    span,
                },
                collection(MirCollectionOp::TensorMatMul, &[3, 5], 6),
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(6),
                )))),
                span,
            },
            span,
        }],
        return_ty: r2,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn subchain_additional_outputs_resolve_in_emission_signature() {
    // D-2b-1: a subchain with `outputs = [primary, extra]` resolves an
    // emission signature whose `extra_outputs` carry the extra local's
    // device-view element type and element count — the tuple-return surface
    // the emitters' multi-output tuple-tail paths consume in D-2b-2.
    let mut types = TypeTable::new();
    let function = matmul_with_consumed_elementwise_destination(&mut types);
    // The tuple type is interned from the SOURCE LOCALS' types (the ids the
    // synthetic's `find_tuple` resolves — each `rank2_tensor_ty` call makes a
    // fresh tensor id, so the fixture's locals are the source of truth).
    let local2_ty = function_local_ty(&function, MirLocalId(2)).expect("h1 is typed");
    let local3_ty = function_local_ty(&function, MirLocalId(3)).expect("t is typed");
    let element_ty = {
        let Type::Tensor(element, _) = types.get(local3_ty.semantic_id()) else {
            panic!("t is a tensor");
        };
        MirType::semantic(*element)
    };
    // The synthetic's tuple return type must be interned before the
    // synthetic is built (the decomposition seam holds only `&TypeTable`,
    // mirroring the reverse-AD `find_tuple` pattern).
    types.tuple(vec![local2_ty.semantic_id(), local3_ty.semantic_id()]);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        crate::kernel_decomposition::decompose_kernel_function(&function, &validation)
            .expect("matmul lane with a consumed elementwise destination decomposes");
    assert_eq!(
        decomposition.subchains[0].outputs,
        vec![MirLocalId(2), MirLocalId(3)],
        "subchain 0 exposes the recipe result h1 and the consumed add destination t"
    );

    let synthetic = decomposition.subchain_function(&function, 0, &types);
    let contract = crate::abi::collection_op_contract(&synthetic, &validation)
        .expect("subchain contract resolves")
        .expect("subchain 0 carries a recipe contract");
    let signature = subchain_signature_for_emission(
        &synthetic,
        &contract,
        &validation,
        &decomposition.subchains[0].outputs,
    )
    .expect("the additional output resolves in the emission signature");

    // The primary output stays the contract-shaped recipe output (h1).
    assert_eq!(signature.output.source_local, Some(MirLocalId(2)));
    assert_eq!(signature.output.binding, 2);
    // The consumed elementwise destination binds as the single extra output.
    assert_eq!(signature.extra_outputs.len(), 1);
    let extra = &signature.extra_outputs[0];
    assert_eq!(extra.source_local, Some(MirLocalId(3)));
    assert_eq!(
        extra.binding, 3,
        "the extra output binds after the recipe inputs and the primary output"
    );
    assert_eq!(extra.role, MirKernelResourceRole::Output);
    assert_eq!(extra.access, MirKernelResourceAccess::Write);
    assert_eq!(
        extra.element_ty, element_ty,
        "the extra output carries the tensor's device-view element type"
    );
    assert_eq!(
        extra.element_count, 16,
        "the 4×4 tensor output is 16 elements"
    );

    // `outputs.len() == 1` keeps the existing single-output shape.
    let single =
        subchain_signature_for_emission(&synthetic, &contract, &validation, &[MirLocalId(2)])
            .expect("a single-output subchain keeps the existing shape");
    assert!(single.extra_outputs.is_empty());
    assert_eq!(single.output.binding, 2);
}
