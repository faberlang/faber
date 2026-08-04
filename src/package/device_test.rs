use super::*;
use faber::device::DeviceBackend;
use faber_host_macos_arm64::composite_host::{CompositeHost, DataFlowEdge, ReceiptBuffer};
use faber_host_macos_arm64::device_descriptor::{
    DescriptorBuffer, DescriptorBufferVersion, DescriptorKernel, DescriptorLaunch,
    DescriptorResult, DeviceBufferInitialization, DeviceBufferLifetime, DeviceBufferRole,
    DeviceDataType,
};
use faber_host_macos_arm64::device_host::DeviceRuntime;
use faber_host_macos_arm64::MetalHostSession;
use radix::mir::LoweredMirUnit;
use radix_mir::device_program::DataFlowPair;
use std::path::PathBuf;

fn dev_norma_library_home() -> PathBuf {
    if let Some(home) = std::env::var_os("FABER_LIBRARY_HOME")
        .map(PathBuf::from)
        .filter(|path| path.join("norma/src").exists())
    {
        return home;
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("work/ianzepp"))
        .unwrap_or_else(|_| PathBuf::from("/Users/ianzepp/work/ianzepp"))
}

fn device_program_from_corpus_fixture(relative: &str) -> DeviceProgram {
    device_program_and_semantics_from_corpus_fixture(relative).0
}

fn device_program_and_semantics_from_corpus_fixture(
    relative: &str,
) -> (DeviceProgram, DeviceSemantics) {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../radix/corpus")
        .join(relative);
    super::super::with_lowered_package_mir(
        &radix::driver::Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("kernel fixture yields a device program")
        },
    )
    .expect("fixture lowers")
}

// ── Device-program constructor ─────────────────────────────────────────────

#[test]
fn device_program_constructor_finds_compute_kernel() {
    let program = device_program_from_corpus_fixture("cuda/summa-proof.fab");

    program.validate().expect("constructed program validates");
    assert_eq!(program.kernels.len(), 1);
    let kernel = &program.kernels[0];
    assert_eq!(kernel.entry, "summa");
    // Input buffer + output buffer, in binding order.
    assert_eq!(kernel.resources.len(), 2);
    assert_eq!(kernel.resources[0].buffer.role, BufferRole::Input);
    assert_eq!(kernel.resources[0].version.element_count, 256);
    assert_eq!(kernel.resources[1].buffer.role, BufferRole::Output);
    assert_eq!(kernel.resources[1].version.element_count, 1);
    // S2-4: the constructor derives typed lifetimes from the ABI facts —
    // Input → PerProgram, Output → ObservationPoint (N2.4 default mapping).
    assert_eq!(
        kernel.resources[0].buffer.lifetime,
        BufferLifetime::PerProgram
    );
    assert_eq!(
        kernel.resources[1].buffer.lifetime,
        BufferLifetime::ObservationPoint
    );
    // Reduction of 256 elements with a 256-lane workgroup: one workgroup.
    assert_eq!(kernel.launch.workgroup_count.x, 1);
    assert_eq!(kernel.launch.workgroup.x, 256);
    assert_eq!(program.launches.len(), 1);
    assert_eq!(program.results.len(), 1);
}

#[test]
fn device_program_constructor_returns_none_without_kernels() {
    let entry =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../radix/corpus/literalia/ascii.fab");
    let program = super::super::with_lowered_package_mir(
        &radix::driver::Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
        },
    )
    .expect("fixture lowers");
    assert!(program.is_none(), "no kernel → no device program");
}

/// Build a minimal device section carrying a program's typed wire (no
/// artifacts / inputs) for the codec, admission, and descriptor tests.
fn section_for_program(program: &DeviceProgram, semantics: &DeviceSemantics) -> FmirDeviceSection {
    FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: DEVICE_RUN_PLAN_VERSION,
            program: wire_program_for_program(program, semantics),
        },
        selection: FmirDeviceSelection::Auto,
        artifacts: FmirDeviceArtifactsSection {
            artifact: Vec::new(),
        },
        declared_inputs: Vec::new(),
        runtime_requirements: Vec::new(),
    }
}

/// Build the complete two-kernel fixture (`a` → `medius` → `result`), the
/// ordinary `device-summa-recollige` chain.
fn two_kernel_fixture() -> (DeviceProgram, DeviceSemantics) {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/training/device-summa-recollige/src/device_summa_recollige.fab");
    super::super::with_lowered_package_mir(
        &radix::driver::Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("fixture yields a device program")
        },
    )
    .expect("fixture lowers")
}

/// Build the S2-5 two-kernel fixture's device program (constructor
/// identity-unification test substrate).
fn two_kernel_program() -> DeviceProgram {
    two_kernel_fixture().0
}

/// The typed wire carries the COMPLETE program (S3-A4): kernels (function id,
/// entry, plan, typed resources with access + content version, launch), the
/// ordered launches, the program lifetime, and the explicit result buffers —
/// nothing thinned, nothing dropped.
#[test]
fn wire_carries_the_complete_program() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let wire = wire_program_for_program(&program, &semantics);

    // Kernels: function id + entry + plan + typed resources + launch.
    assert_eq!(wire.kernels.len(), program.kernels.len());
    let kernel = &wire.kernels[0];
    assert_eq!(kernel.function, program.kernels[0].function.0);
    assert_eq!(kernel.entry, "summa");
    // The reduction kernel carries its tree-reduction plan (a program fact,
    // never dropped on the wire).
    assert!(matches!(
        kernel.plan,
        WireCollectionKernelPlan::TreeReduction(_)
    ));
    // Per-resource access + version are distinct fields (N3.4).
    assert_eq!(kernel.resources.len(), 2);
    assert_eq!(kernel.resources[0].access, WireResourceAccess::Read);
    assert_eq!(kernel.resources[0].version.version, 1);
    assert_eq!(kernel.resources[0].version.element_ty, "f32");
    assert_eq!(kernel.launch.workgroup.x, 256);

    // Launches (ordered, may be >1 per kernel).
    assert_eq!(wire.launches.len(), program.launches.len());
    assert_eq!(wire.launches[0].kernel_index, 0);

    // Results carry produced_by.
    assert_eq!(wire.results.len(), program.results.len());
    assert_eq!(
        wire.results[0].produced_by,
        program.results[0].produced_by.0
    );

    // Lifetime regime.
    assert_eq!(wire.lifetime, WireProgramLifetime::SingleRun);
}

/// The constructor-derived lifetimes ride the typed wire (S2-4 + S3-A4): the
/// host receives the carried facts from the image, never re-deriving them.
#[test]
fn constructor_derived_lifetimes_ride_the_typed_wire() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let wire = wire_program_for_program(&program, &semantics);
    let resources = &wire.kernels[0].resources;
    assert_eq!(resources[0].buffer.role, WireBufferRole::Input);
    assert_eq!(resources[0].buffer.lifetime, WireBufferLifetime::PerProgram);
    assert_eq!(resources[1].buffer.role, WireBufferRole::Output);
    assert_eq!(
        resources[1].buffer.lifetime,
        WireBufferLifetime::ObservationPoint
    );
}

/// The wire round-trips deterministically: the same program derives the same
/// canonical complete-program bytes every time (the A10 identity substrate),
/// and CUDA symbols + declared inputs never touch those bytes.
#[test]
fn wire_round_trips_deterministically() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let first = wire_program_for_program(&program, &semantics);
    let second = wire_program_for_program(&program, &semantics);
    assert_eq!(
        radix_mir_fmir::canonical_program_bytes(&first),
        radix_mir_fmir::canonical_program_bytes(&second),
        "identical program → identical complete-program bytes"
    );

    // A section whose metadata differs (symbols + inputs) still derives the
    // same canonical bytes — the identity is over the complete program only.
    let mut section = section_for_program(&program, &semantics);
    section.declared_inputs = vec![FmirDeviceInput {
        name: "a".to_owned(),
        values: vec![1.0],
    }];
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Cuda,
        blob: "ptx".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: vec![FmirDeviceSymbol {
            entry: "summa".to_owned(),
            symbol: "f0".to_owned(),
        }],
    }];
    assert_eq!(
        radix_mir_fmir::canonical_program_bytes(&section.device_program.program),
        radix_mir_fmir::canonical_program_bytes(&first),
        "metadata rides beside the canonical bytes, never inside them"
    );
}

/// Old wire versions fail closed with the structured `payload_version`
/// diagnostic (S3-A4 done-when): the wire-version gate runs before any
/// field-level interpretation.
#[test]
fn wire_v2_fails_closed_with_payload_version_diagnostic() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let mut section = section_for_program(&program, &semantics);
    section.device_program.v = 2;
    let error = admit_device_program_section(&section.device_program)
        .expect_err("v2 wire must fail closed");
    let message = &error[0].message;
    assert!(
        message.contains("version 2 is not supported"),
        "old wires are rejected by the version gate: {message}"
    );
    assert!(
        error[0]
            .args
            .iter()
            .any(|arg| arg.name == "payload_version" && arg.value == "2"),
        "the structured `payload_version` diagnostic names the offending version"
    );
}

/// The CUDA logical-entry → symbol mapping rides the artifact metadata (not
/// the canonical program bytes); the descriptor consumes it per-artifact.
#[test]
fn cuda_descriptor_consumes_artifact_symbol_mapping() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![
        FmirDeviceArtifact {
            backend: FmirDeviceBackend::Metal,
            blob: "msl".to_owned(),
            hash: "fnv64:0000000000000000".to_owned(),
            symbols: Vec::new(),
        },
        FmirDeviceArtifact {
            backend: FmirDeviceBackend::Cuda,
            blob: "ptx".to_owned(),
            hash: "fnv64:0000000000000000".to_owned(),
            symbols: vec![FmirDeviceSymbol {
                entry: "summa".to_owned(),
                symbol: "f0".to_owned(),
            }],
        },
    ];

    let metal = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect("admitted wire builds the metal descriptor");
    assert_eq!(
        metal.kernels[0].entry, "summa",
        "Metal launches by the logical entry"
    );

    let cuda = descriptor_for_backend(&section, DeviceBackend::Cuda, b"ptx blob")
        .expect("admitted wire builds the cuda descriptor");
    assert_eq!(
        cuda.kernels[0].entry, "f0",
        "the CUDA descriptor consumes the artifact's symbol mapping"
    );
    assert_eq!(cuda.module_image, b"ptx blob");
    assert_eq!(cuda.kernels[0].buffers[0].buffer_id, 1);
    assert_eq!(cuda.kernels[0].block, [256, 1, 1]);
}

/// The wire's typed lifetimes are mapped onto the host descriptor — the host
/// receives the carried facts and never re-derives a lifetime from slot role
/// (S2-4, now over the typed wire).
#[test]
fn descriptor_maps_wire_lifetimes_onto_host_descriptor() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    // The intermediate remains present in the descriptor as an InOut slot,
    // but this projection test supplies only the supported final observation.
    section
        .device_program
        .program
        .results
        .retain(|result| result.role == WireBufferRole::Output);

    let descriptor = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect("admitted wire builds the descriptor");
    assert_eq!(
        descriptor.program_lifetime,
        HostDeviceProgramLifetime::SingleRun,
        "the program regime rides the wire into the host descriptor"
    );
    let slots = &descriptor.kernels[0].buffers;
    assert_eq!(slots[0].lifetime, DeviceBufferLifetime::PerProgram);
    assert_eq!(slots[1].lifetime, DeviceBufferLifetime::PerStep);
    let kernel1_slots = &descriptor.kernels[1].buffers;
    assert_eq!(kernel1_slots[0].lifetime, DeviceBufferLifetime::PerStep);
    assert_eq!(
        kernel1_slots[1].lifetime,
        DeviceBufferLifetime::ObservationPoint
    );

    // G4 (F5): the wire's carried initialization axis is projected onto the
    // descriptor verbatim — the host honors it at allocation; it never
    // re-derives initialization from role or lifetime. The two-kernel
    // fixture: the input is HostProvided, the S2-5 intermediate is
    // kernel-initialized (written before read), the output is
    // kernel-initialized.
    assert_eq!(
        slots[0].initialization,
        DeviceBufferInitialization::HostProvided,
        "a read-only host input is host-provided"
    );
    assert_eq!(
        slots[1].initialization,
        DeviceBufferInitialization::KernelInitialized,
        "the S2-5 intermediate is kernel-initialized"
    );
    assert_eq!(
        kernel1_slots[1].initialization,
        DeviceBufferInitialization::KernelInitialized,
        "the output is kernel-initialized"
    );

    // R2: the host descriptor carries the wire's content versions and
    // data-flow edges — the A10 graph consumes real facts, never a
    // hardcoded `version: 1` or a first-writer coincidence derivation.
    assert_eq!(
        slots[1].version, 1,
        "the medius intermediate carries its wire content version"
    );
    assert!(
        descriptor.data_flow.contains(&HostDescriptorDataFlow {
            buffer_id: 2,
            version: 1,
            producer: 1,
            consumer: 2,
        }),
        "the carried producer/consumer edge must ride the descriptor: {:?}",
        descriptor.data_flow
    );
}

#[test]
fn descriptor_preserves_wire_launch_order_and_version_keys() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];

    {
        let wire = &mut section.device_program.program;
        wire.launches = vec![
            WireLaunchUnit {
                id: 11,
                kernel_index: 1,
            },
            WireLaunchUnit {
                id: 12,
                kernel_index: 0,
            },
            WireLaunchUnit {
                id: 13,
                kernel_index: 1,
            },
        ];
        // Re-declare the writable InOut intermediate as a DECLARED
        // observation point (ObservationPoint form) so this projection test
        // can exercise ordered readback of two results. The constructor
        // itself never exposes an intermediate without an explicit
        // observation declaration (F6); the host readback contract supports
        // only observation-point results.
        let medius_buffer = wire.kernels[0].resources[1].buffer.clone();
        let medius_version = wire.kernels[0].resources[1].version.clone();
        let result_buffer = wire.kernels[1].resources[1].buffer.clone();
        let result_version = wire.kernels[1].resources[1].version.clone();
        wire.results.clear();
        wire.results.push(WireResultBuffer {
            buffer: WireBufferIdentity {
                lifetime: WireBufferLifetime::ObservationPoint,
                ..medius_buffer
            },
            version: medius_version,
            role: WireBufferRole::InOut,
            produced_by: 12,
            observation: WireObservationFact { at_launch: 12 },
        });
        wire.results.push(WireResultBuffer {
            buffer: result_buffer,
            version: result_version,
            role: WireBufferRole::Output,
            produced_by: 11,
            observation: WireObservationFact { at_launch: 11 },
        });
        // Make the intermediate's slots observation-point (the form the host
        // readback contract accepts for this projection test).
        for kernel in &mut wire.kernels {
            for resource in &mut kernel.resources {
                if resource.buffer.id == 2 {
                    resource.buffer.lifetime = WireBufferLifetime::ObservationPoint;
                }
            }
        }
    }
    let medius_id = section.device_program.program.kernels[0].resources[1]
        .buffer
        .id;
    let (_, initial_edges) = wire_resource_graph(&section);
    assert_eq!(
        initial_edges,
        vec![
            WireGraphEdge {
                buffer_id: medius_id,
                version: 1,
                producer: 12,
                consumer: 11,
            },
            WireGraphEdge {
                buffer_id: medius_id,
                version: 1,
                producer: 12,
                consumer: 13,
            },
        ],
        "the graph follows the ordered launch identities, including repetition"
    );
    {
        let wire = &mut section.device_program.program;
        wire.kernels[0].resources[1].version.version = 1;
        wire.kernels[1].resources[0].version.version = 2;
        wire.kernels[1].resources[0].version.element_count = 64;
    }
    let (versioned_graph, versioned_edges) = wire_resource_graph(&section);
    assert_eq!(
        versioned_graph
            .iter()
            .filter(|buffer| buffer.id == medius_id)
            .map(|buffer| (buffer.id, buffer.version, buffer.element_count))
            .collect::<Vec<_>>(),
        vec![(medius_id, 2, 64), (medius_id, 1, 4)]
    );
    assert!(versioned_edges.is_empty());

    // F3 (U5): the host consumes the wire's CARRIED graph facts verbatim, so
    // the mutated wire must carry a consistent root set and dependency set —
    // the roots name the mutated launch sequence, and the version-diverged
    // intermediate carries no dependency edge.
    {
        let wire = &mut section.device_program.program;
        wire.roots = vec![11, 12, 13];
        wire.dependencies.clear();
    }

    let descriptor = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect("complete wire projects into the host descriptor");
    assert_eq!(
        descriptor.launches,
        vec![
            DescriptorLaunch {
                id: 11,
                kernel_index: 1,
            },
            DescriptorLaunch {
                id: 12,
                kernel_index: 0,
            },
            DescriptorLaunch {
                id: 13,
                kernel_index: 1,
            },
        ]
    );
    assert_eq!(
        descriptor
            .kernels
            .iter()
            .flat_map(|kernel| kernel.buffers.iter())
            .filter(|slot| slot.buffer_id == medius_id)
            .map(|slot| (slot.buffer_id, slot.version, slot.element_count))
            .collect::<Vec<_>>(),
        vec![(medius_id, 1, 4), (medius_id, 2, 64)]
    );
    assert!(descriptor
        .buffer_versions
        .contains(&DescriptorBufferVersion {
            buffer_id: medius_id,
            version: 1,
            element_ty: DeviceDataType::F32,
            element_count: 4,
        }));
    assert!(descriptor
        .buffer_versions
        .contains(&DescriptorBufferVersion {
            buffer_id: medius_id,
            version: 2,
            element_ty: DeviceDataType::F32,
            element_count: 64,
        }));
    assert!(descriptor.data_flow.is_empty());

    assert_eq!(
        host_receipt_launch_order_line(&descriptor),
        "device: launch order: [#0 id=11 kernel_index=1 backend_entry=`recollige`, #1 id=12 kernel_index=0 backend_entry=`collige`, #2 id=13 kernel_index=1 backend_entry=`recollige`]"
    );
}

/// U5 done-when (E_DEVICE_DESCRIPTOR gone): the ordinary two-kernel package
/// (the device-summa-recollige shape — a dependent chain) projects onto a
/// host descriptor that ADMITS at the host. The old P1-4 contradiction —
/// the constructor exposing every writable Output/InOut as a result while
/// the host admits only declared observation points — is gone: constructor
/// and host admission agree on one readback rule (F6). The descriptor also
/// carries the frozen graph facts (F1/F3): semantic value identities per
/// buffer, the declared roots, the carried dependency edge, and the explicit
/// observation point.
#[test]
fn two_kernel_descriptor_admits_on_host_with_carried_graph() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];

    let descriptor = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect("the ordinary two-kernel package must construct an admissible host descriptor");
    descriptor
        .validate()
        .expect("constructor and host admission agree — no E_DEVICE_DESCRIPTOR contradiction");

    // F3: the carried graph rides the descriptor — the declared root (launch
    // 1) and the intermediate's producer/consumer edge, verbatim from the
    // wire's `dependencies`.
    assert_eq!(descriptor.roots, vec![1]);
    assert_eq!(
        descriptor.data_flow,
        vec![HostDescriptorDataFlow {
            buffer_id: 2,
            version: 1,
            producer: 1,
            consumer: 2,
        }]
    );

    // F6: the single declared observation point is the readback set; the
    // writable intermediate is never a result merely because it is writable.
    assert_eq!(descriptor.results.len(), 1);
    assert_eq!(descriptor.results[0].buffer_id, 3);
    assert_eq!(descriptor.results[0].version, 1);
    assert_eq!(descriptor.results[0].produced_by, 2);
    assert_eq!(descriptor.results[0].at_launch, 2);

    // F1: every buffer carries its wire semantic value identity (non-zero and
    // one distinct value per buffer).
    let mut semantic_ids: Vec<u32> = descriptor
        .kernels
        .iter()
        .flat_map(|kernel| kernel.buffers.iter())
        .map(|slot| slot.semantic_value)
        .collect();
    semantic_ids.dedup();
    assert_eq!(
        semantic_ids.len(),
        3,
        "one stable semantic value per buffer (a, medius, result)"
    );
    assert!(semantic_ids.iter().all(|value| *value != 0));
}

#[test]
fn descriptor_missing_keyed_version_metadata_fails_closed() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    section
        .device_program
        .program
        .results
        .retain(|result| result.role == WireBufferRole::Output);
    let mut descriptor = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect("baseline descriptor is valid");
    descriptor.buffer_versions.clear();

    let error = descriptor
        .validate()
        .expect_err("a slot without keyed metadata must fail closed");
    assert!(error.message.contains("no version-keyed buffer metadata"));
}

#[test]
fn descriptor_rejects_result_only_wire_record() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    section.device_program.program.results[0].buffer.id = 99;

    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("a result with no launched resource must fail closed");
    assert!(error[0].message.contains("no matching resource"));
    assert!(error[0].message.contains("buffer 99"));
}

#[test]
fn descriptor_rejects_result_with_contradictory_role() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    section.device_program.program.results[0].role = WireBufferRole::Input;

    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("a result with an input observation role must fail closed");
    assert!(error[0].message.contains("invalid observation role input"));
}

#[test]
fn descriptor_rejects_result_with_contradictory_producer() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    section
        .device_program
        .program
        .results
        .retain(|result| result.role == WireBufferRole::Output);
    let first_launch = section.device_program.program.launches[0].id;
    section
        .device_program
        .program
        .results
        .last_mut()
        .expect("two-kernel fixture has a final result")
        .produced_by = first_launch;

    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("a result named by the wrong producer must fail closed");
    assert!(error[0].message.contains("producing launch"));
    assert!(error[0].message.contains("no matching resource"));
}

#[test]
fn descriptor_rejects_result_with_contradictory_version_shape() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    section
        .device_program
        .program
        .results
        .retain(|result| result.role == WireBufferRole::Output);
    let result = section
        .device_program
        .program
        .results
        .last_mut()
        .expect("two-kernel fixture has a final result");
    result.version.element_count += 1;

    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("a result with contradictory producer shape must fail closed");
    assert!(error[0].message.contains("carries shape"));
    assert!(error[0].message.contains("producing launch"));
}

#[test]
fn descriptor_rejects_result_with_contradictory_version_number() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    section
        .device_program
        .program
        .results
        .retain(|result| result.role == WireBufferRole::Output);
    section
        .device_program
        .program
        .results
        .last_mut()
        .expect("two-kernel fixture has a final result")
        .version
        .version += 1;

    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("a result with a contradictory version must fail closed");
    assert!(error[0].message.contains("declares version"));
    assert!(error[0].message.contains("producing launch"));
}

#[test]
fn receipt_rendering_uses_host_carried_graph_facts() {
    let resource_graph = vec![ReceiptBuffer {
        id: 9,
        name: "acc".to_owned(),
        role: DeviceBufferRole::InOut,
        lifetime: DeviceBufferLifetime::PerStep,
        element_ty: DeviceDataType::F32,
        element_count: 64,
        version: 2,
    }];
    let data_flow_edges = vec![DataFlowEdge {
        buffer_id: 9,
        version: 2,
        producer: 12,
        consumer: 13,
    }];

    let lines = host_receipt_graph_lines(&resource_graph, &data_flow_edges);
    assert!(lines
        .iter()
        .any(|line| { line.contains("buffer 9 `acc` in-out per-step version 2 (f32[64])") }));
    assert!(lines
        .iter()
        .any(|line| line.contains("data-flow 12 -> 13 via buffer 9 version 2")));
    assert!(lines[0].contains("host receipt"));
}

#[test]
fn descriptor_requires_declared_backend_artifact() {
    let artifacts = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    assert!(artifact_for_backend(&artifacts, DeviceBackend::Metal).is_some());
    assert!(
        artifact_for_backend(&artifacts, DeviceBackend::Cuda).is_none(),
        "an undeclared backend artifact fails closed"
    );
}

/// A carried element-type spelling outside the campaign dtype surface fails
/// the descriptor construction closed — never a silent default, never an
/// unreachable arm.
#[test]
fn descriptor_rejects_unknown_element_type_spelling() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let mut section = section_for_program(&program, &semantics);
    section.device_program.program.kernels[0].resources[0]
        .version
        .element_ty = "f64".to_owned();
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("unknown element type must fail closed");
    assert!(
        error[0].message.contains("f64"),
        "the diagnostic names the offending spelling: {}",
        error[0].message
    );
}

#[test]
fn descriptor_rejects_inout_result_before_host_projection() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    // Inject a PerStep InOut result row (the writable intermediate exposed
    // as a result without an explicit observation declaration). The
    // constructor never produces this form (F6); the host admission must
    // reject it before host construction.
    let medius = section.device_program.program.kernels[0].resources[1]
        .buffer
        .clone();
    let medius_version = section.device_program.program.kernels[0].resources[1]
        .version
        .clone();
    section.device_program.program.results.insert(
        0,
        WireResultBuffer {
            buffer: medius,
            version: medius_version,
            role: WireBufferRole::InOut,
            produced_by: 1,
            observation: WireObservationFact { at_launch: 1 },
        },
    );

    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("a PerStep InOut result must fail before host construction");
    assert!(error[0].message.contains("result 0"));
    assert!(error[0].message.contains("per-step"));
    assert!(error[0].message.contains("observation-point"));
}

#[test]
fn descriptor_rejects_duplicate_result_buffer_before_host_projection() {
    let (program, semantics) =
        device_program_and_semantics_from_corpus_fixture("cuda/summa-proof.fab");
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    let duplicate = section
        .device_program
        .program
        .results
        .first()
        .cloned()
        .expect("summa fixture has one result");
    section.device_program.program.results.push(duplicate);

    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("duplicate result buffers must fail before host construction");
    assert!(error[0].message.contains("repeats observation buffer"));
}

/// S2-5 constructor identity unification: the two-kernel chain shares ONE
/// `BufferId` for the device-resident intermediate (`medius`, InOut role,
/// PerStep lifetime) across both kernels — produced by launch 1, consumed by
/// launch 2 (a data-flow edge) — while the declared input stays PerProgram
/// and the final output stays ObservationPoint.
#[test]
fn two_kernel_chain_unifies_intermediate_identity() {
    let program = two_kernel_program();
    program.validate().expect("program validates");

    // Two kernels, three distinct buffers (a, medius, result).
    assert_eq!(program.kernels.len(), 2);
    assert_eq!(program.kernels[0].entry, "collige");
    assert_eq!(program.kernels[1].entry, "recollige");

    let buffers = program.buffer_registry();
    assert_eq!(buffers.buffers.len(), 3);
    let by_name = |name: &str| {
        buffers
            .buffers
            .iter()
            .find(|entry| entry.identity.name == name)
            .unwrap_or_else(|| panic!("buffer `{name}` must exist"))
    };

    let input = by_name("a");
    assert_eq!(input.identity.role, BufferRole::Input);
    assert_eq!(input.identity.lifetime, BufferLifetime::PerProgram);

    let intermediate = by_name("medius");
    assert_eq!(intermediate.identity.role, BufferRole::InOut);
    assert_eq!(intermediate.identity.lifetime, BufferLifetime::PerStep);

    let output = by_name("result");
    assert_eq!(output.identity.role, BufferRole::Output);
    assert_eq!(output.identity.lifetime, BufferLifetime::ObservationPoint);

    // The intermediate is referenced by BOTH kernels under the same id:
    // kernel 1 writes it (Write), kernel 2 reads it (Read).
    let medius_id = intermediate.identity.id;
    let kernel0_medius = program.kernels[0]
        .resources
        .iter()
        .find(|resource| resource.buffer.id == medius_id)
        .expect("kernel 1 references the intermediate");
    assert_eq!(kernel0_medius.access, MirKernelResourceAccess::Write);
    let kernel1_medius = program.kernels[1]
        .resources
        .iter()
        .find(|resource| resource.buffer.id == medius_id)
        .expect("kernel 2 references the intermediate");
    assert_eq!(kernel1_medius.access, MirKernelResourceAccess::Read);
    assert_eq!(
        kernel0_medius.version.element_count,
        kernel1_medius.version.element_count
    );

    // Data-flow edge: launch 1 produces the intermediate, launch 2 consumes
    // it (the schema's BufferRegistry/DataFlowPair model).
    assert_eq!(
        program.buffer_registry().data_flow_pairs(),
        vec![DataFlowPair {
            buffer: medius_id,
            version: 1,
            producer: LaunchId(1),
            consumer: LaunchId(2),
        }]
    );

    // F6: results name DECLARED observation points only — the program's
    // Output-role buffer (`result`). The writable InOut intermediate
    // (`medius`) is not a result merely because it is writable.
    let results: Vec<_> = program
        .results
        .iter()
        .map(|result| (result.buffer.name.as_str(), result.produced_by.0))
        .collect();
    assert_eq!(results, vec![("result", 2)]);
}

/// The unified lifetimes ride the typed wire (S3-A4): the intermediate's
/// resources are in-out/per-step at both kernels under ONE buffer id, the
/// final output is observation-point, and the ordinary readback set is
/// exactly the observation point — the intermediate is never read back.
#[test]
fn two_kernel_wire_carries_unified_lifetimes() {
    let (program, semantics) = two_kernel_fixture();
    let section = section_for_program(&program, &semantics);
    let wire = &section.device_program.program;
    assert_eq!(wire.kernels.len(), 2);

    // Kernel 1: a (input/per-program) + medius (in-out/per-step).
    let kernel0_resources = &wire.kernels[0].resources;
    assert_eq!(kernel0_resources.len(), 2);
    assert_eq!(kernel0_resources[0].buffer.name, "a");
    assert_eq!(kernel0_resources[0].buffer.role, WireBufferRole::Input);
    assert_eq!(
        kernel0_resources[0].buffer.lifetime,
        WireBufferLifetime::PerProgram
    );
    assert_eq!(kernel0_resources[1].buffer.name, "medius");
    assert_eq!(kernel0_resources[1].buffer.role, WireBufferRole::InOut);
    assert_eq!(
        kernel0_resources[1].buffer.lifetime,
        WireBufferLifetime::PerStep
    );

    // Kernel 2: medius (in-out/per-step, same id) + result
    // (output/observation-point).
    let kernel1_resources = &wire.kernels[1].resources;
    assert_eq!(kernel1_resources.len(), 2);
    assert_eq!(
        kernel1_resources[0].buffer.id, kernel0_resources[1].buffer.id,
        "one BufferId for the intermediate"
    );
    assert_eq!(kernel1_resources[0].buffer.role, WireBufferRole::InOut);
    assert_eq!(
        kernel1_resources[0].buffer.lifetime,
        WireBufferLifetime::PerStep
    );
    assert_eq!(kernel1_resources[1].buffer.name, "result");
    assert_eq!(kernel1_resources[1].buffer.role, WireBufferRole::Output);
    assert_eq!(
        kernel1_resources[1].buffer.lifetime,
        WireBufferLifetime::ObservationPoint
    );

    // F6: the explicit result rows are the readback set — ONLY the declared
    // observation point (the final Output). The writable InOut intermediate
    // is never a result and never read back.
    let readbacks = observation_buffer_ids(&section);
    assert_eq!(readbacks, vec![kernel1_resources[1].buffer.id]);

    // The wire round-trips deterministically with the unified lifetimes
    // intact (canonical bytes stable).
    assert_eq!(
        radix_mir_fmir::canonical_program_bytes(wire),
        radix_mir_fmir::canonical_program_bytes(&wire_program_for_program(&program, &semantics))
    );
}

/// The wire-derived A10 resource graph (S3-A4) matches the radix-mir
/// registry's declared facts for the two-kernel chain: the intermediate's
/// version-1 edge is launch 1 → launch 2 (no coincidence-based re-derivation
/// — the edge comes from the carried access + launches).
#[test]
fn wire_resource_graph_matches_registry_facts() {
    let (program, semantics) = two_kernel_fixture();
    let section = section_for_program(&program, &semantics);
    let (graph, edges) = wire_resource_graph(&section);

    // Three buffers in first-reference order.
    let names: Vec<&str> = graph.iter().map(|buffer| buffer.name.as_str()).collect();
    assert_eq!(names, vec!["a", "medius", "result"]);

    // The intermediate's version-1 edge: launch 1 produces, launch 2
    // consumes (the same fact `DeviceProgram::buffer_registry` derives).
    let medius = graph
        .iter()
        .find(|buffer| buffer.name == "medius")
        .expect("intermediate is on the wire");
    assert_eq!(medius.version, 1);
    assert_eq!(
        edges,
        vec![WireGraphEdge {
            buffer_id: medius.id,
            version: 1,
            producer: 1,
            consumer: 2,
        }]
    );
}

/// The `FABER_DEVICE_REPEAT` leak-proof hook: absent → 1, valid number →
/// that count, garbage → fail closed (never a silent fallback to 1).
#[test]
fn device_repeat_count_is_fail_closed() {
    let previous = std::env::var("FABER_DEVICE_REPEAT").ok();
    std::env::remove_var("FABER_DEVICE_REPEAT");
    assert_eq!(device_repeat_count().expect("defaults to one"), 1);

    std::env::set_var("FABER_DEVICE_REPEAT", "5");
    assert_eq!(device_repeat_count().expect("parses"), 5);

    std::env::set_var("FABER_DEVICE_REPEAT", "lots");
    assert!(
        device_repeat_count().is_err(),
        "a non-numeric repeat count must fail closed"
    );

    match previous {
        Some(value) => std::env::set_var("FABER_DEVICE_REPEAT", value),
        None => std::env::remove_var("FABER_DEVICE_REPEAT"),
    }
}

/// The S3-A2 materializer: a package whose primal is BOTH a nucleum forward
/// kernel and `@ radix backward`-annotated produces a DeviceProgram whose
/// kernel set + order is [forward loss, companion loss_backward] — the
/// companion's tuple gradients lower into distinct output resources and its
/// inputs unify with the forward's device-resident buffers (S2-5 identity).
#[test]
fn companion_forward_and_backward_kernel_set_and_order() {
    let entry = PathBuf::from("/tmp/s3a2probe/src/probe.fab");
    let (program, semantics) = super::super::with_lowered_package_mir(
        &radix::driver::Config::default()
            .with_stdlib(dev_norma_library_home())
            .with_target(radix::codegen::Target::Fmir),
        &entry,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("device package yields a device program")
        },
    )
    .expect("fixture lowers");
    program.validate().expect("program validates");

    // F4: the lossless primal/companion relation survives onto the carried
    // semantics (and later the serialized wire) — the forward's companion
    // row with the gradient-to-primal identity is present, never dropped.
    assert_eq!(semantics.relations.len(), 1);
    let carried = &semantics.relations[0];
    assert!(
        carried.device_resident,
        "the device-resident primal's companion row rides the carried semantics"
    );

    // Kernel set + order: the forward primal first, then the companion.
    let entries: Vec<&str> = program.kernels.iter().map(|k| k.entry.as_str()).collect();
    assert_eq!(entries, vec!["loss", "loss_backward"]);

    // The companion carries the multi-output ABI: two gradient output
    // resources binding distinct slots (S3-A1).
    let companion = &program.kernels[1];
    let grad_outputs: Vec<_> = companion
        .resources
        .iter()
        .filter(|r| r.buffer.role == BufferRole::Output)
        .collect();
    assert_eq!(grad_outputs.len(), 2);
    assert_ne!(
        grad_outputs[0].binding.binding, grad_outputs[1].binding.binding,
        "two gradient outputs must bind distinct slots"
    );

    // The companion's inputs unify with the forward's device-resident
    // buffers: x and w share ONE BufferId across both kernels.
    let forward_x = program.kernels[0]
        .resources
        .iter()
        .find(|r| r.buffer.name == "x")
        .expect("forward reads x");
    let companion_x = companion
        .resources
        .iter()
        .find(|r| r.buffer.name == "x")
        .expect("companion reads x");
    assert_eq!(
        forward_x.buffer.id, companion_x.buffer.id,
        "x unifies by S2-5 identity"
    );
}

/// The S3-A2 carrier round-trips primal → companion with the derivative kind
/// and the device-residency placement fact; a carried companion MISSING from
/// the lowered MIR fails construction closed with the typed diagnostic (the
/// relation facts are the routing surface — never a name heuristic).
#[test]
fn companion_carrier_round_trips_and_missing_companion_fails_closed() {
    let entry = PathBuf::from("/tmp/s3a2probe/src/probe.fab");
    let config = radix::driver::Config::default()
        .with_stdlib(dev_norma_library_home())
        .with_target(radix::codegen::Target::Fmir);
    super::super::with_lowered_package_mir(&config, &entry, |lowered| {
        // Round-trip: exactly one carried companion, VJP derivative,
        // device-resident (the primal is a nucleum kernel).
        let entries: Vec<_> = lowered.companions.iter().collect();
        assert_eq!(entries.len(), 1);
        let carried = entries[0];
        assert_eq!(
            carried.derivative,
            radix_mir::device::MirCompanionDerivativeKind::ReverseModeVjp
        );
        assert!(
            carried.device_resident,
            "the primal carries explicit device intent (@ nucleum), so its companion is device-resident"
        );

        // Fail closed: a carried companion absent from the MIR yields the
        // typed diagnostic.
        let mut phantom = lowered.companions.clone();
        phantom.insert(radix_mir::device::MirCompanionEntry {
            primal: radix::hir::DefId(9_999),
            companion: radix::hir::DefId(9_998),
            derivative: radix_mir::device::MirCompanionDerivativeKind::ReverseModeVjp,
            device_resident: true,
        });
        let error = device_program_for_lowered(&lowered.validated, &lowered.interner, &phantom)
            .expect_err("a carried companion missing from the MIR must fail closed");
        assert!(
            error.iter().any(|diagnostic| diagnostic.message.contains("missing from the lowered MIR")),
            "the fail-closed diagnostic names the missing companion: {:?}",
            error.iter().map(|d| d.message.clone()).collect::<Vec<_>>()
        );
    })
    .expect("lower");
}

// ── S3-A3 fail-closed plan surface (N3.2) ──────────────────────────────────

/// Lower an inline package entry from raw source (the corpus-fixture pattern
/// for fixtures that do not belong in the corpus).
fn with_inline_package<R>(
    name: &str,
    source: &str,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    let root = std::env::temp_dir().join(format!("faber-s3a3-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(root.join("src")).expect("temp fixture dir");
    let entry = root.join("src").join("probe.fab");
    std::fs::write(&entry, source).expect("write temp fixture");
    let config = radix::driver::Config::default()
        .with_stdlib(dev_norma_library_home())
        .with_target(radix::codegen::Target::Fmir);
    super::super::with_lowered_package_mir(&config, &entry, run)
}

#[test]
fn device_program_constructor_rejects_unplannable_op_with_typed_diagnostic() {
    // N3.2 / D1: a device-routed kernel whose body carries TensorSoftmax
    // (no recipe, not an elementwise transform) fails construction with the
    // typed plan diagnostic — never a silent Elementwise floor.
    let result = with_inline_package(
        "softmaxio",
        r#"@ nucleum
functio softmaxio(tf32[2,2] x) → tf32[2,2] {
    redde x.activatio_softmax()
}"#,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
        },
    )
    .expect("fixture lowers");
    let diagnostics = result.expect_err("an unplannable op must fail construction");
    let messages = diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>();
    let joined = messages.join(" | ");
    assert!(
        joined.contains("TensorSoftmax"),
        "the typed diagnostic names the op: {joined}"
    );
    assert!(
        joined.contains("no kernel plan"),
        "the typed diagnostic says no kernel plan: {joined}"
    );
}

#[test]
fn device_program_constructor_derives_explicit_plans_for_companion_program() {
    // N3.2 done_when 3: the S3-A2 companion program (elementwise mul forward
    // with its generated backward) builds with an explicit plan for EVERY op
    // — the function-level scan decides each kernel, never a silent fallback.
    // The forward's mul-only body and the companion's mul VJP body are both
    // decided Elementwise.
    let (program, _semantics) = with_inline_package(
        "companion-mul",
        r#"@ nucleum
@ radix lane "air"
@ radix backward "loss_backward"
functio loss(tf32[2] x, tf32[2] w) → tf32[2] {
    redde x.multiplica(w)
}"#,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("device package yields a device program")
        },
    )
    .expect("fixture lowers");
    assert_eq!(program.kernels.len(), 2);
    let forward = &program.kernels[0];
    assert_eq!(forward.entry, "loss");
    assert_eq!(
        forward.plan,
        radix_mir::kernel_plan::CollectionKernelPlan::Elementwise,
        "the mul-only forward kernel is decided elementwise"
    );
    let companion = &program.kernels[1];
    assert_eq!(companion.entry, "loss_backward");
    assert_eq!(
        companion.plan,
        radix_mir::kernel_plan::CollectionKernelPlan::Elementwise,
        "the companion's mul VJP body is decided elementwise"
    );
}

#[test]
fn device_program_constructor_multi_op_mul_mean_derives_reduction_plan() {
    // N3.2 done_when 2: the Mul + Mean workload — the campaign's actual
    // forward shape — is scanned across ALL statements. The body mixes an
    // elementwise transform (mul) with a recipe op (mean); the full scan
    // derives the reduction recipe (the old single-op scan saw only the mul
    // and silently floored the kernel to Elementwise).
    let (program, _semantics) = with_inline_package(
        "mul-mean",
        r#"@ nucleum
functio mean_mul(tf32[2] x, tf32[2] w, tf32[1] out, u32 id) → vacuum {
    fixum tf32[2] t ← x.multiplica(w)
    fixum f32 total ← t.media()
    out[id] ← total
}"#,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("device package yields a device program")
        },
    )
    .expect("fixture lowers");
    assert_eq!(program.kernels.len(), 1);
    let forward = &program.kernels[0];
    assert_eq!(forward.entry, "mean_mul");
    let CollectionKernelPlan::TreeReduction(reduction) = &forward.plan else {
        panic!(
            "the mul+mean forward kernel must carry the reduction recipe, got {:?}",
            forward.plan
        );
    };
    assert_eq!(reduction.op, radix_mir::kernel_plan::ReduceOp::Mean);
    assert_eq!(reduction.length, 2);
}

// ── Stage 3R U2: independent resource-state axes (F5) ─────────────────────

/// The faber constructor's projection carries every resource-state axis on
/// the wire as an independent fact: one semantic value per buffer minted
/// from carried MIR facts (F1), explicit generations (F2), per-buffer
/// initialization policies and allocation lifetimes decided from access
/// facts (F5), carried roots + producer/consumer dependencies (F3), and an
/// explicit observation fact on every result row (F6).
#[test]
fn constructor_projects_independent_axes_onto_the_wire() {
    let (program, semantics) = two_kernel_fixture();
    let wire = wire_program_for_program(&program, &semantics);

    // F1: one semantic value per buffer, minted from the carried MIR-local
    // facts (a, medius, result all flow from kernel-slot locals) with
    // distinct origins (two values never alias), and every buffer reference
    // carrying the value it holds.
    assert_eq!(wire.semantic_values.len(), 3);
    let names: Vec<&str> = wire
        .semantic_values
        .iter()
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(names, vec!["a", "medius", "result"]);
    assert!(
        wire.semantic_values
            .iter()
            .all(|value| matches!(value.origin, WireSemanticValueOrigin::MirLocal { .. })),
        "the two-kernel chain's value identities derive from carried MIR locals, never \
         from buffer ids or synthetic labels"
    );
    for left in &wire.semantic_values {
        for right in &wire.semantic_values {
            if left.id != right.id {
                assert_ne!(
                    left.origin, right.origin,
                    "distinct values must carry distinct origins (F1)"
                );
            }
        }
    }
    for kernel in &wire.kernels {
        for resource in &kernel.resources {
            assert!(
                wire.semantic_values
                    .iter()
                    .any(|value| value.id == resource.buffer.semantic_value),
                "buffer {} references a declared semantic value",
                resource.buffer.id
            );
        }
    }

    // F2: every slot carries an explicit generation (never the unset 0).
    for kernel in &wire.kernels {
        for resource in &kernel.resources {
            assert!(
                resource.generation >= 1,
                "slot on buffer {} carries generation {}",
                resource.buffer.id,
                resource.generation
            );
        }
    }

    // F5: the initialization axis is decided from access facts (host-provided
    // read-only input vs kernel-written), never from role.
    let init_by_buffer: Vec<(u32, WireInitializationPolicy)> = wire
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .map(|resource| (resource.buffer.id, resource.initialization))
        .collect();
    assert!(
        init_by_buffer
            .iter()
            .all(|(id, policy)| (*id != 1)
                == (*policy == WireInitializationPolicy::KernelInitialized))
    );
    assert!(init_by_buffer
        .iter()
        .any(|(id, policy)| *id == 1 && *policy == WireInitializationPolicy::HostProvided));

    // F3: carried roots + the producer/consumer dependency (launch 1 writes
    // medius, launch 2 reads it) — the host schedules from these facts.
    assert_eq!(wire.roots, vec![1]);
    assert_eq!(
        wire.dependencies,
        vec![WireDependencyEdge {
            producer: 1,
            consumer: 2,
            buffer: 2,
            version: 1,
        }]
    );
    assert!(wire.relations.is_empty());

    // F6: every result row is a declared observation point at its producing
    // launch's completion boundary — and results name the declared Output
    // only (the writable InOut intermediate is never a result).
    assert_eq!(wire.results.len(), 1);
    assert_eq!(wire.results[0].buffer.name, "result");
    for result in &wire.results {
        assert_eq!(
            result.observation.at_launch, result.produced_by,
            "result on buffer {} must carry an explicit observation fact",
            result.buffer.id
        );
    }
}

/// F5 coupling proof on the wire: the initialization, lifetime, generation,
/// and observation axes are independent facts — changing one axis alone
/// never silently rewrites another.
#[test]
fn wire_axes_vary_independently() {
    let (program, semantics) = two_kernel_fixture();
    let mut wire = wire_program_for_program(&program, &semantics);

    let generations = |wire: &WireDeviceProgram| -> Vec<u32> {
        wire.kernels
            .iter()
            .flat_map(|kernel| kernel.resources.iter())
            .map(|resource| resource.generation)
            .collect()
    };
    let lifetimes = |wire: &WireDeviceProgram| -> Vec<WireBufferLifetime> {
        wire.kernels
            .iter()
            .flat_map(|kernel| kernel.resources.iter())
            .map(|resource| resource.buffer.lifetime)
            .collect()
    };
    let observations = |wire: &WireDeviceProgram| -> Vec<u32> {
        wire.results
            .iter()
            .map(|result| result.observation.at_launch)
            .collect()
    };
    let base_generations = generations(&wire);
    let base_lifetimes = lifetimes(&wire);
    let base_observations = observations(&wire);

    // 1. Initialization changes alone (input buffer → ZeroFill): no other
    // axis is touched.
    for kernel in &mut wire.kernels {
        for resource in &mut kernel.resources {
            if resource.buffer.id == 1 {
                resource.initialization = WireInitializationPolicy::ZeroFill;
            }
        }
    }
    assert_eq!(generations(&wire), base_generations);
    assert_eq!(lifetimes(&wire), base_lifetimes);
    assert_eq!(observations(&wire), base_observations);

    // 2. Lifetime changes alone (result → per-step): init/generation/
    // observation untouched.
    for kernel in &mut wire.kernels {
        for resource in &mut kernel.resources {
            if resource.buffer.name == "result" {
                resource.buffer.lifetime = WireBufferLifetime::PerStep;
            }
        }
    }
    assert_eq!(generations(&wire), base_generations);
    assert_eq!(observations(&wire), base_observations);
    let after_lifetime_change = lifetimes(&wire);
    assert!(wire
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .any(|resource| {
            resource.buffer.name == "result"
                && resource.buffer.lifetime == WireBufferLifetime::PerStep
                && resource.initialization == WireInitializationPolicy::KernelInitialized
        }));

    // 3. Generation changes alone (a later write is a new generation): the
    // lifetimes as set in step 2 and the observations are untouched.
    for kernel in &mut wire.kernels {
        for resource in &mut kernel.resources {
            if resource.buffer.id == 2 && resource.access == WireResourceAccess::Read {
                resource.generation = 2;
            }
        }
    }
    assert_eq!(lifetimes(&wire), after_lifetime_change);
    assert_eq!(observations(&wire), base_observations);
    assert!(generations(&wire).contains(&2));

    // 4. Observation changes alone: init/lifetime/generation untouched.
    wire.results[0].observation.at_launch = 9;
    assert_eq!(
        generations(&wire)
            .iter()
            .filter(|generation| **generation >= 1)
            .count(),
        4
    );
    assert_eq!(lifetimes(&wire), after_lifetime_change);
    assert!(wire
        .results
        .iter()
        .any(|result| result.observation.at_launch == 9));
    assert!(wire
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .any(|resource| resource.buffer.id == 1
            && resource.initialization == WireInitializationPolicy::ZeroFill));
}

/// F6 red test: a writable intermediate (or final) exposed as a result whose
/// explicit observation fact contradicts its producing launch is rejected
/// before host construction — the constructor and host admission agree.
#[test]
fn result_without_explicit_observation_fact_is_rejected() {
    let (program, semantics) = two_kernel_fixture();
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    section
        .device_program
        .program
        .results
        .retain(|result| result.role == WireBufferRole::Output);
    let first_launch = section.device_program.program.launches[0].id;
    section
        .device_program
        .program
        .results
        .last_mut()
        .expect("two-kernel fixture has a final result")
        .observation
        .at_launch = first_launch;

    let error = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect_err("a result without an explicit observation fact must fail closed");
    assert!(
        error[0].message.contains("observation fact"),
        "the fail-closed diagnostic names the observation fact: {}",
        error[0].message
    );
    assert!(error[0].message.contains("producing launch"));
}

/// The constructor's projected wire round-trips through the radix decode
/// boundary: the F1–F7 admission rules run on the faber-produced wire at
/// package load, so the axes the constructor carries are exactly the facts
/// the hosts admit (U2 done-when: wire round-trip carries all axes).
#[test]
fn constructor_wire_admits_through_radix_decode() {
    let (program, semantics) = two_kernel_fixture();
    let section = section_for_program(&program, &semantics);
    let image = radix_mir_fmir::FmirBinaryImageFile {
        version: radix_mir_fmir::PACKAGE_MIR_ARTIFACT_VERSION,
        target: radix_mir_fmir::FMIR_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: "main.fab".to_owned(),
        entry_function: "run_entry".to_owned(),
        toolchain: radix_mir_fmir::FmirTextToolchainSection {
            faber_cli_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        runtime: radix_mir_fmir::FmirTextRuntimeSection {
            requirement: vec!["host:argv".to_owned()],
        },
        sources: radix_mir_fmir::FmirTextSourcesSection { source: Vec::new() },
        cli: None,
        exit_code: None,
        types: radix_mir_fmir::FmirTextTypesSection {
            table: radix::semantic::TypeTable::new().snapshot(),
        },
        interner: Vec::new(),
        program: radix::mir::MirProgram::new(),
        device: Some(section),
    };
    let bytes = radix_mir_fmir::encode_binary_image(&image).expect("encode binary image");
    let decoded = radix_mir_fmir::decode_binary_image(&bytes, env!("CARGO_PKG_VERSION"))
        .expect("the faber-projected wire admits at the radix decode boundary");
    let wire = &decoded
        .device
        .expect("device section present")
        .device_program
        .program;
    assert_eq!(wire.kernels.len(), 2);
    assert_eq!(wire.results.len(), 1);
    assert_eq!(wire.semantic_values.len(), 3);
    assert_eq!(wire.roots, vec![1]);
    assert_eq!(wire.dependencies.len(), 1);
    // The semantic values ride the wire with their carried MIR origins:
    // every value here flows from a kernel-slot MIR local (a, medius,
    // result) — never a synthetic buffer-id label.
    let origins: Vec<_> = wire
        .semantic_values
        .iter()
        .map(|v| v.origin.clone())
        .collect();
    assert_eq!(origins.len(), 3);
    assert!(
        origins
            .iter()
            .all(|origin| matches!(origin, WireSemanticValueOrigin::MirLocal { .. })),
        "the two-kernel chain's values are minted from carried MIR locals: {origins:?}"
    );
}

// ── Stage 3R U4: faithful materialization ─────────────────────────────────

/// Decode-admission helper: encode the section as a binary FMIR image and
/// decode it back, running the radix F1–F7 wire admission. Returns the
/// admitted wire program.
fn wire_admits_through_radix_decode(section: FmirDeviceSection) -> WireDeviceProgram {
    let image = radix_mir_fmir::FmirBinaryImageFile {
        version: radix_mir_fmir::PACKAGE_MIR_ARTIFACT_VERSION,
        target: radix_mir_fmir::FMIR_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: "main.fab".to_owned(),
        entry_function: "run_entry".to_owned(),
        toolchain: radix_mir_fmir::FmirTextToolchainSection {
            faber_cli_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        runtime: radix_mir_fmir::FmirTextRuntimeSection {
            requirement: vec!["host:argv".to_owned()],
        },
        sources: radix_mir_fmir::FmirTextSourcesSection { source: Vec::new() },
        cli: None,
        exit_code: None,
        types: radix_mir_fmir::FmirTextTypesSection {
            table: radix::semantic::TypeTable::new().snapshot(),
        },
        interner: Vec::new(),
        program: radix::mir::MirProgram::new(),
        device: Some(section),
    };
    let bytes = radix_mir_fmir::encode_binary_image(&image).expect("encode binary image");
    let decoded = radix_mir_fmir::decode_binary_image(&bytes, env!("CARGO_PKG_VERSION"))
        .expect("the projected wire admits at the radix decode boundary");
    decoded
        .device
        .expect("device section present")
        .device_program
        .program
}

/// F1 red test at the constructor: two UNRELATED same-name/same-shape values
/// (two kernels each writing an `out` buffer of the same shape) materialize
/// to distinct semantic identities and never alias — no unified buffer, no
/// shared value, no aliased origin.
#[test]
fn same_name_same_shape_unrelated_outputs_never_alias() {
    let (program, semantics) = with_inline_package(
        "u4-same-name",
        r#"@ nucleum
functio produco_a(tf32[2] x, tf32[2] out, u32 id) → vacuum {
    fixum f32 total ← x.summa()
    out[id] ← total
}
@ nucleum
functio produco_b(tf32[2] y, tf32[2] out, u32 id) → vacuum {
    fixum f32 total ← y.summa()
    out[id] ← total
}"#,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("fixture yields a device program")
        },
    )
    .expect("fixture lowers");

    // Four distinct buffers: x, out(a), y, out(b) — the two same-name
    // same-shape outputs never unify into one BufferId.
    let buffers = program.buffer_registry();
    assert_eq!(buffers.buffers.len(), 4);
    let outs: Vec<_> = buffers
        .buffers
        .iter()
        .filter(|buffer| buffer.identity.name == "out")
        .collect();
    assert_eq!(outs.len(), 2, "two unrelated `out` buffers, never aliased");
    assert_ne!(
        outs[0].identity.id, outs[1].identity.id,
        "the two same-name same-shape values get distinct buffer identities"
    );

    // The two `out` values carry distinct semantic identities — one per
    // producing kernel's MIR local, never a name/shape coincidence.
    let out_values: Vec<_> = semantics
        .values
        .iter()
        .filter(|value| value.name == "out")
        .collect();
    assert_eq!(out_values.len(), 2);
    assert_ne!(out_values[0].origin, out_values[1].origin);
    assert!(out_values
        .iter()
        .all(|value| matches!(value.origin, SemanticValueOrigin::MirLocal { .. })));

    // The projected wire admits through the radix decode boundary with the
    // distinct identities intact (never aliased).
    let wire = wire_admits_through_radix_decode(section_for_program(&program, &semantics));
    let wire_outs: Vec<_> = wire
        .semantic_values
        .iter()
        .filter(|value| value.name == "out")
        .collect();
    assert_eq!(wire_outs.len(), 2);
    assert_ne!(wire_outs[0].origin, wire_outs[1].origin);
    assert_eq!(wire.results.len(), 2, "one declared output per buffer");
    assert_ne!(wire.results[0].buffer.id, wire.results[1].buffer.id);
}

/// F1: renaming a value's symbol leaves the program graph unchanged — the
/// semantic identity derives from the carried MIR local (stable under
/// rename), never from the diagnostic name.
#[test]
fn symbol_rename_leaves_program_graph_unchanged() {
    let source = |intermediate: &str| {
        format!(
            r#"@ nucleum
functio collige(tf32[1024] a, tf32[4] {intermediate}, u32 id) → vacuum {{
    fixum f32 total ← a.summa()
    {intermediate}[id] ← total
}}
@ nucleum
functio recollige(tf32[4] {intermediate}, tf32[1] result, u32 id) → vacuum {{
    fixum f32 total ← {intermediate}.summa()
    result[id] ← total
}}"#
        )
    };
    let lower = |name: &str, intermediate: &str| {
        with_inline_package(name, &source(intermediate), |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("fixture yields a device program")
        })
        .expect("fixture lowers")
    };
    let (named_program, named_semantics) = lower("u4-rename-a", "medius");
    let (renamed_program, renamed_semantics) = lower("u4-rename-b", "medius2");

    let graph = |program: &DeviceProgram, semantics: &DeviceSemantics| {
        (
            program
                .buffer_registry()
                .buffers
                .iter()
                .map(|buffer| buffer.identity.id)
                .collect::<Vec<_>>(),
            semantics
                .values
                .iter()
                .map(|value| (value.id, value.origin.clone()))
                .collect::<Vec<_>>(),
            program
                .launches
                .iter()
                .map(|launch| (launch.id, launch.kernel_index))
                .collect::<Vec<_>>(),
            program
                .results
                .iter()
                .map(|result| (result.buffer.id, result.produced_by))
                .collect::<Vec<_>>(),
            program.buffer_registry().data_flow_pairs(),
        )
    };
    let named = graph(&named_program, &named_semantics);
    let renamed = graph(&renamed_program, &renamed_semantics);
    assert_eq!(named.0, renamed.0, "buffer identities are name-independent");
    assert_eq!(
        named.1, renamed.1,
        "value identities + origins are name-independent"
    );
    assert_eq!(named.2, renamed.2, "launch order is name-independent");
    assert_eq!(named.3, renamed.3, "results are name-independent");
    assert_eq!(named.4, renamed.4, "data-flow edges are name-independent");

    // Only the diagnostic name differs — proving the identity did NOT follow
    // the name.
    let name_of = |program: &DeviceProgram, id: BufferId| {
        program
            .buffer_registry()
            .buffer(id)
            .map(|buffer| buffer.identity.name.clone())
    };
    assert_eq!(
        name_of(&named_program, BufferId(2)).as_deref(),
        Some("medius")
    );
    assert_eq!(
        name_of(&renamed_program, BufferId(2)).as_deref(),
        Some("medius2")
    );
}

/// F2: a value written twice materializes two explicit generations with
/// correct producers/consumers — a later write is a NEW generation, never
/// another producer of the same one.
#[test]
fn double_write_carries_two_explicit_generations() {
    let mut types = radix::semantic::TypeTable::new();
    let element_ty = MirType::semantic(types.sized_numeric(
        radix::semantic::Primitive::Fractus,
        radix::semantic::NumericWidth::F32,
    ));
    let identity = BufferIdentity {
        id: BufferId(1),
        name: "acc".to_owned(),
        role: BufferRole::Output,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime: BufferLifetime::ObservationPoint,
    };
    let version = BufferVersion {
        version: 1,
        element_ty,
        element_count: 4,
    };
    let launch_plan = KernelLaunchPlan {
        workgroup: radix_mir::abi::MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: radix_mir::abi::MirKernelDispatchSize { x: 1, y: 1, z: 1 },
        workgroup_count: radix_mir::abi::MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    };
    let kernel = |entry: &str, function: u32| KernelUnit {
        function: MirFunctionId(function),
        entry: entry.to_owned(),
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![DeviceResource {
            buffer: identity.clone(),
            version: version.clone(),
            binding: Binding {
                group: 0,
                binding: 1,
            },
            access: MirKernelResourceAccess::Write,
        }],
        launch: launch_plan,
    };
    let program = DeviceProgram {
        kernels: vec![kernel("step_1", 0), kernel("step_2", 1)],
        launches: vec![
            LaunchUnit {
                id: LaunchId(1),
                kernel_index: 0,
            },
            LaunchUnit {
                id: LaunchId(2),
                kernel_index: 1,
            },
        ],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: Vec::new(),
    };
    let semantics = DeviceSemantics {
        values: vec![SemanticValue {
            id: SemanticValueId(1),
            name: "acc".to_owned(),
            element_ty,
            element_count: 4,
            origin: SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 1,
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
                element_count: 4,
                produced_by: LaunchId(1),
            },
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 2,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(2),
            },
        ],
        roots: vec![LaunchId(1), LaunchId(2)],
        dependencies: Vec::new(),
        relations: Vec::new(),
        initializations: vec![InitializationFact {
            buffer: BufferId(1),
            policy: InitializationPolicy::KernelInitialized,
        }],
        observations: Vec::new(),
    };
    program
        .validate_with_semantics(&semantics)
        .expect("two distinct generations validate (F2)");

    // The wire carries the two explicit generations: each write slot names
    // its own generation — never a universal `1` and never a second producer
    // of the same generation.
    let wire = wire_program_for_program(&program, &semantics);
    assert_eq!(wire.kernels[0].resources[0].generation, 1);
    assert_eq!(wire.kernels[1].resources[0].generation, 2);
    assert_eq!(wire.semantic_values.len(), 1);
}

/// F2/F3: the ordinary two-kernel chain's generations and producers are
/// correct on the wire — the intermediate's write is generation 1 produced
/// by launch 1 and consumed by launch 2, the final output's write is
/// generation 1 produced by launch 2.
#[test]
fn two_kernel_chain_generations_and_producers_are_correct() {
    let (program, semantics) = two_kernel_fixture();
    let generations: Vec<_> = semantics
        .generations
        .iter()
        .map(|generation| {
            (
                generation.value.0,
                generation.generation,
                generation.produced_by.0,
            )
        })
        .collect();
    assert_eq!(
        generations,
        vec![(2, 1, 1), (3, 1, 2)],
        "medius generation 1 produced by launch 1; result generation 1 by launch 2"
    );
    let wire = wire_program_for_program(&program, &semantics);
    // collige: a-read consumes gen 1 (initial state), medius-write produces
    // gen 1; recollige: medius-read consumes gen 1, result-write produces
    // gen 1.
    assert_eq!(wire.kernels[0].resources[0].generation, 1);
    assert_eq!(wire.kernels[0].resources[1].generation, 1);
    assert_eq!(wire.kernels[1].resources[0].generation, 1);
    assert_eq!(wire.kernels[1].resources[1].generation, 1);
}

/// F3: the launch sequence follows the carried producer/consumer dependency
/// graph, never kernel declaration order. A chain DECLARED backwards
/// (consumer first) still executes producer → consumer.
#[test]
fn launches_follow_carried_dependencies_not_declaration_order() {
    let (program, semantics) = with_inline_package(
        "u4-reversed-chain",
        r#"@ nucleum
functio recollige(tf32[4] medius, tf32[1] result, u32 id) → vacuum {
    fixum f32 total ← medius.summa()
    result[id] ← total
}
@ nucleum
functio collige(tf32[1024] a, tf32[4] medius, u32 id) → vacuum {
    fixum f32 total ← a.summa()
    medius[id] ← total
}"#,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("fixture yields a device program")
        },
    )
    .expect("fixture lowers");

    // recollige is DECLARED first (kernel 0, launch id 1) but DEPENDS on
    // collige's write of medius. The execution sequence therefore runs
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
    assert_eq!(semantics.roots, vec![LaunchId(2)]);
    assert_eq!(
        semantics.dependencies,
        vec![DependencyEdge {
            producer: LaunchId(2),
            consumer: LaunchId(1),
            // medius is minted first here (recollige is declared first), so
            // the shared intermediate is buffer 1.
            buffer: BufferId(1),
            version: 1,
        }]
    );

    // The wire admits through the radix decode boundary with the carried
    // order + roots + dependency intact.
    let wire = wire_admits_through_radix_decode(section_for_program(&program, &semantics));
    assert_eq!(wire.roots, vec![2]);
    assert_eq!(
        wire.launches
            .iter()
            .map(|launch| (launch.id, launch.kernel_index))
            .collect::<Vec<_>>(),
        vec![(2, 1), (1, 0)]
    );
}

/// F4: the lossless primal/companion relation (selected inputs with the
/// gradient-to-primal identity) projects from the carried carrier onto the
/// serialized wire and admits through the radix decode boundary.
#[test]
fn companion_relation_projected_onto_the_wire() {
    let entry = PathBuf::from("/tmp/s3a2probe/src/probe.fab");
    let (program, semantics) = super::super::with_lowered_package_mir(
        &radix::driver::Config::default()
            .with_stdlib(dev_norma_library_home())
            .with_target(radix::codegen::Target::Fmir),
        &entry,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
                .expect("constructor succeeds")
                .expect("device package yields a device program")
        },
    )
    .expect("fixture lowers");

    // The lossless row rides the carried semantics (F4).
    assert_eq!(semantics.relations.len(), 1);
    let wire = wire_program_for_program(&program, &semantics);
    assert_eq!(wire.relations.len(), 1);
    let relation = &wire.relations[0];
    assert!(relation.device_resident);
    assert_eq!(
        relation.derivative,
        radix_mir_fmir::schema::WireCompanionDerivativeKind::ReverseModeVjp
    );
    // The gradient-to-primal identity survives: each selected input names
    // the companion result-tuple slot carrying its gradient.
    assert_eq!(relation.selected_inputs.len(), 2, "x and w selected");
    assert_eq!(relation.selected_inputs[0].gradient_slot, 0);
    assert_eq!(relation.selected_inputs[1].gradient_slot, 1);
    assert_eq!(relation.selected_outputs.len(), 1);

    // The wire admits through the radix decode boundary with the relation
    // intact (F4 + F7).
    let admitted = wire_admits_through_radix_decode(section_for_program(&program, &semantics));
    assert_eq!(admitted.relations.len(), 1);
}

// ── S5-U5: RepeatingStep constructor materialization + route step-driving ──

/// The S5-U5 training-loop fixture: a device-resident forward (mul + add),
/// its generated companion, a package-local train_step (SGD update:
/// param' = param − grad) called from a constant-bounded entry loop, and the
/// loop's re-bindings. The declared source loop bound (100) matches the
/// default declared RepeatingStep step count, so the constructor admits it.
///
/// The forward returns a TENSOR (not `fractus`): a scalar-return primal's
/// companion carries an f64-typed upstream gradient, outside the current
/// kernel scalar layout surface (the campaign dtype is f32) — a tensor
/// forward keeps every companion param an f32 device view.
const TRAINING_LOOP_FIXTURE_100: &str = r#"@ nucleum
@ radix lane "air"
@ radix backward "loss_backward"
functio loss(tf32[2] x, tf32[2] w, tf32[2] b) → tf32[2] {
    fixum tf32[2] product ← x.multiplica(w)
    redde product.addita(b)
}

functio train_step(tf32[2] w, tf32[2] b, tf32[2] gw, tf32[2] gb) → iuncta<tf32[2], tf32[2]> {
    fixum tf32[2] nw ← w.subtrahe(gw)
    fixum tf32[2] nb ← b.subtrahe(gb)
    redde iuncta<tf32[2], tf32[2]> [nw, nb]
}

functio nil() → vacuum {
    tacet
}

incipit {
    fixum lista<f32> data_x ← [1.0, 2.0]
    fixum lista<f32> data_w ← [0.5, 0.5]
    fixum lista<f32> data_b ← [0.1, 0.1]
    fixum lista<f32> data_u ← [1.0, 1.0]
    fixum tensor<f32, []> seed ← vacua
    fixum tensor<f32, [2]> x ← seed.strue(data_x, [2])
    varia tensor<f32, [2]> w ← seed.strue(data_w, [2])
    varia tensor<f32, [2]> b ← seed.strue(data_b, [2])
    fixum tensor<f32, [2]> upstream ← seed.strue(data_u, [2])
    fixum numerus steps ← 100
    itera ab 0‥steps fixum step {
        fixum _ grads ← loss_backward(x, w, b, nil(), upstream)
        fixum [_, gw, gb] ← grads
        fixum [nw, nb] ← train_step(w, b, gw, gb)
        w ← nw
        b ← nb
    }
}
"#;

/// The same training loop with a source bound (8) that contradicts the
/// default declared RepeatingStep step count (100) — the step-count
/// admission must fail closed.
const TRAINING_LOOP_FIXTURE_MISMATCH: &str = r#"@ nucleum
@ radix lane "air"
@ radix backward "loss_backward"
functio loss(tf32[2] x, tf32[2] w, tf32[2] b) → tf32[2] {
    fixum tf32[2] product ← x.multiplica(w)
    redde product.addita(b)
}

functio train_step(tf32[2] w, tf32[2] b, tf32[2] gw, tf32[2] gb) → iuncta<tf32[2], tf32[2]> {
    fixum tf32[2] nw ← w.subtrahe(gw)
    fixum tf32[2] nb ← b.subtrahe(gb)
    redde iuncta<tf32[2], tf32[2]> [nw, nb]
}

functio nil() → vacuum {
    tacet
}

incipit {
    fixum lista<f32> data_x ← [1.0, 2.0]
    fixum lista<f32> data_w ← [0.5, 0.5]
    fixum lista<f32> data_b ← [0.1, 0.1]
    fixum lista<f32> data_u ← [1.0, 1.0]
    fixum tensor<f32, []> seed ← vacua
    fixum tensor<f32, [2]> x ← seed.strue(data_x, [2])
    varia tensor<f32, [2]> w ← seed.strue(data_w, [2])
    varia tensor<f32, [2]> b ← seed.strue(data_b, [2])
    fixum tensor<f32, [2]> upstream ← seed.strue(data_u, [2])
    fixum numerus steps ← 8
    itera ab 0‥steps fixum step {
        fixum _ grads ← loss_backward(x, w, b, nil(), upstream)
        fixum [_, gw, gb] ← grads
        fixum [nw, nb] ← train_step(w, b, gw, gb)
        w ← nw
        b ← nb
    }
}
"#;

/// Build the training fixture's RepeatingStep device program through the
/// ordinary constructor.
fn training_fixture_program(source: &str, name: &str) -> (DeviceProgram, DeviceSemantics) {
    with_inline_package(name, source, |lowered| {
        device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
            .expect("constructor succeeds")
            .expect("training fixture yields a device program")
    })
    .expect("fixture lowers")
}

/// The S5-U5 constructor materializes the MLP-shaped training program: the
/// decomposed forward + backward + train_step kernel set on a RepeatingStep
/// lifetime, with the trainable params projected to PerProgram InOut buffers
/// with HostProvided init, and the loss observation as the single declared
/// result.
#[test]
fn repeating_step_constructor_materializes_training_program() {
    let (program, semantics) = training_fixture_program(TRAINING_LOOP_FIXTURE_100, "s5u5-train-100");

    program.validate().expect("constructed program validates");
    assert_eq!(
        program.lifetime,
        DeviceProgramLifetime::RepeatingStep,
        "a training loop materializes a RepeatingStep program"
    );

    // Kernel set, in materialization order: forward, companion, train_step.
    let entries: Vec<&str> = program.kernels.iter().map(|kernel| kernel.entry.as_str()).collect();
    assert_eq!(
        entries,
        vec!["loss", "loss_backward", "train_step"],
        "forward + companion + optimizer-step kernels"
    );

    // The trainable params w/b project to PerProgram InOut buffers.
    let param_ids: Vec<BufferId> = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .filter(|resource| resource.buffer.name == "w" || resource.buffer.name == "b")
        .map(|resource| resource.buffer.id)
        .collect();
    let mut seen = Vec::new();
    for id in &param_ids {
        if !seen.contains(id) {
            seen.push(*id);
        }
    }
    assert_eq!(seen.len(), 2, "w and b are distinct buffers");

    // The train_step kernel binds each param as an InOut slot AND aliases its
    // update output slot to the same buffer (the in-place update).
    let step = &program.kernels[2];
    for id in &seen {
        let slots: Vec<&radix_mir::device_program::DeviceResource> =
            step.resources.iter().filter(|resource| resource.buffer.id == *id).collect();
        assert_eq!(
            slots.len(),
            2,
            "the train_step kernel reads and writes buffer {id:?} (in-place update)"
        );
        for slot in &slots {
            assert_eq!(slot.buffer.role, BufferRole::InOut);
            assert_eq!(slot.buffer.lifetime, BufferLifetime::PerProgram);
        }
        assert!(
            slots.iter().any(|slot| slot.access == MirKernelResourceAccess::Read)
                && slots.iter().any(|slot| slot.access == MirKernelResourceAccess::Write),
            "the param buffer has a read slot and a write slot in the step kernel"
        );
    }

    // The forward kernels read the params as plain reads of the same buffers.
    let forward = &program.kernels[0];
    for id in &seen {
        let read_slots: Vec<_> = forward
            .resources
            .iter()
            .filter(|resource| resource.buffer.id == *id)
            .collect();
        assert!(
            !read_slots.is_empty(),
            "the forward kernel reads param buffer {id:?}"
        );
        assert!(
            read_slots
                .iter()
                .all(|slot| slot.access == MirKernelResourceAccess::Read),
            "the forward reads the param without writing it"
        );
    }

    // F6: the ONLY declared observation is the loss result (the forward's
    // final output); the gradient intermediates and the param aliases are
    // never results merely because they are writable.
    assert_eq!(program.results.len(), 1, "one loss observation point");
    assert_eq!(program.results[0].buffer.role, BufferRole::Output);

    // The semantics mint HostProvided init for the params (never ZeroFill
    // despite the InOut access) and carry no param dependency edge (the
    // persistent-state exclusion keeps the single-step graph acyclic).
    for fact in &semantics.initializations {
        let buffer = seen.iter().find(|id| **id == fact.buffer);
        if let Some(id) = buffer {
            assert_eq!(
                fact.policy,
                InitializationPolicy::HostProvided,
                "param buffer {id:?} is HostProvided, copied in exactly once"
            );
        }
    }
    let param_edges: Vec<_> = semantics
        .dependencies
        .iter()
        .filter(|edge| seen.contains(&edge.buffer))
        .collect();
    assert!(
        param_edges.is_empty(),
        "no param edge rides the dependency graph (persistent state)"
    );

    // F3: the launch sequence is acyclic and covers every kernel.
    let launches: Vec<u32> = program.launches.iter().map(|launch| launch.id.0).collect();
    assert_eq!(launches, vec![1, 2, 3]);
}

/// The training-plan analysis derives the trainable-param identity rows from
/// the entry loop's value graph (param-identity.md): w at param position 0
/// updates result-tuple slot 0; b at position 1 updates slot 1. The step
/// count is the default declared 100 and the source loop bound is 100.
#[test]
fn training_plan_facts_derives_trainable_param_identity() {
    with_inline_package("s5u5-facts", TRAINING_LOOP_FIXTURE_100, |lowered| {
        let facts = training_plan_facts(&lowered.validated, &lowered.interner, &lowered.companions)
            .expect("training facts resolve")
            .expect("a training loop is detected");

        assert_eq!(facts.step_count, DEFAULT_TRAINING_STEPS);
        assert_eq!(facts.source_loop_bound, Some(100));

        assert_eq!(facts.trainable.len(), 2, "w and b are the trainable params");
        let first = &facts.trainable[0];
        assert_eq!(first.param_position, 0, "w is the first step param");
        assert_eq!(first.update_output, 0, "w's update is tuple slot 0");
        let second = &facts.trainable[1];
        assert_eq!(second.param_position, 1, "b is the second step param");
        assert_eq!(second.update_output, 1, "b's update is tuple slot 1");
    })
    .expect("fixture lowers");
}

/// Step-count admission fails closed: a declared count that contradicts the
/// source loop bound (when present) is rejected — both at the pure
/// admission function and through the constructor (a training loop bounded
/// at 8 steps contradicts the default declared 100).
#[test]
fn repeating_step_step_count_mismatch_fails_closed() {
    // Pure admission: declared 100 vs source bound 8.
    let error = admit_step_count(100, Some(8)).expect_err("declared vs source mismatch fails");
    assert!(
        error[0].message.contains("contradicts the source training loop bound"),
        "the typed diagnostic names the contradiction: {}",
        error[0].message
    );
    // Agreement is admitted.
    admit_step_count(100, Some(100)).expect("agreement is admitted");
    admit_step_count(100, None).expect("no source bound → the declared count applies");

    // Constructor: the 8-step training loop cannot build a RepeatingStep
    // image under the default declared 100.
    let result = with_inline_package("s5u5-train-8", TRAINING_LOOP_FIXTURE_MISMATCH, |lowered| {
        device_program_for_lowered(&lowered.validated, &lowered.interner, &lowered.companions)
    })
    .expect("fixture lowers");
    let diagnostics = result.expect_err("a declared/source step-count mismatch fails construction");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("step count")),
        "the fail-closed diagnostic names the step-count contradiction: {:?}",
        diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>()
    );
}

/// The RepeatingStep wire + host descriptor projection (the image/host agree
/// surface): the wire carries the RepeatingStep lifetime, the params as
/// InOut + PerProgram + HostProvided, and the descriptor admits through
/// `descriptor_for_backend` with the same facts and the loss observation.
#[test]
fn repeating_step_wire_and_descriptor_project_params_and_observation() {
    let (program, semantics) = training_fixture_program(TRAINING_LOOP_FIXTURE_100, "s5u5-wire");
    let mut section = section_for_program(&program, &semantics);
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
        symbols: Vec::new(),
    }];
    section.declared_inputs = vec![
        FmirDeviceInput {
            name: "x".to_owned(),
            values: vec![1.0, 2.0],
        },
        FmirDeviceInput {
            name: "w".to_owned(),
            values: vec![0.5, 0.5],
        },
        FmirDeviceInput {
            name: "b".to_owned(),
            values: vec![0.1, 0.1],
        },
    ];

    let wire = &section.device_program.program;
    assert_eq!(wire.lifetime, WireProgramLifetime::RepeatingStep);

    // The params ride the wire as InOut + PerProgram + HostProvided.
    let mut param_slots: Vec<&WireDeviceResource> = Vec::new();
    for kernel in &wire.kernels {
        for resource in &kernel.resources {
            if resource.buffer.name == "w" || resource.buffer.name == "b" {
                if !param_slots
                    .iter()
                    .any(|slot| slot.buffer.id == resource.buffer.id && slot.access == WireResourceAccess::Write)
                {
                    param_slots.push(resource);
                }
            }
        }
    }
    for slot in &param_slots {
        assert_eq!(slot.buffer.role, WireBufferRole::InOut);
        assert_eq!(slot.buffer.lifetime, WireBufferLifetime::PerProgram);
        assert_eq!(slot.initialization, WireInitializationPolicy::HostProvided);
    }

    // The descriptor admits with the same facts + the loss observation.
    let descriptor = descriptor_for_backend(&section, DeviceBackend::Metal, b"msl blob")
        .expect("the RepeatingStep wire constructs an admissible host descriptor");
    descriptor
        .validate()
        .expect("constructor and host admission agree on the RepeatingStep program");
    assert_eq!(
        descriptor.program_lifetime,
        HostDeviceProgramLifetime::RepeatingStep
    );
    let param_versions: Vec<_> = descriptor
        .buffer_versions
        .iter()
        .filter(|version| {
            descriptor.kernels.iter().any(|kernel| {
                kernel.buffers.iter().any(|slot| {
                    slot.buffer_id == version.buffer_id
                        && (slot.buffer_name == "w" || slot.buffer_name == "b")
                        && slot.lifetime == DeviceBufferLifetime::PerProgram
                        && slot.initialization == DeviceBufferInitialization::HostProvided
                })
            })
        })
        .collect();
    assert_eq!(param_versions.len(), 2, "w and b params project to the descriptor");
    assert_eq!(descriptor.results.len(), 1, "the loss observation is the readback set");
}

/// The route step-driving core (S5-U5): a RepeatingStep session once-inits
/// its HostProvided params, executes N steps on ONE session, collects the
/// per-step loss trace, and reports the convergence verdict. Driven on the
/// fake Metal driver — no real device required.
#[test]
fn repeating_step_route_runs_steps_once_init_loss_trace_convergence() {
    // A RepeatingStep descriptor the fake driver can simulate:
    //   launch 1 `accumulate`: inc (PerProgram HostProvided) += param p
    //   launch 2 `observa`:    loss (ObservationPoint) = copy(p)
    // p starts at 1.0, inc = -0.01 → p decreases toward 0; loss = p.
    let descriptor = DeviceDescriptor {
        backend: DeviceBackend::Metal,
        module_image: b"fake msl".to_vec(),
        kernels: vec![
            DescriptorKernel {
                entry: "accumulate".to_owned(),
                buffers: vec![
                    DescriptorBuffer {
                        buffer_id: 1,
                        buffer_name: "inc".to_owned(),
                        semantic_value: 1,
                        role: DeviceBufferRole::Input,
                        lifetime: DeviceBufferLifetime::PerProgram,
                        initialization: DeviceBufferInitialization::HostProvided,
                        binding: 0,
                        element_ty: DeviceDataType::F32,
                        element_count: 1,
                        version: 1,
                    },
                    DescriptorBuffer {
                        buffer_id: 2,
                        buffer_name: "p".to_owned(),
                        semantic_value: 2,
                        role: DeviceBufferRole::InOut,
                        lifetime: DeviceBufferLifetime::PerProgram,
                        initialization: DeviceBufferInitialization::HostProvided,
                        binding: 1,
                        element_ty: DeviceDataType::F32,
                        element_count: 1,
                        version: 1,
                    },
                ],
                grid: [1, 1, 1],
                block: [1, 1, 1],
            },
            DescriptorKernel {
                entry: "observa".to_owned(),
                buffers: vec![
                    DescriptorBuffer {
                        buffer_id: 2,
                        buffer_name: "p".to_owned(),
                        semantic_value: 2,
                        role: DeviceBufferRole::Input,
                        lifetime: DeviceBufferLifetime::PerProgram,
                        initialization: DeviceBufferInitialization::HostProvided,
                        binding: 0,
                        element_ty: DeviceDataType::F32,
                        element_count: 1,
                        version: 1,
                    },
                    DescriptorBuffer {
                        buffer_id: 3,
                        buffer_name: "loss".to_owned(),
                        semantic_value: 3,
                        role: DeviceBufferRole::Output,
                        lifetime: DeviceBufferLifetime::ObservationPoint,
                        initialization: DeviceBufferInitialization::KernelInitialized,
                        binding: 1,
                        element_ty: DeviceDataType::F32,
                        element_count: 1,
                        version: 1,
                    },
                ],
                grid: [1, 1, 1],
                block: [1, 1, 1],
            },
        ],
        launches: vec![
            DescriptorLaunch { id: 1, kernel_index: 0 },
            DescriptorLaunch { id: 2, kernel_index: 1 },
        ],
        buffer_versions: vec![
            DescriptorBufferVersion {
                buffer_id: 1,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: 1,
            },
            DescriptorBufferVersion {
                buffer_id: 2,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: 1,
            },
            DescriptorBufferVersion {
                buffer_id: 3,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: 1,
            },
        ],
        program_lifetime: HostDeviceProgramLifetime::RepeatingStep,
        data_flow: Vec::new(),
        roots: vec![1, 2],
        results: vec![DescriptorResult {
            buffer_id: 3,
            version: 1,
            produced_by: 2,
            at_launch: 2,
        }],
    };
    descriptor.validate().expect("hand-built descriptor validates");

    let mut host = CompositeHost::with_device(
        DeviceRuntime::Metal(
            MetalHostSession::with_driver(Box::new(
                faber_host_macos_arm64::FakeMetalDriver::default()
                    .with_known_entry("accumulate")
                    .with_known_entry("observa"),
            ))
            .expect("fake metal admit"),
        ),
        "fake-metal-device",
    )
    .expect("fake host");

    // The once-init params: inc = -0.01 (each step p += inc), p = 1.0.
    let mut params = BTreeMap::new();
    params.insert(1, vec![-0.01]);
    params.insert(2, vec![1.0]);

    let mut session = super::super::host_factory::create_program_session(&mut host, &descriptor)
        .expect("fake session");

    let steps = 100u32;
    let receipts =
        execute_session_receipts(&mut session, &descriptor, &params, steps).expect("steps execute");
    session.teardown().expect("teardown");

    assert_eq!(receipts.len(), 100, "exactly 100 steps on ONE session");
    // The loss trace strictly decreases: loss_k = 1.0 − 0.01·(k+1).
    let report = step_run_report(&receipts);
    assert_eq!(report.step_count, 100);
    assert_eq!(report.loss_trace.len(), 100);
    let first = report.loss_trace[0].get(&3).expect("loss observed").first().copied().unwrap();
    let last = report
        .loss_trace
        .last()
        .and_then(|observed| observed.get(&3))
        .and_then(|values| values.first())
        .copied()
        .unwrap();
    assert!((first - 0.99).abs() < 1e-6, "first loss = 1.0 − 0.01");
    assert!(last < 0.1 * first, "the loss decreased below 10% of the initial");
    assert!(
        report.converged,
        "final loss < 0.1 * initial loss → converged"
    );

    // Leak-free: teardown returns the live handle count to 0.
    assert_eq!(host.device().map(|runtime| runtime.live_handle_count()).unwrap_or(0), 0);
}

/// `FABER_DEVICE_STEPS` drives the repeating step count fail-closed: a
/// non-numeric value is rejected; the default is [`DEFAULT_TRAINING_STEPS`].
#[test]
fn device_step_count_defaults_and_fails_closed() {
    std::env::remove_var("FABER_DEVICE_STEPS");
    assert_eq!(
        device_step_count().expect("default"),
        DEFAULT_TRAINING_STEPS
    );
    std::env::set_var("FABER_DEVICE_STEPS", "250");
    assert_eq!(device_step_count().expect("explicit"), 250);
    std::env::set_var("FABER_DEVICE_STEPS", "not-a-number");
    let error = device_step_count().expect_err("non-numeric value fails closed");
    assert!(error[0].message.contains("FABER_DEVICE_STEPS"));
    std::env::remove_var("FABER_DEVICE_STEPS");
}
/// The constructor's RepeatingStep param materialization is
/// emitter-compatible: the forward kernel (PerProgram InOut HostProvided
/// param bindings) emits through the S5-U1b decomposition-aware Metal
/// emitter — the carried subchain plan and the InOut param bindings are
/// consumed without a real device. (The full training image's multi-output
/// elementwise train_step + companion chains exceed the current emitter
/// elementwise surface — a filed radix-mir-metal gap, S5-U7 blocker.)
#[test]
fn repeating_step_forward_kernel_emits_metal_artifact() {
    with_inline_package("s5u5-emit", TRAINING_LOOP_FIXTURE_100, |lowered| {
        let (program, _semantics) = device_program_for_lowered(
            &lowered.validated,
            &lowered.interner,
            &lowered.companions,
        )
        .expect("constructor succeeds")
        .expect("training fixture yields a device program");

        // The forward kernel alone: the constructor's kernel resources must
        // agree with the carried plan and the ABI signature (`carried_resources_agree`).
        let forward = program
            .kernels
            .iter()
            .find(|kernel| kernel.entry == "loss")
            .expect("the forward kernel");
        let mut one = radix_mir::device_program::DeviceProgram::new(
            radix_mir::device_program::DeviceProgramLifetime::SingleRun,
        );
        one.kernels.push(forward.clone());
        one.launches.push(radix_mir::device_program::LaunchUnit {
            id: radix_mir::device_program::LaunchId(1),
            kernel_index: 0,
        });
        let artifact = radix_mir_metal::emit_metal_device_artifact(
            &one,
            &lowered.validated,
            &lowered.interner,
        )
        .expect("the forward kernel emits MSL");
        assert!(artifact.source.contains("kernel void loss("), "the MSL declares the forward kernel");
        // The params bind as typed device buffers (the InOut program role on
        // the forward's read slots is compatible with the ABI's (Input, Read)
        // pair — carried_resources_agree).
        assert!(
            artifact.source.contains("device const float* w_in [[buffer(1)]]"),
            "the forward's param binds as a typed device buffer:\n{}",
            artifact.source
        );
    })
    .expect("fixture lowers");
}
