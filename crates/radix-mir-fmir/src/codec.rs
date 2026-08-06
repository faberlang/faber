//! Text (`fmir-text`, TOML) and binary (`fmir`, postcard) FMIR codecs.
//!
//! All decode paths gate on the artifact version and target tag before
//! returning the image, and the toolchain version is checked against the
//! expected compiler version supplied by the caller (faber's crate version).
//! Every failure is a structured [`FmirImageError`]; no path panics.

use crate::error::FmirImageError;
use crate::schema::{
    admit_device_section, check_target, check_toolchain, check_version, FmirBinaryImageFile,
    FmirTextImageFile, FMIR_TARGET_NAME, FMIR_TEXT_TARGET_NAME, PACKAGE_MIR_ARTIFACT_VERSION,
};

/// Serialize a text image to pretty TOML.
///
/// # Errors
///
/// Returns [`FmirImageError::Parse`] if the image shape cannot be
/// serialized to TOML (e.g. unsupported field values).
pub fn encode_text_image(image: &FmirTextImageFile) -> Result<String, FmirImageError> {
    toml::to_string_pretty(image).map_err(|error| FmirImageError::Parse(error.to_string()))
}

/// Serialize a binary image to `postcard` bytes.
///
/// # Errors
///
/// Returns [`FmirImageError::Parse`] if the image shape cannot be
/// serialized to `postcard` bytes.
pub fn encode_binary_image(image: &FmirBinaryImageFile) -> Result<Vec<u8>, FmirImageError> {
    postcard::to_allocvec(image).map_err(|error| FmirImageError::Parse(error.to_string()))
}

/// Decode and validate a text image.
///
/// The `expected_toolchain` is the compiler crate version the image must have
/// been produced with (faber passes its own `CARGO_PKG_VERSION`).
///
/// # Errors
///
/// Returns [`FmirImageError::Parse`] on TOML decode failure, and the
/// version / target / toolchain / device-admission gate errors
/// ([`FmirImageError::UnsupportedVersion`], target and toolchain
/// mismatches) from the fail-closed admission checks.
pub fn decode_text_image(
    text: &str,
    expected_toolchain: &str,
) -> Result<FmirTextImageFile, FmirImageError> {
    let image: FmirTextImageFile =
        toml::from_str(text).map_err(|error| FmirImageError::Parse(error.to_string()))?;
    check_version(image.version)?;
    check_target(&image.target, FMIR_TEXT_TARGET_NAME)?;
    check_toolchain(&image.toolchain.faber_cli_version, expected_toolchain)?;
    if let Some(device) = &image.device {
        admit_device_section(device)?;
    }
    Ok(image)
}

/// Pre-scan the first wire field of a postcard image as a `u32` varint.
///
/// `version` is the first serialized field in both image formats. A full
/// struct parse cannot report [`FmirImageError::UnsupportedVersion`] for a
/// legacy-layout image (the layout diverges before the parse completes), so
/// [`decode_binary_image`] pre-scans the varint to gate on the version first.
///
/// Returns `None` when the prefix is not a well-formed `u32` varint (empty
/// input, an unterminated continuation, or a fifth byte carrying more than
/// the top four bits); the full decode is then still attempted and fails
/// closed with [`FmirImageError::Parse`].
fn read_version_varint(bytes: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    for (shift, &byte) in bytes.iter().take(5).enumerate() {
        let payload = u32::from(byte & 0x7f);
        if shift == 4 && payload > 0x0f {
            // The fifth byte carries only bits 28..=31; more overflows u32.
            return None;
        }
        value |= payload << (shift * 7);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    // Continuation bit never cleared within the five-byte u32 varint bound.
    None
}

/// Decode and validate a binary image.
///
/// The first wire field (`version`) is pre-scanned as a postcard varint so a
/// legacy-layout image (e.g. version 3 without the device section, N1.7)
/// fails with [`FmirImageError::UnsupportedVersion`] instead of a structural
/// `Parse` error.
///
/// The `expected_toolchain` is the compiler crate version the image must have
/// been produced with (faber passes its own `CARGO_PKG_VERSION`).
///
/// # Errors
///
/// Returns [`FmirImageError::UnsupportedVersion`] when the pre-scanned
/// version field does not match the supported artifact version,
/// [`FmirImageError::Parse`] on postcard decode failure, and the target /
/// toolchain / device-admission gate errors from the fail-closed checks.
pub fn decode_binary_image(
    bytes: &[u8],
    expected_toolchain: &str,
) -> Result<FmirBinaryImageFile, FmirImageError> {
    // Gate on the version before the structural decode: a legacy image whose
    // layout predates the optional device section (3→4 clean break, N1.7)
    // must report UnsupportedVersion, not a generic Parse error.
    if let Some(actual) = read_version_varint(bytes) {
        if actual != PACKAGE_MIR_ARTIFACT_VERSION {
            return Err(FmirImageError::UnsupportedVersion {
                actual,
                expected: PACKAGE_MIR_ARTIFACT_VERSION,
            });
        }
    }
    let image: FmirBinaryImageFile =
        postcard::from_bytes(bytes).map_err(|error| FmirImageError::Parse(error.to_string()))?;
    check_version(image.version)?;
    check_target(&image.target, FMIR_TARGET_NAME)?;
    check_toolchain(&image.toolchain.faber_cli_version, expected_toolchain)?;
    if let Some(device) = &image.device {
        admit_device_section(device)?;
    }
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
        // A pre-break version-5 image fails closed under the 5→6 clean break
        // (Stage 3R U1: the wire carries the F1–F7 semantic facts).
        let mut image = text_image();
        image.version = 5;
        let text = encode_text_image(&image).expect("encode text image");
        assert!(matches!(
            decode_text_image(&text, "0.79.0"),
            Err(FmirImageError::UnsupportedVersion {
                actual: 5,
                expected: 6
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
        // Leading byte has the continuation bit set, so the first varint is
        // malformed; neither the pre-scan nor the full decode can read it.
        assert!(matches!(
            decode_binary_image(b"\x80\xff\xfe\xfd", "0.79.0"),
            Err(FmirImageError::Parse(_))
        ));
    }
}
