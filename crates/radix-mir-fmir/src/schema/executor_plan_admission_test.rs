use super::*;
use crate::schema::executor_plan::{
    WireAllocationFact, WireAllocationIdentity, WireBackendRequirement, WireExecutorPlan,
    WireExecutorPlanV2, WireLogicalSignature, WirePlanProvenance,
};
use crate::schema::validate::validate_wire_program;
use crate::schema::wire::{
    WireBarrierPhase, WireBarrierPoint, WireBinding, WireBufferIdentity, WireBufferLifetime,
    WireBufferRole, WireBufferVersion, WireCausalMaskedSoftmaxPlan, WireCollectionKernelPlan,
    WireDependencyEdge, WireDeviceProgram, WireDeviceResource, WireDispatchSize,
    WireInitializationPolicy, WireKernelLaunchPlan, WireKernelUnit, WireLaunchUnit,
    WireMatMulLayout, WireMatMulPlan, WireMatMulSharedMemory, WireObservationCadence,
    WireObservationFact, WireOobPaddingPolicy, WireProgramLifetime, WireResourceAccess,
    WireResultBuffer, WireRmsNormalizationPlan, WireRopePlan, WireSemanticValue,
    WireSemanticValueOrigin, WireSharedMemoryLayout, WireStorageLayout, WireTransposePlan,
    WireWorkgroupCount, WireWorkgroupSize,
};
use std::collections::BTreeSet;

fn payload() -> WireExecutorPlanV2 {
    WireExecutorPlanV2 {
        abi_version: super::super::executor_plan::EXECUTOR_PLAN_V2_VERSION,
        selections: vec![WireExecutorPlan {
            kernel_index: 0,
            library_entry: Some("kv_append".to_owned()),
            logical_signature: WireLogicalSignature {
                operation: "kv_append".to_owned(),
                inputs: vec!["k".to_owned()],
                outputs: vec!["cache_k".to_owned()],
            },
            primary_bind: super::super::executor_plan::WireBindDescriptor {
                dims: vec![2, 4],
                strides: vec![4, 1],
                layout: "strided".to_owned(),
                grid: [1, 1, 1],
            },
            binds: Vec::new(),
            uniforms: Vec::new(),
            offsets: Vec::new(),
            workspace: None,
            backend_requirements: vec![WireBackendRequirement {
                backend: "metal".to_owned(),
                feature: "buffer-offsets".to_owned(),
            }],
            provenance: WirePlanProvenance {
                pass: "kv-cache-plan".to_owned(),
                revision: 2,
                source_nodes: vec![17],
            },
        }],
        allocations: vec![WireAllocationFact {
            buffer: WireAllocationIdentity {
                id: 7,
                name: "cache_k".to_owned(),
                role: WireBufferRole::InOut,
                storage: WireStorageLayout::DeviceHandle,
                lifetime: WireBufferLifetime::PerProgram,
            },
            dtype: "F32".to_owned(),
            capacity_bytes: 4096,
            lifetime: WireBufferLifetime::PerProgram,
            initialization: WireInitializationPolicy::ZeroFill,
        }],
        views: vec![
            super::super::executor_plan::WireViewFact {
                allocation: 7,
                logical_dims: vec![2, 4],
                strides: vec![4, 1],
                static_base: 0,
                maximum_span: 32,
            },
            super::super::executor_plan::WireViewFact {
                allocation: 7,
                logical_dims: vec![1, 4],
                strides: vec![4, 1],
                static_base: 4,
                maximum_span: 16,
            },
        ],
        launch_bindings: vec![super::super::executor_plan::WireLaunchBinding {
            kernel_index: 0,
            handle: 7,
            binding: WireBinding {
                group: 0,
                binding: 3,
            },
            byte_offset: 0,
            view_span: 32,
            runtime_source: WireExecutorPlanSource::Runtime {
                source: WireRuntimeSource::Position,
            },
        }],
    }
}

#[test]
fn v2_runtime_sources_and_views_round_trip() {
    let original = payload();
    let bytes = postcard::to_allocvec(&original).expect("v2 payload serializes");
    let decoded: WireExecutorPlanV2 =
        postcard::from_bytes(&bytes).expect("v2 payload deserializes");
    assert_eq!(decoded, original);
    admit_executor_plan_v2(&decoded, 1).expect("round-tripped v2 payload admits");
}

#[test]
fn producer_v2_facts_project_to_the_fmir_mirror() {
    let producer = radix_mir::device_program_plans::ExecutorPlanExtensionV2 {
        abi_version: radix_mir::device_program_plans::EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION,
        selections: Vec::new(),
        allocations: vec![radix_mir::device_program_plans::AllocationFact {
            buffer: radix_mir::BufferIdentity {
                id: radix_mir::BufferId(7),
                name: "cache_k".to_owned(),
                role: radix_mir::BufferRole::InOut,
                storage: radix_mir::MirTensorStorageLayout::DeviceHandle,
                lifetime: radix_mir::BufferLifetime::PerProgram,
            },
            dtype: radix_mir::UniformType::F32,
            capacity_bytes: 4096,
            lifetime: radix_mir::BufferLifetime::PerProgram,
            initialization: radix_mir::device_semantics::InitializationPolicy::ZeroFill,
        }],
        views: vec![radix_mir::device_program_plans::ViewFact {
            allocation: radix_mir::BufferId(7),
            logical_dims: vec![2, 4],
            strides: vec![4, 1],
            static_base: 0,
            maximum_span: 32,
        }],
        launch_bindings: vec![radix_mir::device_program_plans::LaunchBinding::new(
            0,
            radix_mir::BufferId(7),
            radix_mir::Binding {
                group: 0,
                binding: 3,
            },
            0,
            32,
            radix_mir::device_program_plans::ExecutorPlanSource::runtime(
                radix_mir::device_program_plans::RuntimeSource::Position,
            ),
        )],
    };
    let mirror = WireExecutorPlanV2::from_mir(&producer);
    assert_eq!(mirror.abi_version, 2);
    assert_eq!(mirror.allocations[0].buffer.id, 7);
    assert_eq!(mirror.views[0].maximum_span, 32);
    assert!(matches!(
        mirror.launch_bindings[0].runtime_source,
        WireExecutorPlanSource::Runtime {
            source: WireRuntimeSource::Position
        }
    ));
}

#[test]
fn unknown_runtime_source_fails_closed() {
    let mut payload = payload();
    payload.launch_bindings[0].runtime_source = WireExecutorPlanSource::Runtime {
        source: WireRuntimeSource::Unknown("future_cursor".to_owned()),
    };
    assert!(matches!(
        admit_executor_plan_v2(&payload, 1),
        Err(ExecutorPlanAdmissionError::UnknownRuntimeSource { source })
            if source == "future_cursor"
    ));
}

#[test]
fn overflowing_view_span_fails_closed() {
    let mut payload = payload();
    payload.views[0].logical_dims = vec![u64::MAX];
    payload.views[0].strides = vec![2];
    payload.views[0].maximum_span = u64::MAX;
    assert!(matches!(
        admit_executor_plan_v2(&payload, 1),
        Err(ExecutorPlanAdmissionError::ArithmeticOverflow { .. })
    ));
}

#[test]
fn overflowing_launch_span_fails_closed() {
    let mut payload = payload();
    payload.launch_bindings[0].byte_offset = u64::MAX - 1;
    payload.launch_bindings[0].view_span = 2;
    assert!(matches!(
        admit_executor_plan_v2(&payload, 1),
        Err(ExecutorPlanAdmissionError::ArithmeticOverflow { .. })
    ));
}

#[test]
fn payload_version_skew_fails_before_field_admission() {
    let mut payload = payload();
    payload.abi_version = 1;
    payload.selections.clear();
    assert!(matches!(
        admit_executor_plan_v2(&payload, 1),
        Err(ExecutorPlanAdmissionError::UnsupportedVersion {
            actual: 1,
            expected: 2
        })
    ));
}

#[test]
fn outer_section_version_skew_fails_before_payload_admission() {
    let payload = payload();
    assert!(matches!(
        admit_executor_plan_section_v2_with_version(1, &payload, 1),
        Err(ExecutorPlanAdmissionError::UnsupportedSectionVersion {
            actual: 1,
            expected: 2
        })
    ));
}

#[test]
fn v1_static_plan_has_no_runtime_source_shape() {
    let legacy = WireExecutorPlan {
        kernel_index: 0,
        library_entry: None,
        logical_signature: WireLogicalSignature {
            operation: "legacy".to_owned(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        },
        primary_bind: super::super::executor_plan::WireBindDescriptor {
            dims: vec![1],
            strides: vec![1],
            layout: "flat".to_owned(),
            grid: [1, 1, 1],
        },
        binds: Vec::new(),
        uniforms: vec![super::super::executor_plan::WireUniformBinding {
            name: "constant".to_owned(),
            ty: "U32".to_owned(),
            bits: 7,
        }],
        offsets: Vec::new(),
        workspace: None,
        backend_requirements: Vec::new(),
        provenance: WirePlanProvenance {
            pass: "legacy".to_owned(),
            revision: 1,
            source_nodes: vec![1],
        },
    };
    assert_eq!(legacy.uniforms[0].bits, 7);
    let section = crate::schema::device::FmirExecutorPlanSection {
        v: crate::schema::device::LEGACY_EXECUTOR_PLAN_SECTION_VERSION,
        plans: vec![legacy],
    };
    admit_legacy_static_executor_plan_section(&section)
        .expect("a v1 static selection carrier still admits");
}

#[test]
fn v1_section_version_with_runtime_facts_fails_closed() {
    let payload = payload();
    assert!(matches!(
        admit_executor_plan_section_v2_with_version(
            crate::schema::device::LEGACY_EXECUTOR_PLAN_SECTION_VERSION,
            &payload,
            1
        ),
        Err(ExecutorPlanAdmissionError::UnsupportedSectionVersion {
            actual: 1,
            expected: 2
        })
    ));
}

#[test]
fn unknown_dtype_fails_closed() {
    let mut payload = payload();
    payload.allocations[0].dtype = "F64".to_owned();
    assert!(matches!(
        admit_executor_plan_v2(&payload, 1),
        Err(ExecutorPlanAdmissionError::UnknownDtype { dtype }) if dtype == "F64"
    ));
}

fn gea1_bf16_payload() -> WireExecutorPlanV2 {
    let mut payload = payload();
    // [320, 960] BF16 weights occupy exactly 320 * 960 * 2 bytes.
    payload.allocations[0].dtype = "BF16".to_owned();
    payload.allocations[0].capacity_bytes = 614_400;
    payload.views[0].logical_dims = vec![320, 960];
    payload.views[0].strides = vec![960, 1];
    payload.views[0].maximum_span = 614_400;
    payload.views[1].logical_dims = vec![320];
    payload.views[1].strides = vec![1];
    payload.views[1].static_base = 0;
    payload.views[1].maximum_span = 640;
    payload.launch_bindings[0].view_span = 614_400;
    payload
}

#[test]
fn gea1_executor_plan_admits_complete_ranges() {
    let payload = gea1_bf16_payload();
    assert_eq!(payload.allocations[0].capacity_bytes, 320 * 960 * 2);
    admit_executor_plan_v2(&payload, 1).expect("complete BF16 GEA1 ranges admit");
}

#[test]
fn gea1_executor_plan_rejects_dtype_range_or_geometry_drift() {
    let mut wrong_dtype = gea1_bf16_payload();
    wrong_dtype.allocations[0].dtype = "F64".to_owned();
    assert!(matches!(
        admit_executor_plan_v2(&wrong_dtype, 1),
        Err(ExecutorPlanAdmissionError::UnknownDtype { dtype }) if dtype == "F64"
    ));

    let mut short_range = gea1_bf16_payload();
    short_range.views[0].maximum_span = 614_399;
    assert!(matches!(
        admit_executor_plan_v2(&short_range, 1),
        Err(ExecutorPlanAdmissionError::InvalidFact { .. })
    ));

    let mut wrong_stride = gea1_bf16_payload();
    wrong_stride.views[0].strides = vec![961, 1];
    assert!(matches!(
        admit_executor_plan_v2(&wrong_stride, 1),
        Err(ExecutorPlanAdmissionError::InvalidFact { .. })
    ));

    let mut wrong_offset = gea1_bf16_payload();
    wrong_offset.views[0].static_base = 1;
    assert!(matches!(
        admit_executor_plan_v2(&wrong_offset, 1),
        Err(ExecutorPlanAdmissionError::InvalidFact { .. })
    ));

    let mut wrong_geometry = gea1_bf16_payload();
    wrong_geometry.selections[0].kernel_index = 1;
    assert!(matches!(
        admit_executor_plan_v2(&wrong_geometry, 1),
        Err(ExecutorPlanAdmissionError::SelectionShape { .. })
    ));
}

// ---------------------------------------------------------------------------
// GEA2-U4 block-program admission
// ---------------------------------------------------------------------------

const GEA2_ENTRY_NAMES: [&str; 13] = [
    "rmsnorm",
    "gemm_qo",
    "gemm_kv",
    "gemm_gate_up",
    "gemm_down",
    "rope_q",
    "rope_k",
    "transpose",
    "score_gemm",
    "causal_softmax",
    "context_gemm",
    "swiglu",
    "residual_add",
];

const GEA2_INSTANCE_COUNTS: [usize; 13] = [2, 2, 2, 2, 1, 1, 1, 5, 15, 15, 15, 1, 2];

fn gea2_matmul_plan(m: u64, k: u64, n: u64) -> WireCollectionKernelPlan {
    WireCollectionKernelPlan::TiledMatMul(WireMatMulPlan {
        m,
        k,
        n,
        left_layout: WireMatMulLayout::F32,
        right_layout: WireMatMulLayout::F32,
        tile: 8,
        workgroup_x: 8,
        workgroup_y: 8,
        shared_memory: WireMatMulSharedMemory {
            shared_a: WireSharedMemoryLayout {
                element_byte_width: 4,
                slot_count: 64,
                buffer_name: "shared_a".to_owned(),
            },
            shared_b: WireSharedMemoryLayout {
                element_byte_width: 4,
                slot_count: 64,
                buffer_name: "shared_b".to_owned(),
            },
        },
        barriers: vec![
            WireBarrierPoint {
                after: WireBarrierPhase::SharedMemoryLoad,
            },
            WireBarrierPoint {
                after: WireBarrierPhase::InnerProductStep,
            },
        ],
        oob_padding: WireOobPaddingPolicy::ZeroFill,
    })
}

fn gea2_plan(entry: &str) -> WireCollectionKernelPlan {
    match entry {
        "rmsnorm" => WireCollectionKernelPlan::RmsNormalization(WireRmsNormalizationPlan {
            axis: 1,
            epsilon_bits: (1.0e-5_f32).to_bits(),
            width: 960,
        }),
        "gemm_qo" => gea2_matmul_plan(8, 960, 960),
        "gemm_kv" => gea2_matmul_plan(8, 960, 320),
        "gemm_gate_up" => gea2_matmul_plan(8, 960, 2560),
        "gemm_down" => gea2_matmul_plan(8, 2560, 960),
        "rope_q" => WireCollectionKernelPlan::Rope(WireRopePlan {
            pos: 0,
            dim: 64,
            width: 960,
            per_row: true,
            rows: 8,
            rotate_half: false,
        }),
        "rope_k" => WireCollectionKernelPlan::Rope(WireRopePlan {
            pos: 0,
            dim: 64,
            width: 320,
            per_row: true,
            rows: 8,
            rotate_half: false,
        }),
        "transpose" => WireCollectionKernelPlan::Transpose(WireTransposePlan {
            m: 8,
            n: 64,
            workgroup_x: 64,
            dispatch_x: 512,
        }),
        "score_gemm" => gea2_matmul_plan(8, 64, 8),
        "causal_softmax" => {
            WireCollectionKernelPlan::CausalMaskedSoftmax(WireCausalMaskedSoftmaxPlan {
                rows: 8,
                cols: 8,
            })
        }
        "context_gemm" => gea2_matmul_plan(8, 8, 64),
        "swiglu" | "residual_add" => WireCollectionKernelPlan::Elementwise,
        other => panic!("unknown GEA2 entry {other}"),
    }
}

fn gea2_dispatch(entry: &str) -> WireKernelLaunchPlan {
    let (workgroup, dispatch) = match entry {
        "gemm_qo" | "gemm_down" => ([8, 8, 1], [120, 1, 1]),
        "gemm_kv" => ([8, 8, 1], [40, 1, 1]),
        "gemm_gate_up" => ([8, 8, 1], [320, 1, 1]),
        "score_gemm" => ([8, 8, 1], [1, 1, 1]),
        "context_gemm" => ([8, 8, 1], [8, 1, 1]),
        "transpose" => ([64, 1, 1], [512, 1, 1]),
        "causal_softmax" => ([1, 1, 1], [64, 1, 1]),
        "swiglu" => ([1, 1, 1], [20480, 1, 1]),
        "rmsnorm" | "rope_q" | "rope_k" | "residual_add" => ([1, 1, 1], [7680, 1, 1]),
        other => panic!("unknown GEA2 entry {other}"),
    };
    WireKernelLaunchPlan {
        workgroup: WireWorkgroupSize {
            x: workgroup[0],
            y: workgroup[1],
            z: workgroup[2],
        },
        dispatch_size: WireDispatchSize {
            x: dispatch[0],
            y: dispatch[1],
            z: dispatch[2],
        },
        workgroup_count: WireWorkgroupCount {
            x: dispatch[0],
            y: dispatch[1],
            z: dispatch[2],
        },
    }
}

fn gea2_wire_buffer(
    id: u32,
    name: impl Into<String>,
    role: WireBufferRole,
    lifetime: WireBufferLifetime,
) -> WireBufferIdentity {
    WireBufferIdentity {
        id,
        name: name.into(),
        role,
        storage: WireStorageLayout::DeviceHandle,
        lifetime,
        semantic_value: id,
    }
}

fn gea2_wire_resource(
    buffer: &WireBufferIdentity,
    element_count: u64,
    binding: u32,
    access: WireResourceAccess,
    initialization: WireInitializationPolicy,
) -> WireDeviceResource {
    WireDeviceResource {
        buffer: buffer.clone(),
        version: WireBufferVersion {
            version: 1,
            element_ty: "f32".to_owned(),
            element_count,
            shape: None,
            reduced_projection: None,
        },
        binding: WireBinding { group: 0, binding },
        access,
        generation: 1,
        initialization,
    }
}

fn gea2_add_semantic_value(values: &mut Vec<WireSemanticValue>, buffer: &WireBufferIdentity) {
    if values.iter().any(|value| value.id == buffer.id) {
        return;
    }
    values.push(WireSemanticValue {
        id: buffer.id,
        name: buffer.name.clone(),
        origin: WireSemanticValueOrigin::Synthetic {
            label: format!("gea2-buffer-{}", buffer.id),
        },
    });
}

fn gea2_append_wire_kernel(
    kernels: &mut Vec<WireKernelUnit>,
    launches: &mut Vec<WireLaunchUnit>,
    semantic_values: &mut Vec<WireSemanticValue>,
    entry: &str,
    resources: Vec<WireDeviceResource>,
) -> u32 {
    for resource in &resources {
        gea2_add_semantic_value(semantic_values, &resource.buffer);
    }
    let kernel_index = u32::try_from(kernels.len()).expect("GEA2 kernel count fits u32");
    let launch_id = u32::try_from(launches.len() + 1).expect("GEA2 launch count fits u32");
    let function = GEA2_ENTRY_NAMES
        .iter()
        .position(|candidate| *candidate == entry)
        .and_then(|index| u32::try_from(index).ok())
        .expect("GEA2 entry function index fits u32");
    kernels.push(WireKernelUnit {
        function,
        entry: entry.to_owned(),
        plan: gea2_plan(entry),
        resources,
        launch: gea2_dispatch(entry),
    });
    launches.push(WireLaunchUnit {
        id: launch_id,
        kernel_index,
    });
    launch_id
}

fn gea2_wire_edge(producer: u32, consumer: u32, buffer: &WireBufferIdentity) -> WireDependencyEdge {
    WireDependencyEdge {
        producer,
        consumer,
        buffer: buffer.id,
        version: 1,
    }
}

fn gea2_block_program() -> WireDeviceProgram {
    let x = gea2_wire_buffer(
        1,
        "activation_x",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let attn_norm = gea2_wire_buffer(
        2,
        "attn_norm_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let q_weight = gea2_wire_buffer(
        3,
        "q_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let k_weight = gea2_wire_buffer(
        4,
        "k_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let v_weight = gea2_wire_buffer(
        5,
        "v_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let rope_table = gea2_wire_buffer(
        6,
        "rope_table",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let o_weight = gea2_wire_buffer(
        7,
        "o_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let ffn_norm = gea2_wire_buffer(
        8,
        "ffn_norm_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let gate_weight = gea2_wire_buffer(
        9,
        "gate_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let up_weight = gea2_wire_buffer(
        10,
        "up_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let down_weight = gea2_wire_buffer(
        11,
        "down_weight",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let mut next_intermediate = 100_u32;
    let mut intermediate = |name: &str| {
        let id = next_intermediate;
        next_intermediate += 1;
        gea2_wire_buffer(id, name, WireBufferRole::InOut, WireBufferLifetime::PerStep)
    };
    let ln1 = intermediate("ln1");
    let q = intermediate("q");
    let k = intermediate("k");
    let v = intermediate("v");
    let rope_q = intermediate("rope_q");
    let rope_k = intermediate("rope_k");
    let transposes: Vec<_> = (0..5)
        .map(|head| intermediate(&format!("key_transpose_{head}")))
        .collect();
    let mut scores = Vec::new();
    let mut probabilities = Vec::new();
    let mut contexts = Vec::new();
    for head in 0..15 {
        scores.push(intermediate(&format!("score_{head}")));
        probabilities.push(intermediate(&format!("probabilities_{head}")));
        contexts.push(intermediate(&format!("context_{head}")));
    }
    let o_projection = intermediate("o_projection");
    let residual1 = intermediate("residual1");
    let ln2 = intermediate("ln2");
    let gate = intermediate("gate");
    let up = intermediate("up");
    let swiglu = intermediate("swiglu");
    let down = intermediate("down");
    let output = gea2_wire_buffer(
        next_intermediate,
        "block_output",
        WireBufferRole::Output,
        WireBufferLifetime::ObservationPoint,
    );

    let mut kernels = Vec::new();
    let mut launches = Vec::new();
    let mut semantic_values = Vec::new();
    let mut dependencies = Vec::new();
    let host = WireInitializationPolicy::HostProvided;
    let device = WireInitializationPolicy::KernelInitialized;
    let read = |buffer: &WireBufferIdentity, count: u64, binding: u32, init| {
        gea2_wire_resource(buffer, count, binding, WireResourceAccess::Read, init)
    };
    let write = |buffer: &WireBufferIdentity, count: u64, binding| {
        gea2_wire_resource(buffer, count, binding, WireResourceAccess::Write, device)
    };

    let launch_rmsnorm1 = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "rmsnorm",
        vec![
            read(&x, 7680, 0, host),
            read(&attn_norm, 960, 1, host),
            write(&ln1, 7680, 2),
        ],
    );
    let launch_q = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "gemm_qo",
        vec![
            read(&ln1, 7680, 0, device),
            read(&q_weight, 921_600, 1, host),
            write(&q, 7680, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_rmsnorm1, launch_q, &ln1));
    let launch_k = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "gemm_kv",
        vec![
            read(&ln1, 7680, 0, device),
            read(&k_weight, 307_200, 1, host),
            write(&k, 2560, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_rmsnorm1, launch_k, &ln1));
    let launch_v = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "gemm_kv",
        vec![
            read(&ln1, 7680, 0, device),
            read(&v_weight, 307_200, 1, host),
            write(&v, 2560, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_rmsnorm1, launch_v, &ln1));
    let launch_rope_q = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "rope_q",
        vec![
            read(&q, 7680, 0, device),
            read(&rope_table, 768, 1, host),
            write(&rope_q, 7680, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_q, launch_rope_q, &q));
    let launch_rope_k = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "rope_k",
        vec![
            read(&k, 2560, 0, device),
            read(&rope_table, 768, 1, host),
            write(&rope_k, 2560, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_k, launch_rope_k, &k));

    for transpose in &transposes {
        let launch = gea2_append_wire_kernel(
            &mut kernels,
            &mut launches,
            &mut semantic_values,
            "transpose",
            vec![read(&rope_k, 512, 0, device), write(transpose, 512, 1)],
        );
        dependencies.push(gea2_wire_edge(launch_rope_k, launch, &rope_k));
    }

    let mut context_launches = Vec::new();
    for head in 0..15 {
        let transpose = &transposes[head / 3];
        let score = gea2_append_wire_kernel(
            &mut kernels,
            &mut launches,
            &mut semantic_values,
            "score_gemm",
            vec![
                read(&rope_q, 512, 0, device),
                read(transpose, 512, 1, device),
                read(&x, 64, 2, host),
                write(&scores[head], 64, 3),
            ],
        );
        dependencies.push(gea2_wire_edge(launch_rope_q, score, &rope_q));
        let softmax = gea2_append_wire_kernel(
            &mut kernels,
            &mut launches,
            &mut semantic_values,
            "causal_softmax",
            vec![
                read(&scores[head], 64, 0, device),
                write(&probabilities[head], 64, 1),
            ],
        );
        dependencies.push(gea2_wire_edge(score, softmax, &scores[head]));
        let context = gea2_append_wire_kernel(
            &mut kernels,
            &mut launches,
            &mut semantic_values,
            "context_gemm",
            vec![
                read(&probabilities[head], 64, 0, device),
                read(&v, 512, 1, device),
                write(&contexts[head], 512, 2),
            ],
        );
        dependencies.push(gea2_wire_edge(softmax, context, &probabilities[head]));
        context_launches.push(context);
    }

    let mut o_resources = contexts
        .iter()
        .enumerate()
        .map(|(binding, context)| {
            read(
                context,
                512,
                u32::try_from(binding).expect("context binding fits"),
                device,
            )
        })
        .collect::<Vec<_>>();
    o_resources.push(read(&o_weight, 921_600, 15, host));
    o_resources.push(write(&o_projection, 7680, 16));
    let launch_o = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "gemm_qo",
        o_resources,
    );
    dependencies.push(gea2_wire_edge(
        *context_launches.last().expect("context launches"),
        launch_o,
        contexts.last().expect("context output"),
    ));
    let launch_residual1 = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "residual_add",
        vec![
            read(&x, 7680, 0, host),
            read(&o_projection, 7680, 1, device),
            write(&residual1, 7680, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_o, launch_residual1, &o_projection));
    let launch_rmsnorm2 = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "rmsnorm",
        vec![
            read(&residual1, 7680, 0, device),
            read(&ffn_norm, 960, 1, host),
            write(&ln2, 7680, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(
        launch_residual1,
        launch_rmsnorm2,
        &residual1,
    ));
    let launch_gate = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "gemm_gate_up",
        vec![
            read(&ln2, 7680, 0, device),
            read(&gate_weight, 2_457_600, 1, host),
            write(&gate, 20480, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_rmsnorm2, launch_gate, &ln2));
    let launch_up = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "gemm_gate_up",
        vec![
            read(&ln2, 7680, 0, device),
            read(&up_weight, 2_457_600, 1, host),
            write(&up, 20480, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_rmsnorm2, launch_up, &ln2));
    let launch_swiglu = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "swiglu",
        vec![
            read(&gate, 20480, 0, device),
            read(&up, 20480, 1, device),
            write(&swiglu, 20480, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_gate, launch_swiglu, &gate));
    let launch_down = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "gemm_down",
        vec![
            read(&swiglu, 20480, 0, device),
            read(&down_weight, 2_457_600, 1, host),
            write(&down, 7680, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_swiglu, launch_down, &swiglu));
    let launch_output = gea2_append_wire_kernel(
        &mut kernels,
        &mut launches,
        &mut semantic_values,
        "residual_add",
        vec![
            read(&residual1, 7680, 0, device),
            read(&down, 7680, 1, device),
            write(&output, 7680, 2),
        ],
    );
    dependencies.push(gea2_wire_edge(launch_down, launch_output, &down));

    assert_eq!(kernels.len(), 64);
    assert_eq!(launches.len(), 64);
    assert_eq!(dependencies.len(), 63);
    WireDeviceProgram {
        kernels,
        launches,
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: output,
            version: WireBufferVersion {
                version: 1,
                element_ty: "f32".to_owned(),
                element_count: 7680,
                shape: None,
                reduced_projection: None,
            },
            role: WireBufferRole::Output,
            produced_by: launch_output,
            observation: WireObservationFact {
                at_launch: launch_output,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values,
        roots: vec![1],
        dependencies,
        relations: Vec::new(),
    }
}

fn admit_gea2_block_program(program: &WireDeviceProgram) -> Result<(), String> {
    validate_wire_program(program).map_err(|error| error.to_string())?;
    let expected_launches: usize = GEA2_INSTANCE_COUNTS.iter().sum();
    if program.kernels.len() != expected_launches || program.launches.len() != expected_launches {
        return Err(format!(
            "GEA2 instance-expanded program has {} kernels and {} launches, expected {expected_launches}",
            program.kernels.len(),
            program.launches.len()
        ));
    }
    for (index, launch) in program.launches.iter().enumerate() {
        let expected_id = u32::try_from(index + 1).expect("GEA2 launch id fits u32");
        let expected_kernel = u32::try_from(index).expect("GEA2 kernel index fits u32");
        if launch.id != expected_id || launch.kernel_index != expected_kernel {
            return Err(format!(
                "launch {expected_id} is not a one-to-one instance declaration"
            ));
        }
    }
    for (entry, expected) in GEA2_ENTRY_NAMES.iter().zip(GEA2_INSTANCE_COUNTS) {
        let actual = program
            .kernels
            .iter()
            .filter(|kernel| kernel.entry == *entry)
            .count();
        if actual != expected {
            return Err(format!(
                "GEA2 instance count for {entry} is {actual}, expected {expected}"
            ));
        }
    }
    let buffers = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .map(|resource| resource.buffer.id)
        .collect::<BTreeSet<_>>();
    let producer_buffers = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .filter(|resource| {
            matches!(
                resource.access,
                WireResourceAccess::Write | WireResourceAccess::ReadWrite
            )
        })
        .map(|resource| resource.buffer.id)
        .collect::<BTreeSet<_>>();
    for edge in &program.dependencies {
        if edge.producer >= edge.consumer {
            return Err(format!(
                "dependency edge {} → {} is non-topological",
                edge.producer, edge.consumer
            ));
        }
        if !buffers.contains(&edge.buffer) {
            return Err(format!(
                "dependency edge {} → {} names undeclared intermediate {}",
                edge.producer, edge.consumer, edge.buffer
            ));
        }
        if !producer_buffers.contains(&edge.buffer) {
            return Err(format!(
                "dependency edge {} → {} names buffer {} without a producer",
                edge.producer, edge.consumer, edge.buffer
            ));
        }
    }
    let expected_edges = program.launches.len().saturating_sub(1);
    if program.dependencies.len() != expected_edges {
        return Err(format!(
            "missing dependency edge: {} present, {expected_edges} expected",
            program.dependencies.len()
        ));
    }
    if program.roots != vec![1] {
        return Err(format!(
            "GEA2 roots must be exactly [1], got {:?}",
            program.roots
        ));
    }
    let result = program
        .results
        .first()
        .ok_or("block output is undeclared")?;
    if result.version.element_ty != "f32"
        || result.version.element_count != 8 * 960
        || result.buffer.lifetime != WireBufferLifetime::ObservationPoint
        || result.produced_by != 64
        || result.observation.at_launch != 64
    {
        return Err("block output must be declared as F32 [8,960]".to_owned());
    }
    if !program
        .dependencies
        .iter()
        .any(|edge| edge.producer == 6 && edge.consumer == 7)
        || !program
            .dependencies
            .iter()
            .any(|edge| edge.producer == 12 && edge.consumer == 13)
        || !program
            .dependencies
            .iter()
            .any(|edge| edge.producer == 13 && edge.consumer == 14)
    {
        return Err("GEA2 per-instance attention edges are missing".to_owned());
    }
    Ok(())
}

#[test]
fn gea2_block_program_admits_complete_plan() {
    let program = gea2_block_program();
    admit_gea2_block_program(&program).expect("complete GEA2 block graph admits");
    assert_eq!(program.kernels.len(), 64);
    assert_eq!(program.launches.len(), 64);
    assert_eq!(program.dependencies.len(), 63);
    assert_eq!(program.roots, vec![1]);
}

#[test]
fn gea2_block_program_rejects_missing_edges_or_undeclared_intermediates() {
    let mut missing_edge = gea2_block_program();
    missing_edge.dependencies.remove(11);
    let error = admit_gea2_block_program(&missing_edge).expect_err("missing edge fails closed");
    assert!(
        error.contains("missing dependency") || error.contains("not reachable"),
        "unexpected error: {error}"
    );

    let mut undeclared = gea2_block_program();
    undeclared.dependencies[0].buffer = u32::MAX;
    let error =
        admit_gea2_block_program(&undeclared).expect_err("undeclared intermediate fails closed");
    assert!(
        error.contains("undeclared intermediate"),
        "unexpected error: {error}"
    );

    let mut non_topological = gea2_block_program();
    non_topological.dependencies[0].consumer = non_topological.dependencies[0].producer;
    let error =
        admit_gea2_block_program(&non_topological).expect_err("non-topological edge fails closed");
    assert!(
        error.contains("itself") || error.contains("non-topological"),
        "unexpected error: {error}"
    );

    let mut undeclared_root = gea2_block_program();
    undeclared_root.roots = vec![99];
    let error =
        admit_gea2_block_program(&undeclared_root).expect_err("undeclared root fails closed");
    assert!(error.contains("root 99"), "unexpected error: {error}");

    let mut wrong_output = gea2_block_program();
    wrong_output.results[0].version.element_count = 1;
    let error =
        admit_gea2_block_program(&wrong_output).expect_err("wrong declared output fails closed");
    assert!(error.contains("F32 [8,960]"), "unexpected error: {error}");
}

// ---------------------------------------------------------------------------
// GEA3-U4 full-model program admission
// ---------------------------------------------------------------------------

const GEA3_LAYER_COUNT: usize = 32;
const GEA3_BLOCK_LAUNCHES: usize = 64;
const GEA3_HISTORY_CAPACITY: u64 = 76;
const GEA3_KV_WIDTH: u64 = 320;
const GEA3_VOCAB: u64 = 49_152;
const GEA3_PREFILL_ROWS: u64 = 36;

fn gea3_block_entries(prefill: bool) -> Vec<&'static str> {
    let (
        rmsnorm,
        projection,
        kv,
        rope_q,
        rope_k,
        transpose,
        score,
        softmax,
        context,
        gate_up,
        down,
        swiglu,
        residual,
    ) = if prefill {
        (
            "prefill_rmsnorm",
            "prefill_gemm_qo",
            "prefill_gemm_kv",
            "prefill_rope_q",
            "prefill_rope_k",
            "prefill_key_transpose",
            "prefill_score_gemm",
            "prefill_causal_softmax",
            "prefill_context_gemm",
            "prefill_gemm_gate_up",
            "prefill_gemm_down",
            "prefill_swiglu",
            "prefill_residual_add",
        )
    } else {
        (
            "decode_rmsnorm",
            "decode_gemv_qo",
            "decode_gemv_kv",
            "decode_rope_q",
            "decode_rope_k",
            "decode_key_transpose",
            "decode_score_gemm",
            "decode_masked_softmax",
            "decode_context_gemm",
            "decode_gemv_gate_up",
            "decode_gemv_down",
            "decode_swiglu",
            "decode_residual_add",
        )
    };
    let mut entries = vec![rmsnorm, projection, kv, kv, rope_q, rope_k];
    entries.extend(std::iter::repeat_n(transpose, 5));
    entries.extend(std::iter::repeat_n(score, 15));
    entries.extend(std::iter::repeat_n(softmax, 15));
    entries.extend(std::iter::repeat_n(context, 15));
    entries.extend([
        projection, residual, rmsnorm, gate_up, gate_up, swiglu, down, residual,
    ]);
    assert_eq!(entries.len(), GEA3_BLOCK_LAUNCHES);
    entries
}

fn gea3_wire_plan(entry: &str) -> WireCollectionKernelPlan {
    if entry.ends_with("rmsnorm") {
        WireCollectionKernelPlan::RmsNormalization(WireRmsNormalizationPlan {
            axis: 1,
            epsilon_bits: (1.0e-5_f32).to_bits(),
            width: 960,
        })
    } else {
        WireCollectionKernelPlan::Elementwise
    }
}

fn gea3_wire_launch(element_count: u64) -> WireKernelLaunchPlan {
    WireKernelLaunchPlan {
        workgroup: WireWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: WireDispatchSize {
            x: element_count,
            y: 1,
            z: 1,
        },
        workgroup_count: WireWorkgroupCount {
            x: element_count,
            y: 1,
            z: 1,
        },
    }
}

fn gea3_wire_buffer(
    id: u32,
    name: impl Into<String>,
    role: WireBufferRole,
    lifetime: WireBufferLifetime,
) -> WireBufferIdentity {
    WireBufferIdentity {
        id,
        name: name.into(),
        role,
        storage: WireStorageLayout::DeviceHandle,
        lifetime,
        semantic_value: id,
    }
}

// Private fixture builder: each arg names one wire-table field under test.
// Precedent: faber dense_full_model_run_test.rs test constructors.
#[allow(clippy::too_many_arguments)]
fn gea3_append_kernel(
    kernels: &mut Vec<WireKernelUnit>,
    launches: &mut Vec<WireLaunchUnit>,
    semantic_values: &mut Vec<WireSemanticValue>,
    entry: &str,
    input: &WireBufferIdentity,
    output: &WireBufferIdentity,
    extras: Vec<WireDeviceResource>,
    output_count: u64,
) -> u32 {
    let input_initialization = if input.id == 1 {
        WireInitializationPolicy::HostProvided
    } else {
        WireInitializationPolicy::KernelInitialized
    };
    let mut resources = vec![gea2_wire_resource(
        input,
        960,
        0,
        WireResourceAccess::Read,
        input_initialization,
    )];
    resources.extend(extras);
    resources.push(gea2_wire_resource(
        output,
        output_count,
        u32::try_from(resources.len()).expect("GEA3 resource binding fits u32"),
        WireResourceAccess::Write,
        WireInitializationPolicy::KernelInitialized,
    ));
    for resource in &resources {
        gea2_add_semantic_value(semantic_values, &resource.buffer);
    }
    let kernel_index = u32::try_from(kernels.len()).expect("GEA3 kernel index fits u32");
    let launch_id = u32::try_from(launches.len() + 1).expect("GEA3 launch id fits u32");
    kernels.push(WireKernelUnit {
        function: kernel_index,
        entry: entry.to_owned(),
        plan: gea3_wire_plan(entry),
        resources,
        launch: gea3_wire_launch(output_count),
    });
    launches.push(WireLaunchUnit {
        id: launch_id,
        kernel_index,
    });
    launch_id
}

fn gea3_full_model_program(prefill: bool) -> WireDeviceProgram {
    let expected_launches = GEA3_LAYER_COUNT * GEA3_BLOCK_LAUNCHES + 3;
    let result_count = if prefill {
        GEA3_PREFILL_ROWS * GEA3_VOCAB
    } else {
        GEA3_VOCAB
    };
    let mut next_buffer = 1_u32;
    let input = gea3_wire_buffer(
        next_buffer,
        "token_ids",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    next_buffer += 1;
    let mut prior = input.clone();
    let mut kernels = Vec::new();
    let mut launches = Vec::new();
    let mut semantic_values = Vec::new();
    gea2_add_semantic_value(&mut semantic_values, &input);
    let mut dependencies = Vec::new();

    for index in 0..expected_launches {
        let layer = index
            .checked_sub(1)
            .map(|offset| offset / GEA3_BLOCK_LAUNCHES);
        let ordinal = index
            .checked_sub(1)
            .map(|offset| offset % GEA3_BLOCK_LAUNCHES);
        let entry = if index == 0 {
            "embedding_gather"
        } else if index == expected_launches - 2 {
            "head_rmsnorm"
        } else if index == expected_launches - 1 {
            "lm_head_gemv"
        } else {
            gea3_block_entries(prefill)[ordinal.expect("GEA3 block ordinal")]
        };
        let is_result = index == expected_launches - 1;
        let output_count = if is_result { result_count } else { 960 };
        let output = gea3_wire_buffer(
            next_buffer,
            format!("layer_{layer:?}_{index:04}_{entry}_output"),
            if is_result {
                WireBufferRole::Output
            } else {
                WireBufferRole::InOut
            },
            if is_result {
                WireBufferLifetime::ObservationPoint
            } else {
                WireBufferLifetime::PerStep
            },
        );
        next_buffer += 1;
        let mut extras = Vec::new();
        if index == 0 {
            let weight = gea3_wire_buffer(
                next_buffer,
                "token_embd.weight",
                WireBufferRole::Input,
                WireBufferLifetime::PerProgram,
            );
            next_buffer += 1;
            extras.push(gea2_wire_resource(
                &weight,
                960,
                1,
                WireResourceAccess::Read,
                WireInitializationPolicy::HostProvided,
            ));
        }
        if let Some(layer) = layer.filter(|_| ordinal == Some(0)) {
            for weight in [
                "attn_norm",
                "attn_q",
                "attn_k",
                "attn_v",
                "attn_output",
                "ffn_norm",
                "ffn_gate",
                "ffn_up",
                "ffn_down",
            ] {
                let identity = gea3_wire_buffer(
                    next_buffer,
                    format!("blk.{layer:02}.{weight}.weight"),
                    WireBufferRole::Input,
                    WireBufferLifetime::PerProgram,
                );
                next_buffer += 1;
                extras.push(gea2_wire_resource(
                    &identity,
                    960,
                    u32::try_from(extras.len() + 1).expect("GEA3 weight binding fits u32"),
                    WireResourceAccess::Read,
                    WireInitializationPolicy::HostProvided,
                ));
            }
            for cache in ["kv_k", "kv_v"] {
                let identity = gea3_wire_buffer(
                    next_buffer,
                    format!("blk.{layer:02}.{cache}"),
                    WireBufferRole::InOut,
                    WireBufferLifetime::PerProgram,
                );
                next_buffer += 1;
                extras.push(gea2_wire_resource(
                    &identity,
                    GEA3_HISTORY_CAPACITY * GEA3_KV_WIDTH,
                    u32::try_from(extras.len() + 1).expect("GEA3 cache binding fits u32"),
                    WireResourceAccess::ReadWrite,
                    WireInitializationPolicy::ZeroFill,
                ));
            }
        }
        let launch = gea3_append_kernel(
            &mut kernels,
            &mut launches,
            &mut semantic_values,
            entry,
            &prior,
            &output,
            extras,
            output_count,
        );
        if index > 0 {
            dependencies.push(gea2_wire_edge(launch - 1, launch, &prior));
        }
        prior = output;
    }

    let final_launch = u32::try_from(expected_launches).expect("GEA3 final launch fits u32");
    WireDeviceProgram {
        kernels,
        launches,
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: prior,
            version: WireBufferVersion {
                version: 1,
                element_ty: "f32".to_owned(),
                element_count: result_count,
                shape: None,
                reduced_projection: None,
            },
            role: WireBufferRole::Output,
            produced_by: final_launch,
            observation: WireObservationFact {
                at_launch: final_launch,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values,
        roots: vec![1],
        dependencies,
        relations: Vec::new(),
    }
}

fn admit_gea3_full_model_program(program: &WireDeviceProgram, prefill: bool) -> Result<(), String> {
    let expected = GEA3_LAYER_COUNT * GEA3_BLOCK_LAUNCHES + 3;
    if program.kernels.len() != expected || program.launches.len() != expected {
        return Err("GEA3 program launch count drifted".to_owned());
    }
    if program.dependencies.len() != expected - 1 {
        return Err("missing dependency edge".to_owned());
    }
    let buffers = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .map(|resource| resource.buffer.id)
        .collect::<BTreeSet<_>>();
    for edge in &program.dependencies {
        if edge.producer >= edge.consumer {
            return Err("dependency cycle".to_owned());
        }
        if !buffers.contains(&edge.buffer) {
            return Err(format!("undeclared intermediate {}", edge.buffer));
        }
    }
    for resource in program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
    {
        if resource.version.element_ty != "f32" || resource.version.element_count == 0 {
            return Err("dtype/shape/range drift".to_owned());
        }
    }
    if program.roots != vec![1] {
        return Err("undeclared root".to_owned());
    }
    if let WireCollectionKernelPlan::RmsNormalization(plan) = &program.kernels[1].plan {
        if plan.axis != 1 || plan.width != 960 {
            return Err("wrong kernel geometry".to_owned());
        }
    } else {
        return Err("missing RMSNorm geometry".to_owned());
    }
    validate_wire_program(program).map_err(|error| error.to_string())?;
    if program.kernels[0].entry != "embedding_gather"
        || program.kernels[expected - 2].entry != "head_rmsnorm"
        || program.kernels[expected - 1].entry != "lm_head_gemv"
    {
        return Err("GEA3 head route is incomplete".to_owned());
    }
    let expected_count = if prefill {
        GEA3_PREFILL_ROWS * GEA3_VOCAB
    } else {
        GEA3_VOCAB
    };
    let result = program
        .results
        .first()
        .ok_or("logits output is undeclared")?;
    if result.version.element_ty != "f32" || result.version.element_count != expected_count {
        return Err("wrong logits geometry".to_owned());
    }
    for layer in 0..GEA3_LAYER_COUNT {
        for weight in [
            "attn_norm",
            "attn_q",
            "attn_k",
            "attn_v",
            "attn_output",
            "ffn_norm",
            "ffn_gate",
            "ffn_up",
            "ffn_down",
        ] {
            let name = format!("blk.{layer:02}.{weight}.weight");
            if !program
                .kernels
                .iter()
                .flat_map(|kernel| kernel.resources.iter())
                .any(|resource| resource.buffer.name == name)
            {
                return Err(format!("missing per-layer weight {name}"));
            }
        }
        for cache in ["kv_k", "kv_v"] {
            let name = format!("blk.{layer:02}.{cache}");
            let resource = program
                .kernels
                .iter()
                .flat_map(|kernel| kernel.resources.iter())
                .find(|resource| resource.buffer.name == name)
                .ok_or_else(|| format!("missing per-layer KV buffer {name}"))?;
            if resource.version.element_count != GEA3_HISTORY_CAPACITY * GEA3_KV_WIDTH
                || resource.version.element_ty != "f32"
            {
                return Err(format!("wrong KV geometry {name}"));
            }
        }
    }
    Ok(())
}

#[test]
fn gea3_full_model_program_admits_complete_plan() {
    for prefill in [false, true] {
        let program = gea3_full_model_program(prefill);
        admit_gea3_full_model_program(&program, prefill)
            .expect("complete GEA3 two-program graph admits");
        assert_eq!(
            program.kernels.len(),
            GEA3_LAYER_COUNT * GEA3_BLOCK_LAUNCHES + 3
        );
        assert_eq!(program.dependencies.len(), program.kernels.len() - 1);
        assert_eq!(program.roots, vec![1]);
    }
}

#[test]
fn gea3_program_rejects_missing_edges_or_undeclared_facts() {
    let complete = gea3_full_model_program(false);
    admit_gea3_full_model_program(&complete, false).expect("complete GEA3 graph admits");

    let mut missing_edge = complete.clone();
    missing_edge.dependencies.pop();
    let error =
        admit_gea3_full_model_program(&missing_edge, false).expect_err("missing edge fails closed");
    assert!(
        error.contains("missing dependency"),
        "unexpected error: {error}"
    );

    let mut undeclared = complete.clone();
    undeclared.dependencies[0].buffer = u32::MAX;
    let error = admit_gea3_full_model_program(&undeclared, false)
        .expect_err("undeclared intermediate fails closed");
    assert!(
        error.contains("buffer") || error.contains("declared"),
        "unexpected error: {error}"
    );

    let mut cycle = complete.clone();
    cycle.dependencies[0].consumer = cycle.dependencies[0].producer;
    let error = admit_gea3_full_model_program(&cycle, false).expect_err("cycle fails closed");
    assert!(error.contains("cycle") || error.contains("itself") || error.contains("producer"));

    let mut undeclared_root = complete.clone();
    undeclared_root.roots = vec![u32::MAX];
    let error = admit_gea3_full_model_program(&undeclared_root, false)
        .expect_err("undeclared root fails closed");
    assert!(error.contains("root") || error.contains("launch"));

    let mut wrong_dtype = complete.clone();
    wrong_dtype.kernels[1].resources[0].version.element_ty = "bf16".to_owned();
    assert!(admit_gea3_full_model_program(&wrong_dtype, false).is_err());

    let mut wrong_shape = complete.clone();
    wrong_shape.kernels[1].resources[0].version.element_count = 0;
    assert!(admit_gea3_full_model_program(&wrong_shape, false).is_err());

    let mut wrong_geometry = complete;
    let WireCollectionKernelPlan::RmsNormalization(plan) = &mut wrong_geometry.kernels[1].plan
    else {
        panic!("GEA3 block RMSNorm plan is missing");
    };
    plan.axis = 0;
    assert!(admit_gea3_full_model_program(&wrong_geometry, false).is_err());
}

#[test]
fn wire_reconstruction_preserves_legacy_elementwise_library_selection() {
    // PD-29 oracle (Option A): the persisted wire entry name
    // is the legacy ABI discriminant. A wire-reconstructed Elementwise unit
    // must select the same library body it selected before the typed-tag
    // bridge; the assertion is on the selection outcome, not the tag field.
    let cases: [(&str, Option<&str>); 5] = [
        ("swiglu", Some("swiglu")),
        ("swiglu_gate_up", Some("swiglu")),
        ("residual_add", Some("residual")),
        ("sum_o_partial", Some("residual")),
        ("vector_scale", None),
    ];
    let mut kernels = Vec::new();
    let mut launches = Vec::new();
    let mut semantic_values = Vec::new();
    for (index, (entry, _)) in cases.iter().enumerate() {
        let id = u32::try_from(index + 1).expect("fixture buffer ids fit u32");
        let buffer = gea2_wire_buffer(
            id,
            format!("{entry}_io"),
            WireBufferRole::InOut,
            WireBufferLifetime::PerProgram,
        );
        gea2_add_semantic_value(&mut semantic_values, &buffer);
        kernels.push(WireKernelUnit {
            function: u32::try_from(index).expect("fixture function ids fit u32"),
            entry: (*entry).to_owned(),
            plan: WireCollectionKernelPlan::Elementwise,
            resources: vec![gea2_wire_resource(
                &buffer,
                16,
                0,
                WireResourceAccess::ReadWrite,
                WireInitializationPolicy::ZeroFill,
            )],
            launch: WireKernelLaunchPlan {
                workgroup: WireWorkgroupSize { x: 1, y: 1, z: 1 },
                dispatch_size: WireDispatchSize { x: 16, y: 1, z: 1 },
                workgroup_count: WireWorkgroupCount { x: 16, y: 1, z: 1 },
            },
        });
        launches.push(WireLaunchUnit {
            id: u32::try_from(index + 1).expect("fixture launch ids fit u32"),
            kernel_index: u32::try_from(index).expect("fixture kernel indices fit u32"),
        });
    }
    let wire = WireDeviceProgram {
        kernels,
        launches,
        lifetime: WireProgramLifetime::SingleRun,
        results: Vec::new(),
        semantic_values,
        roots: Vec::new(),
        dependencies: Vec::new(),
        relations: Vec::new(),
    };
    let program =
        crate::schema::wire_program_to_program(&wire).expect("fixture wire program reconstructs");
    let extension = radix_mir::device_program_plans::executor_plan_extension(&program);
    assert_eq!(extension.selections.len(), cases.len());
    for ((entry, expected), selection) in cases.iter().zip(extension.selections.iter()) {
        assert_eq!(
            selection.library_entry.as_deref(),
            *expected,
            "wire-reconstructed elementwise entry {entry} must select its legacy library body"
        );
    }
}
