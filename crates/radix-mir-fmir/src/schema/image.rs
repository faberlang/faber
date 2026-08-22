use radix_mir::MirProgram;
use radix_types::TypeTableSnapshot;
use serde::{Deserialize, Serialize};

use super::device::FmirDeviceSection;

/// TOML text image top-level schema. `deny_unknown_fields` is part of the
/// contract: unknown keys fail closed at parse time.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirTextImageFile {
    /// First wire field in both formats. Both decoders gate on it before any
    /// payload interpretation: the text decoder checks it after the TOML
    /// parse and before the target/toolchain/device admission checks; the
    /// binary decoder pre-scans the first postcard varint so a version
    /// mismatch fails with
    /// [`UnsupportedVersion`](crate::error::FmirImageError::VersionMismatch)
    /// before any structural decode.
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
    /// Optional device payload (N1.7). Absent (`None`) for pure CPU/FMIR
    /// packages; present for packages compiled with a GPU backend.
    pub device: Option<FmirDeviceSection>,
}

/// Postcard binary image schema. Field order is the serialized layout
/// (`version` first) and must not be reordered — a silent byte break.
#[derive(Debug, Serialize, Deserialize)]
pub struct FmirBinaryImageFile {
    /// First wire field (postcard varint). [`decode_binary_image`](crate::decode_binary_image)
    /// pre-scans this varint so a legacy-layout image (e.g. version 3
    /// without the device field, N1.7) fails with
    /// [`UnsupportedVersion`](crate::error::FmirImageError::VersionMismatch)
    /// instead of a structural `Parse` error.
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
    /// Optional device payload (N1.7). Absent (`None`) for pure CPU/FMIR
    /// packages; present for packages compiled with a GPU backend.
    pub device: Option<FmirDeviceSection>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FmirTextCliValueType {
    Textus,
    Numerus,
    Fractus,
    Bivalens,
    /// Final rest `textus` / `lista<textus>` operand. Consumes remaining argv.
    ListaTextus,
}
