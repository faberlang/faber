// Fixture casts are small known-in-range numbers.
#![allow(clippy::cast_possible_truncation)]
use super::build_device_program;
use crate::abi::{MirKernelResourceAccess, MirKernelResourceKind};
use crate::device::MirCompanionMap;
use crate::device::{MirDeviceFunctionMetadata, MirDeviceFunctionMetadataMap};
use crate::device_program::{
    BufferId, BufferLifetime, BufferRole, DeviceGradientLink, DeviceProgram, DeviceProgramLifetime,
    DeviceTrainableParam, DeviceTrainingFacts, ObservationCadence,
};
use crate::device_semantics::{
    DeviceSemantics, LosslessMirCompanionEntry, MirCompanionSelectedInput,
    MirCompanionSelectedOutput, SemanticValueOrigin,
};
use crate::kernel_plan::CollectionKernelPlan;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirBlockId,
    MirCollectionOp, MirFunction, MirFunctionId, MirFunctionOrigin, MirFunctionVisibility,
    MirIntrinsic, MirKernelShaderStage, MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode,
    MirPlace, MirProgram, MirRuntimeCall, MirStatement, MirStatementKind,
    MirSyntheticFunctionIdentity, MirTerminator, MirTerminatorKind, MirType, MirValidationContext,
    ValidatedMir,
};
use radix_hir::DefId;
use radix_lexer::{Interner, Span};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};
use std::collections::BTreeSet;

fn f32(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn rank1_tensor_ty(types: &mut TypeTable, count: u64) -> MirType {
    let f32_id = f32(types).semantic_id();
    let len = types.intern_index(IndexExpr::Literal(count));
    MirType::semantic(types.intern(Type::Tensor(f32_id, len)))
}

fn u32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32))
}

fn vacuum_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.intern(Type::Primitive(Primitive::Vacuum)))
}

/// `out[id] ← value` — the indexed store that makes a vacuum-return kernel
/// an indexed-view kernel (the ABI's output param carries the store target's
/// identity).
fn index_store_statement(out: u32, index: u32, value: u32, ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace {
                base: crate::MirPlaceBase::Local(MirLocalId(out)),
                projections: vec![crate::MirProjection::Index(MirOperand::Place(
                    MirPlace::local(MirLocalId(index)),
                ))],
            },
            value: crate::MirValue {
                id: crate::MirValueId(0),
                kind: crate::MirValueKind::Operand(MirOperand::Place(MirPlace::local(MirLocalId(
                    value,
                )))),
                ty,
                span: span(),
            },
        },
        span: span(),
    }
}

/// `destination ← source` — a plain copy of a local into another local or an
/// output parameter (`y ⇇ d`).
fn copy_statement(destination: u32, source: u32, ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(destination)),
            value: crate::MirValue {
                id: crate::MirValueId(destination),
                kind: crate::MirValueKind::Operand(MirOperand::Place(MirPlace::local(MirLocalId(
                    source,
                )))),
                ty,
                span: span(),
            },
        },
        span: span(),
    }
}

fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32_id = f32(types).semantic_id();
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_id, shape)))
}

/// A rank-2 `f32` tensor whose axes are bound at run time: each entry is the
/// scalar-parameter id of a `RuntimeExtentId::ScalarParameter` extent.
fn rank2_tensor_ty_runtime(types: &mut TypeTable, dims: [u32; 2]) -> MirType {
    let f32_id = f32(types).semantic_id();
    let dim_ids = dims
        .iter()
        .map(|&parameter| {
            types.intern_index(IndexExpr::Runtime(
                radix_types::RuntimeExtentId::ScalarParameter { parameter },
            ))
        })
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_id, shape)))
}

fn span() -> Span {
    Span::default()
}

fn collection_statement(
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
        span: span(),
    }
}

/// A tuple-construct statement binding `locals` into a fresh temp, plus the
/// temp's id — the multi-output return shape the ABI flattens into distinct
/// output resources.
fn tuple_construct(locals: &[u32], ty: MirType) -> (MirStatement, crate::MirTempId) {
    let temp = crate::MirTempId(0);
    (
        MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::temp(temp),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Tuple,
                    ty,
                    fields: MirAggregateFields::Ordered(
                        locals
                            .iter()
                            .map(|&local| {
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(local),
                                )))
                            })
                            .collect(),
                    ),
                },
            },
            span: span(),
        },
        temp,
    )
}

/// Assemble a single-block function from params (named, in order) and
/// statements; every statement destination is typed in the locals table from
/// its producing statement's return type (the plan/signature passes read the
/// preserved local tables).
#[allow(clippy::too_many_arguments)]
fn device_function(
    id: u32,
    interner: &mut Interner,
    name: &str,
    named_locals: &[(u32, &str, MirType)],
    params: &[u32],
    statements: Vec<MirStatement>,
    return_operand: Option<MirOperand>,
    temps: Vec<crate::MirTemp>,
    return_ty: MirType,
    compute: bool,
) -> MirFunction {
    let locals = named_locals
        .iter()
        .map(|&(id, name, ty)| MirLocal {
            id: MirLocalId(id),
            name: Some(interner.intern(name)),
            ty,
            mutable: false,
            span: span(),
        })
        .collect::<Vec<_>>();
    // Statement destinations beyond the named table default to the function
    // return type (fixtures type every producer explicitly or return it).
    let param_ty = |wanted: u32| {
        named_locals
            .iter()
            .find(|&&(id, _, _)| id == wanted)
            .map_or(return_ty, |&(_, _, ty)| ty)
    };
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: Some(interner.intern(name)),
        params: params
            .iter()
            .map(|&id| MirParam {
                local: MirLocalId(id),
                name: named_locals
                    .iter()
                    .find(|&&(local, _, _)| local == id)
                    .map(|&(_, name, _)| interner.intern(name)),
                ty: param_ty(id),
                mode: MirParamMode::Owned,
                span: span(),
            })
            .collect(),
        locals,
        temps,
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(return_operand),
                span: span(),
            },
            span: span(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: compute.then_some(MirKernelShaderStage::Compute),
        span: span(),
    }
}

/// Explicit carrier rows for the fixture's compute-stage functions: under
/// the KCM-05 explicit-facts admission law a fixture declares its device
/// kernels through the carrier (public rows — the named-entry analog) instead
/// of relying on the compute stage alone.
fn fixture_metadata(functions: &[MirFunction]) -> MirDeviceFunctionMetadataMap {
    functions
        .iter()
        .filter(|function| function.shader_stage == Some(MirKernelShaderStage::Compute))
        .map(|function| {
            (
                function.id,
                MirDeviceFunctionMetadata {
                    identity: MirSyntheticFunctionIdentity {
                        parent: MirFunctionId(0),
                        ordinal: 0,
                    },
                    role: crate::MirDeviceRole::Kernel,
                    shader_stage: MirKernelShaderStage::Compute,
                    visibility: MirFunctionVisibility::Public,
                    origin: MirFunctionOrigin::Source,
                    lane_domain_local: None,
                    refused_backends: Default::default(),
                    tier: crate::device::KernelTier::Default,
                },
            )
        })
        .collect()
}

fn validated(types: &TypeTable, functions: Vec<MirFunction>) -> ValidatedMir<'_> {
    ValidatedMir::new(MirProgram { functions }, MirValidationContext::new(types))
        .expect("the constructor fixture validates")
}

/// The constructor's launch rows follow the same explicit visibility gate
/// as HIR entry-role selection: a public named entry is materialized, while an
/// unmarked named helper stays available only to composition and never reaches
/// the program's kernel, launch, or ABI rows.
#[test]
fn constructor_excludes_private_named_helpers_from_device_program() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let scalar = f32(&mut types);
    let mut public = device_function(
        1,
        &mut interner,
        "public_entry",
        &[(0, "value", scalar)],
        &[0],
        Vec::new(),
        Some(MirOperand::Place(MirPlace::local(MirLocalId(0)))),
        Vec::new(),
        scalar,
        true,
    );
    public.source = Some(DefId(1));
    let mut private = device_function(
        2,
        &mut interner,
        "private_helper",
        &[(0, "value", scalar)],
        &[0],
        Vec::new(),
        Some(MirOperand::Place(MirPlace::local(MirLocalId(0)))),
        Vec::new(),
        scalar,
        true,
    );
    private.source = Some(DefId(2));
    let validated = validated(&types, vec![public, private]);
    let mut metadata = fixture_metadata(&validated.program().functions);
    metadata
        .get_mut(&MirFunctionId(2))
        .expect("private named helper has a source metadata row")
        .visibility = MirFunctionVisibility::Private;

    let (program, _semantics, _steps) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &metadata,
    )
    .expect("public named entry materializes");

    assert_eq!(
        program.kernels.len(),
        1,
        "only the public entry has a kernel row"
    );
    assert_eq!(program.kernels[0].function, MirFunctionId(1));
    // ca2499dc: the entry is the compiler-owned `k<function id>`; the
    // author's `public_entry` spelling never reaches device source.
    assert_eq!(program.kernels[0].entry, "k1");
    assert_eq!(
        program.launches.len(),
        1,
        "only the public entry is launched"
    );
    assert_eq!(
        program.results.len(),
        1,
        "only the public entry has ABI results"
    );
    assert!(
        program
            .kernels
            .iter()
            .all(|kernel| kernel.function != MirFunctionId(2)),
        "private named helper must be absent from kernel/ABI rows"
    );
}

#[test]
fn constructor_returns_program_and_semantics_from_validated_mir() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let scalar = f32(&mut types);
    let local = MirLocal {
        id: MirLocalId(10),
        name: None,
        ty: scalar,
        mutable: true,
        span: Span::default(),
    };
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(1),
            source: None,
            name: Some(interner.intern("scale_kernel")),
            params: vec![MirParam {
                local: local.id,
                name: None,
                ty: scalar,
                mode: MirParamMode::Owned,
                span: Span::default(),
            }],
            locals: vec![local],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::<MirStatement>::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(10),
                    )))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: scalar,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: Some(MirKernelShaderStage::Compute),
            span: Span::default(),
        }],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the constructor fixture validates");

    let (device_program, semantics, _steps) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("the constructor derives the device program");

    assert_eq!(device_program.kernels.len(), 1);
    // ca2499dc: entries are the neutral `k<function id>`, not the source name.
    assert_eq!(device_program.kernels[0].entry, "k1");
    assert_eq!(device_program.launches.len(), 1);
    assert_eq!(device_program.results.len(), 1);
    assert_eq!(semantics.values.len(), 2);
    assert_eq!(semantics.bindings.len(), 2);
    assert!(semantics.dependencies.is_empty());
    device_program
        .validate_with_semantics(&semantics)
        .expect("constructor output passes the shared semantic gate");
}

/// Spec `stage-2-device-program-materialization` §4 H1 / B6: the real
/// Faber-lowered MLP body materializes through the GENERIC decomposition
/// path as four children — never a composed one-kernel shortcut. The
/// fixture ships unnamed with no shader stage; the constructor selects
/// named Compute-stage functions and resolves entry names through the
/// interner, so a mutable copy carries both facts explicitly.
#[test]
fn device_program_composed_body_materializes_four_generic_children() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let mut function =
        crate::device_program_plans::tests::composed_mlp_real_lowered_function(&mut types);
    function.name = Some(interner.intern("composed_mlp"));
    function.shader_stage = Some(MirKernelShaderStage::Compute);
    // The fixture's collection results carry the lowering's alias type ids;
    // the full MIR validator requires plain tensor results (`type_kind`
    // does not look through `Type::Alias`), so normalize each call's return
    // type to its alias target on the mutable copy.
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            if let MirStatementKind::RuntimeCall { call, .. } = &mut statement.kind
                && let Type::Alias(_, inner) = types.get(call.return_ty.semantic_id())
            {
                call.return_ty = MirType::semantic(*inner);
            }
        }
    }
    // The real lowering names its locals; the fixture ships unnamed for the
    // plan-pass tests. Names keep the constructor's name+shape wiring from
    // false-joining same-shape anonymous inputs (gate vs up weights).
    let local_names: [(u32, &str); 10] = [
        (0, "x"),
        (1, "gate_weight"),
        (2, "up_weight"),
        (3, "down_weight"),
        (4, "down"),
        (5, "seed"),
        (6, "gate"),
        (7, "up"),
        (8, "activated"),
        (9, "fused"),
    ];
    for (id, name) in local_names {
        let symbol = interner.intern(name);
        if let Some(param) = function
            .params
            .iter_mut()
            .find(|param| param.local == MirLocalId(id))
        {
            param.name = Some(symbol);
        }
        if let Some(local) = function
            .locals
            .iter_mut()
            .find(|local| local.id == MirLocalId(id))
        {
            local.name = Some(symbol);
        }
    }
    let validated = validated(&types, vec![function]);
    let (program, semantics, _) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("the composed MLP body materializes through the generic path");

    // Four children with deterministic entries and plans by position: the
    // gate matmul, the up matmul, the elementwise SiLU join, the down matmul.
    assert_eq!(program.kernels.len(), 4);
    let entries: Vec<&str> = program
        .kernels
        .iter()
        .map(|kernel| kernel.entry.as_str())
        .collect();
    // The fixture function is `MirFunctionId(0)`; ca2499dc makes each
    // subchain entry the neutral `k<function id>__<subchain index>`.
    assert_eq!(entries, ["k0__0", "k0__1", "k0__2", "k0__3"]);
    for (kernel, wants_matmul) in program.kernels.iter().zip([true, true, false, true]) {
        let plan_matches = if wants_matmul {
            matches!(kernel.plan, CollectionKernelPlan::TiledMatMul(_))
        } else {
            matches!(kernel.plan, CollectionKernelPlan::Elementwise)
        };
        assert!(
            plan_matches,
            "kernel {} carries the wrong plan",
            kernel.entry
        );
    }

    // Four launches, each bound to exactly its kernel.
    assert_eq!(program.launches.len(), 4);
    let launch_id = |kernel_index: usize| {
        let bound: Vec<_> = program
            .launches
            .iter()
            .filter(|launch| launch.kernel_index == kernel_index)
            .collect();
        assert_eq!(
            bound.len(),
            1,
            "kernel {kernel_index} has exactly one launch"
        );
        bound[0].id
    };
    let (gate, up, join, down) = (launch_id(0), launch_id(1), launch_id(2), launch_id(3));

    // A buffer's semantic origin, resolved through the value bindings.
    let origin_of = |buffer: BufferId| {
        let binding = semantics
            .bindings
            .iter()
            .find(|binding| binding.buffer == buffer)
            .expect("every program buffer has a semantic binding");
        semantics
            .values
            .iter()
            .find(|value| value.id == binding.value)
            .expect("every binding has a semantic value")
            .origin
            .clone()
    };

    // Roots: neither gate nor up carries an incoming edge.
    assert!(
        semantics
            .dependencies
            .iter()
            .all(|edge| edge.consumer != gate && edge.consumer != up),
        "gate and up are independent roots"
    );

    // Join: exactly two producer edges — the gate result (local 6) and the
    // up result (local 7) — on distinct buffers.
    let join_edges: Vec<_> = semantics
        .dependencies
        .iter()
        .filter(|edge| edge.consumer == join)
        .collect();
    assert_eq!(join_edges.len(), 2, "the join consumes both roots");
    assert_ne!(join_edges[0].buffer, join_edges[1].buffer);
    let join_origins: Vec<_> = join_edges
        .iter()
        .map(|edge| origin_of(edge.buffer))
        .collect();
    assert!(join_origins.contains(&SemanticValueOrigin::MirLocal {
        function: MirFunctionId(0),
        local: 6,
    }));
    assert!(join_origins.contains(&SemanticValueOrigin::MirLocal {
        function: MirFunctionId(0),
        local: 7,
    }));

    // Negative: no gate → up edge in either direction.
    assert!(
        !semantics.dependencies.iter().any(|edge| {
            (edge.producer == gate && edge.consumer == up)
                || (edge.producer == up && edge.consumer == gate)
        }),
        "gate and up are never serialized against each other"
    );

    // Down: exactly one incoming edge from the join (the fused activation).
    let down_edges: Vec<_> = semantics
        .dependencies
        .iter()
        .filter(|edge| edge.consumer == down)
        .collect();
    assert_eq!(down_edges.len(), 1);
    assert_eq!(down_edges[0].producer, join);

    // The MutRef output param (local 4) binds as the program's final
    // result, produced by the down child's launch.
    let output_origin = SemanticValueOrigin::MirLocal {
        function: MirFunctionId(0),
        local: 4,
    };
    let result = program
        .results
        .iter()
        .find(|result| origin_of(result.buffer.id) == output_origin)
        .expect("the MutRef output param binds as the program result");
    assert_eq!(result.produced_by, down);

    program
        .validate_with_semantics(&semantics)
        .expect("the four-child program passes the shared semantic gate");
}

// ── DPSC-U4a gap 1: the decomposition fallback (oracle: faber
//    device_test.rs `device_program_constructor_decomposes_transformer_
//    recipe_subchain`) ────────────────────────────────────────────────────

/// A multi-recipe body (`m = x.matmul(w); s = m.softmax()`) whose
/// whole-function ABI synthesis fails (the matmul return-buffer
/// element-count law) decomposes at the recipe boundary into one kernel per
/// subchain; the transformer-recipe subchain (softmax) dispatches its
/// signature through the P1 seam and carries the `RowSoftmax` plan with the
/// transformer-shaped resource layout — never the generic return-buffer ABI.
#[test]
fn constructor_decomposes_multi_recipe_body_into_subchain_kernels() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let x8 = rank2_tensor_ty(&mut types, [2, 8]);
    let w8 = rank2_tensor_ty(&mut types, [8, 2]);
    let m2 = rank2_tensor_ty(&mut types, [2, 2]);
    let function = device_function(
        1,
        &mut interner,
        "mixio",
        &[(0, "x", x8), (1, "w", w8), (2, "m", m2), (3, "s", m2)],
        &[0, 1],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, m2),
            collection_statement(MirCollectionOp::TensorSoftmax, &[2], 3, m2),
        ],
        Some(MirOperand::Place(MirPlace::local(MirLocalId(3)))),
        Vec::new(),
        m2,
        true,
    );
    let validated = validated(&types, vec![function]);

    let (program, semantics, _) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("the multi-recipe body decomposes instead of failing closed");

    assert_eq!(
        program.kernels.len(),
        2,
        "matmul + softmax subchain kernels"
    );
    let softmax_kernel = program
        .kernels
        .iter()
        // ca2499dc: function 1's second subchain is `k1__1`.
        .find(|kernel| kernel.entry == "k1__1")
        .expect("the softmax subchain kernel exists");
    assert!(matches!(
        softmax_kernel.plan,
        CollectionKernelPlan::RowSoftmax(_)
    ));
    // The transformer-shaped emission signature: the recipe operand input at
    // binding 0 and the primary output at binding 1, both at the [2,2]
    // device-view element count.
    let element_counts: Vec<(u32, u64)> = softmax_kernel
        .resources
        .iter()
        .map(|resource| (resource.binding.binding, resource.version.element_count))
        .collect();
    assert!(
        element_counts
            .iter()
            .any(|&(binding, count)| binding == 0 && count == 4),
        "the recipe operand input binds at 0 with the [2,2] count: {element_counts:?}"
    );
    assert!(
        element_counts
            .iter()
            .any(|&(binding, count)| binding == 1 && count == 4),
        "the primary output binds at 1 with the [2,2] count: {element_counts:?}"
    );
    program
        .validate_with_semantics(&semantics)
        .expect("the decomposed program passes the shared semantic gate");
}

/// A runtime-shaped elementwise tail: two matmuls (`m = a · b`, `n = a · c`)
/// write run-time-shaped locals, and the elementwise join `out = m + n`
/// writes the run-time-shaped mut out param. The whole-function ABI refuses
/// the run-time dims, so the body decomposes at the recipe boundaries; the
/// join subchain is elementwise-only and must take the runtime-aware
/// elementwise emission signature instead of the literal-only ABI synthesis
/// (which cannot size a run-time-shaped buffer).
#[test]
fn constructor_decomposes_a_runtime_shaped_elementwise_tail() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
    use crate::kernel_plan::MatMulDims;

    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    // The run-time-extent ABI binds each consumed axis as a `u32` extent
    // resource, so the fixture's type table carries that width.
    let _extent = u32_ty(&mut types);
    let a = rank2_tensor_ty_runtime(&mut types, [0, 1]);
    let b = rank2_tensor_ty_runtime(&mut types, [1, 2]);
    let out = rank2_tensor_ty_runtime(&mut types, [0, 2]);
    let mut function = device_function(
        1,
        &mut interner,
        "mlp",
        &[
            (0, "a", a),
            (1, "b", b),
            (2, "c", b),
            (3, "out", out),
            (4, "m", out),
            (5, "n", out),
        ],
        &[0, 1, 2, 3],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 4, out),
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 2], 5, out),
            collection_statement(MirCollectionOp::TensorAdd, &[4, 5], 3, out),
        ],
        Some(MirOperand::Place(MirPlace::local(MirLocalId(3)))),
        Vec::new(),
        out,
        true,
    );
    // `out` is the written destination (the MLP's mut out param).
    function.params[3].mode = MirParamMode::MutRef;
    let validated = validated(&types, vec![function]);

    let (program, semantics, _) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("the runtime-shaped tail decomposes instead of failing closed");

    assert_eq!(
        program.kernels.len(),
        3,
        "two matmul subchain kernels plus the elementwise tail: {:?}",
        program
            .kernels
            .iter()
            .map(|kernel| (kernel.entry.as_str(), kernel.plan.clone()))
            .collect::<Vec<_>>()
    );
    let matmul_kernel = program
        .kernels
        .iter()
        .find(|kernel| matches!(kernel.plan, CollectionKernelPlan::TiledMatMul(_)))
        .expect("the matmul subchain kernel exists");
    let CollectionKernelPlan::TiledMatMul(plan) = &matmul_kernel.plan else {
        panic!("the matmul subchain carries the tiled matmul plan");
    };
    assert!(
        matches!(plan.dims, MatMulDims::Runtime { .. }),
        "the run-time dims survive into the matmul plan: {:?}",
        plan.dims
    );
    assert!(
        matmul_kernel
            .resources
            .iter()
            .any(|resource| resource.version.element_count == RUNTIME_SHAPED_ELEMENT_COUNT),
        "the matmul resources carry the runtime marker"
    );

    let tail = program
        .kernels
        .iter()
        .find(|kernel| kernel.entry == "k1__2")
        .expect("the elementwise tail subchain kernel exists");
    assert_eq!(tail.plan, CollectionKernelPlan::Elementwise);
    assert!(
        !tail.resources.is_empty()
            && tail
                .resources
                .iter()
                .all(|resource| resource.version.element_count == RUNTIME_SHAPED_ELEMENT_COUNT),
        "every tail resource binds the runtime marker: {:?}",
        tail.resources
            .iter()
            .map(|resource| resource.version.element_count)
            .collect::<Vec<_>>()
    );
    assert!(
        tail.resources
            .iter()
            .all(|resource| resource.version.shape.is_none()),
        "a run-time-shaped buffer carries no literal shape"
    );
    program
        .validate_with_semantics(&semantics)
        .expect("the decomposed program passes the shared semantic gate");
}

/// An indexed-view kernel `(x, w, v, b, mut y, id) → vacuum` with two matmuls
/// and a fused bias tail (`c = x·w; e = c·v; d = e + b; y ⇇ d`): the body
/// decomposes into one kernel per matmul, and the store to `y` must land in
/// the final subchain's output instead of a dead local.
#[test]
fn constructor_binds_the_final_subchain_to_the_explicit_output_parameter() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let x = rank2_tensor_ty(&mut types, [4, 3]);
    let w = rank2_tensor_ty(&mut types, [3, 2]);
    let v = rank2_tensor_ty(&mut types, [2, 2]);
    let r2 = rank2_tensor_ty(&mut types, [4, 2]);
    let id = u32_ty(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let mut function = device_function(
        1,
        &mut interner,
        "linear_two",
        &[
            (0, "x", x),
            (1, "w", w),
            (2, "v", v),
            (3, "b", r2),
            (4, "y", r2),
            (5, "id", id),
            (6, "c", r2),
            (7, "e", r2),
            (8, "d", r2),
        ],
        &[0, 1, 2, 3, 4, 5],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 6, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[6, 2], 7, r2),
            collection_statement(MirCollectionOp::TensorAdd, &[7, 3], 8, r2),
            copy_statement(4, 8, r2),
        ],
        None,
        Vec::new(),
        vacuum,
        true,
    );
    function.params[4].mode = MirParamMode::MutRef;
    let validated = validated(&types, vec![function]);

    let (program, semantics, _) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("the two-matmul output-parameter body decomposes instead of failing closed");

    assert_eq!(program.kernels.len(), 2, "one kernel per matmul");
    let names = |kernel: usize, access: MirKernelResourceAccess| -> Vec<(u32, String)> {
        program.kernels[kernel]
            .resources
            .iter()
            .filter(|resource| resource.access == access)
            .map(|resource| (resource.binding.binding, resource.buffer.name.clone()))
            .collect()
    };
    assert_eq!(
        names(0, MirKernelResourceAccess::Write),
        vec![(2, "c".to_owned())],
        "the first matmul's product is the carried data-flow buffer"
    );
    assert_eq!(
        names(1, MirKernelResourceAccess::Read),
        vec![
            (0, "c".to_owned()),
            (1, "v".to_owned()),
            (2, "b".to_owned())
        ],
        "the final kernel reads the carried product, the right operand and the fused bias"
    );
    assert_eq!(
        names(1, MirKernelResourceAccess::Write),
        vec![(3, "y".to_owned())],
        "the final kernel's output is the declared output parameter, not the recipe result"
    );
    // `y` is the caller's buffer, the `c` buffer is shared by both kernels.
    let written = |kernel: usize| {
        program.kernels[kernel]
            .resources
            .iter()
            .find(|resource| resource.access == MirKernelResourceAccess::Write)
            .map(|resource| resource.buffer.id)
            .expect("each kernel writes one buffer")
    };
    let carried = program.kernels[1]
        .resources
        .iter()
        .find(|resource| resource.binding.binding == 0)
        .map(|resource| resource.buffer.id)
        .expect("the final kernel reads the carried product");
    assert_eq!(written(0), carried, "kernel 1 reads what kernel 0 wrote");
    assert_ne!(
        written(1),
        carried,
        "the output parameter is its own buffer"
    );
    program
        .validate_with_semantics(&semantics)
        .expect("the decomposed program passes the shared semantic gate");
}

/// A store to the output parameter that no kernel can carry (the first
/// matmul's subchain holds it, the second returns `e`) fails closed at the
/// constructor with a named refusal instead of dropping the store.
#[test]
fn constructor_refuses_an_output_parameter_stored_before_the_final_subchain() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let x = rank2_tensor_ty(&mut types, [4, 3]);
    let w = rank2_tensor_ty(&mut types, [3, 2]);
    let v = rank2_tensor_ty(&mut types, [2, 2]);
    let r2 = rank2_tensor_ty(&mut types, [4, 2]);
    let id = u32_ty(&mut types);
    let mut function = device_function(
        1,
        &mut interner,
        "linear_two",
        &[
            (0, "x", x),
            (1, "w", w),
            (2, "v", v),
            (4, "y", r2),
            (5, "id", id),
            (6, "c", r2),
            (7, "e", r2),
        ],
        &[0, 1, 2, 4, 5],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 6, r2),
            copy_statement(4, 6, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[6, 2], 7, r2),
        ],
        Some(MirOperand::Place(MirPlace::local(MirLocalId(7)))),
        Vec::new(),
        r2,
        true,
    );
    function.params[3].mode = MirParamMode::MutRef;
    let validated = validated(&types, vec![function]);

    let error = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect_err("the early store to the output parameter has no kernel to carry it");
    assert!(
        error.to_string().contains("only the final subchain"),
        "named refusal: {error}"
    );
}

// ── DPSC-U4a gap 3: dependency-ordered launches (oracle: faber
//    device_test.rs `launches_follow_carried_dependencies_not_declaration_
//    order`) ─────────────────────────────────────────────────────────────

/// Parameter names of the reversed-chain kernels (NAMEJOIN NJ2): buffer
/// identity comes from the host value, never from these names, so a fixture
/// may name them anything.
struct ChainNames {
    /// `recollige(medius, result, id)` and its scalar local.
    consumer: [&'static str; 4],
    /// `collige(a, medius, id)` and its scalar local.
    producer: [&'static str; 4],
}

const ORIGINAL_CHAIN_NAMES: ChainNames = ChainNames {
    consumer: ["medius", "result", "id", "total"],
    producer: ["a", "medius", "id", "total"],
};

/// Every kernel parameter renamed (distinct from the originals AND from each
/// other across the two kernels).
const RENAMED_CHAIN_NAMES: ChainNames = ChainNames {
    consumer: ["mid_in", "res_out", "lane_c", "acc_c"],
    producer: ["src_a", "mid_out", "lane_p", "acc_p"],
};

/// A host `Call` statement of `callee` with bare-local arguments.
fn host_call(callee: u32, args: &[u32]) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call {
            destination: None,
            callee: crate::MirCallee::Function(MirFunctionId(callee)),
            args: args
                .iter()
                .map(|&local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
                .collect(),
        },
        span: span(),
    }
}

/// The reversed chain WITH a host (the NJ2 shape of the faber
/// `u4-reversed-chain` oracle): kernels DECLARED consumer-first, then a host
/// `main` with locals a[1024], medius[4], result[1] and an id that calls
/// `collige(a, medius, id)` then `recollige(medius, result, id)`.
fn reversed_chain_functions(
    types: &mut TypeTable,
    interner: &mut Interner,
    names: &ChainNames,
) -> Vec<MirFunction> {
    let r1024 = rank1_tensor_ty(types, 1024);
    let r4 = rank1_tensor_ty(types, 4);
    let r1 = rank1_tensor_ty(types, 1);
    let scalar = f32(types);
    let index = u32_ty(types);
    let vacuum = vacuum_ty(types);
    // Declared first: recollige(medius, result, id) — reduces medius and
    // stores into result.
    let consumer = device_function(
        1,
        interner,
        "recollige",
        &[
            (0, names.consumer[0], r4),
            (1, names.consumer[1], r1),
            (2, names.consumer[2], index),
            (3, names.consumer[3], scalar),
        ],
        &[0, 1, 2],
        vec![
            collection_statement(MirCollectionOp::Sum, &[0], 3, scalar),
            index_store_statement(1, 2, 3, scalar),
        ],
        None,
        Vec::new(),
        vacuum,
        true,
    );
    // Declared second: collige(a, medius, id) — reduces a (1024 → 4 partials,
    // the medius count) and stores into medius.
    let producer = device_function(
        2,
        interner,
        "collige",
        &[
            (0, names.producer[0], r1024),
            (1, names.producer[1], r4),
            (2, names.producer[2], index),
            (3, names.producer[3], scalar),
        ],
        &[0, 1, 2],
        vec![
            collection_statement(MirCollectionOp::Sum, &[0], 3, scalar),
            index_store_statement(1, 2, 3, scalar),
        ],
        None,
        Vec::new(),
        vacuum,
        true,
    );
    // The host passes ONE value (`medius`, local 21) as collige's output and
    // as recollige's input: that value identity wires the buffer.
    let main = device_function(
        3,
        interner,
        "main",
        &[
            (20, "a", r1024),
            (21, "medius", r4),
            (22, "result", r1),
            (23, "id", index),
        ],
        &[],
        vec![host_call(2, &[20, 21, 23]), host_call(1, &[21, 22, 23])],
        None,
        Vec::new(),
        vacuum,
        false,
    );
    vec![consumer, producer, main]
}

fn build_reversed_chain(names: &ChainNames) -> (DeviceProgram, DeviceSemantics) {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let functions = reversed_chain_functions(&mut types, &mut interner, names);
    let validated = validated(&types, functions);
    let (program, semantics, _) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("the reversed chain builds");
    (program, semantics)
}

/// The launch sequence follows the carried producer/consumer dependency
/// graph, never kernel declaration order: a chain DECLARED backwards
/// (consumer first) still executes producer → consumer. The chain is wired by
/// the host value `medius`, which the kernels name differently.
#[test]
fn constructor_launch_sequence_follows_dependencies_not_declaration_order() {
    let (program, semantics) = build_reversed_chain(&RENAMED_CHAIN_NAMES);

    // recollige is DECLARED first (kernel 0, launch id 1) but DEPENDS on
    // collige's write of the host value: the execution sequence runs
    // collige's launch (id 2, kernel 1) before recollige's (id 1, kernel 0).
    assert_eq!(
        program
            .launches
            .iter()
            .map(|launch| (launch.id.0, launch.kernel_index))
            .collect::<Vec<_>>(),
        vec![(2, 1), (1, 0)],
        "the launch sequence follows the carried dependency, not declaration order"
    );
    assert_eq!(semantics.roots, vec![crate::device_program::LaunchId(2)]);
    let medius_edges: Vec<_> = semantics
        .dependencies
        .iter()
        .filter(|edge| edge.buffer.0 == 1)
        .collect();
    assert_eq!(
        medius_edges.len(),
        1,
        "one medius producer→consumer edge: {:?}",
        semantics.dependencies
    );
    assert_eq!(medius_edges[0].producer, crate::device_program::LaunchId(2));
    assert_eq!(medius_edges[0].consumer, crate::device_program::LaunchId(1));
    program
        .validate_with_semantics(&semantics)
        .expect("the reordered program passes the shared semantic gate");
}

// ── Need: a host loop's in-place update is a loop carry, not a
//    cycle ────────────────────────────────────────────────────────────────

/// A host `Call` statement of `callee` with bare-local arguments whose result
/// lands in `destination`.
fn host_call_into(callee: u32, args: &[u32], destination: u32) -> MirStatement {
    let mut call = host_call(callee, args);
    if let MirStatementKind::Call {
        destination: slot, ..
    } = &mut call.kind
    {
        *slot = Some(MirPlace::local(MirLocalId(destination)));
    }
    call
}

/// The host-loop shape of an in-place weight update: elementwise kernels
/// DECLARED `upd(y) → w` first and `fwd(w) → y` second, and a host `main`
/// whose body is `y = fwd(w)` then `w = upd(y)`. With `looped` the body sits in
/// a loop (`bb0 → bb1 ⇄ bb2`, exit `bb3`); without it the body runs once.
fn in_place_update_functions(
    types: &mut TypeTable,
    interner: &mut Interner,
    looped: bool,
) -> Vec<MirFunction> {
    let r4 = rank1_tensor_ty(types, 4);
    let kernel = |interner: &mut Interner,
                  id: u32,
                  name: &str,
                  input: &'static str,
                  output: &'static str| {
        device_function(
            id,
            interner,
            name,
            &[(0, input, r4), (1, output, r4)],
            &[0],
            vec![collection_statement(
                MirCollectionOp::TensorMul,
                &[0, 0],
                1,
                r4,
            )],
            Some(MirOperand::Place(MirPlace::local(MirLocalId(1)))),
            Vec::new(),
            r4,
            true,
        )
    };
    let upd = kernel(interner, 1, "upd", "y_in", "w_out");
    let fwd = kernel(interner, 2, "fwd", "w_in", "y_out");
    let mut main = device_function(
        3,
        interner,
        "main",
        &[(20, "w", r4), (21, "y", r4)],
        &[],
        vec![host_call_into(2, &[20], 21), host_call_into(1, &[21], 20)],
        None,
        Vec::new(),
        vacuum_ty(types),
        false,
    );
    if looped {
        let goto = |target: u32| MirTerminator {
            kind: MirTerminatorKind::Goto(MirBlockId(target)),
            span: span(),
        };
        let block = |id: u32, statements: Vec<MirStatement>, terminator: MirTerminator| MirBlock {
            id: MirBlockId(id),
            statements,
            terminator,
            span: span(),
        };
        let body = std::mem::take(&mut main.blocks[0].statements);
        main.blocks = vec![
            block(0, Vec::new(), goto(1)),
            block(
                1,
                Vec::new(),
                MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(crate::MirConstant::Bool(true)),
                        then_block: MirBlockId(2),
                        else_block: MirBlockId(3),
                    },
                    span: span(),
                },
            ),
            block(2, body, goto(1)),
            block(
                3,
                Vec::new(),
                MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: span(),
                },
            ),
        ];
    }
    vec![upd, fwd, main]
}

fn build_in_place_update(
    looped: bool,
) -> Result<(DeviceProgram, DeviceSemantics), super::DeviceProgramBuildError> {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let functions = in_place_update_functions(&mut types, &mut interner, looped);
    let validated = validated(&types, functions);
    build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .map(|(program, semantics, _)| (program, semantics))
}

/// `fwd(w, y)` then `upd(y, w)` in a host loop: `fwd` reads the `w` that `upd`
/// wrote one iteration earlier. That edge is the loop carry, so the launch
/// sequence is the call order (`fwd` before `upd`), although `upd` is declared
/// first, and only the in-iteration `y` edge remains.
#[test]
fn an_in_place_update_in_a_host_loop_is_a_loop_carry_not_a_cycle() {
    let (program, semantics) = build_in_place_update(true).expect("a loop-carried update builds");
    // fwd is declared second (kernel 1, launch id 2) and runs first.
    assert_eq!(
        program
            .launches
            .iter()
            .map(|launch| (launch.id.0, launch.kernel_index))
            .collect::<Vec<_>>(),
        vec![(2, 1), (1, 0)],
        "the launch sequence follows the host call order"
    );
    assert_eq!(semantics.roots, vec![crate::device_program::LaunchId(2)]);
    assert_eq!(
        semantics
            .dependencies
            .iter()
            .map(|edge| (edge.producer.0, edge.consumer.0))
            .collect::<Vec<_>>(),
        vec![(2, 1)],
        "only the in-iteration y edge orders a launch: {:?}",
        semantics.dependencies
    );
    program
        .validate_with_semantics(&semantics)
        .expect("the loop-carried program passes the shared semantic gate");
}

/// The same two calls run once, with no loop, carry nothing: the `w` edge is
/// not a loop carry and the cycle still fails closed.
#[test]
fn the_same_calls_outside_a_loop_still_fail_as_a_cycle() {
    let error = build_in_place_update(false).expect_err("straight-line code has no carry");
    assert!(
        error.to_string().contains("contains a cycle"),
        "named refusal: {error}"
    );
}

// ── DPSC-U4a gap 2 + 4 fixtures: a minimal training program (forward loss,
//    generated companion, train_step) mirroring the faber S5-U5 oracle ────

const FORWARD_FN: u32 = 1;
const STEP_FN: u32 = 2;
const COMPANION_FN: u32 = 3;
const PRIMAL_DEF: DefId = DefId(8);
const COMPANION_DEF: DefId = DefId(9);

/// loss(x, w) = x*w — the device-resident forward (its return output is the
/// declared per-step observation).
fn forward_function(types: &mut TypeTable, interner: &mut Interner) -> MirFunction {
    let r2 = rank1_tensor_ty(types, 2);
    device_function(
        FORWARD_FN,
        interner,
        "loss",
        &[(0, "x", r2), (1, "w", r2), (2, "product", r2)],
        &[0, 1],
        vec![collection_statement(
            MirCollectionOp::TensorMul,
            &[0, 1],
            2,
            r2,
        )],
        Some(MirOperand::Place(MirPlace::local(MirLocalId(2)))),
        Vec::new(),
        r2,
        true,
    )
}

/// `train_step(w`, gw) = (w - gw) — the optimizer step; its tuple update
/// output aliases the parameter buffer (the in-place update, S5-U5).
fn train_step_function(types: &mut TypeTable, interner: &mut Interner) -> MirFunction {
    let r2 = rank1_tensor_ty(types, 2);
    let tuple_ty = MirType::semantic(types.tuple(vec![r2.semantic_id()]));
    let (construct, temp) = tuple_construct(&[2], tuple_ty);
    device_function(
        STEP_FN,
        interner,
        "train_step",
        &[(0, "w", r2), (1, "gw", r2), (2, "nw", r2)],
        &[0, 1],
        vec![
            collection_statement(MirCollectionOp::TensorSub, &[0, 1], 2, r2),
            construct,
        ],
        Some(MirOperand::Temp(temp)),
        vec![crate::MirTemp {
            id: temp,
            ty: tuple_ty,
            span: span(),
        }],
        tuple_ty,
        false,
    )
}

/// The generated backward companion: `loss_backward(x`, w, upstream) =
/// (gx, gw) — a tuple gradient return whose slots the gradient-flow link
/// feeds into the `train_step`.
/// The compute flag selects the companion's shader stage, so post-GT12 drivers
/// can run it as Compute.
fn companion_function(
    types: &mut TypeTable,
    interner: &mut Interner,
    compute: bool,
) -> MirFunction {
    let r2 = rank1_tensor_ty(types, 2);
    let tuple_ty = MirType::semantic(types.tuple(vec![r2.semantic_id(), r2.semantic_id()]));
    let (construct, temp) = tuple_construct(&[3, 4], tuple_ty);
    let mut function = device_function(
        COMPANION_FN,
        interner,
        "loss_backward",
        &[
            (0, "x", r2),
            (1, "w", r2),
            (2, "upstream", r2),
            (3, "gx", r2),
            (4, "gw", r2),
        ],
        &[0, 1, 2],
        vec![
            collection_statement(MirCollectionOp::TensorMul, &[0, 2], 3, r2),
            collection_statement(MirCollectionOp::TensorMul, &[1, 2], 4, r2),
            construct,
        ],
        Some(MirOperand::Temp(temp)),
        vec![crate::MirTemp {
            id: temp,
            ty: tuple_ty,
            span: span(),
        }],
        tuple_ty,
        compute,
    );
    function.source = Some(COMPANION_DEF);
    function
}

fn training_companions(tensor_id: TypeId) -> MirCompanionMap {
    let mut map = MirCompanionMap::default();
    map.insert(LosslessMirCompanionEntry {
        primal: PRIMAL_DEF,
        companion: COMPANION_DEF,
        derivative: crate::device::MirCompanionDerivativeKind::ReverseModeVjp,
        device_resident: true,
        selected_inputs: vec![
            MirCompanionSelectedInput {
                param: DefId(100),
                position: 0,
                ty: tensor_id,
                gradient_slot: 0,
            },
            MirCompanionSelectedInput {
                param: DefId(101),
                position: 1,
                ty: tensor_id,
                gradient_slot: 1,
            },
        ],
        selected_outputs: vec![MirCompanionSelectedOutput {
            position: 0,
            ty: tensor_id,
            upstream_gradient_ty: tensor_id,
        }],
    });
    map
}

fn training_facts() -> DeviceTrainingFacts {
    DeviceTrainingFacts {
        step_count: 100,
        device_function_ids: vec![MirFunctionId(STEP_FN)],
        persistent_buffers: BTreeSet::from_iter([(MirFunctionId(STEP_FN), MirLocalId(0))]),
        trainable: vec![DeviceTrainableParam {
            step_function: MirFunctionId(STEP_FN),
            param_position: 0,
            update_output: 0,
        }],
        gradients: vec![DeviceGradientLink {
            step_function: MirFunctionId(STEP_FN),
            gradient_position: 1,
            companion: COMPANION_DEF,
            gradient_slot: 1,
        }],
    }
}

/// `compute_companion` selects the companion's shader stage (Compute when true).
fn training_fixture<R>(
    compute_companion: bool,
    run: impl FnOnce(&Interner, &ValidatedMir<'_>, &MirCompanionMap) -> R,
) -> R {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor_id = rank1_tensor_ty(&mut types, 2).semantic_id();
    let forward = forward_function(&mut types, &mut interner);
    let step = train_step_function(&mut types, &mut interner);
    let companion = companion_function(&mut types, &mut interner, compute_companion);
    let companions = training_companions(tensor_id);
    let validated = validated(&types, vec![forward, step, companion]);
    run(&interner, &validated, &companions)
}

// ── DPSC-U4a gap 2: param-update/gradient aliasing (oracle: faber
//    device_test.rs `repeating_step_constructor_materializes_training_
//    program`) ───────────────────────────────────────────────────────────

/// The `train_step` kernel binds the trainable param as an `InOut` slot AND
/// aliases its update output slot to the same buffer (the in-place update);
/// the param is `PerProgram` with `HostProvided` init and carries no
/// dependency edge (persistent state); the gradient input aliases the
/// companion's gradient output buffer (the backward → `train_step` edge).
#[test]
fn constructor_aliases_param_updates_and_gradients() {
    let facts = training_facts();

    training_fixture(false, |interner, validated, companions| {
        let (program, semantics, _) = build_device_program(
            validated,
            interner,
            companions,
            Some(&facts),
            DeviceProgramLifetime::RepeatingStep,
            100,
            &fixture_metadata(&validated.program().functions),
        )
        .expect("the training program builds");

        program.validate().expect("constructed program validates");
        let step_kernel = program
            .kernels
            .iter()
            .find(|kernel| kernel.function == MirFunctionId(STEP_FN))
            .expect("the train_step kernel exists");
        assert_eq!(step_kernel.entry, "k2", "the step entry is neutral k<id>");

        // The trainable param `w` projects to ONE buffer with a read slot and
        // a write slot in the step kernel (the in-place update).
        let param_id = step_kernel
            .resources
            .iter()
            .find(|resource| resource.buffer.name == "w")
            .expect("the step kernel reads the param")
            .buffer
            .id;
        let slots: Vec<_> = step_kernel
            .resources
            .iter()
            .filter(|resource| resource.buffer.id == param_id)
            .collect();
        assert_eq!(
            slots.len(),
            2,
            "the train_step kernel reads and writes the param buffer (in-place update)"
        );
        for slot in &slots {
            assert_eq!(slot.buffer.role, BufferRole::InOut);
            assert_eq!(slot.buffer.lifetime, BufferLifetime::PerProgram);
        }
        assert!(
            slots
                .iter()
                .any(|slot| slot.access == MirKernelResourceAccess::Read)
                && slots
                    .iter()
                    .any(|slot| slot.access == MirKernelResourceAccess::Write),
            "the param buffer has a read slot and a write slot in the step kernel"
        );

        // The forward kernel reads the param as a plain read of the same buffer.
        let forward_kernel = program
            .kernels
            .iter()
            .find(|kernel| kernel.function == MirFunctionId(FORWARD_FN))
            .expect("the forward kernel exists");
        assert_eq!(
            forward_kernel.entry, "k1",
            "the forward entry is neutral k<id>"
        );
        let forward_reads: Vec<_> = forward_kernel
            .resources
            .iter()
            .filter(|resource| resource.buffer.id == param_id)
            .collect();
        assert!(
            !forward_reads.is_empty()
                && forward_reads
                    .iter()
                    .all(|slot| slot.access == MirKernelResourceAccess::Read),
            "the forward reads the param without writing it"
        );

        // The gradient input `gw` aliases the companion's gradient output
        // buffer (the backward → train_step data-flow edge).
        let companion_kernel = program
            .kernels
            .iter()
            .find(|kernel| kernel.function == MirFunctionId(COMPANION_FN))
            .expect("the companion kernel exists");
        assert_eq!(
            companion_kernel.entry, "k3",
            "the companion entry is neutral k<id>"
        );
        let gw_buffer = step_kernel
            .resources
            .iter()
            .find(|resource| resource.buffer.name == "gw")
            .expect("the step kernel reads the gradient")
            .buffer
            .id;
        assert!(
            companion_kernel
                .resources
                .iter()
                .any(|resource| resource.buffer.id == gw_buffer
                    && resource.access == MirKernelResourceAccess::Write),
            "the companion's gradient write and the step's gradient read are one buffer"
        );

        // The params mint HostProvided init (never ZeroFill despite the InOut
        // access) and carry no dependency edge (the persistent-state
        // exclusion).
        for fact in &semantics.initializations {
            if fact.buffer == param_id {
                assert_eq!(
                    fact.policy,
                    crate::device_semantics::InitializationPolicy::HostProvided,
                    "the param buffer is HostProvided, copied in exactly once"
                );
            }
        }
        assert!(
            !semantics
                .dependencies
                .iter()
                .any(|edge| edge.buffer == param_id),
            "no param edge rides the dependency graph (persistent state)"
        );
    });
}

// ── DPSC-U4a gap 4: S5A-U1 declared cadence/results + step count ────────

/// The DECLARED observation cadence set: the forward's return output (the
/// loss) is the single `PerStep` result; the final params are declared
/// `EndOfRun`; the effective step count is the training facts' admitted count
/// (the caller's declared value when there is no training program).
#[test]
fn constructor_declares_observation_cadence_and_effective_steps() {
    let facts = training_facts();

    training_fixture(false, |interner, validated, companions| {
        let (program, _semantics, effective_steps) = build_device_program(
            validated,
            interner,
            companions,
            Some(&facts),
            DeviceProgramLifetime::RepeatingStep,
            100,
            &fixture_metadata(&validated.program().functions),
        )
        .expect("the training program builds");

        assert_eq!(program.lifetime, DeviceProgramLifetime::RepeatingStep);
        assert_eq!(
            effective_steps, 100,
            "the effective step count is the training facts' admitted count"
        );

        // The single PerStep observation: the forward's return output.
        let per_step: Vec<_> = program
            .results
            .iter()
            .filter(|result| result.cadence == ObservationCadence::PerStep)
            .collect();
        assert_eq!(per_step.len(), 1, "one per-step loss observation");
        assert_eq!(per_step[0].buffer.role, BufferRole::Output);

        // The params are declared EndOfRun finals (alongside the final
        // forward / gradient outputs — every other written final).
        let end_of_run: Vec<_> = program
            .results
            .iter()
            .filter(|result| result.cadence == ObservationCadence::EndOfRun)
            .collect();
        assert!(
            end_of_run.iter().any(|result| result.buffer.name == "w"),
            "the final params are declared EndOfRun: {:?}",
            program.results
        );

        // No training facts: the caller's declared count applies unchanged.
        let (_plain, _plain_semantics, plain_steps) = build_device_program(
            validated,
            interner,
            &MirCompanionMap::default(),
            None,
            DeviceProgramLifetime::SingleRun,
            7,
            &fixture_metadata(&validated.program().functions),
        )
        .expect("the non-training program builds");
        assert_eq!(plain_steps, 7, "no training facts → the declared count");
    });
}

/// S5A-U1 forward-set purity: a device-resident companion that carries the
/// compute stage (the post-GT12 driver shape) is still NOT a forward kernel —
/// it is admitted through the companion map, not a metadata row, and the
/// per-step observation must stay the FORWARD kernel's return output, never
/// the companion's first gradient write.
#[test]
fn forward_set_excludes_compute_stage_companions_from_per_step_observation() {
    let facts = training_facts();

    training_fixture(true, |interner, validated, companions| {
        // Metadata covers the forward and step kernels only: the real driver
        // admits the companion through the companion map, not a metadata row.
        let declared: Vec<MirFunction> = validated
            .program()
            .functions
            .iter()
            .filter(|function| function.id != MirFunctionId(COMPANION_FN))
            .cloned()
            .collect();
        let metadata = fixture_metadata(&declared);

        let (program, _semantics, _) = build_device_program(
            validated,
            interner,
            companions,
            Some(&facts),
            DeviceProgramLifetime::RepeatingStep,
            100,
            &metadata,
        )
        .expect("the training program builds with a compute-stage companion");

        let per_step: Vec<_> = program
            .results
            .iter()
            .filter(|result| result.cadence == ObservationCadence::PerStep)
            .collect();
        assert_eq!(per_step.len(), 1, "one per-step loss observation");
        // The forward's return local (`product`, local 2) is the kernel's
        // ABI output — `output_2` — never the companion's first gradient
        // write (`gx`).
        assert_eq!(
            per_step[0].buffer.name, "output_2",
            "the observation is the forward's return output, not the companion's `gx` write: {:?}",
            program.results
        );
        let launch = program
            .launches
            .iter()
            .find(|launch| launch.id == per_step[0].produced_by)
            .expect("the observation's producing launch exists");
        assert_eq!(
            program.kernels[launch.kernel_index].function,
            MirFunctionId(FORWARD_FN),
            "the observed output is produced by the forward kernel"
        );
    });
}

/// The empty-device-set early return must still report the admitted
/// training step count: the declared count is the caller's fallback only
/// when there are no training facts at all.
#[test]
fn empty_device_set_returns_training_step_count_not_declared_steps() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let scalar = f32(&mut types);
    let host_only = device_function(
        1,
        &mut interner,
        "host_only",
        &[(0, "value", scalar)],
        &[0],
        Vec::new(),
        Some(MirOperand::Place(MirPlace::local(MirLocalId(0)))),
        Vec::new(),
        scalar,
        false,
    );
    let validated = validated(&types, vec![host_only]);
    let facts = DeviceTrainingFacts {
        step_count: 5,
        ..DeviceTrainingFacts::default()
    };

    let (program, _semantics, effective_steps) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        Some(&facts),
        DeviceProgramLifetime::RepeatingStep,
        9,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("a program with no device body builds the empty pair");

    assert!(program.kernels.is_empty(), "no device body, no kernels");
    assert_eq!(
        effective_steps, 5,
        "the training facts' admitted count wins over the declared count"
    );
}

/// An elementwise-only body is ONE whole-function kernel whose resources bind
/// in the source parameter order — the order every emitter lays out the
/// kernel's buffer indices in. `rk(x, v) = v ⊙ v + x` reads `v` before `x`;
/// the read-order decomposition synthetic would bind `v` at slot 0 while the
/// emitted kernel reads `x` there, silently swapping the operands on device.
#[test]
fn elementwise_only_body_binds_resources_in_source_param_order() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor = rank1_tensor_ty(&mut types, 8);
    let mut function = device_function(
        1,
        &mut interner,
        "rk",
        &[
            (0, "x", tensor),
            (1, "v", tensor),
            (2, "t", tensor),
            (3, "r", tensor),
        ],
        &[0, 1],
        vec![
            collection_statement(MirCollectionOp::TensorMul, &[1, 1], 2, tensor),
            collection_statement(MirCollectionOp::TensorAdd, &[2, 0], 3, tensor),
        ],
        Some(MirOperand::Place(MirPlace::local(MirLocalId(3)))),
        Vec::new(),
        tensor,
        true,
    );
    function.source = Some(DefId(1));
    let validated = validated(&types, vec![function]);

    let (program, _semantics, _steps) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("elementwise body materializes");

    assert_eq!(program.kernels.len(), 1);
    let kernel = &program.kernels[0];
    assert_eq!(kernel.plan, CollectionKernelPlan::Elementwise);
    let slots: Vec<(u32, &str, BufferRole)> = kernel
        .resources
        .iter()
        .map(|resource| {
            (
                resource.binding.binding,
                resource.buffer.name.as_str(),
                resource.buffer.role,
            )
        })
        .collect();
    assert_eq!(
        slots[..2],
        [(0, "x", BufferRole::Input), (1, "v", BufferRole::Input)],
        "input slots must follow the source parameter order"
    );
    assert_eq!(slots.len(), 3);
    assert_eq!(slots[2].0, 2);
    assert_eq!(slots[2].2, BufferRole::Output);
}

fn lane_assign(place: MirPlace, id: u32, kind: crate::MirValueKind, ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place,
            value: crate::MirValue {
                id: crate::MirValueId(id),
                kind,
                ty,
                span: span(),
            },
        },
        span: span(),
    }
}

/// A lane form's domain comes from the exact `for from src` source, not the
/// first readable ABI resource. `other` precedes `src` and has a different
/// shape; the constructor must preserve source identity through the carrier.
#[test]
fn lane_grid_plan_is_selected_for_the_lane_form_shape() {
    use crate::{MirBinOp, MirTemp, MirTempId, MirValueKind};
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let other_tensor = rank1_tensor_ty(&mut types, 3);
    let tensor = rank2_tensor_ty(&mut types, [7, 5]);
    let u32t = u32_ty(&mut types);
    let boolean = MirType::semantic(types.primitive(Primitive::Bivalens));
    let float = f32(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let local = |id: u32| MirOperand::Place(MirPlace::local(MirLocalId(id)));
    let temp = |id: u32| MirOperand::Temp(MirTempId(id));
    let binary = |op, lhs, rhs| MirValueKind::Binary { op, lhs, rhs };
    let temps: Vec<MirTemp> = [
        (0, u32t),
        (1, u32t),
        (2, u32t),
        (3, boolean),
        (4, u32t),
        (5, u32t),
        (6, u32t),
        (7, float),
    ]
    .into_iter()
    .map(|(id, ty)| MirTemp {
        id: MirTempId(id),
        ty,
        span: span(),
    })
    .collect();
    let index = |base: u32, at: MirOperand| MirPlace {
        base: crate::MirPlaceBase::Local(MirLocalId(base)),
        projections: vec![crate::MirProjection::Index(at)],
    };
    let entry = vec![
        lane_assign(
            MirPlace::temp(MirTempId(0)),
            0,
            MirValueKind::TensorExtent {
                tensor: local(1),
                axis: 0,
            },
            u32t,
        ),
        lane_assign(
            MirPlace::temp(MirTempId(1)),
            1,
            MirValueKind::TensorExtent {
                tensor: local(1),
                axis: 1,
            },
            u32t,
        ),
        lane_assign(
            MirPlace::temp(MirTempId(2)),
            2,
            binary(MirBinOp::Mul, temp(0), temp(1)),
            u32t,
        ),
        lane_assign(
            MirPlace::temp(MirTempId(3)),
            3,
            binary(MirBinOp::Lt, local(3), temp(2)),
            boolean,
        ),
    ];
    let body = vec![
        lane_assign(
            MirPlace::temp(MirTempId(4)),
            4,
            binary(MirBinOp::Div, local(3), temp(1)),
            u32t,
        ),
        lane_assign(
            MirPlace::temp(MirTempId(5)),
            5,
            binary(MirBinOp::Mod, local(3), temp(1)),
            u32t,
        ),
        lane_assign(
            MirPlace::temp(MirTempId(6)),
            6,
            binary(MirBinOp::Add, temp(4), temp(5)),
            u32t,
        ),
        lane_assign(
            MirPlace::temp(MirTempId(7)),
            7,
            MirValueKind::Operand(MirOperand::Place(index(1, local(3)))),
            float,
        ),
        lane_assign(index(2, local(3)), 8, MirValueKind::Operand(temp(7)), float),
    ];
    let block = |id: u32, statements, kind| MirBlock {
        id: MirBlockId(id),
        statements,
        terminator: MirTerminator { kind, span: span() },
        span: span(),
    };
    let names = [
        interner.intern("other"),
        interner.intern("src"),
        interner.intern("dst"),
        interner.intern("lane"),
    ];
    let fn_name = interner.intern("lane_copy");
    let param = |id: u32, ty, mode| MirParam {
        local: MirLocalId(id),
        name: Some(names[id as usize]),
        ty,
        mode,
        span: span(),
    };
    let local_decl = |id: u32, ty, mutable| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable,
        span: span(),
    };
    let function = MirFunction {
        id: MirFunctionId(1),
        source: Some(DefId(1)),
        name: Some(fn_name),
        params: vec![
            param(0, other_tensor, MirParamMode::Ref),
            param(1, tensor, MirParamMode::Ref),
            param(2, tensor, MirParamMode::MutRef),
            param(3, u32t, MirParamMode::Lane),
        ],
        locals: vec![
            local_decl(0, other_tensor, false),
            local_decl(1, tensor, false),
            local_decl(2, tensor, true),
            local_decl(3, u32t, false),
        ],
        temps,
        blocks: vec![
            block(
                0,
                entry,
                MirTerminatorKind::Branch {
                    condition: temp(3),
                    then_block: MirBlockId(1),
                    else_block: MirBlockId(2),
                },
            ),
            block(1, body, MirTerminatorKind::Goto(MirBlockId(2))),
            block(2, Vec::new(), MirTerminatorKind::Return(None)),
        ],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: span(),
    };
    let validated = validated(&types, vec![function]);

    // The ABI signature synthesis accepts the shape: inputs, output, builtin.
    let signature =
        crate::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            &validated.program().functions[0],
            validated.validation(),
            &interner,
        )
        .expect("the lane-form shape has an ABI signature");
    assert_eq!(signature.builtin_inputs.len(), 1);
    assert_eq!(signature.builtin_inputs[0].local, MirLocalId(3));

    let mut metadata = fixture_metadata(&validated.program().functions);
    metadata
        .get_mut(&MirFunctionId(1))
        .expect("source kernel metadata")
        .lane_domain_local = Some(MirLocalId(1));

    let (program, _semantics, _steps) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &metadata,
    )
    .expect("the lane-form kernel materializes");
    assert_eq!(program.kernels.len(), 1);
    let kernel = &program.kernels[0];
    assert_eq!(
        kernel.plan,
        CollectionKernelPlan::LaneGrid {
            source_resource: 1,
            rank: 2
        }
    );
    assert_eq!(kernel.resources[0].buffer.name, "other");
    assert_eq!(kernel.resources[0].access, MirKernelResourceAccess::Read);
    assert_eq!(kernel.resources[1].buffer.name, "src");
    assert_eq!(kernel.resources[1].access, MirKernelResourceAccess::Read);
    assert_eq!(kernel.resources[2].buffer.name, "dst");
    assert_eq!(kernel.resources[2].access, MirKernelResourceAccess::Write);
}

/// gpu-reset A2 rung 3: a void function with no `Lane` parameter, no index
/// parameter and one `mut` tensor is sequential. It gets a signature with no
/// builtin input and a 1x1x1 dispatch, and the constructor selects
/// `SingleLane`; the same shape with a trailing `u32` index parameter (the
/// positional `input*/output/index` form) is not single-lane.
#[test]
fn single_lane_plan_is_selected_for_a_function_with_no_lane_loop() {
    use crate::{MirTemp, MirTempId, MirValueKind};
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor = rank1_tensor_ty(&mut types, 8);
    let u32t = u32_ty(&mut types);
    let float = f32(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let temp = |id: u32| MirOperand::Temp(MirTempId(id));
    let index = |base: u32, at: u32| MirPlace {
        base: crate::MirPlaceBase::Local(MirLocalId(base)),
        projections: vec![crate::MirProjection::Index(MirOperand::Constant(
            crate::MirConstant::Int(i64::from(at)),
        ))],
    };
    let body = vec![
        lane_assign(
            MirPlace::temp(MirTempId(0)),
            0,
            MirValueKind::Operand(MirOperand::Place(index(1, 3))),
            float,
        ),
        lane_assign(index(1, 5), 1, MirValueKind::Operand(temp(0)), float),
    ];
    let make = |interner: &mut Interner, with_index: bool| {
        let names = ["scale", "t", "i"].map(|name| interner.intern(name));
        let param = |id: u32, ty, mode| MirParam {
            local: MirLocalId(id),
            name: Some(names[id as usize]),
            ty,
            mode,
            span: span(),
        };
        let local_decl = |id: u32, ty, mutable| MirLocal {
            id: MirLocalId(id),
            name: None,
            ty,
            mutable,
            span: span(),
        };
        let mut params = vec![
            param(0, float, MirParamMode::Ref),
            param(1, tensor, MirParamMode::MutRef),
        ];
        let mut locals = vec![local_decl(0, float, false), local_decl(1, tensor, true)];
        if with_index {
            params.push(param(2, u32t, MirParamMode::Ref));
            locals.push(local_decl(2, u32t, false));
        }
        MirFunction {
            id: MirFunctionId(1),
            source: Some(DefId(1)),
            name: Some(interner.intern("advance")),
            params,
            locals,
            temps: vec![MirTemp {
                id: MirTempId(0),
                ty: float,
                span: span(),
            }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: body.clone(),
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
        }
    };

    let indexed = make(&mut interner, true);
    let validated_indexed = validated(&types, vec![indexed]);
    assert!(
        !crate::abi::MirKernelSignature::is_single_lane_form(
            &validated_indexed.program().functions[0],
            validated_indexed.validation()
        ),
        "a trailing u32 index parameter is the positional indexed form"
    );

    let function = make(&mut interner, false);
    let validated = validated(&types, vec![function]);
    assert!(crate::abi::MirKernelSignature::is_single_lane_form(
        &validated.program().functions[0],
        validated.validation()
    ));
    let signature =
        crate::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            &validated.program().functions[0],
            validated.validation(),
            &interner,
        )
        .expect("the single-lane shape has an ABI signature");
    assert!(signature.builtin_inputs.is_empty());
    assert_eq!(
        (
            signature.dispatch_size.x,
            signature.dispatch_size.y,
            signature.dispatch_size.z
        ),
        (1, 1, 1)
    );
    assert_eq!(signature.output.binding, 1);
    assert_eq!(signature.input.binding, 0);

    let (program, _semantics, _steps) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("the single-lane kernel materializes");
    assert_eq!(program.kernels.len(), 1);
    let kernel = &program.kernels[0];
    assert_eq!(kernel.plan, CollectionKernelPlan::SingleLane);
    assert_eq!(
        (
            kernel.launch.workgroup_count.x,
            kernel.launch.workgroup.x,
            kernel.launch.workgroup.y,
            kernel.launch.workgroup.z
        ),
        (1, 1, 1, 1),
        "one thread"
    );
}

/// gpu-reset C4-2e-a: the lane-copy shape (params `src`, `mut dst`, `Lane`)
/// whose single block reads the given `(parameter local, axis)` pairs through
/// `TensorExtent`, in the given order, and returns. Only the signature matters
/// to the callers, so the body is the extent reads alone.
fn lane_copy_with_extent_reads(reads: &[(u32, u32)]) -> (TypeTable, Interner, MirFunction) {
    use crate::{MirTemp, MirTempId, MirValueKind};
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor = rank2_tensor_ty(&mut types, [7, 5]);
    let u32t = u32_ty(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let temps: Vec<MirTemp> = (0..reads.len() as u32)
        .map(|id| MirTemp {
            id: MirTempId(id),
            ty: u32t,
            span: span(),
        })
        .collect();
    let statements: Vec<MirStatement> = reads
        .iter()
        .enumerate()
        .map(|(index, &(tensor_local, axis))| {
            lane_assign(
                MirPlace::temp(MirTempId(index as u32)),
                index as u32,
                MirValueKind::TensorExtent {
                    tensor: MirOperand::Place(MirPlace::local(MirLocalId(tensor_local))),
                    axis,
                },
                u32t,
            )
        })
        .collect();
    let names = [
        interner.intern("src"),
        interner.intern("dst"),
        interner.intern("lane"),
    ];
    let fn_name = interner.intern("lane_copy_extents");
    let param = |id: u32, ty, mode| MirParam {
        local: MirLocalId(id),
        name: Some(names[id as usize]),
        ty,
        mode,
        span: span(),
    };
    let local_decl = |id: u32, ty, mutable| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable,
        span: span(),
    };
    let function = MirFunction {
        id: MirFunctionId(1),
        source: Some(DefId(1)),
        name: Some(fn_name),
        params: vec![
            param(0, tensor, MirParamMode::Ref),
            param(1, tensor, MirParamMode::MutRef),
            param(2, u32t, MirParamMode::Lane),
        ],
        locals: vec![
            local_decl(0, tensor, false),
            local_decl(1, tensor, true),
            local_decl(2, u32t, false),
        ],
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
    (types, interner, function)
}

/// The signature's resources in dispatcher-slot (binding) order, as
/// `(binding, kind)`.
fn lane_signature_slots(reads: &[(u32, u32)]) -> Vec<(u32, MirKernelResourceKind)> {
    let (types, interner, function) = lane_copy_with_extent_reads(reads);
    let validated = validated(&types, vec![function]);
    let signature =
        crate::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            &validated.program().functions[0],
            validated.validation(),
            &interner,
        )
        .expect("the lane-form shape has an ABI signature");
    let mut slots: Vec<(u32, MirKernelResourceKind)> = signature
        .resources()
        .map(|resource| (resource.binding, resource.kind))
        .collect();
    slots.sort_by_key(|(binding, _)| *binding);
    slots
}

/// Parameter order spec for [`lane_signature_bindings`]: `i` input tensor,
/// `m` `mut` output tensor, `s` f32 scalar. A Lane parameter always follows.
fn lane_signature_bindings(
    order: &str,
) -> Vec<(Option<String>, u32, crate::abi::MirKernelResourceRole)> {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor = rank2_tensor_ty(&mut types, [7, 5]);
    let scalar = f32(&mut types);
    let u32t = u32_ty(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let mut params = Vec::new();
    let mut locals = Vec::new();
    for (index, kind) in order.chars().enumerate() {
        let (ty, mode, mutable) = match kind {
            'i' => (tensor, MirParamMode::Ref, false),
            'm' => (tensor, MirParamMode::MutRef, true),
            's' => (scalar, MirParamMode::Owned, false),
            other => panic!("unknown param kind {other}"),
        };
        let id = index as u32;
        params.push(MirParam {
            local: MirLocalId(id),
            name: Some(interner.intern(&format!("p{index}"))),
            ty,
            mode,
            span: span(),
        });
        locals.push(MirLocal {
            id: MirLocalId(id),
            name: None,
            ty,
            mutable,
            span: span(),
        });
    }
    let lane_id = order.len() as u32;
    params.push(MirParam {
        local: MirLocalId(lane_id),
        name: Some(interner.intern("lane")),
        ty: u32t,
        mode: MirParamMode::Lane,
        span: span(),
    });
    locals.push(MirLocal {
        id: MirLocalId(lane_id),
        name: None,
        ty: u32t,
        mutable: false,
        span: span(),
    });
    let fn_name = interner.intern("lane_order");
    let function = MirFunction {
        id: MirFunctionId(1),
        source: Some(DefId(1)),
        name: Some(fn_name),
        params,
        locals,
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
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
    let validated = validated(&types, vec![function]);
    let signature =
        crate::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            &validated.program().functions[0],
            validated.validation(),
            &interner,
        )
        .expect("the lane-form shape has an ABI signature");
    // Binding order; each entry names its source parameter.
    signature
        .resources_by_binding()
        .map(|resource| {
            (
                resource.source_name.clone(),
                resource.binding,
                resource.role,
            )
        })
        .collect()
}

/// Assert the bindings equal the declared parameter order (`p0`, `p1`, ...)
/// with the given roles, so the binding index IS the parameter index.
fn assert_declared_order(order: &str) {
    use crate::abi::MirKernelResourceRole::{Input, Output};
    let got = lane_signature_bindings(order);
    let want: Vec<_> = order
        .chars()
        .enumerate()
        .map(|(index, kind)| {
            (
                Some(format!("p{index}")),
                index as u32,
                if kind == 'm' { Output } else { Input },
            )
        })
        .collect();
    assert_eq!(got, want, "order `{order}`");
}

#[test]
fn lane_output_may_follow_a_scalar() {
    assert_declared_order("ism");
}

#[test]
fn lane_scalar_may_follow_the_output() {
    assert_declared_order("ims");
}

#[test]
fn lane_two_scalars_interleave_with_tensors() {
    assert_declared_order("sisim");
    assert_declared_order("simss");
    assert_declared_order("sism");
    assert_declared_order("sims");
}

#[test]
fn lane_two_inputs_with_a_scalar_between() {
    assert_declared_order("isim");
}

/// A kernel whose only input is a scalar (`seed(mut w, i32 salt)`) is a
/// count-one input beside the `mut` tensor, not a tensor device view.
#[test]
fn lane_output_with_only_a_scalar_input() {
    assert_declared_order("ms");
    assert_declared_order("sm");
}

#[test]
fn lane_output_may_come_first() {
    assert_declared_order("mi");
    assert_declared_order("mis");
}

#[test]
fn tensor_extent_reads_become_trailing_count_one_extent_resources() {
    // Reads arrive out of order and src axis 0 twice: the resources are
    // ordered by tensor parameter then axis, and deduplicated.
    let (types, interner, function) =
        lane_copy_with_extent_reads(&[(1, 1), (0, 1), (0, 0), (0, 0)]);
    let validated = validated(&types, vec![function]);
    let signature =
        crate::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            &validated.program().functions[0],
            validated.validation(),
            &interner,
        )
        .expect("signature");
    let mut resources: Vec<_> = signature.resources().collect();
    resources.sort_by_key(|resource| resource.binding);
    let kinds: Vec<_> = resources
        .iter()
        .map(|resource| (resource.binding, resource.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (0, MirKernelResourceKind::StorageBuffer),
            (1, MirKernelResourceKind::StorageBuffer),
            (
                2,
                MirKernelResourceKind::TensorAxisExtent {
                    tensor_binding: 0,
                    axis: 0
                }
            ),
            (
                3,
                MirKernelResourceKind::TensorAxisExtent {
                    tensor_binding: 0,
                    axis: 1
                }
            ),
            (
                4,
                MirKernelResourceKind::TensorAxisExtent {
                    tensor_binding: 1,
                    axis: 1
                }
            ),
        ]
    );
    for extent in &resources[2..] {
        assert_eq!(extent.element_count, 1);
        assert_eq!(extent.role, crate::abi::MirKernelResourceRole::Input);
        assert_eq!(extent.access, MirKernelResourceAccess::Read);
        assert_eq!(extent.element_ty, resources[2].element_ty);
    }
    assert_eq!(signature.builtin_inputs.len(), 1);
}

#[test]
fn repeated_tensor_extent_read_is_one_resource() {
    let slots = lane_signature_slots(&[(0, 0), (0, 0)]);
    assert_eq!(
        slots,
        vec![
            (0, MirKernelResourceKind::StorageBuffer),
            (1, MirKernelResourceKind::StorageBuffer),
            (
                2,
                MirKernelResourceKind::TensorAxisExtent {
                    tensor_binding: 0,
                    axis: 0
                }
            ),
        ]
    );
}

#[test]
fn kernel_without_tensor_extent_reads_has_no_extent_resources() {
    let slots = lane_signature_slots(&[]);
    assert_eq!(
        slots,
        vec![
            (0, MirKernelResourceKind::StorageBuffer),
            (1, MirKernelResourceKind::StorageBuffer),
        ]
    );
}

/// A lane-form kernel may declare several `mut` outputs: the first is the
/// output, the rest are `extra_outputs`, and every resource binds at its
/// declared parameter position `(mut a, dom, mut b, lane)` -> 0 / 1 / 2.
#[test]
fn lane_form_with_two_mut_outputs_binds_every_resource_at_its_declared_position() {
    use crate::MirValueKind;
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor = rank1_tensor_ty(&mut types, 8);
    let u32t = u32_ty(&mut types);
    let float = f32(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let lane = MirOperand::Place(MirPlace::local(MirLocalId(3)));
    let index = |base: u32| MirPlace {
        base: crate::MirPlaceBase::Local(MirLocalId(base)),
        projections: vec![crate::MirProjection::Index(lane.clone())],
    };
    let read = MirValueKind::Operand(MirOperand::Place(index(1)));
    let body = vec![
        lane_assign(index(0), 0, read.clone(), float),
        lane_assign(index(2), 1, read, float),
    ];
    let names = ["a", "dom", "b", "lane"].map(|name| interner.intern(name));
    let param = |id: u32, ty, mode| MirParam {
        local: MirLocalId(id),
        name: Some(names[id as usize]),
        ty,
        mode,
        span: span(),
    };
    let local_decl = |id: u32, ty, mutable| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable,
        span: span(),
    };
    let function = MirFunction {
        id: MirFunctionId(1),
        source: Some(DefId(1)),
        name: Some(interner.intern("two_out")),
        params: vec![
            param(0, tensor, MirParamMode::MutRef),
            param(1, tensor, MirParamMode::Ref),
            param(2, tensor, MirParamMode::MutRef),
            param(3, u32t, MirParamMode::Lane),
        ],
        locals: vec![
            local_decl(0, tensor, true),
            local_decl(1, tensor, false),
            local_decl(2, tensor, true),
            local_decl(3, u32t, false),
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: body,
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
    let validated = validated(&types, vec![function]);
    let signature =
        crate::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            &validated.program().functions[0],
            validated.validation(),
            &interner,
        )
        .expect("a lane-form kernel with two `mut` outputs has an ABI signature");
    assert_eq!(signature.output.binding, 0);
    assert_eq!(signature.input.binding, 1);
    assert_eq!(signature.extra_outputs.len(), 1);
    assert_eq!(signature.extra_outputs[0].binding, 2);
    assert_eq!(
        signature.extra_outputs[0].access,
        MirKernelResourceAccess::Write
    );
    assert_eq!(signature.extra_outputs[0].source_local, Some(MirLocalId(2)));
    assert_eq!(signature.builtin_inputs.len(), 1);
    assert_eq!(signature.dispatch_size.x, 8);
}

/// If the exact source cannot be reconstructed, the build error points to the
/// lane kernel and the call site that requested it rather than an earlier
/// kernel in the launch chain.
#[test]
fn missing_lane_domain_error_names_the_kernel_and_call_site() {
    use crate::MirValueKind;
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor = rank1_tensor_ty(&mut types, 8);
    let float = f32(&mut types);
    let u32t = u32_ty(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let lane = MirOperand::Place(MirPlace::local(MirLocalId(2)));
    let output = MirPlace {
        base: crate::MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![crate::MirProjection::Index(lane)],
    };
    let kernel = MirFunction {
        id: MirFunctionId(1),
        source: Some(DefId(1)),
        name: Some(interner.intern("missing_domain_kernel")),
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: Some(interner.intern("unused_domain")),
                ty: tensor,
                mode: MirParamMode::Ref,
                span: span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: Some(interner.intern("out")),
                ty: tensor,
                mode: MirParamMode::MutRef,
                span: span(),
            },
            MirParam {
                local: MirLocalId(2),
                name: Some(interner.intern("lane")),
                ty: u32t,
                mode: MirParamMode::Lane,
                span: span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor,
                mutable: false,
                span: span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: tensor,
                mutable: true,
                span: span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: u32t,
                mutable: false,
                span: span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![lane_assign(
                output,
                0,
                MirValueKind::Operand(MirOperand::Constant(crate::MirConstant::Float(1.0))),
                float,
            )],
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
    let caller = device_function(
        2,
        &mut interner,
        "same_module_caller",
        &[(10, "domain", tensor), (11, "out", tensor)],
        &[10, 11],
        vec![host_call(1, &[10, 11])],
        None,
        Vec::new(),
        vacuum,
        false,
    );
    let validated = validated(&types, vec![kernel, caller]);
    let error = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect_err("an unused domain input cannot be guessed as the lane source");
    let rendered = error.to_string();
    assert!(
        matches!(
            &error,
            super::DeviceProgramBuildError::KernelAbi { function, .. }
                if *function == MirFunctionId(1)
        ),
        "the error belongs to the selected lane kernel: {error:?}"
    );
    assert!(rendered.contains("missing_domain_kernel"), "{rendered}");
    assert!(rendered.contains("same_module_caller"), "{rendered}");
    assert!(rendered.contains("bytes 0..0"), "{rendered}");
    assert!(
        rendered.contains("lane kernel has no carried lane-domain source identity"),
        "{rendered}"
    );
}

/// A single-lane kernel `name(tensor x, mut tensor y)`: both slots carry the
/// declared names `x` and `y`, so two of them collide under the name join.
fn single_lane_xy_kernel(
    id: u32,
    interner: &mut Interner,
    name: &str,
    tensor: MirType,
    float: MirType,
    vacuum: MirType,
) -> MirFunction {
    single_lane_named_kernel(id, interner, name, ["x", "y"], tensor, float, vacuum)
}

/// A single-lane kernel `name(tensor <names[0]>, mut tensor <names[1]>)`.
fn single_lane_named_kernel(
    id: u32,
    interner: &mut Interner,
    name: &str,
    names: [&str; 2],
    tensor: MirType,
    float: MirType,
    vacuum: MirType,
) -> MirFunction {
    let read = crate::MirValueKind::Operand(MirOperand::Place(MirPlace {
        base: crate::MirPlaceBase::Local(MirLocalId(0)),
        projections: vec![crate::MirProjection::Index(MirOperand::Constant(
            crate::MirConstant::Int(3),
        ))],
    }));
    let store = MirPlace {
        base: crate::MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![crate::MirProjection::Index(MirOperand::Constant(
            crate::MirConstant::Int(5),
        ))],
    };
    let names = names.map(|name| interner.intern(name));
    let param = |local: u32, mode| MirParam {
        local: MirLocalId(local),
        name: Some(names[local as usize]),
        ty: tensor,
        mode,
        span: span(),
    };
    let local_decl = |local: u32, mutable| MirLocal {
        id: MirLocalId(local),
        name: None,
        ty: tensor,
        mutable,
        span: span(),
    };
    MirFunction {
        id: MirFunctionId(id),
        source: Some(DefId(id)),
        name: Some(interner.intern(name)),
        params: vec![param(0, MirParamMode::Ref), param(1, MirParamMode::MutRef)],
        locals: vec![local_decl(0, false), local_decl(1, true)],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![lane_assign(store, 0, read, float)],
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
    }
}

/// Two SingleRun kernels that declare the same slot names and shapes own
/// their slots: the cross-kernel name join belongs to training and
/// device-resident programs, so four distinct buffers result.
#[test]
fn single_run_kernels_with_the_same_slot_names_never_share_a_buffer() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor = rank1_tensor_ty(&mut types, 8);
    let float = f32(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let k1 = single_lane_xy_kernel(1, &mut interner, "k1", tensor, float, vacuum);
    let k2 = single_lane_xy_kernel(2, &mut interner, "k2", tensor, float, vacuum);
    let validated = validated(&types, vec![k1, k2]);
    let (program, semantics, _steps) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("both kernels materialize");
    assert_eq!(program.kernels.len(), 2);
    let ids: BTreeSet<BufferId> = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter().map(|resource| resource.buffer.id))
        .collect();
    assert_eq!(ids.len(), 4, "x and y of each kernel are their own buffers");
    program
        .validate_with_semantics(&semantics)
        .expect("the program passes the shared semantic gate");
}

/// A kernel's `y` writer and an unrelated kernel's `y` reader never share a
/// buffer when the host passes DIFFERENT values: buffer identity is the host
/// value, never the parameter name. Four buffers, no dependency edge.
#[test]
fn writer_in_one_kernel_and_unrelated_reader_of_the_same_name_in_another_never_share_a_buffer() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor = rank1_tensor_ty(&mut types, 8);
    let float = f32(&mut types);
    let vacuum = vacuum_ty(&mut types);
    let index = u32_ty(&mut types);
    // k1(x, mut y) writes `y`; k2(y, mut z) reads a slot ALSO named `y`.
    let k1 = single_lane_named_kernel(1, &mut interner, "k1", ["x", "y"], tensor, float, vacuum);
    let k2 = single_lane_named_kernel(2, &mut interner, "k2", ["y", "z"], tensor, float, vacuum);
    let main = device_function(
        3,
        &mut interner,
        "main",
        &[
            (20, "h1", tensor),
            (21, "h2", tensor),
            (22, "h3", tensor),
            (23, "h4", tensor),
            (24, "unused", index),
        ],
        &[],
        vec![host_call(1, &[20, 21]), host_call(2, &[22, 23])],
        None,
        Vec::new(),
        vacuum,
        false,
    );
    let validated = validated(&types, vec![k1, k2, main]);
    let (program, semantics, _steps) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &fixture_metadata(&validated.program().functions),
    )
    .expect("both kernels materialize");
    assert_eq!(program.kernels.len(), 2);
    let ids: BTreeSet<BufferId> = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter().map(|resource| resource.buffer.id))
        .collect();
    assert_eq!(ids.len(), 4, "k1.y and k2.y are different host values");
    assert!(
        semantics.dependencies.is_empty(),
        "no producer→consumer edge: {:?}",
        semantics.dependencies
    );
    program
        .validate_with_semantics(&semantics)
        .expect("the program passes the shared semantic gate");
}

/// Clear every diagnostic name a built program carries (buffer identities of
/// kernel resources and results, semantic value names).
fn blank_names(program: &mut DeviceProgram, semantics: &mut DeviceSemantics) {
    for kernel in &mut program.kernels {
        for resource in &mut kernel.resources {
            resource.buffer.name.clear();
        }
    }
    for result in &mut program.results {
        result.buffer.name.clear();
    }
    for value in &mut semantics.values {
        value.name.clear();
    }
}

/// The acceptance test of the value-identity rule: renaming every kernel
/// parameter changes nothing about the built program except the names in it.
#[test]
fn renaming_every_kernel_parameter_changes_only_names() {
    let (original_program, original_semantics) = build_reversed_chain(&ORIGINAL_CHAIN_NAMES);
    let (renamed_program, renamed_semantics) = build_reversed_chain(&RENAMED_CHAIN_NAMES);

    let names = |program: &DeviceProgram| -> Vec<String> {
        program
            .kernels
            .iter()
            .flat_map(|kernel| kernel.resources.iter().map(|r| r.buffer.name.clone()))
            .collect()
    };
    assert_ne!(
        names(&original_program),
        names(&renamed_program),
        "the rename really changed the names carried by the program"
    );
    assert_ne!(
        original_semantics
            .values
            .iter()
            .map(|value| value.name.clone())
            .collect::<Vec<_>>(),
        renamed_semantics
            .values
            .iter()
            .map(|value| value.name.clone())
            .collect::<Vec<_>>(),
        "the rename really changed the semantic value names"
    );

    let (mut program_a, mut semantics_a) = (original_program, original_semantics);
    let (mut program_b, mut semantics_b) = (renamed_program, renamed_semantics);
    blank_names(&mut program_a, &mut semantics_a);
    blank_names(&mut program_b, &mut semantics_b);
    assert_eq!(
        program_a, program_b,
        "the device program differs by more than names"
    );
    assert_eq!(
        semantics_a, semantics_b,
        "the device semantics differ by more than names"
    );
    // The chain is real in both: the medius buffer has one edge.
    assert_eq!(semantics_a.dependencies.len(), 1);
}
