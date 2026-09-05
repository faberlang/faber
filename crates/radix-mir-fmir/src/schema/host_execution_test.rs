//! HEP-3A red tests for the Radix-owned physical execution wire.
//!
//! These tests deliberately name the producer surface before its
//! implementation exists. The packet is physical-only: semantic program
//! facts must not be smuggled into the Host wire or recovered by sorting,
//! defaulting, or interpreting names.

use super::*;

const UNFAMILIAR_SINGLE_KERNEL_ARTIFACT: &[u8] =
    include_bytes!("../../tests/fixtures/host-execution-v1/unfamiliar-single-kernel.artifact.msl");
const MULTI_LAUNCH_DEPENDENCY_ARTIFACT: &[u8] =
    include_bytes!("../../tests/fixtures/host-execution-v1/multi-launch-dependency.artifact.msl");
const PREPARED_RESOURCE_ARTIFACT: &[u8] =
    include_bytes!("../../tests/fixtures/host-execution-v1/prepared-resource.artifact.msl");
const INVOCATION_WINDOW_ARTIFACT: &[u8] =
    include_bytes!("../../tests/fixtures/host-execution-v1/invocation-window.artifact.msl");
const OBSERVATION_ARTIFACT: &[u8] =
    include_bytes!("../../tests/fixtures/host-execution-v1/observation.artifact.msl");
const FINAL_ENTRY_DIVERGENCE_ARTIFACT: &[u8] =
    include_bytes!("../../tests/fixtures/host-execution-v1/final-entry-divergence.artifact.ptx");

fn single_kernel_execution() -> WireHostExecutionEnvelopeV1 {
    WireHostExecutionEnvelopeV1 {
        schema: HOST_EXECUTION_SCHEMA.to_owned(),
        version: HOST_EXECUTION_WIRE_VERSION,
        program: WireHostExecutionProgramV1 {
            target: WireHostTargetV1 {
                backend: "metal".to_owned(),
                required_features: vec!["macos-arm64".to_owned(), "metal".to_owned()],
            },
            artifact: artifact("msl", UNFAMILIAR_SINGLE_KERNEL_ARTIFACT),
            resources: vec![WireHostResourceV1 {
                id: 1,
                byte_capacity: 4,
                byte_alignment: 4,
                lifetime: WireHostResourceLifetimeV1::PreparedExecution,
                initialization: WireHostInitializationV1::Supplied { input_slot: 1 },
            }],
            kernels: vec![WireHostKernelV1 {
                id: 1,
                entry: "mystery_kernel".to_owned(),
                block: [1, 1, 1],
                slots: vec![WireHostKernelSlotV1 {
                    id: 1,
                    group: 0,
                    binding: 0,
                    access: WireHostAccessV1::Read,
                }],
            }],
            launches: vec![WireHostLaunchV1 {
                id: 1,
                kernel: 1,
                grid: [1, 1, 1],
                bindings: vec![WireHostLaunchBindingV1 {
                    slot: 1,
                    resource: 1,
                    window: WireHostBindingWindowV1::Exact {
                        byte_offset: 0,
                        byte_length: 4,
                    },
                }],
            }],
            dependencies: Vec::new(),
            observations: Vec::new(),
        },
    }
}

fn invocation() -> WireHostInvocationEnvelopeV1 {
    WireHostInvocationEnvelopeV1 {
        schema: HOST_INVOCATION_SCHEMA.to_owned(),
        version: HOST_INVOCATION_WIRE_VERSION,
        invocation: WireHostInvocationV1 {
            execution_sha256:
                "sha256:b71f50cb1d0644c6302337b1bb7c1e0b5c94a08007eb6d037daab1e895bb719b".to_owned(),
            correlation_id: "corr-1".to_owned(),
            windows: vec![WireHostInvocationWindowV1 {
                window_slot: 1,
                byte_offset: 8,
                byte_length: 16,
            }],
            requested_observations: vec![1],
        },
    }
}

#[test]
fn v1_execution_and_invocation_envelope_constants_are_independent() {
    assert_eq!(HOST_EXECUTION_SCHEMA, "faber.host-execution");
    assert_eq!(HOST_EXECUTION_WIRE_VERSION, 1);
    assert_eq!(HOST_INVOCATION_SCHEMA, "faber.host-invocation");
    assert_eq!(HOST_INVOCATION_WIRE_VERSION, 1);
    assert_ne!(HOST_EXECUTION_SCHEMA, HOST_INVOCATION_SCHEMA);
}

#[test]
fn canonical_execution_bytes_and_sha256_are_exact() {
    let execution = single_kernel_execution();
    let artifact_sha256 = execution.program.artifact.sha256.clone();
    let bytes = encode_host_execution_v1(&execution).expect("valid packet");
    let expected = format!(
        r#"{{"schema":"faber.host-execution","version":1,"program":{{"target":{{"backend":"metal","required_features":["macos-arm64","metal"]}},"artifact":{{"format":"msl","abi_version":{},"byte_length":204,"sha256":"{}"}},"resources":[{{"id":1,"byte_capacity":4,"byte_alignment":4,"lifetime":"prepared_execution","initialization":{{"kind":"supplied","input_slot":1}}}}],"kernels":[{{"id":1,"entry":"mystery_kernel","block":[1,1,1],"slots":[{{"id":1,"group":0,"binding":0,"access":"read"}}]}}],"launches":[{{"id":1,"kernel":1,"grid":[1,1,1],"bindings":[{{"slot":1,"resource":1,"window":{{"kind":"exact","byte_offset":0,"byte_length":4}}}}]}}],"dependencies":[],"observations":[]}}}}"#,
        ARTIFACT_ABI_VERSION, artifact_sha256
    );
    assert_eq!(
        String::from_utf8(bytes.clone()).expect("canonical JSON is UTF-8"),
        expected
    );
    assert_eq!(
        sha256_digest(&bytes),
        "sha256:b71f50cb1d0644c6302337b1bb7c1e0b5c94a08007eb6d037daab1e895bb719b"
    );
}

#[test]
fn canonical_invocation_bytes_and_sha256_are_exact() {
    let bytes = encode_host_invocation_v1(&invocation()).expect("valid invocation");
    assert_eq!(
        String::from_utf8(bytes.clone()).expect("canonical JSON is UTF-8"),
        r#"{"schema":"faber.host-invocation","version":1,"invocation":{"execution_sha256":"sha256:b71f50cb1d0644c6302337b1bb7c1e0b5c94a08007eb6d037daab1e895bb719b","correlation_id":"corr-1","windows":[{"window_slot":1,"byte_offset":8,"byte_length":16}],"requested_observations":[1]}}"#
    );
    assert_eq!(
        sha256_digest(&bytes),
        "sha256:b9160000139c631893bac780b60155277de2308eaf42f43778b86c9923263f23"
    );
}

#[test]
fn producer_rejects_noncanonical_order_and_invalid_physical_values() {
    let mut unsorted_features = single_kernel_execution();
    unsorted_features.program.target.required_features.reverse();
    assert!(encode_host_execution_v1(&unsorted_features).is_err());

    let mut unsorted_launch_ids = single_kernel_execution();
    let mut second_launch = unsorted_launch_ids.program.launches[0].clone();
    second_launch.id = 2;
    unsorted_launch_ids.program.launches.push(second_launch);
    unsorted_launch_ids.program.launches.reverse();
    assert!(encode_host_execution_v1(&unsorted_launch_ids).is_err());

    let mut unsorted_dependencies = single_kernel_execution();
    for id in [2, 3] {
        let mut launch = unsorted_dependencies.program.launches[0].clone();
        launch.id = id;
        unsorted_dependencies.program.launches.push(launch);
    }
    unsorted_dependencies.program.dependencies = vec![
        WireHostDependencyV1 {
            before: 2,
            after: 3,
        },
        WireHostDependencyV1 {
            before: 1,
            after: 2,
        },
    ];
    assert!(encode_host_execution_v1(&unsorted_dependencies).is_err());

    let mut zero_id = single_kernel_execution();
    zero_id.program.resources[0].id = 0;
    assert!(encode_host_execution_v1(&zero_id).is_err());

    let mut duplicate_id = single_kernel_execution();
    let duplicate_resource = duplicate_id.program.resources[0].clone();
    duplicate_id.program.resources.push(duplicate_resource);
    assert!(encode_host_execution_v1(&duplicate_id).is_err());

    let mut invalid_hash = single_kernel_execution();
    invalid_hash.program.artifact.sha256 = "not-a-sha256-identity".to_owned();
    assert!(encode_host_execution_v1(&invalid_hash).is_err());

    let mut empty_entry = single_kernel_execution();
    empty_entry.program.kernels[0].entry.clear();
    assert!(encode_host_execution_v1(&empty_entry).is_err());

    let mut zero_capacity = single_kernel_execution();
    zero_capacity.program.resources[0].byte_capacity = 0;
    assert!(encode_host_execution_v1(&zero_capacity).is_err());

    let mut zero_dimension = single_kernel_execution();
    zero_dimension.program.kernels[0].block[0] = 0;
    assert!(encode_host_execution_v1(&zero_dimension).is_err());

    let mut overflowing_dimension = single_kernel_execution();
    overflowing_dimension.program.kernels[0].block = [u32::MAX, u32::MAX, 2];
    assert!(encode_host_execution_v1(&overflowing_dimension).is_err());

    let mut unsorted_observations = single_kernel_execution();
    unsorted_observations.program.observations = vec![
        WireHostObservationV1 {
            id: 2,
            resource: 1,
            after_launch: 1,
            window: WireHostBindingWindowV1::Exact {
                byte_offset: 0,
                byte_length: 4,
            },
        },
        WireHostObservationV1 {
            id: 1,
            resource: 1,
            after_launch: 1,
            window: WireHostBindingWindowV1::Exact {
                byte_offset: 0,
                byte_length: 4,
            },
        },
    ];
    assert!(encode_host_execution_v1(&unsorted_observations).is_err());
}

#[test]
fn payload_tags_match_the_accepted_serde_shapes() {
    assert_eq!(
        serde_json::to_string(&WireHostInitializationV1::Supplied { input_slot: 2 })
            .expect("initialization serializes"),
        r#"{"kind":"supplied","input_slot":2}"#
    );
    assert_eq!(
        serde_json::to_string(&WireHostBindingWindowV1::Exact {
            byte_offset: 0,
            byte_length: 64,
        })
        .expect("binding window serializes"),
        r#"{"kind":"exact","byte_offset":0,"byte_length":64}"#
    );
    assert_eq!(
        serde_json::to_string(&WireHostResourceLifetimeV1::PreparedExecution)
            .expect("lifetime serializes"),
        r#""prepared_execution""#
    );
    assert_eq!(
        serde_json::to_string(&WireHostAccessV1::ReadWrite).expect("access serializes"),
        r#""read_write""#
    );
}

#[test]
fn semantic_fields_do_not_cross_the_physical_wire() {
    let execution = serde_json::to_value(single_kernel_execution()).expect("execution JSON");
    let invocation = serde_json::to_value(invocation()).expect("invocation JSON");
    let mut keys = Vec::new();
    collect_object_keys(&execution, &mut keys);
    collect_object_keys(&invocation, &mut keys);

    for forbidden in [
        "semantic_value",
        "kernel_plan",
        "shape",
        "dtype",
        "packed_format",
        "model",
        "cursor",
        "session",
        "prefill",
        "decode",
        "kv_cache",
        "query_rows",
        "sequence_epoch",
        "cadence",
        "operation",
    ] {
        assert!(
            !keys.iter().any(|key| key == forbidden),
            "semantic field `{forbidden}` crossed the physical wire"
        );
    }
}

fn collect_object_keys(value: &serde_json::Value, keys: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                keys.push(key.clone());
                collect_object_keys(child, keys);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                collect_object_keys(child, keys);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

#[test]
fn committed_v1_fixtures_are_exact_radix_producer_packets() {
    assert_fixture(
        "unfamiliar-single-kernel",
        encode_host_execution_v1(&single_kernel_execution()).expect("valid packet"),
        include_bytes!(
            "../../tests/fixtures/host-execution-v1/unfamiliar-single-kernel.execution.json"
        ),
        include_str!(
            "../../tests/fixtures/host-execution-v1/unfamiliar-single-kernel.execution.sha256"
        ),
    );

    let multi_launch = WireHostExecutionEnvelopeV1 {
        schema: HOST_EXECUTION_SCHEMA.to_owned(),
        version: HOST_EXECUTION_WIRE_VERSION,
        program: WireHostExecutionProgramV1 {
            target: WireHostTargetV1 {
                backend: "metal".to_owned(),
                required_features: vec!["metal".to_owned()],
            },
            artifact: artifact("msl", MULTI_LAUNCH_DEPENDENCY_ARTIFACT),
            resources: vec![resource(
                1,
                64,
                4,
                WireHostResourceLifetimeV1::Invocation,
                WireHostInitializationV1::DeviceWritten,
            )],
            kernels: vec![
                kernel(1, "backend_entry_a", WireHostAccessV1::Write),
                kernel(2, "backend_entry_b", WireHostAccessV1::Read),
            ],
            launches: vec![launch(1, 1, 1, 64), launch(2, 2, 1, 64)],
            dependencies: vec![WireHostDependencyV1 {
                before: 1,
                after: 2,
            }],
            observations: Vec::new(),
        },
    };
    assert_fixture(
        "multi-launch-dependency",
        encode_host_execution_v1(&multi_launch).expect("valid packet"),
        include_bytes!(
            "../../tests/fixtures/host-execution-v1/multi-launch-dependency.execution.json"
        ),
        include_str!(
            "../../tests/fixtures/host-execution-v1/multi-launch-dependency.execution.sha256"
        ),
    );

    let prepared_resource = WireHostExecutionEnvelopeV1 {
        schema: HOST_EXECUTION_SCHEMA.to_owned(),
        version: HOST_EXECUTION_WIRE_VERSION,
        program: WireHostExecutionProgramV1 {
            target: WireHostTargetV1 {
                backend: "metal".to_owned(),
                required_features: vec!["metal".to_owned()],
            },
            artifact: artifact("msl", PREPARED_RESOURCE_ARTIFACT),
            resources: vec![
                resource(
                    1,
                    16,
                    8,
                    WireHostResourceLifetimeV1::PreparedExecution,
                    WireHostInitializationV1::Supplied { input_slot: 1 },
                ),
                resource(
                    2,
                    16,
                    8,
                    WireHostResourceLifetimeV1::Invocation,
                    WireHostInitializationV1::DeviceWritten,
                ),
            ],
            kernels: vec![WireHostKernelV1 {
                id: 1,
                entry: "final_copy_symbol".to_owned(),
                block: [1, 1, 1],
                slots: vec![
                    WireHostKernelSlotV1 {
                        id: 1,
                        group: 0,
                        binding: 0,
                        access: WireHostAccessV1::Read,
                    },
                    WireHostKernelSlotV1 {
                        id: 2,
                        group: 0,
                        binding: 1,
                        access: WireHostAccessV1::Write,
                    },
                ],
            }],
            launches: vec![WireHostLaunchV1 {
                id: 1,
                kernel: 1,
                grid: [1, 1, 1],
                bindings: vec![binding(1, 1, 16), binding(2, 2, 16)],
            }],
            dependencies: Vec::new(),
            observations: Vec::new(),
        },
    };
    let prepared_bytes = encode_host_execution_v1(&prepared_resource).expect("valid packet");
    assert_fixture(
        "prepared-resource",
        prepared_bytes.clone(),
        include_bytes!("../../tests/fixtures/host-execution-v1/prepared-resource.execution.json"),
        include_str!("../../tests/fixtures/host-execution-v1/prepared-resource.execution.sha256"),
    );

    let prepared_execution_sha256 = host_execution_sha256(&prepared_bytes);
    for (name, correlation_id) in [
        ("prepared-resource-first", "prepared-first"),
        ("prepared-resource-second", "prepared-second"),
    ] {
        let invocation = WireHostInvocationEnvelopeV1 {
            schema: HOST_INVOCATION_SCHEMA.to_owned(),
            version: HOST_INVOCATION_WIRE_VERSION,
            invocation: WireHostInvocationV1 {
                execution_sha256: prepared_execution_sha256.clone(),
                correlation_id: correlation_id.to_owned(),
                windows: Vec::new(),
                requested_observations: Vec::new(),
            },
        };
        let (bytes, hash) = if name == "prepared-resource-first" {
            (
                include_bytes!(
                    "../../tests/fixtures/host-execution-v1/prepared-resource-first.invocation.json"
                ) as &[u8],
                include_str!(
                    "../../tests/fixtures/host-execution-v1/prepared-resource-first.invocation.sha256"
                ),
            )
        } else {
            (
                include_bytes!(
                    "../../tests/fixtures/host-execution-v1/prepared-resource-second.invocation.json"
                ) as &[u8],
                include_str!(
                    "../../tests/fixtures/host-execution-v1/prepared-resource-second.invocation.sha256"
                ),
            )
        };
        assert_fixture(
            name,
            encode_host_invocation_v1(&invocation).expect("valid packet"),
            bytes,
            hash,
        );
    }

    let invocation_window_program = WireHostExecutionEnvelopeV1 {
        schema: HOST_EXECUTION_SCHEMA.to_owned(),
        version: HOST_EXECUTION_WIRE_VERSION,
        program: WireHostExecutionProgramV1 {
            target: WireHostTargetV1 {
                backend: "metal".to_owned(),
                required_features: vec!["metal".to_owned()],
            },
            artifact: artifact("msl", INVOCATION_WINDOW_ARTIFACT),
            resources: vec![resource(
                1,
                96,
                16,
                WireHostResourceLifetimeV1::Invocation,
                WireHostInitializationV1::Supplied { input_slot: 1 },
            )],
            kernels: vec![kernel(1, "opaque_dynamic_entry", WireHostAccessV1::Read)],
            launches: vec![WireHostLaunchV1 {
                id: 1,
                kernel: 1,
                grid: [1, 1, 1],
                bindings: vec![WireHostLaunchBindingV1 {
                    slot: 1,
                    resource: 1,
                    window: WireHostBindingWindowV1::Invocation {
                        window_slot: 1,
                        maximum_byte_offset: 64,
                        maximum_byte_length: 32,
                        byte_alignment: 16,
                    },
                }],
            }],
            dependencies: Vec::new(),
            observations: Vec::new(),
        },
    };
    let invocation_window_bytes =
        encode_host_execution_v1(&invocation_window_program).expect("valid packet");
    assert_fixture(
        "invocation-window-program",
        invocation_window_bytes.clone(),
        include_bytes!("../../tests/fixtures/host-execution-v1/invocation-window.execution.json"),
        include_str!("../../tests/fixtures/host-execution-v1/invocation-window.execution.sha256"),
    );

    let invocation_window_sha256 = host_execution_sha256(&invocation_window_bytes);
    for (name, byte_offset, byte_length) in [
        ("invocation-window-first", 16, 16),
        ("invocation-window-second", 64, 32),
    ] {
        let invocation = WireHostInvocationEnvelopeV1 {
            schema: HOST_INVOCATION_SCHEMA.to_owned(),
            version: HOST_INVOCATION_WIRE_VERSION,
            invocation: WireHostInvocationV1 {
                execution_sha256: invocation_window_sha256.clone(),
                correlation_id: name.to_owned(),
                windows: vec![WireHostInvocationWindowV1 {
                    window_slot: 1,
                    byte_offset,
                    byte_length,
                }],
                requested_observations: Vec::new(),
            },
        };
        let (bytes, hash) = if name == "invocation-window-first" {
            (
                include_bytes!(
                    "../../tests/fixtures/host-execution-v1/invocation-window-first.invocation.json"
                ) as &[u8],
                include_str!(
                    "../../tests/fixtures/host-execution-v1/invocation-window-first.invocation.sha256"
                ),
            )
        } else {
            (
                include_bytes!(
                    "../../tests/fixtures/host-execution-v1/invocation-window-second.invocation.json"
                ) as &[u8],
                include_str!(
                    "../../tests/fixtures/host-execution-v1/invocation-window-second.invocation.sha256"
                ),
            )
        };
        assert_fixture(
            name,
            encode_host_invocation_v1(&invocation).expect("valid packet"),
            bytes,
            hash,
        );
    }

    let observation_program = WireHostExecutionEnvelopeV1 {
        schema: HOST_EXECUTION_SCHEMA.to_owned(),
        version: HOST_EXECUTION_WIRE_VERSION,
        program: WireHostExecutionProgramV1 {
            target: WireHostTargetV1 {
                backend: "metal".to_owned(),
                required_features: vec!["metal".to_owned()],
            },
            artifact: artifact("msl", OBSERVATION_ARTIFACT),
            resources: vec![resource(
                1,
                8,
                4,
                WireHostResourceLifetimeV1::Invocation,
                WireHostInitializationV1::DeviceWritten,
            )],
            kernels: vec![kernel(
                1,
                "opaque_observation_entry",
                WireHostAccessV1::Write,
            )],
            launches: vec![launch(1, 1, 1, 8)],
            dependencies: Vec::new(),
            observations: vec![WireHostObservationV1 {
                id: 1,
                resource: 1,
                after_launch: 1,
                window: WireHostBindingWindowV1::Exact {
                    byte_offset: 0,
                    byte_length: 8,
                },
            }],
        },
    };
    let observation_program_bytes =
        encode_host_execution_v1(&observation_program).expect("valid packet");
    assert_fixture(
        "observation-program",
        observation_program_bytes.clone(),
        include_bytes!("../../tests/fixtures/host-execution-v1/observation.execution.json"),
        include_str!("../../tests/fixtures/host-execution-v1/observation.execution.sha256"),
    );

    let observation_request = WireHostInvocationEnvelopeV1 {
        schema: HOST_INVOCATION_SCHEMA.to_owned(),
        version: HOST_INVOCATION_WIRE_VERSION,
        invocation: WireHostInvocationV1 {
            execution_sha256: host_execution_sha256(&observation_program_bytes),
            correlation_id: "opaque-observation-correlation".to_owned(),
            windows: Vec::new(),
            requested_observations: vec![1],
        },
    };
    assert_fixture(
        "observation-request",
        encode_host_invocation_v1(&observation_request).expect("valid packet"),
        include_bytes!(
            "../../tests/fixtures/host-execution-v1/observation-request.invocation.json"
        ),
        include_str!(
            "../../tests/fixtures/host-execution-v1/observation-request.invocation.sha256"
        ),
    );

    let final_entry_divergence = WireHostExecutionEnvelopeV1 {
        schema: HOST_EXECUTION_SCHEMA.to_owned(),
        version: HOST_EXECUTION_WIRE_VERSION,
        program: WireHostExecutionProgramV1 {
            target: WireHostTargetV1 {
                backend: "cuda".to_owned(),
                required_features: vec!["compute_90".to_owned(), "cuda".to_owned()],
            },
            artifact: artifact("ptx", FINAL_ENTRY_DIVERGENCE_ARTIFACT),
            resources: vec![resource(
                1,
                8,
                4,
                WireHostResourceLifetimeV1::Invocation,
                WireHostInitializationV1::Zeroed,
            )],
            kernels: vec![kernel(1, "__radix_f4b7", WireHostAccessV1::Write)],
            launches: vec![launch(1, 1, 1, 8)],
            dependencies: Vec::new(),
            observations: Vec::new(),
        },
    };
    assert_fixture(
        "final-entry-divergence",
        encode_host_execution_v1(&final_entry_divergence).expect("valid packet"),
        include_bytes!(
            "../../tests/fixtures/host-execution-v1/final-entry-divergence.execution.json"
        ),
        include_str!(
            "../../tests/fixtures/host-execution-v1/final-entry-divergence.execution.sha256"
        ),
    );
}

fn assert_fixture(name: &str, produced: Vec<u8>, expected: &[u8], expected_hash: &str) {
    assert!(
        !expected.ends_with(b"\n"),
        "{name} must contain exact compact packet bytes without a trailing newline"
    );
    assert_eq!(produced, expected, "{name} bytes are stale");
    assert_eq!(
        expected_hash,
        format!("{}\n", sha256_digest(expected)),
        "{name} identity is stale"
    );
}

fn artifact(format: &str, bytes: &[u8]) -> WireHostArtifactV1 {
    WireHostArtifactV1 {
        format: format.to_owned(),
        abi_version: ARTIFACT_ABI_VERSION,
        byte_length: u64::try_from(bytes.len()).expect("fixture byte length fits u64"),
        sha256: sha256_digest(bytes),
    }
}

fn resource(
    id: u32,
    byte_capacity: u64,
    byte_alignment: u32,
    lifetime: WireHostResourceLifetimeV1,
    initialization: WireHostInitializationV1,
) -> WireHostResourceV1 {
    WireHostResourceV1 {
        id,
        byte_capacity,
        byte_alignment,
        lifetime,
        initialization,
    }
}

fn kernel(id: u32, entry: &str, access: WireHostAccessV1) -> WireHostKernelV1 {
    WireHostKernelV1 {
        id,
        entry: entry.to_owned(),
        block: [1, 1, 1],
        slots: vec![WireHostKernelSlotV1 {
            id: 1,
            group: 0,
            binding: 0,
            access,
        }],
    }
}

fn launch(id: u32, kernel: u32, resource: u32, byte_length: u64) -> WireHostLaunchV1 {
    WireHostLaunchV1 {
        id,
        kernel,
        grid: [1, 1, 1],
        bindings: vec![binding(1, resource, byte_length)],
    }
}

fn binding(slot: u32, resource: u32, byte_length: u64) -> WireHostLaunchBindingV1 {
    WireHostLaunchBindingV1 {
        slot,
        resource,
        window: WireHostBindingWindowV1::Exact {
            byte_offset: 0,
            byte_length,
        },
    }
}
