use super::build_device_program;
use crate::abi::MirKernelResourceAccess;
use crate::device::MirCompanionMap;
use crate::device::{MirDeviceFunctionMetadata, MirDeviceFunctionMetadataMap};
use crate::device_program::{
    BufferLifetime, BufferRole, DeviceGradientLink, DeviceProgramLifetime, DeviceTrainableParam,
    DeviceTrainingFacts, ObservationCadence,
};
use crate::device_semantics::{
    LosslessMirCompanionEntry, MirCompanionSelectedInput, MirCompanionSelectedOutput,
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

fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32_id = f32(types).semantic_id();
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
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
    let mut locals = named_locals
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
    let _ = &mut locals;
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
                },
            )
        })
        .collect()
}

fn validated(types: &TypeTable, functions: Vec<MirFunction>) -> ValidatedMir<'_> {
    ValidatedMir::new(MirProgram { functions }, MirValidationContext::new(types))
        .expect("the constructor fixture validates")
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
    assert_eq!(device_program.kernels[0].entry, "scale_kernel");
    assert_eq!(device_program.launches.len(), 1);
    assert_eq!(device_program.results.len(), 1);
    assert_eq!(semantics.values.len(), 2);
    assert_eq!(semantics.bindings.len(), 2);
    assert!(semantics.dependencies.is_empty());
    device_program
        .validate_with_semantics(&semantics)
        .expect("constructor output passes the shared semantic gate");
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
        .find(|kernel| kernel.entry == "mixio__1")
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

// ── DPSC-U4a gap 3: dependency-ordered launches (oracle: faber
//    device_test.rs `launches_follow_carried_dependencies_not_declaration_
//    order`) ─────────────────────────────────────────────────────────────

/// The launch sequence follows the carried producer/consumer dependency
/// graph, never kernel declaration order: a chain DECLARED backwards
/// (consumer first) still executes producer → consumer.
#[test]
fn constructor_launch_sequence_follows_dependencies_not_declaration_order() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let r1024 = rank1_tensor_ty(&mut types, 1024);
    let r4 = rank1_tensor_ty(&mut types, 4);
    let r1 = rank1_tensor_ty(&mut types, 1);
    let scalar = f32(&mut types);
    let index = u32_ty(&mut types);
    let vacuum = vacuum_ty(&mut types);
    // Declared first (the faber `u4-reversed-chain` oracle shape):
    // recollige(medius, result, id) — reduces medius and stores into result.
    let consumer = device_function(
        1,
        &mut interner,
        "recollige",
        &[
            (0, "medius", r4),
            (1, "result", r1),
            (2, "id", index),
            (3, "total", scalar),
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
    // the medius count) and stores into medius (the reverse-declared
    // producer→consumer chain).
    let producer = device_function(
        2,
        &mut interner,
        "collige",
        &[
            (0, "a", r1024),
            (1, "medius", r4),
            (2, "id", index),
            (3, "total", scalar),
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
    let validated = validated(&types, vec![consumer, producer]);

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

    // recollige is DECLARED first (kernel 0, launch id 1) but DEPENDS on
    // collige's write of medius: the execution sequence runs collige's
    // launch (id 2, kernel 1) before recollige's (id 1, kernel 0).
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
fn companion_function(types: &mut TypeTable, interner: &mut Interner) -> MirFunction {
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
        false,
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

fn training_fixture<R>(run: impl FnOnce(&Interner, &ValidatedMir<'_>, &MirCompanionMap) -> R) -> R {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let tensor_id = rank1_tensor_ty(&mut types, 2).semantic_id();
    let forward = forward_function(&mut types, &mut interner);
    let step = train_step_function(&mut types, &mut interner);
    let companion = companion_function(&mut types, &mut interner);
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

    training_fixture(|interner, validated, companions| {
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
            .find(|kernel| kernel.entry == "train_step")
            .expect("the train_step kernel exists");

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
            .find(|kernel| kernel.entry == "loss")
            .expect("the forward kernel exists");
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
            .find(|kernel| kernel.entry == "loss_backward")
            .expect("the companion kernel exists");
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

    training_fixture(|interner, validated, companions| {
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
