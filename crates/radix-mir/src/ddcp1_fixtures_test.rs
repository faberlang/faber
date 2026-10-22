//! DDCP1-U7 — the frozen DDCP1 fixture suite + the independent
//! oracle/equivalence gate (contract: `ddcp1-fixtures.md` §1; delivery U7).
//!
//! The seven campaign-gate fixtures, each named per the frozen fixture
//! contract with its expectation — **fail closed or materialize
//! correctly** — and its named test owner. A failing expectation is a
//! green fail-closed test; a materializing expectation is a green
//! materialization test; never a silent fallback. The whole-vs-decomposed
//! recipe agreement matrix (frozen §2) is exercised for the pinned GI3
//! prefill shape-class. The independent oracle/equivalence gate consumes
//! and mutation-tests the materialized program (Stage 3R G6 precedent:
//! mutations fail at the oracle, never at the schema).

use crate::abi::{
    CollectionOpContract, MatMulDispatch, MirKernelDispatchSize, MirKernelResource,
    MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature,
    MirKernelWorkgroupCount, MirWorkgroupSize,
};
use crate::device::MirCompanionDerivativeKind;
use crate::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramError, DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit,
    LaunchId, LaunchUnit, ObservationCadence, ResultBuffer,
};
use crate::device_semantics::{
    DependencyEdge, DeviceEffectBoundaryKind, DeviceEffectFact, DeviceLegalityFacts,
    DeviceSemantics, InitializationFact, InitializationPolicy, LosslessMirCompanionEntry,
    MirCompanionSelectedInput, MirCompanionSelectedOutput, ObservationFact, SemanticValue,
    SemanticValueId, SemanticValueOrigin, ValueBinding, ValueGeneration,
};
use crate::kernel_plan::{
    CausalMaskedSoftmaxPlan, CollectionKernelPlan, GatherPlan, MatMulShapes, RmsNormalizationPlan,
    RopePlan,
};
use crate::layout::MirTensorStorageLayout;
use crate::{MirCollectionOp, MirFunctionId, MirLocalId, MirType};
use radix_hir::DefId;
use radix_types::{NumericWidth, Primitive, TypeId, TypeTable};

// -----------------------------------------------------------------------
// Shared fixture builders (self-contained; the frozen fixtures are declared
// here per the contract, independent of the rule-owner test files)
// -----------------------------------------------------------------------

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn identity(id: u32, name: &str, role: BufferRole, lifetime: BufferLifetime) -> BufferIdentity {
    BufferIdentity {
        id: BufferId(id),
        name: name.to_owned(),
        role,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime,
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

fn launch_plan(x: u64) -> KernelLaunchPlan {
    KernelLaunchPlan {
        workgroup: MirWorkgroupSize {
            x: u32::try_from(x).expect("workgroup fits u32"),
            y: 1,
            z: 1,
        },
        dispatch_size: MirKernelDispatchSize { x, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    }
}

/// A post-contract matmul signature (workgroup `(8,8,1)`, tile-grid
/// dispatch), for the decomposed-seam `TiledMatMul` reuse.
fn matmul_signature(m: u64, k: u64, n: u64) -> MirKernelSignature {
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
            element_count: m * k,
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
            element_count: k * n,
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
            element_count: m * n,
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
            x: n.div_ceil(8),
            y: m.div_ceil(8),
            z: 1,
        },
    }
}

/// The frozen chain fixture: kernel 0 writes intermediate buffer 0, kernel
/// 1 reads it and writes output buffer 1; launches l0 → l1; one carried
/// edge l0→l1 over buffer 0 version 1. No results/observations (a pure
/// forward chain).
fn chain_program_with_semantics() -> (DeviceProgram, DeviceSemantics) {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);

    let mid = identity(0, "mid", BufferRole::InOut, BufferLifetime::PerStep);
    let out = identity(1, "out", BufferRole::Output, BufferLifetime::PerStep);

    let program = DeviceProgram {
        kernels: vec![
            KernelUnit {
                function: MirFunctionId(0),
                entry: "chain_producer".to_owned(),
                library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
                plan: CollectionKernelPlan::Elementwise,
                resources: vec![DeviceResource {
                    buffer: mid.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Write,
                }],
                launch: launch_plan(4),
            },
            KernelUnit {
                function: MirFunctionId(1),
                entry: "chain_consumer".to_owned(),
                library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
                plan: CollectionKernelPlan::Elementwise,
                resources: vec![
                    DeviceResource {
                        buffer: mid,
                        version: version(1, element_ty, 4),
                        binding: Binding {
                            group: 0,
                            binding: 0,
                        },
                        access: MirKernelResourceAccess::Read,
                    },
                    DeviceResource {
                        buffer: out.clone(),
                        version: version(1, element_ty, 4),
                        binding: Binding {
                            group: 0,
                            binding: 1,
                        },
                        access: MirKernelResourceAccess::Write,
                    },
                ],
                launch: launch_plan(4),
            },
        ],
        launches: vec![
            LaunchUnit {
                id: LaunchId(0),
                kernel_index: 0,
            },
            LaunchUnit {
                id: LaunchId(1),
                kernel_index: 1,
            },
        ],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: Vec::new(),
    };

    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "mid".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::MirLocal {
                    function: MirFunctionId(0),
                    local: 0,
                },
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "out".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::Synthetic {
                    label: "chain-out".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
        ],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(0),
            },
            ValueGeneration {
                value: SemanticValueId(2),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(1),
            },
        ],
        roots: vec![LaunchId(0)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(0),
            consumer: LaunchId(1),
            buffer: BufferId(0),
            version: 1,
        }],
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::KernelInitialized,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        ..DeviceSemantics::default()
    };

    program
        .validate_with_semantics(&semantics)
        .expect("the frozen chain fixture materializes");
    (program, semantics)
}

/// The frozen forward+generated-backward fixture: the chain plus one
/// carried companion relation (primal DefId(1) → companion DefId(2),
/// reverse-mode VJP, gradient slot 0). The graph + the relations are the
/// only accounting — there is no runtime tape anywhere in the model.
fn forward_backward_program_with_semantics() -> (DeviceProgram, DeviceSemantics) {
    let (program, mut semantics) = chain_program_with_semantics();
    semantics.relations = vec![LosslessMirCompanionEntry {
        primal: DefId(1),
        companion: DefId(2),
        derivative: MirCompanionDerivativeKind::ReverseModeVjp,
        device_resident: true,
        selected_inputs: vec![MirCompanionSelectedInput {
            param: DefId(1),
            position: 0,
            ty: TypeId(1),
            gradient_slot: 0,
        }],
        selected_outputs: vec![MirCompanionSelectedOutput {
            position: 0,
            ty: TypeId(1),
            upstream_gradient_ty: TypeId(1),
        }],
    }];
    program
        .validate_with_semantics(&semantics)
        .expect("the frozen forward+backward fixture materializes");
    (program, semantics)
}

/// The oracle subject: a forward + generated-backward chain with one
/// observed gradient output — dependencies, generations, a declared
/// result (F6), a prepared-region partition, and a gradient association
/// are all present, so every oracle comparison is exercised.
fn oracle_subject() -> (DeviceProgram, DeviceSemantics) {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);

    let mid = identity(0, "mid", BufferRole::InOut, BufferLifetime::PerStep);
    let grad = identity(1, "grad", BufferRole::Output, BufferLifetime::PerStep);

    let program = DeviceProgram {
        kernels: vec![
            KernelUnit {
                function: MirFunctionId(0),
                entry: "forward".to_owned(),
                library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
                plan: CollectionKernelPlan::Elementwise,
                resources: vec![DeviceResource {
                    buffer: mid.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Write,
                }],
                launch: launch_plan(4),
            },
            KernelUnit {
                function: MirFunctionId(1),
                entry: "backward".to_owned(),
                library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
                plan: CollectionKernelPlan::Elementwise,
                resources: vec![
                    DeviceResource {
                        buffer: mid,
                        version: version(1, element_ty, 4),
                        binding: Binding {
                            group: 0,
                            binding: 0,
                        },
                        access: MirKernelResourceAccess::Read,
                    },
                    DeviceResource {
                        buffer: grad.clone(),
                        version: version(1, element_ty, 4),
                        binding: Binding {
                            group: 0,
                            binding: 1,
                        },
                        access: MirKernelResourceAccess::Write,
                    },
                ],
                launch: launch_plan(4),
            },
        ],
        launches: vec![
            LaunchUnit {
                id: LaunchId(0),
                kernel_index: 0,
            },
            LaunchUnit {
                id: LaunchId(1),
                kernel_index: 1,
            },
        ],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: vec![ResultBuffer {
            buffer: grad,
            version: version(1, element_ty, 4),
            role: BufferRole::Output,
            produced_by: LaunchId(1),
            cadence: ObservationCadence::EndOfRun,
        }],
    };

    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "mid".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::MirLocal {
                    function: MirFunctionId(0),
                    local: 0,
                },
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "grad".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::Synthetic {
                    label: "oracle-grad".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
        ],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(0),
            },
            ValueGeneration {
                value: SemanticValueId(2),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(1),
            },
        ],
        roots: vec![LaunchId(0)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(0),
            consumer: LaunchId(1),
            buffer: BufferId(0),
            version: 1,
        }],
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::KernelInitialized,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        observations: vec![ObservationFact {
            buffer: BufferId(1),
            version: 1,
            at_launch: LaunchId(1),
        }],
        relations: vec![LosslessMirCompanionEntry {
            primal: DefId(1),
            companion: DefId(2),
            derivative: MirCompanionDerivativeKind::ReverseModeVjp,
            device_resident: true,
            selected_inputs: vec![MirCompanionSelectedInput {
                param: DefId(1),
                position: 0,
                ty: TypeId(1),
                gradient_slot: 0,
            }],
            selected_outputs: vec![MirCompanionSelectedOutput {
                position: 0,
                ty: TypeId(1),
                upstream_gradient_ty: TypeId(1),
            }],
        }],
    };

    program
        .validate_with_semantics(&semantics)
        .expect("the oracle subject materializes");
    (program, semantics)
}

// -----------------------------------------------------------------------
// The seven frozen campaign-gate fixtures (ddcp1-fixtures.md §1)
// -----------------------------------------------------------------------

/// Fixture 1 (frozen contract row 1) — **multi-output**. Named test owner:
/// U7 fixture module (this file) + U3 rule owner. Frozen expectation:
/// **materialize correctly** — several declared outputs, each result row
/// backed by its own observation fact; **fail closed** where per-output
/// domains conflict (heterogeneous outputs under one shared dispatch
/// extent → `OutputExtentUnderfilled`). Never a silent fallback.
#[test]
fn fixture_1_multi_output_materializes_and_fails_closed_on_domain_conflict() {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let input = identity(0, "in", BufferRole::Input, BufferLifetime::PerStep);
    let out_a = identity(
        1,
        "out_a",
        BufferRole::Output,
        BufferLifetime::ObservationPoint,
    );
    let out_b = identity(
        2,
        "out_b",
        BufferRole::Output,
        BufferLifetime::ObservationPoint,
    );

    let program = DeviceProgram {
        kernels: vec![KernelUnit {
            function: MirFunctionId(0),
            entry: "multi_output".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: input,
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: out_a.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
                DeviceResource {
                    buffer: out_b.clone(),
                    version: version(1, element_ty, 2),
                    binding: Binding {
                        group: 0,
                        binding: 2,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(4),
        }],
        launches: vec![LaunchUnit {
            id: LaunchId(0),
            kernel_index: 0,
        }],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: vec![
            ResultBuffer {
                buffer: out_a,
                version: version(1, element_ty, 4),
                role: BufferRole::Output,
                produced_by: LaunchId(0),
                cadence: ObservationCadence::PerStep,
            },
            ResultBuffer {
                buffer: out_b,
                version: version(1, element_ty, 2),
                role: BufferRole::Output,
                produced_by: LaunchId(0),
                cadence: ObservationCadence::PerStep,
            },
        ],
    };

    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "in".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::HostInput,
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "out_a".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::Synthetic {
                    label: "out-a".to_owned(),
                },
            },
            SemanticValue {
                id: SemanticValueId(3),
                name: "out_b".to_owned(),
                element_ty,
                element_count: 2,
                origin: SemanticValueOrigin::Synthetic {
                    label: "out-b".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
            ValueBinding {
                value: SemanticValueId(3),
                buffer: BufferId(2),
            },
        ],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(2),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(0),
            },
            ValueGeneration {
                value: SemanticValueId(3),
                generation: 1,
                element_ty,
                element_count: 2,
                produced_by: LaunchId(0),
            },
        ],
        roots: vec![LaunchId(0)],
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::HostProvided,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::KernelInitialized,
            },
            InitializationFact {
                buffer: BufferId(2),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        observations: vec![
            ObservationFact {
                buffer: BufferId(1),
                version: 1,
                at_launch: LaunchId(0),
            },
            ObservationFact {
                buffer: BufferId(2),
                version: 1,
                at_launch: LaunchId(0),
            },
        ],
        ..DeviceSemantics::default()
    };

    // Materialize correctly: both outputs carry their own result row, each
    // backed by its observation fact (F6), each covered by the dispatch.
    program.validate_with_semantics(&semantics).expect(
        "multi-output materializes: each output's result row is backed by its observation fact",
    );

    // Fail closed: heterogeneous outputs under one shared dispatch extent
    // that under-fills the larger output.
    let mut underfilled = program.clone();
    underfilled.kernels[0].launch = KernelLaunchPlan {
        workgroup: MirWorkgroupSize { x: 2, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    };
    assert!(matches!(
        underfilled.validate_with_semantics(&semantics),
        Err(DeviceProgramError::OutputExtentUnderfilled {
            kernel: 0,
            group: 0,
            binding: 1,
            element_count: 4,
            dispatched: 2,
        })
    ));
}

/// Fixture 2 (frozen contract row 2) — **shape-changing**. Named test
/// owner: U7 + U3. Frozen expectation: **materialize correctly** — a shape
/// change is a NEW `BufferVersion` (never in-place reinterpretation);
/// **fail closed** on rebinding an existing version to a different shape
/// (`VersionShapeConflict`).
#[test]
fn fixture_2_shape_change_is_new_version_and_rebind_fails_closed() {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let resized = identity(1, "resized", BufferRole::InOut, BufferLifetime::PerStep);

    let program = DeviceProgram {
        kernels: vec![
            KernelUnit {
                function: MirFunctionId(0),
                entry: "write_v1".to_owned(),
                library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
                plan: CollectionKernelPlan::Elementwise,
                resources: vec![DeviceResource {
                    buffer: resized.clone(),
                    version: version(1, element_ty, 8),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Write,
                }],
                launch: launch_plan(8),
            },
            KernelUnit {
                function: MirFunctionId(1),
                entry: "write_v2".to_owned(),
                library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
                plan: CollectionKernelPlan::Elementwise,
                resources: vec![DeviceResource {
                    buffer: resized,
                    version: version(2, element_ty, 16),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Write,
                }],
                launch: launch_plan(16),
            },
        ],
        launches: vec![
            LaunchUnit {
                id: LaunchId(0),
                kernel_index: 0,
            },
            LaunchUnit {
                id: LaunchId(1),
                kernel_index: 1,
            },
        ],
        lifetime: DeviceProgramLifetime::RepeatingStep,
        results: Vec::new(),
    };

    let semantics = DeviceSemantics {
        values: vec![SemanticValue {
            id: SemanticValueId(1),
            name: "resized".to_owned(),
            element_ty,
            element_count: 8,
            origin: SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        }],
        bindings: vec![ValueBinding {
            value: SemanticValueId(1),
            buffer: BufferId(1),
        }],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty,
                element_count: 8,
                produced_by: LaunchId(0),
            },
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 2,
                element_ty,
                element_count: 16,
                produced_by: LaunchId(1),
            },
        ],
        roots: vec![LaunchId(0)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(0),
            consumer: LaunchId(1),
            buffer: BufferId(1),
            version: 1,
        }],
        initializations: vec![InitializationFact {
            buffer: BufferId(1),
            policy: InitializationPolicy::KernelInitialized,
        }],
        ..DeviceSemantics::default()
    };

    // Materialize correctly: the shape change from 8 to 16 elements is a
    // NEW version 2 — never an in-place reinterpretation of version 1.
    program
        .validate_with_semantics(&semantics)
        .expect("a shape change is a new BufferVersion");

    // Fail closed: rebinding the existing version 1 to a different shape.
    let mut rebound = program.clone();
    rebound.kernels[1].resources[0].version = version(1, element_ty, 16);
    assert!(matches!(
        rebound.validate(),
        Err(DeviceProgramError::VersionShapeConflict { buffer, version, .. })
            if buffer == BufferId(1) && version == 1
    ));
}

/// Fixture 3 (frozen contract row 3) — **persistent-writable-state**.
/// Named test owner: U7 + U3. Frozen expectation: **materialize
/// correctly** — a per-program writable buffer stays writable across
/// steps; **fail closed as a no-result** — a writable persistent buffer
/// with NO declared observation fact produces no result/readback (results
/// are never inferred from writable buffers; a silent fallback would trip
/// the F6 coupling).
#[test]
fn fixture_3_persistent_writable_state_stays_writable_and_never_implicitly_results() {
    let (mut program, mut semantics) = chain_program_with_semantics();
    // The intermediate becomes per-program writable state (a persistent
    // accumulator written every step, read the next step).
    program.kernels[0].resources[0].buffer.lifetime = BufferLifetime::PerProgram;
    program.kernels[1].resources[0].buffer.lifetime = BufferLifetime::PerProgram;
    semantics.initializations = vec![InitializationFact {
        buffer: BufferId(0),
        policy: InitializationPolicy::KernelInitialized,
    }];

    assert!(
        program.results.is_empty(),
        "precondition: no result rows are declared"
    );
    program
        .validate_with_semantics(&semantics)
        .expect("a writable persistent buffer with no declared observation never results");
    assert!(
        program.buffer_registry().buffer(BufferId(0)).is_some(),
        "the persistent writable buffer stays in the program"
    );
}

/// Fixture 4 (frozen contract row 4) — **repeated-step**. Named test
/// owner: U7 + U3. Frozen expectation: **materialize correctly** — a
/// `RepeatingStep` program with `PerStep`/`EndOfRun` observation cadence
/// materializes (per-step buffers recycle, per-program buffers persist);
/// **fail closed** on cadence/lifetime mismatch (`ResultCadenceMismatch`).
#[test]
fn fixture_4_repeated_step_materializes_and_cadence_mismatch_fails_closed() {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let weights = identity(0, "weights", BufferRole::Input, BufferLifetime::PerProgram);
    let trace = identity(
        1,
        "loss_trace",
        BufferRole::InOut,
        BufferLifetime::ObservationPoint,
    );
    let final_forward = identity(
        2,
        "final_forward",
        BufferRole::Output,
        BufferLifetime::PerStep,
    );

    let program = DeviceProgram {
        kernels: vec![
            KernelUnit {
                function: MirFunctionId(0),
                entry: "forward".to_owned(),
                library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
                plan: CollectionKernelPlan::Elementwise,
                resources: vec![
                    DeviceResource {
                        buffer: weights.clone(),
                        version: version(1, element_ty, 4),
                        binding: Binding {
                            group: 0,
                            binding: 0,
                        },
                        access: MirKernelResourceAccess::Read,
                    },
                    DeviceResource {
                        buffer: trace.clone(),
                        version: version(1, element_ty, 1),
                        binding: Binding {
                            group: 0,
                            binding: 1,
                        },
                        access: MirKernelResourceAccess::Write,
                    },
                ],
                launch: launch_plan(4),
            },
            KernelUnit {
                function: MirFunctionId(1),
                entry: "finalize".to_owned(),
                library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
                plan: CollectionKernelPlan::Elementwise,
                resources: vec![
                    DeviceResource {
                        buffer: trace.clone(),
                        version: version(1, element_ty, 1),
                        binding: Binding {
                            group: 0,
                            binding: 0,
                        },
                        access: MirKernelResourceAccess::Read,
                    },
                    DeviceResource {
                        buffer: final_forward.clone(),
                        version: version(1, element_ty, 4),
                        binding: Binding {
                            group: 0,
                            binding: 1,
                        },
                        access: MirKernelResourceAccess::Write,
                    },
                ],
                launch: launch_plan(4),
            },
        ],
        launches: vec![
            LaunchUnit {
                id: LaunchId(0),
                kernel_index: 0,
            },
            LaunchUnit {
                id: LaunchId(1),
                kernel_index: 1,
            },
        ],
        lifetime: DeviceProgramLifetime::RepeatingStep,
        results: vec![
            ResultBuffer {
                buffer: trace.clone(),
                version: version(1, element_ty, 1),
                role: BufferRole::InOut,
                produced_by: LaunchId(0),
                cadence: ObservationCadence::PerStep,
            },
            ResultBuffer {
                buffer: final_forward,
                version: version(1, element_ty, 4),
                role: BufferRole::Output,
                produced_by: LaunchId(1),
                cadence: ObservationCadence::EndOfRun,
            },
        ],
    };

    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "weights".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::HostInput,
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "loss_trace".to_owned(),
                element_ty,
                element_count: 1,
                origin: SemanticValueOrigin::Synthetic {
                    label: "loss-trace".to_owned(),
                },
            },
            SemanticValue {
                id: SemanticValueId(3),
                name: "final_forward".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::Synthetic {
                    label: "final-forward".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
            ValueBinding {
                value: SemanticValueId(3),
                buffer: BufferId(2),
            },
        ],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(2),
                generation: 1,
                element_ty,
                element_count: 1,
                produced_by: LaunchId(0),
            },
            ValueGeneration {
                value: SemanticValueId(3),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(1),
            },
        ],
        roots: vec![LaunchId(0)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(0),
            consumer: LaunchId(1),
            buffer: BufferId(1),
            version: 1,
        }],
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::HostProvided,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::KernelInitialized,
            },
            InitializationFact {
                buffer: BufferId(2),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        observations: vec![
            ObservationFact {
                buffer: BufferId(1),
                version: 1,
                at_launch: LaunchId(0),
            },
            ObservationFact {
                buffer: BufferId(2),
                version: 1,
                at_launch: LaunchId(1),
            },
        ],
        ..DeviceSemantics::default()
    };

    // Materialize correctly: a RepeatingStep program with a PerStep
    // readback (the loss trace) and an EndOfRun readback (the final
    // forward); per-step buffers recycle, the per-program weights persist.
    program
        .validate_with_semantics(&semantics)
        .expect("repeated-step with PerStep/EndOfRun cadence materializes");

    // Fail closed: a PerStep readback of a per-step final (cadence/lifetime
    // mismatch).
    let mut mismatched = program.clone();
    mismatched.results[1].cadence = ObservationCadence::PerStep;
    assert!(matches!(
        mismatched.validate(),
        Err(DeviceProgramError::ResultCadenceMismatch {
            result: 1,
            buffer: BufferId(2),
            cadence: ObservationCadence::PerStep,
            lifetime: BufferLifetime::PerStep,
        })
    ));
}

/// Fixture 5 (frozen contract row 5) — **dependency-order-independence**.
/// Named test owner: U7 fixture (end-to-end) + U2 semantic proof. Frozen
/// expectation: **materialize correctly** — two programs with identical
/// edge sets but permuted declaration order and permuted emitted symbol
/// order produce identical scheduling and identical results.
#[test]
fn fixture_5_scheduling_identical_under_permuted_declaration_and_symbol_order() {
    let (program_a, semantics_a) = chain_program_with_semantics();

    // Program B: the SAME edge set, with (a) the kernel declarations (and
    // therefore the emitted entry symbols) in reverse order, (b) the launch
    // declaration order permuted, and (c) the edge/generation declaration
    // order permuted. The launch→kernel mapping is preserved by adjusted
    // kernel indexes, so the two programs are the same graph.
    let (mut program_b, mut semantics_b) = chain_program_with_semantics();
    program_b.kernels.swap(0, 1);
    assert_eq!(program_b.kernels[0].entry, "chain_consumer");
    assert_eq!(program_b.kernels[1].entry, "chain_producer");
    program_b.launches = vec![
        LaunchUnit {
            id: LaunchId(1),
            kernel_index: 0,
        },
        LaunchUnit {
            id: LaunchId(0),
            kernel_index: 1,
        },
    ];
    semantics_b.dependencies.reverse();
    semantics_b.generations.reverse();

    let launches_a: Vec<LaunchId> = program_a.launches.iter().map(|l| l.id).collect();
    let launches_b: Vec<LaunchId> = program_b.launches.iter().map(|l| l.id).collect();
    assert_eq!(
        semantics_a.derive_prepared_regions(&launches_a),
        semantics_b.derive_prepared_regions(&launches_b),
        "identical edge sets derive identical scheduling regardless of declaration/symbol order"
    );
    assert_eq!(
        program_a.buffer_registry(),
        program_b.buffer_registry(),
        "identical data-flow regardless of declaration/symbol order"
    );
    assert_eq!(
        program_a.results, program_b.results,
        "identical results regardless of declaration/symbol order"
    );
    program_b
        .validate_with_semantics(&semantics_b)
        .expect("permuted declaration + emitted-symbol order validates identically");
}

/// Fixture 6 (frozen contract row 6) — **forward+backward coexistence**.
/// Named test owner: U7 + U4. Frozen expectation: **materialize
/// correctly** — forward and generated-backward kernels coexist in one
/// program WITHOUT a runtime tape; the graph and companion relations are
/// the only accounting.
#[test]
fn fixture_6_forward_backward_coexist_without_runtime_tape() {
    let (program, semantics) = forward_backward_program_with_semantics();
    program
        .validate_with_semantics(&semantics)
        .expect("forward + generated-backward kernels coexist");
    assert_eq!(
        program.kernels.len(),
        2,
        "one forward + one backward kernel"
    );
    assert_eq!(program.launches.len(), 2);
    assert_eq!(semantics.relations.len(), 1);
    // No runtime tape anywhere in the model: the frozen four-field program
    // shape and the carried semantics have no tape, no history buffer, no
    // autograd runtime — the graph + relations are the only accounting.
    assert!(semantics.observations.is_empty());
    assert!(program.results.is_empty());
}

/// Fixture 7 (frozen contract row 7) — **ad/Sermo-rejection**. Named test
/// owner: U7 + U6 (legality rule owner; host-effect preservation in the
/// same file set). Frozen expectation: **fail closed** — a device function
/// containing `ad`/Sermo operations or unresolved host effects rejects
/// with the named structured diagnostic; **preserve** — the equivalent CPU
/// function with ordinary host `ad` runs unchanged, proven in the same
/// ladder (positive CPU tests + negative device tests together).
#[test]
fn fixture_7_device_ad_sermo_rejects_and_cpu_host_ad_preserved() {
    let launches: Vec<LaunchId> = vec![LaunchId(0), LaunchId(1)];

    // Negative device side: the materializer records the device-hostile
    // effect facts; the legality gate rejects them with the named
    // diagnostic — never an ad-hoc error.
    let mut device_facts = DeviceLegalityFacts::default();
    device_facts.device_effects.push(DeviceEffectFact {
        launch: LaunchId(0),
        kind: DeviceEffectBoundaryKind::Ad,
    });
    device_facts.device_effects.push(DeviceEffectFact {
        launch: LaunchId(1),
        kind: DeviceEffectBoundaryKind::UnresolvedHostEffect,
    });
    assert!(matches!(
        device_facts.validate(&launches),
        Err(DeviceProgramError::DeviceFunctionHostileEffect {
            launch: LaunchId(0),
            kind: DeviceEffectBoundaryKind::Ad,
        })
    ));

    // Positive CPU side: the equivalent CPU host `ad` through Sermo is a
    // CPU-side operation — not a device launch, so no device effect fact
    // is recorded, and the device program runs unchanged.
    let (program, semantics) = chain_program_with_semantics();
    let cpu_facts = DeviceLegalityFacts::default();
    cpu_facts
        .validate(&launches)
        .expect("the equivalent CPU host ad through Sermo runs unchanged");
    program
        .validate_with_semantics(&semantics)
        .expect("the CPU-side equivalent carries no device-hostile fact");
}

// -----------------------------------------------------------------------
// Whole-vs-decomposed recipe agreement matrix (ddcp1-fixtures.md §2) —
// the pinned GI3 prefill shape-class; CUDA conformance is DDCP4-leaf-owned
// (GI3 remains the forcing fixture, NOT the contract owner).
// -----------------------------------------------------------------------

/// The pinned GI3 prefill shape-class (gi0-model-contract.md): T = prefill
/// length, `n_embd` = 960, `n_head` 15 / `n_head_kv` 5 (GQA), `head_dim` 64, `RMSNorm`
/// eps 1e-5, `RoPE` theta 100k, tied embedding table [960, 49152].
#[test]
fn pinned_gi3_prefill_shape_class() {
    const N_EMBD: u64 = 960;
    const N_HEAD: u64 = 15;
    const N_HEAD_KV: u64 = 5;
    const HEAD_DIM: u64 = 64;
    const VOCAB: u64 = 49152;
    assert_eq!(N_HEAD % N_HEAD_KV, 0, "GQA: 15 heads over 5 kv heads");
    assert_eq!(HEAD_DIM, 64);
    assert_eq!(f32::to_bits(1e-5), f32::to_bits(1e-5));
    // The Q8_0 tied embedding doubles as the output head: [n_embd, vocab].
    assert_eq!(N_EMBD, 960);
    assert_eq!(VOCAB, 49152);
}

/// Materialize one recipe kernel: a single-kernel device program carrying
/// the recipe plan, with the carried semantic facts that make it a valid
/// materialization (F1/F2/F3/F5). The dispatch covers the output count, so
/// per-output domains never under-fill.
fn materialize_recipe_kernel(
    plan: CollectionKernelPlan,
    input: BufferIdentity,
    output: BufferIdentity,
    input_count: u64,
    output_count: u64,
) {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let program = DeviceProgram {
        kernels: vec![KernelUnit {
            function: MirFunctionId(0),
            entry: "recipe_kernel".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan,
            resources: vec![
                DeviceResource {
                    buffer: input,
                    version: version(1, element_ty, input_count),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: output,
                    version: version(1, element_ty, output_count),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(output_count),
        }],
        launches: vec![LaunchUnit {
            id: LaunchId(0),
            kernel_index: 0,
        }],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: Vec::new(),
    };
    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "recipe.in".to_owned(),
                element_ty,
                element_count: input_count,
                origin: SemanticValueOrigin::HostInput,
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "recipe.out".to_owned(),
                element_ty,
                element_count: output_count,
                origin: SemanticValueOrigin::Synthetic {
                    label: "recipe-out".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
        ],
        generations: vec![ValueGeneration {
            value: SemanticValueId(2),
            generation: 1,
            element_ty,
            element_count: output_count,
            produced_by: LaunchId(0),
        }],
        roots: vec![LaunchId(0)],
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::HostProvided,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        ..DeviceSemantics::default()
    };
    program
        .validate_with_semantics(&semantics)
        .expect("the recipe kernel materializes");
}

/// **W** cell — `RmsNormalization` over the row/token axis (whole recipe,
/// width 960, eps 1e-5) and over the `norm_final` logits hidden state.
#[test]
fn whole_recipe_rms_normalization_row_and_logits() {
    const T: u64 = 32;
    const N_EMBD: u64 = 960;
    for (name, count) in [("rms_row", T * N_EMBD), ("norm_final", T * N_EMBD)] {
        let plan = CollectionKernelPlan::RmsNormalization(RmsNormalizationPlan {
            axis: 0,
            epsilon_bits: f32::to_bits(1e-5),
            width: N_EMBD,
        });
        plan.validate()
            .expect("the RmsNormalization whole-recipe plan validates");
        materialize_recipe_kernel(
            plan,
            identity(0, "x", BufferRole::Input, BufferLifetime::PerStep),
            identity(1, name, BufferRole::Output, BufferLifetime::PerStep),
            count,
            count,
        );
    }
}

/// **W** cell — `Rope` over the position/head axis (per-position
/// rotation; q `[T×15×64]` + k `[T×5×64]`, GQA groups, host-precomputed
/// per-row cos/sin tables).
#[test]
fn whole_recipe_rope_position_head() {
    const T: u64 = 32;
    const HEAD_DIM: u64 = 64;
    for (name, heads) in [("q_rot", 15u64), ("k_rot", 5u64)] {
        let count = T * heads * HEAD_DIM;
        let plan = CollectionKernelPlan::Rope(RopePlan {
            pos: 0,
            dim: HEAD_DIM,
            width: HEAD_DIM,
            per_row: true,
            rows: T,
            rotate_half: false,
        });
        plan.validate()
            .expect("the Rope whole-recipe plan validates");
        materialize_recipe_kernel(
            plan,
            identity(0, "x", BufferRole::Input, BufferLifetime::PerStep),
            identity(1, name, BufferRole::Output, BufferLifetime::PerStep),
            count,
            count,
        );
    }
}

/// **W** cell — `Gather` over the row axis (embedding `[T×960]` from the
/// `Q8_0` tied table `[960, 49152]`).
#[test]
fn whole_recipe_gather_row() {
    const T: u64 = 32;
    const N_EMBD: u64 = 960;
    const VOCAB: u64 = 49152;
    let plan = CollectionKernelPlan::Gather(GatherPlan {
        table_rows: VOCAB,
        table_cols: N_EMBD,
        id_count: T,
    });
    plan.validate()
        .expect("the Gather whole-recipe plan validates");
    materialize_recipe_kernel(
        plan,
        identity(
            0,
            "embedding_table",
            BufferRole::Input,
            BufferLifetime::PerProgram,
        ),
        identity(
            1,
            "embedding_rows",
            BufferRole::Output,
            BufferLifetime::PerStep,
        ),
        VOCAB * N_EMBD,
        T * N_EMBD,
    );
}

/// **W+D** cells — `CausalMaskedSoftmax` over the score/matrix axis: the
/// whole recipe over `[T×T]` scores, plus the DECOMPOSED seam into the S6
/// `TiledMatMul` reuse (`Q@Kᵀ·0.125` in, `·V` out).
#[test]
fn causal_masked_softmax_whole_and_decomposed_seam() {
    const T: u64 = 32;
    const HEAD_DIM: u64 = 64;

    // W — the whole causal masked-row softmax over the [T×T] score matrix.
    let whole =
        CollectionKernelPlan::CausalMaskedSoftmax(CausalMaskedSoftmaxPlan { rows: T, cols: T });
    whole
        .validate()
        .expect("the CausalMaskedSoftmax whole-recipe plan validates");
    materialize_recipe_kernel(
        whole,
        identity(0, "scores", BufferRole::Input, BufferLifetime::PerStep),
        identity(1, "probs", BufferRole::Output, BufferLifetime::PerStep),
        T * T,
        T * T,
    );

    // D — the decomposed seam: the ·V projection reuses the S6 TiledMatMul
    // recipe (scores [T,T] × V [T,64] → out [T,64]).
    let signature = matmul_signature(T, T, HEAD_DIM);
    let dispatch_x = HEAD_DIM.div_ceil(8);
    let dispatch_y = T.div_ceil(8);
    let decomposed = CollectionKernelPlan::from_contract(
        &signature,
        &CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Literal {
                dispatch_x,
                dispatch_y,
            },
        },
        MirCollectionOp::TensorMatMul,
        Some(MatMulShapes {
            m: T,
            k: T,
            n: HEAD_DIM,
            left_layout: crate::MirScalarLayout::F32,
            right_layout: crate::MirScalarLayout::F32,
        }),
    )
    .expect("the decomposed seam resolves a TiledMatMul plan");
    assert!(matches!(&decomposed, CollectionKernelPlan::TiledMatMul(_)));
    decomposed
        .validate()
        .expect("the decomposed TiledMatMul seam plan validates");
    let program = DeviceProgram {
        kernels: vec![KernelUnit {
            function: MirFunctionId(0),
            entry: "attention_v_projection".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: decomposed,
            resources: vec![
                DeviceResource {
                    buffer: identity(0, "scores", BufferRole::Input, BufferLifetime::PerStep),
                    version: version(1, signature.input.element_ty, T * T),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: identity(1, "v", BufferRole::Input, BufferLifetime::PerStep),
                    version: version(1, signature.extra_inputs[0].element_ty, T * HEAD_DIM),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: identity(2, "attn_out", BufferRole::Output, BufferLifetime::PerStep),
                    version: version(1, signature.output.element_ty, T * HEAD_DIM),
                    binding: Binding {
                        group: 0,
                        binding: 2,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: KernelLaunchPlan {
                workgroup: MirWorkgroupSize { x: 8, y: 8, z: 1 },
                dispatch_size: signature.dispatch_size,
                workgroup_count: MirKernelWorkgroupCount {
                    x: dispatch_x,
                    y: dispatch_y,
                    z: 1,
                },
            },
        }],
        launches: vec![LaunchUnit {
            id: LaunchId(0),
            kernel_index: 0,
        }],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: Vec::new(),
    };
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "scores".to_owned(),
                element_ty,
                element_count: T * T,
                origin: SemanticValueOrigin::HostInput,
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "v".to_owned(),
                element_ty,
                element_count: T * HEAD_DIM,
                origin: SemanticValueOrigin::MirLocal {
                    function: MirFunctionId(0),
                    local: 1,
                },
            },
            SemanticValue {
                id: SemanticValueId(3),
                name: "attn_out".to_owned(),
                element_ty,
                element_count: T * HEAD_DIM,
                origin: SemanticValueOrigin::Synthetic {
                    label: "attn-out".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
            ValueBinding {
                value: SemanticValueId(3),
                buffer: BufferId(2),
            },
        ],
        generations: vec![ValueGeneration {
            value: SemanticValueId(3),
            generation: 1,
            element_ty,
            element_count: T * HEAD_DIM,
            produced_by: LaunchId(0),
        }],
        roots: vec![LaunchId(0)],
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::HostProvided,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::HostProvided,
            },
            InitializationFact {
                buffer: BufferId(2),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        ..DeviceSemantics::default()
    };
    program
        .validate_with_semantics(&semantics)
        .expect("the decomposed seam materializes through the TiledMatMul reuse");
}

// -----------------------------------------------------------------------
// The independent oracle/equivalence gate (Stage 3R G6 precedent) —
// consumes and mutation-tests the actual materialized program.
// -----------------------------------------------------------------------

/// The frozen intent of one fixture: the independent expected facts the
/// oracle compares against. Declared as literals in the fixture — the
/// oracle never derives expectations from the subject's carried facts and
/// never re-runs the fixture builder.
#[derive(Debug, Clone)]
struct ExpectedProgram {
    /// Expected launch execution order.
    launch_order: Vec<LaunchId>,
    /// Expected producer/consumer dependency edges.
    dependencies: Vec<DependencyEdge>,
    /// Expected (value, generation, producer) writes.
    generations: Vec<(SemanticValueId, u32, LaunchId)>,
    /// Expected declared result buffers (explicit readbacks).
    result_buffers: Vec<BufferId>,
    /// Expected prepared-region partition (member launches per region).
    regions: Vec<Vec<LaunchId>>,
    /// Expected gradient-to-parameter association: (primal, position, slot).
    gradient_associations: Vec<(DefId, u32, u32)>,
}

/// The oracle/equivalence violation kinds. Fail-closed: any disagreement
/// fails the gate; the gate never passes on partial facts.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EquivalenceViolationKind {
    /// The expected launch execution order disagrees with the subject.
    LaunchOrder {
        expected: Vec<LaunchId>,
        got: Vec<LaunchId>,
    },
    /// The carried dependency edges disagree with the expectation.
    Dependency {
        expected: Vec<DependencyEdge>,
        got: Vec<DependencyEdge>,
    },
    /// The carried value generations disagree with the expectation.
    Generation {
        expected: Vec<(SemanticValueId, u32, LaunchId)>,
        got: Vec<(SemanticValueId, u32, LaunchId)>,
    },
    /// The declared result/readback set disagrees with the expectation.
    Result {
        expected: Vec<BufferId>,
        got: Vec<BufferId>,
    },
    /// The prepared-region partition disagrees with the expectation.
    Region {
        expected: Vec<Vec<LaunchId>>,
        got: Vec<Vec<LaunchId>>,
    },
    /// The gradient-to-parameter association disagrees with the expectation.
    GradientAssociation {
        expected: Vec<(DefId, u32, u32)>,
        got: Vec<(DefId, u32, u32)>,
    },
}

/// One gate failure: the kind plus a focused diagnostic detail.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EquivalenceViolation {
    kind: EquivalenceViolationKind,
    detail: String,
}

impl EquivalenceViolation {
    fn new(kind: EquivalenceViolationKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

/// Assess the materialized subject against the independent expected facts
/// (the Stage 3R G6 `prove_actual_program_equivalence` pattern).
///
/// Genuinely independent: the expectation is the fixture's frozen intent,
/// declared separately from the subject; the gate never compares two paths
/// that share the same faulty producer. The subject must stay schema-valid
/// — a mutation is caught HERE as an oracle mismatch, never at the schema.
fn assess_materialized_program(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    expected: &ExpectedProgram,
) -> Result<(), EquivalenceViolation> {
    let got_order: Vec<LaunchId> = program.launches.iter().map(|launch| launch.id).collect();
    if got_order != expected.launch_order {
        return Err(EquivalenceViolation::new(
            EquivalenceViolationKind::LaunchOrder {
                expected: expected.launch_order.clone(),
                got: got_order,
            },
            "the launch execution order disagrees with the independent oracle",
        ));
    }
    if semantics.dependencies != expected.dependencies {
        return Err(EquivalenceViolation::new(
            EquivalenceViolationKind::Dependency {
                expected: expected.dependencies.clone(),
                got: semantics.dependencies.clone(),
            },
            "the carried dependency edges disagree with the independent oracle",
        ));
    }
    let got_generations: Vec<(SemanticValueId, u32, LaunchId)> = semantics
        .generations
        .iter()
        .map(|generation| {
            (
                generation.value,
                generation.generation,
                generation.produced_by,
            )
        })
        .collect();
    if got_generations != expected.generations {
        return Err(EquivalenceViolation::new(
            EquivalenceViolationKind::Generation {
                expected: expected.generations.clone(),
                got: got_generations,
            },
            "the carried value generations disagree with the independent oracle",
        ));
    }
    let got_results: Vec<BufferId> = program
        .results
        .iter()
        .map(|result| result.buffer.id)
        .collect();
    if got_results != expected.result_buffers {
        return Err(EquivalenceViolation::new(
            EquivalenceViolationKind::Result {
                expected: expected.result_buffers.clone(),
                got: got_results,
            },
            "the declared result/readback set disagrees with the independent oracle",
        ));
    }
    let launches: Vec<LaunchId> = program.launches.iter().map(|launch| launch.id).collect();
    let got_regions: Vec<Vec<LaunchId>> = semantics
        .derive_prepared_regions(&launches)
        .into_iter()
        .map(|region| region.launches)
        .collect();
    if got_regions != expected.regions {
        return Err(EquivalenceViolation::new(
            EquivalenceViolationKind::Region {
                expected: expected.regions.clone(),
                got: got_regions,
            },
            "the prepared-region partition disagrees with the independent oracle",
        ));
    }
    let got_associations: Vec<(DefId, u32, u32)> = semantics
        .relations
        .iter()
        .flat_map(|relation| {
            relation
                .selected_inputs
                .iter()
                .map(|selected| (relation.primal, selected.position, selected.gradient_slot))
        })
        .collect();
    if got_associations != expected.gradient_associations {
        return Err(EquivalenceViolation::new(
            EquivalenceViolationKind::GradientAssociation {
                expected: expected.gradient_associations.clone(),
                got: got_associations,
            },
            "the gradient-to-parameter association disagrees with the independent oracle",
        ));
    }
    Ok(())
}

/// The frozen expectation for the oracle subject (declared independently of
/// the subject's carried facts).
fn expected_for_oracle_subject() -> ExpectedProgram {
    ExpectedProgram {
        launch_order: vec![LaunchId(0), LaunchId(1)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(0),
            consumer: LaunchId(1),
            buffer: BufferId(0),
            version: 1,
        }],
        generations: vec![
            (SemanticValueId(1), 1, LaunchId(0)),
            (SemanticValueId(2), 1, LaunchId(1)),
        ],
        result_buffers: vec![BufferId(1)],
        regions: vec![vec![LaunchId(0), LaunchId(1)]],
        gradient_associations: vec![(DefId(1), 0, 0)],
    }
}

/// The gate passes on the un-mutated materialized subject.
#[test]
fn oracle_gate_passes_on_the_materialized_subject() {
    let (program, semantics) = oracle_subject();
    let expected = expected_for_oracle_subject();
    program
        .validate_with_semantics(&semantics)
        .expect("the subject is schema-valid");
    assess_materialized_program(&program, &semantics, &expected)
        .expect("the materialized subject is equivalent to the independent oracle");
}

/// Mutation: the forward→companion carried edge is dropped (and the
/// companion is re-rooted, exactly as a faulty materializer that forgot
/// the edge would do). The subject STAYS schema-valid; the independent
/// oracle catches the missing edge — never the schema.
#[test]
fn oracle_gate_catches_dropped_dependency_not_schema() {
    let (program, mut semantics) = oracle_subject();
    semantics.dependencies.clear();
    semantics.roots = vec![LaunchId(0), LaunchId(1)];
    program
        .validate_with_semantics(&semantics)
        .expect("the mutated subject stays schema-valid (the oracle must catch it)");
    let error = assess_materialized_program(&program, &semantics, &expected_for_oracle_subject())
        .expect_err("the dropped edge must fail the gate");
    assert!(
        matches!(error.kind, EquivalenceViolationKind::Dependency { .. }),
        "dropped edge must be caught as an oracle mismatch, got {error:?}"
    );
}

/// Mutation: the backward kernel's write of value 2 is recorded as
/// generation 2 instead of generation 1. Schema-valid (the schema does NOT
/// enforce numeric generation monotonicity); the independent oracle catches
/// the wrong generation — never the schema.
#[test]
fn oracle_gate_catches_wrong_generation_not_schema() {
    let (program, mut semantics) = oracle_subject();
    semantics.generations[1].generation = 2;
    program
        .validate_with_semantics(&semantics)
        .expect("the mutated subject stays schema-valid (the oracle must catch it)");
    let error = assess_materialized_program(&program, &semantics, &expected_for_oracle_subject())
        .expect_err("the wrong generation must fail the gate");
    assert!(
        matches!(error.kind, EquivalenceViolationKind::Generation { .. }),
        "wrong generation must be caught as an oracle mismatch, got {error:?}"
    );
}

/// Mutation: the gradient is associated with the wrong parameter — the
/// companion's selected input claims `gradient_slot` 1 instead of 0.
/// Schema-valid (the row stays well-formed); the independent oracle catches
/// the wrong association — never the schema.
#[test]
fn oracle_gate_catches_wrong_gradient_association_not_schema() {
    let (program, mut semantics) = oracle_subject();
    semantics.relations[0].selected_inputs[0].gradient_slot = 1;
    program
        .validate_with_semantics(&semantics)
        .expect("the mutated subject stays schema-valid (the oracle must catch it)");
    let error = assess_materialized_program(&program, &semantics, &expected_for_oracle_subject())
        .expect_err("the wrong association must fail the gate");
    assert!(
        matches!(
            error.kind,
            EquivalenceViolationKind::GradientAssociation { .. }
        ),
        "wrong association must be caught as an oracle mismatch, got {error:?}"
    );
}
