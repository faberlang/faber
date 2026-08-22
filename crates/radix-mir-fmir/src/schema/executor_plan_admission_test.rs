use super::*;
use crate::schema::executor_plan::{
    WireAllocationFact, WireAllocationIdentity, WireBackendRequirement, WireExecutorPlan,
    WireExecutorPlanV2, WireLogicalSignature, WirePlanProvenance,
};
use crate::schema::wire::{
    WireBinding, WireBufferLifetime, WireBufferRole, WireInitializationPolicy, WireStorageLayout,
};

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
