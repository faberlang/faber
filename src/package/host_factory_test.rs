//! CLI-level tests for the S1-5 faber host factory: the one host-construction
//! policy (N1.1 auto rule + N1.4 table), the structured diagnostic mapping,
//! discovery receipts, and the fail-before-launch descriptor rows. All
//! descriptor tests use injected fake drivers — no real hardware (S1-6 owns
//! the real-device proofs).

use super::*;
use faber::device::{DeviceBackend, DeviceSelection};
use faber_host_macos_arm64::composite_host::{CompositeHost, CompositeHostConfig};
use faber_host_macos_arm64::device_descriptor::{
    DescriptorBuffer, DescriptorBufferVersion, DescriptorKernel, DescriptorLaunch,
    DescriptorResult, DeviceBufferInitialization, DeviceBufferLifetime, DeviceBufferRole,
    DeviceDataType, DeviceDescriptor, DeviceProgramLifetime,
};
use faber_host_macos_arm64::device_host::DeviceRuntime;
use faber_host_macos_arm64::{FakeMetalDriver, HostError, MetalHostSession};
use std::collections::BTreeMap;

const MODULE_IMAGE: &[u8] = b"// fake compiler-owned module image";

/// The S2-4 lifetime mapping the faber constructor derives (Input → PerProgram,
/// Output → ObservationPoint, InOut → PerStep); test descriptors mirror it.
fn lifetime_for_role(role: DeviceBufferRole) -> DeviceBufferLifetime {
    match role {
        DeviceBufferRole::Input => DeviceBufferLifetime::PerProgram,
        DeviceBufferRole::Output => DeviceBufferLifetime::ObservationPoint,
        DeviceBufferRole::InOut => DeviceBufferLifetime::PerStep,
    }
}

fn add_slot(
    id: u32,
    name: &str,
    role: DeviceBufferRole,
    binding: u32,
    count: u64,
) -> DescriptorBuffer {
    DescriptorBuffer {
        buffer_id: id,
        buffer_name: name.to_owned(),
        // F1: one distinct semantic value per buffer identity.
        semantic_value: id,
        role,
        lifetime: lifetime_for_role(role),
        // F5: the initialization axis rides the descriptor (HostProvided
        // inputs, ZeroFill InOut state, KernelInitialized outputs).
        initialization: match role {
            DeviceBufferRole::Input => DeviceBufferInitialization::HostProvided,
            DeviceBufferRole::InOut => DeviceBufferInitialization::ZeroFill,
            DeviceBufferRole::Output => DeviceBufferInitialization::KernelInitialized,
        },
        binding,
        element_ty: DeviceDataType::F32,
        element_count: count,
        version: 1,
    }
}

/// One declared observation point (F6): the buffer the host reads back at
/// its producing launch's completion boundary.
fn result(id: u32) -> DescriptorResult {
    DescriptorResult {
        buffer_id: id,
        version: 1,
        produced_by: 1,
        at_launch: 1,
    }
}

fn elementwise_add_descriptor(backend: DeviceBackend, entry: &str, count: u64) -> DeviceDescriptor {
    DeviceDescriptor {
        backend,
        module_image: MODULE_IMAGE.to_vec(),
        kernels: vec![DescriptorKernel {
            entry: entry.to_owned(),
            buffers: vec![
                add_slot(1, "a", DeviceBufferRole::Input, 0, count),
                add_slot(2, "b", DeviceBufferRole::Input, 1, count),
                add_slot(3, "out", DeviceBufferRole::Output, 2, count),
            ],
            grid: [1, 1, 1],
            block: [count as u32, 1, 1],
        }],
        launches: vec![DescriptorLaunch {
            id: 1,
            kernel_index: 0,
        }],
        buffer_versions: vec![
            DescriptorBufferVersion {
                buffer_id: 1,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: count,
            },
            DescriptorBufferVersion {
                buffer_id: 2,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: count,
            },
            DescriptorBufferVersion {
                buffer_id: 3,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: count,
            },
        ],
        program_lifetime: DeviceProgramLifetime::SingleRun,
        data_flow: Vec::new(),
        // F3/F6: the single launch is the legal root and its output the only
        // declared observation point.
        roots: vec![1],
        results: vec![result(3)],
    }
}

fn metal_composite(entry: &str) -> CompositeHost {
    let runtime = DeviceRuntime::Metal(
        MetalHostSession::with_driver(Box::new(FakeMetalDriver::default().with_known_entry(entry)))
            .expect("fake metal admit"),
    );
    CompositeHost::with_device(runtime, "fake-metal-device").expect("composite host")
}

fn add_inputs(a: Vec<f32>, b: Vec<f32>) -> BTreeMap<u32, Vec<f32>> {
    let mut inputs = BTreeMap::new();
    inputs.insert(1, a);
    inputs.insert(2, b);
    inputs
}

// ---------------------------------------------------------------------------
// N1.1 precedence: CLI flag > manifest > default auto
// ---------------------------------------------------------------------------

#[test]
fn effective_selection_cli_overrides_manifest() {
    let selection =
        effective_backend_selection(Some(DeviceSelection::Metal), Some(DeviceSelection::Auto));
    assert_eq!(selection, DeviceSelection::Metal);
}

#[test]
fn effective_selection_manifest_used_when_no_cli_flag() {
    let selection = effective_backend_selection(None, Some(DeviceSelection::Cuda));
    assert_eq!(selection, DeviceSelection::Cuda);
}

#[test]
fn effective_selection_defaults_to_auto_when_both_absent() {
    let selection = effective_backend_selection(None, None);
    assert_eq!(selection, DeviceSelection::Auto);
}

// ---------------------------------------------------------------------------
// N1.1 auto rule + N1.4 resolution rows (pure over the injected admitted list)
// ---------------------------------------------------------------------------

#[test]
fn auto_without_device_program_is_cpu_route() {
    let result = resolve_backend_selection(DeviceSelection::Auto, false, &[DeviceBackend::Metal])
        .expect("cpu route");
    assert_eq!(result, None);
}

#[test]
fn auto_with_device_program_picks_the_single_admitted_backend() {
    let result = resolve_backend_selection(DeviceSelection::Auto, true, &[DeviceBackend::Metal])
        .expect("single admitted");
    assert_eq!(result, Some(DeviceBackend::Metal));
}

#[test]
fn auto_with_zero_admitted_backends_fails_closed() {
    let err = resolve_backend_selection(DeviceSelection::Auto, true, &[])
        .expect_err("zero admitted must fail closed");
    assert_eq!(err.issue(), Some(E_BACKEND_UNAVAILABLE));
}

#[test]
fn auto_with_multiple_admitted_backends_fails_closed_and_names_candidates() {
    let err = resolve_backend_selection(
        DeviceSelection::Auto,
        true,
        &[DeviceBackend::Metal, DeviceBackend::Cuda],
    )
    .expect_err("multiple admitted must fail closed");
    assert_eq!(err.issue(), Some(E_BACKEND_UNAVAILABLE));
    assert!(err.message.contains("metal"));
    assert!(err.message.contains("cuda"));
    assert!(err.message.contains("--backend"));
}

#[test]
fn explicit_backend_on_payloadless_route_is_rejected() {
    let err = resolve_backend_selection(DeviceSelection::Metal, false, &[DeviceBackend::Metal])
        .expect_err("explicit GPU on a payload-less package must fail closed");
    assert_eq!(err.issue(), Some(E_NO_DEVICE_PROGRAM));
    assert!(err.message.contains("no device program"));
}

#[test]
fn explicit_unavailable_backend_never_silently_falls_back() {
    let err = resolve_backend_selection(DeviceSelection::Cuda, true, &[DeviceBackend::Metal])
        .expect_err("explicit cuda with only metal admitted must fail closed");
    assert_eq!(err.issue(), Some(E_BACKEND_UNAVAILABLE));
    assert!(err.message.contains("never silently falls back"));
}

#[test]
fn explicit_admitted_backend_resolves() {
    let result = resolve_backend_selection(DeviceSelection::Metal, true, &[DeviceBackend::Metal])
        .expect("explicit admitted");
    assert_eq!(result, Some(DeviceBackend::Metal));
}

// ---------------------------------------------------------------------------
// Structured diagnostics: code + issue + named args (N1.4)
// ---------------------------------------------------------------------------

#[test]
fn host_error_diagnostic_keeps_the_stable_host_code() {
    let error = HostError {
        code: E_BACKEND_UNAVAILABLE.to_owned(),
        message: "requested backend `cuda` is not admitted".to_owned(),
        retryable: false,
    };
    let diagnostic = host_error_diagnostic(&error);
    assert_eq!(diagnostic.issue(), Some(E_BACKEND_UNAVAILABLE));
    assert_eq!(
        diagnostic.message,
        "requested backend `cuda` is not admitted"
    );
}

#[test]
fn resolution_diagnostic_carries_selection_and_requires_device_args() {
    let err = resolve_backend_selection(DeviceSelection::Cuda, true, &[DeviceBackend::Metal])
        .expect_err("must fail closed");
    let args: Vec<(String, String)> = err
        .args
        .iter()
        .map(|arg| (arg.name.to_owned(), arg.value.clone()))
        .collect();
    assert!(args.contains(&("selection".to_owned(), "cuda".to_owned())));
    assert!(args.contains(&("requires_device".to_owned(), "true".to_owned())));
}

#[test]
fn missing_device_descriptor_is_a_structured_diagnostic() {
    let diagnostic = missing_device_descriptor(DeviceBackend::Metal);
    assert_eq!(diagnostic.issue(), Some(E_DEVICE_DESCRIPTOR));
    assert!(diagnostic.message.contains("metal"));
}

// ---------------------------------------------------------------------------
// Discovery receipts (selected device + artifact hash)
// ---------------------------------------------------------------------------

#[test]
fn discovery_receipt_picks_the_matching_artifact_hash() {
    let artifacts = vec![
        radix_mir_fmir::FmirDeviceArtifact {
            backend: radix_mir_fmir::FmirDeviceBackend::Metal,
            blob: "msl source".to_owned(),
            hash: "fnv64:1111".to_owned(),
            symbols: Vec::new(),
        },
        radix_mir_fmir::FmirDeviceArtifact {
            backend: radix_mir_fmir::FmirDeviceBackend::Cuda,
            blob: "ptx text".to_owned(),
            hash: "fnv64:2222".to_owned(),
            symbols: Vec::new(),
        },
    ];
    let receipt = discovery_receipt(DeviceBackend::Cuda, &artifacts).expect("cuda artifact");
    assert_eq!(receipt.backend, DeviceBackend::Cuda);
    assert_eq!(receipt.artifact_hash, "fnv64:2222");
}

#[test]
fn discovery_receipt_without_matching_artifact_is_missing_descriptor() {
    let artifacts = vec![radix_mir_fmir::FmirDeviceArtifact {
        backend: radix_mir_fmir::FmirDeviceBackend::Metal,
        blob: "msl source".to_owned(),
        hash: "fnv64:1111".to_owned(),
        symbols: Vec::new(),
    }];
    let receipt = discovery_receipt(DeviceBackend::Cuda, &artifacts);
    assert!(receipt.is_none());
}

// ---------------------------------------------------------------------------
// Fail-before-launch descriptor rows (N1.4) through the factory seam
// ---------------------------------------------------------------------------

#[test]
fn cpu_only_host_rejects_descriptor_execution() {
    let mut host = CompositeHost::new(CompositeHostConfig::cpu()).expect("cpu composite");
    let descriptor = elementwise_add_descriptor(DeviceBackend::Metal, "add_one", 2);
    let err = execute_device_descriptor(&mut host, &descriptor, &BTreeMap::new())
        .expect_err("cpu-only host must refuse device execution");
    assert_eq!(err.issue(), Some(E_NO_DEVICE_PROGRAM));
}

#[test]
fn empty_module_image_is_a_bad_descriptor() {
    let mut host = metal_composite("add_one");
    let mut descriptor = elementwise_add_descriptor(DeviceBackend::Metal, "add_one", 2);
    descriptor.module_image.clear();
    let err = execute_device_descriptor(&mut host, &descriptor, &BTreeMap::new())
        .expect_err("empty module image must fail before launch");
    assert_eq!(err.issue(), Some(E_DEVICE_DESCRIPTOR));
}

#[test]
fn duplicate_binding_fails_as_abi_mismatch() {
    let mut host = metal_composite("add_one");
    let mut descriptor = elementwise_add_descriptor(DeviceBackend::Metal, "add_one", 2);
    descriptor.kernels[0].buffers[2].binding = 0; // collides with slot `a`
    let err = execute_device_descriptor(&mut host, &descriptor, &BTreeMap::new())
        .expect_err("duplicate binding must fail as an ABI mismatch");
    assert_eq!(err.issue(), Some(E_DEVICE_ABI_MISMATCH));
}

#[test]
fn unknown_kernel_entry_fails_before_launch() {
    let mut host = metal_composite("add_one");
    let descriptor = elementwise_add_descriptor(DeviceBackend::Metal, "add_two", 2);
    let err = execute_device_descriptor(
        &mut host,
        &descriptor,
        &add_inputs(vec![1.0, 2.0], vec![3.0, 4.0]),
    )
    .expect_err("unknown entry must fail before launch");
    assert_eq!(err.issue(), Some(E_DEVICE_ENTRY_MISMATCH));
}

#[test]
fn conflicting_dtypes_fail_as_dtype_mismatch() {
    let mut host = metal_composite("add_one");
    let mut descriptor = elementwise_add_descriptor(DeviceBackend::Metal, "add_one", 2);
    // Two kernels reference buffer id 3 with the same count but different
    // element types: a dtype conflict must fail before launch.
    descriptor.kernels[0].buffers = vec![
        add_slot(1, "a", DeviceBufferRole::Input, 0, 2),
        add_slot(2, "b", DeviceBufferRole::Input, 1, 2),
        add_slot(3, "x", DeviceBufferRole::InOut, 2, 2),
    ];
    descriptor.kernels.push(DescriptorKernel {
        entry: "add_one".to_owned(),
        buffers: vec![
            {
                let mut slot = add_slot(3, "x", DeviceBufferRole::InOut, 0, 2);
                slot.element_ty = DeviceDataType::I32;
                slot
            },
            add_slot(4, "c", DeviceBufferRole::Input, 1, 2),
            add_slot(5, "out", DeviceBufferRole::Output, 2, 2),
        ],
        grid: [1, 1, 1],
        block: [2, 1, 1],
    });
    let err = execute_device_descriptor(&mut host, &descriptor, &BTreeMap::new())
        .expect_err("dtype conflict must fail before launch");
    assert_eq!(err.issue(), Some(E_DEVICE_DTYPE_MISMATCH));
}

#[test]
fn conflicting_shapes_fail_as_shape_mismatch() {
    let mut host = metal_composite("add_one");
    let descriptor = DeviceDescriptor {
        backend: DeviceBackend::Metal,
        module_image: MODULE_IMAGE.to_vec(),
        kernels: vec![
            DescriptorKernel {
                entry: "add_one".to_owned(),
                buffers: vec![
                    add_slot(1, "a", DeviceBufferRole::Input, 0, 2),
                    add_slot(2, "b", DeviceBufferRole::Input, 1, 2),
                    add_slot(3, "x", DeviceBufferRole::InOut, 2, 2),
                ],
                grid: [1, 1, 1],
                block: [2, 1, 1],
            },
            DescriptorKernel {
                entry: "add_one".to_owned(),
                buffers: vec![
                    add_slot(3, "x", DeviceBufferRole::InOut, 0, 4), // conflict
                    add_slot(4, "c", DeviceBufferRole::Input, 1, 4),
                    add_slot(5, "out", DeviceBufferRole::Output, 2, 4),
                ],
                grid: [1, 1, 1],
                block: [4, 1, 1],
            },
        ],
        launches: vec![
            DescriptorLaunch {
                id: 1,
                kernel_index: 0,
            },
            DescriptorLaunch {
                id: 2,
                kernel_index: 1,
            },
        ],
        buffer_versions: vec![
            DescriptorBufferVersion {
                buffer_id: 1,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: 2,
            },
            DescriptorBufferVersion {
                buffer_id: 2,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: 2,
            },
            DescriptorBufferVersion {
                buffer_id: 3,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: 2,
            },
            DescriptorBufferVersion {
                buffer_id: 4,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: 4,
            },
            DescriptorBufferVersion {
                buffer_id: 5,
                version: 1,
                element_ty: DeviceDataType::F32,
                element_count: 4,
            },
        ],
        program_lifetime: DeviceProgramLifetime::SingleRun,
        data_flow: Vec::new(),
        roots: vec![1, 2],
        results: Vec::new(),
    };
    let err = execute_device_descriptor(&mut host, &descriptor, &BTreeMap::new())
        .expect_err("shape conflict must fail before launch");
    assert_eq!(err.issue(), Some(E_DEVICE_SHAPE_MISMATCH));
}

#[test]
fn missing_declared_input_fails_before_launch() {
    let mut host = metal_composite("add_one");
    let descriptor = elementwise_add_descriptor(DeviceBackend::Metal, "add_one", 2);
    let mut inputs = BTreeMap::new();
    inputs.insert(1, vec![1.0, 2.0]); // buffer 2 missing
    let err = execute_device_descriptor(&mut host, &descriptor, &inputs)
        .expect_err("missing declared input must fail before launch");
    assert_eq!(err.issue(), Some(E_DEVICE_SHAPE_MISMATCH));
}

// ---------------------------------------------------------------------------
// Program-session seam (S2-1): create_program_session
// ---------------------------------------------------------------------------

#[test]
fn create_program_session_returns_a_session_for_a_device_program() {
    let mut host = metal_composite("add_one");
    let descriptor = elementwise_add_descriptor(DeviceBackend::Metal, "add_one", 2);
    let mut session = create_program_session(&mut host, &descriptor)
        .expect("device-bearing image on a fake driver must yield a session");
    // The session is usable: one execute() call drives the full lifecycle on
    // the already-loaded module and the pre-allocated per-program buffers.
    let receipt = session
        .execute(&add_inputs(vec![1.0, 2.0], vec![3.0, 4.0]))
        .expect("session executes without reloading or re-allocating");
    assert_eq!(receipt.launches, 1);
    assert_eq!(
        receipt.outputs.get(&3).map(Vec::as_slice),
        Some(&[4.0, 6.0][..])
    );
}

#[test]
fn create_program_session_on_cpu_only_host_refuses_fail_closed() {
    let mut host = CompositeHost::new(CompositeHostConfig::cpu()).expect("cpu composite");
    let descriptor = elementwise_add_descriptor(DeviceBackend::Metal, "add_one", 2);
    let err = match create_program_session(&mut host, &descriptor) {
        Ok(_) => panic!("cpu-only host must refuse a device program session, not panic"),
        Err(diagnostic) => diagnostic,
    };
    assert_eq!(err.issue(), Some(E_NO_DEVICE_PROGRAM));
    assert!(err.message.contains("no device session"));
}
