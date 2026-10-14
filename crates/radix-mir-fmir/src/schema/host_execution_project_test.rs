//! HEP-3B tests for the fail-closed Radix physical-projection boundary.

use super::*;

fn artifact() -> FmirDeviceArtifact {
    let mut artifact = FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        format: DeviceArtifactFormat::Msl,
        stage: MaterializationStage::CompilerInput,
        target: DeviceTargetId {
            id: "macos-arm64".to_owned(),
            required_features: Vec::new(),
        },
        abi_version: ARTIFACT_ABI_VERSION,
        bytes: b"kernel void mystery_backend_symbol() {}".to_vec(),
        encoding: DevicePayloadEncoding::Text,
        entrypoints: vec![FmirDeviceSymbol {
            entry: "logical_kernel".to_owned(),
            symbol: "mystery_backend_symbol".to_owned(),
        }],
        content_sha256: String::new(),
        packet_sha256: String::new(),
        compiler_input_packet_sha256: None,
        reflection: None,
    };
    artifact.content_sha256 = artifact.compute_content_sha256();
    artifact.packet_sha256 = artifact.compute_packet_sha256();
    artifact
}

fn device() -> FmirDeviceSection {
    let input = WireDeviceResource {
        buffer: WireBufferIdentity {
            id: 10,
            name: "semantic_input".to_owned(),
            role: WireBufferRole::Input,
            storage: WireStorageLayout::DeviceHandle,
            lifetime: WireBufferLifetime::PerProgram,
            semantic_value: 10,
        },
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
    };
    let output = WireDeviceResource {
        buffer: WireBufferIdentity {
            id: 20,
            name: "semantic_output".to_owned(),
            role: WireBufferRole::Output,
            storage: WireStorageLayout::DeviceHandle,
            lifetime: WireBufferLifetime::ObservationPoint,
            semantic_value: 20,
        },
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
    };
    let artifact = artifact();
    FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: WIRE_DEVICE_PROGRAM_VERSION,
            program: WireDeviceProgram {
                kernels: vec![WireKernelUnit {
                    function: 0,
                    entry: "logical_kernel".to_owned(),
                    plan: WireCollectionKernelPlan::Elementwise,
                    resources: vec![input, output.clone()],
                    launch: WireKernelLaunchPlan {
                        workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
                        dispatch_size: WireDispatchSize { x: 4, y: 1, z: 1 },
                        workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
                    },
                }],
                launches: vec![WireLaunchUnit {
                    id: 7,
                    kernel_index: 0,
                }],
                lifetime: WireProgramLifetime::SingleRun,
                results: vec![WireResultBuffer {
                    buffer: output.buffer.clone(),
                    version: output.version.clone(),
                    role: WireBufferRole::Output,
                    produced_by: 7,
                    observation: WireObservationFact {
                        at_launch: 7,
                        cadence: WireObservationCadence::PerStep,
                    },
                }],
                semantic_values: vec![
                    WireSemanticValue {
                        id: 10,
                        name: "semantic_input".to_owned(),
                        origin: WireSemanticValueOrigin::HostInput,
                    },
                    WireSemanticValue {
                        id: 20,
                        name: "semantic_output".to_owned(),
                        origin: WireSemanticValueOrigin::Synthetic {
                            label: "semantic-output".to_owned(),
                        },
                    },
                ],
                roots: vec![7],
                dependencies: Vec::new(),
                relations: Vec::new(),
            },
        },
        executor_plan: None,
        selection: FmirDeviceSelection::Metal,
        artifacts: FmirDeviceArtifactsSection {
            artifact: vec![artifact],
        },
        declared_inputs: vec![FmirDeviceInput {
            name: "semantic_input".to_owned(),
            values: vec![0.0; 4],
        }],
        distributed: None,
        session: None,
    }
}

#[test]
fn projector_fails_closed_before_packet_emission_without_an_allocation_view_plan() {
    let device = device();

    assert!(matches!(
        project_host_execution_v1(&device, &device.artifacts.artifact[0]),
        Err(HostExecutionProjectionError::MissingPhysicalFact { fact })
            if fact == "admitted physical allocation/view plan"
    ));
}

#[test]
fn projector_requires_a_final_artifact_entry_before_reporting_the_physical_gap() {
    let mut device = device();
    let artifact = &mut device.artifacts.artifact[0];
    artifact.entrypoints.clear();
    artifact.packet_sha256 = artifact.compute_packet_sha256();

    assert!(matches!(
        project_host_execution_v1(&device, &device.artifacts.artifact[0]),
        Err(HostExecutionProjectionError::MissingArtifactEntry { entry }) if entry == "logical_kernel"
    ));
}

#[test]
fn projector_uses_reflection_for_the_final_entry_before_reporting_the_physical_gap() {
    let mut device = device();
    let artifact = &mut device.artifacts.artifact[0];
    artifact.entrypoints.clear();
    artifact.reflection = Some(FmirArtifactReflection {
        v: ARTIFACT_REFLECTION_VERSION,
        kernels: vec![FmirReflectedKernel {
            function: 0,
            entry_name: "mystery_backend_symbol".to_owned(),
            resources: Vec::new(),
        }],
    });
    artifact.packet_sha256 = artifact.compute_packet_sha256();

    assert!(matches!(
        project_host_execution_v1(&device, &device.artifacts.artifact[0]),
        Err(HostExecutionProjectionError::MissingPhysicalFact { fact })
            if fact == "admitted physical allocation/view plan"
    ));
}

#[test]
fn reflected_kernel_resolves_by_function_id_not_entry_name() {
    // G-06: the reflected kernel's identity is the typed function id. Two
    // rows whose entry text collides with the other row's emitted symbol
    // must still resolve to their own symbol by id — a name lookup would
    // pick the wrong row or report a spurious ambiguity.
    let mut device = device();
    let artifact = &mut device.artifacts.artifact[0];
    artifact.entrypoints.clear();
    artifact.reflection = Some(FmirArtifactReflection {
        v: ARTIFACT_REFLECTION_VERSION,
        kernels: vec![
            FmirReflectedKernel {
                function: 1,
                entry_name: "symbol_one".to_owned(),
                resources: Vec::new(),
            },
            FmirReflectedKernel {
                function: 2,
                entry_name: "symbol_two".to_owned(),
                resources: Vec::new(),
            },
        ],
    });
    artifact.packet_sha256 = artifact.compute_packet_sha256();

    assert_eq!(
        resolve_final_entry(artifact, 1, "symbol_two").expect("function 1 resolves"),
        "symbol_one"
    );
    assert_eq!(
        resolve_final_entry(artifact, 2, "symbol_one").expect("function 2 resolves"),
        "symbol_two"
    );
    assert!(matches!(
        resolve_final_entry(artifact, 3, "symbol_one"),
        Err(HostExecutionProjectionError::MissingArtifactEntry { entry }) if entry == "symbol_one"
    ));
}

#[test]
fn projector_rejects_an_empty_final_artifact_entry() {
    let mut device = device();
    let artifact = &mut device.artifacts.artifact[0];
    artifact.entrypoints[0].symbol.clear();
    artifact.packet_sha256 = artifact.compute_packet_sha256();

    assert!(matches!(
        project_host_execution_v1(&device, &device.artifacts.artifact[0]),
        Err(HostExecutionProjectionError::MissingArtifactEntry { entry }) if entry == "logical_kernel"
    ));
}

#[test]
fn projector_rejects_an_artifact_that_disagrees_with_explicit_selection() {
    let device = device();
    let mut wrong_artifact = device.artifacts.artifact[0].clone();
    wrong_artifact.backend = FmirDeviceBackend::Cuda;
    wrong_artifact.target = DeviceTargetId {
        id: "sm_120".to_owned(),
        required_features: vec!["sm_120".to_owned()],
    };
    wrong_artifact.format = DeviceArtifactFormat::Ptx;
    wrong_artifact.bytes =
        b".version 8.7\n.target sm_120\n.entry mystery_backend_symbol() {}".to_vec();
    wrong_artifact.content_sha256 = wrong_artifact.compute_content_sha256();
    wrong_artifact.packet_sha256 = wrong_artifact.compute_packet_sha256();
    let mut device = device;
    device.artifacts.artifact.push(wrong_artifact.clone());

    assert!(matches!(
        project_host_execution_v1(&device, &wrong_artifact),
        Err(HostExecutionProjectionError::ArtifactSelectionMismatch {
            selection,
            artifact_backend,
        }) if selection == "metal" && artifact_backend == "cuda"
    ));
}
