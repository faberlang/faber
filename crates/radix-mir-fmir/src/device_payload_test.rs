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
    canonical_program_bytes, device_identity_hash, fnv1a64_blob_hash, FmirBinaryImageFile,
    FmirDeviceArtifact, FmirDeviceArtifactsSection, FmirDeviceBackend, FmirDeviceInput,
    FmirDeviceProgramSection, FmirDeviceSection, FmirDeviceSelection, FmirDeviceSymbol,
    FmirTextCliRootSection, FmirTextCliSection, FmirTextImageFile, FmirTextProgramSection,
    FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, WireAxisReductionPlan, WireBinding,
    WireBroadcastDeclaration, WireBroadcastFact, WireBufferIdentity, WireBufferLifetime,
    WireBufferRole, WireBufferVersion, WireCausalMaskedSoftmaxPlan, WireCollectionKernelPlan,
    WireCompanionDerivativeKind, WireCompanionRelation, WireCompanionSelectedInput,
    WireCompanionSelectedOutput, WireDependencyEdge, WireDeviceProgram, WireDeviceResource,
    WireDispatchSize, WireGatherPlan, WireInitializationPolicy, WireKernelLaunchPlan,
    WireKernelUnit, WireLaunchUnit, WireLayerNormalizationPlan, WireObservationCadence,
    WireObservationFact, WireProgramLifetime, WireReduceOp, WireResourceAccess, WireResultBuffer,
    WireRmsNormalizationPlan, WireRopePlan, WireRowSoftmaxPlan, WireSemanticValue,
    WireSemanticValueOrigin, WireStorageLayout, WireWorkgroupCount, WireWorkgroupSize,
    FMIR_TARGET_NAME, FMIR_TEXT_TARGET_NAME, PACKAGE_MIR_ARTIFACT_VERSION,
    WIRE_DEVICE_PROGRAM_VERSION,
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

fn metal_artifact(blob: &str) -> FmirDeviceArtifact {
    FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: blob.to_owned(),
        hash: fnv1a64_blob_hash(blob.as_bytes()),
        symbols: Vec::new(),
    }
}

fn cuda_artifact(blob: &str) -> FmirDeviceArtifact {
    FmirDeviceArtifact {
        backend: FmirDeviceBackend::Cuda,
        blob: blob.to_owned(),
        hash: fnv1a64_blob_hash(blob.as_bytes()),
        symbols: vec![FmirDeviceSymbol {
            entry: "addita".to_owned(),
            symbol: "_Z6additaPU3AS1fS0_".to_owned(),
        }],
    }
}

/// A device section with both backends declared, the frozen device
/// requirements, a declared input, and the typed complete program.
fn sample_device_section() -> FmirDeviceSection {
    FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: WIRE_VERSION,
            program: sample_wire_program(),
        },
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
        runtime_requirements: vec!["device:metal".to_owned(), "device:cuda".to_owned()],
        // MD2-W1: absent for single-device packages (MD-A15) — the
        // independent single-device path.
        distributed: None,
    }
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
        device.runtime_requirements,
        vec!["device:metal".to_owned(), "device:cuda".to_owned()]
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
    assert_eq!(device.artifacts.artifact[1].symbols.len(), 1);
    assert_eq!(device.artifacts.artifact[1].symbols[0].entry, "addita");
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
fn decode_rejects_unknown_device_requirement() {
    let mut section = sample_device_section();
    section
        .runtime_requirements
        .push("device:vulkan".to_owned());
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::UnsupportedRuntimeRequirement { requirement }) => {
            assert_eq!(requirement, "device:vulkan");
        }
        other => panic!("expected UnsupportedRuntimeRequirement, got {other:?}"),
    }
}

#[test]
fn decode_rejects_misplaced_host_requirement_in_device_section() {
    // Host requirements belong in the top-level runtime section; a `host:`
    // entry in the device section is not a known device requirement and fails
    // admission.
    let mut section = sample_device_section();
    section.runtime_requirements.push("host:fs".to_owned());
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::UnsupportedRuntimeRequirement { requirement }) if requirement == "host:fs"
    ));
}

#[test]
fn decode_rejects_artifact_hash_mismatch_text() {
    let mut section = sample_device_section();
    section.artifacts.artifact[0].hash = "fnv64:deadbeefdeadbeef".to_owned();
    let image = text_image_with_device(Some(section));
    let text = encode_text_image(&image).expect("encode text image");
    match decode_text_image(&text, TOOLCHAIN) {
        Err(FmirImageError::ArtifactHashMismatch {
            backend,
            expected,
            actual,
        }) => {
            assert_eq!(backend, FmirDeviceBackend::Metal);
            assert_eq!(expected, "fnv64:deadbeefdeadbeef");
            assert_ne!(actual, expected);
        }
        other => panic!("expected ArtifactHashMismatch, got {other:?}"),
    }
}

#[test]
fn decode_rejects_artifact_hash_mismatch_binary() {
    let mut section = sample_device_section();
    // Tamper with the stored hash so it no longer matches the blob.
    section.artifacts.artifact[1].hash = fnv1a64_blob_hash(b"not the cuda blob");
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, TOOLCHAIN),
        Err(FmirImageError::ArtifactHashMismatch { backend, .. })
            if backend == FmirDeviceBackend::Cuda
    ));
}

#[test]
fn decode_rejects_tampered_blob_after_correct_hash() {
    // A correctly hashed artifact whose blob is mutated post-encode must be
    // caught. We build a valid image, then rewrite the blob text in the TOML
    // so the stored hash no longer matches.
    let image = text_image_with_device(Some(sample_device_section()));
    let mut text = encode_text_image(&image).expect("encode text image");
    text = text.replace("kernel void add", "kernel void sub");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::ArtifactHashMismatch { .. })
    ));
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

#[test]
fn decode_rejects_unknown_selection_value() {
    let image = text_image_with_device(Some(sample_device_section()));
    let mut text = encode_text_image(&image).expect("encode text image");
    text = text.replace("selection = \"auto\"", "selection = \"vulkan\"");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::Parse(_))
    ));
}

#[test]
fn decode_rejects_unknown_backend_value() {
    let image = text_image_with_device(Some(sample_device_section()));
    let mut text = encode_text_image(&image).expect("encode text image");
    text = text.replace("backend = \"metal\"", "backend = \"rocm\"");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::Parse(_))
    ));
}

// ---------------------------------------------------------------------------
// Version / toolchain admission ordering
// ---------------------------------------------------------------------------

#[test]
fn decode_rejects_pre_break_version_5_image_with_device() {
    // A version-5 image (the pre-3R layout without the F1–F7 wire sections)
    // carrying a device section must fail closed on the outer version gate
    // before any device admission runs (5→6 clean break, Stage 3R U1).
    let mut image = text_image_with_device(Some(sample_device_section()));
    image.version = 5;
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::UnsupportedVersion {
            actual: 5,
            expected: 6
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
        Err(FmirImageError::UnsupportedVersion {
            actual: 5,
            expected: 6
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
        Err(FmirImageError::UnsupportedDeviceProgramVersion { actual, expected }) => {
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
        Err(FmirImageError::UnsupportedDeviceProgramVersion { actual, expected })
            if actual == 3 && expected == WIRE_DEVICE_PROGRAM_VERSION
    ));
}

#[test]
fn decode_rejects_outer_version_independently_of_nested_version() {
    // The outer gate is also independent: a v5 image whose nested device
    // program is already the current v4 still fails on the OUTER version
    // first — the two versions never gate through each other (F7).
    let mut image = text_image_with_device(Some(sample_device_section()));
    image.version = 5;
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::UnsupportedVersion {
            actual: 5,
            expected: 6
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
        Err(FmirImageError::UnsupportedToolchain { found, expected })
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
    section.artifacts.artifact[1].symbols = vec![FmirDeviceSymbol {
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
        Err(FmirImageError::UnsupportedDeviceProgramVersion { actual, expected })
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
fn fnv1a64_blob_hash_admits_a_correctly_hashed_artifact() {
    // Round-trip: an artifact whose stored hash was computed by the helper
    // admits cleanly (positive admission).
    let blob = ".version 8.7\n.target sm_120\n";
    let mut section = sample_device_section();
    section.artifacts.artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Cuda,
        blob: blob.to_owned(),
        hash: fnv1a64_blob_hash(blob.as_bytes()),
        symbols: Vec::new(),
    }];
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    let decoded = decode_binary_image(&bytes, TOOLCHAIN).expect("admits cleanly");
    assert_eq!(
        decoded.device.unwrap().artifacts.artifact[0].hash,
        fnv1a64_blob_hash(blob.as_bytes())
    );
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

/// GI3-1: the inference-recipe wire mirrors (Gather / RmsNormalization /
/// Rope / CausalMaskedSoftmax) serialize and round-trip through the text
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
            epsilon_bits: (1e-5_f64 as f32).to_bits(),
            width: 960,
        }),
        WireCollectionKernelPlan::Rope(WireRopePlan {
            pos: 0,
            dim: 64,
            width: 64,
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
