use super::*;
use crate::schema::{
    FmirTextCliRootSection, FmirTextCliSection, FmirTextProgramSection, FmirTextRuntimeSection,
    FmirTextSourceIdentity, FmirTextSourcesSection, FmirTextToolchainSection, FmirTextTypesSection,
    PACKAGE_MIR_ARTIFACT_VERSION,
};
use radix_mir::MirProgram;
use radix_types::TypeTable;

fn text_image() -> FmirTextImageFile {
    FmirTextImageFile {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: FMIR_TEXT_TARGET_NAME.to_owned(),
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
        device: None,
    }
}

fn binary_image() -> FmirBinaryImageFile {
    FmirBinaryImageFile {
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
        device: None,
    }
}

#[test]
fn text_image_round_trips_through_codec() {
    let image = text_image();
    let text = encode_text_image(&image).expect("encode text image");
    let decoded = decode_text_image(&text, "0.79.0").expect("decode text image");
    assert_eq!(decoded.version, PACKAGE_MIR_ARTIFACT_VERSION);
    assert_eq!(decoded.target, FMIR_TEXT_TARGET_NAME);
    assert_eq!(decoded.entry_function, "run_entry");
    assert_eq!(decoded.toolchain.faber_cli_version, "0.79.0");
    assert_eq!(decoded.runtime.requirement, vec!["host:argv"]);
}

#[test]
fn binary_image_round_trips_through_codec() {
    let image = binary_image();
    let bytes = encode_binary_image(&image).expect("encode binary image");
    let decoded = decode_binary_image(&bytes, "0.79.0").expect("decode binary image");
    assert_eq!(decoded.version, PACKAGE_MIR_ARTIFACT_VERSION);
    assert_eq!(decoded.target, FMIR_TARGET_NAME);
    assert_eq!(decoded.entry_function, "run_entry");
}

#[test]
fn decode_rejects_unsupported_version() {
    // A pre-break version-5 image fails closed under the clean-break
    // ratchet (the image schema now carries the DeviceArtifact packet).
    let mut image = text_image();
    image.version = 5;
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, "0.79.0"),
        Err(FmirImageError::VersionMismatch {
            actual: 5,
            expected: PACKAGE_MIR_ARTIFACT_VERSION
        })
    ));
}

#[test]
fn decode_rejects_wrong_target() {
    let mut image = binary_image();
    image.target = "scena".to_owned();
    let bytes = encode_binary_image(&image).expect("encode binary image");
    assert!(matches!(
        decode_binary_image(&bytes, "0.79.0"),
        Err(FmirImageError::TargetMismatch { found, expected: FMIR_TARGET_NAME }) if found == "scena"
    ));
}

#[test]
fn decode_rejects_mismatched_toolchain() {
    let mut image = text_image();
    image.toolchain.faber_cli_version = "0.78.0".to_owned();
    let text = encode_text_image(&image).expect("encode text image");
    assert!(matches!(
        decode_text_image(&text, "0.79.0"),
        Err(FmirImageError::ToolchainMismatch { found, expected }) if found == "0.78.0" && expected == "0.79.0"
    ));
}

#[test]
fn decode_rejects_garbage_fail_closed() {
    assert!(matches!(
        decode_text_image("not toml at all {{{", "0.79.0"),
        Err(FmirImageError::Parse(_))
    ));
    // Leading byte has the continuation bit set, so the first varint is
    // malformed; neither the pre-scan nor the full decode can read it.
    assert!(matches!(
        decode_binary_image(b"\x80\xff\xfe\xfd", "0.79.0"),
        Err(FmirImageError::Parse(_))
    ));
}
