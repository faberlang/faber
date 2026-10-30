//! Coverage for typed device-program plan mapping and launch descriptors.
//!
//! These tests prove fail-closed recipe mapping, carried-fact agreement, and
//! subchain emission signatures. A folded elementwise prefix must bind the
//! recipe operands, never the first collection statement.

use super::*;
use crate::abi::{
    CollectionOpContract, MatMulDispatch, MirKernelDispatchSize, MirKernelResource,
    MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature,
    MirKernelWorkgroupCount, MirWorkgroupSize, collection_op_contract,
};
use crate::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramError, DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit,
    LaunchId, LaunchUnit, ObservationCadence, ResultBuffer,
};
use crate::kernel_plan::{
    CollectionKernelPlan, MATMUL_SHARED_A_BUFFER_NAME, MATMUL_SHARED_B_BUFFER_NAME, MatMulShapes,
    MatMulSharedMemory, OobPaddingPolicy, ReduceOp, SharedMemoryLayout,
};
use crate::layout::MirTensorStorageLayout;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirLocal,
    MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall, MirStatement,
    MirStatementKind, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType, MirUnOp,
    MirValidationContext, MirValue, MirValueId, MirValueKind,
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

fn rank3_tensor_ty(types: &mut TypeTable, dims: [u64; 3]) -> MirType {
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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
    matmul_function_over(left, right, out)
}

/// A rank-2 `f32` tensor type whose listed axes are run-time extents
/// (`Some(parameter)`) and whose other axes are literal.
fn rank2_tensor_ty_mixed(types: &mut TypeTable, dims: [Result<u64, u32>; 2]) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids = dims
        .iter()
        .map(|&dim| match dim {
            Ok(count) => types.intern_index(IndexExpr::Literal(count)),
            Err(parameter) => types.intern_index(IndexExpr::Runtime(
                radix_types::RuntimeExtentId::ScalarParameter { parameter },
            )),
        })
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)))
}

/// A rank-`N` `f32` tensor type whose listed axes are run-time extents
/// (`Err(parameter)`) and whose other axes are literal — the batched matmul
/// operand shapes (`[B, M, K]`, `[B, K, N]`, `[B, M, N]`).
fn batched_tensor_ty_mixed<const N: usize>(
    types: &mut TypeTable,
    dims: [Result<u64, u32>; N],
) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids = dims
        .iter()
        .map(|&dim| match dim {
            Ok(count) => types.intern_index(IndexExpr::Literal(count)),
            Err(parameter) => types.intern_index(IndexExpr::Runtime(
                radix_types::RuntimeExtentId::ScalarParameter { parameter },
            )),
        })
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)))
}

fn matmul_function_over(left: MirType, right: MirType, out: MirType) -> MirFunction {
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

/// `m1 = matmul(x, w1); m2 = matmul(x, w2); out = add(m1, m2)` — TWO live
/// same-kind recipe statements of identical operand shapes (the trailing add
/// keeps both results live): the deduplicated op scan sees ONE
/// `TensorMatMul` op and would silently adopt one whole-body plan while
/// `kernel_matmul_shapes` reads only the first statement.
fn double_matmul_function(types: &mut TypeTable) -> MirFunction {
    let left = rank2_tensor_ty(types, [17, 12]);
    let right = rank2_tensor_ty(types, [12, 33]);
    let out = rank2_tensor_ty(types, [17, 33]);
    let local = |id: u32, ty: MirType| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: false,
        span: test_span(),
    };
    let matmul = |left: u32, right: u32, destination: u32| MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(destination))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                args: vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(left))),
                    MirOperand::Place(MirPlace::local(MirLocalId(right))),
                ],
                return_ty: out,
            },
        },
        span: test_span(),
    };
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
            MirParam {
                local: MirLocalId(2),
                name: None,
                ty: right,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            local(0, left),
            local(1, right),
            local(2, right),
            local(3, out),
            local(4, out),
            local(5, out),
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                matmul(0, 1, 3),
                matmul(0, 2, 4),
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(5))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(3))),
                                MirOperand::Place(MirPlace::local(MirLocalId(4))),
                            ],
                            return_ty: out,
                        },
                    },
                    span: test_span(),
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(5),
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

/// The exact direct-op parent shape emitted by the Faber lowering. The
/// collection calls write result-specific temps, while the following typed
/// assignments forward those results into the declared stage locals. The
/// result temps use an equivalent alias type, which mirrors the real
/// lowering's distinct call-result and declared-local type ids.
pub(crate) fn composed_mlp_real_lowered_function(types: &mut TypeTable) -> MirFunction {
    let input = rank2_tensor_ty(types, [2, 4]);
    let gate_weights = rank2_tensor_ty(types, [4, 3]);
    let up_weights = rank2_tensor_ty(types, [4, 3]);
    let down_weights = rank2_tensor_ty(types, [3, 5]);
    let gate = rank2_tensor_ty(types, [2, 3]);
    let up = rank2_tensor_ty(types, [2, 3]);
    let activated = rank2_tensor_ty(types, [2, 3]);
    let fused = rank2_tensor_ty(types, [2, 3]);
    let down = rank2_tensor_ty(types, [2, 5]);
    let alias = |name: u32, ty: MirType, types: &mut TypeTable| {
        MirType::semantic(types.intern(Type::Alias(radix_hir::DefId(name), ty.semantic_id())))
    };
    let gate_result = alias(100, gate, types);
    let up_result = alias(101, up, types);
    let activated_result = alias(102, activated, types);
    let fused_result = alias(103, fused, types);
    let down_result = alias(104, down, types);
    let span = test_span();
    let matmul = |left: MirLocalId,
                  right: MirLocalId,
                  destination: MirTempId,
                  return_ty: MirType| MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::temp(destination)),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                args: vec![
                    MirOperand::Place(MirPlace::local(left)),
                    MirOperand::Place(MirPlace::local(right)),
                ],
                return_ty,
            },
        },
        span,
    };
    let forward = |local: MirLocalId, temp: MirTempId, ty: MirType, id: u32| MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(local),
            value: MirValue {
                id: MirValueId(id),
                kind: MirValueKind::Operand(MirOperand::Temp(temp)),
                ty,
                span,
            },
        },
        span,
    };
    let runtime = |op: MirCollectionOp,
                   destination: MirTempId,
                   args: Vec<MirOperand>,
                   return_ty: MirType| MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::temp(destination)),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(op),
                args,
                return_ty,
            },
        },
        span,
    };
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: input,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: gate_weights,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(2),
                name: None,
                ty: up_weights,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(3),
                name: None,
                ty: down_weights,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(4),
                name: None,
                ty: down,
                mode: MirParamMode::MutRef,
                span,
            },
            MirParam {
                local: MirLocalId(5),
                name: None,
                ty: f32_ty(types),
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(6),
                name: None,
                ty: gate,
                mutable: true,
                span,
            },
            MirLocal {
                id: MirLocalId(7),
                name: None,
                ty: up,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(8),
                name: None,
                ty: activated,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(9),
                name: None,
                ty: fused,
                mutable: false,
                span,
            },
        ],
        temps: vec![
            MirTemp {
                id: MirTempId(0),
                ty: gate_result,
                span,
            },
            MirTemp {
                id: MirTempId(1),
                ty: up_result,
                span,
            },
            MirTemp {
                id: MirTempId(2),
                ty: activated,
                span,
            },
            MirTemp {
                id: MirTempId(3),
                ty: activated,
                span,
            },
            MirTemp {
                id: MirTempId(4),
                ty: activated,
                span,
            },
            MirTemp {
                id: MirTempId(5),
                ty: activated_result,
                span,
            },
            MirTemp {
                id: MirTempId(6),
                ty: fused_result,
                span,
            },
            MirTemp {
                id: MirTempId(7),
                ty: down_result,
                span,
            },
        ],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                matmul(MirLocalId(0), MirLocalId(1), MirTempId(0), gate_result),
                forward(MirLocalId(6), MirTempId(0), gate, 0),
                matmul(MirLocalId(0), MirLocalId(2), MirTempId(1), up_result),
                forward(MirLocalId(7), MirTempId(1), up, 1),
                runtime(
                    MirCollectionOp::TensorNeg,
                    MirTempId(2),
                    vec![MirOperand::Place(MirPlace::local(MirLocalId(6)))],
                    activated,
                ),
                MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::temp(MirTempId(3)),
                        value: MirValue {
                            id: MirValueId(2),
                            kind: MirValueKind::Unary {
                                op: MirUnOp::Exp,
                                operand: MirOperand::Temp(MirTempId(2)),
                            },
                            ty: activated,
                            span,
                        },
                    },
                    span,
                },
                runtime(
                    MirCollectionOp::TensorAdd,
                    MirTempId(4),
                    vec![
                        MirOperand::Temp(MirTempId(3)),
                        MirOperand::Constant(MirConstant::Float(1.0)),
                    ],
                    activated,
                ),
                runtime(
                    MirCollectionOp::TensorDiv,
                    MirTempId(5),
                    vec![
                        MirOperand::Place(MirPlace::local(MirLocalId(6))),
                        MirOperand::Temp(MirTempId(4)),
                    ],
                    activated_result,
                ),
                forward(MirLocalId(8), MirTempId(5), activated, 3),
                runtime(
                    MirCollectionOp::TensorMul,
                    MirTempId(6),
                    vec![
                        MirOperand::Place(MirPlace::local(MirLocalId(8))),
                        MirOperand::Place(MirPlace::local(MirLocalId(7))),
                    ],
                    fused_result,
                ),
                forward(MirLocalId(9), MirTempId(6), fused, 4),
                matmul(MirLocalId(9), MirLocalId(3), MirTempId(7), down_result),
                forward(MirLocalId(4), MirTempId(7), down, 5),
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span,
            },
            span,
        }],
        return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
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

/// A rank-2 affine layer-norm kernel: `t = layernorm(x, 1, 1e-5, gamma, beta)`
/// over `[4,4]` with rank-1 `[4]` affine peers — the const layout
/// `[x, axis, eps, gamma, beta]`. The `args[3]` gamma occupies the same
/// operand slot `rope_table_local` reads, so this is the non-RoPE shape the
/// `is_rope()` guard (3933dcd00) exists for.
fn layer_norm_affine_function(types: &mut TypeTable) -> MirFunction {
    let input = rank2_tensor_ty(types, [4, 4]);
    let affine = tensor_ty(types, 4);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: input,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: affine,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(2),
                name: None,
                ty: affine,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: input,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: affine,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: affine,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: input,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(3))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorLayerNorm),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Constant(MirConstant::Int(1)),
                            MirOperand::Constant(MirConstant::Float(1e-5)),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            MirOperand::Place(MirPlace::local(MirLocalId(2))),
                        ],
                        return_ty: input,
                    },
                },
                span: test_span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(3),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: input,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
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
        shape: None,
        reduced_projection: None,
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
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
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

#[test]
fn elementwise_library_selection_uses_typed_operation() {
    let mut program = elementwise_device_program();
    program.kernels[0].entry = "unrelated_symbol".to_owned();
    program.kernels[0].library_operation = crate::device_plan_compiler::LibraryOperation::SwiGlu;
    assert_eq!(
        executor_plan_extension(&program).selections[0]
            .library_entry
            .as_deref(),
        Some("swiglu")
    );

    program.kernels[0].library_operation =
        crate::device_plan_compiler::LibraryOperation::ResidualRmsNorm;
    assert_eq!(
        executor_plan_extension(&program).selections[0]
            .library_entry
            .as_deref(),
        Some("residual")
    );

    program.kernels[0].library_operation = crate::device_plan_compiler::LibraryOperation::Generated;
    assert_eq!(
        executor_plan_extension(&program).selections[0]
            .library_entry
            .as_deref(),
        None
    );
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
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let signature = matmul_signature(shapes, 5, 3);
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 5,
            dispatch_y: 3,
        },
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
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
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
fn carried_resources_agree_accepts_unified_inout_on_coherent_abi() {
    // A unified intermediate carries InOut on slots whose ABI is a coherent
    // (Input, Read) or (Output, Write) pair.
    let signature = reduction_signature(4, 1, 1);
    let carried = vec![
        resource(
            identity(1, "x", BufferRole::InOut),
            version(1, signature.input.element_ty, 4),
            0,
            MirKernelResourceAccess::Read,
        ),
        resource(
            identity(2, "y", BufferRole::InOut),
            version(1, signature.output.element_ty, 1),
            1,
            MirKernelResourceAccess::Write,
        ),
    ];
    assert!(carried_resources_agree(&carried, &signature));
}

#[test]
fn carried_resources_agree_rejects_inout_on_incoherent_abi() {
    // InOut does not paper over an incoherent ABI pair (writing an input).
    let mut signature = reduction_signature(4, 1, 1);
    signature.input.access = MirKernelResourceAccess::Write;
    let carried = vec![
        resource(
            identity(1, "x", BufferRole::InOut),
            version(1, signature.input.element_ty, 4),
            0,
            MirKernelResourceAccess::Write,
        ),
        resource(
            identity(2, "y", BufferRole::Output),
            version(1, signature.output.element_ty, 1),
            1,
            MirKernelResourceAccess::Write,
        ),
    ];
    assert!(!carried_resources_agree(&carried, &signature));
}

#[test]
fn library_dispatch_agreement_admits_extras_on_fresh_bindings() {
    // A fused library dispatch carries host-dispatch resources (the K/V
    // weights and outputs of a QkvProjection launch) on bindings outside
    // the carrier signature.
    let signature = reduction_signature(4, 1, 1);
    let mut carried = vec![
        resource(
            identity(1, "x", BufferRole::Input),
            version(1, signature.input.element_ty, 4),
            0,
            MirKernelResourceAccess::Read,
        ),
        resource(
            identity(2, "y", BufferRole::Output),
            version(1, signature.output.element_ty, 1),
            1,
            MirKernelResourceAccess::Write,
        ),
    ];
    let dispatch_extras = vec![
        resource(
            identity(3, "k_weight", BufferRole::Input),
            version(1, signature.input.element_ty, 8),
            2,
            MirKernelResourceAccess::Read,
        ),
        resource(
            identity(4, "k_out", BufferRole::Output),
            version(1, signature.output.element_ty, 2),
            3,
            MirKernelResourceAccess::ReadWrite,
        ),
    ];
    carried.extend(dispatch_extras);
    assert!(carried_resources_agree_for_library_dispatch(
        &carried, &signature
    ));
    // The exact predicate still fails the same carried list — the superset
    // tolerance is scoped to library dispatches, never the default law.
    assert!(!carried_resources_agree(&carried, &signature));
}

#[test]
fn library_dispatch_agreement_keeps_exact_law_on_signature_slots() {
    // The fused admission is not a weakening: a signature slot with wrong
    // typed facts, or a missing signature slot, still fails closed.
    let signature = reduction_signature(4, 1, 1);
    let wrong_count = vec![
        resource(
            identity(1, "x", BufferRole::Input),
            version(1, signature.input.element_ty, 5),
            0,
            MirKernelResourceAccess::Read,
        ),
        resource(
            identity(2, "y", BufferRole::Output),
            version(1, signature.output.element_ty, 1),
            1,
            MirKernelResourceAccess::Write,
        ),
    ];
    assert!(!carried_resources_agree_for_library_dispatch(
        &wrong_count,
        &signature
    ));
    let missing_output = vec![resource(
        identity(1, "x", BufferRole::Input),
        version(1, signature.input.element_ty, 4),
        0,
        MirKernelResourceAccess::Read,
    )];
    assert!(!carried_resources_agree_for_library_dispatch(
        &missing_output,
        &signature
    ));
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
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 5,
            dispatch_y: 3,
        },
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
    let carried = CollectionKernelPlan::RowSoftmax(crate::kernel_plan::RowSoftmaxPlan {
        dims: crate::kernel_plan::RowOpDims::Literal { rows: 4, width: 4 },
        axis: 1,
        batched: None,
    });
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
        projection: crate::abi::ReducedProjection {
            axis_extent: 4,
            inner_stride: 1,
        },
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
/// count exceeds the active [`StorageBufferLimitRecord`] (shared default:
/// minimum capacity across admitted lanes) is refused with a typed diagnostic so the callers fall
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
            reduced_projection: None,
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
            dims: crate::kernel_plan::RowOpDims::Literal { rows: 4, width: 4 },
            axis: 1,
            epsilon_bits: 1e-5_f32.to_bits(),
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
                projection: crate::abi::ReducedProjection {
                    axis_extent: 4,
                    inner_stride: 1,
                },
            }
        )),
        RecipeKind::AxisReduction
    );
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::RowSoftmax(
            crate::kernel_plan::RowSoftmaxPlan {
                dims: crate::kernel_plan::RowOpDims::Literal { rows: 1, width: 1 },
                axis: 1,
                batched: None
            }
        )),
        RecipeKind::RowSoftmax
    );
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::LayerNormalization(
            crate::kernel_plan::LayerNormalizationPlan {
                dims: crate::kernel_plan::RowOpDims::Literal { rows: 1, width: 1 },
                axis: 1,
                epsilon_bits: 0,
            }
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
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::RowSoftmax(crate::kernel_plan::RowSoftmaxPlan {
            dims: crate::kernel_plan::RowOpDims::Literal { rows: 4, width: 4 },
            axis: 1,
            batched: None,
        }),
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
        projection: crate::abi::ReducedProjection {
            axis_extent: 4,
            inner_stride: 1,
        },
    });
    assert!(carried_plan_agrees(&carried, Some(&plan)));
}

#[test]
fn rms_norm_signature_binds_gamma_as_a_kernel_resource() {
    // GEA2-U4a: gamma is a typed read resource, not a field on the stable
    // RmsNormalizationPlan. The source-shaped and emission signatures must
    // agree on x, gamma, output binding order.
    let mut types = TypeTable::new();
    let function = rms_norm_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let shape = transformer_shape_signature(&function, MirCollectionOp::TensorRmsNorm, &validation)
        .expect("RMSNorm shape signature resolves");
    assert_eq!(shape.input.source_local, Some(MirLocalId(0)));
    assert_eq!(shape.input.element_count, 2 * 960);
    assert_eq!(shape.extra_inputs.len(), 1);
    assert_eq!(shape.extra_inputs[0].binding, 1);
    assert_eq!(shape.extra_inputs[0].source_local, Some(MirLocalId(1)));
    assert_eq!(shape.extra_inputs[0].element_count, 960);
    assert_eq!(shape.output.binding, 2);

    let emission = transformer_subchain_signature_for_emission(
        &function,
        MirCollectionOp::TensorRmsNorm,
        &validation,
        &[MirLocalId(2)],
    )
    .expect("RMSNorm emission signature resolves");
    assert_eq!(emission.extra_inputs.len(), 1);
    assert_eq!(emission.extra_inputs[0].source_local, Some(MirLocalId(1)));
    assert_eq!(emission.output.binding, 2);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &shape,
            &function,
            &validation,
            MirCollectionOp::TensorRmsNorm,
        )
        .is_some()
    );
}

#[test]
fn layer_norm_affine_emission_signature_binds_the_affine_peers() {
    // 3933dcd00's guard: `rope_table_local` reads `args[3]`, which an affine
    // layer-norm call fills with its GAMMA peer. Without the `is_rope()`
    // guard the gamma was mistaken for a RoPE table operand and marked
    // bound, so the data-flow tail dropped it from the emitted signature —
    // a non-RoPE call silently missing its affine input buffer. The
    // emission signature must carry BOTH affine peers as read inputs.
    let mut types = TypeTable::new();
    let function = layer_norm_affine_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let signature = transformer_subchain_signature_for_emission(
        &function,
        MirCollectionOp::TensorLayerNorm,
        &validation,
        &[MirLocalId(3)],
    )
    .expect("affine layer-norm emission signature resolves");
    assert_eq!(
        signature.input.source_local,
        Some(MirLocalId(0)),
        "the recipe operand x stays the primary read input"
    );
    assert_eq!(
        signature
            .extra_inputs
            .iter()
            .filter(|resource| resource.source_local == Some(MirLocalId(1)))
            .count(),
        1,
        "gamma (args[3]) must be bound as a read input, never swallowed as a \
         RoPE table operand"
    );
    assert_eq!(
        signature
            .extra_inputs
            .iter()
            .filter(|resource| resource.source_local == Some(MirLocalId(2)))
            .count(),
        1,
        "beta (args[4]) must be bound as a read input"
    );
    assert_eq!(signature.extra_inputs.len(), 2);
    assert_eq!(signature.extra_inputs[0].binding, 1);
    assert_eq!(signature.extra_inputs[0].element_count, 4);
    assert_eq!(signature.extra_inputs[1].binding, 2);
    assert_eq!(signature.extra_inputs[1].element_count, 4);
    assert_eq!(
        signature.output.binding, 3,
        "the output binds after both affine peers"
    );
    assert_eq!(signature.output.source_local, Some(MirLocalId(3)));
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
        batched: None,
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
fn whole_function_plan_rejects_multiple_recipe_statements() {
    // Same-kind multiplicity is invisible to the deduplicated op scan (it
    // adopts the second recipe by plan equality while the matmul shape scan
    // reads only the first statement) — the live-recipe census fails closed
    // instead. Distinct-op mixes keep the "conflicting recipes" diagnostic
    // (the test above); this is the same-op tier.
    let mut types = TypeTable::new();
    let function = double_matmul_function(&mut types);
    let shapes = MatMulShapes {
        m: 17,
        k: 12,
        n: 33,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let signature = matmul_signature(shapes, 5, 3);
    let validation = MirValidationContext::new(&types);
    let error = kernel_plan_for_function(&function, &signature, &validation)
        .expect_err("two live same-kind recipe statements must not resolve one plan");
    assert!(
        error.message.contains("one recipe statement per kernel"),
        "expected the census diagnostic, got: {}",
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
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
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
        dims: crate::kernel_plan::MatMulDims::Literal {
            m: 17,
            k: 12,
            n: 33,
        },
        m: 17,
        k: 12,
        n: 33,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
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
        batch: 1,
        right_batch: 1,
    };
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "multiplico".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
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
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 1,
            dispatch_y: 1,
        },
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
fn folded_elementwise_before_recipe_binds_recipe_operands() {
    // Decomposition folds a leading elementwise op into the adjacent recipe
    // subchain (`t = mul(x, w); s = sum(t)`). The emission signature must
    // bind the reduction input to the Sum operand (t), not the first
    // collection statement's operands (x, w).
    let mut types = TypeTable::new();
    let function = mul_sum_function(&mut types, 1024);
    let validation = MirValidationContext::new(&types);
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };

    let signature =
        subchain_signature_for_emission(&function, &contract, &validation, &[MirLocalId(3)])
            .expect("mul+sum emission signature resolves");

    assert_eq!(
        signature.input.source_local,
        Some(MirLocalId(2)),
        "the reduction input must be the Sum operand (mul result), not the first mul operand"
    );
    assert_eq!(
        signature.extra_inputs.len(),
        2,
        "x and w remain data-flow inputs of the folded mul"
    );
    assert_eq!(signature.extra_inputs[0].source_local, Some(MirLocalId(0)));
    assert_eq!(signature.extra_inputs[1].source_local, Some(MirLocalId(1)));
    assert_eq!(signature.output.source_local, Some(MirLocalId(3)));
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
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 1,
            dispatch_y: 1,
        },
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
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 1,
            dispatch_y: 1,
        },
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
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 1,
            dispatch_y: 1,
        },
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

// -----------------------------------------------------------------------
// GI3-1 inference-recipe admission (Gather / RmsNormalization / Rope /
// CausalMaskedSoftmax resolve + validate on the shared plan surface; the
// pinned-shape fixtures prove the pinned row's decoder op surface; SiLU is
// verified as a composition of the admitted elementwise surface + the
// S6-P1 Exp unary — never a recipe).
// -----------------------------------------------------------------------

/// A rank-2 `RMSNorm` kernel: `t = rms_norm(x, 1, 1e-5, gamma)` over `[2, 960]`
/// (the pinned hidden width), gamma `[960]` — the CCI-1 const layout
/// `[x, axis, eps, gamma]` (axis = last, eps = 1e-5, gamma mandatory).
fn rms_norm_function(types: &mut TypeTable) -> MirFunction {
    let input = rank2_tensor_ty(types, [2, 960]);
    let gamma = tensor_ty(types, 960);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: input,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: gamma,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: input,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: gamma,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: input,
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
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRmsNorm),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Constant(MirConstant::Int(1)),
                            MirOperand::Constant(MirConstant::Float(1e-5)),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: input,
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
        return_ty: input,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A rank-2 `RoPE` kernel: `t = rope_norm(x, pos, dim, rope_table)` over
/// `[2, 64]` with the explicit interleaved `[2, 32, 3]` table.
fn rope_function(types: &mut TypeTable, pos: i64, dim: i64) -> MirFunction {
    rope_function_for_op(types, pos, dim, MirCollectionOp::TensorRope)
}

fn rope_function_for_op(
    types: &mut TypeTable,
    pos: i64,
    dim: i64,
    op: MirCollectionOp,
) -> MirFunction {
    let input = rank2_tensor_ty(types, [2, 64]);
    let table = rank3_tensor_ty(types, [2, 32, 3]);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: input,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: table,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: input,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: table,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: input,
                mutable: true,
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
                        intrinsic: MirIntrinsic::Collection(op),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Constant(MirConstant::Int(pos)),
                            MirOperand::Constant(MirConstant::Int(dim)),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: input,
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
        return_ty: input,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A row-gather kernel: `t = gather(table, ids)` over the pinned `[960, 49152]`
/// token-embedding table with 9 ids → `[9, 49152]` (the tied output head
/// doubles as the embedding table). The ids vector is a `tensor<u32, [9]>`
/// operand (K7c): its extent is the id count the plan cross-checks against
/// the return-type dims, while the emitted kernel body bounds-checks the
/// runtime token against the table row count (SEC-G1).
fn gather_function(types: &mut TypeTable) -> MirFunction {
    let table = rank2_tensor_ty(types, [960, 49152]);
    let output = rank2_tensor_ty(types, [9, 49152]);
    let ids_element = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let ids_extent = types.intern_index(IndexExpr::Literal(9));
    let ids_shape = types.intern_index(IndexExpr::Tuple(vec![ids_extent]));
    let ids = MirType::semantic(types.intern(Type::Tensor(ids_element, ids_shape)));
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: table,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: ids,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: table,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: ids,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: output,
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
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGather),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: output,
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
        return_ty: output,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A rank-2 causal masked-softmax kernel: `t = causal_masked_softmax(x)`
/// over the pinned `[9, 9]` causal score matrix (the 9-token correctness
/// fixture prompt).
fn causal_masked_softmax_function(types: &mut TypeTable) -> MirFunction {
    rank2_const_op_function(
        types,
        [9, 9],
        MirCollectionOp::TensorCausalMaskedSoftmax,
        vec![],
    )
}

/// The gather kernel's transformer-shaped signature: table at binding 0
/// (f32, V·D), the ids vector as the first extra input at binding 1 (u32,
/// T), and the output at binding 2 (f32, T·D) — the shape
/// [`transformer_shape_signature`] synthesizes.
fn gather_signature() -> MirKernelSignature {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let ids_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    MirKernelSignature {
        function: MirFunctionId(0),
        input: MirKernelResource {
            group: 0,
            binding: 0,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: 960 * 49_152,
            source_local: Some(MirLocalId(0)),
            source_name: None,
            reduced_projection: None,
        },
        extra_inputs: vec![MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty: ids_ty,
            element_count: 9,
            source_local: Some(MirLocalId(1)),
            source_name: None,
            reduced_projection: None,
        }],
        output: MirKernelResource {
            group: 0,
            binding: 2,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: 9 * 49_152,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: 9 * 49_152,
            y: 1,
            z: 1,
        },
    }
}

#[test]
fn rms_norm_function_resolves_rms_normalization_plan() {
    // A TensorRmsNorm-bearing body resolves the RmsNormalization recipe with
    // the axis from the trailing const operand, the eps (as f32 bits) from
    // the const operand, and the width from the operand dims.
    let mut types = TypeTable::new();
    let function = rms_norm_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let signature =
        transformer_shape_signature(&function, MirCollectionOp::TensorRmsNorm, &validation)
            .expect("rms_norm shape signature resolves");
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("rms_norm function facts resolve")
        .expect("rms_norm function carries a recipe plan");
    let CollectionKernelPlan::RmsNormalization(plan) = &derived else {
        panic!("rms_norm body must derive an RMS-normalization plan, got {derived:?}");
    };
    assert_eq!(plan.axis, 1, "RMSNorm normalizes over the last axis");
    assert_eq!(
        plan.epsilon_bits,
        (1e-5_f32).to_bits(),
        "eps is the f32 const operand bits (the pinned row's 1e-5)"
    );
    assert_eq!(plan.width, 960, "the normalization width is the hidden dim");
    derived.validate().expect("the derived plan validates");
    let carried =
        CollectionKernelPlan::RmsNormalization(crate::kernel_plan::RmsNormalizationPlan {
            dims: crate::kernel_plan::RowOpDims::Literal {
                rows: 2,
                width: 960,
            },
            axis: 1,
            epsilon_bits: (1e-5_f32).to_bits(),
            width: 960,
        });
    assert!(carried_plan_agrees(&carried, Some(&derived)));
}

#[test]
fn rope_function_resolves_rope_plan() {
    // A TensorRope-bearing body resolves the Rope recipe with the pinned
    // NORM consecutive-pair (pos=0, dim=64) and the row width 64.
    let mut types = TypeTable::new();
    let function = rope_function(&mut types, 0, 64);
    let validation = MirValidationContext::new(&types);
    let signature =
        transformer_shape_signature(&function, MirCollectionOp::TensorRope, &validation)
            .expect("rope shape signature resolves");
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("rope function facts resolve")
        .expect("rope function carries a recipe plan");
    let CollectionKernelPlan::Rope(plan) = &derived else {
        panic!("rope body must derive a rope plan, got {derived:?}");
    };
    assert_eq!(
        plan.pos, 0,
        "position comes from the trailing const operand"
    );
    assert_eq!(plan.dim, 64, "rotated width comes from the const operand");
    assert_eq!(plan.width, 64, "row width is the last-axis extent");
    assert!(!plan.rotate_half, "TensorRope stays llama consecutive-pair");
    derived.validate().expect("the derived plan validates");
    let carried = CollectionKernelPlan::Rope(crate::kernel_plan::RopePlan {
        pos: 0,
        dim: 64,
        width: 64,
        per_row: true,
        rows: 2,
        rotate_half: false,
    });
    assert!(carried_plan_agrees(&carried, Some(&derived)));
}

#[test]
fn rope_half_function_resolves_rotate_half_plan() {
    let mut types = TypeTable::new();
    let function = rope_function_for_op(&mut types, 0, 64, MirCollectionOp::TensorRopeHalf);
    let validation = MirValidationContext::new(&types);
    let signature =
        transformer_shape_signature(&function, MirCollectionOp::TensorRopeHalf, &validation)
            .expect("rope_half shape signature resolves");
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("rope_half function facts resolve")
        .expect("rope_half function carries a recipe plan");
    let CollectionKernelPlan::Rope(plan) = &derived else {
        panic!("rope_half body must derive a rope plan, got {derived:?}");
    };
    assert!(
        plan.rotate_half,
        "TensorRopeHalf must select rotate_half pairing"
    );
    assert_eq!(plan.dim, 64);
    assert_eq!(plan.width, 64);
    derived.validate().expect("the derived plan validates");
}

#[test]
fn gather_function_resolves_gather_plan() {
    // A TensorGather-bearing body resolves the Gather recipe over the pinned
    // [960, 49152] table with 9 ids → [9, 49152]. The ids vector binds as
    // the signature's first extra input (binding 1) at the u32 index layout.
    let mut types = TypeTable::new();
    let function = gather_function(&mut types);
    let signature = gather_signature();
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("gather function facts resolve")
        .expect("gather function carries a recipe plan");
    let CollectionKernelPlan::Gather(plan) = &derived else {
        panic!("gather body must derive a gather plan, got {derived:?}");
    };
    assert_eq!(
        plan.table_rows,
        crate::kernel_plan::PlanDim::Literal(960),
        "table rows V come from the operand dims"
    );
    assert_eq!(
        plan.table_cols,
        crate::kernel_plan::PlanDim::Literal(49_152),
        "table cols D come from the operand dims"
    );
    assert_eq!(
        plan.id_count,
        crate::kernel_plan::PlanDim::Literal(9),
        "id count T comes from the return-type dims"
    );
    derived.validate().expect("the derived plan validates");
    let carried = CollectionKernelPlan::Gather(crate::kernel_plan::GatherPlan {
        table_rows: crate::kernel_plan::PlanDim::Literal(960),
        table_cols: crate::kernel_plan::PlanDim::Literal(49_152),
        id_count: crate::kernel_plan::PlanDim::Literal(9),
    });
    assert!(carried_plan_agrees(&carried, Some(&derived)));
}

#[test]
fn gather_shape_signature_binds_the_ids_extra_input() {
    // The transformer-shaped signature seam binds the ids vector as the
    // first extra input (binding 1, u32, count 9) with the output at
    // binding 2 — the shape the plan resolution cross-checks and the
    // subchain emission signature preserves.
    let mut types = TypeTable::new();
    let function = gather_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let signature =
        transformer_shape_signature(&function, MirCollectionOp::TensorGather, &validation)
            .expect("the gather seam resolves its transformer-shaped signature");
    assert_eq!(signature.input.element_count, 960 * 49_152);
    assert_eq!(signature.input.source_local, Some(MirLocalId(0)));
    assert_eq!(
        signature.extra_inputs.len(),
        1,
        "the ids bind as the extra input"
    );
    assert_eq!(signature.extra_inputs[0].element_count, 9);
    assert_eq!(signature.extra_inputs[0].binding, 1);
    assert_eq!(signature.extra_inputs[0].source_local, None);
    assert_eq!(signature.output.element_count, 9 * 49_152);
    assert_eq!(signature.output.binding, 2);
    // The plan pass resolves the seam signature exactly as it resolves the
    // whole-function signature — one source of recipe truth.
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorGather,
    )
    .expect("the seam signature resolves the gather plan");
    assert!(matches!(plan, CollectionKernelPlan::Gather(_)));
}

#[test]
fn gather_runtime_extents_append_axis_resources() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;

    let mut types = TypeTable::new();
    let mut function = gather_function(&mut types);
    let table = rank2_tensor_ty_mixed(&mut types, [Err(0), Err(1)]);
    let output = rank2_tensor_ty_mixed(&mut types, [Err(2), Err(1)]);
    let ids_element = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let ids_extent = types.intern_index(IndexExpr::Runtime(
        radix_types::RuntimeExtentId::ScalarParameter { parameter: 2 },
    ));
    let ids_shape = types.intern_index(IndexExpr::Tuple(vec![ids_extent]));
    let ids = MirType::semantic(types.intern(Type::Tensor(ids_element, ids_shape)));
    function.params[0].ty = table;
    function.params[1].ty = ids;
    function.locals[0].ty = table;
    function.locals[1].ty = ids;
    function.locals[2].ty = output;
    function.params.push(MirParam {
        local: MirLocalId(2),
        name: None,
        ty: output,
        mode: MirParamMode::MutRef,
        span: test_span(),
    });
    let index_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    function.params.push(MirParam {
        local: MirLocalId(3),
        name: None,
        ty: index_ty,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    function.locals.push(MirLocal {
        id: MirLocalId(3),
        name: None,
        ty: index_ty,
        mutable: false,
        span: test_span(),
    });
    function.return_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    function.blocks[0].terminator.kind = MirTerminatorKind::Return(None);
    if let MirStatementKind::RuntimeCall { call, .. } = &mut function.blocks[0].statements[0].kind {
        call.return_ty = output;
    }

    let mut validation = MirValidationContext::new(&types);
    for ty in [table, ids, output] {
        let view = match validation
            .layouts
            .layout_for_type(ty)
            .map(|layout| layout.kind)
        {
            Ok(crate::MirLayoutKind::DeviceView(view)) => view,
            other => panic!("tensor fixture must resolve as a device view: {other:?}"),
        };
        let mut device_view = crate::MirDeviceViewLayout::device_handle_row_major(
            crate::MirScalarLayout::F32,
            view.rank,
        );
        device_view.element = view.element;
        validation.layouts.set_type_layout(
            ty.semantic_id(),
            crate::MirLayoutKind::DeviceView(device_view),
        );
    }
    let recipe_signature =
        transformer_shape_signature(&function, MirCollectionOp::TensorGather, &validation)
            .expect("run-time gather transformer signature resolves");
    assert_eq!(
        recipe_signature.input.element_count,
        RUNTIME_SHAPED_ELEMENT_COUNT
    );
    assert_eq!(
        recipe_signature.output.element_count,
        RUNTIME_SHAPED_ELEMENT_COUNT
    );
    assert_eq!(
        recipe_signature.extra_inputs[0].element_count,
        RUNTIME_SHAPED_ELEMENT_COUNT
    );
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect("run-time gather signature resolves with its axis extents");
    assert_eq!(signature.input.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    assert_eq!(signature.output.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    let ids_input = signature
        .extra_inputs
        .iter()
        .find(|resource| resource.source_local == Some(MirLocalId(1)))
        .expect("the ids tensor remains bound as the second storage input");
    assert_eq!(ids_input.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    assert_eq!(ids_input.binding, 1);

    let extents: Vec<_> = signature
        .resources()
        .filter_map(|resource| match resource.kind {
            MirKernelResourceKind::TensorAxisExtent {
                tensor_binding,
                axis,
            } => Some((resource.binding, tensor_binding, axis)),
            _ => None,
        })
        .collect();
    assert_eq!(extents, vec![(3, 0, 0), (4, 0, 1), (5, 1, 0)]);
}

#[test]
fn causal_masked_softmax_function_resolves_causal_masked_softmax_plan() {
    // A TensorCausalMaskedSoftmax-bearing body resolves the
    // CausalMaskedSoftmax recipe over the pinned [9, 9] causal score matrix.
    let mut types = TypeTable::new();
    let function = causal_masked_softmax_function(&mut types);
    let signature = transformer_signature(9 * 9, 9 * 9);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("causal masked-softmax function facts resolve")
        .expect("causal masked-softmax function carries a recipe plan");
    let CollectionKernelPlan::CausalMaskedSoftmax(plan) = &derived else {
        panic!(
            "causal masked-softmax body must derive a causal masked-softmax plan, got {derived:?}"
        );
    };
    assert_eq!(plan.rows, 9, "the row (query) extent");
    assert_eq!(plan.cols, 9, "the column (key) extent");
    derived.validate().expect("the derived plan validates");
    let carried =
        CollectionKernelPlan::CausalMaskedSoftmax(crate::kernel_plan::CausalMaskedSoftmaxPlan {
            dims: crate::kernel_plan::RowOpDims::Literal { rows: 9, width: 9 },
            rows: 9,
            cols: 9,
        });
    assert!(carried_plan_agrees(&carried, Some(&derived)));
}

#[test]
fn inference_recipe_kinds_are_typed() {
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::Gather(
            crate::kernel_plan::GatherPlan {
                table_rows: crate::kernel_plan::PlanDim::Literal(960),
                table_cols: crate::kernel_plan::PlanDim::Literal(49_152),
                id_count: crate::kernel_plan::PlanDim::Literal(9),
            }
        )),
        RecipeKind::Gather
    );
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::RmsNormalization(
            crate::kernel_plan::RmsNormalizationPlan {
                dims: crate::kernel_plan::RowOpDims::Literal {
                    rows: 1,
                    width: 960,
                },
                axis: 1,
                epsilon_bits: (1e-5_f32).to_bits(),
                width: 960,
            }
        )),
        RecipeKind::RmsNormalization
    );
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::Rope(crate::kernel_plan::RopePlan {
            pos: 0,
            dim: 64,
            width: 64,
            per_row: false,
            rows: 0,
            rotate_half: false,
        })),
        RecipeKind::Rope
    );
    assert_eq!(
        RecipeKind::from_plan(&CollectionKernelPlan::CausalMaskedSoftmax(
            crate::kernel_plan::CausalMaskedSoftmaxPlan {
                dims: crate::kernel_plan::RowOpDims::Literal { rows: 9, width: 9 },
                rows: 9,
                cols: 9
            }
        )),
        RecipeKind::CausalMaskedSoftmax
    );
}

#[test]
fn const_violating_inference_ops_fail_closed() {
    // GI3-1 fail-closed: a non-last RMSNorm axis, an odd RoPE dim, and a
    // gather whose return width disagrees with the table stay closed with
    // the typed plan diagnostic — the plan never infers facts (A3).
    let mut types = TypeTable::new();
    let mut bad_axis = rms_norm_function(&mut types);
    // Rewrite the axis const (args[1]) to 0 (not the last axis).
    if let MirStatementKind::RuntimeCall { call, .. } = &mut bad_axis.blocks[0].statements[0].kind {
        call.args[1] = MirOperand::Constant(MirConstant::Int(0));
    }
    let validation = MirValidationContext::new(&types);
    let error = kernel_plan_for_function(
        &bad_axis,
        &transformer_signature(2 * 960, 2 * 960),
        &validation,
    )
    .expect_err("a non-last RMSNorm axis must fail closed");
    assert!(
        error.message.contains("TensorRmsNorm"),
        "the diagnostic names the op: {error}"
    );

    let mut types = TypeTable::new();
    let odd_rope = rope_function(&mut types, 0, 63);
    let validation = MirValidationContext::new(&types);
    let error = kernel_plan_for_function(
        &odd_rope,
        &transformer_signature(2 * 64, 2 * 64),
        &validation,
    )
    .expect_err("an odd RoPE dim must fail closed");
    assert!(
        error.message.contains("TensorRope"),
        "the diagnostic names the op: {error}"
    );

    // A non-admitted collection op (TensorSlice — no recipe, not elementwise)
    // still fails closed with the typed diagnostic.
    let mut types = TypeTable::new();
    let slice = rank2_const_op_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorSlice,
        vec![MirConstant::Int(0)],
    );
    let validation = MirValidationContext::new(&types);
    let error = kernel_plan_for_function(&slice, &transformer_signature(16, 16), &validation)
        .expect_err("a non-admitted collection op must fail closed");
    assert!(
        error.message.contains("TensorSlice"),
        "the diagnostic names the op: {error}"
    );
}

/// The `SiLU` composition fixture: `y = x·sigmoid(x)` with
/// `sigmoid(x) = 1/(1+exp(−x))` expressed as the admitted elementwise
/// surface (`TensorNeg` for `−x`, `TensorDiv` for `1/(1+e)`, `TensorMul` for
/// `x·d`) plus the S6-P1 `MirUnOp::Exp` tensor unary — the composition
/// GI3-1 verifies in place of a dedicated activation recipe.
fn silu_composition_function(types: &mut TypeTable) -> MirFunction {
    let r1 = tensor_ty(types, 4);
    let exp = MirValue {
        id: MirValueId(0),
        kind: MirValueKind::Unary {
            op: MirUnOp::Exp,
            operand: MirOperand::Place(MirPlace::local(MirLocalId(1))),
        },
        ty: r1,
        span: test_span(),
    };
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: r1,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: r1,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: r1,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: r1,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: r1,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(4),
                name: None,
                ty: r1,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(5),
                name: None,
                ty: r1,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                // n = -x
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(1))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorNeg),
                            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                            return_ty: r1,
                        },
                    },
                    span: test_span(),
                },
                // e = exp(n) — the S6-P1 tensor unary
                MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::local(MirLocalId(2)),
                        value: exp,
                    },
                    span: test_span(),
                },
                // a = 1 + e
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(3))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                            args: vec![
                                MirOperand::Constant(MirConstant::Float(1.0)),
                                MirOperand::Place(MirPlace::local(MirLocalId(2))),
                            ],
                            return_ty: r1,
                        },
                    },
                    span: test_span(),
                },
                // d = 1 / a
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(4))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorDiv),
                            args: vec![
                                MirOperand::Constant(MirConstant::Float(1.0)),
                                MirOperand::Place(MirPlace::local(MirLocalId(3))),
                            ],
                            return_ty: r1,
                        },
                    },
                    span: test_span(),
                },
                // y = x * d
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(5))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMul),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                MirOperand::Place(MirPlace::local(MirLocalId(4))),
                            ],
                            return_ty: r1,
                        },
                    },
                    span: test_span(),
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(5),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: r1,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

#[test]
fn silu_composition_is_elementwise_on_the_admitted_surface() {
    // SiLU(x) = x·sigmoid(x) = x/(1+exp(−x)) is expressible on the admitted
    // elementwise surface (TensorNeg/TensorAdd/TensorDiv/TensorMul) plus the
    // S6-P1 `MirUnOp::Exp` tensor unary — the composition needs NO dedicated
    // activation recipe, so the whole body stays an elementwise kernel
    // (kernel_plan_for_function returns the no-recipe `None`, never a
    // fail-closed diagnostic and never a silent recipe).
    assert!(
        crate::device_program_plans::is_admitted_tensor_unary(MirUnOp::Exp),
        "the S6-P1 Exp unary is admitted on the tensor elementwise surface"
    );
    let mut types = TypeTable::new();
    let function = silu_composition_function(&mut types);
    let signature = transformer_signature(4, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("the SiLU composition facts resolve without a fail-closed diagnostic");
    assert_eq!(
        derived, None,
        "the SiLU composition is elementwise (no recipe), so the plan is the no-recipe fallback"
    );
    // The complete collection-op set of the composition is elementwise —
    // the closed-set predicate admits every member.
    for op in crate::collection_ops::collection_ops_for_function(&function) {
        assert!(
            is_elementwise_collection_op(op),
            "the SiLU composition op {op:?} must be elementwise-admitted"
        );
    }
}

// -----------------------------------------------------------------------
// DDCP1-U3: per-output bounds/index domains (contract §Per-output bounds).
// -----------------------------------------------------------------------

/// A kernel writing two outputs of different extents (4 and 2 elements)
/// under one dispatch sized for 4 lanes.
fn dual_output_kernel_program() -> DeviceProgram {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let first = identity(1, "out.a", BufferRole::Output);
    let second = identity(2, "out.b", BufferRole::Output);
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "dual_output".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![
            resource(
                first,
                version(1, element_ty, 4),
                1,
                MirKernelResourceAccess::Write,
            ),
            resource(
                second,
                version(1, element_ty, 2),
                2,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 4, y: 1, z: 1 },
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
        results: Vec::new(),
    }
}

#[test]
fn per_output_domains_derived_per_output() {
    // Contract §Per-output bounds rule 1: every output has its OWN
    // bounds/index domain — heterogeneous outputs are not collapsed into a
    // single shared extent.
    let program = dual_output_kernel_program();
    let domains = per_output_domains(&program.kernels[0]);
    assert_eq!(domains.len(), 2, "one derived domain per output slot");
    assert_eq!(domains[0].element_count, 4);
    assert_eq!(domains[1].element_count, 2);
    assert_ne!(
        domains[0].element_count, domains[1].element_count,
        "heterogeneous outputs keep distinct per-output extents"
    );
}

#[test]
fn launch_descriptor_carries_per_output_domains() {
    // The launch descriptor exposes each output's own domain so hosts
    // dispatch per output (rule 2/3: no guessed shared extent).
    let program = dual_output_kernel_program();
    let descriptors = launch_descriptors(&program);
    assert_eq!(descriptors[0].output_domains.len(), 2);
    assert_eq!(descriptors[0].output_domains[0].element_count, 4);
    assert_eq!(descriptors[0].output_domains[1].element_count, 2);
}

#[test]
fn valid_program_with_covered_outputs_validates() {
    // A shared dispatch covering every output's own domain is legal.
    let program = elementwise_device_program();
    validate_per_output_domains(&program)
        .expect("an output covered by the dispatched lanes validates");
}

#[test]
fn underfilled_output_fails_closed() {
    // Contract §Per-output bounds rules 2/3: no shared dispatch extent may
    // under-fill an output — an output larger than the dispatched lanes
    // fails closed.
    let mut program = dual_output_kernel_program();
    program.kernels[0].launch = KernelLaunchPlan {
        workgroup: MirWorkgroupSize { x: 2, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    };
    assert!(matches!(
        validate_per_output_domains(&program),
        Err(DeviceProgramError::OutputExtentUnderfilled {
            kernel: 0,
            group: 0,
            binding: 1,
            element_count: 4,
            dispatched: 2,
        })
    ));
}

fn quantized_matmul_function(types: &mut TypeTable) -> MirFunction {
    let left = rank2_tensor_ty(types, [8, 16]);
    let right = rank2_tensor_ty(types, [16, 16]);
    let out = rank2_tensor_ty(types, [8, 16]);
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
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorQuantizedMatMul),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            MirOperand::Constant(MirConstant::Int(12)),
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

#[test]
fn kernel_plan_for_function_derives_q4k_quantized_matmul_plan() {
    let mut types = TypeTable::new();
    let function = quantized_matmul_function(&mut types);
    let shapes = MatMulShapes {
        m: 8,
        k: 16,
        n: 16,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let signature = matmul_signature(shapes, 2, 1);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("Q4_K function facts resolve")
        .expect("Q4_K function carries a packed recipe plan");
    let CollectionKernelPlan::QuantizedMatMul(plan) = &derived else {
        panic!("Q4_K fixture must resolve QuantizedMatMul, got {derived:?}");
    };
    assert_eq!(plan.layout, crate::abi::PackedStorageLayout::Q4_K);
    assert_eq!((plan.m, plan.k, plan.n), (8, 16, 16));
}

#[test]
fn executor_plan_exposes_packed_region_and_decode_gemv_facts() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let plan = CollectionKernelPlan::QuantizedMatMul(crate::kernel_plan::QuantizedMatMulPlan {
        m: 1,
        k: 32,
        n: 32,
        layout: crate::abi::PackedStorageLayout::Q8_0,
        tile: 8,
        workgroup_x: 8,
        workgroup_y: 8,
    });
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "qmm".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan,
        resources: vec![
            resource(
                identity(1, "left", BufferRole::InOut),
                version(1, ty, 32),
                0,
                MirKernelResourceAccess::Read,
            ),
            resource(
                identity(2, "weights", BufferRole::Input),
                version(1, ty, 32 * 32),
                1,
                MirKernelResourceAccess::Read,
            ),
            resource(
                identity(3, "out", BufferRole::Output),
                version(1, ty, 32),
                2,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 8, y: 8, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    };
    let program = DeviceProgram {
        kernels: vec![kernel],
        launches: vec![LaunchUnit {
            id: LaunchId(1),
            kernel_index: 0,
        }],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: Vec::new(),
    };
    let extension = executor_plan_extension(&program);
    extension
        .validate_for_kernel_count(1)
        .expect("packed executor-plan section is well-formed");
    let selection = &extension.selections[0];
    // CTO f083c057: batched is the decode numeric baseline, so the M=1
    // kernel selects the GEMM entry and reports decode_gemv=0 until PB-3
    // re-enables selection with a numeric-identity proof.
    assert_eq!(
        selection.library_entry.as_deref(),
        Some(QUANTIZED_GEMM_ENTRY)
    );
    let packed_bytes = crate::abi::PackedStorageLayout::Q8_0
        .packed_bytes_for_elements(32 * 32)
        .expect("Q8_0 32x32 packs");
    assert_eq!(selection.binds.len(), 3);
    assert_eq!(selection.binds[0].dims, vec![1, 32]);
    assert_eq!(selection.binds[0].strides, vec![32, 1]);
    assert_eq!(selection.binds[1].dims, vec![packed_bytes]);
    assert_eq!(selection.binds[1].strides, vec![1]);
    assert_eq!(selection.binds[2].dims, vec![1, 32]);
    assert_eq!(selection.binds[2].strides, vec![32, 1]);
    assert_eq!(
        selection.offsets,
        vec![
            OffsetBinding {
                name: "buffer0".to_owned(),
                offset_elements: 0,
                strides: vec![32, 1],
            },
            OffsetBinding {
                name: "buffer1".to_owned(),
                offset_elements: 0,
                strides: vec![1],
            },
            OffsetBinding {
                name: "buffer2".to_owned(),
                offset_elements: 0,
                strides: vec![32, 1],
            },
        ]
    );
    assert_eq!(
        selection
            .uniforms
            .iter()
            .find(|uniform| uniform.name == DECODE_GEMV_UNIFORM)
            .map(|uniform| uniform.bits),
        Some(0)
    );
    assert_eq!(
        selection
            .uniforms
            .iter()
            .find(|uniform| uniform.name == PACKED_LAYOUT_GGML_TYPE_ID_UNIFORM)
            .map(|uniform| uniform.bits),
        Some(8)
    );
    assert_eq!(
        selection
            .uniforms
            .iter()
            .find(|uniform| uniform.name == PACKED_BYTES_UNIFORM)
            .map(|uniform| uniform.bits),
        Some(u32::try_from(packed_bytes).expect("packed bytes fit u32"))
    );
    assert_eq!(
        selection.backend_requirements[0].feature,
        QUANTIZED_GEMM_ENTRY
    );
}

#[test]
fn static_executor_plan_remains_v1() {
    let program = elementwise_device_program();
    let extension = executor_plan_extension(&program);
    assert_eq!(extension.abi_version, EXECUTOR_PLAN_BIND_ABI_VERSION);
    extension
        .validate_for_kernel_count(program.kernels.len())
        .expect("legacy static executor plan remains valid");
    let selection = &extension.selections[0];
    assert_eq!(selection.binds.len(), 2);
    for bind in &selection.binds {
        assert_eq!(bind.dims, vec![4]);
        assert_eq!(bind.strides, vec![1]);
        assert_eq!(bind.offset_elements, 0);
    }
    assert_eq!(
        selection.offsets,
        vec![
            OffsetBinding {
                name: "buffer0".to_owned(),
                offset_elements: 0,
                strides: vec![1],
            },
            OffsetBinding {
                name: "buffer1".to_owned(),
                offset_elements: 0,
                strides: vec![1],
            },
        ]
    );
}

fn v2_facts_for_elementwise() -> ExecutorPlanV2Facts {
    let program = elementwise_device_program();
    let input = &program.kernels[0].resources[0].buffer;
    ExecutorPlanV2Facts {
        allocations: vec![AllocationFact {
            buffer: input.clone(),
            dtype: UniformType::F32,
            capacity_bytes: 64,
            lifetime: BufferLifetime::PerProgram,
            initialization: InitializationPolicy::HostProvided,
        }],
        views: vec![
            ViewFact {
                allocation: input.id,
                logical_dims: vec![4],
                strides: vec![1],
                static_base: 0,
                maximum_span: 16,
            },
            // A second view proves that capacity and view extent are separate
            // facts and that two views may share one allocation identity.
            ViewFact {
                allocation: input.id,
                logical_dims: vec![2],
                strides: vec![1],
                static_base: 4,
                maximum_span: 8,
            },
        ],
        launch_bindings: vec![LaunchBinding::new(
            0,
            input.id,
            Binding {
                group: 0,
                binding: 0,
            },
            0,
            16,
            ExecutorPlanSource::runtime(RuntimeSource::Position),
        )],
    }
}

#[test]
fn runtime_bindings_emit_v2_with_typed_sources_and_views() {
    let program = elementwise_device_program();
    let facts = v2_facts_for_elementwise();
    let extension = executor_plan_extension_v2(&program, facts)
        .expect("typed allocation/view facts should produce a v2 plan");

    assert_eq!(
        extension.abi_version,
        EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION
    );
    assert_eq!(extension.allocations[0].buffer.id, BufferId(1));
    assert_eq!(extension.allocations[0].capacity_bytes, 64);
    assert_eq!(extension.views.len(), 2);
    assert_eq!(extension.views[1].static_base, 4);
    assert_eq!(extension.views[1].maximum_span, 8);
    assert_eq!(extension.launch_bindings[0].handle, BufferId(1));
    assert_eq!(extension.launch_bindings[0].binding.binding, 0);
    assert_eq!(
        extension.launch_bindings[0].runtime_source,
        ExecutorPlanSource::Runtime(RuntimeSource::Position)
    );
    extension
        .validate_for_kernel_count(program.kernels.len())
        .expect("v2 plan remains valid after production");
}

#[test]
fn overflowing_view_span_fails_before_v2_emission() {
    let program = elementwise_device_program();
    let mut facts = v2_facts_for_elementwise();
    facts.views[0] = ViewFact {
        allocation: BufferId(1),
        logical_dims: vec![u64::MAX],
        strides: vec![2],
        static_base: 0,
        maximum_span: u64::MAX,
    };
    let error = executor_plan_extension_v2(&program, facts)
        .expect_err("overflowing logical span must be rejected before emission");
    assert!(
        error.to_string().contains("overflows"),
        "expected an overflow diagnostic, got {error}"
    );
}

#[test]
fn overflowing_launch_span_fails_before_v2_emission() {
    let program = elementwise_device_program();
    let mut facts = v2_facts_for_elementwise();
    facts.launch_bindings[0].byte_offset = u64::MAX - 1;
    facts.launch_bindings[0].view_span = 2;
    let error = executor_plan_extension_v2(&program, facts)
        .expect_err("overflowing launch span must be rejected before emission");
    assert!(
        error.to_string().contains("overflows"),
        "expected an overflow diagnostic, got {error}"
    );
}

#[test]
fn untagged_static_sources_do_not_claim_v2_runtime_bindings() {
    let program = elementwise_device_program();
    let mut facts = v2_facts_for_elementwise();
    facts.launch_bindings[0].runtime_source = ExecutorPlanSource::constant(UniformType::U32, 7);
    let error = executor_plan_extension_v2(&program, facts)
        .expect_err("a static-only binding set must stay on the v1 producer");
    assert_eq!(error, ExecutorPlanV2Error::MissingRuntimeSource);
}

fn speculative_verification_admission() -> VerificationBlockAdmission {
    VerificationBlockAdmission {
        abi_version: EXECUTOR_PLAN_VERIFICATION_ABI_VERSION,
        prefix_before: 12,
        query_rows: 3,
        candidate_ids: vec![7, 8, 9],
        committed_generation: Some(4),
        transaction_id: Some(19),
        plan: VerificationPlanShape {
            model_shape: vec![4096, 32],
            vocab: 32_000,
            logits_layout: VerificationLogitsLayout::RowMajor,
        },
    }
}

fn speculative_verification_result() -> VerificationBlockResult {
    VerificationBlockResult {
        transaction_id: Some(19),
        committed_generation: Some(4),
        logits_shape: vec![3, 32_000],
        logits_layout: VerificationLogitsLayout::RowMajor,
        accepted_length: 2,
    }
}

#[test]
fn speculative_verification_admits_explicit_block_and_result() {
    let admission = speculative_verification_admission()
        .admit()
        .expect("nonzero prefix and explicit candidate count admit");
    let result = admission
        .admit_result(speculative_verification_result())
        .expect("matching generation, transaction, shape, and r admit");
    assert_eq!(result.accepted_length, 2);
    assert_eq!(admission.static_plan_identity().digest().len(), 71);
}

#[test]
fn speculative_verification_rejects_missing_generation() {
    let mut admission = speculative_verification_admission();
    admission.committed_generation = None;
    assert_eq!(
        admission.admit().expect_err("generation is required"),
        VerificationAdmissionError::MissingGeneration
    );
}

#[test]
fn speculative_verification_rejects_zero_k_and_fresh_prefill() {
    let mut admission = speculative_verification_admission();
    admission.query_rows = 0;
    assert_eq!(
        admission.admit().expect_err("k=0 is not a block"),
        VerificationAdmissionError::InvalidQueryRows
    );

    let mut fresh = speculative_verification_admission();
    fresh.prefix_before = 0;
    assert_eq!(
        fresh
            .admit()
            .expect_err("fresh prefill must not masquerade as verification"),
        VerificationAdmissionError::FreshPrefillMasquerade
    );
}

#[test]
fn speculative_verification_rejects_k_inferred_from_candidate_sequence() {
    let mut admission = speculative_verification_admission();
    admission.query_rows = 2;
    assert_eq!(
        admission
            .admit()
            .expect_err("the explicit k must match candidate rows"),
        VerificationAdmissionError::CandidateCountMismatch {
            declared: 2,
            actual: 3,
        }
    );
}

#[test]
fn speculative_verification_rejects_invalid_accepted_lengths() {
    let admission = speculative_verification_admission();
    for accepted_length in [-1, 4] {
        let mut result = speculative_verification_result();
        result.accepted_length = accepted_length;
        assert!(matches!(
            admission.admit_result(result),
            Err(VerificationAdmissionError::InvalidAcceptedLength { .. })
        ));
    }
}

#[test]
fn speculative_verification_rejects_shape_and_layout_skew() {
    let admission = speculative_verification_admission();
    let mut shape_skew = speculative_verification_result();
    shape_skew.logits_shape = vec![2, 32_000];
    assert!(matches!(
        admission.admit_result(shape_skew),
        Err(VerificationAdmissionError::LogitsShapeSkew { .. })
    ));

    let mut layout_skew = speculative_verification_result();
    layout_skew.logits_layout = VerificationLogitsLayout::ColumnMajor;
    assert!(matches!(
        admission.admit_result(layout_skew),
        Err(VerificationAdmissionError::LogitsLayoutSkew { .. })
    ));
}

#[test]
fn speculative_verification_rejects_stale_generation_and_row_mismatch() {
    let admission = speculative_verification_admission();
    let mut stale = speculative_verification_result();
    stale.committed_generation = Some(3);
    assert!(matches!(
        admission.admit_result(stale),
        Err(VerificationAdmissionError::StaleGeneration { .. })
    ));

    let mut row_mismatch = speculative_verification_result();
    row_mismatch.logits_shape = vec![4, 32_000];
    assert!(matches!(
        admission.admit_result(row_mismatch),
        Err(VerificationAdmissionError::LogitsShapeSkew { .. })
    ));
}

#[test]
fn speculative_verification_static_identity_excludes_live_invocation_facts() {
    let admission = speculative_verification_admission();
    let base = admission.static_plan_identity();
    let mut different_invocation = admission.clone();
    different_invocation.prefix_before = 99;
    different_invocation.query_rows = 1;
    different_invocation.candidate_ids = vec![11];
    different_invocation.committed_generation = Some(88);
    different_invocation.transaction_id = Some(77);
    assert_eq!(base, different_invocation.static_plan_identity());

    let mut different_shape = admission;
    different_shape.plan.vocab += 1;
    assert_ne!(base, different_shape.static_plan_identity());
}

#[test]
fn speculative_verification_version_skew_fails_closed() {
    assert!(
        admit_executor_plan_verification_version(EXECUTOR_PLAN_VERIFICATION_ABI_VERSION).is_ok()
    );
    assert!(matches!(
        admit_executor_plan_verification_version(EXECUTOR_PLAN_VERIFICATION_ABI_VERSION - 1),
        Err(VerificationAdmissionError::UnsupportedVersion { .. })
    ));
}

// -----------------------------------------------------------------------
// Declared distributed sum (SFR-5: `summa ex … filum …` plan entry).
// -----------------------------------------------------------------------

fn filum_declaration(lane_width: u32) -> crate::MirFilumReduceOp {
    crate::MirFilumReduceOp {
        op: crate::MirReduceOp::Sum,
        form: crate::MirFilumForm::Declared,
        term_ty: MirType::semantic(radix_types::TypeId(0)),
        group: None,
        filum_binder: None,
        term_binder: MirLocalId(2),
        term_binder_name: None,
        coordinates: vec![crate::MirFilumCoordinate {
            local: MirLocalId(3),
            name: None,
        }],
        lane_width,
        term_body: Vec::new(),
        term_captures: None,
        term_result: MirLocalId(4),
    }
}

fn filum_function(types: &mut TypeTable) -> MirFunction {
    let tensor = tensor_ty(types, 1024);
    let scalar = f32_ty(types);
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
                        intrinsic: MirIntrinsic::FilumReduce(filum_declaration(32)),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: scalar,
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
        return_ty: scalar,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

#[test]
fn kernel_plan_for_function_derives_distributed_sum_plan() {
    let mut types = TypeTable::new();
    let function = filum_function(&mut types);
    let signature = reduction_signature(1024, 32, 32);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("filum declaration facts resolve")
        .expect("filum function carries the declared recipe plan");
    let CollectionKernelPlan::DistributedSum(plan) = &derived else {
        panic!("filum declaration must resolve to DistributedSum, got {derived:?}");
    };
    assert_eq!(plan.length, 1024);
    assert_eq!(plan.workgroup_x, 32);
    assert_eq!(plan.partials, 32);
    assert_eq!(plan.filum.lane_width, 32);
    derived
        .validate()
        .expect("declared distributed sum plan validates");
}

#[test]
fn kernel_plan_for_function_reads_declared_filum_source_through_a_parameter_copy() {
    // Composition binds a callee's tensor parameter by a whole copy of the
    // caller's, so a declared filum inlined from a kernel helper reads the
    // copy. The copy is the parameter's storage: the plan must resolve to the
    // kernel's source buffer exactly as for the direct read.
    let mut types = TypeTable::new();
    let mut function = filum_function(&mut types);
    let tensor = function.locals[0].ty;
    function.locals.push(MirLocal {
        id: MirLocalId(5),
        name: None,
        ty: tensor,
        mutable: false,
        span: test_span(),
    });
    function.blocks[0].statements.insert(
        0,
        MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::local(MirLocalId(5)),
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(MirLocalId(0)))),
                    ty: tensor,
                    span: test_span(),
                },
            },
            span: test_span(),
        },
    );
    let MirStatementKind::RuntimeCall { call, .. } = &mut function.blocks[0].statements[1].kind
    else {
        panic!("filum function statement is a runtime call");
    };
    call.args = vec![MirOperand::Place(MirPlace::local(MirLocalId(5)))];
    let signature = reduction_signature(1024, 32, 32);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("a filum read through a parameter copy resolves its source buffer")
        .expect("filum function carries the declared recipe plan");
    let CollectionKernelPlan::DistributedSum(plan) = &derived else {
        panic!("filum declaration must resolve to DistributedSum, got {derived:?}");
    };
    assert_eq!(plan.length, 1024);
}

#[test]
fn nested_filum_form_is_never_a_declared_sum_plan() {
    // gpu-reset rule 14.5 vocabulary: the nested lane-body form validates
    // like Grid but must never be taken for a Declared plan.
    assert_eq!(crate::MirFilumForm::Nested.name(), "nested");
    let mut types = TypeTable::new();
    let mut function = filum_function(&mut types);
    let mut nested = filum_declaration(32);
    nested.form = crate::MirFilumForm::Nested;
    let MirStatementKind::RuntimeCall { call, .. } = &mut function.blocks[0].statements[0].kind
    else {
        panic!("filum function statement is a runtime call");
    };
    call.intrinsic = MirIntrinsic::FilumReduce(nested);
    let signature = reduction_signature(1024, 32, 32);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation);
    assert!(
        !matches!(&derived, Ok(Some(CollectionKernelPlan::DistributedSum(_)))),
        "a Nested filum node must not resolve to a DistributedSum plan, got {derived:?}"
    );
}

#[test]
fn distributed_sum_floor_is_the_tree_reduction_recipe() {
    // SFR-5 plan law: a backend with no native arm lowers the declared
    // distributed sum through the shared-memory tree floor — the floor facts
    // must equal the TreeReduction recipe's exactly, by construction.
    let types = TypeTable::new();
    let signature = reduction_signature(1024, 32, 32);
    let validation = MirValidationContext::new(&types);
    let filum = filum_declaration(32);
    let declared = CollectionKernelPlan::from_filum_summa(&filum, 1024)
        .expect("declaration facts resolve a plan");
    let recognized = CollectionKernelPlan::from_contract(
        &signature,
        &CollectionOpContract::Reduction {
            length: 1024,
            workgroup_x: 32,
            partials: 32,
        },
        MirCollectionOp::Sum,
        None,
    )
    .expect("reduction contract resolves the tree recipe");
    let CollectionKernelPlan::DistributedSum(declared_plan) = &declared else {
        panic!("from_filum_summa yields DistributedSum");
    };
    let CollectionKernelPlan::TreeReduction(tree) = &recognized else {
        panic!("from_contract yields TreeReduction");
    };
    assert_eq!(declared_plan.workgroup_x, tree.workgroup_x);
    assert_eq!(declared_plan.length, tree.length);
    assert_eq!(declared_plan.partials, tree.partials);
    assert_eq!(declared_plan.shared_memory, tree.shared_memory);
    assert_eq!(declared_plan.barriers, tree.barriers);
    assert_eq!(declared_plan.oob_padding, tree.oob_padding);
    // The launch tag selects the tree-reduction semantics — the no-native-
    // arm floor.
    assert_eq!(
        RecipeKind::from_plan(&declared),
        RecipeKind::from_plan(&recognized)
    );
    assert_eq!(
        plan_shared_memory_bytes(&declared),
        plan_shared_memory_bytes(&recognized)
    );
    let _ = validation;
}

#[test]
fn distributed_sum_second_recipe_fails_closed() {
    let mut types = TypeTable::new();
    let mut function = filum_function(&mut types);
    // A second, recognized recipe op in the same body must not silently
    // merge with the declared distributed sum.
    function.blocks[0].statements.push(MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(1))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSoftmax),
                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(1)))],
                return_ty: f32_ty(&mut types),
            },
        },
        span: test_span(),
    });
    let signature = reduction_signature(1024, 32, 32);
    let validation = MirValidationContext::new(&types);
    assert!(kernel_plan_for_function(&function, &signature, &validation).is_err());
}

#[test]
fn from_filum_summa_rejects_degenerate_declarations() {
    let filum = filum_declaration(0);
    assert!(CollectionKernelPlan::from_filum_summa(&filum, 1024).is_none());
    let filum = filum_declaration(24);
    assert!(CollectionKernelPlan::from_filum_summa(&filum, 1024).is_none());
    let filum = filum_declaration(32);
    assert!(CollectionKernelPlan::from_filum_summa(&filum, 0).is_none());
}

#[test]
fn matmul_contract_literal_dims_keep_the_numeric_tile_grid() {
    let mut types = TypeTable::new();
    let function = matmul_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("literal matmul resolves")
        .expect("literal matmul has a contract");
    assert_eq!(
        contract,
        CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Literal {
                dispatch_x: 5,
                dispatch_y: 3
            }
        }
    );
}

#[test]
fn data_flow_or_runtime_binds_runtime_dims_at_the_marker() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;

    let mut types = TypeTable::new();
    let runtime = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(4)]);
    let literal = rank2_tensor_ty(&mut types, [3, 5]);
    let overflow = rank2_tensor_ty(&mut types, [4_294_967_296, 4_294_967_296]);
    let expected_element_ty = f32_ty(&mut types);
    let validation = MirValidationContext::new(&types);

    let (runtime_element_ty, runtime_count) =
        data_flow_element_ty_and_count_or_runtime(runtime, &validation)
            .expect("runtime-shaped data-flow tensor resolves");
    assert_eq!(runtime_element_ty, expected_element_ty);
    assert_eq!(runtime_count, RUNTIME_SHAPED_ELEMENT_COUNT);

    let (literal_element_ty, literal_count) =
        data_flow_element_ty_and_count_or_runtime(literal, &validation)
            .expect("literal data-flow tensor resolves");
    assert_eq!(literal_element_ty, expected_element_ty);
    assert_eq!(literal_count, 15);

    assert!(data_flow_element_ty_and_count_or_runtime(overflow, &validation).is_err());
}

#[test]
fn subchain_emission_signature_keeps_runtime_data_flow_facts() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;

    let mut types = TypeTable::new();
    let left = rank2_tensor_ty(&mut types, [4, 4]);
    let right = rank2_tensor_ty(&mut types, [4, 4]);
    let output = rank2_tensor_ty(&mut types, [4, 4]);
    let runtime = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(4)]);
    let mut function = matmul_function_over(left, right, output);
    function.params.push(MirParam {
        local: MirLocalId(3),
        name: None,
        ty: runtime,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    function.locals.extend([
        MirLocal {
            id: MirLocalId(3),
            name: None,
            ty: runtime,
            mutable: false,
            span: test_span(),
        },
        MirLocal {
            id: MirLocalId(4),
            name: None,
            ty: runtime,
            mutable: false,
            span: test_span(),
        },
    ]);
    let validation = MirValidationContext::new(&types);
    let signature = subchain_signature_for_emission(
        &function,
        &CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Literal {
                dispatch_x: 1,
                dispatch_y: 1,
            },
        },
        &validation,
        &[MirLocalId(2), MirLocalId(4)],
    )
    .expect("runtime-shaped data-flow input and output resolve");

    assert_eq!(signature.extra_inputs[1].source_local, Some(MirLocalId(3)));
    assert_eq!(
        signature.extra_inputs[1].element_count,
        RUNTIME_SHAPED_ELEMENT_COUNT
    );
    assert_eq!(signature.extra_outputs[0].source_local, Some(MirLocalId(4)));
    assert_eq!(
        signature.extra_outputs[0].element_count,
        RUNTIME_SHAPED_ELEMENT_COUNT
    );
}

/// The elementwise-only subchain emission signature binds a runtime-shaped
/// intermediate at the [`crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT`] marker:
/// params in declaration order (first at binding 0, the rest as extra
/// inputs), then the outputs — and the predicate agrees the surface carries
/// a run-time shape.
#[test]
fn elementwise_subchain_signature_binds_runtime_markers() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;

    let mut types = TypeTable::new();
    let runtime = rank2_tensor_ty_mixed(&mut types, [Err(0), Err(1)]);
    let mut function = matmul_function_over(runtime, runtime, runtime);
    // The matmul destination local (2) must be declared for the output local
    // type resolution, exactly as a decomposed subchain declares it.
    function.locals.push(MirLocal {
        id: MirLocalId(2),
        name: None,
        ty: runtime,
        mutable: false,
        span: test_span(),
    });
    let validation = MirValidationContext::new(&types);
    assert!(subchain_has_runtime_shaped_buffer(
        &function,
        &[MirLocalId(2)],
        &validation
    ));

    let signature =
        elementwise_subchain_signature_for_emission(&function, &validation, &[MirLocalId(2)])
            .expect("runtime-shaped elementwise subchain resolves");

    // Params bind in declaration order: a at binding 0, bias at binding 1.
    assert_eq!(signature.input.binding, 0);
    assert_eq!(signature.input.source_local, Some(MirLocalId(0)));
    assert_eq!(signature.input.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    assert_eq!(signature.extra_inputs.len(), 1);
    assert_eq!(signature.extra_inputs[0].binding, 1);
    assert_eq!(signature.extra_inputs[0].source_local, Some(MirLocalId(1)));
    assert_eq!(
        signature.extra_inputs[0].element_count,
        RUNTIME_SHAPED_ELEMENT_COUNT
    );
    // The primary output binds at the next binding (2).
    assert_eq!(signature.output.binding, 2);
    assert_eq!(signature.output.source_local, Some(MirLocalId(2)));
    assert_eq!(signature.output.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    // The launch geometry of a runtime-shaped kernel is a per-call fact.
    assert_eq!(signature.workgroup, MirWorkgroupSize { x: 1, y: 1, z: 1 });
    assert_eq!(
        signature.dispatch_size,
        MirKernelDispatchSize { x: 1, y: 1, z: 1 }
    );
}

/// The elementwise subchain signature keeps the literal ABI counts (no
/// runtime marker), the predicate reads the literal surface as false, and a
/// kernel-scalar param binds as a 1-element input at its own type.
#[test]
fn elementwise_subchain_signature_keeps_literal_counts() {
    let mut types = TypeTable::new();
    let left = rank2_tensor_ty(&mut types, [3, 5]);
    let right = rank2_tensor_ty(&mut types, [3, 5]);
    let out = rank2_tensor_ty(&mut types, [3, 5]);
    let scalar = f32_ty(&mut types);
    let mut function = matmul_function_over(left, right, out);
    function.locals.push(MirLocal {
        id: MirLocalId(2),
        name: None,
        ty: out,
        mutable: false,
        span: test_span(),
    });
    // A kernel-scalar data-flow param (the elementwise scalar seed) binds at
    // count 1, mirroring the ABI kernel scalar surface.
    function.params.push(MirParam {
        local: MirLocalId(3),
        name: None,
        ty: scalar,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    function.locals.push(MirLocal {
        id: MirLocalId(3),
        name: None,
        ty: scalar,
        mutable: false,
        span: test_span(),
    });
    let validation = MirValidationContext::new(&types);
    assert!(!subchain_has_runtime_shaped_buffer(
        &function,
        &[MirLocalId(2)],
        &validation
    ));

    let signature =
        elementwise_subchain_signature_for_emission(&function, &validation, &[MirLocalId(2)])
            .expect("literal elementwise subchain resolves");

    assert_eq!(signature.input.binding, 0);
    assert_eq!(signature.input.element_count, 15);
    assert_eq!(signature.extra_inputs.len(), 2);
    assert_eq!(signature.extra_inputs[0].binding, 1);
    assert_eq!(signature.extra_inputs[0].element_count, 15);
    assert_eq!(signature.extra_inputs[1].binding, 2);
    assert_eq!(signature.extra_inputs[1].element_count, 1);
    assert_eq!(signature.extra_inputs[1].element_ty, scalar);
    assert_eq!(signature.output.binding, 3);
    assert_eq!(signature.output.element_count, 15);
}

#[test]
fn literal_data_flow_sites_still_fail_closed() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;

    let mut types = TypeTable::new();
    let runtime = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(4)]);

    // RMSNorm's gamma peer remains a literal-demanding data-flow resource.
    let mut rms = rms_norm_function(&mut types);
    rms.params[1].ty = runtime;
    rms.locals[1].ty = runtime;
    let validation = MirValidationContext::new(&types);
    let error = transformer_shape_signature(&rms, MirCollectionOp::TensorRmsNorm, &validation)
        .expect_err("runtime gamma stays literal-demanding");
    assert_eq!(
        error.message,
        "kernel collection op requires literal dimensions"
    );

    // The batched-attention operand reader also keeps its literal law.
    let left = runtime;
    let right = rank2_tensor_ty(&mut types, [4, 4]);
    let output = rank2_tensor_ty(&mut types, [4, 4]);
    let mut attention = matmul_function_over(left, right, output);
    if let MirStatementKind::RuntimeCall { call, .. } = &mut attention.blocks[0].statements[0].kind
    {
        call.intrinsic = MirIntrinsic::Collection(MirCollectionOp::TensorBatchedAttention);
        call.args.extend([
            MirOperand::Constant(MirConstant::Int(0)),
            MirOperand::Constant(MirConstant::Int(2)),
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Int(4)),
            MirOperand::Constant(MirConstant::Int(4)),
            MirOperand::Constant(MirConstant::Int(4)),
        ]);
    }
    let validation = MirValidationContext::new(&types);
    let contract = CollectionOpContract::BatchedAttention {
        kind: crate::kernel_plan::BatchedAttentionKind::Scores,
        kv_heads: 1,
        q_per_kv: 1,
        seq: 4,
        head_dim: 4,
        hidden: 4,
        layout: None,
        rotate_half: false,
        scale_bits: 0,
        output_elements: 16,
    };
    let error = contract_shape_signature(&attention, &contract, &validation)
        .expect_err("runtime attention operand stays literal-demanding");
    assert_eq!(
        error.message,
        "kernel collection op requires literal dimensions"
    );

    // The table-peer reader in the transformer signature keeps the same
    // literal-only policy (this is the current call site for table resources).
    let mut rope_types = TypeTable::new();
    let mut rope = rope_function(&mut rope_types, 0, 64);
    let runtime_table = rank2_tensor_ty_mixed(&mut rope_types, [Err(0), Ok(4)]);
    rope.params[1].ty = runtime_table;
    rope.locals[1].ty = runtime_table;
    let validation = MirValidationContext::new(&rope_types);
    let error = transformer_shape_signature(&rope, MirCollectionOp::TensorRope, &validation)
        .expect_err("runtime table peer stays literal-demanding");
    assert_eq!(
        error.message,
        "kernel collection op requires literal dimensions"
    );

    // The gather table's transformer shape reader is the rule-11 exception:
    // a run-time table extent builds a runtime-shaped input (the planner
    // cross-checks each resource against the extents it is sized from), while
    // the rank-2 table law still fails closed.
    let mut gather_types = TypeTable::new();
    let mut gather = gather_function(&mut gather_types);
    let runtime_table = rank2_tensor_ty_mixed(&mut gather_types, [Err(0), Ok(4)]);
    gather.params[0].ty = runtime_table;
    gather.locals[0].ty = runtime_table;
    let validation = MirValidationContext::new(&gather_types);
    let signature =
        transformer_shape_signature(&gather, MirCollectionOp::TensorGather, &validation)
            .expect("a run-time gather table extent builds a runtime-shaped input");
    assert_eq!(signature.input.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);

    let mut rank3_types = TypeTable::new();
    let mut rank3_gather = gather_function(&mut rank3_types);
    let rank3_table = rank3_tensor_ty(&mut rank3_types, [960, 4, 4]);
    rank3_gather.params[0].ty = rank3_table;
    rank3_gather.locals[0].ty = rank3_table;
    let validation = MirValidationContext::new(&rank3_types);
    assert!(
        transformer_shape_signature(&rank3_gather, MirCollectionOp::TensorGather, &validation)
            .is_err(),
        "a rank-3 gather table fails closed"
    );
}

#[test]
fn matmul_contract_runtime_dims_resolve_without_a_numeric_grid() {
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let left = rank2_tensor_ty_mixed(&mut types, [Err(0), Err(1)]);
    let right = rank2_tensor_ty_mixed(&mut types, [Err(1), Err(2)]);
    let out = rank2_tensor_ty_mixed(&mut types, [Err(0), Err(2)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("run-time matmul resolves")
        .expect("run-time matmul has a contract");
    assert_eq!(
        contract,
        CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime
        }
    );
    // `MatMulShapes` carries literal dims only, so it still refuses run-time
    // dims by name; the plan pass skips it for `MatMulDispatch::Runtime` and builds the
    // plan from the signature alone.
    let error = kernel_matmul_shapes(&function, &validation).expect_err("no literal shapes");
    assert!(
        error.message.contains("run-time extents"),
        "{}",
        error.message
    );
    let plan = kernel_plan_for_function(
        &function,
        &contract_shape_signature(&function, &contract, &validation).expect("signature"),
        &validation,
    )
    .expect("run-time matmul plans")
    .expect("run-time matmul has a plan");
    assert!(matches!(
        plan,
        CollectionKernelPlan::TiledMatMul(crate::kernel_plan::MatMulPlan {
            dims: crate::kernel_plan::MatMulDims::Runtime { .. },
            ..
        })
    ));
}

/// Design-note item 4: the run-time matmul signature carries runtime-shaped
/// operand/output buffers plus one `TensorAxisExtent` resource per run-time
/// operand axis, after the output.
#[test]
fn matmul_runtime_tensor_extent_resources_follow_the_output() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    // a [?0, ?1] x b [?1, 33] -> o [?0, 33]: M, K runtime; N literal.
    let left = rank2_tensor_ty_mixed(&mut types, [Err(0), Err(1)]);
    let right = rank2_tensor_ty_mixed(&mut types, [Err(1), Ok(33)]);
    let out = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(33)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let signature = contract_shape_signature(
        &function,
        &CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime,
        },
        &validation,
    )
    .expect("run-time matmul signature resolves");
    assert_eq!(signature.input.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    assert_eq!(signature.output.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    assert_eq!(signature.output.binding, 2);
    assert_eq!(signature.output.source_local, Some(MirLocalId(2)));
    assert_eq!(
        signature.extra_inputs[0].element_count,
        RUNTIME_SHAPED_ELEMENT_COUNT
    );
    let extents: Vec<_> = signature
        .extra_inputs
        .iter()
        .filter_map(|resource| match resource.kind {
            MirKernelResourceKind::TensorAxisExtent {
                tensor_binding,
                axis,
            } => {
                assert_eq!(resource.element_count, 1);
                Some((resource.binding, tensor_binding, axis))
            }
            _ => None,
        })
        .collect();
    // a axis 0 (M), a axis 1 (K), b axis 0 (K); b axis 1 is literal.
    assert_eq!(extents, vec![(3, 0, 0), (4, 0, 1), (5, 1, 0)]);
    assert_eq!(signature.workgroup, MirWorkgroupSize { x: 8, y: 8, z: 1 });
}

/// Only K runtime: the output stays a literal `M*N` buffer.
#[test]
fn matmul_runtime_tensor_extent_resources_literal_output_when_only_k_is_runtime() {
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let left = rank2_tensor_ty_mixed(&mut types, [Ok(4), Err(0)]);
    let right = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(5)]);
    let out = rank2_tensor_ty_mixed(&mut types, [Ok(4), Ok(5)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let signature = contract_shape_signature(
        &function,
        &CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime,
        },
        &validation,
    )
    .expect("run-time matmul signature resolves");
    assert_eq!(signature.output.element_count, 20);
}

/// The run-time matmul output count comes from the checked dims reader on the
/// DECLARED result: a zero literal axis is a refusal (never the runtime
/// marker), an overflowing literal product is a refusal (never a saturated
/// count), and a run-time result axis stays the runtime marker.
#[test]
fn matmul_runtime_output_count_uses_the_checked_reader() {
    use crate::tensor_type::{DIM_PRODUCT_OVERFLOW_MESSAGE, ZERO_LITERAL_DIM_MESSAGE};

    // a [?0, ?1] x b [?1, 5] -> o [?0, 0]: a zero literal result axis.
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let left = rank2_tensor_ty_mixed(&mut types, [Err(0), Err(1)]);
    let right = rank2_tensor_ty_mixed(&mut types, [Err(1), Ok(5)]);
    let out = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(0)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("run-time matmul resolves")
        .expect("run-time matmul has a contract");
    let error = contract_shape_signature(&function, &contract, &validation)
        .expect_err("a zero literal result axis is refused");
    assert_eq!(error.message, ZERO_LITERAL_DIM_MESSAGE);

    // a [2^32, ?1] x b [?1, 2^32] -> o [2^32, 2^32]: the literal product
    // leaves u64, so the count is refused instead of saturated.
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let left = rank2_tensor_ty_mixed(&mut types, [Ok(4_294_967_296), Err(1)]);
    let right = rank2_tensor_ty_mixed(&mut types, [Err(1), Ok(4_294_967_296)]);
    let out = rank2_tensor_ty_mixed(&mut types, [Ok(4_294_967_296), Ok(4_294_967_296)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("run-time matmul resolves")
        .expect("run-time matmul has a contract");
    let error = contract_shape_signature(&function, &contract, &validation)
        .expect_err("an overflowing literal result count is refused");
    assert_eq!(error.message, DIM_PRODUCT_OVERFLOW_MESSAGE);

    // a [?0, ?1] x b [?1, 33] -> o [?0, 33]: a run-time result axis is the
    // runtime marker, not a literal count.
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let left = rank2_tensor_ty_mixed(&mut types, [Err(0), Err(1)]);
    let right = rank2_tensor_ty_mixed(&mut types, [Err(1), Ok(33)]);
    let out = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(33)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("run-time matmul resolves")
        .expect("run-time matmul has a contract");
    let signature = contract_shape_signature(&function, &contract, &validation)
        .expect("run-time matmul signature resolves");
    assert_eq!(
        signature.output.element_count,
        crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT
    );
}

#[test]
fn matmul_contract_mixed_dims_with_equal_literal_inner_is_runtime() {
    let mut types = TypeTable::new();
    let left = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(12)]);
    let right = rank2_tensor_ty_mixed(&mut types, [Ok(12), Ok(33)]);
    let out = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(33)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    assert_eq!(
        collection_op_contract(&function, &validation).unwrap(),
        Some(CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime
        })
    );
}

#[test]
fn matmul_contract_runtime_dims_still_reject_literal_inner_mismatch() {
    let mut types = TypeTable::new();
    let left = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(12)]);
    let right = rank2_tensor_ty_mixed(&mut types, [Ok(11), Ok(33)]);
    let out = rank2_tensor_ty_mixed(&mut types, [Err(0), Ok(33)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let error = collection_op_contract(&function, &validation)
        .expect_err("literal inner mismatch is still an error");
    assert!(
        error.message.contains("inner dimensions"),
        "{}",
        error.message
    );
}

/// The batched dense run-time form takes the literal arm's shape law:
/// `[?, ?, ?] · [?, ?]` resolves as the run-time contraction, and a right
/// operand of rank greater than the left's is still refused.
#[test]
fn matmul_contract_batched_runtime_dims_resolve() {
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let left = batched_tensor_ty_mixed(&mut types, [Err(0), Err(1), Err(2)]);
    let right = batched_tensor_ty_mixed(&mut types, [Err(0), Err(2)]);
    let out = batched_tensor_ty_mixed(&mut types, [Err(0), Err(1), Err(3)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    assert_eq!(
        collection_op_contract(&function, &validation).expect("batched run-time matmul resolves"),
        Some(CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime,
        })
    );

    // A right operand of rank greater than the left's is not a batched
    // contraction, whatever the extents are.
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let left = batched_tensor_ty_mixed(&mut types, [Err(0), Err(1), Err(2)]);
    let right = batched_tensor_ty_mixed(&mut types, [Err(0), Err(0), Err(2), Err(3)]);
    let out = batched_tensor_ty_mixed(&mut types, [Err(0), Err(1), Err(3)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    assert!(
        collection_op_contract(&function, &validation).is_err(),
        "a right rank greater than the left's stays refused"
    );
}

/// The batched run-time signature binds the operands' full extents and puts
/// one `TensorAxisExtent` resource per run-time operand axis after the
/// output. Only-K run-time keeps the literal full output product.
#[test]
fn matmul_runtime_batched_signature_extents_follow_the_operands() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    // a [?0, ?1, ?2] x b [?0, ?2] -> o [?0, ?1, ?3]: every axis run-time.
    let left = batched_tensor_ty_mixed(&mut types, [Err(0), Err(1), Err(2)]);
    let right = batched_tensor_ty_mixed(&mut types, [Err(0), Err(2)]);
    let out = batched_tensor_ty_mixed(&mut types, [Err(0), Err(1), Err(3)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let signature = contract_shape_signature(
        &function,
        &CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime,
        },
        &validation,
    )
    .expect("batched run-time matmul signature resolves");
    assert_eq!(signature.output.binding, 2);
    assert_eq!(signature.output.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    let extents: Vec<_> = signature
        .extra_inputs
        .iter()
        .filter_map(|resource| match resource.kind {
            MirKernelResourceKind::TensorAxisExtent {
                tensor_binding,
                axis,
            } => {
                assert_eq!(resource.element_count, 1);
                Some((resource.binding, tensor_binding, axis))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        extents,
        vec![(3, 0, 0), (4, 0, 1), (5, 0, 2), (6, 1, 0), (7, 1, 1)]
    );
    assert_eq!(signature.workgroup, MirWorkgroupSize { x: 8, y: 8, z: 1 });

    // Only K runtime: a [5, 3, ?] x b [5, ?, 7] -> o [5, 3, 7], so the
    // output extent is the literal full product 5 * 3 * 7.
    let mut types = TypeTable::new();
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let left = batched_tensor_ty_mixed(&mut types, [Ok(5), Ok(3), Err(0)]);
    let right = batched_tensor_ty_mixed(&mut types, [Ok(5), Err(0), Ok(7)]);
    let out = batched_tensor_ty_mixed(&mut types, [Ok(5), Ok(3), Ok(7)]);
    let function = matmul_function_over(left, right, out);
    let validation = MirValidationContext::new(&types);
    let signature = contract_shape_signature(
        &function,
        &CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime,
        },
        &validation,
    )
    .expect("only-K run-time batched matmul signature resolves");
    assert_eq!(signature.output.element_count, 105);
}
