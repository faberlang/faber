//! GI4-2 tests: the optional cadence/session section (schema), its version
//! ratchet + admission (the MD2-W1 sibling-field precedent — own ratchet,
//! no nested device-program bump for this session change), the wire-level session-facts
//! validation, and the mutation tests proving admission inspects the
//! **actually-delivered session facts** (the equivalence-gate discipline:
//! flipped cadence, altered KV layout, mutated invocation mode, mutated
//! serialized byte → fail closed with a typed diagnostic naming the violated
//! class + exact failing fact).
//!
//! Proves the GI4-2 done-when:
//!
//! - the session facts round-trip on the wire (session section + cadence +
//!   invocation mode + KV layout) through admission and the binary codec;
//! - a wire-11 image WITHOUT the section admits unchanged (single-device
//!   packages do not require the section — the MD-A15 precedent);
//! - every mutated session fact fails admission with a typed diagnostic
//!   naming the violated class + exact failing fact;
//! - the versioned ratchet is declared ([`WIRE_SESSION_SECTION_VERSION`]) and
//!   enforced (a stale version fails closed);
//! - the accepted wire versions stay untouched (the nested device-program
//!   wire 11, the outer image version 6, and the distributed-section version
//!   1 are all unchanged).

use super::{
    EXECUTOR_PLAN_SECTION_VERSION, EXECUTOR_PLAN_V2_VERSION, FMIR_TARGET_NAME, FmirBinaryImageFile,
    FmirDeviceArtifactsSection, FmirDeviceProgramSection, FmirDeviceSection, FmirDeviceSelection,
    FmirExecutorPlanSection, FmirExecutorPlanSectionV2, FmirSessionSection, FmirSessionSectionV2,
    FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, LEGACY_EXECUTOR_PLAN_SECTION_VERSION,
    LEGACY_SESSION_SECTION_VERSION, PACKAGE_MIR_ARTIFACT_VERSION, WIRE_DEVICE_PROGRAM_VERSION,
    WIRE_DISTRIBUTED_SECTION_VERSION, WIRE_KV_CACHE_SEMANTIC_CONTRACT_VERSION,
    WIRE_SESSION_SECTION_VERSION, WireBinding, WireBufferIdentity, WireBufferLifetime,
    WireBufferRole, WireBufferVersion, WireCollectionKernelPlan, WireDeviceProgram,
    WireDeviceResource, WireDispatchSize, WireExecutorPlanV2, WireInitializationPolicy,
    WireInputUpdateCadence, WireInvocationMode, WireKernelLaunchPlan, WireKernelUnit,
    WireKvCacheAttentionFamily, WireKvCacheCapacityPolicy, WireKvCacheDtype, WireKvCacheLayout,
    WireKvCacheRopePairing, WireKvCacheSemanticV2, WireKvCacheVLayout, WireKvReservePolicy,
    WireLaunchUnit, WireObservationCadence, WireObservationFact, WireProgramLifetime,
    WireResourceAccess, WireResultBuffer, WireSemanticValue, WireSemanticValueOrigin,
    WireSessionInput, WireSessionObservationCadence, WireStorageLayout, WireWorkgroupCount,
    WireWorkgroupSize, admit_device_section, admit_session_section,
};
use crate::error::FmirImageError;
use crate::{decode_binary_image, encode_binary_image};
use radix_mir::MirProgram;
use radix_types::TypeTable;

/// The single-device program fixture (the session-section substrate): a
/// minimal program that passes `validate_wire_program` (F1–F7) and declares
/// the two session-input buffers — `w` (a once-init `PerProgram`
/// host-provided input → the resident weights analog) and `t` (a
/// `PerStep` host-provided input → the per-invocation token analog) — plus
/// the observed output `o`.
fn wire_program_fixture() -> WireDeviceProgram {
    let w = WireBufferIdentity {
        id: 1,
        name: "w".to_owned(),
        role: WireBufferRole::Input,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerProgram,
        semantic_value: 1,
    };
    let t = WireBufferIdentity {
        id: 2,
        name: "t".to_owned(),
        role: WireBufferRole::Input,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerStep,
        semantic_value: 2,
    };
    let o = WireBufferIdentity {
        id: 3,
        name: "o".to_owned(),
        role: WireBufferRole::Output,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::ObservationPoint,
        semantic_value: 3,
    };
    let v_w = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: 64,
        shape: None,
        reduced_projection: None,
    };
    let v_t = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: 1,
        shape: None,
        reduced_projection: None,
    };
    let v_o = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: 32,
        shape: None,
        reduced_projection: None,
    };
    let launch = WireKernelLaunchPlan {
        workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
        dispatch_size: WireDispatchSize { x: 16, y: 1, z: 1 },
        workgroup_count: WireWorkgroupCount { x: 2, y: 1, z: 1 },
    };
    WireDeviceProgram {
        kernels: vec![WireKernelUnit {
            function: 0,
            entry: "proj".to_owned(),
            plan: WireCollectionKernelPlan::Elementwise,
            resources: vec![
                WireDeviceResource {
                    buffer: w.clone(),
                    version: v_w.clone(),
                    binding: WireBinding {
                        group: 0,
                        binding: 0,
                    },
                    access: WireResourceAccess::Read,
                    generation: 1,
                    initialization: WireInitializationPolicy::HostProvided,
                },
                WireDeviceResource {
                    buffer: t.clone(),
                    version: v_t.clone(),
                    binding: WireBinding {
                        group: 0,
                        binding: 1,
                    },
                    access: WireResourceAccess::Read,
                    generation: 1,
                    initialization: WireInitializationPolicy::HostProvided,
                },
                WireDeviceResource {
                    buffer: o.clone(),
                    version: v_o.clone(),
                    binding: WireBinding {
                        group: 0,
                        binding: 2,
                    },
                    access: WireResourceAccess::Write,
                    generation: 1,
                    initialization: WireInitializationPolicy::KernelInitialized,
                },
            ],
            launch,
        }],
        launches: vec![WireLaunchUnit {
            id: 1,
            kernel_index: 0,
        }],
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: o.clone(),
            version: v_o.clone(),
            role: WireBufferRole::Output,
            produced_by: 1,
            observation: WireObservationFact {
                at_launch: 1,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values: vec![
            WireSemanticValue {
                id: 1,
                name: "w".to_owned(),
                origin: WireSemanticValueOrigin::HostInput,
            },
            WireSemanticValue {
                id: 2,
                name: "t".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "token-input".to_owned(),
                },
            },
            WireSemanticValue {
                id: 3,
                name: "o".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "proj-output".to_owned(),
                },
            },
        ],
        roots: vec![1],
        dependencies: Vec::new(),
        relations: Vec::new(),
    }
}

/// The honest cadence/session section: the pinned-row `KvCacheLayout`
/// carriage (slots 1, ctx 8192, 32 layers, 5 KV heads, head dim 64, f32,
/// zero reserve), the two workload modes, a per-invocation observation
/// cadence, and the two declared session inputs (`w` resident, `t`
/// per-invocation).
fn honest_session_section() -> FmirSessionSection {
    FmirSessionSection {
        v: WIRE_SESSION_SECTION_VERSION,
        inputs: vec![
            WireSessionInput {
                slot: 1,
                cadence: WireInputUpdateCadence::Resident,
            },
            WireSessionInput {
                slot: 2,
                cadence: WireInputUpdateCadence::PerInvocation,
            },
        ],
        invocation_modes: vec![
            WireInvocationMode::Prefill,
            WireInvocationMode::ScalarDecode,
        ],
        observation_cadence: WireSessionObservationCadence::PerInvocation,
        kv_layout: WireKvCacheLayout {
            slots: 1,
            context_length: 8192,
            layer_count: 32,
            kv_head_count: 5,
            head_dim: 64,
            dtype: WireKvCacheDtype::F32,
            reserve_policy: WireKvReservePolicy { bytes: 0 },
        },
    }
}

/// A device section carrying the given (optional) session section.
fn device_section_with_session(session: Option<FmirSessionSection>) -> FmirDeviceSection {
    FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: WIRE_DEVICE_PROGRAM_VERSION,
            program: wire_program_fixture(),
        },
        executor_plan: None,
        selection: FmirDeviceSelection::Auto,
        artifacts: FmirDeviceArtifactsSection {
            artifact: Vec::new(),
        },
        declared_inputs: Vec::new(),
        distributed: None,
        session,
    }
}

// ---------------------------------------------------------------------------
// Round trip: build → decode → admit (done-when)
// ---------------------------------------------------------------------------

#[test]
fn session_section_round_trips_through_admission() {
    let section = honest_session_section();
    admit_session_section(&section, &wire_program_fixture())
        .expect("honest session section admits");
    // The full device-section admission (the load boundary) also admits it.
    admit_device_section(&device_section_with_session(Some(section)))
        .expect("device section with the honest session section admits");
}

#[test]
fn session_image_round_trips_through_the_binary_codec() {
    let image = FmirBinaryImageFile {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: FMIR_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: "main.fab".to_owned(),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: "0.79.0".to_owned(),
        },
        runtime: FmirTextRuntimeSection {
            requirement: vec!["host:argv".to_owned()],
        },
        sources: FmirTextSourcesSection {
            source: vec![FmirTextSourceIdentity {
                file: "main.fab".to_owned(),
                hash: "fnv64:0000000000000000".to_owned(),
            }],
        },
        cli: None,
        exit_code: Some(0),
        types: FmirTextTypesSection {
            table: TypeTable::new().snapshot(),
        },
        interner: vec!["main".to_owned()],
        program: MirProgram::new(),
        device: Some(device_section_with_session(Some(honest_session_section()))),
    };
    let bytes = encode_binary_image(&image).expect("encode binary image");
    let decoded = decode_binary_image(&bytes, "0.79.0").expect("session image decodes and admits");
    assert!(decoded.device.is_some());
    let session = decoded
        .device
        .as_ref()
        .and_then(|device| device.session.as_ref())
        .expect("the decoded image carries the session section");
    assert_eq!(session.v, WIRE_SESSION_SECTION_VERSION);
    assert_eq!(session.kv_layout.context_length, 8192);
    assert_eq!(session.invocation_modes.len(), 2);
    assert_eq!(session.inputs.len(), 2);
}

// ---------------------------------------------------------------------------
// Single-device regression (the MD-A15 precedent): no section, no change
// ---------------------------------------------------------------------------

#[test]
fn single_device_section_without_session_admits_unchanged() {
    // A wire-11 device section WITHOUT the cadence/session section admits
    // through the existing device-section admission unchanged (single-device
    // packages do not require the section — the MD-A15 precedent).
    let section = device_section_with_session(None);
    admit_device_section(&section).expect("single-device section admits");
}

// ---------------------------------------------------------------------------
// Mutation tests: admission inspects the actually-delivered session facts
// ---------------------------------------------------------------------------

/// Assert that a mutated session section fails the session admission with a
/// `SessionSectionInvalid` diagnostic whose detail names `needle` (the
/// violated class + exact failing fact) and also fails the full
/// device-section load boundary.
fn assert_session_fails_closed(section: &FmirSessionSection, needle: &str) {
    match admit_session_section(section, &wire_program_fixture()) {
        Err(FmirImageError::SessionSectionInvalid { detail }) => {
            assert!(
                detail.contains(needle),
                "the fail-closed diagnostic must name `{needle}`, got: {detail}"
            );
        }
        other => panic!("expected SessionSectionInvalid naming `{needle}`, got {other:?}"),
    }
    assert!(matches!(
        admit_device_section(&device_section_with_session(Some(section.clone()))),
        Err(FmirImageError::SessionSectionInvalid { .. })
    ));
}

#[test]
fn flipped_resident_cadence_fails_admission() {
    // Flip the once-init `w` input's cadence Resident → PerInvocation: the
    // admission inspects the delivered cadence against the carried program's
    // buffer facts and fails — a per-invocation cadence on a once-init
    // per-program host-provided buffer is the SingleRun re-copy semantic.
    let mut section = honest_session_section();
    section.inputs[0].cadence = WireInputUpdateCadence::PerInvocation;
    assert_session_fails_closed(&section, "once-init");
}

#[test]
fn flipped_per_invocation_cadence_fails_admission() {
    // Flip the per-invocation `t` input's cadence PerInvocation → Resident:
    // a resident cadence names a PerProgram buffer, and `t` is PerStep.
    let mut section = honest_session_section();
    section.inputs[1].cadence = WireInputUpdateCadence::Resident;
    assert_session_fails_closed(&section, "per-program");
}

#[test]
fn altered_kv_layout_dimension_fails_admission() {
    // Zero a KV layout dimension: the delivered layout is not a layout.
    let mut section = honest_session_section();
    section.kv_layout.context_length = 0;
    assert_session_fails_closed(&section, "context_length");
}

#[test]
fn mutated_invocation_mode_fails_admission() {
    // Empty the invocation-mode set: a session that executes no workload
    // mode is a contradiction.
    let mut section = honest_session_section();
    section.invocation_modes = Vec::new();
    assert_session_fails_closed(&section, "no invocation mode");

    // A duplicated workload mode is equally a contradiction.
    let mut section = honest_session_section();
    section.invocation_modes = vec![WireInvocationMode::Prefill, WireInvocationMode::Prefill];
    assert_session_fails_closed(&section, "more than once");
}

#[test]
fn mutated_observation_cadence_fails_admission() {
    // A scalar-decode session observes per invocation (the per-token logits
    // readback); a per-session observation cadence contradicts it.
    let mut section = honest_session_section();
    section.observation_cadence = WireSessionObservationCadence::PerSession;
    assert_session_fails_closed(&section, "observation cadence");
}

#[test]
fn duplicate_session_input_slot_fails_admission() {
    let mut section = honest_session_section();
    section.inputs.push(WireSessionInput {
        slot: 1,
        cadence: WireInputUpdateCadence::PerInvocation,
    });
    assert_session_fails_closed(&section, "more than once");
}

#[test]
fn unresolved_session_input_slot_fails_admission() {
    let mut section = honest_session_section();
    section.inputs.push(WireSessionInput {
        slot: 99,
        cadence: WireInputUpdateCadence::Resident,
    });
    assert_session_fails_closed(&section, "slot 99");
}

#[test]
fn mutated_byte_in_the_serialized_session_section_fails_admission() {
    // Change a byte in the serialized section's first session-input cadence
    // field (Resident → PerInvocation). The section still decodes (the byte
    // is a well-formed enum tag), and admission inspects the delivered
    // cadence against the carried program and fails closed — the equivalence
    // gate inspects the delivered facts, never a self-declared hash.
    let section = honest_session_section();
    let bytes = postcard::to_allocvec(&section).expect("section serializes");
    // Layout: [v=2][inputs len][slot 1][cadence 0][slot 2][cadence 1]…
    assert_eq!(
        &bytes[..6],
        &[0x02, 0x02, 0x01, 0x00, 0x02, 0x01],
        "the honest section's version/input prefix is the expected encoding"
    );
    let mut mutated = bytes.clone();
    mutated[3] = 0x01; // the first input's cadence tag 0 → 1
    assert_ne!(mutated, bytes);

    let decoded: FmirSessionSection =
        postcard::from_bytes(&mutated).expect("mutated section still decodes");
    assert_session_fails_closed(&decoded, "once-init");
}

#[test]
fn mutated_byte_in_the_serialized_version_fails_closed() {
    // Change the section's version byte (2 → 1): the versioned ratchet fails
    // closed BEFORE any field interpretation.
    let section = honest_session_section();
    let bytes = postcard::to_allocvec(&section).expect("section serializes");
    let mut mutated = bytes.clone();
    assert_eq!(mutated[0], 0x02, "the version varint is 2");
    mutated[0] = 0x01;
    let decoded: FmirSessionSection =
        postcard::from_bytes(&mutated).expect("mutated section still decodes");
    assert!(matches!(
        admit_session_section(&decoded, &wire_program_fixture()),
        Err(FmirImageError::SessionSectionVersionMismatch { actual, expected })
            if actual == 1 && expected == WIRE_SESSION_SECTION_VERSION
    ));
}

// ---------------------------------------------------------------------------
// A4V: explicit v2 carriers and legacy v1 isolation
// ---------------------------------------------------------------------------

fn semantic_v2_fixture() -> WireKvCacheSemanticV2 {
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

fn runtime_bindings_v2_fixture() -> WireExecutorPlanV2 {
    WireExecutorPlanV2 {
        abi_version: EXECUTOR_PLAN_V2_VERSION,
        selections: Vec::new(),
        allocations: Vec::new(),
        views: Vec::new(),
        launch_bindings: Vec::new(),
    }
}

#[test]
fn v2_semantic_and_runtime_payloads_round_trip_through_explicit_carriers() {
    let semantic = FmirSessionSectionV2::from_semantic(
        vec![
            WireSessionInput {
                slot: 1,
                cadence: WireInputUpdateCadence::Resident,
            },
            WireSessionInput {
                slot: 2,
                cadence: WireInputUpdateCadence::PerInvocation,
            },
        ],
        vec![
            WireInvocationMode::Prefill,
            WireInvocationMode::ScalarDecode,
        ],
        WireSessionObservationCadence::PerInvocation,
        semantic_v2_fixture(),
    );
    let semantic_bytes = postcard::to_allocvec(&semantic).expect("semantic carrier serializes");
    let decoded_semantic: FmirSessionSectionV2 =
        postcard::from_bytes(&semantic_bytes).expect("semantic carrier decodes");
    assert_eq!(decoded_semantic, semantic);
    assert!(decoded_semantic.has_supported_version());
    assert_eq!(
        decoded_semantic.semantic.semantic_contract_version,
        WIRE_KV_CACHE_SEMANTIC_CONTRACT_VERSION
    );

    let runtime = FmirExecutorPlanSectionV2::from_runtime_bindings(runtime_bindings_v2_fixture());
    let runtime_bytes = postcard::to_allocvec(&runtime).expect("runtime carrier serializes");
    let decoded_runtime: FmirExecutorPlanSectionV2 =
        postcard::from_bytes(&runtime_bytes).expect("runtime carrier decodes");
    assert_eq!(decoded_runtime, runtime);
    assert!(decoded_runtime.has_supported_version());
    assert_eq!(
        decoded_runtime.runtime_bindings.abi_version,
        EXECUTOR_PLAN_V2_VERSION
    );
}

#[test]
fn v1_static_executor_plan_remains_a_v1_carrier_only() {
    let legacy = FmirExecutorPlanSection {
        v: LEGACY_EXECUTOR_PLAN_SECTION_VERSION,
        plans: Vec::new(),
    };
    let bytes = postcard::to_allocvec(&legacy).expect("legacy static plan serializes");
    let decoded: FmirExecutorPlanSection =
        postcard::from_bytes(&bytes).expect("legacy static plan decodes");
    assert_eq!(decoded, legacy);
    assert_eq!(decoded.v, 1);
    assert_ne!(decoded.v, EXECUTOR_PLAN_SECTION_VERSION);
}

#[test]
fn v1_v2_carrier_skew_is_not_default_filled() {
    let mut semantic = FmirSessionSectionV2::from_semantic(
        Vec::new(),
        vec![WireInvocationMode::Prefill],
        WireSessionObservationCadence::PerSession,
        semantic_v2_fixture(),
    );
    semantic.v = LEGACY_SESSION_SECTION_VERSION;
    assert!(!semantic.has_supported_version());

    let mut runtime =
        FmirExecutorPlanSectionV2::from_runtime_bindings(runtime_bindings_v2_fixture());
    runtime.v = LEGACY_EXECUTOR_PLAN_SECTION_VERSION;
    assert!(!runtime.has_supported_version());
    assert_eq!(
        runtime.runtime_bindings.abi_version,
        EXECUTOR_PLAN_V2_VERSION
    );
}

// ---------------------------------------------------------------------------
// A7: the section's own version ratchet fails closed
// ---------------------------------------------------------------------------

#[test]
fn stale_session_section_version_fails_closed() {
    for stale in [
        WIRE_SESSION_SECTION_VERSION - 1,
        WIRE_SESSION_SECTION_VERSION + 1,
    ] {
        let mut section = honest_session_section();
        section.v = stale;
        match admit_session_section(&section, &wire_program_fixture()) {
            Err(FmirImageError::SessionSectionVersionMismatch { actual, expected }) => {
                assert_eq!(actual, stale);
                assert_eq!(expected, WIRE_SESSION_SECTION_VERSION);
            }
            other => {
                panic!("expected UnsupportedSessionSectionVersion for {stale}, got {other:?}")
            }
        }
        // The stale section also fails the full device-section load boundary.
        assert!(matches!(
            admit_device_section(&device_section_with_session(Some(section))),
            Err(FmirImageError::SessionSectionVersionMismatch { .. })
        ));
    }
}

// ---------------------------------------------------------------------------
// The versioned ratchet + the accepted wire versions stay untouched
// ---------------------------------------------------------------------------

#[test]
fn session_section_version_ratchet_is_declared() {
    // The session surface carries its OWN version ratchet (the MD2-W1
    // sibling-field precedent): the v2 semantic carrier is explicit and
    // the accepted wire versions are untouched.
    assert_eq!(WIRE_SESSION_SECTION_VERSION, 2);
    assert_eq!(WIRE_KV_CACHE_SEMANTIC_CONTRACT_VERSION, 2);
    assert_eq!(EXECUTOR_PLAN_SECTION_VERSION, 2);
    assert_eq!(EXECUTOR_PLAN_V2_VERSION, 2);
    assert_eq!(LEGACY_EXECUTOR_PLAN_SECTION_VERSION, 1);
    assert_eq!(WIRE_DEVICE_PROGRAM_VERSION, 14);
    assert_eq!(PACKAGE_MIR_ARTIFACT_VERSION, 8);
    assert_eq!(WIRE_DISTRIBUTED_SECTION_VERSION, 1);
}
