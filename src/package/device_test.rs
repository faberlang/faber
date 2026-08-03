use super::*;
use faber::device::DeviceBackend;
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
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../radix/corpus")
        .join(relative);
    super::super::with_lowered_package_mir(
        &radix::driver::Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner)
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
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../radix/corpus/literalia/ascii.fab");
    let program = super::super::with_lowered_package_mir(
        &radix::driver::Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner)
                .expect("constructor succeeds")
        },
    )
    .expect("fixture lowers");
    assert!(program.is_none(), "no kernel → no device program");
}

/// The constructor-derived lifetimes ride the packaged payload (S2-4):
/// `build_run_plan_with_ids` maps each buffer's radix `BufferLifetime` onto
/// the payload slot lifetime and the plan round-trips through the v2 codec —
/// the host receives the typed facts from the image, never re-deriving them.
#[test]
fn constructor_derived_lifetimes_ride_the_packaged_payload() {
    let program = device_program_from_corpus_fixture("cuda/summa-proof.fab");
    let plan = build_run_plan_with_ids(&program, None, &BTreeMap::new());
    assert_eq!(plan.v, DEVICE_RUN_PLAN_VERSION);
    assert_eq!(plan.lifetime, PlanProgramLifetime::SingleRun);
    let slots = &plan.kernels[0].slots;
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[0].role, "input");
    assert_eq!(slots[0].lifetime, PlanSlotLifetime::PerProgram);
    assert_eq!(slots[1].role, "output");
    assert_eq!(slots[1].lifetime, PlanSlotLifetime::ObservationPoint);
    // The v2 payload round-trips with the derived lifetimes intact.
    let encoded = encode_payload(&plan).expect("encodes");
    let parsed = parse_payload(&encoded).expect("parses back");
    assert_eq!(parsed, plan);
}

// ── Payload codec ──────────────────────────────────────────────────────────

#[test]
fn payload_round_trips_deterministically() {
    let plan = DeviceRunPlan {
        v: DEVICE_RUN_PLAN_VERSION,
        lifetime: PlanProgramLifetime::SingleRun,
        kernels: vec![PlanKernel {
            entry: "summa".to_owned(),
            slots: vec![
                PlanSlot {
                    id: 1,
                    name: "a".to_owned(),
                    role: "input".to_owned(),
                    lifetime: PlanSlotLifetime::PerProgram,
                    binding: 0,
                    ty: "f32".to_owned(),
                    count: 256,
                },
                PlanSlot {
                    id: 2,
                    name: "out".to_owned(),
                    role: "output".to_owned(),
                    lifetime: PlanSlotLifetime::ObservationPoint,
                    binding: 1,
                    ty: "f32".to_owned(),
                    count: 1,
                },
            ],
            grid: [1, 1, 1],
            block: [256, 1, 1],
        }],
        cuda_kernels: vec![PlanCudaKernel {
            entry: "summa".to_owned(),
            symbol: "f0".to_owned(),
        }],
        inputs: vec![PlanInput {
            name: "a".to_owned(),
            values: (0..256).map(|i| i as f32).collect(),
        }],
    };

    let first = encode_payload(&plan).expect("encodes");
    let second = encode_payload(&plan).expect("encodes deterministically");
    assert_eq!(first, second, "identical plan → identical payload bytes");
    let parsed = parse_payload(&first).expect("parses back");
    assert_eq!(parsed, plan, "round-trip preserves the plan");
}

/// Codec v2 admission: the payload carries the typed per-buffer lifetimes and
/// the program lifetime (S2-4), and the serialized spelling is stable.
#[test]
fn payload_v2_carries_typed_lifetimes() {
    let plan = DeviceRunPlan {
        v: DEVICE_RUN_PLAN_VERSION,
        lifetime: PlanProgramLifetime::RepeatingStep,
        kernels: vec![PlanKernel {
            entry: "chain".to_owned(),
            slots: vec![
                PlanSlot {
                    id: 1,
                    name: "a".to_owned(),
                    role: "input".to_owned(),
                    lifetime: PlanSlotLifetime::PerProgram,
                    binding: 0,
                    ty: "f32".to_owned(),
                    count: 4,
                },
                PlanSlot {
                    id: 2,
                    name: "acc".to_owned(),
                    role: "in-out".to_owned(),
                    lifetime: PlanSlotLifetime::PerStep,
                    binding: 1,
                    ty: "f32".to_owned(),
                    count: 4,
                },
                PlanSlot {
                    id: 3,
                    name: "out".to_owned(),
                    role: "output".to_owned(),
                    lifetime: PlanSlotLifetime::ObservationPoint,
                    binding: 2,
                    ty: "f32".to_owned(),
                    count: 1,
                },
            ],
            grid: [1, 1, 1],
            block: [4, 1, 1],
        }],
        cuda_kernels: Vec::new(),
        inputs: Vec::new(),
    };
    let encoded = encode_payload(&plan).expect("encodes");
    assert!(encoded.contains("\"lifetime\":\"per-program\""));
    assert!(encoded.contains("\"lifetime\":\"per-step\""));
    assert!(encoded.contains("\"lifetime\":\"observation-point\""));
    assert!(encoded.contains("\"lifetime\":\"repeating-step\""));
    let parsed = parse_payload(&encoded).expect("parses back");
    assert_eq!(parsed, plan);
}

/// Old v1 payloads fail closed with the structured `payload_version`
/// diagnostic (S2-4 done-when): the version gate runs before any field-level
/// parse, so a v1 payload is rejected by version, not by a missing `lifetime`
/// field, and never silently defaulted.
#[test]
fn payload_v1_fails_closed_with_payload_version_diagnostic() {
    // A payload with `v: 1` and NO `lifetime` fields — exactly the pre-S2-4
    // v1 representation. The admission gate must reject it by version with
    // the structured diagnostic, never attempt a field-level parse.
    let v1_payload = r#"{"v":1,"kernels":[{"entry":"summa","slots":[{"id":1,"name":"a","role":"input","binding":0,"ty":"f32","count":256}],"grid":[1,1,1],"block":[256,1,1]}],"cuda_kernels":[],"inputs":[]}"#;
    let error = parse_payload(v1_payload).expect_err("v1 payload must fail closed");
    let message = &error[0].message;
    assert!(
        message.contains("version 1 is not supported"),
        "old payloads are rejected by the version gate: {message}"
    );
    assert!(
        error[0]
            .args
            .iter()
            .any(|arg| arg.name == "payload_version" && arg.value == "1"),
        "the structured `payload_version` diagnostic names the offending version"
    );
}

#[test]
fn payload_unsupported_version_fails_closed() {
    let plan = DeviceRunPlan {
        v: 99,
        lifetime: PlanProgramLifetime::SingleRun,
        kernels: Vec::new(),
        cuda_kernels: Vec::new(),
        inputs: Vec::new(),
    };
    let encoded = encode_payload(&plan).expect("encodes");
    let error = parse_payload(&encoded).expect_err("unsupported version must fail closed");
    assert!(error[0].message.contains("version 99 is not supported"));
}

#[test]
fn payload_garbage_fails_closed() {
    let error = parse_payload("not json").expect_err("garbage payload must fail closed");
    assert!(error[0].message.contains("not a valid run plan"));
}

// ── Descriptor construction (CudaKernelIdentity consumption) ───────────────

#[test]
fn cuda_descriptor_consumes_nvvm_symbol_mapping() {
    let plan = DeviceRunPlan {
        v: DEVICE_RUN_PLAN_VERSION,
        lifetime: PlanProgramLifetime::SingleRun,
        kernels: vec![PlanKernel {
            entry: "summa".to_owned(),
            slots: vec![PlanSlot {
                id: 1,
                name: "a".to_owned(),
                role: "input".to_owned(),
                lifetime: PlanSlotLifetime::PerProgram,
                binding: 0,
                ty: "f32".to_owned(),
                count: 256,
            }],
            grid: [1, 1, 1],
            block: [256, 1, 1],
        }],
        cuda_kernels: vec![PlanCudaKernel {
            entry: "summa".to_owned(),
            symbol: "f0".to_owned(),
        }],
        inputs: Vec::new(),
    };

    let metal = descriptor_for_backend(&plan, DeviceBackend::Metal, b"msl blob");
    assert_eq!(metal.kernels[0].entry, "summa", "Metal launches by the logical entry");

    let cuda = descriptor_for_backend(&plan, DeviceBackend::Cuda, b"ptx blob");
    assert_eq!(
        cuda.kernels[0].entry, "f0",
        "the CUDA descriptor consumes the S1-3 CudaKernelIdentity symbol"
    );
    assert_eq!(cuda.module_image, b"ptx blob");
    assert_eq!(cuda.kernels[0].buffers[0].buffer_id, 1);
    assert_eq!(cuda.kernels[0].grid, [1, 1, 1]);
    assert_eq!(cuda.kernels[0].block, [256, 1, 1]);
}

/// The payload's typed lifetimes (codec v2) are mapped onto the host
/// descriptor — the host receives the constructor-derived facts and never
/// re-derives a lifetime from slot role (S2-4).
#[test]
fn descriptor_maps_payload_lifetimes_onto_host_descriptor() {
    let plan = DeviceRunPlan {
        v: DEVICE_RUN_PLAN_VERSION,
        lifetime: PlanProgramLifetime::RepeatingStep,
        kernels: vec![PlanKernel {
            entry: "chain".to_owned(),
            slots: vec![
                PlanSlot {
                    id: 1,
                    name: "a".to_owned(),
                    role: "input".to_owned(),
                    lifetime: PlanSlotLifetime::PerProgram,
                    binding: 0,
                    ty: "f32".to_owned(),
                    count: 4,
                },
                PlanSlot {
                    id: 2,
                    name: "acc".to_owned(),
                    role: "in-out".to_owned(),
                    lifetime: PlanSlotLifetime::PerStep,
                    binding: 1,
                    ty: "f32".to_owned(),
                    count: 4,
                },
                PlanSlot {
                    id: 3,
                    name: "out".to_owned(),
                    role: "output".to_owned(),
                    lifetime: PlanSlotLifetime::ObservationPoint,
                    binding: 2,
                    ty: "f32".to_owned(),
                    count: 1,
                },
            ],
            grid: [1, 1, 1],
            block: [4, 1, 1],
        }],
        cuda_kernels: Vec::new(),
        inputs: Vec::new(),
    };

    let descriptor = descriptor_for_backend(&plan, DeviceBackend::Metal, b"msl blob");
    assert_eq!(
        descriptor.program_lifetime,
        HostDeviceProgramLifetime::RepeatingStep,
        "the program regime rides the payload into the host descriptor"
    );
    let slots = &descriptor.kernels[0].buffers;
    assert_eq!(slots[0].lifetime, DeviceBufferLifetime::PerProgram);
    assert_eq!(slots[1].lifetime, DeviceBufferLifetime::PerStep);
    assert_eq!(slots[2].lifetime, DeviceBufferLifetime::ObservationPoint);
}

/// A v2 payload whose slot carries an unknown lifetime spelling fails closed
/// at admission (never a silent default to a role-derived lifetime).
#[test]
fn payload_unknown_lifetime_spelling_fails_closed() {
    let v2_payload = r#"{"v":2,"lifetime":"single-run","kernels":[{"entry":"summa","slots":[{"id":1,"name":"a","role":"input","lifetime":"forever","binding":0,"ty":"f32","count":256}],"grid":[1,1,1],"block":[256,1,1]}],"cuda_kernels":[],"inputs":[]}"#;
    let error = parse_payload(v2_payload).expect_err("unknown lifetime spelling must fail closed");
    assert!(error[0].message.contains("not a valid run plan"));
}

#[test]
fn descriptor_requires_declared_backend_artifact() {
    let artifacts = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: "msl".to_owned(),
        hash: "fnv64:0000000000000000".to_owned(),
    }];
    assert!(artifact_for_backend(&artifacts, DeviceBackend::Metal).is_some());
    assert!(
        artifact_for_backend(&artifacts, DeviceBackend::Cuda).is_none(),
        "an undeclared backend artifact fails closed"
    );
}


/// Build the S2-5 two-kernel fixture's device program (constructor
/// identity-unification test substrate).
fn two_kernel_program() -> DeviceProgram {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/training/device-summa-recollige/src/device_summa_recollige.fab");
    super::super::with_lowered_package_mir(
        &radix::driver::Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        |lowered| {
            device_program_for_lowered(&lowered.validated, &lowered.interner)
                .expect("constructor succeeds")
                .expect("fixture yields a device program")
        },
    )
    .expect("fixture lowers")
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

    // Results name only the producing launches: the intermediate is produced
    // by launch 1, the final output by launch 2.
    let results: Vec<_> = program
        .results
        .iter()
        .map(|result| (result.buffer.name.as_str(), result.produced_by.0))
        .collect();
    assert_eq!(results, vec![("medius", 1), ("result", 2)]);
}

/// The unified lifetimes ride the run-plan payload (codec v2): the
/// intermediate's plan slots are in-out/per-step at both kernels, the final
/// output is observation-point, and the ordinary readback set is exactly the
/// observation point — the intermediate is never read back.
#[test]
fn two_kernel_run_plan_carries_unified_lifetimes() {
    let program = two_kernel_program();
    let plan = build_run_plan_with_ids(&program, None, &BTreeMap::new());
    assert_eq!(plan.v, DEVICE_RUN_PLAN_VERSION);
    assert_eq!(plan.kernels.len(), 2);

    // Kernel 1: a (input/per-program) + medius (in-out/per-step).
    let kernel0_slots = &plan.kernels[0].slots;
    assert_eq!(kernel0_slots.len(), 2);
    assert_eq!(kernel0_slots[0].name, "a");
    assert_eq!(kernel0_slots[0].role, "input");
    assert_eq!(kernel0_slots[0].lifetime, PlanSlotLifetime::PerProgram);
    assert_eq!(kernel0_slots[1].name, "medius");
    assert_eq!(kernel0_slots[1].role, "in-out");
    assert_eq!(kernel0_slots[1].lifetime, PlanSlotLifetime::PerStep);

    // Kernel 2: medius (in-out/per-step, same id) + result
    // (output/observation-point).
    let kernel1_slots = &plan.kernels[1].slots;
    assert_eq!(kernel1_slots.len(), 2);
    assert_eq!(kernel1_slots[0].id, kernel0_slots[1].id, "one BufferId for the intermediate");
    assert_eq!(kernel1_slots[0].role, "in-out");
    assert_eq!(kernel1_slots[0].lifetime, PlanSlotLifetime::PerStep);
    assert_eq!(kernel1_slots[1].name, "result");
    assert_eq!(kernel1_slots[1].role, "output");
    assert_eq!(kernel1_slots[1].lifetime, PlanSlotLifetime::ObservationPoint);

    // The ordinary readback set is exactly the observation point; the
    // PerStep intermediate is never read back (no undeclared readback).
    let readbacks = observation_buffer_ids(&plan);
    assert_eq!(readbacks, vec![kernel1_slots[1].id]);

    // The v2 payload round-trips with the unified lifetimes intact.
    let encoded = encode_payload(&plan).expect("encodes");
    let parsed = parse_payload(&encoded).expect("parses back");
    assert_eq!(parsed, plan);
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
