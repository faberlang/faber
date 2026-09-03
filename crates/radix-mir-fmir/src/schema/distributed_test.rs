//! MD2-W1 tests: the FMIR distributed execution section (schema), admission
//! (A7 version ratchet + V1 gate + equivalence gate + H1 hash cross-check),
//! mutation tests (exit-gate bullet 4), the single-device regression (bullet
//! 5), and the section-level A10 exclusion.
//!
//! Proves the MD2-W1 done-when:
//!
//! - a wire-11 single-device image WITHOUT the section decodes, admits, and
//!   runs unchanged (the section is `None` — the independent single-device
//!   path; `admit_device_section` admits it and `device_identity_hash` is
//!   unchanged);
//! - a distributed image round-trips (build → decode → admit) with the
//!   section present;
//! - every mutation test fails admission with a focused diagnostic
//!   (delivered-owner / edge / byte-contract mutations + serialized-byte
//!   mutation), and the equivalence gate fails for every mutation — the gate
//!   inspects the delivered bytes, never a self-declared hash;
//! - a stale `WIRE_DISTRIBUTED_SECTION_VERSION` fails closed;
//! - the equivalence gate passes for the unmutated fixture.
//!
//! MD3H-F1 8-rank / 1-partition execution fixtures live in
//! `md3h_distributed_test.rs` (this module stays the MD2-W1 two-partition
//! admission surface).

use super::{
    FMIR_TARGET_NAME, FmirBinaryImageFile, FmirDeviceArtifactsSection, FmirDeviceProgramSection,
    FmirDeviceSection, FmirDeviceSelection, FmirDistributedExecutionSection,
    FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, PACKAGE_MIR_ARTIFACT_VERSION,
    SHA256_IDENTITY_PREFIX, WIRE_DEVICE_PROGRAM_VERSION, WIRE_DISTRIBUTED_SECTION_VERSION,
    WireBinding, WireBufferIdentity, WireBufferLifetime, WireBufferRole, WireBufferVersion,
    WireCollectionKernelPlan, WireDependencyEdge, WireDeviceProgram, WireDeviceResource,
    WireDispatchSize, WireExecutionOperation, WireKernelLaunchPlan, WireKernelUnit, WireLaunchUnit,
    WireObservationCadence, WireObservationFact, WirePlacementContract, WirePlacementFact,
    WireProgramLifetime, WireResourceAccess, WireResultBuffer, WireSemanticValue,
    WireSemanticValueOrigin, WireStorageLayout, WireWorkgroupCount, WireWorkgroupSize,
    admit_device_section, admit_distributed_section, canonical_program_bytes, device_identity_hash,
    equivalence_gate, logical_distributed_plan_hash, plan_to_wire_plan, wire_facts_to_facts,
    wire_plan_to_plan, wire_program_to_program,
};
use crate::error::FmirImageError;
use crate::{decode_binary_image, encode_binary_image};
use radix_mir::MirProgram;
use radix_mir::distributed::{
    DeclaredPlacement, DistributedExecutionPlan, LOGICAL_PLAN_VERSION, LogicalPlacement,
    PartitionId, PlacementConstraint, PlacementObjective, PlanVersion,
};
use radix_types::TypeTable;

/// The single-device program fixture (the section-level A10 substrate): the
/// two-partition CS-1-style mechanics shape — `w` (P0), `m` (P0, cross-read
/// by L2 on P1), `o` (P1, declared result). Passes `validate_wire_program`
/// (F1–F7).
fn wire_program_fixture() -> WireDeviceProgram {
    let w = WireBufferIdentity {
        id: 1,
        name: "w".to_owned(),
        role: WireBufferRole::Input,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerProgram,
        semantic_value: 1,
    };
    let m = WireBufferIdentity {
        id: 2,
        name: "m".to_owned(),
        role: WireBufferRole::InOut,
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
        reduced_projection: None,
    };
    let v_m = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: 128,
        reduced_projection: None,
    };
    let v_o = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: 32,
        reduced_projection: None,
    };
    let launch = WireKernelLaunchPlan {
        workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
        dispatch_size: WireDispatchSize { x: 16, y: 1, z: 1 },
        workgroup_count: WireWorkgroupCount { x: 2, y: 1, z: 1 },
    };
    let launch_b = WireKernelLaunchPlan {
        workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
        dispatch_size: WireDispatchSize { x: 8, y: 1, z: 1 },
        workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
    };
    WireDeviceProgram {
        kernels: vec![
            WireKernelUnit {
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
                        initialization: super::WireInitializationPolicy::HostProvided,
                    },
                    WireDeviceResource {
                        buffer: m.clone(),
                        version: v_m.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 1,
                        },
                        access: WireResourceAccess::Write,
                        generation: 1,
                        initialization: super::WireInitializationPolicy::KernelInitialized,
                    },
                ],
                launch,
            },
            WireKernelUnit {
                function: 1,
                entry: "emit".to_owned(),
                plan: WireCollectionKernelPlan::Elementwise,
                resources: vec![
                    WireDeviceResource {
                        buffer: m.clone(),
                        version: v_m.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 0,
                        },
                        access: WireResourceAccess::Read,
                        generation: 1,
                        initialization: super::WireInitializationPolicy::KernelInitialized,
                    },
                    WireDeviceResource {
                        buffer: o.clone(),
                        version: v_o.clone(),
                        binding: WireBinding {
                            group: 0,
                            binding: 1,
                        },
                        access: WireResourceAccess::Write,
                        generation: 1,
                        initialization: super::WireInitializationPolicy::KernelInitialized,
                    },
                ],
                launch: launch_b,
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
            buffer: o.clone(),
            version: v_o.clone(),
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
                name: "w".to_owned(),
                origin: WireSemanticValueOrigin::HostInput,
            },
            WireSemanticValue {
                id: 2,
                name: "m".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "proj-output".to_owned(),
                },
            },
            WireSemanticValue {
                id: 3,
                name: "o".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "emit-output".to_owned(),
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

/// The declared placement facts: `w` and `m` owned by partition 0, `o` owned
/// by partition 1, each with a declared byte contract.
fn wire_facts_fixture() -> Vec<WirePlacementFact> {
    vec![
        WirePlacementFact {
            value: 1,
            buffer: 1,
            contract: WirePlacementContract::Owned { owner: 0 },
            byte_count: 256,
        },
        WirePlacementFact {
            value: 2,
            buffer: 2,
            contract: WirePlacementContract::Owned { owner: 0 },
            byte_count: 512,
        },
        WirePlacementFact {
            value: 3,
            buffer: 3,
            contract: WirePlacementContract::Owned { owner: 1 },
            byte_count: 128,
        },
    ]
}

/// Assemble the logical plan the fixture derives: the declared placement
/// (facts + the CS-1 160 MiB partition budgets + one objective) + the
/// deterministic derivation.
fn sample_plan() -> DistributedExecutionPlan {
    let facts = wire_facts_to_facts(&wire_facts_fixture());
    let program = wire_program_to_program(&wire_program_fixture()).expect("fixture reconstructs");
    let (partitions, graph) = radix_mir::distributed::derive(&program, &facts);
    let mut placements: Vec<LogicalPlacement> =
        facts.iter().map(LogicalPlacement::from_fact).collect();
    placements.sort_by_key(|placement| placement.value.0);
    DistributedExecutionPlan::assemble(
        DeclaredPlacement {
            placements,
            constraints: vec![
                PlacementConstraint::PartitionBudgetBytes {
                    partition: PartitionId(0),
                    budget_bytes: 160 * 1024 * 1024,
                },
                PlacementConstraint::PartitionBudgetBytes {
                    partition: PartitionId(1),
                    budget_bytes: 160 * 1024 * 1024,
                },
            ],
            objectives: vec![PlacementObjective::MinimizeTransferBytes],
        },
        partitions,
        graph,
        PlanVersion(LOGICAL_PLAN_VERSION),
    )
}

/// The honest section: the wire mirror of the derived logical plan + the
/// declared placement facts + the H1 hash over the **wire-reconstructed**
/// plan (the schema's element-type reconstruction contract).
fn honest_section() -> FmirDistributedExecutionSection {
    let plan = sample_plan();
    let wire_plan = plan_to_wire_plan(&plan);
    let reconstructed = wire_plan_to_plan(&wire_plan).expect("fixture plan round-trips");
    FmirDistributedExecutionSection {
        v: WIRE_DISTRIBUTED_SECTION_VERSION,
        logical_plan: wire_plan,
        declared_placement: wire_facts_fixture(),
        logical_distributed_plan_hash: logical_distributed_plan_hash(&reconstructed),
    }
}

/// A device section carrying the honest distributed section.
fn device_section_with_distributed(
    distributed: Option<FmirDistributedExecutionSection>,
) -> FmirDeviceSection {
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
        distributed,
        // GI4-2: the single-device fixture carries no cadence/session
        // section (single-device packages do not require it — MD-A15).
        session: None,
    }
}

// ---------------------------------------------------------------------------
// Round trip: build → decode → admit (done-when)
// ---------------------------------------------------------------------------

#[test]
fn distributed_section_round_trips_through_admission() {
    let section = honest_section();
    admit_distributed_section(&section, &wire_program_fixture()).expect("honest section admits");
    // The full device-section admission (the load boundary) also admits it.
    admit_device_section(&device_section_with_distributed(Some(section)))
        .expect("device section with the honest distributed section admits");
}

#[test]
fn distributed_image_round_trips_through_the_binary_codec() {
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
        device: Some(device_section_with_distributed(Some(honest_section()))),
    };
    let bytes = encode_binary_image(&image).expect("encode binary image");
    let decoded =
        decode_binary_image(&bytes, "0.79.0").expect("distributed image decodes and admits");
    assert!(decoded.device.is_some());
    assert_eq!(
        decoded
            .device
            .as_ref()
            .and_then(|device| device.distributed.as_ref())
            .map(|section| section.v),
        Some(WIRE_DISTRIBUTED_SECTION_VERSION)
    );
}

// ---------------------------------------------------------------------------
// Equivalence gate: passes for the unmutated fixture
// ---------------------------------------------------------------------------

#[test]
fn equivalence_gate_passes_for_the_unmutated_fixture() {
    let section = honest_section();
    equivalence_gate(&section, &wire_program_fixture()).expect("equivalence gate passes");

    // The delivered plan (reconstructed) is exactly the deterministic
    // derivation of the declared program + placement, byte-for-byte.
    let delivered = wire_plan_to_plan(&section.logical_plan).expect("plan reconstructs");
    let facts = wire_facts_to_facts(&section.declared_placement);
    let program = wire_program_to_program(&wire_program_fixture()).expect("program reconstructs");
    let (partitions, graph) = radix_mir::distributed::derive(&program, &facts);
    assert_eq!(partitions.len(), 2);
    assert_eq!(graph.operations.len(), 4);
    assert_eq!(graph.transfers().len(), 1);
    assert_eq!(graph.barriers().len(), 1);
    assert_eq!(delivered.partitions.len(), 2);
}

// ---------------------------------------------------------------------------
// Mutation tests (exit-gate bullet 4): the gate inspects the delivered bytes
// ---------------------------------------------------------------------------

/// The delivered plan's canonical bytes for a mutated section must differ
/// from the honest plan's canonical bytes, so the equivalence gate fails.
fn assert_equivalence_gate_fails(section: &FmirDistributedExecutionSection) {
    match equivalence_gate(section, &wire_program_fixture()) {
        Err(FmirImageError::DistributedSectionEquivalenceMismatch { .. }) => {}
        other => panic!("expected the equivalence gate to fail, got {other:?}"),
    }
}

#[test]
fn mutated_delivered_partition_owner_fails_admission() {
    let mut section = honest_section();
    // Re-own value 1 (`w`) to partition 1 in the DELIVERED plan only; the
    // declared placement facts still declare partition 0.
    section.logical_plan.declared_placement.placements[0].contract =
        WirePlacementContract::Owned { owner: 1 };
    assert_equivalence_gate_fails(&section);
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionInvalid { .. })
    ));
}

#[test]
fn mutated_delivered_partition_placement_owner_fails_admission() {
    let mut section = honest_section();
    // Re-own value 2 (`m`) inside partition 0's placement records only.
    section.logical_plan.partitions[0].placements[1].contract =
        WirePlacementContract::Owned { owner: 1 };
    assert_equivalence_gate_fails(&section);
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionInvalid { .. })
    ));
}

#[test]
fn flipped_graph_edge_fails_admission() {
    let mut section = honest_section();
    // Flip the M transfer edge: P0 → P1 becomes P1 → P0.
    let transfer = match &mut section.logical_plan.graph.operations[2] {
        WireExecutionOperation::Transfer(transfer) => transfer,
        other => panic!("operation 2 must be the transfer, got {other:?}"),
    };
    std::mem::swap(&mut transfer.source, &mut transfer.destination);
    assert_equivalence_gate_fails(&section);
    // The flipped edge still passes the V1 fail-before-launch gate (both
    // partitions declared; the destination holds a copy or runs a launch),
    // so the equivalence gate is the check that rejects it.
    let delivered = wire_plan_to_plan(&section.logical_plan).expect("plan reconstructs");
    delivered.validate().expect("flipped edge stays V1-valid");
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionEquivalenceMismatch { .. })
    ));
}

#[test]
fn altered_transfer_byte_contract_fails_admission() {
    let mut section = honest_section();
    // Alter the M transfer's declared byte contract (512 → 1024) in the
    // DELIVERED plan only; the declared placement fact still says 512.
    let transfer = match &mut section.logical_plan.graph.operations[2] {
        WireExecutionOperation::Transfer(transfer) => transfer,
        other => panic!("operation 2 must be the transfer, got {other:?}"),
    };
    transfer.byte_count = 1_024;
    assert_equivalence_gate_fails(&section);
    // A single altered byte contract does not violate the V1 gate (no
    // conflicting mover), so the equivalence gate is the check that rejects
    // it — proving the gate inspects the delivered bytes.
    let delivered = wire_plan_to_plan(&section.logical_plan).expect("plan reconstructs");
    delivered
        .validate()
        .expect("altered byte contract stays V1-valid");
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionEquivalenceMismatch { .. })
    ));
}

#[test]
fn mutated_declared_placement_fails_admission() {
    let mut section = honest_section();
    // Re-own value 1 in the DECLARED PLACEMENT FACTS (derivation input) —
    // the re-derived expected plan changes and no longer matches the
    // delivered plan. The delivered plan itself is unchanged (V1 passes), so
    // the equivalence gate is the check that rejects it.
    section.declared_placement[0].contract = WirePlacementContract::Owned { owner: 1 };
    assert_equivalence_gate_fails(&section);
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionEquivalenceMismatch { .. })
    ));
}

#[test]
fn mutated_byte_in_the_serialized_plan_fails_admission() {
    // Change a byte in the serialized section's PLAN region: the plan's
    // shape version varint (the last field of the wire plan) is flipped
    // 1 → 2. The image decodes (the byte is a well-formed varint), and the
    // plan's own stale-version check rejects it with a focused diagnostic.
    let section = honest_section();
    let bytes = postcard::to_allocvec(&section).expect("section serializes");
    let plan_bytes = postcard::to_allocvec(&section.logical_plan).expect("plan serializes");
    let plan_start = bytes
        .windows(plan_bytes.len())
        .position(|window| window == plan_bytes.as_slice())
        .expect("the plan's bytes ride the section verbatim");
    let mut mutated = bytes.clone();
    let version_byte = plan_start + plan_bytes.len() - 1;
    assert_eq!(mutated[version_byte], 0x01, "plan version varint is 1");
    mutated[version_byte] = 0x02;
    assert_ne!(mutated, bytes);

    let decoded: FmirDistributedExecutionSection =
        postcard::from_bytes(&mutated).expect("mutated section still decodes");
    assert!(matches!(
        admit_distributed_section(&decoded, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionInvalid { detail }) if detail.contains("stale-version")
    ));
}

#[test]
fn mutated_byte_in_the_serialized_hash_fails_admission() {
    // Change a byte in the serialized section's HASH region: a hex digit of
    // the declared `logical_distributed_plan_hash` is flipped. The section
    // decodes, the plan is unchanged (equivalence passes), and the H1 hash
    // cross-check rejects the stale self-declared hash.
    let section = honest_section();
    let bytes = postcard::to_allocvec(&section).expect("section serializes");
    let hash_start = bytes
        .windows(SHA256_IDENTITY_PREFIX.len())
        .position(|window| window == SHA256_IDENTITY_PREFIX.as_bytes())
        .expect("the declared hash rides the section");
    let mut mutated = bytes.clone();
    let first_digest_byte = hash_start + SHA256_IDENTITY_PREFIX.len();
    let flipped = match mutated[first_digest_byte] {
        b'0' => b'1',
        other => other ^ 0x01,
    };
    mutated[first_digest_byte] = flipped;
    assert_ne!(mutated, bytes);

    let decoded: FmirDistributedExecutionSection =
        postcard::from_bytes(&mutated).expect("mutated section still decodes");
    assert!(matches!(
        admit_distributed_section(&decoded, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionHashMismatch { .. })
    ));
}

#[test]
fn declared_hash_mismatch_fails_closed() {
    // The equivalence gate never trusts the self-declared hash: an honest
    // plan with a wrong declared hash fails the H1 cross-check.
    let mut section = honest_section();
    section.logical_distributed_plan_hash = format!("{SHA256_IDENTITY_PREFIX}{}", "0".repeat(64));
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionHashMismatch { .. })
    ));
}

// ---------------------------------------------------------------------------
// A7: the section's own version ratchet fails closed
// ---------------------------------------------------------------------------

#[test]
fn stale_distributed_section_version_fails_closed() {
    for stale in [
        WIRE_DISTRIBUTED_SECTION_VERSION - 1,
        WIRE_DISTRIBUTED_SECTION_VERSION + 1,
    ] {
        let mut section = honest_section();
        section.v = stale;
        match admit_distributed_section(&section, &wire_program_fixture()) {
            Err(FmirImageError::DistributedSectionVersionMismatch { actual, expected }) => {
                assert_eq!(actual, stale);
                assert_eq!(expected, WIRE_DISTRIBUTED_SECTION_VERSION);
            }
            other => {
                panic!("expected UnsupportedDistributedSectionVersion for {stale}, got {other:?}")
            }
        }
        // The stale section also fails the full device-section load boundary.
        assert!(matches!(
            admit_device_section(&device_section_with_distributed(Some(section))),
            Err(FmirImageError::DistributedSectionVersionMismatch { .. })
        ));
    }
}

// ---------------------------------------------------------------------------
// Wire-level section validation
// ---------------------------------------------------------------------------

#[test]
fn unresolved_placement_fact_fails_wire_validation() {
    // A fact naming an undeclared semantic value cannot resolve against the
    // single-device program.
    let mut section = honest_section();
    section.declared_placement.push(WirePlacementFact {
        value: 99,
        buffer: 1,
        contract: WirePlacementContract::Owned { owner: 0 },
        byte_count: 16,
    });
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionInvalid { detail }) if detail.contains("semantic value 99")
    ));

    // A fact naming an unreferenced buffer.
    let mut section = honest_section();
    section.declared_placement.push(WirePlacementFact {
        value: 1,
        buffer: 99,
        contract: WirePlacementContract::Owned { owner: 0 },
        byte_count: 16,
    });
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionInvalid { detail }) if detail.contains("buffer 99")
    ));

    // A zero byte contract is a missing byte/shape contract.
    let mut section = honest_section();
    section.declared_placement[0].byte_count = 0;
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionInvalid { detail }) if detail.contains("byte count 0")
    ));
}

#[test]
fn non_canonical_declared_hash_spelling_fails_wire_validation() {
    let mut section = honest_section();
    section.logical_distributed_plan_hash = "md5:deadbeef".to_owned();
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionInvalid { detail }) if detail.contains("sha256:")
    ));
}

#[test]
fn unknown_element_type_spelling_fails_closed() {
    // The campaign dtype surface is f32; an unknown transfer element-type
    // spelling fails the reconstruction closed — never a silent default.
    let mut section = honest_section();
    let transfer = match &mut section.logical_plan.graph.operations[2] {
        WireExecutionOperation::Transfer(transfer) => transfer,
        other => panic!("operation 2 must be the transfer, got {other:?}"),
    };
    transfer.element_ty = "f64".to_owned();
    assert!(matches!(
        admit_distributed_section(&section, &wire_program_fixture()),
        Err(FmirImageError::DistributedSectionInvalid { detail }) if detail.contains("outside the campaign dtype surface")
    ));
}

// ---------------------------------------------------------------------------
// Single-device regression (bullet 5) + section-level A10 exclusion
// ---------------------------------------------------------------------------

#[test]
fn single_device_section_without_distributed_admits_unchanged() {
    // A wire-11 device section WITHOUT the distributed section admits through
    // the existing device-section admission unchanged (the independent
    // single-device path; MD-A15).
    let section = device_section_with_distributed(None);
    admit_device_section(&section).expect("single-device section admits");
    assert!(section.distributed.is_none());
}

#[test]
fn device_identity_hash_is_identical_with_and_without_the_distributed_section() {
    // Section-level A10 exclusion (naming contract §2): the A10 identity is
    // a function of source hashes + `canonical_program_bytes(&WireDeviceProgram)`
    // only — the distributed section never enters the substrate, so the hash
    // is byte-for-byte identical whether or not the section is present.
    let sources = ["fnv64:0000000000000000", "fnv64:1111111111111111"];
    let program = wire_program_fixture();
    let base = device_identity_hash(&sources, &canonical_program_bytes(&program));

    let with_distributed = device_section_with_distributed(Some(honest_section()));
    let without_distributed = device_section_with_distributed(None);
    assert_eq!(
        canonical_program_bytes(&with_distributed.device_program.program),
        canonical_program_bytes(&without_distributed.device_program.program)
    );
    assert_eq!(
        device_identity_hash(
            &sources,
            &canonical_program_bytes(&with_distributed.device_program.program)
        ),
        base
    );
    assert_eq!(
        device_identity_hash(
            &sources,
            &canonical_program_bytes(&without_distributed.device_program.program)
        ),
        base
    );
}

// ---------------------------------------------------------------------------
// The wire mirror round-trips the derived plan losslessly
// ---------------------------------------------------------------------------

#[test]
fn wire_plan_mirror_round_trips_the_derived_plan() {
    let plan = sample_plan();
    let wire = plan_to_wire_plan(&plan);
    let reconstructed = wire_plan_to_plan(&wire).expect("plan round-trips");
    assert_eq!(
        radix_mir::distributed::encode_logical_plan(&reconstructed),
        radix_mir::distributed::encode_logical_plan(&plan),
        "the wire mirror must round-trip the plan losslessly for the equivalence gate"
    );
}
