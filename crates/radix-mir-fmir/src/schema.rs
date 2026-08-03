//! FMIR portable package-image schema.
//!
//! Owns the wire structs, the artifact-version constant (R0 red: clean break
//! on `MirConstant::UInt`), the `fmir-text` / `fmir` target tags, the FNV-1a
//! hash used for source identities and runner fingerprints, and the host
//! runtime-requirement allowlist.

use radix_mir::MirProgram;
use radix_types::TypeTableSnapshot;
use serde::{Deserialize, Serialize};

/// Image schema version (R0 red: 2→3 clean break for `MirConstant::UInt`).
pub const PACKAGE_MIR_ARTIFACT_VERSION: u32 = 3;

/// Text image `target` tag.
pub const FMIR_TEXT_TARGET_NAME: &str = "fmir-text";

/// Binary image `target` tag.
pub const FMIR_TARGET_NAME: &str = "fmir";

/// FNV-1a 64-bit offset basis (standard).
pub const FNV1A64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64-bit prime (standard).
pub const FNV1A64_PRIME: u64 = 0x0100_0000_01b3;

/// TOML text image top-level schema. `deny_unknown_fields` is part of the
/// contract: unknown keys fail closed at parse time.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextImageFile {
    /// First wire field in both formats; decode gates on it before any
    /// payload interpretation.
    pub version: u32,
    pub target: String,
    pub package_root: String,
    pub entry: String,
    pub entry_function: String,
    pub toolchain: FmirTextToolchainSection,
    pub runtime: FmirTextRuntimeSection,
    pub sources: FmirTextSourcesSection,
    pub cli: Option<FmirTextCliSection>,
    pub exit_code: Option<i32>,
    pub types: FmirTextTypesSection,
    pub interner: Vec<String>,
    pub program: FmirTextProgramSection,
}

/// postcard binary image schema. Field order is the serialized layout
/// (`version` first) and must not be reordered — a silent byte break.
#[derive(Debug, Serialize, Deserialize)]
pub struct FmirBinaryImageFile {
    /// First wire field (postcard varint); decode gates on it first.
    pub version: u32,
    pub target: String,
    pub package_root: String,
    pub entry: String,
    pub entry_function: String,
    pub toolchain: FmirTextToolchainSection,
    pub runtime: FmirTextRuntimeSection,
    pub sources: FmirTextSourcesSection,
    pub cli: Option<FmirTextCliSection>,
    pub exit_code: Option<i32>,
    pub types: FmirTextTypesSection,
    pub interner: Vec<String>,
    pub program: MirProgram,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextRuntimeSection {
    pub requirement: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextToolchainSection {
    pub faber_cli_version: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextSourcesSection {
    pub source: Vec<FmirTextSourceIdentity>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextSourceIdentity {
    pub file: String,
    pub hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextTypesSection {
    pub table: TypeTableSnapshot,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextProgramSection {
    pub json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextCliSection {
    pub root: FmirTextCliRootSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextCliRootSection {
    pub record: String,
    pub operand: Vec<FmirTextCliOperand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextCliOperand {
    pub field: String,
    pub ty: FmirTextCliValueType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FmirTextCliValueType {
    Textus,
    Numerus,
    Fractus,
    Bivalens,
}

/// Standard FNV-1a 64-bit hash, used for source-identity hashes and the
/// generated runner's embedded-image fingerprint.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV1A64_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }
    hash
}

/// Whether a runtime requirement is a known `host:*` capability.
///
/// `kernel:*` requirements are validated by the `faber` loading policy, which
/// has access to the kernel module/verb manifest resolution in the `radix`
/// package.
pub fn is_known_host_requirement(requirement: &str) -> bool {
    matches!(
        requirement,
        "host:argv"
            | "host:exit"
            | "host:stdout"
            | "host:stderr"
            | "host:stdin"
            | "host:fs"
            | "host:env"
            | "host:cwd"
            | "host:pid"
            | "host:random"
            | "host:process"
    )
}

/// Fail-closed artifact-version gate. Versions ratchet as clean breaks;
/// there is no compatibility decoder.
pub(crate) fn check_version(actual: &u32) -> Result<(), crate::error::FmirImageError> {
    if *actual == PACKAGE_MIR_ARTIFACT_VERSION {
        Ok(())
    } else {
        Err(crate::error::FmirImageError::UnsupportedVersion {
            actual: *actual,
            expected: PACKAGE_MIR_ARTIFACT_VERSION,
        })
    }
}

/// Fail-closed target-tag gate: the image must declare the format it was
/// decoded as.
pub(crate) fn check_target(
    found: &str,
    expected: &'static str,
) -> Result<(), crate::error::FmirImageError> {
    if found == expected {
        Ok(())
    } else {
        Err(crate::error::FmirImageError::WrongTarget {
            found: found.to_owned(),
            expected,
        })
    }
}

/// Fail-closed toolchain gate: the image must have been produced by the
/// expected compiler version (supplied by the caller, not baked into this
/// crate).
pub(crate) fn check_toolchain(
    found: &str,
    expected: &str,
) -> Result<(), crate::error::FmirImageError> {
    if found == expected {
        Ok(())
    } else {
        Err(crate::error::FmirImageError::UnsupportedToolchain {
            found: found.to_owned(),
            expected: expected.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_version_is_3_for_unsigned_constant_schema() {
        assert_eq!(PACKAGE_MIR_ARTIFACT_VERSION, 3);
    }

    #[test]
    fn target_tags_are_configured() {
        assert_eq!(FMIR_TEXT_TARGET_NAME, "fmir-text");
        assert_eq!(FMIR_TARGET_NAME, "fmir");
    }

    #[test]
    fn fnv1a64_matches_reference_vector() {
        // Empty input must equal the offset basis.
        assert_eq!(fnv1a64(b""), FNV1A64_OFFSET);
        // Single byte "a".
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn host_requirements_allowlist() {
        assert!(is_known_host_requirement("host:argv"));
        assert!(is_known_host_requirement("host:fs"));
        assert!(is_known_host_requirement("host:process"));
        assert!(!is_known_host_requirement("host:nope"));
        assert!(!is_known_host_requirement("kernel:foo.bar"));
        assert!(!is_known_host_requirement(""));
    }
}
