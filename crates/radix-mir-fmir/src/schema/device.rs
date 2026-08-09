use serde::{Deserialize, Serialize};

use super::distributed::FmirDistributedExecutionSection;
use super::wire::{
    WireDeviceProgram, WireInvocationMode, WireKvCacheLayout, WireSessionInput,
    WireSessionObservationCadence,
};

// ---------------------------------------------------------------------------
// Device payload (N1.7 / architecture record §7)
// ---------------------------------------------------------------------------

/// The optional `device` section on an FMIR image (N1.7). Absent for pure
/// CPU/FMIR packages. Carries the canonical backend-neutral device program,
/// the backend selection request, the declared backend artifacts (each with
/// its own FNV-1a provenance hash), and the device runtime requirements.
///
/// The section is shared verbatim between the text (`fmir-text`, TOML) and
/// binary (`fmir`, postcard) images so Metal and CUDA routes derive identical
/// semantic bytes (A10). `deny_unknown_fields` is part of the contract: an
/// unknown key fails closed at parse time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceSection {
    /// Canonical backend-neutral device-program wire (N3.3): the typed
    /// COMPLETE device program, identical in both image formats (A10).
    pub device_program: FmirDeviceProgramSection,
    /// Backend selection request (from `--backend` / manifest / `auto`).
    pub selection: FmirDeviceSelection,
    /// Declared backend artifacts (Metal MSL, CUDA PTX) + per-blob FNV-1a
    /// provenance hashes (§3.4). The CUDA artifact carries its
    /// logical-entry → symbol mapping as per-artifact metadata. Blobs are
    /// **excluded** from the same-package identity; each carries its own
    /// provenance hash for A9 receipts.
    pub artifacts: FmirDeviceArtifactsSection,
    /// Declared host input values for the program's input buffers (run data,
    /// NOT program semantics — excluded from the canonical program bytes).
    pub declared_inputs: Vec<FmirDeviceInput>,
    /// Device runtime requirements. Extends the
    /// [`crate::schema::is_known_host_requirement`] allowlist with `device:metal`,
    /// `device:cuda`. Unknown `device:` requirements fail admission at
    /// package load ([`crate::schema::check_device_requirements`]).
    pub runtime_requirements: Vec<String>,
    /// The optional distributed execution section (MD2-W1). `None` for
    /// single-device packages (MD-A15) — single-device images keep
    /// the independent single-device path (the A10 identity and the nested
    /// `WIRE_DEVICE_PROGRAM_VERSION` bytes are untouched). `Some` for
    /// distributed images: the section carries its OWN version ratchet
    /// ([`crate::schema::WIRE_DISTRIBUTED_SECTION_VERSION`]) and is admitted
    /// fail-closed at load; the single-device run route rejects it
    /// ("distributed execution requires MD3") until MD3. Old consumers
    /// reject the new field via `deny_unknown_fields` (fail-closed, A7).
    pub distributed: Option<FmirDistributedExecutionSection>,
    /// The optional cadence/session section (GI4-2). `None` for packages
    /// without an inference-session surface — single-device packages do not
    /// require the section (the MD-A15 precedent; the single-device
    /// path, the A10 identity, and the nested
    /// `WIRE_DEVICE_PROGRAM_VERSION` bytes are untouched). `Some` for
    /// session-carrying images: the section carries its OWN version ratchet
    /// ([`crate::schema::WIRE_SESSION_SECTION_VERSION`]) and is admitted
    /// fail-closed at load (the MD2-W1 sibling-field precedent — no
    /// `WIRE_DEVICE_PROGRAM_VERSION` bump). The MD2 `distributed` sibling
    /// section files are never co-edited.
    pub session: Option<FmirSessionSection>,
}

/// The typed device-program section (S3-A4 / N3.3): the opaque payload string
/// of v4 is replaced by the typed complete program with `deny_unknown_fields`
/// admission. CUDA symbols and host input values are absent here — they are
/// not program semantics; they ride the per-artifact metadata and the
/// declared-inputs section, so the canonical program bytes are semantics-only
/// (A10 identity becomes honest).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceProgramSection {
    /// Wire/codec version — the faber-owned codec ratchet, bumped in
    /// lockstep with [`crate::schema::PACKAGE_MIR_ARTIFACT_VERSION`]. Fail-closed admission:
    /// checked before any field-level interpretation, so an old payload
    /// fails with a structured version diagnostic (never a silent default).
    pub v: u32,
    /// The complete device program: kernels, launches, lifetime, results.
    pub program: WireDeviceProgram,
}

/// One declared host input for an input buffer (run data, not program
/// semantics — absent from the canonical program bytes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceInput {
    /// Buffer name this input targets (the program's input-buffer identity).
    pub name: String,
    /// Flat f32 values (row-major).
    pub values: Vec<f32>,
}

/// One logical-entrypoint → emitted-symbol identity for a backend artifact
/// (the **entrypoint symbol map** — the DDCP2-U2 migration of
/// `FmirDeviceSymbol.entry/.symbol`; per-artifact metadata, NOT a program
/// semantic). The map is resolved at preparation, never on the hot path.
/// Metal artifacts carry no mapping: the logical entry IS the launch name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceSymbol {
    /// Logical entrypoint identity (a kernel-unit fact of the program).
    pub entry: String,
    /// Emitted `.entry` symbol the host launches by.
    pub symbol: String,
}
/// Backend selection request recorded in the image (N1.1). Fail-closed: an
/// unknown value fails at parse time. Migrated to backend-id enum semantics
/// (DDCP2-U2): an unknown selection id is `Unknown(String)` — representable
/// on the image and rejected honestly at selection resolution, never
/// silently remapped to a compiled-in backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FmirDeviceSelection {
    /// Automatic backend resolution at run time.
    Auto,
    /// Apple Metal.
    Metal,
    /// NVIDIA CUDA.
    Cuda,
    /// Any other declared backend-selection id. Representable; rejected
    /// honestly at selection resolution (no compiled-in leaf matches).
    Unknown(String),
}

/// Declared backend artifacts list (N1.7 / §3.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceArtifactsSection {
    pub artifact: Vec<FmirDeviceArtifact>,
}

/// One declared device artifact: the frozen **DeviceArtifact packet**
/// (DDCP2-U2). A versioned, encoded, identity-bound artifact fact — typed
/// backend id (unknown-representable), explicit format id, materialization
/// stage, typed target + features, ABI/schema version, canonical raw bytes,
/// explicit payload encoding, the entrypoint symbol map, and the
/// `content_sha256`/`packet_sha256` identity fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceArtifact {
    /// Typed backend identity — compiled-in leaves plus the representable
    /// unknown case ([`FmirDeviceBackend::Unknown`]). Unknown backends are
    /// rejected honestly at admission (contract §Unknown future backends),
    /// never silently dropped, remapped, or fallback-laned.
    pub backend: FmirDeviceBackend,
    /// Explicit format id — declared, never inferred from blob content.
    pub format: DeviceArtifactFormat,
    /// Materialization stage (`compiler-input` | `finalized-binary`).
    pub stage: MaterializationStage,
    /// Typed target identity + required features (the typed replacement for
    /// the string `runtime_requirements` allowlist).
    pub target: DeviceTargetId,
    /// ABI/schema version of the artifact (an input to `packet_sha256`).
    pub abi_version: u32,
    /// Canonical raw bytes of the artifact payload — the hashing substrate
    /// (`content_sha256` covers decoded bytes only, never the transport
    /// spelling).
    pub bytes: Vec<u8>,
    /// Explicit payload-encoding declaration (`text` = canonical UTF-8;
    /// `binary` = the declared `binary:<encoding-tag>:<decoded-byte-length>:
    /// <canonical-unpadded-base64>` spelling). No binary is smuggled through
    /// UTF-8 or base64 without this declaration.
    pub encoding: DevicePayloadEncoding,
    /// Entrypoint symbol map: logical entrypoint identity → emitted symbol
    /// (per-artifact metadata, NOT a program semantic; resolved at
    /// preparation, never on the hot path). Empty for Metal (the logical
    /// entry IS the launch name).
    pub entrypoints: Vec<FmirDeviceSymbol>,
    /// Content digest of the canonical decoded raw payload bytes (canonical
    /// `sha256:<64-hex>`). Computed and re-verified by the hashing/identity
    /// unit (DDCP2-U3); the field is part of the frozen packet shape.
    pub content_sha256: String,
    /// Packet digest over the versioned artifact metadata + `content_sha256`
    /// (never semantic identity; canonical `sha256:<64-hex>`). Computed and
    /// re-verified by the hashing/identity unit (DDCP2-U3).
    pub packet_sha256: String,
}

impl FmirDeviceArtifact {
    /// The declared binary transport spelling
    /// (`binary:<encoding-tag>:<decoded-byte-length>:<canonical-unpadded-base64>`)
    /// for a binary-encoded artifact; empty for text-encoded payloads
    /// (contract §Payload encoding).
    #[must_use]
    pub fn binary_payload_spelling(&self) -> String {
        match &self.encoding {
            DevicePayloadEncoding::Text => String::new(),
            DevicePayloadEncoding::Binary { encoding_tag, .. } => format!(
                "binary:{encoding_tag}:{}:{}",
                self.bytes.len(),
                canonical_unpadded_base64(&self.bytes)
            ),
        }
    }

    /// Validate the explicit payload-encoding declaration against the
    /// canonical raw bytes (contract §Payload encoding): text payloads are
    /// canonical UTF-8 (valid UTF-8, no BOM, no alternative-encoding
    /// disguise); binary payloads carry the declared
    /// `binary:<encoding-tag>:<decoded-byte-length>:<canonical-unpadded-base64>`
    /// spelling with a decoded byte length equal to the artifact bytes and a
    /// canonical unpadded base64 that re-encodes the artifact bytes. No
    /// binary is smuggled through UTF-8 or base64 without this schema-level
    /// declaration.
    ///
    /// # Errors
    ///
    /// Returns the first [`DevicePayloadEncodingError`] the declaration
    /// violates.
    pub fn validate_encoding(&self) -> Result<(), DevicePayloadEncodingError> {
        match &self.encoding {
            DevicePayloadEncoding::Text => {
                let text = std::str::from_utf8(&self.bytes).map_err(|error| {
                    DevicePayloadEncodingError::NonCanonicalUtf8 {
                        detail: error.to_string(),
                    }
                })?;
                if text.starts_with('\u{feff}') {
                    return Err(DevicePayloadEncodingError::Utf8Bom {
                        detail: "text payload begins with a UTF-8 BOM".to_owned(),
                    });
                }
                Ok(())
            }
            DevicePayloadEncoding::Binary {
                encoding_tag,
                decoded_byte_length,
                canonical_unpadded_base64: declared_base64,
            } => {
                if encoding_tag.is_empty() {
                    return Err(DevicePayloadEncodingError::EmptyEncodingTag);
                }
                if *decoded_byte_length != self.bytes.len() as u64 {
                    return Err(DevicePayloadEncodingError::DecodedByteLengthMismatch {
                        declared: *decoded_byte_length,
                        actual: self.bytes.len(),
                    });
                }
                let canonical = canonical_unpadded_base64(&self.bytes);
                if canonical != *declared_base64 {
                    return Err(DevicePayloadEncodingError::NonCanonicalBase64 {
                        detail: format!(
                            "declared `{declared_base64}` is not the canonical unpadded base64 (`{canonical}`) of the decoded bytes"
                        ),
                    });
                }
                Ok(())
            }
        }
    }
}

/// Backend a declared artifact targets (N1.3). `auto` is a selection, not a
/// backend. Migrated to backend-id enum semantics (DDCP2-U2): the
/// compiled-in leaves are the unit variants; any other backend id is
/// `Unknown(id)` — representable on the artifact packet and rejected
/// honestly by admission, never silently dropped, never remapped to a
/// different backend, never given a fallback lane (contract §Unknown future
/// backends).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FmirDeviceBackend {
    /// Apple Metal (MSL source).
    Metal,
    /// NVIDIA CUDA (PTX text).
    Cuda,
    /// Any other backend id (e.g. a future AMD/WebGPU leaf). Representable;
    /// rejected honestly at admission with a stable structured diagnostic.
    Unknown(String),
}

impl FmirDeviceBackend {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn spelling(&self) -> String {
        match self {
            Self::Metal => "metal".to_owned(),
            Self::Cuda => "cuda".to_owned(),
            Self::Unknown(id) => id.clone(),
        }
    }
}

/// The explicit artifact format id (DDCP2-U2 / contract §ArtifactPacket) —
/// a declared fact, never inferred from blob content. Unknown formats are
/// representable and rejected honestly, like unknown backends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceArtifactFormat {
    /// Metal Shading Language source.
    Msl,
    /// NVIDIA PTX text.
    Ptx,
    /// WGSL source.
    Wgsl,
    /// LLVM IR text.
    LlvmIr,
    /// A linked ELF code object.
    Elf,
    /// Any other declared format id.
    Unknown(String),
}

impl DeviceArtifactFormat {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn spelling(&self) -> String {
        match self {
            Self::Msl => "msl".to_owned(),
            Self::Ptx => "ptx".to_owned(),
            Self::Wgsl => "wgsl".to_owned(),
            Self::LlvmIr => "llvm-ir".to_owned(),
            Self::Elf => "elf".to_owned(),
            Self::Unknown(id) => id.clone(),
        }
    }
}

/// The materialization stage of a device artifact (DDCP2-U2 / contract
/// §ArtifactPacket): `compiler-input` is the materializer's pre-lowering
/// text input; `finalized-binary` is the linked executable artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaterializationStage {
    /// The materializer's compiler input (pre-lowering text).
    CompilerInput,
    /// The linked final artifact the host loads.
    FinalizedBinary,
}

/// The typed target identity of a device artifact (DDCP2-U2): the declared
/// target id plus the required feature ids — the typed replacement for the
/// string `runtime_requirements` allowlist. Unknown target/feature facts are
/// representable and rejected honestly, never guessed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceTargetId {
    /// The declared target id (e.g. `macos-arm64`, `sm_120`).
    pub id: String,
    /// Required feature ids (typed).
    pub required_features: Vec<String>,
}

/// The explicit payload-encoding declaration (DDCP2-U2 / contract §Payload
/// encoding). Text payloads are canonical UTF-8 (valid UTF-8, no BOM, no
/// alternative-encoding disguise). Binary payloads use the declared transport
/// spelling
/// `binary:<encoding-tag>:<decoded-byte-length>:<canonical-unpadded-base64>`
/// — no binary is smuggled through UTF-8 or base64 without this schema-level
/// declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DevicePayloadEncoding {
    /// Text payload: the artifact bytes are the canonical UTF-8 payload.
    Text,
    /// Binary payload with the declared transport spelling.
    Binary {
        /// The encoding tag (e.g. `llvm-ir`, `elf`).
        encoding_tag: String,
        /// The decoded byte length (must equal the artifact bytes' length).
        decoded_byte_length: u64,
        /// The canonical unpadded base64 of the decoded bytes.
        canonical_unpadded_base64: String,
    },
}

/// A payload-encoding declaration inconsistent with the artifact's canonical
/// raw bytes (contract §Payload encoding). Stable structured diagnostics;
/// admission rejects with [`crate::error::FmirImageError::ArtifactEncodingMismatch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePayloadEncodingError {
    /// A text-encoded payload's bytes are not valid UTF-8.
    NonCanonicalUtf8 { detail: String },
    /// A text-encoded payload begins with a UTF-8 BOM.
    Utf8Bom { detail: String },
    /// A binary-encoded payload carries an empty encoding tag.
    EmptyEncodingTag,
    /// The declared decoded byte length disagrees with the artifact bytes.
    DecodedByteLengthMismatch { declared: u64, actual: usize },
    /// The declared base64 is not the canonical unpadded base64 of the bytes.
    NonCanonicalBase64 { detail: String },
}

impl std::fmt::Display for DevicePayloadEncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonCanonicalUtf8 { detail } => {
                write!(f, "text payload is not canonical UTF-8: {detail}")
            }
            Self::Utf8Bom { detail } => write!(f, "{detail}"),
            Self::EmptyEncodingTag => {
                write!(f, "binary payload carries an empty encoding tag")
            }
            Self::DecodedByteLengthMismatch { declared, actual } => write!(
                f,
                "binary payload declares {declared} decoded bytes but the artifact carries {actual}"
            ),
            Self::NonCanonicalBase64 { detail } => write!(f, "{detail}"),
        }
    }
}

/// Canonical unpadded base64 (standard alphabet, no padding) of `bytes` —
/// the base64 of the binary transport spelling
/// (`binary:<encoding-tag>:<decoded-byte-length>:<canonical-unpadded-base64>`).
/// Hand-rolled (no base64 crate dependency; radix-mir-fmir keeps its
/// dependency surface).
#[must_use]
pub fn canonical_unpadded_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(triple >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[triple as usize & 0x3f] as char);
        }
    }
    out
}

/// The version ratchet of the optional cadence/session section (GI4-2; CTO
/// S12). The section carries its OWN nested version, admitted fail-closed
/// **before field interpretation** (the `admit_device_section` pattern, FC4)
/// — independently of the nested device-program wire version and the outer
/// image version. The first session-section shape is v1; the nested
/// `WIRE_DEVICE_PROGRAM_VERSION` and the outer
/// `PACKAGE_MIR_ARTIFACT_VERSION = 7` stay untouched (the MD2-W1
/// sibling-field precedent — no `WIRE_DEVICE_PROGRAM_VERSION` bump).
pub const WIRE_SESSION_SECTION_VERSION: u32 = 1;

/// The optional cadence/session section of an FMIR device image (GI4-2).
///
/// Carries the GI4-1-frozen inference-session facts on the accepted wire
/// version as a versioned session surface with its own ratchet + admission:
/// the per-invocation input cadence (`WireInputUpdateCadence` per declared
/// session/host-input slot — which inputs update per token vs stay
/// resident), the workload modes ([`WireInvocationMode`] — `prefill` /
/// `scalar_decode`), the observation cadence
/// ([`WireSessionObservationCadence`]), and the typed [`WireKvCacheLayout`]
/// carriage (GI4-1 §3). Absent (`None`) for packages without an
/// inference-session surface — single-device packages do not require the
/// section (the MD-A15 precedent). The MD2 `distributed` sibling section is
/// never co-edited (single schema owner; lane queue §2/§5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirSessionSection {
    /// The section's OWN version ratchet (A7): admitted fail-closed before
    /// any field-level interpretation, independently of the nested
    /// device-program wire version and the outer image version.
    pub v: u32,
    /// Declared session/host-input slots with their per-invocation input
    /// cadence.
    pub inputs: Vec<WireSessionInput>,
    /// The workload modes this session executes (`prefill` / `scalar_decode`
    /// — the regime labels for engine receipts, S6 regime schema).
    pub invocation_modes: Vec<WireInvocationMode>,
    /// The declared observation cadence of the session's observation.
    pub observation_cadence: WireSessionObservationCadence,
    /// The typed KV-cache layout carriage (GI4-1 §3).
    pub kv_layout: WireKvCacheLayout,
}
