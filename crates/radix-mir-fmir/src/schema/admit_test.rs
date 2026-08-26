use super::super::device::{
    FmirExecutorPlanSectionV2, FmirSessionSectionV2, EXECUTOR_PLAN_SECTION_VERSION,
    WIRE_SESSION_SECTION_VERSION,
};
use super::super::executor_plan::{
    WireAllocationFact, WireAllocationIdentity, WireBindDescriptor, WireExecutorPlan,
    WireExecutorPlanSource, WireExecutorPlanV2, WireLaunchBinding, WireLogicalSignature,
    WirePlanProvenance, WireRuntimeSource, WireViewFact,
};
use super::super::wire::{
    WireBinding, WireBufferLifetime, WireBufferRole, WireCollectionKernelPlan, WireDeviceProgram,
    WireInitializationPolicy, WireInvocationMode, WireKernelLaunchPlan, WireKernelUnit,
    WireKvCacheAttentionFamily, WireKvCacheCapacityPolicy, WireKvCacheDtype,
    WireKvCacheRopePairing, WireKvCacheSemanticV2, WireKvCacheVLayout,
    WireSessionObservationCadence, WireStorageLayout, WIRE_KV_CACHE_SEMANTIC_CONTRACT_VERSION,
};
use super::admit_session_section_v2;
use crate::error::FmirImageError;

fn semantic() -> WireKvCacheSemanticV2 {
    WireKvCacheSemanticV2 {
        semantic_contract_version: WIRE_KV_CACHE_SEMANTIC_CONTRACT_VERSION,
        kv_dtype_k: WireKvCacheDtype::F32,
        kv_dtype_v: WireKvCacheDtype::F32,
        v_layout: WireKvCacheVLayout::Straight,
        attention_family: WireKvCacheAttentionFamily::Flash,
        query_head_count: 16,
        kv_head_count: 4,
        head_dim: 64,
        layer_count: 32,
        context_capacity: 8192,
        capacity_policy: WireKvCacheCapacityPolicy::Fixed,
        rope_dimension: 64,
        rope_frequency_base: 100_000.0,
        rope_frequency_scale: 1.0,
        rope_pairing: WireKvCacheRopePairing::RotateHalf,
        k_stored_post_rope: true,
    }
}

fn session() -> FmirSessionSectionV2 {
    FmirSessionSectionV2::from_semantic(
        Vec::new(),
        vec![
            WireInvocationMode::Prefill,
            WireInvocationMode::ScalarDecode,
        ],
        WireSessionObservationCadence::PerInvocation,
        semantic(),
    )
}

fn wire_program() -> WireDeviceProgram {
    WireDeviceProgram {
        kernels: vec![WireKernelUnit {
            function: 0,
            entry: "kv_append".to_owned(),
            plan: WireCollectionKernelPlan::Elementwise,
            resources: Vec::new(),
            launch: WireKernelLaunchPlan {
                workgroup: super::super::wire::WireWorkgroupSize { x: 1, y: 1, z: 1 },
                dispatch_size: super::super::wire::WireDispatchSize { x: 1, y: 1, z: 1 },
                workgroup_count: super::super::wire::WireWorkgroupCount { x: 1, y: 1, z: 1 },
            },
        }],
        ..WireDeviceProgram::default()
    }
}

fn runtime() -> FmirExecutorPlanSectionV2 {
    FmirExecutorPlanSectionV2::from_runtime_bindings(WireExecutorPlanV2 {
        abi_version: super::super::executor_plan::EXECUTOR_PLAN_V2_VERSION,
        selections: vec![WireExecutorPlan {
            kernel_index: 0,
            library_entry: Some("kv_append".to_owned()),
            logical_signature: WireLogicalSignature {
                operation: "kv_append".to_owned(),
                inputs: vec!["k".to_owned()],
                outputs: vec!["cache_k".to_owned()],
            },
            primary_bind: WireBindDescriptor {
                dims: vec![1],
                strides: vec![1],
                layout: "strided".to_owned(),
                grid: [1, 1, 1],
            },
            binds: Vec::new(),
            uniforms: Vec::new(),
            offsets: Vec::new(),
            workspace: None,
            backend_requirements: Vec::new(),
            provenance: WirePlanProvenance {
                pass: "kv-cache-plan".to_owned(),
                revision: 2,
                source_nodes: vec![1],
            },
        }],
        allocations: vec![WireAllocationFact {
            buffer: WireAllocationIdentity {
                id: 1,
                name: "cache_k".to_owned(),
                role: WireBufferRole::InOut,
                storage: WireStorageLayout::DeviceHandle,
                lifetime: WireBufferLifetime::PerProgram,
            },
            dtype: "F32".to_owned(),
            capacity_bytes: 64,
            lifetime: WireBufferLifetime::PerProgram,
            initialization: WireInitializationPolicy::ZeroFill,
        }],
        views: vec![WireViewFact {
            allocation: 1,
            logical_dims: vec![1],
            strides: vec![1],
            static_base: 0,
            maximum_span: 4,
        }],
        launch_bindings: vec![WireLaunchBinding {
            kernel_index: 0,
            handle: 1,
            binding: WireBinding {
                group: 0,
                binding: 0,
            },
            byte_offset: 0,
            view_span: 4,
            runtime_source: WireExecutorPlanSource::Runtime {
                source: WireRuntimeSource::Position,
            },
        }],
    })
}

fn admit(
    section: &FmirSessionSectionV2,
    executor_plan: &FmirExecutorPlanSectionV2,
) -> Result<(), FmirImageError> {
    admit_session_section_v2(section, executor_plan, &wire_program())
}

#[test]
fn v2_semantic_and_runtime_sections_admit_together() {
    admit(&session(), &runtime()).expect("the complete v2 KV boundary admits");
}

#[test]
fn gqa_ratio_fails_before_host_construction() {
    let mut section = session();
    section.semantic.query_head_count = 15;
    let error = admit(&section, &runtime()).expect_err("non-divisible GQA must fail");
    assert!(
        matches!(error, FmirImageError::SessionSectionInvalid { detail } if detail.contains("not divisible"))
    );
}

#[test]
fn semantic_bounds_and_rope_pairing_fail_closed() {
    let mut section = session();
    section.semantic.context_capacity = 0;
    let error = admit(&section, &runtime()).expect_err("zero capacity must fail");
    assert!(
        matches!(error, FmirImageError::SessionSectionInvalid { detail } if detail.contains("context_capacity"))
    );

    let mut section = session();
    section.semantic.rope_dimension = 63;
    let error = admit(&section, &runtime()).expect_err("odd paired RoPE dimension must fail");
    assert!(
        matches!(error, FmirImageError::SessionSectionInvalid { detail } if detail.contains("even"))
    );

    let mut section = session();
    section.semantic.rope_dimension = 66;
    let error = admit(&section, &runtime()).expect_err("out-of-head RoPE dimension must fail");
    assert!(
        matches!(error, FmirImageError::SessionSectionInvalid { detail } if detail.contains("larger than head_dim"))
    );
}

#[test]
fn semantic_dimensions_must_be_positive() {
    type SemanticCase = (
        &'static str,
        for<'a> fn(&'a mut super::super::wire::WireKvCacheSemanticV2),
    );
    let cases: [SemanticCase; 5] = [
        ("query_head_count", |semantic| semantic.query_head_count = 0),
        ("kv_head_count", |semantic| semantic.kv_head_count = 0),
        ("head_dim", |semantic| semantic.head_dim = 0),
        ("layer_count", |semantic| semantic.layer_count = 0),
        ("context_capacity", |semantic| semantic.context_capacity = 0),
    ];
    for (name, mutate) in cases {
        let mut section = session();
        mutate(&mut section.semantic);
        let error = admit(&section, &runtime()).expect_err("zero semantic fact must fail");
        assert!(
            matches!(error, FmirImageError::SessionSectionInvalid { ref detail } if detail.contains(name)),
            "diagnostic must name {name}: {error}"
        );
    }
}

#[test]
fn semantic_version_storage_rule_and_modes_fail_closed() {
    let mut section = session();
    section.semantic.semantic_contract_version = 1;
    let error = admit(&section, &runtime()).expect_err("semantic version skew must fail");
    assert!(
        matches!(error, FmirImageError::SessionSectionInvalid { detail } if detail.contains("contract version"))
    );

    let mut section = session();
    section.semantic.k_stored_post_rope = false;
    let error = admit(&section, &runtime()).expect_err("pre-RoPE K storage must fail");
    assert!(
        matches!(error, FmirImageError::SessionSectionInvalid { detail } if detail.contains("before RoPE"))
    );

    let mut section = session();
    section.invocation_modes.clear();
    let error = admit(&section, &runtime()).expect_err("an empty mode set must fail");
    assert!(
        matches!(error, FmirImageError::SessionSectionInvalid { detail } if detail.contains("no invocation mode"))
    );
}

#[test]
fn either_v2_section_version_skew_fails_closed() {
    let mut section = session();
    section.v = WIRE_SESSION_SECTION_VERSION - 1;
    assert!(matches!(
        admit(&section, &runtime()),
        Err(FmirImageError::SessionSectionVersionMismatch {
            actual: 1,
            expected: 2
        })
    ));

    let mut runtime = runtime();
    runtime.v = EXECUTOR_PLAN_SECTION_VERSION - 1;
    assert!(matches!(
        admit(&session(), &runtime),
        Err(FmirImageError::ExecutorPlanSectionVersionMismatch {
            actual: 1,
            expected: 2
        })
    ));
}

#[test]
fn b2_runtime_source_and_overflow_diagnostics_are_preserved() {
    let mut unknown_runtime = runtime();
    unknown_runtime.runtime_bindings.launch_bindings[0].runtime_source =
        WireExecutorPlanSource::Runtime {
            source: WireRuntimeSource::Unknown("future_cursor".to_owned()),
        };
    let error = admit(&session(), &unknown_runtime).expect_err("unknown runtime source must fail");
    assert!(
        matches!(error, FmirImageError::ExecutorPlanSectionInvalid { detail } if detail.contains("future_cursor"))
    );

    let mut overflow_runtime = runtime();
    overflow_runtime.runtime_bindings.launch_bindings[0].byte_offset = u64::MAX - 1;
    overflow_runtime.runtime_bindings.launch_bindings[0].view_span = 2;
    let error =
        admit(&session(), &overflow_runtime).expect_err("overflowing runtime span must fail");
    assert!(
        matches!(error, FmirImageError::ExecutorPlanSectionInvalid { detail } if detail.contains("arithmetic overflow"))
    );

    let mut skewed_runtime = runtime();
    skewed_runtime.runtime_bindings.abi_version = 1;
    let error =
        admit(&session(), &skewed_runtime).expect_err("runtime payload version skew must fail");
    assert!(
        matches!(error, FmirImageError::ExecutorPlanSectionInvalid { detail } if detail.contains("payload version"))
    );
}
