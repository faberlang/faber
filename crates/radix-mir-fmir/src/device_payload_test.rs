//! Device payload (N1.7) round-trip and fail-closed admission tests.
//!
//! Implements spec **Checkpoint A** (S1-2 end): FMIR images with a device
//! payload round-trip through both codecs and fail closed on wrong
//! version/target, unknown `device:` requirements, and artifact-hash
//! mismatch. Also covers `deny_unknown_fields` on the new section and the
//! no-regression case (`device: None`).
//!
//! These are the additive device tests; the schema/codec inline modules still
//! own the pre-existing surface tests (updated only for the 3→4 break).

use crate::codec::{
    decode_binary_image, decode_text_image, encode_binary_image, encode_text_image,
};
use crate::error::FmirImageError;
use crate::schema::{
    ARTIFACT_ABI_VERSION, DeviceArtifactFormat, DevicePayloadEncoding, DeviceTargetId,
    EXECUTOR_PLAN_SECTION_VERSION, FMIR_TARGET_NAME, FMIR_TEXT_TARGET_NAME, FmirArtifactReflection,
    FmirBinaryImageFile, FmirDeviceArtifact, FmirDeviceArtifactsSection, FmirDeviceBackend,
    FmirDeviceInput, FmirDeviceProgramSection, FmirDeviceSection, FmirDeviceSelection,
    FmirDeviceSymbol, FmirExecutorPlanSection, FmirExecutorPlanSectionV2, FmirReflectedKernel,
    FmirReflectedResource, FmirTextCliRootSection, FmirTextCliSection, FmirTextImageFile,
    FmirTextProgramSection, FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, LEGACY_EXECUTOR_PLAN_SECTION_VERSION,
    MaterializationStage, PACKAGE_MIR_ARTIFACT_VERSION, WIRE_DEVICE_PROGRAM_VERSION,
    WireAxisReductionPlan, WireBarrierPhase, WireBarrierPoint, WireBatchedMatMulPlan,
    WireBatchedRowSoftmaxPlan, WireBatchedTransposePlan, WireBinding, WireBroadcastDeclaration,
    WireBroadcastFact, WireBufferIdentity, WireBufferLifetime, WireBufferRole, WireBufferVersion,
    WireCausalMaskedSoftmaxPlan, WireCollectionKernelPlan, WireCompanionDerivativeKind,
    WireCompanionRelation, WireCompanionSelectedInput, WireCompanionSelectedOutput,
    WireDependencyEdge, WireDeviceProgram, WireDeviceResource, WireDispatchSize, WireGatherPlan,
    WireGroupedMatMulPlan, WireInitializationPolicy, WireKernelLaunchPlan, WireKernelUnit,
    WireLaunchUnit, WireLayerNormalizationPlan, WireMatMulLayout, WireMatMulSharedMemory,
    WireObservationCadence, WireObservationFact, WireOobPaddingPolicy, WirePackedStorageLayout,
    WireProgramLifetime, WireQuantizedMatMulPlan, WireReduceOp, WireReducedProjection,
    WireResourceAccess, WireResultBuffer, WireRmsNormalizationPlan, WireRopePlan,
    WireRowSoftmaxPlan, WireSemanticValue, WireSemanticValueOrigin, WireSharedMemoryLayout,
    WireSsmConv1dPlan, WireSsmScanPlan, WireStorageLayout, WireWorkgroupCount, WireWorkgroupSize,
    admit_executor_plan_section_v2_with_version, canonical_program_bytes,
    canonical_unpadded_base64, device_identity_hash, packet_sha256_canonical, sha256_digest,
};
use radix_mir::MirProgram;
use radix_types::TypeTable;

const TOOLCHAIN: &str = "0.79.0";

/// The faber-owned wire/codec version (`DEVICE_RUN_PLAN_VERSION` bump, Stage 3R
/// U1 S1): a pre-break v3 payload must fail the nested admission closed.
const WIRE_VERSION: u32 = WIRE_DEVICE_PROGRAM_VERSION;

/// The typed wire fixture: a one-kernel elementwise program (two buffers, one
/// launch, one result) mirroring the S2-5 `addita` shape, now carrying the
/// F1–F7 semantic facts (semantic values, generations, root, axes).
fn sample_wire_program() -> WireDeviceProgram {
    let input = WireBufferIdentity {
        id: 1,
        name: "x".to_owned(),
        role: WireBufferRole::Input,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerProgram,
        semantic_value: 1,
    };
    let output = WireBufferIdentity {
        id: 2,
        name: "y".to_owned(),
        role: WireBufferRole::Output,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::ObservationPoint,
        semantic_value: 2,
    };
    WireDeviceProgram {
        kernels: vec![WireKernelUnit {
            function: 0,
            entry: "addita".to_owned(),
            plan: WireCollectionKernelPlan::Elementwise,
            resources: vec![
                WireDeviceResource {
                    buffer: input.clone(),
                    version: WireBufferVersion {
                        version: 1,
                        element_ty: "f32".to_owned(),
                        element_count: 4,
                        shape: None,
                        reduced_projection: None,
                    },
                    binding: WireBinding {
                        group: 0,
                        binding: 0,
                    },
                    access: WireResourceAccess::Read,
                    generation: 1,
                    initialization: WireInitializationPolicy::HostProvided,
                },
                WireDeviceResource {
                    buffer: output.clone(),
                    version: WireBufferVersion {
                        version: 1,
                        element_ty: "f32".to_owned(),
                        element_count: 4,
                        shape: None,
                        reduced_projection: None,
                    },
                    binding: WireBinding {
                        group: 0,
                        binding: 1,
                    },
                    access: WireResourceAccess::Write,
                    generation: 1,
                    initialization: WireInitializationPolicy::KernelInitialized,
                },
            ],
            launch: WireKernelLaunchPlan {
                workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
                dispatch_size: WireDispatchSize { x: 4, y: 1, z: 1 },
                workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
            },
        }],
        launches: vec![WireLaunchUnit {
            id: 1,
            kernel_index: 0,
        }],
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: output,
            version: WireBufferVersion {
                version: 1,
                element_ty: "f32".to_owned(),
                element_count: 4,
                shape: None,
                reduced_projection: None,
            },
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
                name: "x".to_owned(),
                origin: WireSemanticValueOrigin::HostInput,
            },
            WireSemanticValue {
                id: 2,
                name: "y".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "addita-output".to_owned(),
                },
            },
        ],
        roots: vec![1],
        dependencies: Vec::new(),
        relations: Vec::new(),
    }
}

/// Compute the canonical digests of a test artifact (DDCP2-U3: the packet
/// carries the REAL content/packet digests, computed and re-verified at
/// admission).
fn with_computed_digests(mut artifact: FmirDeviceArtifact) -> FmirDeviceArtifact {
    artifact.content_sha256 = artifact.compute_content_sha256();
    artifact.packet_sha256 = artifact.compute_packet_sha256();
    artifact
}

fn metal_artifact(blob: &str) -> FmirDeviceArtifact {
    with_computed_digests(FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        format: DeviceArtifactFormat::Msl,
        stage: MaterializationStage::CompilerInput,
        target: DeviceTargetId {
            id: "macos-arm64".to_owned(),
            required_features: Vec::new(),
        },
        abi_version: ARTIFACT_ABI_VERSION,
        bytes: blob.as_bytes().to_vec(),
        encoding: DevicePayloadEncoding::Text,
        entrypoints: Vec::new(),
        content_sha256: String::new(),
        packet_sha256: String::new(),
        compiler_input_packet_sha256: None,
        reflection: None,
    })
}

fn cuda_artifact(blob: &str) -> FmirDeviceArtifact {
    with_computed_digests(FmirDeviceArtifact {
        backend: FmirDeviceBackend::Cuda,
        format: DeviceArtifactFormat::Ptx,
        stage: MaterializationStage::CompilerInput,
        target: DeviceTargetId {
            id: "sm_120".to_owned(),
            required_features: vec!["sm_120".to_owned()],
        },
        abi_version: ARTIFACT_ABI_VERSION,
        bytes: blob.as_bytes().to_vec(),
        encoding: DevicePayloadEncoding::Text,
        entrypoints: vec![FmirDeviceSymbol {
            entry: "addita".to_owned(),
            symbol: "_Z6additaPU3AS1fS0_".to_owned(),
        }],
        content_sha256: String::new(),
        packet_sha256: String::new(),
        compiler_input_packet_sha256: None,
        reflection: None,
    })
}

/// A device section with both backends declared, a declared input, and the
/// typed complete program. Each artifact carries the computed content/packet
/// digests; the typed target features replace the old `runtime_requirements`
/// string allowlist.
fn sample_device_section() -> FmirDeviceSection {
    FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: WIRE_VERSION,
            program: sample_wire_program(),
        },
        executor_plan: None,
        selection: FmirDeviceSelection::Auto,
        artifacts: FmirDeviceArtifactsSection {
            artifact: vec![
                metal_artifact("kernel void add(device float* a [[buffer(0)]]) {}"),
                cuda_artifact(".version 8.7\n.target sm_120\n.entry add() {}"),
            ],
        },
        declared_inputs: vec![FmirDeviceInput {
            name: "x".to_owned(),
            values: vec![1.0, 2.0, 3.0, 4.0],
        }],
        // MD2-W1: absent for single-device packages (MD-A15) — the
        // independent single-device path.
        distributed: None,
        // GI4-2: absent for packages without an inference-session surface
        // (single-device packages do not require the section — the MD-A15
        // precedent).
        session: None,
    }
}

#[test]
fn executor_plan_section_version_skew_fails_closed_before_field_admission() {
    let mut section = sample_device_section();
    section.executor_plan = Some(FmirExecutorPlanSection {
        v: 3,
        plans: Vec::new(),
    });
    let error = crate::schema::admit_device_section_with_compat(
        &section,
        &crate::schema::default_compatibility_table(),
    )
    .expect_err("executor-plan version skew must fail closed");
    assert!(matches!(
        error,
        FmirImageError::ExecutorPlanSectionVersionMismatch {
            actual: 3,
            expected: 2
        }
    ));
}

fn static_executor_plan() -> radix_mir::ExecutorPlanExtension {
    radix_mir::ExecutorPlanExtension {
        abi_version: 1,
        selections: vec![radix_mir::LibrarySelection {
            kernel_index: 0,
            library_entry: Some("addita".to_owned()),
            logical_signature: radix_mir::LogicalSignature {
                operation: "addita".to_owned(),
                inputs: vec!["x".to_owned()],
                outputs: vec!["y".to_owned()],
            },
            bind: radix_mir::BindDescriptor {
                dims: vec![4],
                strides: vec![1],
                layout: radix_mir::BindLayout::Flat,
                grid: radix_mir::MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
            },
            binds: Vec::new(),
            uniforms: Vec::new(),
            offsets: Vec::new(),
            workspace: None,
            backend_requirements: Vec::new(),
            provenance: radix_mir::PlanProvenance {
                pass: "device-program-plan".to_owned(),
                revision: 1,
                source_nodes: vec![0],
            },
        }],
    }
}

fn runtime_executor_plan_v2() -> radix_mir::device_program_plans::ExecutorPlanExtensionV2 {
    use radix_mir::device_program_plans::{
        AllocationFact, EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION, ExecutorPlanExtensionV2,
        ExecutorPlanSource, LaunchBinding, RuntimeSource, ViewFact,
    };
    ExecutorPlanExtensionV2 {
        abi_version: EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION,
        selections: static_executor_plan().selections,
        allocations: vec![AllocationFact {
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
        views: vec![ViewFact {
            allocation: radix_mir::BufferId(7),
            logical_dims: vec![4],
            strides: vec![1],
            static_base: 0,
            maximum_span: 16,
        }],
        launch_bindings: vec![LaunchBinding::new(
            0,
            radix_mir::BufferId(7),
            radix_mir::Binding {
                group: 0,
                binding: 3,
            },
            0,
            16,
            ExecutorPlanSource::runtime(RuntimeSource::Position),
        )],
    }
}

#[test]
fn producer_executor_plan_round_trips_through_binary_image() {
    let mut section = sample_device_section();
    let produced = FmirExecutorPlanSection::from_executor_plan(&static_executor_plan());
    assert_eq!(produced.v, EXECUTOR_PLAN_SECTION_VERSION);
    assert!(produced.has_supported_version());
    section.executor_plan = Some(produced.clone());

    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    let decoded = decode_binary_image(&bytes, TOOLCHAIN)
        .expect("a produced executor-plan section must admit and decode");
    let plan = decoded
        .device
        .expect("device section present")
        .executor_plan
        .expect("executor-plan section present");
    assert_eq!(plan, produced);
    assert_eq!(plan.v, EXECUTOR_PLAN_SECTION_VERSION);
    assert_eq!(plan.plans[0].logical_signature.operation, "addita");
}

#[test]
fn producer_runtime_bindings_admit_and_decode_at_v2() {
    let produced = FmirExecutorPlanSectionV2::from_executor_plan(&runtime_executor_plan_v2());
    assert_eq!(produced.v, EXECUTOR_PLAN_SECTION_VERSION);
    assert!(produced.has_supported_version());

    let bytes = postcard::to_allocvec(&produced).expect("v2 producer payload serializes");
    let decoded: FmirExecutorPlanSectionV2 =
        postcard::from_bytes(&bytes).expect("v2 producer payload decodes");
    assert_eq!(decoded, produced);
    admit_executor_plan_section_v2_with_version(decoded.v, &decoded.runtime_bindings, 1)
        .expect("a produced v2 runtime-binding section admits");
}

#[test]
fn producer_static_v2_plan_stays_on_legacy_version() {
    let mut plan = runtime_executor_plan_v2();
    plan.launch_bindings[0].runtime_source =
        radix_mir::device_program_plans::ExecutorPlanSource::constant(
            radix_mir::UniformType::U32,
            7,
        );
    let produced = FmirExecutorPlanSectionV2::from_executor_plan(&plan);
    assert_eq!(produced.v, LEGACY_EXECUTOR_PLAN_SECTION_VERSION);
    assert!(!produced.has_supported_version());
    assert!(matches!(
        admit_executor_plan_section_v2_with_version(produced.v, &produced.runtime_bindings, 1),
        Err(
            crate::schema::ExecutorPlanAdmissionError::UnsupportedSectionVersion {
                actual: 1,
                expected: 2
            }
        )
    ));
}

fn text_image_with_device(device: Option<FmirDeviceSection>) -> FmirTextImageFile {
    FmirTextImageFile {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: FMIR_TEXT_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: "main.fab".to_owned(),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: TOOLCHAIN.to_owned(),
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
        cli: Some(FmirTextCliSection {
            root: FmirTextCliRootSection {
                record: "run_entry".to_owned(),
                operand: Vec::new(),
            },
        }),
        exit_code: Some(0),
        types: FmirTextTypesSection {
            table: TypeTable::new().snapshot(),
        },
        interner: vec!["main".to_owned()],
        program: FmirTextProgramSection {
            json: "{}".to_owned(),
        },
        device,
    }
}

fn binary_image_with_device(device: Option<FmirDeviceSection>) -> FmirBinaryImageFile {
    FmirBinaryImageFile {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: FMIR_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: "main.fab".to_owned(),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: TOOLCHAIN.to_owned(),
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
        cli: Some(FmirTextCliSection {
            root: FmirTextCliRootSection {
                record: "run_entry".to_owned(),
                operand: Vec::new(),
            },
        }),
        exit_code: Some(0),
        types: FmirTextTypesSection {
            table: TypeTable::new().snapshot(),
        },
        interner: vec!["main".to_owned()],
        program: MirProgram::new(),
        device,
    }
}

// ---------------------------------------------------------------------------
// Round-trip (Checkpoint A positive)
// ---------------------------------------------------------------------------

#[test]
fn device_section_round_trips_through_text_image() {
    let image = text_image_with_device(Some(sample_device_section()));
    let text = encode_text_image(&image).expect("encode text image");
    let decoded = decode_text_image(&text, TOOLCHAIN).expect("decode text image");
    let device = decoded.device.expect("device section present");
    assert_eq!(device.device_program.v, WIRE_VERSION);
    assert_eq!(
        device.device_program.program,
        sample_wire_program(),
        "the typed program round-trips field-for-field"
    );
    assert_eq!(device.selection, FmirDeviceSelection::Auto);
    assert_eq!(device.artifacts.artifact.len(), 2);
    assert_eq!(
        device.artifacts.artifact[0].backend,
        FmirDeviceBackend::Metal
    );
    assert_eq!(
        device.artifacts.artifact[1].backend,
        FmirDeviceBackend::Cuda
    );
    assert_eq!(
        device.declared_inputs,
        vec![FmirDeviceInput {
            name: "x".to_owned(),
            values: vec![1.0, 2.0, 3.0, 4.0],
        }]
    );
    assert_eq!(
        device.artifacts.artifact[0].target.required_features,
        Vec::<String>::new()
    );
    assert_eq!(
        device.artifacts.artifact[1].target.required_features,
        vec!["sm_120".to_owned()]
    );
}

#[test]
fn device_section_round_trips_through_binary_image() {
    let image = binary_image_with_device(Some(sample_device_section()));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    let decoded = decode_binary_image(&bytes, TOOLCHAIN).expect("decode binary image");
    let device = decoded.device.expect("device section present");
    assert_eq!(device.device_program.v, WIRE_VERSION);
    assert_eq!(
        device.device_program.program,
        sample_wire_program(),
        "the typed program round-trips field-for-field"
    );
    assert_eq!(device.selection, FmirDeviceSelection::Auto);
    assert_eq!(device.artifacts.artifact.len(), 2);
    // The canonical complete-program bytes are identical in both formats
    // (A10 over the COMPLETE program): the text-decoded and binary-decoded
    // programs canonicalize to the same identity substrate.
    let text_image = text_image_with_device(Some(sample_device_section()));
    let text_decoded =
        decode_text_image(&encode_text_image(&text_image).unwrap(), TOOLCHAIN).unwrap();
    assert_eq!(
        canonical_program_bytes(&text_decoded.device.unwrap().device_program.program),
        canonical_program_bytes(&device.device_program.program)
    );
    // The CUDA symbol mapping and the declared inputs ride the metadata, not
    // the canonical program bytes.
    assert_eq!(device.artifacts.artifact[1].entrypoints.len(), 1);
    assert_eq!(device.artifacts.artifact[1].entrypoints[0].entry, "addita");
}

#[test]
fn device_section_with_no_artifacts_round_trips() {
    // A device section may declare zero artifacts (e.g. an `auto` package
    // before backend emission). Admission must accept an empty artifact list.
    let mut section = sample_device_section();
    section.artifacts.artifact.clear();
    let image = text_image_with_device(Some(section));
    let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .expect("decode with empty artifacts");
    assert!(decoded.device.unwrap().artifacts.artifact.is_empty());
}

#[test]
fn reduced_projection_program_decodes_under_the_new_wire_version() {
    // Council-8 CB-3 decode-path coverage: a device program carrying the
    // new wire-11 reduced-resource projection facts — the `AxisReduction`
    // plan projection AND the reduced buffer version's carried projection —
    // decodes through BOTH codec paths (the text `fmir-text` codec and the
    // binary postcard codec) under the accepted wire version, with the
    // carried facts intact. The wire bump must not regress either decode
    // path, and the new facts must survive both.
    let mut program = sample_wire_program();
    program.kernels[0].plan = WireCollectionKernelPlan::AxisReduction(WireAxisReductionPlan {
        op: WireReduceOp::Sum,
        axis: 0,
        projection: WireReducedProjection {
            axis_extent: 4,
            inner_stride: 4,
        },
    });
    // The reduced output buffer version carries the same producer-defined
    // projection on the wire version itself.
    program.results[0].version.reduced_projection = Some(WireReducedProjection {
        axis_extent: 4,
        inner_stride: 4,
    });
    program.kernels[0].resources[1].version.reduced_projection = Some(WireReducedProjection {
        axis_extent: 4,
        inner_stride: 4,
    });
    let mut section = sample_device_section();
    section.device_program.program = program;

    // Text decode path (the `fmir-text` TOML codec).
    let text_image = text_image_with_device(Some(section.clone()));
    let text_decoded = decode_text_image(&encode_text_image(&text_image).unwrap(), TOOLCHAIN)
        .expect("wire-11 program decodes through the text codec");
    let text_device = text_decoded.device.expect("device section present");
    assert_eq!(text_device.device_program.v, WIRE_DEVICE_PROGRAM_VERSION);
    assert_eq!(
        text_device.device_program.program.kernels[0].plan,
        WireCollectionKernelPlan::AxisReduction(WireAxisReductionPlan {
            op: WireReduceOp::Sum,
            axis: 0,
            projection: WireReducedProjection {
                axis_extent: 4,
                inner_stride: 4,
            },
        }),
        "the axis-reduction plan projection survives the text decode"
    );
    assert_eq!(
        text_device.device_program.program.results[0]
            .version
            .reduced_projection,
        Some(WireReducedProjection {
            axis_extent: 4,
            inner_stride: 4,
        }),
        "the reduced buffer version's projection survives the text decode"
    );

    // Binary decode path (the postcard codec).
    let binary_image = binary_image_with_device(Some(section));
    let binary_decoded =
        decode_binary_image(&encode_binary_image(&binary_image).unwrap(), TOOLCHAIN)
            .expect("wire-11 program decodes through the postcard codec");
    let binary_device = binary_decoded.device.expect("device section present");
    assert_eq!(binary_device.device_program.v, WIRE_DEVICE_PROGRAM_VERSION);
    assert_eq!(
        binary_device.device_program.program.kernels[0].plan,
        WireCollectionKernelPlan::AxisReduction(WireAxisReductionPlan {
            op: WireReduceOp::Sum,
            axis: 0,
            projection: WireReducedProjection {
                axis_extent: 4,
                inner_stride: 4,
            },
        }),
        "the axis-reduction plan projection survives the postcard decode"
    );
    assert_eq!(
        binary_device.device_program.program.results[0]
            .version
            .reduced_projection,
        Some(WireReducedProjection {
            axis_extent: 4,
            inner_stride: 4,
        }),
        "the reduced buffer version's projection survives the postcard decode"
    );
}

// ---------------------------------------------------------------------------
// No regression (device: None is the pure CPU/FMIR route)
// ---------------------------------------------------------------------------

#[test]
fn image_without_device_section_round_trips_no_regression() {
    let text_image = text_image_with_device(None);
    let text_decoded =
        decode_text_image(&encode_text_image(&text_image).unwrap(), TOOLCHAIN).unwrap();
    assert!(text_decoded.device.is_none());

    let binary_image = binary_image_with_device(None);
    let binary_decoded =
        decode_binary_image(&encode_binary_image(&binary_image).unwrap(), TOOLCHAIN).unwrap();
    assert!(binary_decoded.device.is_none());
}

// ---------------------------------------------------------------------------
// Fail-closed admission (Checkpoint A negative)
// ---------------------------------------------------------------------------

#[test]
fn decode_rejects_unknown_target_feature() {
    // DDCP2-U3: the typed target features replace the `runtime_requirements`
    // allowlist — an unknown required feature fails closed with the named
    // rule, never guessed, never silently dropped.
    let mut section = sample_device_section();
    section.artifacts.artifact[1]
        .target
        .required_features
        .push("vulkan".to_owned());
    section.artifacts.artifact[1].packet_sha256 =
        section.artifacts.artifact[1].compute_packet_sha256();
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::NonAdmittedTargetFeature { feature }) => {
            assert_eq!(feature, "vulkan");
        }
        other => panic!("expected UnsupportedTargetFeature, got {other:?}"),
    }
}

#[test]
fn decode_rejects_incompatible_compiled_in_target() {
    // DDCP2-U3: compiled-in leaves keep exhaustive target validation — a
    // Metal artifact declaring the CUDA leaf's target id fails closed with
    // the named rule.
    let mut section = sample_device_section();
    section.artifacts.artifact[0].target.id = "sm_120".to_owned();
    section.artifacts.artifact[0].packet_sha256 =
        section.artifacts.artifact[0].compute_packet_sha256();
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::TargetIdMismatch { backend, .. }) if backend == "metal"
    ));
}

#[test]
fn decode_rejects_noncanonical_utf8_text_artifact() {
    // Contract §Payload encoding rule 2: a text-encoded payload must be
    // canonical UTF-8 — invalid UTF-8 bytes fail admission.
    let mut section = sample_device_section();
    section.artifacts.artifact[0].bytes = vec![0xff, 0xfe, 0xfd];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::ArtifactEncodingMismatch { backend, detail }) => {
            assert_eq!(backend, "metal");
            assert!(detail.contains("not canonical UTF-8"), "got {detail}");
        }
        other => panic!("expected ArtifactEncodingMismatch, got {other:?}"),
    }
}

#[test]
fn decode_rejects_text_artifact_with_bom() {
    // Contract §Payload encoding rule 2: canonical UTF-8 has no BOM.
    let mut section = sample_device_section();
    section.artifacts.artifact[0].bytes =
        b"\xef\xbb\xbfkernel void add(device float* a [[buffer(0)]]) {}".to_vec();
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::ArtifactEncodingMismatch { backend, .. }) if backend == "metal"
    ));
}

#[test]
fn decode_rejects_binary_byte_length_mismatch() {
    // Contract §Payload encoding rule 1: the declared decoded byte length
    // must equal the artifact bytes' length.
    let mut section = sample_device_section();
    section.artifacts.artifact[1] = FmirDeviceArtifact {
        encoding: DevicePayloadEncoding::Binary {
            encoding_tag: "llvm-ir".to_owned(),
            decoded_byte_length: 999,
            canonical_unpadded_base64: canonical_unpadded_base64(b"ir bytes"),
        },
        bytes: b"ir bytes".to_vec(),
        ..cuda_artifact("ir bytes")
    };
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::ArtifactEncodingMismatch { backend, .. }) if backend == "cuda"
    ));
}

#[test]
fn decode_rejects_noncanonical_binary_base64() {
    // Contract §Payload encoding rule 1/3: the declared base64 must be the
    // canonical unpadded base64 of the decoded bytes.
    let mut section = sample_device_section();
    section.artifacts.artifact[1] = FmirDeviceArtifact {
        encoding: DevicePayloadEncoding::Binary {
            encoding_tag: "llvm-ir".to_owned(),
            decoded_byte_length: 8,
            canonical_unpadded_base64: "NOT-BASE64-!!!".to_owned(),
        },
        bytes: b"ir bytes".to_vec(),
        ..cuda_artifact("ir bytes")
    };
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::ArtifactEncodingMismatch { .. })
    ));
}

#[test]
fn binary_payload_spelling_is_the_declared_transport_form() {
    // Contract §Payload encoding rule 1: a binary payload's transport
    // spelling is `binary:<encoding-tag>:<decoded-byte-length>:<canonical-
    // unpadded-base64>`, and it round-trips through both codecs.
    let artifact = with_computed_digests(FmirDeviceArtifact {
        encoding: DevicePayloadEncoding::Binary {
            encoding_tag: "elf".to_owned(),
            decoded_byte_length: 4,
            canonical_unpadded_base64: canonical_unpadded_base64(&[1, 2, 3, 4]),
        },
        bytes: vec![1, 2, 3, 4],
        ..cuda_artifact("")
    });
    assert_eq!(
        artifact.binary_payload_spelling(),
        format!("binary:elf:4:{}", canonical_unpadded_base64(&[1, 2, 3, 4]))
    );
    artifact
        .validate_encoding()
        .expect("the declared binary spelling is consistent");
    // Round-trip through the binary codec (the section carries the artifact).
    let mut section = sample_device_section();
    section.artifacts.artifact[1] = artifact;
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    decode_binary_image(&bytes, TOOLCHAIN).expect("the binary artifact round-trips");
}

#[test]
fn unknown_backend_artifact_preserved_until_selection() {
    // DDCP3-U0 (f) / CTO9-6 f: the blanket load-time rejection is REMOVED —
    // an artifact whose backend id is not a compiled-in leaf is representable
    // on the packet, validated as far as its declared fields allow
    // (encodings + canonical digests re-verified), and **admitted at load**
    // (preserved-until-selection). Never silently dropped, never remapped,
    // never fallback-laned.
    let mut section = sample_device_section();
    section
        .artifacts
        .artifact
        .push(with_computed_digests(FmirDeviceArtifact {
            // AMD-A3: `amd` is now the explicit compiled-in backend-id (OQ1
            // default) — the unknown-backend example uses a genuinely-unknown
            // id (`webgpu`); `Unknown` is kept for other ids.
            backend: FmirDeviceBackend::Unknown("webgpu".to_owned()),
            ..metal_artifact("webgpu kernel")
        }));
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    decode_text_image(&text, TOOLCHAIN)
        .expect("an unknown-backend artifact is admitted at load (preserve-until-selection)");
}

// ---------------------------------------------------------------------------
// deny_unknown_fields on the new section + enum fail-closed
// ---------------------------------------------------------------------------

#[test]
fn decode_rejects_unknown_device_section_key() {
    let image = text_image_with_device(Some(sample_device_section()));
    let mut text = encode_text_image(&image).expect("encode text image");
    // Inject an unknown key into the [device] table. TOML places it after the
    // known keys; deny_unknown_fields must reject it at parse time.
    text = text.replace(
        "[device]\n",
        "[device]\nsecret_key = \"should be rejected\"\n",
    );
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::Parse(_))
    ));
}

#[test]
fn decode_rejects_unknown_device_program_key() {
    let image = text_image_with_device(Some(sample_device_section()));
    let mut text = encode_text_image(&image).expect("encode text image");
    // The v4 opaque `payload` string is gone; a v4-era key is an unknown
    // field and deny_unknown_fields must reject it at parse time.
    text = text.replace(
        "[device.device_program]\n",
        "[device.device_program]\npayload = \"deny me\"\n",
    );
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::Parse(_))
    ));
}

/// The DDCP0-U6 fixture-01 MSL payload text (the canonical UTF-8 text
/// payload; `content_sha256` `60b43333…` is over these exact bytes).
const FIXTURE_MSL_TEXT: &str = "// DDCP0-U6 round-trip fixture — canonical UTF-8 text payload
// Scale kernel MSL source (NGAB0-U11 minimal one-prepared-region fixture:
// one host call → one prepared submission region containing one kernel).
// Text payloads are canonical UTF-8: valid UTF-8, no BOM, no alternative-
// encoding disguise; the canonical representation is the byte sequence itself.
// content_sha256 is the SHA-256 over these exact bytes — nothing else.
#include <metal_stdlib>
using namespace metal;

// oracle(x) = x * 2.0f + 1.0f
kernel void scale_kernel(device const float* in  [[buffer(0)]],
                         device       float* out [[buffer(1)]],
                         uint id [[thread_position_in_grid]]) {
    out[id] = in[id] * 2.0f + 1.0f;
}
";

/// The DDCP0-U6 fixture-02b decoded binary payload bytes
/// (`metallib-stub-v1`, 16 bytes; the `binary:metallib:16:bWV0Y…` spelling).
const FIXTURE_METALLIB_BYTES: &[u8] = b"metallib-stub-v1";

/// Round-trip `section` through both codecs and assert the decoded packet's
/// canonical decoded bytes + identity digests are byte-identical to the
/// source (DDCP2-U4 gate bullet 1: encode → decode → admit with identical
/// canonical decoded bytes and identical `content_sha256`/`packet_sha256`
/// across the wire).
fn assert_packet_round_trips(section: FmirDeviceSection, artifact_index: usize) {
    let source = &section.artifacts.artifact[artifact_index];
    let source_bytes = source.bytes.clone();
    let source_content = source.content_sha256.clone();
    let source_packet = source.packet_sha256.clone();

    let text_image = text_image_with_device(Some(section.clone()));
    let text = encode_text_image(&text_image).expect("encode text image");
    let decoded_text = decode_text_image(&text, TOOLCHAIN).expect("decode text image");
    let text_artifact = &decoded_text
        .device
        .expect("device section")
        .artifacts
        .artifact[artifact_index];
    assert_eq!(
        text_artifact.bytes, source_bytes,
        "text codec preserves the decoded bytes"
    );
    assert_eq!(
        text_artifact.content_sha256, source_content,
        "text codec preserves content_sha256"
    );
    assert_eq!(
        text_artifact.packet_sha256, source_packet,
        "text codec preserves packet_sha256"
    );

    let binary_image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&binary_image).expect("encode binary image");
    let decoded_binary = decode_binary_image(&bytes, TOOLCHAIN).expect("decode binary image");
    let binary_artifact = &decoded_binary
        .device
        .expect("device section")
        .artifacts
        .artifact[artifact_index];
    assert_eq!(
        binary_artifact.bytes, source_bytes,
        "binary codec preserves the decoded bytes"
    );
    assert_eq!(
        binary_artifact.content_sha256, source_content,
        "binary codec preserves content_sha256"
    );
    assert_eq!(
        binary_artifact.packet_sha256, source_packet,
        "binary codec preserves packet_sha256"
    );
}

#[test]
fn fixture_01_msl_text_payload_round_trips_with_identical_digests() {
    // DDCP0-U6 fixture-01 retarget: the canonical UTF-8 MSL text payload
    // round-trips through both codecs with identical canonical decoded bytes
    // and identical content/packet digests; the recorded content digest
    // (plain sha256 over the decoded bytes) recomputes under the schema.
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![metal_artifact(FIXTURE_MSL_TEXT)];
    assert_eq!(
        section.artifacts.artifact[0].compute_content_sha256(),
        "sha256:60b433339e212477ee083722fbedb69d5f57520853ea6fa989f6cacfb1feae8b",
        "fixture-01 content_sha256 recomputes under the schema"
    );
    assert_packet_round_trips(section, 0);
}

#[test]
fn fixture_ptx_text_payload_round_trips_with_identical_digests() {
    // The PTX text payload (the CUDA artifact spelling) round-trips through
    // both codecs with identical decoded bytes + digests.
    let ptx = ".version 8.7\n.target sm_120\n.entry scale_kernel\n";
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![cuda_artifact(ptx)];
    assert_packet_round_trips(section, 0);
}

#[test]
fn fixture_02_binary_payload_round_trips_with_declared_spelling() {
    // DDCP0-U6 fixture-02/02b retarget: the binary payload (16 decoded
    // bytes `metallib-stub-v1`) round-trips through both codecs with the
    // declared `binary:metallib:16:bWV0Y…` transport spelling; the recorded
    // content digest recomputes over the DECODED bytes. DDCP3-U0 (d)
    // coherence: a `metallib` is a finalized binary, so the fixture artifact
    // carries `stage = finalized-binary` + its hash-bound parent provenance
    // (the fixture-04 compiler-input packet) — the combination-valid shape.
    let mut section = sample_device_section();
    let artifact = with_computed_digests(FmirDeviceArtifact {
        format: DeviceArtifactFormat::Unknown("metallib".to_owned()),
        stage: MaterializationStage::FinalizedBinary,
        encoding: DevicePayloadEncoding::Binary {
            encoding_tag: "metallib".to_owned(),
            decoded_byte_length: 16,
            canonical_unpadded_base64: canonical_unpadded_base64(FIXTURE_METALLIB_BYTES),
        },
        bytes: FIXTURE_METALLIB_BYTES.to_vec(),
        compiler_input_packet_sha256: Some(
            "sha256:ce6abbcc288b78bb53bae05edd5676f9a634d036195794c99587753434d5470e".to_owned(),
        ),
        ..metal_artifact("")
    });
    assert_eq!(
        artifact.binary_payload_spelling(),
        "binary:metallib:16:bWV0YWxsaWItc3R1Yi12MQ",
        "the schema produces the fixture's declared transport spelling"
    );
    assert_eq!(
        artifact.compute_content_sha256(),
        "sha256:84433df026a63d867a6e10c08d574e887d0d315006fd8f0b1a48dbcbbebcd8a7",
        "fixture-02 content_sha256 recomputes over the DECODED bytes"
    );
    section.artifacts.artifact = vec![artifact];
    // The metallib's hash-bound parent names the compiler-input sibling
    // carried in the same section (P3-1): fixture-04's msl packet.
    let mut parent = metal_artifact(FIXTURE_MSL_TEXT);
    parent.entrypoints = vec![FmirDeviceSymbol {
        entry: "scale_kernel".to_owned(),
        symbol: "scale_kernel".to_owned(),
    }];
    let parent = with_computed_digests(parent);
    assert_eq!(
        parent.packet_sha256,
        "sha256:ce6abbcc288b78bb53bae05edd5676f9a634d036195794c99587753434d5470e",
        "the sibling is fixture-04's compiler-input packet (the recorded parent)"
    );
    section.artifacts.artifact.push(parent);
    assert_packet_round_trips(section, 0);
}

#[test]
fn content_sha256_covers_decoded_bytes_never_the_base64_spelling() {
    // DDCP2-U4 gate bullet 3 / contract §IdentityDomains rule 3: hashes
    // cover canonical decoded bytes, never the transport spelling — the
    // content digest is over the decoded bytes, and the base64 spelling
    // (padded vs unpadded) is transport, not payload.
    let decoded = FIXTURE_METALLIB_BYTES;
    let base64_string = "bWV0YWxsaWItc3R1Yi12MQ=="; // the PADDED spelling of the same bytes
    assert_eq!(
        sha256_digest(decoded),
        "sha256:84433df026a63d867a6e10c08d574e887d0d315006fd8f0b1a48dbcbbebcd8a7"
    );
    assert_ne!(
        sha256_digest(decoded),
        sha256_digest(base64_string.as_bytes()),
        "the digest of the base64 STRING differs from the digest of the decoded bytes"
    );
    // The artifact's content digest is over its decoded bytes only: a
    // different (padded) spelling in the encoding declaration does NOT
    // change the computed content digest.
    let artifact = with_computed_digests(FmirDeviceArtifact {
        format: DeviceArtifactFormat::Unknown("metallib".to_owned()),
        encoding: DevicePayloadEncoding::Binary {
            encoding_tag: "metallib".to_owned(),
            decoded_byte_length: 16,
            canonical_unpadded_base64: base64_string.to_owned(),
        },
        bytes: decoded.to_vec(),
        ..metal_artifact("")
    });
    assert_eq!(
        artifact.compute_content_sha256(),
        "sha256:84433df026a63d867a6e10c08d574e887d0d315006fd8f0b1a48dbcbbebcd8a7",
        "content_sha256 is over the decoded bytes, never the transport spelling"
    );
}

#[test]
fn fixture_04_and_06_packet_digests_recompute_under_the_schema() {
    // DDCP0-U6 evidence retarget: the fixture-04 compiler-input packet and
    // the fixture-06 finalized packet recompute under the schema's canonical
    // serialization — including the compiler_input_packet_sha256 fire-2 #5
    // fold (the parent packet is part of the finalized packet's OWN
    // identity; provenance is never silently replaced). DDCP3-U0 recompute
    // (truth over safety): the recompute now runs the CANONICAL packet
    // identity (packet_sha256_canonical — canonical ordering enforced +
    // the reflection fold). The fixture's inputs are already canonical
    // (empty required_features, single-entry entrypoint maps, `reflection =
    // None` folds nothing), so the recorded digests hold unchanged under the
    // revised hashing — proven here.
    let compiler_input = packet_sha256_canonical(
        ARTIFACT_ABI_VERSION,
        "metal",
        "msl",
        MaterializationStage::CompilerInput,
        "macos-arm64",
        &[],
        &[FmirDeviceSymbol {
            entry: "scale_kernel".to_owned(),
            symbol: "scale_kernel".to_owned(),
        }],
        None,
        "sha256:60b433339e212477ee083722fbedb69d5f57520853ea6fa989f6cacfb1feae8b",
        None,
    );
    assert_eq!(
        compiler_input, "sha256:ce6abbcc288b78bb53bae05edd5676f9a634d036195794c99587753434d5470e",
        "fixture-04 packet_sha256 recomputes under the schema (canonical identity)"
    );
    let finalized = packet_sha256_canonical(
        ARTIFACT_ABI_VERSION,
        "metal",
        "metallib",
        MaterializationStage::FinalizedBinary,
        "macos-arm64",
        &[],
        &[FmirDeviceSymbol {
            entry: "scale_kernel".to_owned(),
            symbol: "_scale_kernel".to_owned(),
        }],
        None,
        "sha256:84433df026a63d867a6e10c08d574e887d0d315006fd8f0b1a48dbcbbebcd8a7",
        Some(&compiler_input),
    );
    assert_eq!(
        finalized, "sha256:d46fa0db02b221c001e9c760bfb3dcf564953c9e171732eeb198ab410d4787b1",
        "fixture-06 packet_sha256 recomputes under the schema with the parent fold"
    );
}

// ---------------------------------------------------------------------------
// DDCP3-U0 (a)–(f): reflection carrier + hash fold, canonical ordering,
// ABI-version admission, combination validation, preserve-until-selection
// ---------------------------------------------------------------------------

#[test]
fn equivalent_sets_hash_to_identical_packet_identity() {
    // CTO9-6 (b): canonical ordering is ENFORCED before hashing — equivalent
    // sets (differing caller order, no duplicates) produce an identical
    // packet identity. `required_features` and `entrypoints` are sorted
    // internally; the shared `canonicalize_artifact` helper also normalizes
    // the stored vectors for producers.
    let forward = with_computed_digests(FmirDeviceArtifact {
        target: DeviceTargetId {
            id: "sm_120".to_owned(),
            required_features: vec!["cuda".to_owned(), "sm_120".to_owned(), "sm_80".to_owned()],
        },
        entrypoints: vec![
            FmirDeviceSymbol {
                entry: "beta".to_owned(),
                symbol: "_Z4beta".to_owned(),
            },
            FmirDeviceSymbol {
                entry: "alpha".to_owned(),
                symbol: "_Z5alpha".to_owned(),
            },
        ],
        ..cuda_artifact("ir bytes")
    });
    let reversed = with_computed_digests(FmirDeviceArtifact {
        target: DeviceTargetId {
            id: "sm_120".to_owned(),
            required_features: vec!["sm_80".to_owned(), "cuda".to_owned(), "sm_120".to_owned()],
        },
        entrypoints: vec![
            FmirDeviceSymbol {
                entry: "alpha".to_owned(),
                symbol: "_Z5alpha".to_owned(),
            },
            FmirDeviceSymbol {
                entry: "beta".to_owned(),
                symbol: "_Z4beta".to_owned(),
            },
        ],
        ..cuda_artifact("ir bytes")
    });
    assert_eq!(
        forward.packet_sha256, reversed.packet_sha256,
        "equivalent sets (differing caller order, no duplicates) hash to an identical packet identity"
    );
    // The stored vectors are normalized by the shared helper. Canonical
    // feature order is lexicographic: `sm_120` sorts BEFORE `sm_80`.
    let mut canonicalized = forward;
    canonicalized.canonicalize_artifact();
    assert_eq!(
        canonicalized.target.required_features,
        vec!["cuda".to_owned(), "sm_120".to_owned(), "sm_80".to_owned()]
    );
    assert_eq!(canonicalized.entrypoints.first().unwrap().entry, "alpha");
}

#[test]
fn non_canonical_stored_packet_fails_admission_reverification() {
    // CTO9-6 (b): a stored packet whose `packet_sha256` was computed over a
    // NON-canonical order fails admission re-verification — the identity is
    // the canonical set, so the recomputed digest (canonical) disagrees with
    // the declared-order digest (the pre-DDCP3-U0 producer behavior).
    let mut artifact = FmirDeviceArtifact {
        target: DeviceTargetId {
            id: "sm_120".to_owned(),
            // Non-canonical caller order: canonical (lexicographic) sorting
            // yields ["sm_120", "sm_80"] ("1" < "8").
            required_features: vec!["sm_80".to_owned(), "sm_120".to_owned()],
        },
        ..cuda_artifact("ir bytes")
    };
    artifact.content_sha256 = artifact.compute_content_sha256();
    artifact.packet_sha256 = crate::schema::packet_sha256_in_declared_order(
        artifact.abi_version,
        &artifact.backend.spelling(),
        &artifact.format.spelling(),
        artifact.stage,
        &artifact.target.id,
        &artifact.target.required_features,
        &artifact.entrypoints,
        &artifact.content_sha256,
        artifact.compiler_input_packet_sha256.as_deref(),
    );
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![artifact];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::ArtifactDigestMismatch {
            backend,
            field: "packet_sha256",
            ..
        }) => {
            assert_eq!(backend, "cuda");
        }
        other => panic!(
            "expected packet_sha256 re-verification failure for a non-canonical stored packet, got {other:?}"
        ),
    }
}

#[test]
fn duplicate_required_features_rejected_at_admission() {
    // CTO9-6 (b): duplicate required features are rejected at admission with
    // the named rule (independent of hashing).
    let artifact = with_computed_digests(FmirDeviceArtifact {
        target: DeviceTargetId {
            id: "sm_120".to_owned(),
            required_features: vec!["sm_120".to_owned(), "sm_120".to_owned()],
        },
        ..cuda_artifact("ir bytes")
    });
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![artifact];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::DuplicateRequiredFeature { feature }) if feature == "sm_120"
    ));
}

#[test]
fn duplicate_entrypoints_rejected_at_admission() {
    // CTO9-6 (b): duplicate entrypoint identities are rejected at admission
    // with the named rule.
    let artifact = with_computed_digests(FmirDeviceArtifact {
        entrypoints: vec![
            FmirDeviceSymbol {
                entry: "addita".to_owned(),
                symbol: "_Z6addita".to_owned(),
            },
            FmirDeviceSymbol {
                entry: "addita".to_owned(),
                symbol: "_Z6addita2".to_owned(),
            },
        ],
        ..cuda_artifact("ir bytes")
    });
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![artifact];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::DuplicateEntrypoint { entry }) if entry == "addita"
    ));
}

#[test]
fn unsupported_artifact_abi_version_rejected_at_admission() {
    // CTO9-6 (c): an artifact whose `abi_version` is not the accepted
    // `ARTIFACT_ABI_VERSION` (the coordinated boundary) fails admission with
    // the named diagnostic — exact-match clean break.
    let mut artifact = metal_artifact("kernel void add() {}");
    artifact.abi_version = ARTIFACT_ABI_VERSION - 1;
    artifact = with_computed_digests(artifact);
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![artifact];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::ArtifactAbiVersionMismatch {
            backend,
            actual,
            accepted,
        }) => {
            assert_eq!(backend, "metal");
            assert_eq!(actual, ARTIFACT_ABI_VERSION - 1);
            assert_eq!(accepted, ARTIFACT_ABI_VERSION);
        }
        other => panic!("expected UnsupportedArtifactAbiVersion, got {other:?}"),
    }
}

#[test]
fn incompatible_artifact_combination_rejected_at_admission() {
    // CTO9-6 (d): a known backend cannot carry an incompatible known format
    // or an unrelated known feature — the (backend, format, stage, feature)
    // combination is validated against the assembled compatibility table.
    // metal + elf fails (no metal/elf row).
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        format: DeviceArtifactFormat::Elf,
        ..metal_artifact("ir bytes")
    })];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::IncompatibleArtifactCombination { backend, detail }) => {
            assert_eq!(backend, "metal");
            assert!(
                detail.contains("elf"),
                "the diagnostic must name the violated combination, got: {detail}"
            );
        }
        other => panic!("expected IncompatibleArtifactCombination for metal+elf, got {other:?}"),
    }

    // cuda + msl fails (no cuda/msl row).
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        format: DeviceArtifactFormat::Msl,
        ..cuda_artifact("ir bytes")
    })];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::IncompatibleArtifactCombination { backend, .. }) if backend == "cuda"
    ));

    // metal + an unrelated KNOWN feature (sm_120) fails — the feature is
    // known globally but not permitted for the metal backend.
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        target: DeviceTargetId {
            id: "macos-arm64".to_owned(),
            required_features: vec!["sm_120".to_owned()],
        },
        ..metal_artifact("ir bytes")
    })];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::IncompatibleArtifactCombination { backend, detail }) => {
            assert_eq!(backend, "metal");
            assert!(
                detail.contains("sm_120"),
                "the diagnostic must name the unrelated feature, got: {detail}"
            );
        }
        other => panic!(
            "expected IncompatibleArtifactCombination for an unrelated known feature, got {other:?}"
        ),
    }

    // An unknown global feature still fails with the named rule.
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        target: DeviceTargetId {
            id: "macos-arm64".to_owned(),
            required_features: vec!["vulkan".to_owned()],
        },
        ..metal_artifact("ir bytes")
    })];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::NonAdmittedTargetFeature { feature }) if feature == "vulkan"
    ));
}

#[test]
fn unknown_backend_artifact_skips_leaf_combination_rules() {
    // CTO9-6 (d): unknown-backend artifacts are validated as far as their
    // declared fields allow — the leaf-specific (backend, format, stage,
    // feature) combination rules are skipped (no leaf knowledge), while the
    // global known-feature check + digest re-verification still apply.
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        // AMD-A3: `amd` is now the explicit compiled-in backend-id (OQ1
        // default) — the unknown-backend example uses a genuinely-unknown id.
        backend: FmirDeviceBackend::Unknown("webgpu".to_owned()),
        format: DeviceArtifactFormat::Unknown("amdgcn".to_owned()),
        target: DeviceTargetId {
            id: "gfx1100".to_owned(),
            required_features: vec!["amdgpu".to_owned()],
        },
        ..metal_artifact("webgpu kernel")
    })];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    decode_text_image(&text, TOOLCHAIN)
        .expect("an unknown-backend artifact is admitted as far as its declared fields allow");
}

#[test]
fn reflection_round_trips_and_folds_into_packet_sha256() {
    // CTO9-6 (a): the typed reflection carrier folds into `packet_sha256`
    // (canonical order) and round-trips through both codecs. A carried
    // `None` folds nothing (the DDCP2 identity is preserved for
    // reflection-less packets); a carried `Some` changes the identity.
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![metal_artifact("kernel void add() {}")];
    let without_reflection = section.artifacts.artifact[0].compute_packet_sha256();

    let reflected = FmirArtifactReflection {
        v: crate::schema::ARTIFACT_REFLECTION_VERSION,
        kernels: vec![FmirReflectedKernel {
            function: 1,
            entry_name: "add".to_owned(),
            resources: vec![FmirReflectedResource {
                name: "buffer0".to_owned(),
                binding: 0,
            }],
        }],
    };
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        reflection: Some(reflected.clone()),
        ..metal_artifact("kernel void add() {}")
    })];
    let with_reflection = section.artifacts.artifact[0].compute_packet_sha256();
    assert_ne!(
        without_reflection, with_reflection,
        "a carried reflection changes the packet identity (the reflection fold)"
    );

    // Reflection round-trips through both codecs with the digests intact.
    let image = text_image_with_device(Some(section.clone()));
    let text = encode_text_image(&image).expect("encode text image");
    let decoded = decode_text_image(&text, TOOLCHAIN).expect("decode text image");
    let artifact = &decoded
        .device
        .expect("device section present")
        .artifacts
        .artifact[0];
    assert_eq!(artifact.reflection, Some(reflected.clone()));
    assert_eq!(artifact.packet_sha256, with_reflection);

    let binary_image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&binary_image).expect("encode binary image");
    let decoded = decode_binary_image(&bytes, TOOLCHAIN).expect("decode binary image");
    let artifact = &decoded
        .device
        .expect("device section present")
        .artifacts
        .artifact[0];
    assert_eq!(artifact.reflection, Some(reflected));
    assert_eq!(artifact.packet_sha256, with_reflection);
}

/// A device section whose single Metal artifact carries a reflection with the
/// given nested `v` and freshly recomputed digests, so the packet is
/// self-consistent and only the reflection version can fail admission.
fn section_with_reflection_version(v: u32) -> FmirDeviceSection {
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        reflection: Some(FmirArtifactReflection { v, kernels: vec![] }),
        ..metal_artifact("kernel void add() {}")
    })];
    section
}

#[test]
fn reflection_version_mismatch_is_rejected_by_text_decoder() {
    for actual in [crate::schema::ARTIFACT_REFLECTION_VERSION + 1, 1] {
        let image = text_image_with_device(Some(section_with_reflection_version(actual)));
        let text = encode_text_image(&image).expect("encode text image");
        match decode_text_image(&text, TOOLCHAIN) {
            Err(FmirImageError::ArtifactReflectionVersionMismatch {
                actual: found,
                expected,
                ..
            }) => {
                assert_eq!(found, actual);
                assert_eq!(expected, crate::schema::ARTIFACT_REFLECTION_VERSION);
            }
            other => panic!("expected ArtifactReflectionVersionMismatch, got {other:?}"),
        }
    }
}

#[test]
fn reflection_version_mismatch_is_rejected_by_binary_decoder() {
    let actual = crate::schema::ARTIFACT_REFLECTION_VERSION + 1;
    let image = binary_image_with_device(Some(section_with_reflection_version(actual)));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    match decode_binary_image(&bytes, TOOLCHAIN) {
        Err(FmirImageError::ArtifactReflectionVersionMismatch {
            actual: found,
            expected,
            ..
        }) => {
            assert_eq!(found, actual);
            assert_eq!(expected, crate::schema::ARTIFACT_REFLECTION_VERSION);
        }
        other => panic!("expected ArtifactReflectionVersionMismatch, got {other:?}"),
    }
}

#[test]
fn reflection_content_folds_in_canonical_order_invariant() {
    // DDCP4-U4 content: the reflection fold is over the CANONICAL order of
    // the carrier rows — two packets whose reflected kernels carry the same
    // facts in different orders produce an identical packet identity after
    // `canonicalize_artifact` (equivalent sets always hash to the same
    // identity; admission re-verifies the canonical order regardless).
    let reflection_a = FmirArtifactReflection {
        v: crate::schema::ARTIFACT_REFLECTION_VERSION,
        kernels: vec![
            FmirReflectedKernel {
                function: 1,
                entry_name: "add".to_owned(),
                resources: vec![
                    FmirReflectedResource {
                        name: "z".to_owned(),
                        binding: 1,
                    },
                    FmirReflectedResource {
                        name: "a".to_owned(),
                        binding: 0,
                    },
                ],
            },
            FmirReflectedKernel {
                function: 2,
                entry_name: "mul".to_owned(),
                resources: vec![FmirReflectedResource {
                    name: "b".to_owned(),
                    binding: 0,
                }],
            },
        ],
    };
    let reflection_b = FmirArtifactReflection {
        v: crate::schema::ARTIFACT_REFLECTION_VERSION,
        kernels: vec![
            FmirReflectedKernel {
                function: 2,
                entry_name: "mul".to_owned(),
                resources: vec![FmirReflectedResource {
                    name: "b".to_owned(),
                    binding: 0,
                }],
            },
            FmirReflectedKernel {
                function: 1,
                entry_name: "add".to_owned(),
                resources: vec![
                    FmirReflectedResource {
                        name: "a".to_owned(),
                        binding: 0,
                    },
                    FmirReflectedResource {
                        name: "z".to_owned(),
                        binding: 1,
                    },
                ],
            },
        ],
    };

    let mut packet_a = with_computed_digests(FmirDeviceArtifact {
        reflection: Some(reflection_a),
        ..metal_artifact("kernel void add() {}")
    });
    packet_a.canonicalize_artifact();
    let mut packet_b = with_computed_digests(FmirDeviceArtifact {
        reflection: Some(reflection_b),
        ..metal_artifact("kernel void add() {}")
    });
    packet_b.canonicalize_artifact();

    assert_eq!(
        packet_a.packet_sha256, packet_b.packet_sha256,
        "equivalent reflection content in different orders must fold to one packet identity"
    );
}

#[test]
fn unknown_selection_is_representable_and_rejected_at_resolution() {
    // DDCP2-U2: the selection enum migrates to backend-id enum semantics —
    // an unknown selection id is `Unknown(id)`, representable on the image
    // (it DECODES), and rejected honestly at selection resolution (no
    // compiled-in leaf matches), never silently remapped. DDCP3-U0 (f): an
    // explicit selection of an unknown backend now fails ADMISSION with
    // `NonAdmittedSelectedBackend` (selection resolution at the radix gate);
    // unknown-backend ARTIFACTS are preserved-until-selection.
    let image = text_image_with_device(Some(sample_device_section()));
    let mut text = encode_text_image(&image).expect("encode text image");
    text = text.replace(
        "selection = \"auto\"",
        "selection = { unknown = \"vulkan\" }",
    );
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::NonAdmittedSelectedBackend { backend }) => {
            assert_eq!(backend, "vulkan");
        }
        other => panic!("expected NonAdmittedSelectedBackend, got {other:?}"),
    }
}

#[test]
fn non_admitted_artifact_backend_display_names_the_selection_variant() {
    // The Display text is the only prose surface naming the fail-closed
    // follow-up path; it must name the variant that actually exists.
    let error = FmirImageError::NonAdmittedArtifactBackend {
        backend: "vulkan".into(),
    };
    let message = error.to_string();
    assert!(
        message.contains("NonAdmittedSelectedBackend"),
        "the artifact-backend diagnostic must name the selection variant, got: {message}"
    );
    assert!(
        !message.contains("UnsupportedSelectedBackend"),
        "the artifact-backend diagnostic must not name a nonexistent variant, got: {message}"
    );
}

#[test]
fn unknown_backend_is_representable_and_preserved_at_load() {
    // Contract §Unknown future backends + DDCP3-U0 (f): an artifact backend
    // id that is not a compiled-in leaf DECODES as `Unknown(id)` and is
    // **preserved-until-selection** — admitted at load when its declared
    // fields are well-formed (encodings + canonical digests re-verified),
    // rejected at selection use. The text-replace trick can no longer probe
    // a load-time rejection (that path is gone); build the packet through
    // the typed helpers so the digests are canonical and admission passes.
    let mut section = sample_device_section();
    section
        .artifacts
        .artifact
        .push(with_computed_digests(FmirDeviceArtifact {
            backend: FmirDeviceBackend::Unknown("rocm".to_owned()),
            ..metal_artifact("rocm kernel")
        }));
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    let decoded = decode_text_image(&text, TOOLCHAIN)
        .expect("unknown-backend artifact is preserved-until-selection (admitted at load)");
    let artifact = &decoded
        .device
        .expect("device section present")
        .artifacts
        .artifact[2];
    assert_eq!(
        artifact.backend,
        FmirDeviceBackend::Unknown("rocm".to_owned())
    );

    // The same unknown-backend ARTIFACT under an explicit unknown SELECTION
    // fails admission at selection resolution.
    let mut section = sample_device_section();
    section.selection = FmirDeviceSelection::Unknown("rocm".to_owned());
    section
        .artifacts
        .artifact
        .push(with_computed_digests(FmirDeviceArtifact {
            backend: FmirDeviceBackend::Unknown("rocm".to_owned()),
            ..metal_artifact("rocm kernel")
        }));
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::NonAdmittedSelectedBackend { backend }) if backend == "rocm"
    ));
}

// ── AMD-A3: the explicit `amd` backend-id (OQ1 spelling default) ───────────

/// The AMD artifact fixture: format `llvm-ir`, stage `compiler-input`,
/// target `gfx942`, CDNA3 feature set — the admission table's `amd` row.
fn amd_artifact(blob: &str) -> FmirDeviceArtifact {
    with_computed_digests(FmirDeviceArtifact {
        backend: FmirDeviceBackend::Amd,
        format: DeviceArtifactFormat::LlvmIr,
        stage: MaterializationStage::CompilerInput,
        target: DeviceTargetId {
            id: "gfx942".to_owned(),
            required_features: vec!["amdgpu".to_owned(), "cdna3".to_owned(), "gfx942".to_owned()],
        },
        ..metal_artifact(blob)
    })
}

#[test]
fn amd_artifact_admits_and_round_trips_both_codecs() {
    // AMD-A3 done_when (b): the `_ =>` admission
    // (`check_artifact_targets`) admits an
    // `amd`/`llvm-ir`/`compiler-input`/`gfx942` artifact honestly with the
    // default-table amd row, and the artifact + explicit `Amd` selection
    // round-trip through both codecs (wire round-trip green — the additive
    // variant posture the seed established; no carrier-version bump).
    let mut section = sample_device_section();
    section.selection = FmirDeviceSelection::Amd;
    section
        .artifacts
        .artifact
        .push(amd_artifact("; amd module"));
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    let decoded = decode_text_image(&text, TOOLCHAIN)
        .expect("the amd artifact is admitted with the gfx942 row");
    let device = decoded.device.expect("device section present");
    let amd = device
        .artifacts
        .artifact
        .last()
        .expect("amd artifact present");
    assert_eq!(amd.backend, FmirDeviceBackend::Amd);
    assert_eq!(amd.backend.spelling(), "amd");
    assert_eq!(amd.format, DeviceArtifactFormat::LlvmIr);
    assert_eq!(amd.target.id, "gfx942");
    assert_eq!(device.selection, FmirDeviceSelection::Amd);
    assert_eq!(device.selection.spelling(), "amd");

    // Binary codec round-trip too.
    let mut section = sample_device_section();
    section.selection = FmirDeviceSelection::Amd;
    section
        .artifacts
        .artifact
        .push(amd_artifact("; amd module"));
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    decode_binary_image(&bytes, TOOLCHAIN)
        .expect("the amd artifact + Amd selection round-trip through the binary codec");
}

#[test]
fn amd_artifact_mismatched_target_fails_closed() {
    // AMD-A3 done_when (b): a mismatched target/format combination fails
    // closed with the named rule — the amd row pins the explicit `gfx942`
    // target.
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        backend: FmirDeviceBackend::Amd,
        format: DeviceArtifactFormat::LlvmIr,
        stage: MaterializationStage::CompilerInput,
        target: DeviceTargetId {
            id: "sm_120".to_owned(),
            required_features: vec!["amdgpu".to_owned(), "cdna3".to_owned(), "gfx942".to_owned()],
        },
        ..metal_artifact("; amd module")
    })];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::TargetIdMismatch { backend, target, .. })
            if backend == "amd" && target == "sm_120"
    ));
}

#[test]
fn amd_artifact_wrong_format_fails_closed() {
    // AMD-A3 done_when (b): the amd leaf admits `llvm-ir` (compiler-input)
    // only — a mismatched format fails closed with the named rule.
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![with_computed_digests(FmirDeviceArtifact {
        backend: FmirDeviceBackend::Amd,
        format: DeviceArtifactFormat::Ptx,
        stage: MaterializationStage::CompilerInput,
        target: DeviceTargetId {
            id: "gfx942".to_owned(),
            required_features: vec!["amdgpu".to_owned(), "cdna3".to_owned(), "gfx942".to_owned()],
        },
        ..metal_artifact("; amd module")
    })];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::IncompatibleArtifactCombination { backend, .. }) if backend == "amd"
    ));
}

#[test]
fn amd_selection_requires_an_amd_artifact() {
    // AMD-A3: the selection table admits the amd combination fail-closed —
    // an explicit `Amd` selection without an `amd` artifact fails with
    // `MissingArtifactForSelection` (never a silent fallback lane).
    let mut section = sample_device_section();
    section.selection = FmirDeviceSelection::Amd;
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::AbsentArtifactForSelection { selection }) if selection == "amd"
    ));
}

#[test]
fn amd_spelling_round_trips_as_explicit_variant_not_unknown() {
    // AMD-A3 done_when (a)+(c): the OQ1 `amd` id decodes to the explicit
    // `Amd` variant (the same wire value as the seed's `amdgpu` id), while a
    // genuinely-unknown id (`rocm`) still decodes as `Unknown` and rejects
    // honestly at selection — never remapped, never fallback-laned.
    let mut section = sample_device_section();
    section.selection = FmirDeviceSelection::Unknown("rocm".to_owned());
    section
        .artifacts
        .artifact
        .push(with_computed_digests(FmirDeviceArtifact {
            backend: FmirDeviceBackend::Unknown("rocm".to_owned()),
            ..metal_artifact("rocm kernel")
        }));
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::NonAdmittedSelectedBackend { backend }) if backend == "rocm"
    ));
}

// ---------------------------------------------------------------------------
// Version / toolchain admission ordering
// ---------------------------------------------------------------------------

#[test]
fn decode_rejects_pre_break_version_5_image_with_device() {
    // A version-5 image (the pre-3R layout without the F1–F7 wire sections)
    // carrying a device section must fail closed on the outer version gate
    // before any device admission runs (the clean-break ratchet; a legacy
    // image fails closed with the named diagnostic).
    let mut image = text_image_with_device(Some(sample_device_section()));
    image.version = 5;
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::VersionMismatch {
            actual: 5,
            expected: PACKAGE_MIR_ARTIFACT_VERSION
        })
    ));
}

#[test]
fn decode_rejects_genuine_v5_layout_binary_image() {
    // The 5→6 clean break (Stage 3R U1) adds the F1–F7 semantic sections to
    // the typed device program. A legacy image is any image whose first wire
    // varint is 5: the version pre-scan must report UnsupportedVersion, not a
    // structural Parse error.
    let image = binary_image_with_device(None);
    let mut bytes = encode_binary_image(&image).expect("encode binary image");
    assert_eq!(
        u32::from(bytes[0]),
        PACKAGE_MIR_ARTIFACT_VERSION,
        "version is the first postcard varint byte"
    );
    bytes[0] = 5;
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::VersionMismatch {
            actual: 5,
            expected: PACKAGE_MIR_ARTIFACT_VERSION
        })
    ));
}

// ---------------------------------------------------------------------------
// Stage 3R S8: nested device-program version admission (independent of the
// outer FMIR version)
// ---------------------------------------------------------------------------

#[test]
fn decode_rejects_old_wire_version_in_device_section() {
    // The nested device-program wire version is admitted INDEPENDENTLY of the
    // outer FMIR version: a v6 image (outer version current) carrying a
    // pre-break v3 device-program section fails closed with the focused
    // nested diagnostic, before host construction or launch (S8/F7).
    let mut section = sample_device_section();
    section.device_program.v = 3;
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::DeviceProgramVersionMismatch { actual, expected }) => {
            assert_eq!(actual, 3);
            assert_eq!(expected, WIRE_DEVICE_PROGRAM_VERSION);
        }
        other => panic!("expected UnsupportedDeviceProgramVersion, got {other:?}"),
    }
}

#[test]
fn decode_rejects_old_wire_version_in_binary_image() {
    let mut section = sample_device_section();
    section.device_program.v = 3;
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::DeviceProgramVersionMismatch { actual, expected })
            if actual == 3 && expected == WIRE_DEVICE_PROGRAM_VERSION
    ));
}

#[test]
fn mixed_wire_version_fails_with_the_named_incompatible_diagnostic() {
    // The v10 → v11 clean break (S2: the batched plan variants landed
    // inside wire-10 without a ratchet) makes a device section carrying the
    // immediately-previous wire version a MIXED-version payload. It must
    // fail closed with the NAMED "incompatible wire version" diagnostic
    // before field-level interpretation. The malformed roots below prove
    // that nested-version admission runs first.
    let mut section = sample_device_section();
    section.device_program.v = WIRE_DEVICE_PROGRAM_VERSION - 1;
    section.device_program.program.roots.clear();
    let image = text_image_with_device(Some(section.clone()));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::DeviceProgramVersionMismatch { actual, expected }) => {
            assert_eq!(actual, WIRE_DEVICE_PROGRAM_VERSION - 1);
            assert_eq!(expected, WIRE_DEVICE_PROGRAM_VERSION);
            let message =
                FmirImageError::DeviceProgramVersionMismatch { actual, expected }.to_string();
            assert!(
                message.contains("incompatible") && message.contains("wire version"),
                "the diagnostic must name the incompatible wire version, got: {message}"
            );
        }
        other => panic!("expected the named incompatible-wire-version error, got {other:?}"),
    }

    // The binary (postcard) codec surfaces the SAME named diagnostic.
    let binary_image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&binary_image).expect("encode binary image");
    assert!(
        matches!(
            decode_binary_image(&bytes, TOOLCHAIN),
            Err(FmirImageError::DeviceProgramVersionMismatch { actual, expected })
                if actual == WIRE_DEVICE_PROGRAM_VERSION - 1
                    && expected == WIRE_DEVICE_PROGRAM_VERSION
        ),
        "the binary codec must reject the mixed-version payload with the same focused diagnostic"
    );
}

#[test]
fn decode_rejects_outer_version_independently_of_nested_version() {
    // The outer gate is also independent: a v5 image whose nested device
    // program is already the current v10 still fails on the OUTER version
    // first — the two versions never gate through each other (F7).
    let mut image = text_image_with_device(Some(sample_device_section()));
    image.version = 5;
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::VersionMismatch {
            actual: 5,
            expected: PACKAGE_MIR_ARTIFACT_VERSION
        })
    ));
}

#[test]
fn decode_rejects_device_payload_from_wrong_toolchain() {
    // A device payload is admitted only from the exact producing toolchain
    // version (N1.7): mismatched toolchain fails before device admission.
    let image = text_image_with_device(Some(sample_device_section()));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, "0.78.0"),
        Err(FmirImageError::ToolchainMismatch { found, expected })
            if found == "0.79.0" && expected == "0.78.0"
    ));
}

// ---------------------------------------------------------------------------
// Hash helpers (FNV-1a per blob; SHA-256 same-package identity)
// ---------------------------------------------------------------------------

#[test]
fn device_identity_hash_is_canonical_format() {
    let identity = device_identity_hash(
        &["fnv64:0000000000000000", "fnv64:1111111111111111"],
        &canonical_program_bytes(&sample_wire_program()),
    );
    assert!(identity.starts_with("sha256:"));
    assert_eq!(identity.len(), "sha256:".len() + 64);
    assert!(
        identity["sha256:".len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "identity digest must be lowercase hex"
    );
}

#[test]
fn device_identity_hash_is_deterministic() {
    let sources = &["fnv64:aaaa", "fnv64:bbbb"];
    let payload = canonical_program_bytes(&sample_wire_program());
    assert_eq!(
        device_identity_hash(sources, &payload),
        device_identity_hash(sources, &payload)
    );
}

#[test]
fn device_identity_hash_excludes_backend_blobs_and_run_data() {
    // A10: the identity is over source hashes + the canonical complete-program
    // bytes only. The CUDA symbol mapping and the declared input values ride
    // the metadata and never touch the identity substrate — proven by
    // canonicalizing the same program with different metadata.
    let sources = &["fnv64:aaaa"];
    let mut section = sample_device_section();
    let base = device_identity_hash(
        sources,
        &canonical_program_bytes(&section.device_program.program),
    );
    section.artifacts.artifact[1].entrypoints = vec![FmirDeviceSymbol {
        entry: "other".to_owned(),
        symbol: "_Zother".to_owned(),
    }];
    section.declared_inputs = vec![FmirDeviceInput {
        name: "other".to_owned(),
        values: vec![9.0],
    }];
    assert_eq!(
        device_identity_hash(
            sources,
            &canonical_program_bytes(&section.device_program.program)
        ),
        base
    );
}

#[test]
fn canonical_program_bytes_cover_the_complete_program() {
    // A10 over the COMPLETE program: the canonical bytes cover kernels,
    // launches, lifetime, and results — a thinned slot-list would change the
    // digest. Mutating a kernel field changes the bytes; the metadata does
    // not.
    let program = sample_wire_program();
    let base = canonical_program_bytes(&program);
    let mut mutated = program.clone();
    mutated.kernels[0].entry = "renamed".to_owned();
    assert_ne!(canonical_program_bytes(&mutated), base);
    let mut mutated = program.clone();
    mutated.results.clear();
    assert_ne!(canonical_program_bytes(&mutated), base);
}

/// S5A-U1 (A7): the observation cadence is part of the canonical wire — a
/// cadence edit produces different wire bytes (the readback timing is a
/// carried fact, never derived), and a cadence that contradicts the buffer's
/// lifetime class fails closed at the decode boundary.
#[test]
fn observation_cadence_rides_the_wire_and_fails_closed_on_contradiction() {
    // The canonical bytes change when the cadence changes.
    let program = sample_wire_program();
    let base = canonical_program_bytes(&program);
    let mut re_declared = program.clone();
    re_declared.results[0].observation.cadence = WireObservationCadence::EndOfRun;
    assert_ne!(
        canonical_program_bytes(&re_declared),
        base,
        "the canonical wire bytes change when the observation cadence changes (A7)"
    );

    // A PerStep cadence on a PerStep-lifetime buffer is a contradiction and
    // fails admission (a per-step readback names an observation-point buffer).
    let err = wire_admission_error(|program| {
        program.results[0].buffer.lifetime = WireBufferLifetime::PerStep;
        program.results[0].observation.cadence = WireObservationCadence::PerStep;
    })
    .expect_err("a per-step cadence on a per-step buffer fails closed");
    assert!(
        err.to_string().contains("cadence"),
        "the rejection names the cadence contradiction: {err}"
    );

    // An EndOfRun cadence on an ObservationPoint buffer fails closed (an
    // end-of-run readback names a per-step or per-program buffer — the
    // observation-point buffer is the per-step readback).
    let err = wire_admission_error(|program| {
        program.results[0].observation.cadence = WireObservationCadence::EndOfRun;
    })
    .expect_err("an end-of-run cadence on an observation-point buffer fails closed");
    assert!(
        err.to_string().contains("cadence"),
        "the rejection names the cadence contradiction: {err}"
    );
}

/// Encode a text image carrying a mutated device program and attempt decode.
/// Returns the decode outcome (admission verdict).
fn wire_admission_error(
    mutate: impl FnOnce(&mut WireDeviceProgram),
) -> Result<FmirTextImageFile, FmirImageError> {
    let mut program = sample_wire_program();
    mutate(&mut program);
    let mut section = sample_device_section();
    section.device_program.program = program;
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    decode_text_image(&text, TOOLCHAIN)
}

/// A valid two-kernel chain program: kernel 0 writes value 1 (buffer 1),
/// kernel 1 consumes it and writes value 2 (buffer 2); launch 1 → launch 2.
fn two_kernel_chain_program() -> WireDeviceProgram {
    let value_one = WireBufferIdentity {
        id: 1,
        name: "a".to_owned(),
        role: WireBufferRole::InOut,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerStep,
        semantic_value: 1,
    };
    let value_two = WireBufferIdentity {
        id: 2,
        name: "b".to_owned(),
        role: WireBufferRole::Output,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::ObservationPoint,
        semantic_value: 2,
    };
    WireDeviceProgram {
        kernels: vec![
            WireKernelUnit {
                function: 0,
                entry: "first".to_owned(),
                plan: WireCollectionKernelPlan::Elementwise,
                resources: vec![WireDeviceResource {
                    buffer: value_one.clone(),
                    version: WireBufferVersion {
                        version: 1,
                        element_ty: "f32".to_owned(),
                        element_count: 4,
                        shape: None,
                        reduced_projection: None,
                    },
                    binding: WireBinding {
                        group: 0,
                        binding: 0,
                    },
                    access: WireResourceAccess::Write,
                    generation: 1,
                    initialization: WireInitializationPolicy::KernelInitialized,
                }],
                launch: WireKernelLaunchPlan {
                    workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
                    dispatch_size: WireDispatchSize { x: 4, y: 1, z: 1 },
                    workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
                },
            },
            WireKernelUnit {
                function: 1,
                entry: "second".to_owned(),
                plan: WireCollectionKernelPlan::Elementwise,
                resources: vec![
                    WireDeviceResource {
                        buffer: value_one.clone(),
                        version: WireBufferVersion {
                            version: 1,
                            element_ty: "f32".to_owned(),
                            element_count: 4,
                            shape: None,
                            reduced_projection: None,
                        },
                        binding: WireBinding {
                            group: 0,
                            binding: 0,
                        },
                        access: WireResourceAccess::Read,
                        generation: 1,
                        initialization: WireInitializationPolicy::KernelInitialized,
                    },
                    WireDeviceResource {
                        buffer: value_two.clone(),
                        version: WireBufferVersion {
                            version: 1,
                            element_ty: "f32".to_owned(),
                            element_count: 4,
                            shape: None,
                            reduced_projection: None,
                        },
                        binding: WireBinding {
                            group: 0,
                            binding: 1,
                        },
                        access: WireResourceAccess::Write,
                        generation: 1,
                        initialization: WireInitializationPolicy::KernelInitialized,
                    },
                ],
                launch: WireKernelLaunchPlan {
                    workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
                    dispatch_size: WireDispatchSize { x: 4, y: 1, z: 1 },
                    workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
                },
            },
        ],
        launches: vec![
            WireLaunchUnit {
                id: 1,
                kernel_index: 0,
            },
            WireLaunchUnit {
                id: 2,
                kernel_index: 1,
            },
        ],
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: value_two,
            version: WireBufferVersion {
                version: 1,
                element_ty: "f32".to_owned(),
                element_count: 4,
                shape: None,
                reduced_projection: None,
            },
            role: WireBufferRole::Output,
            produced_by: 2,
            observation: WireObservationFact {
                at_launch: 2,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values: vec![
            WireSemanticValue {
                id: 1,
                name: "a".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "first-output".to_owned(),
                },
            },
            WireSemanticValue {
                id: 2,
                name: "b".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "second-output".to_owned(),
                },
            },
        ],
        roots: vec![1],
        dependencies: vec![WireDependencyEdge {
            producer: 1,
            consumer: 2,
            buffer: 1,
            version: 1,
        }],
        relations: Vec::new(),
    }
}

/// A generic four-child program with ordinary elementwise recipes. Each child
/// consumes the prior child's version and publishes the next one; there is no
/// composed or stage-specific wire shape involved.
fn four_kernel_chain_program() -> WireDeviceProgram {
    let buffer = |id: u32, name: &str, role: WireBufferRole, lifetime: WireBufferLifetime| {
        WireBufferIdentity {
            id,
            name: name.to_owned(),
            role,
            storage: WireStorageLayout::DeviceHandle,
            lifetime,
            semantic_value: id,
        }
    };
    let version = |count| WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: count,
        shape: None,
        reduced_projection: None,
    };
    let resource = |buffer, access, initialization| {
        let count = 4;
        WireDeviceResource {
            buffer,
            version: version(count),
            binding: WireBinding {
                group: 0,
                binding: 0,
            },
            access,
            generation: 1,
            initialization,
        }
    };
    let launch = || WireKernelLaunchPlan {
        workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
        dispatch_size: WireDispatchSize { x: 4, y: 1, z: 1 },
        workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
    };
    let a = buffer(1, "a", WireBufferRole::InOut, WireBufferLifetime::PerStep);
    let b = buffer(2, "b", WireBufferRole::InOut, WireBufferLifetime::PerStep);
    let c = buffer(3, "c", WireBufferRole::InOut, WireBufferLifetime::PerStep);
    let d = buffer(
        4,
        "d",
        WireBufferRole::Output,
        WireBufferLifetime::ObservationPoint,
    );
    let kernel = |function: u32, entry: &str, resources: Vec<WireDeviceResource>| WireKernelUnit {
        function,
        entry: entry.to_owned(),
        plan: WireCollectionKernelPlan::Elementwise,
        resources,
        launch: launch(),
    };
    WireDeviceProgram {
        kernels: vec![
            kernel(
                0,
                "child__0",
                vec![resource(
                    a.clone(),
                    WireResourceAccess::Write,
                    WireInitializationPolicy::KernelInitialized,
                )],
            ),
            kernel(
                1,
                "child__1",
                vec![
                    resource(
                        a.clone(),
                        WireResourceAccess::Read,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                    resource(
                        b.clone(),
                        WireResourceAccess::Write,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                ],
            ),
            kernel(
                2,
                "child__2",
                vec![
                    resource(
                        b.clone(),
                        WireResourceAccess::Read,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                    resource(
                        c.clone(),
                        WireResourceAccess::Write,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                ],
            ),
            kernel(
                3,
                "child__3",
                vec![
                    resource(
                        c.clone(),
                        WireResourceAccess::Read,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                    resource(
                        d.clone(),
                        WireResourceAccess::Write,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                ],
            ),
        ],
        launches: (1..=4)
            .zip(0..4)
            .map(|(id, kernel_index)| WireLaunchUnit { id, kernel_index })
            .collect(),
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: d,
            version: version(4),
            role: WireBufferRole::Output,
            produced_by: 4,
            observation: WireObservationFact {
                at_launch: 4,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values: (1..=4)
            .map(|id| WireSemanticValue {
                id,
                name: format!("value_{id}"),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: format!("child-{id}-output"),
                },
            })
            .collect(),
        roots: vec![1],
        dependencies: vec![
            WireDependencyEdge {
                producer: 1,
                consumer: 2,
                buffer: 1,
                version: 1,
            },
            WireDependencyEdge {
                producer: 2,
                consumer: 3,
                buffer: 2,
                version: 1,
            },
            WireDependencyEdge {
                producer: 3,
                consumer: 4,
                buffer: 3,
                version: 1,
            },
        ],
        relations: Vec::new(),
    }
}

#[test]
fn v10_generic_four_kernel_program_round_trips_with_identical_identity() {
    let program = four_kernel_chain_program();
    let mut section = sample_device_section();
    section.device_program.program = program.clone();
    section.device_program.v = WIRE_VERSION;

    let text_image = text_image_with_device(Some(section.clone()));
    let text = encode_text_image(&text_image).expect("encode v10 text image");
    let text_decoded = decode_text_image(&text, TOOLCHAIN).expect("decode v10 text image");
    let text_program = text_decoded
        .device
        .expect("text device section")
        .device_program
        .program;

    let binary_image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&binary_image).expect("encode v10 binary image");
    let binary_decoded = decode_binary_image(&bytes, TOOLCHAIN).expect("decode v10 binary image");
    let binary_program = binary_decoded
        .device
        .expect("binary device section")
        .device_program
        .program;

    assert_eq!(text_program.kernels.len(), 4);
    assert_eq!(text_program, program);
    assert_eq!(binary_program, program);
    assert_eq!(
        canonical_program_bytes(&text_program),
        canonical_program_bytes(&binary_program),
        "text and binary codecs preserve one canonical generic multi-child identity"
    );
    let sources = ["fnv64:four-child"];
    assert_eq!(
        device_identity_hash(&sources, &canonical_program_bytes(&text_program)),
        device_identity_hash(&sources, &canonical_program_bytes(&binary_program)),
        "text and binary codecs preserve the same device identity"
    );
}

fn sample_relation() -> WireCompanionRelation {
    WireCompanionRelation {
        primal: 1,
        companion: 2,
        derivative: WireCompanionDerivativeKind::ReverseModeVjp,
        device_resident: true,
        selected_inputs: vec![WireCompanionSelectedInput {
            param: 1,
            position: 0,
            ty: 1,
            gradient_slot: 0,
        }],
        selected_outputs: vec![WireCompanionSelectedOutput {
            position: 0,
            ty: 1,
            upstream_gradient_ty: 1,
        }],
    }
}

#[test]
fn two_kernel_chain_program_admits_cleanly() {
    // Sanity: the two-kernel chain fixture (used to probe the graph rules) is
    // itself valid — decode admits it.
    let mut section = sample_device_section();
    section.device_program.program = two_kernel_chain_program();
    let image = text_image_with_device(Some(section));
    let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .expect("the two-kernel chain admits cleanly");
    assert_eq!(
        decoded
            .device
            .unwrap()
            .device_program
            .program
            .launches
            .len(),
        2
    );
}

// ---------------------------------------------------------------------------
// S5-U5b: the declared RepeatingStep step count rides the wire
// ---------------------------------------------------------------------------

#[test]
fn repeating_step_declared_count_round_trips_on_the_wire() {
    // The versioned step-count carriage: a RepeatingStep program carries its
    // declared count in the lifetime variant, and the count survives the
    // text codec round-trip — an image-loaded route recovers it without any
    // env-var authority.
    let mut program = sample_wire_program();
    program.lifetime = WireProgramLifetime::RepeatingStep(250);
    let mut section = sample_device_section();
    section.device_program.program = program;
    let image = text_image_with_device(Some(section));
    let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .expect("a RepeatingStep program with a declared count admits cleanly");
    assert_eq!(
        decoded.device.unwrap().device_program.program.lifetime,
        WireProgramLifetime::RepeatingStep(250),
        "the declared step count survives the wire round-trip"
    );
}

#[test]
fn repeating_step_zero_count_fails_admission_closed() {
    // Fail-closed admission: a RepeatingStep program that declares zero steps
    // is a contradiction (a count the route could never drive) and fails at
    // the decode boundary before host construction.
    let error = wire_admission_error(|program| {
        program.lifetime = WireProgramLifetime::RepeatingStep(0);
    })
    .expect_err("a zero-step RepeatingStep program must fail admission");
    assert!(
        matches!(error, FmirImageError::WireProgramInvalid { .. }),
        "the zero-step count fails with the focused wire-admission diagnostic: {error:?}"
    );
}

#[test]
fn repeating_step_count_rides_the_canonical_identity_bytes() {
    // The declared count is a semantic fact: it is part of the canonical
    // program bytes (A10) — two programs differing only in the declared count
    // derive different identity substrates.
    let mut program = sample_wire_program();
    program.lifetime = WireProgramLifetime::RepeatingStep(100);
    let base = canonical_program_bytes(&program);
    let mut mutated = program.clone();
    mutated.lifetime = WireProgramLifetime::RepeatingStep(250);
    assert_ne!(
        canonical_program_bytes(&mutated),
        base,
        "the declared step count changes the canonical program bytes"
    );
}

/// The exact wire shape the faber constructor projects after Stage 3R U2:
/// a three-buffer two-kernel chain (`a` → `medius` → `result`) carrying the
/// independent axes — one semantic value per buffer (F1), explicit
/// generations (F2), carried roots + a producer/consumer dependency (F3),
/// per-buffer initialization policies (F5), and explicit observation facts
/// on every result row (F6). This is the ordinary `device-summa-recollige`
/// projection's canonical form.
fn faber_projected_two_kernel_wire() -> WireDeviceProgram {
    let a = WireBufferIdentity {
        id: 1,
        name: "a".to_owned(),
        role: WireBufferRole::Input,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerProgram,
        semantic_value: 1,
    };
    let medius = WireBufferIdentity {
        id: 2,
        name: "medius".to_owned(),
        role: WireBufferRole::InOut,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerStep,
        semantic_value: 2,
    };
    let result = WireBufferIdentity {
        id: 3,
        name: "result".to_owned(),
        role: WireBufferRole::Output,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::ObservationPoint,
        semantic_value: 3,
    };
    let version = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: 4,
        shape: None,
        reduced_projection: None,
    };
    WireDeviceProgram {
        kernels: vec![
            WireKernelUnit {
                function: 0,
                entry: "collige".to_owned(),
                plan: WireCollectionKernelPlan::Elementwise,
                resources: vec![
                    WireDeviceResource {
                        buffer: a.clone(),
                        version: version.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 0,
                        },
                        access: WireResourceAccess::Read,
                        generation: 1,
                        initialization: WireInitializationPolicy::HostProvided,
                    },
                    WireDeviceResource {
                        buffer: medius.clone(),
                        version: version.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 1,
                        },
                        access: WireResourceAccess::Write,
                        generation: 1,
                        initialization: WireInitializationPolicy::KernelInitialized,
                    },
                ],
                launch: WireKernelLaunchPlan {
                    workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
                    dispatch_size: WireDispatchSize { x: 4, y: 1, z: 1 },
                    workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
                },
            },
            WireKernelUnit {
                function: 1,
                entry: "recollige".to_owned(),
                plan: WireCollectionKernelPlan::Elementwise,
                resources: vec![
                    WireDeviceResource {
                        buffer: medius.clone(),
                        version: version.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 0,
                        },
                        access: WireResourceAccess::Read,
                        generation: 1,
                        initialization: WireInitializationPolicy::KernelInitialized,
                    },
                    WireDeviceResource {
                        buffer: result.clone(),
                        version: version.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 1,
                        },
                        access: WireResourceAccess::Write,
                        generation: 1,
                        initialization: WireInitializationPolicy::KernelInitialized,
                    },
                ],
                launch: WireKernelLaunchPlan {
                    workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
                    dispatch_size: WireDispatchSize { x: 4, y: 1, z: 1 },
                    workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
                },
            },
        ],
        launches: vec![
            WireLaunchUnit {
                id: 1,
                kernel_index: 0,
            },
            WireLaunchUnit {
                id: 2,
                kernel_index: 1,
            },
        ],
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![
            WireResultBuffer {
                buffer: medius.clone(),
                version: version.clone(),
                role: WireBufferRole::InOut,
                produced_by: 1,
                observation: WireObservationFact {
                    at_launch: 1,
                    cadence: WireObservationCadence::EndOfRun,
                },
            },
            WireResultBuffer {
                buffer: result.clone(),
                version: version.clone(),
                role: WireBufferRole::Output,
                produced_by: 2,
                observation: WireObservationFact {
                    at_launch: 2,
                    cadence: WireObservationCadence::PerStep,
                },
            },
        ],
        semantic_values: vec![
            WireSemanticValue {
                id: 1,
                name: "a".to_owned(),
                origin: WireSemanticValueOrigin::HostInput,
            },
            WireSemanticValue {
                id: 2,
                name: "medius".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "buffer-2".to_owned(),
                },
            },
            WireSemanticValue {
                id: 3,
                name: "result".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "buffer-3".to_owned(),
                },
            },
        ],
        roots: vec![1],
        dependencies: vec![WireDependencyEdge {
            producer: 1,
            consumer: 2,
            buffer: 2,
            version: 1,
        }],
        relations: Vec::new(),
    }
}

#[test]
fn faber_projected_two_kernel_wire_admits_cleanly() {
    // The ordinary two-kernel constructor projection (Stage 3R U2 axes) must
    // pass the radix F1–F7 wire admission as-is — the decode boundary is the
    // agreement point between the constructor and the hosts.
    let mut section = sample_device_section();
    section.device_program.program = faber_projected_two_kernel_wire();
    let image = text_image_with_device(Some(section));
    decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .expect("the faber two-kernel projection admits cleanly");
}

// ---------------------------------------------------------------------------
// Stage 3R F1–F7 wire admission negatives (fail-closed validators)
// ---------------------------------------------------------------------------

#[test]
fn decode_rejects_duplicate_relation_rows_on_wire() {
    // F4/S7: duplicate or contradictory relation rows fail closed at the
    // radix decode boundary.
    let result = wire_admission_error(|program| {
        let relation = sample_relation();
        program.relations = vec![relation.clone(), relation];
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("duplicate or contradictory")
    ));
}

#[test]
fn decode_rejects_contradictory_relation_rows_on_wire() {
    let result = wire_admission_error(|program| {
        let contradictory = sample_relation();
        let mut relation = sample_relation();
        relation.companion = 3;
        program.relations = vec![relation, contradictory];
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("duplicate or contradictory")
    ));
}

#[test]
fn decode_rejects_second_generation_producer_on_wire() {
    // F2: "another producer of generation 1" is rejected — a later write is a
    // new generation.
    let result = wire_admission_error(|program| {
        let duplicate_slot = program.kernels[0].resources[1].clone();
        program.kernels[0].resources.push(duplicate_slot);
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("second producer")
    ));
}

#[test]
fn decode_rejects_zero_generation_on_wire() {
    // F2: generation 0 is the reserved unset sentinel; an unset generation is
    // rejected.
    let result = wire_admission_error(|program| {
        program.kernels[0].resources[1].generation = 0;
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("generation 0")
    ));
}

#[test]
fn decode_rejects_dependency_cycle_on_wire() {
    // F3: validators enforce acyclicity before launch.
    let result = wire_admission_error(|program| {
        *program = two_kernel_chain_program();
        program.dependencies = vec![
            WireDependencyEdge {
                producer: 1,
                consumer: 2,
                buffer: 1,
                version: 1,
            },
            WireDependencyEdge {
                producer: 2,
                consumer: 1,
                buffer: 2,
                version: 1,
            },
        ];
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("cycle")
    ));
}

#[test]
fn decode_rejects_missing_dependency_on_wire() {
    // F3: a generation producer not reachable from any root is a missing
    // dependency — it could never be scheduled.
    let result = wire_admission_error(|program| {
        *program = two_kernel_chain_program();
        program.dependencies.clear();
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("not reachable from any root")
    ));
}

#[test]
fn decode_rejects_illegal_root_on_wire() {
    // F3: roots are carried launch ids; a root naming no launch is illegal.
    let result = wire_admission_error(|program| {
        program.roots = vec![9];
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("root 9 is not a declared launch")
    ));
}

#[test]
fn decode_rejects_result_without_explicit_observation_fact_on_wire() {
    // F6 red test (U2): a writable intermediate exposed as a result whose
    // explicit observation fact contradicts its producing launch is "without
    // an explicit observation fact" — rejected at the decode boundary, before
    // host construction.
    let result = wire_admission_error(|program| {
        *program = two_kernel_chain_program();
        // The chain's only result names producing launch 2; its observation
        // fact must name the same boundary — naming launch 1 instead is a
        // contradictory observation fact.
        program.results[0].observation.at_launch = 1;
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("observation fact")
                && detail.contains("producing launch")
    ));
}

#[test]
fn decode_rejects_aliased_semantic_origins_on_wire() {
    // F1: two distinct value ids minted from one carried origin would alias.
    let result = wire_admission_error(|program| {
        program.semantic_values.push(WireSemanticValue {
            id: 3,
            name: "x-again".to_owned(),
            origin: WireSemanticValueOrigin::HostInput,
        });
    });
    assert!(matches!(
        result,
        Err(FmirImageError::WireProgramInvalid { detail })
            if detail.contains("both derive from origin")
    ));
}

#[test]
fn same_name_same_shape_values_with_distinct_origins_admit_on_wire() {
    // F1: two unrelated same-name values with distinct origins are distinct
    // and admit — never aliased.
    let result = wire_admission_error(|program| {
        program.semantic_values[1] = WireSemanticValue {
            id: 2,
            name: "x".to_owned(),
            origin: WireSemanticValueOrigin::MirLocal {
                function: 0,
                local: 1,
            },
        };
        program.semantic_values[0] = WireSemanticValue {
            id: 1,
            name: "x".to_owned(),
            origin: WireSemanticValueOrigin::MirLocal {
                function: 0,
                local: 0,
            },
        };
    });
    assert!(
        result.is_ok(),
        "distinct origins must never alias on the wire: {result:?}"
    );
}

#[test]
fn wire_version_2_payload_fails_faber_admission_closed() {
    // The faber-owned wire codec bumps in lockstep (v3 → v4, Stage 3R U1): a
    // section still carrying the pre-break v3 wire version is an old payload
    // and must fail the nested admission closed. (The v2 spelling from the
    // pre-3R era is covered by the same gate: any value ≠ 4 rejects.)
    let mut section = sample_device_section();
    section.device_program.v = 2;
    assert_ne!(section.device_program.v, WIRE_VERSION);
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::DeviceProgramVersionMismatch { actual, expected })
            if actual == 2 && expected == WIRE_DEVICE_PROGRAM_VERSION
    ));
}

#[test]
fn device_identity_hash_is_sensitive_to_source_order() {
    // The length-prefixed canonicalization must distinguish source order: a
    // different order is a different package identity.
    let payload = canonical_program_bytes(&sample_wire_program());
    let ab = device_identity_hash(&["fnv64:aaaa", "fnv64:bbbb"], &payload);
    let ba = device_identity_hash(&["fnv64:bbbb", "fnv64:aaaa"], &payload);
    assert_ne!(ab, ba);
}

#[test]
fn device_identity_hash_is_sensitive_to_payload() {
    let sources = &["fnv64:aaaa"];
    assert_ne!(
        device_identity_hash(sources, b"{\"a\":1}"),
        device_identity_hash(sources, b"{\"a\":2}")
    );
}

#[test]
fn device_identity_hash_is_collision_free_across_concatenation() {
    // Length-prefixing prevents ("ab", "cd") from colliding with ("a", "bcd").
    let payload = b"";
    assert_ne!(
        device_identity_hash(&["ab", "cd"], payload),
        device_identity_hash(&["a", "bcd"], payload)
    );
}

#[test]
fn text_encoded_artifact_with_computed_digests_admits_cleanly() {
    // Positive admission: a text-encoded artifact whose encoding declaration
    // is canonical UTF-8 and whose content/packet digests are the COMPUTED
    // values re-verified at admission (DDCP2-U3).
    let blob = ".version 8.7\n.target sm_120\n";
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![cuda_artifact(blob)];
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    let decoded = decode_binary_image(&bytes, TOOLCHAIN).expect("admits cleanly");
    let expected = sha256_digest(blob.as_bytes());
    assert_eq!(
        decoded.device.unwrap().artifacts.artifact[0].content_sha256,
        expected
    );
}

#[test]
fn packet_digest_re_verified_and_sensitive_to_metadata() {
    // DDCP2-U3: packet_sha256 covers the versioned artifact metadata +
    // content_sha256 (never semantic identity), is re-verified at admission,
    // and is sensitive to metadata changes — a tampered entrypoint symbol
    // fails the packet digest, never the content digest. (DDCP3-U0 note: the
    // admission order runs target/combination validation BEFORE digest
    // re-verification, so a tampered TARGET id is caught by the compiled-in
    // target gate; the entrypoint symbol is metadata only the packet digest
    // covers — the focused packet-digest-gate proof.)
    let blob = ".version 8.7\n.target sm_120\n";
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![cuda_artifact(blob)];
    // Tamper with the ENTRYPOINT SYMBOL only: content is untouched, target/
    // combination/duplicate gates pass, the packet digest no longer matches.
    section.artifacts.artifact[0].entrypoints[0].symbol = "_Z6additaTAMPERED".to_owned();
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::ArtifactDigestMismatch { backend, field, .. }) => {
            assert_eq!(backend, "cuda");
            assert_eq!(field, "packet_sha256");
        }
        other => panic!("expected packet digest mismatch, got {other:?}"),
    }
}

#[test]
fn content_digest_re_verified_over_decoded_bytes() {
    // DDCP2-U3 / contract §IdentityDomains rule 3: digests cover decoded
    // bytes, never the transport spelling — a tampered payload byte changes
    // the content digest and fails admission.
    let mut section = sample_device_section();
    section.artifacts.artifact[0].bytes = b"kernel void sub(...)".to_vec();
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::ArtifactDigestMismatch {
            field: "content_sha256",
            ..
        })
    ));
}

#[test]
fn explicit_selection_without_the_selected_artifact_fails_closed() {
    // DDCP2-U3 selection validation: an explicit Metal selection with no
    // Metal artifact fails closed with the named rule (missing artifact =
    // fail closed, never a silent fallback lane).
    let mut section = sample_device_section();
    section.selection = FmirDeviceSelection::Metal;
    section.artifacts.artifact = vec![cuda_artifact("cuda only")];
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::AbsentArtifactForSelection { selection })
            if selection == "metal"
    ));
}

#[test]
fn finalized_artifact_parent_provenance_is_hash_bound() {
    // DDCP2-U3 / contract §IdentityDomains rule 5: a finalized-binary
    // artifact MUST carry its hash-bound parent compiler_input_packet_sha256
    // (provenance never silently replaced); a compiler-input artifact must
    // not. The parent is verified against the section, not just spelled
    // (P3-1): it must be the packet_sha256 of the compiler-input sibling the
    // binary names — a wrong-parent digest, a missing sibling, or no parent
    // at all fails closed with the named diagnostic.
    let blob = "ptx finalized";

    // The compiler-input sibling the finalized binary names.
    let parent_input = cuda_artifact(".version 8.7\n.target sm_120\n.entry add() {}\n");
    let parent_packet = parent_input.packet_sha256.clone();

    // The canonical chain: sibling + finalized naming its packet → admits.
    let mut finalized = cuda_artifact(blob);
    finalized.stage = MaterializationStage::FinalizedBinary;
    finalized.compiler_input_packet_sha256 = Some(parent_packet.clone());
    finalized.packet_sha256 = finalized.compute_packet_sha256();
    let mut chain = sample_device_section();
    chain.artifacts.artifact = vec![parent_input.clone(), finalized.clone()];
    let image = binary_image_with_device(Some(chain));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    decode_binary_image(&bytes, TOOLCHAIN)
        .expect("a finalized binary naming its sibling's packet admits");

    // P3-1 pin: a well-formed `sha256:` parent that is NOT the sibling's
    // packet digest fails closed — wrongness, not just absence.
    let mut wrong_parent = finalized.clone();
    wrong_parent.compiler_input_packet_sha256 = Some(sha256_digest(b"an unrelated packet"));
    wrong_parent.packet_sha256 = wrong_parent.compute_packet_sha256();
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![parent_input.clone(), wrong_parent];
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::CompilerInputProvenance { backend, .. }) if backend == "cuda"
    ));

    // A well-formed parent naming NO sibling in the section fails closed.
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![finalized.clone()];
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::CompilerInputProvenance { backend, .. }) if backend == "cuda"
    ));

    // Finalized without the parent provenance → fail closed.
    finalized.compiler_input_packet_sha256 = None;
    finalized.packet_sha256 = finalized.compute_packet_sha256();
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![finalized];
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::CompilerInputProvenance { backend, .. }) if backend == "cuda"
    ));
}

// ---------------------------------------------------------------------------
// S6-C2: the typed broadcast fact + the recipe-variant wire mirrors
// ---------------------------------------------------------------------------

/// S6-C2: the typed rank-extension broadcast fact round-trips through the
/// text codec (the wire carries it under the versioned bump, A7).
#[test]
fn rank_extension_broadcast_fact_round_trips_on_the_wire() {
    let fact = WireBroadcastFact {
        lhs_shape: vec![2, 8],
        rhs_shape: vec![8],
        result_shape: vec![2, 8],
        declaration: WireBroadcastDeclaration::RankExtension,
    };
    let mut program = sample_wire_program();
    program.kernels[0].plan = WireCollectionKernelPlan::RankExtensionAdd(fact.clone());
    let mut section = sample_device_section();
    section.device_program.program = program;
    let image = text_image_with_device(Some(section));
    let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .expect("a kernel carrying a valid rank-extension broadcast fact admits cleanly");
    assert_eq!(
        decoded.device.unwrap().device_program.program.kernels[0].plan,
        WireCollectionKernelPlan::RankExtensionAdd(fact),
        "the typed broadcast fact survives the wire round-trip"
    );
}

/// S6-C2: the S6-N1-frozen recipe variants gain real wire mirrors — each
/// serializes and round-trips through the text codec.
#[test]
fn frozen_recipe_variant_plans_round_trip_on_the_wire() {
    let plans = [
        WireCollectionKernelPlan::AxisReduction(WireAxisReductionPlan {
            op: WireReduceOp::Sum,
            axis: 1,
            projection: WireReducedProjection {
                axis_extent: 8,
                inner_stride: 1,
            },
        }),
        WireCollectionKernelPlan::RowSoftmax(WireRowSoftmaxPlan { axis: 1 }),
        WireCollectionKernelPlan::LayerNormalization(WireLayerNormalizationPlan { axis: 1 }),
    ];
    for plan in plans {
        let mut program = sample_wire_program();
        program.kernels[0].plan = plan.clone();
        let mut section = sample_device_section();
        section.device_program.program = program;
        let image = text_image_with_device(Some(section));
        let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
            .unwrap_or_else(|error| panic!("recipe plan {plan:?} must admit cleanly: {error}"));
        assert_eq!(
            decoded.device.unwrap().device_program.program.kernels[0].plan,
            plan,
            "the recipe plan survives the wire round-trip"
        );
    }
}

/// S6-C2: the broadcast fact rides the canonical identity bytes (A10) — a
/// program carrying the fact derives different canonical bytes.
#[test]
fn broadcast_fact_changes_the_canonical_identity_bytes() {
    let mut program = sample_wire_program();
    let base = canonical_program_bytes(&program);
    program.kernels[0].plan = WireCollectionKernelPlan::RankExtensionAdd(WireBroadcastFact {
        lhs_shape: vec![2, 8],
        rhs_shape: vec![8],
        result_shape: vec![2, 8],
        declaration: WireBroadcastDeclaration::RankExtension,
    });
    assert_ne!(
        canonical_program_bytes(&program),
        base,
        "the typed broadcast fact changes the canonical program bytes"
    );
}

/// S6-C2 (A7): a carried broadcast fact whose lower-rank dims are not the
/// higher-rank trailing dims fails admission closed at the decode boundary.
#[test]
fn mismatched_broadcast_fact_fails_admission_closed() {
    let error = wire_admission_error(|program| {
        program.kernels[0].plan = WireCollectionKernelPlan::RankExtensionAdd(WireBroadcastFact {
            lhs_shape: vec![2, 8],
            rhs_shape: vec![4],
            result_shape: vec![2, 8],
            declaration: WireBroadcastDeclaration::RankExtension,
        });
    })
    .expect_err("a mismatched broadcast fact must fail admission closed");
    assert!(
        matches!(error, FmirImageError::WireProgramInvalid { .. }),
        "the mismatched broadcast fails with the focused wire-admission diagnostic: {error:?}"
    );
    let detail = match error {
        FmirImageError::WireProgramInvalid { detail } => detail,
        other => panic!("unexpected error variant: {other:?}"),
    };
    assert!(
        detail.contains("rank-extension broadcast"),
        "the diagnostic names the broadcast rule: {detail}"
    );
}

/// S6-C2 (A7): a broadcast fact whose result shape does not equal the
/// higher-rank operand shape fails admission closed.
#[test]
fn broadcast_fact_with_wrong_result_shape_fails_admission_closed() {
    let error = wire_admission_error(|program| {
        program.kernels[0].plan = WireCollectionKernelPlan::RankExtensionAdd(WireBroadcastFact {
            lhs_shape: vec![2, 8],
            rhs_shape: vec![8],
            result_shape: vec![2, 4],
            declaration: WireBroadcastDeclaration::RankExtension,
        });
    })
    .expect_err("a broadcast fact with a wrong result shape must fail admission closed");
    assert!(
        matches!(error, FmirImageError::WireProgramInvalid { .. }),
        "the wrong-result-shape broadcast fails with the focused wire-admission diagnostic: {error:?}"
    );
}

/// GI3-1: the inference-recipe wire mirrors (Gather / `RmsNormalization` /
/// Rope / `CausalMaskedSoftmax`) serialize and round-trip through the text
/// codec under the accepted wire version — the F1 mechanical mirror arms,
/// no `FmirDeviceSection` field change, no version bump.
#[test]
fn gi3_inference_recipe_plans_round_trip_on_the_wire() {
    let plans = [
        WireCollectionKernelPlan::Gather(WireGatherPlan {
            table_rows: 960,
            table_cols: 49_152,
            id_count: 9,
        }),
        WireCollectionKernelPlan::RmsNormalization(WireRmsNormalizationPlan {
            axis: 1,
            epsilon_bits: 1e-5_f32.to_bits(),
            width: 960,
        }),
        WireCollectionKernelPlan::Rope(WireRopePlan {
            pos: 0,
            dim: 64,
            width: 64,
            per_row: false,
            rows: 0,
            rotate_half: false,
        }),
        WireCollectionKernelPlan::CausalMaskedSoftmax(WireCausalMaskedSoftmaxPlan {
            rows: 9,
            cols: 9,
        }),
    ];
    for plan in plans {
        let mut program = sample_wire_program();
        program.kernels[0].plan = plan.clone();
        let mut section = sample_device_section();
        section.device_program.program = program;
        let image = text_image_with_device(Some(section));
        let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
            .unwrap_or_else(|error| panic!("recipe plan {plan:?} must admit cleanly: {error}"));
        assert_eq!(
            decoded.device.unwrap().device_program.program.kernels[0].plan,
            plan,
            "the recipe plan survives the wire round-trip"
        );
    }
}

/// R-PACK-01C: packed-kernel wire mirrors (`QuantizedMatMul` / `GroupedMatMul` /
/// `SsmConv1d` / `SsmScan`) serialize and round-trip through the text codec on
/// the accepted wire version — mechanical field-for-field arms, no extra
/// semantic facts, no version bump.
#[test]
fn r_pack_packed_kernel_plans_round_trip_on_the_wire() {
    assert_eq!(WIRE_DEVICE_PROGRAM_VERSION, 14);
    let plans = [
        WireCollectionKernelPlan::QuantizedMatMul(WireQuantizedMatMulPlan {
            m: 8,
            k: 16,
            n: 16,
            layout: WirePackedStorageLayout::Q4_K,
            tile: 8,
            workgroup_x: 8,
            workgroup_y: 8,
        }),
        WireCollectionKernelPlan::GroupedMatMul(WireGroupedMatMulPlan {
            m: 8,
            k: 16,
            n: 16,
            groups: 4,
            layout: WirePackedStorageLayout::Q4_K,
            tile: 8,
            workgroup_x: 8,
            workgroup_y: 8,
        }),
        WireCollectionKernelPlan::SsmConv1d(WireSsmConv1dPlan {
            length: 8,
            channels: 4,
            kernel_width: 3,
            layout: WirePackedStorageLayout::F32,
        }),
        WireCollectionKernelPlan::SsmScan(WireSsmScanPlan {
            length: 8,
            state_dim: 4,
            layout: WirePackedStorageLayout::F32,
        }),
    ];
    for plan in plans {
        let mut program = sample_wire_program();
        program.kernels[0].plan = plan.clone();
        let mut section = sample_device_section();
        section.device_program.program = program;
        let image = text_image_with_device(Some(section));
        let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
            .unwrap_or_else(|error| panic!("packed plan {plan:?} must admit cleanly: {error}"));
        assert_eq!(
            decoded.device.unwrap().device_program.program.kernels[0].plan,
            plan,
            "the packed-kernel plan survives the wire round-trip"
        );
    }
}

/// U1 head-axis: the batched tiled-matmul wire plan (leading head axes,
/// prefix-shared right batch) round-trips through the text codec on the
/// accepted wire version and decodes to a `TiledMatMul` plan carrying the
/// batch facts — a batched image never decodes as rank-2 by omission, and
/// the rank-2 `TiledMatMul` image is unchanged. The variant landed inside
/// wire-10 un-ratcheted; the S2 clean break records it at wire 11.
#[test]
fn batched_matmul_wire_plan_round_trips_with_batch_facts() {
    assert_eq!(WIRE_DEVICE_PROGRAM_VERSION, 14);
    let shared = || WireMatMulSharedMemory {
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
    };
    let barrier = WireBarrierPoint {
        after: WireBarrierPhase::SharedMemoryLoad,
    };
    // Decode attention: [5,3,1,64] · [5,64,76] → [5,3,1,76].
    let decode = WireCollectionKernelPlan::BatchedTiledMatMul(WireBatchedMatMulPlan {
        m: 1,
        k: 64,
        n: 76,
        left_layout: WireMatMulLayout::F32,
        right_layout: WireMatMulLayout::F32,
        batch: 15,
        right_batch: 5,
        tile: 8,
        workgroup_x: 8,
        workgroup_y: 8,
        shared_memory: shared(),
        barriers: vec![barrier.clone()],
        oob_padding: WireOobPaddingPolicy::ZeroFill,
    });
    // Prefill attention: [5,3,36,64] · [5,64,36] → [5,3,36,36].
    let prefill = WireCollectionKernelPlan::BatchedTiledMatMul(WireBatchedMatMulPlan {
        m: 36,
        k: 64,
        n: 36,
        left_layout: WireMatMulLayout::F32,
        right_layout: WireMatMulLayout::F32,
        batch: 15,
        right_batch: 5,
        tile: 8,
        workgroup_x: 8,
        workgroup_y: 8,
        shared_memory: shared(),
        barriers: vec![barrier],
        oob_padding: WireOobPaddingPolicy::ZeroFill,
    });
    for plan in [decode, prefill] {
        let mut program = sample_wire_program();
        program.kernels[0].plan = plan.clone();
        let mut section = sample_device_section();
        section.device_program.program = program;
        let image = text_image_with_device(Some(section));
        let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
            .unwrap_or_else(|error| panic!("batched plan {plan:?} must admit cleanly: {error}"));
        assert_eq!(
            decoded.device.unwrap().device_program.program.kernels[0].plan,
            plan,
            "the batched tiled-matmul plan survives the wire round-trip"
        );
    }
}

/// A `BatchedTiledMatMul` wire plan violating the batch law (a `right_batch`
/// that does not divide batch) fails wire admission typed, never decodes.
#[test]
fn batched_matmul_wire_plan_batch_law_fails_closed() {
    let mut program = sample_wire_program();
    program.kernels[0].plan = WireCollectionKernelPlan::BatchedTiledMatMul(WireBatchedMatMulPlan {
        m: 1,
        k: 64,
        n: 76,
        left_layout: WireMatMulLayout::F32,
        right_layout: WireMatMulLayout::F32,
        batch: 15,
        right_batch: 4,
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
        barriers: Vec::new(),
        oob_padding: WireOobPaddingPolicy::ZeroFill,
    });
    let mut section = sample_device_section();
    section.device_program.program = program;
    let image = text_image_with_device(Some(section));
    let error = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .expect_err("a batch-law violation must fail wire admission");
    assert!(
        error.to_string().contains("batch law"),
        "expected the typed batch-law diagnostic, got: {error}"
    );
}

/// U2 head-axis: the batched row-softmax wire plan (leading head/query
/// axes, additive mask count kept) round-trips through the text codec on
/// the accepted wire version — a batched image never decodes as the
/// rank-1/2 `RowSoftmax` by omission, and the legacy image is unchanged.
/// The variant landed inside wire-10 un-ratcheted; the S2 clean break
/// records it at wire 11.
#[test]
fn batched_softmax_wire_plan_round_trips_with_batch_facts() {
    assert_eq!(WIRE_DEVICE_PROGRAM_VERSION, 14);
    // Decode attention: scores [5,3,1,76] + declared-length mask [1,76].
    let decode = WireCollectionKernelPlan::BatchedRowSoftmax(WireBatchedRowSoftmaxPlan {
        axis: 3,
        batch: 15,
        width: 76,
        mask_count: Some(76),
    });
    // Prefill attention: scores [5,3,36,36] + causal mask [36,36]
    // (15 heads × 36 query rows = 540 rows of width 36).
    let prefill = WireCollectionKernelPlan::BatchedRowSoftmax(WireBatchedRowSoftmaxPlan {
        axis: 3,
        batch: 15 * 36,
        width: 36,
        mask_count: Some(36 * 36),
    });
    // A plain batched softmax (no mask operand) round-trips with None.
    let plain = WireCollectionKernelPlan::BatchedRowSoftmax(WireBatchedRowSoftmaxPlan {
        axis: 3,
        batch: 15,
        width: 76,
        mask_count: None,
    });
    for plan in [decode, prefill, plain] {
        let mut program = sample_wire_program();
        program.kernels[0].plan = plan.clone();
        let mut section = sample_device_section();
        section.device_program.program = program;
        let image = text_image_with_device(Some(section));
        let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
            .unwrap_or_else(|error| {
                panic!("batched softmax plan {plan:?} must admit cleanly: {error}")
            });
        assert_eq!(
            decoded.device.unwrap().device_program.program.kernels[0].plan,
            plan,
            "the batched row-softmax plan survives the wire round-trip"
        );
    }
}

/// A `BatchedRowSoftmax` wire plan violating the batch law (a mask count
/// that does not divide batch·width) fails wire admission typed, never
/// decodes.
#[test]
fn batched_softmax_wire_plan_batch_law_fails_closed() {
    let mut program = sample_wire_program();
    program.kernels[0].plan =
        WireCollectionKernelPlan::BatchedRowSoftmax(WireBatchedRowSoftmaxPlan {
            axis: 3,
            batch: 15,
            width: 76,
            mask_count: Some(75),
        });
    let mut section = sample_device_section();
    section.device_program.program = program;
    let image = text_image_with_device(Some(section));
    let error = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .expect_err("a batch-law violation must fail wire admission");
    assert!(
        error.to_string().contains("batch law"),
        "expected the typed batch-law diagnostic, got: {error}"
    );
}

/// U3 head-axis: the batched transpose wire plan (leading head axes over
/// the swapped trailing [M, N]) round-trips through the text codec on the
/// accepted wire version — a batched image never decodes as the rank-2
/// `Transpose` by omission, and the legacy image is unchanged. The
/// variant landed inside wire-10 un-ratcheted; the S2 clean break
/// records it at wire 11.
#[test]
fn batched_transpose_wire_plan_round_trips_with_batch_facts() {
    assert_eq!(WIRE_DEVICE_PROGRAM_VERSION, 14);
    // Decode attention: K [5,76,64] → Kᵀ [5,64,76] over the row-major
    // [76,320] arena.
    let decode = WireCollectionKernelPlan::BatchedTranspose(WireBatchedTransposePlan {
        batch: 5,
        m: 76,
        n: 64,
        workgroup_x: 64,
        dispatch_x: 5 * 76 * 64,
    });
    // Prefill attention: K [5,36,64] → Kᵀ [5,64,36].
    let prefill = WireCollectionKernelPlan::BatchedTranspose(WireBatchedTransposePlan {
        batch: 5,
        m: 36,
        n: 64,
        workgroup_x: 64,
        dispatch_x: 5 * 36 * 64,
    });
    for plan in [decode, prefill] {
        let mut program = sample_wire_program();
        program.kernels[0].plan = plan.clone();
        let mut section = sample_device_section();
        section.device_program.program = program;
        let image = text_image_with_device(Some(section));
        let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
            .unwrap_or_else(|error| {
                panic!("batched transpose plan {plan:?} must admit cleanly: {error}")
            });
        assert_eq!(
            decoded.device.unwrap().device_program.program.kernels[0].plan,
            plan,
            "the batched transpose plan survives the wire round-trip"
        );
    }
}

/// A `BatchedTranspose` wire plan violating the batch law (a dispatch
/// extent that does not tile batch·M·N) fails wire admission typed, never
/// decodes.
#[test]
fn batched_transpose_wire_plan_batch_law_fails_closed() {
    let mut program = sample_wire_program();
    program.kernels[0].plan =
        WireCollectionKernelPlan::BatchedTranspose(WireBatchedTransposePlan {
            batch: 5,
            m: 76,
            n: 64,
            workgroup_x: 64,
            dispatch_x: 76 * 64,
        });
    let mut section = sample_device_section();
    section.device_program.program = program;
    let image = text_image_with_device(Some(section));
    let error = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .expect_err("a batch-law violation must fail wire admission");
    assert!(
        error.to_string().contains("batch law"),
        "expected the typed batch-law diagnostic, got: {error}"
    );
}

/// gpu-reset C4-1: the lane-grid plan mirror serializes and round-trips
/// through the text codec on the accepted wire version.
#[test]
fn lane_grid_plan_round_trips_on_the_wire() {
    let plan = WireCollectionKernelPlan::LaneGrid(crate::schema::WireLaneGridPlan {
        source_resource: 0,
        rank: 2,
    });
    let mut program = sample_wire_program();
    program.kernels[0].plan = plan.clone();
    let mut section = sample_device_section();
    section.device_program.program = program;
    let image = text_image_with_device(Some(section));
    let decoded = decode_text_image(&encode_text_image(&image).unwrap(), TOOLCHAIN)
        .unwrap_or_else(|error| panic!("lane grid plan must admit cleanly: {error}"));
    assert_eq!(
        decoded.device.unwrap().device_program.program.kernels[0].plan,
        plan
    );
}
