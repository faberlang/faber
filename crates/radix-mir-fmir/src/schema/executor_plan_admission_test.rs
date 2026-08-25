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

fn gea2_entry_counts(entry: &str) -> (Vec<u64>, u64) {
    match entry {
        "rmsnorm" => (vec![7680, 960], 7680),
        "gemm_qo" => (vec![7680, 921600], 7680),
        "gemm_kv" => (vec![7680, 307200], 2560),
        "gemm_gate_up" => (vec![7680, 2457600], 20480),
        "gemm_down" => (vec![20480, 2457600], 7680),
        "rope_q" => (vec![7680, 768], 7680),
        "rope_k" => (vec![2560, 768], 2560),
        "transpose" => (vec![512], 512),
        "score_gemm" => (vec![512, 512, 64], 64),
        "causal_softmax" => (vec![64], 64),
        "context_gemm" => (vec![64, 512], 512),
        "swiglu" => (vec![20480, 20480], 20480),
        "residual_add" => (vec![7680, 7680], 7680),
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

fn gea2_block_program() -> WireDeviceProgram {
    let mut kernels = Vec::with_capacity(GEA2_ENTRY_NAMES.len());
    let mut semantic_values = Vec::new();
    let mut output_buffers = Vec::new();
    let mut next_buffer = 1_u32;

    for (kernel_index, entry) in GEA2_ENTRY_NAMES.iter().enumerate() {
        let (input_counts, output_count) = gea2_entry_counts(entry);
        let output_id = next_buffer + u32::try_from(input_counts.len()).expect("small input count");
        let output_role = if *entry == "residual_add" {
            WireBufferRole::Output
        } else {
            WireBufferRole::InOut
        };
        let output_lifetime = if *entry == "residual_add" {
            WireBufferLifetime::ObservationPoint
        } else {
            WireBufferLifetime::PerStep
        };
        let output = WireBufferIdentity {
            id: output_id,
            name: format!("{entry}_output"),
            role: output_role,
            storage: WireStorageLayout::DeviceHandle,
            lifetime: output_lifetime,
            semantic_value: output_id,
        };
        let mut resources = Vec::with_capacity(input_counts.len() + 1);
        for (binding, element_count) in input_counts.iter().copied().enumerate() {
            let id = next_buffer + u32::try_from(binding).expect("small binding");
            let buffer = WireBufferIdentity {
                id,
                name: format!("{entry}_input_{binding}"),
                role: WireBufferRole::Input,
                storage: WireStorageLayout::DeviceHandle,
                lifetime: WireBufferLifetime::PerProgram,
                semantic_value: id,
            };
            semantic_values.push(WireSemanticValue {
                id,
                name: buffer.name.clone(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: format!("gea2-host-input-{id}"),
                },
            });
            resources.push(WireDeviceResource {
                buffer,
                version: WireBufferVersion {
                    version: 1,
                    element_ty: "f32".to_owned(),
                    element_count,
                    reduced_projection: None,
                },
                binding: WireBinding {
                    group: 0,
                    binding: u32::try_from(binding).expect("small binding"),
                },
                access: WireResourceAccess::Read,
                generation: 1,
                initialization: WireInitializationPolicy::HostProvided,
            });
        }
        semantic_values.push(WireSemanticValue {
            id: output.id,
            name: output.name.clone(),
            origin: WireSemanticValueOrigin::Synthetic {
                label: format!("gea2-intermediate-{}", output.id),
            },
        });
        resources.push(WireDeviceResource {
            buffer: output.clone(),
            version: WireBufferVersion {
                version: 1,
                element_ty: "f32".to_owned(),
                element_count: output_count,
                reduced_projection: None,
            },
            binding: WireBinding {
                group: 0,
                binding: u32::try_from(input_counts.len()).expect("small binding"),
            },
            access: WireResourceAccess::Write,
            generation: 1,
            initialization: WireInitializationPolicy::KernelInitialized,
        });
        if *entry == "residual_add" {
            output_buffers.push((output, output_count));
        }
        kernels.push(WireKernelUnit {
            function: u32::try_from(kernel_index).expect("small kernel index"),
            entry: (*entry).to_owned(),
            plan: gea2_plan(entry),
            resources,
            launch: gea2_dispatch(entry),
        });
        next_buffer = output_id + 1;
    }

    let mut launches = Vec::new();
    for (kernel_index, count) in GEA2_INSTANCE_COUNTS.iter().copied().enumerate() {
        for _ in 0..count {
            launches.push(WireLaunchUnit {
                id: u32::try_from(launches.len() + 1).expect("GEA2 launch count fits u32"),
                kernel_index: u32::try_from(kernel_index).expect("small kernel index"),
            });
        }
    }
    let dependencies = launches
        .windows(2)
        .enumerate()
        .map(|(index, pair)| WireDependencyEdge {
            producer: pair[0].id,
            consumer: pair[1].id,
            buffer: kernels[index % kernels.len()]
                .resources
                .last()
                .expect("output slot")
                .buffer
                .id,
            version: 1,
        })
        .collect();
    let (final_output, final_count) = output_buffers.pop().expect("block output");
    let final_launch = launches.last().expect("launch sequence").id;
    let result_version = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: final_count,
        reduced_projection: None,
    };

    WireDeviceProgram {
        kernels,
        launches,
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: final_output,
            version: result_version,
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

fn admit_gea2_block_program(program: &WireDeviceProgram) -> Result<(), String> {
    validate_wire_program(program).map_err(|error| error.to_string())?;
    let names = program
        .kernels
        .iter()
        .map(|kernel| kernel.entry.as_str())
        .collect::<Vec<_>>();
    if names != GEA2_ENTRY_NAMES {
        return Err(format!("GEA2 entry table drifted: {names:?}"));
    }
    let expected_launches: usize = GEA2_INSTANCE_COUNTS.iter().sum();
    if program.launches.len() != expected_launches {
        return Err(format!(
            "GEA2 launch sequence has {} rows, expected {expected_launches}",
            program.launches.len()
        ));
    }
    let buffers = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .map(|resource| resource.buffer.id)
        .collect::<BTreeSet<_>>();
    for edge in &program.dependencies {
        if !buffers.contains(&edge.buffer) {
            return Err(format!(
                "dependency edge {} → {} names undeclared intermediate {}",
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
    let result = program
        .results
        .first()
        .ok_or("block output is undeclared")?;
    if result.version.element_ty != "f32" || result.version.element_count != 8 * 960 {
        return Err("block output must be declared as F32 [8,960]".to_owned());
    }
    Ok(())
}

#[test]
fn gea2_block_program_admits_complete_plan() {
    let program = gea2_block_program();
    admit_gea2_block_program(&program).expect("complete GEA2 block graph admits");
    assert_eq!(program.kernels.len(), GEA2_ENTRY_NAMES.len());
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
}
