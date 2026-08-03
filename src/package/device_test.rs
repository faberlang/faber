use super::*;
use faber::device::DeviceBackend;
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

// ── Payload codec ──────────────────────────────────────────────────────────

#[test]
fn payload_round_trips_deterministically() {
    let plan = DeviceRunPlan {
        v: 1,
        kernels: vec![PlanKernel {
            entry: "summa".to_owned(),
            slots: vec![
                PlanSlot {
                    id: 1,
                    name: "a".to_owned(),
                    role: "input".to_owned(),
                    binding: 0,
                    ty: "f32".to_owned(),
                    count: 256,
                },
                PlanSlot {
                    id: 2,
                    name: "out".to_owned(),
                    role: "output".to_owned(),
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

#[test]
fn payload_unsupported_version_fails_closed() {
    let plan = DeviceRunPlan {
        v: 99,
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
        v: 1,
        kernels: vec![PlanKernel {
            entry: "summa".to_owned(),
            slots: vec![PlanSlot {
                id: 1,
                name: "a".to_owned(),
                role: "input".to_owned(),
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
