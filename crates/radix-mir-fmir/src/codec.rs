//! Text (`fmir-text`, TOML) and binary (`fmir`, postcard) FMIR codecs.
//!
//! All decode paths gate on the artifact version and target tag before
//! returning the image, and the toolchain version is checked against the
//! expected compiler version supplied by the caller (faber's crate version).
//! Every failure is a structured [`FmirImageError`]; no path panics.

use crate::error::FmirImageError;
use crate::schema::{
    check_target, check_toolchain, check_version, FmirBinaryImageFile, FmirTextImageFile,
    FMIR_TARGET_NAME, FMIR_TEXT_TARGET_NAME,
};

/// Serialize a text image to pretty TOML.
pub fn encode_text_image(image: &FmirTextImageFile) -> Result<String, FmirImageError> {
    toml::to_string_pretty(image).map_err(|error| FmirImageError::Parse(error.to_string()))
}

/// Serialize a binary image to `postcard` bytes.
pub fn encode_binary_image(image: &FmirBinaryImageFile) -> Result<Vec<u8>, FmirImageError> {
    postcard::to_allocvec(image).map_err(|error| FmirImageError::Parse(error.to_string()))
}

/// Decode and validate a text image.
///
/// The `expected_toolchain` is the compiler crate version the image must have
/// been produced with (faber passes its own `CARGO_PKG_VERSION`).
pub fn decode_text_image(
    text: &str,
    expected_toolchain: &str,
) -> Result<FmirTextImageFile, FmirImageError> {
    let image: FmirTextImageFile =
        toml::from_str(text).map_err(|error| FmirImageError::Parse(error.to_string()))?;
    check_version(&image.version)?;
    check_target(&image.target, FMIR_TEXT_TARGET_NAME)?;
    check_toolchain(&image.toolchain.faber_cli_version, expected_toolchain)?;
    Ok(image)
}

/// Decode and validate a binary image.
///
/// The `expected_toolchain` is the compiler crate version the image must have
/// been produced with (faber passes its own `CARGO_PKG_VERSION`).
pub fn decode_binary_image(
    bytes: &[u8],
    expected_toolchain: &str,
) -> Result<FmirBinaryImageFile, FmirImageError> {
    let image: FmirBinaryImageFile =
        postcard::from_bytes(bytes).map_err(|error| FmirImageError::Parse(error.to_string()))?;
    check_version(&image.version)?;
    check_target(&image.target, FMIR_TARGET_NAME)?;
    check_toolchain(&image.toolchain.faber_cli_version, expected_toolchain)?;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{
        FmirTextCliRootSection, FmirTextCliSection, FmirTextProgramSection, FmirTextRuntimeSection,
        FmirTextSourceIdentity, FmirTextSourcesSection, FmirTextToolchainSection,
        FmirTextTypesSection, PACKAGE_MIR_ARTIFACT_VERSION,
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
        let mut image = text_image();
        image.version = 2;
        let text = encode_text_image(&image).expect("encode text image");
        assert!(matches!(
            decode_text_image(&text, "0.79.0"),
            Err(FmirImageError::UnsupportedVersion {
                actual: 2,
                expected: 3
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
            Err(FmirImageError::WrongTarget { found, expected: FMIR_TARGET_NAME }) if found == "scena"
        ));
    }

    #[test]
    fn decode_rejects_mismatched_toolchain() {
        let mut image = text_image();
        image.toolchain.faber_cli_version = "0.78.0".to_owned();
        let text = encode_text_image(&image).expect("encode text image");
        assert!(matches!(
            decode_text_image(&text, "0.79.0"),
            Err(FmirImageError::UnsupportedToolchain { found, expected }) if found == "0.78.0" && expected == "0.79.0"
        ));
    }

    #[test]
    fn decode_rejects_garbage_fail_closed() {
        assert!(matches!(
            decode_text_image("not toml at all {{{", "0.79.0"),
            Err(FmirImageError::Parse(_))
        ));
        assert!(matches!(
            decode_binary_image(b"\x00\xff\xfe\xfd", "0.79.0"),
            Err(FmirImageError::Parse(_))
        ));
    }
}
