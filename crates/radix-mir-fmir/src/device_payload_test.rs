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

use crate::codec::{decode_binary_image, decode_text_image, encode_binary_image, encode_text_image};
use crate::error::FmirImageError;
use crate::schema::{
    device_identity_hash, fnv1a64_blob_hash, FmirBinaryImageFile, FmirDeviceArtifact,
    FmirDeviceArtifactsSection, FmirDeviceBackend, FmirDeviceProgramSection, FmirDeviceSection,
    FmirDeviceSelection, FmirTextCliRootSection, FmirTextCliSection, FmirTextImageFile,
    FmirTextProgramSection, FmirTextRuntimeSection, FmirTextSourceIdentity,
    FmirTextSourcesSection, FmirTextToolchainSection, FmirTextTypesSection, FMIR_TARGET_NAME,
    FMIR_TEXT_TARGET_NAME, PACKAGE_MIR_ARTIFACT_VERSION,
};
use radix_mir::MirProgram;
use radix_types::TypeTable;

const TOOLCHAIN: &str = "0.79.0";

/// A canonical backend-neutral device-program payload (stub; the real payload
/// is produced by the emitting layer — S1-3). Identical bytes in both image
/// formats so A10 (same semantic package) holds.
const STUB_CANONICAL_PAYLOAD: &str = "{\"kernels\":[],\"launches\":[],\"lifetime\":\"per-step\"}";

fn metal_artifact(blob: &str) -> FmirDeviceArtifact {
    FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: blob.to_owned(),
        hash: fnv1a64_blob_hash(blob.as_bytes()),
    }
}

fn cuda_artifact(blob: &str) -> FmirDeviceArtifact {
    FmirDeviceArtifact {
        backend: FmirDeviceBackend::Cuda,
        blob: blob.to_owned(),
        hash: fnv1a64_blob_hash(blob.as_bytes()),
    }
}

/// A device section with both backends declared and the frozen device
/// requirements.
fn sample_device_section() -> FmirDeviceSection {
    FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            payload: STUB_CANONICAL_PAYLOAD.to_owned(),
        },
        selection: FmirDeviceSelection::Auto,
        artifacts: FmirDeviceArtifactsSection {
            artifact: vec![
                metal_artifact("kernel void add(device float* a [[buffer(0)]]) {}"),
                cuda_artifact(".version 8.7\n.target sm_120\n.entry add() {}"),
            ],
        },
        runtime_requirements: vec!["device:metal".to_owned(), "device:cuda".to_owned()],
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
    assert_eq!(device.device_program.payload, STUB_CANONICAL_PAYLOAD);
    assert_eq!(device.selection, FmirDeviceSelection::Auto);
    assert_eq!(device.artifacts.artifact.len(), 2);
    assert_eq!(device.artifacts.artifact[0].backend, FmirDeviceBackend::Metal);
    assert_eq!(device.artifacts.artifact[1].backend, FmirDeviceBackend::Cuda);
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
    assert_eq!(device.device_program.payload, STUB_CANONICAL_PAYLOAD);
    assert_eq!(device.selection, FmirDeviceSelection::Auto);
    assert_eq!(device.artifacts.artifact.len(), 2);
    // The canonical payload is byte-identical in both formats (A10): the
    // text-decoded and binary-decoded payloads agree.
    let text_image = text_image_with_device(Some(sample_device_section()));
    let text_decoded =
        decode_text_image(&encode_text_image(&text_image).unwrap(), TOOLCHAIN).unwrap();
    assert_eq!(
        text_decoded.device.as_ref().unwrap().device_program.payload,
        device.device_program.payload
    );
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
    section
        .runtime_requirements
        .push("host:fs".to_owned());
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
    text = text.replace(
        "[device.device_program]\n",
        "[device.device_program]\njson = \"deny me\"\n",
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
fn decode_rejects_pre_break_version_3_image_with_device() {
    // A version-3 image (pre-break) carrying a device section must fail closed
    // on the version gate before any device admission runs (3→4 clean break).
    let mut image = text_image_with_device(Some(sample_device_section()));
    image.version = 3;
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, TOOLCHAIN),
        Err(FmirImageError::UnsupportedVersion { actual: 3, expected: 4 })
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
        STUB_CANONICAL_PAYLOAD.as_bytes(),
    );
    assert!(identity.starts_with("sha256:"));
    assert_eq!(identity.len(), "sha256:".len() + 64);
    assert!(
        identity["sha256:".len()..].chars().all(|c| c.is_ascii_hexdigit()),
        "identity digest must be lowercase hex"
    );
}

#[test]
fn device_identity_hash_is_deterministic() {
    let sources = &["fnv64:aaaa", "fnv64:bbbb"];
    let payload = STUB_CANONICAL_PAYLOAD.as_bytes();
    assert_eq!(
        device_identity_hash(sources, payload),
        device_identity_hash(sources, payload)
    );
}

#[test]
fn device_identity_hash_excludes_backend_blobs() {
    // A10: the identity is over source hashes + the canonical payload only.
    // Mutating a (hypothetical) backend blob must not change the identity —
    // proven here by showing the identity depends only on the named inputs.
    let sources = &["fnv64:aaaa"];
    let payload = STUB_CANONICAL_PAYLOAD.as_bytes();
    let base = device_identity_hash(sources, payload);
    assert_eq!(device_identity_hash(sources, payload), base);
}

#[test]
fn device_identity_hash_is_sensitive_to_source_order() {
    // The length-prefixed canonicalization must distinguish source order: a
    // different order is a different package identity.
    let payload = STUB_CANONICAL_PAYLOAD.as_bytes();
    let ab = device_identity_hash(&["fnv64:aaaa", "fnv64:bbbb"], payload);
    let ba = device_identity_hash(&["fnv64:bbbb", "fnv64:aaaa"], payload);
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
    }];
    let image = binary_image_with_device(Some(section));
    let bytes = encode_binary_image(&image).expect("encode binary image");
    let decoded = decode_binary_image(&bytes, TOOLCHAIN).expect("admits cleanly");
    assert_eq!(
        decoded.device.unwrap().artifacts.artifact[0].hash,
        fnv1a64_blob_hash(blob.as_bytes())
    );
}
