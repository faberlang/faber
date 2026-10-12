use serde::{Deserialize, Serialize};

use super::distributed::FmirDistributedExecutionSection;
use super::executor_plan::{WireExecutorPlan, WireExecutorPlanV2};
use super::wire::{
    WireDeviceProgram, WireInvocationMode, WireKvCacheLayout, WireKvCacheSemanticV2,
    WireSessionInput, WireSessionObservationCadence,
};

/// Legacy static executor-plan sections carry only the v1 selection records.
/// They remain a separate carrier: their constants are never reinterpreted as
/// runtime cursor facts.
pub const LEGACY_EXECUTOR_PLAN_SECTION_VERSION: u32 = 1;

/// Version of the executor-plan carrier that carries KV-B runtime bindings.
/// WIRE v8 remains unchanged; this sibling section ratchets independently and
/// fails closed on skew.
pub const EXECUTOR_PLAN_SECTION_VERSION: u32 = 2;

/// Version of the legacy cadence/session carrier. It remains named here so a
/// reader can distinguish an old static layout from the v2 semantic record
/// without filling missing fields with guessed facts.
pub const LEGACY_SESSION_SECTION_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Device payload (N1.7 / architecture record §7)
// ---------------------------------------------------------------------------

/// The optional `device` section on an FMIR image (N1.7). Absent for pure
/// CPU/FMIR packages. Carries the canonical backend-neutral device program,
/// the backend selection request, and the declared backend artifacts (each a
/// versioned `DeviceArtifact` packet with content/packet identity digests and
/// its typed target features).
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
    /// Optional separately versioned executor-plan section. It is absent for
    /// legacy/generated-only programs and old WIRE v8 payloads remain valid.
    #[serde(default)]
    pub executor_plan: Option<FmirExecutorPlanSection>,
    /// Backend selection request (from `--device` / manifest / `auto`).
    pub selection: FmirDeviceSelection,
    /// Declared backend artifacts (Metal MSL, CUDA PTX) — each a versioned
    /// `DeviceArtifact` packet with its `content_sha256`/`packet_sha256`
    /// identity digests, its typed target identity + required features, and
    /// the entrypoint symbol map. Blobs are **excluded** from the
    /// same-package identity; each artifact carries its own identity
    /// digests.
    pub artifacts: FmirDeviceArtifactsSection,
    /// Declared host input values for the program's input buffers (run data,
    /// NOT program semantics — excluded from the canonical program bytes).
    pub declared_inputs: Vec<FmirDeviceInput>,
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

/// The separately versioned executor-plan section carried beside WIRE v8.
/// It contains library entry, logical signature, binds, uniforms,
/// offsets/strides, workspace, backend requirements, and provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirExecutorPlanSection {
    /// Executor-plan section version, admitted before field interpretation.
    pub v: u32,
    /// One selection for each kernel/schedule entry, in program order.
    pub plans: Vec<WireExecutorPlan>,
}

impl FmirExecutorPlanSection {
    /// Project the target-neutral MIR executor section onto the admitted
    /// static wire mirror.
    ///
    /// Shared device-section admission requires [`EXECUTOR_PLAN_SECTION_VERSION`].
    /// This producer stamps that version so a produced image does not fail its
    /// own decode boundary. The payload remains the static selection records;
    /// those constants are never treated as runtime cursor facts.
    /// [`LEGACY_EXECUTOR_PLAN_SECTION_VERSION`] names previously serialized
    /// static carriers only.
    #[must_use]
    pub fn from_executor_plan(plan: &radix_mir::ExecutorPlanExtension) -> Self {
        Self {
            v: EXECUTOR_PLAN_SECTION_VERSION,
            plans: plan
                .selections
                .iter()
                .map(WireExecutorPlan::from_mir)
                .collect(),
        }
    }

    /// Return whether this carrier has the section version admitted on the
    /// device-section path. A legacy v1 static image is a different carrier
    /// and is not treated as a runtime-binding payload.
    #[must_use]
    pub const fn has_supported_version(&self) -> bool {
        self.v == EXECUTOR_PLAN_SECTION_VERSION
    }
}

/// The v2 executor-plan section carrier. Its payload is deliberately a
/// different field and type from [`FmirExecutorPlanSection::plans`], so a
/// legacy v1 static plan cannot be default-filled or reinterpreted as a
/// runtime-binding payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirExecutorPlanSectionV2 {
    /// The executor-plan section's own v2 ratchet.
    pub v: u32,
    /// KV-B's tagged runtime-binding payload.
    pub runtime_bindings: WireExecutorPlanV2,
}

impl FmirExecutorPlanSectionV2 {
    /// Wrap a delivered v2 runtime-binding payload without changing it.
    #[must_use]
    pub fn from_runtime_bindings(runtime_bindings: WireExecutorPlanV2) -> Self {
        Self {
            v: EXECUTOR_PLAN_SECTION_VERSION,
            runtime_bindings,
        }
    }

    /// Project the B2 producer payload onto the v2 section carrier.
    ///
    /// Version 2 is stamped only when a launch binding carries a tagged
    /// runtime source. A purely static plan stays on
    /// [`LEGACY_EXECUTOR_PLAN_SECTION_VERSION`] and must not claim the v2
    /// ratchet.
    #[must_use]
    pub fn from_executor_plan(
        plan: &radix_mir::device_program_plans::ExecutorPlanExtensionV2,
    ) -> Self {
        let v = if plan
            .launch_bindings
            .iter()
            .any(|binding| binding.runtime_source.is_runtime())
        {
            EXECUTOR_PLAN_SECTION_VERSION
        } else {
            LEGACY_EXECUTOR_PLAN_SECTION_VERSION
        };
        Self {
            v,
            runtime_bindings: WireExecutorPlanV2::from_mir(plan),
        }
    }

    /// Return whether this carrier has the exact v2 section version. A
    /// mismatch is rejected rather than decoded through the v1 static shape.
    #[must_use]
    pub const fn has_supported_version(&self) -> bool {
        self.v == EXECUTOR_PLAN_SECTION_VERSION
    }
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

/// The version ratchet of the typed reflection carrier (DDCP3-U0 (a) /
/// CTO9-6 a). The reflection carrier rides its OWN nested version, admitted
/// fail-closed **before field interpretation** (the `admit_device_section`
/// pattern / the MD2-W1 sibling-field precedent) — independently of the
/// artifact ABI version and the outer packet shape. The first carrier shape
/// is v1; concrete Metal/CUDA reflection CONTENT lands with DDCP4 leaf
/// conformance (a content extension bumps this ratchet, never silently).
pub const ARTIFACT_REFLECTION_VERSION: u32 = 1;

/// The versioned typed reflection carrier of a device artifact (DDCP3-U0 (a)
/// / CTO9-6 a): the FMIR mirror of the radix-mir `MirGpuReflection` surface
/// (`crates/radix-mir/src/abi/types.rs`) — the frozen §`ArtifactPacket`
/// `backend reflection/requirements` field. Backend reflection facts are
/// canonical structured facts about bindings/layout the backend requires;
/// the carrier is folded into `packet_sha256` in **canonical order**. This
/// shape is the typed carrier + hash fold + round-trip only — concrete
/// Metal/CUDA reflection content is DDCP4's (DDCP3 non-goal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirArtifactReflection {
    /// The reflection carrier's OWN version ratchet (A7): admitted
    /// fail-closed before any field-level interpretation.
    pub v: u32,
    /// Reflected kernel rows (canonical order at hash time).
    pub kernels: Vec<FmirReflectedKernel>,
}

/// One reflected kernel row of the typed reflection carrier (the FMIR mirror
/// of `MirKernelReflection`'s identity-bearing fields). Typed facts, never
/// reconstructed from emitted text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirReflectedKernel {
    /// The kernel's stable function identity (typed, never name-derived).
    pub function: String,
    /// The emitted entry name.
    pub entry_name: String,
    /// Declared resource bindings (canonical order at hash time).
    pub resources: Vec<FmirReflectedResource>,
}

impl FmirArtifactReflection {
    /// Fold the leaf-produced typed reflection (`MirGpuReflection`) into the
    /// carrier (DDCP4-U4 content): one row per reflected kernel, mapping the
    /// identity-bearing fields — kernel function identity, emitted entry
    /// name, and declared resource name/binding.
    ///
    /// WHY the version stays at [`ARTIFACT_REFLECTION_VERSION`]: the leaf
    /// content fills the existing v1 carrier shape exactly; it does not
    /// extend it (the DDCP3-U0 (a) ratchet rule — bump only on shape
    /// extension, never on content fill). Both leaves today emit the kernel
    /// unit's logical entry as the entry symbol (Metal: identity map; CUDA:
    /// entry → emitted symbol), so the logical entry is the kernel's stable
    /// function identity in the FMIR image — the mapping never derives it
    /// from emitted text.
    #[must_use]
    pub fn from_mir_reflection(reflection: &radix_mir::abi::MirGpuReflection) -> Self {
        Self {
            v: ARTIFACT_REFLECTION_VERSION,
            kernels: reflection
                .kernels
                .iter()
                .map(FmirReflectedKernel::from_mir_kernel)
                .collect(),
        }
    }
}

impl FmirReflectedKernel {
    fn from_mir_kernel(kernel: &radix_mir::abi::MirKernelReflection) -> Self {
        Self {
            function: kernel.entry_name.clone(),
            entry_name: kernel.entry_name.clone(),
            resources: kernel
                .resources
                .iter()
                .map(|resource| FmirReflectedResource {
                    name: resource
                        .source_name
                        .clone()
                        .unwrap_or_else(|| format!("binding:{}", resource.binding)),
                    binding: resource.binding,
                })
                .collect(),
        }
    }
}

/// One declared resource binding of a reflected kernel row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirReflectedResource {
    /// The resource's declared name.
    pub name: String,
    /// The declared binding index.
    pub binding: u32,
}

/// Backend selection request recorded in the image (N1.1). Migrated to
/// backend-id enum semantics (DDCP2-U2): an unknown selection id is
/// `Unknown(String)` — representable on the image and rejected honestly at
/// selection resolution, never silently remapped to a compiled-in backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FmirDeviceSelection {
    /// Automatic backend resolution at run time.
    Auto,
    /// Apple Metal.
    Metal,
    /// NVIDIA CUDA.
    Cuda,
    /// AMD (the OQ1 backend-id default — AMD-A3): the explicit selection of
    /// the AMDGPU leaf; admission requires an `amd` artifact for the
    /// selection (never a silent fallback).
    Amd,
    /// Any other declared backend-selection id. Representable; rejected
    /// honestly at selection resolution (no compiled-in leaf matches).
    Unknown(String),
}

impl FmirDeviceSelection {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn spelling(&self) -> String {
        match self {
            Self::Auto => "auto".to_owned(),
            Self::Metal => "metal".to_owned(),
            Self::Cuda => "cuda".to_owned(),
            Self::Amd => "amd".to_owned(),
            Self::Unknown(id) => id.clone(),
        }
    }
}

/// Declared backend artifacts list (N1.7 / §3.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceArtifactsSection {
    pub artifact: Vec<FmirDeviceArtifact>,
}

/// One declared device artifact: the frozen **`DeviceArtifact` packet**
/// (DDCP2-U2). A versioned, encoded, identity-bound artifact fact — typed
/// backend id (unknown-representable), explicit format id, materialization
/// stage, typed target + features, ABI/schema version, canonical raw bytes,
/// explicit payload encoding, the entrypoint symbol map, and the
/// `content_sha256`/`packet_sha256` identity fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceArtifact {
    /// Typed backend identity — compiled-in leaves plus the representable
    /// unknown case ([`FmirDeviceBackend::Unknown`]). Unknown-backend
    /// ARTIFACTS are **preserved-until-selection** (DDCP3-U0 (f) /
    /// CTO9-6 f, superseding the earlier reject-at-admission stance): they
    /// are admitted at load with encodings/digests/canonicalization still
    /// validated as far as their declared fields allow, and rejected at
    /// selection use ([`FmirImageError::NonAdmittedSelectedBackend`]) —
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
    /// re-verified by admission ([`FmirDeviceArtifact::compute_packet_sha256`]).
    pub packet_sha256: String,
    /// The hash-bound parent provenance of a `FinalizedBinary` artifact: the
    /// `compiler_input_packet_sha256` of the `CompilerInput` artifact this
    /// packet was finalized from. Preserved, never silently replaced —
    /// admission re-verifies the packet identity and the stage/provenance
    /// rule (`None` for `CompilerInput` artifacts, `Some` canonical spelling
    /// for `FinalizedBinary` artifacts).
    pub compiler_input_packet_sha256: Option<String>,
    /// Backend reflection/requirements (frozen §`ArtifactPacket` — the
    /// `backend reflection/requirements` field; DDCP3-U0 (a) / CTO9-6 a):
    /// the versioned typed reflection carrier (FMIR mirror of the radix-mir
    /// `MirGpuReflection` surface), folded into `packet_sha256` (canonical
    /// order). `None` for artifacts without declared reflection — the
    /// **add-only `#[serde(default)]` parse-compat field** (the
    /// same-compatibility-boundary contract with the mid-flight faber
    /// consumer: an old image without the field still parses; a carried
    /// `None` folds nothing into the packet identity, so reflection-less
    /// packets keep the DDCP2 identity). Concrete Metal/CUDA reflection
    /// CONTENT lands with DDCP4 leaf conformance — this shape is the typed
    /// carrier + hash fold only. Added LAST (after
    /// `compiler_input_packet_sha256`) so the binary (postcard) field order
    /// appends rather than repositions existing fields.
    #[serde(default)]
    pub reflection: Option<FmirArtifactReflection>,
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

    /// The canonical `content_sha256` over the artifact's canonical decoded
    /// raw payload bytes only — never the transport spelling (DDCP2-U3 /
    /// contract §`IdentityDomains` rule 1/3).
    #[must_use]
    pub fn compute_content_sha256(&self) -> String {
        crate::schema::sha256_digest(&self.bytes)
    }

    /// The canonical `packet_sha256` over the versioned artifact metadata +
    /// `content_sha256` (DDCP2-U3 / contract §`IdentityDomains` rule 1/4;
    /// DDCP3-U0 (a)/(b) revision): ABI/schema version, backend, format,
    /// materialization stage, target id + required features (**canonical
    /// order**), the entrypoint symbol map (**canonical order**), the typed
    /// `reflection` (canonical order, when present), the content digest, and
    /// the parent `compiler_input_packet_sha256` provenance when present.
    /// Equivalent sets (differing caller order, no duplicates) produce an
    /// identical packet identity; a stored packet hashed over a non-canonical
    /// order fails admission re-verification. Never semantic identity.
    #[must_use]
    pub fn compute_packet_sha256(&self) -> String {
        crate::schema::packet_sha256_canonical(
            self.abi_version,
            &self.backend.spelling(),
            &self.format.spelling(),
            self.stage,
            &self.target.id,
            &self.target.required_features,
            &self.entrypoints,
            self.reflection.as_ref(),
            &self.content_sha256,
            self.compiler_input_packet_sha256.as_deref(),
        )
    }

    /// Canonicalize the artifact's hash-relevant vectors **in place**
    /// (DDCP3-U0 (b); the shared helper producers call at construction):
    /// sort `required_features`, `entrypoints`, and the `reflection` rows
    /// (kernels + resources) so the stored packet carries canonical order
    /// and its `packet_sha256` is computed over the canonicalized inputs.
    /// Admission re-verifies over the canonical order regardless, so a
    /// producer that skips this helper still produces a valid packet —
    /// equivalent sets always hash to an identical packet identity.
    pub fn canonicalize_artifact(&mut self) {
        self.target.required_features.sort();
        self.entrypoints.sort_by(|left, right| {
            (left.entry.as_str(), left.symbol.as_str())
                .cmp(&(right.entry.as_str(), right.symbol.as_str()))
        });
        if let Some(reflection) = &mut self.reflection {
            reflection.kernels.sort_by(|left, right| {
                (left.function.as_str(), left.entry_name.as_str())
                    .cmp(&(right.function.as_str(), right.entry_name.as_str()))
            });
            for kernel in &mut reflection.kernels {
                kernel.resources.sort_by(|left, right| {
                    (left.binding, left.name.as_str()).cmp(&(right.binding, right.name.as_str()))
                });
            }
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
                    DevicePayloadEncodingError::Utf8CanonicalityMismatch {
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
            } => validate_binary_encoding(
                &self.bytes,
                encoding_tag,
                *decoded_byte_length,
                declared_base64,
            ),
        }
    }
}

/// Validate a binary payload declaration against its canonical decoded bytes
/// (contract §Payload encoding): non-empty encoding tag, a decoded length
/// equal to the carried bytes, and a declared base64 that is the canonical
/// unpadded base64 of those bytes.
fn validate_binary_encoding(
    bytes: &[u8],
    encoding_tag: &str,
    decoded_byte_length: u64,
    declared_base64: &str,
) -> Result<(), DevicePayloadEncodingError> {
    if encoding_tag.is_empty() {
        return Err(DevicePayloadEncodingError::EncodingTagEmpty);
    }
    if decoded_byte_length != bytes.len() as u64 {
        return Err(DevicePayloadEncodingError::DecodedByteLengthMismatch {
            declared: decoded_byte_length,
            actual: bytes.len(),
        });
    }
    let canonical = canonical_unpadded_base64(bytes);
    if canonical != declared_base64 {
        return Err(DevicePayloadEncodingError::Base64CanonicalityMismatch {
            detail: format!(
                "declared `{declared_base64}` is not the canonical unpadded base64 (`{canonical}`) of the decoded bytes"
            ),
        });
    }
    Ok(())
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
    /// AMD GPU via the AMDGPU/HIP variant of the LLVM emitter family
    /// (AMDGPU LLVM IR compiler input; `ROCm` clang → HSACO ELF — the third
    /// conformance leaf, goal `amd-gpu-target`). The backend-id spelling is
    /// the OQ1 default **`amd`** (goal open question Q1 — one decision,
    /// recorded; the seed's `amdgpu` id is aligned to it). The variant is
    /// ADDITIVE: the `Unknown` representable-backend path is unchanged, and
    /// any genuinely-unknown spelling (`Unknown("rocm")`, a future WebGPU
    /// id, …) round-trips exactly as before — an `amd` id now DECODES to
    /// this explicit variant (the same wire value as the seed, admitted as
    /// the compiled-in leaf) instead of `Unknown`.
    Amd,
    /// Any other backend id (e.g. a future WebGPU leaf). Representable;
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
            Self::Amd => "amd".to_owned(),
            Self::Unknown(id) => id.clone(),
        }
    }
}

/// The explicit artifact format id (DDCP2-U2 / contract §`ArtifactPacket`) —
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
/// §`ArtifactPacket`): `compiler-input` is the materializer's pre-lowering
/// text input; `finalized-binary` is the linked executable artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaterializationStage {
    /// The materializer's compiler input (pre-lowering text).
    CompilerInput,
    /// The linked final artifact the host loads.
    FinalizedBinary,
}

impl MaterializationStage {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::CompilerInput => "compiler-input",
            Self::FinalizedBinary => "finalized-binary",
        }
    }
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
    Utf8CanonicalityMismatch { detail: String },
    /// A text-encoded payload begins with a UTF-8 BOM.
    Utf8Bom { detail: String },
    /// A binary-encoded payload carries an empty encoding tag.
    EncodingTagEmpty,
    /// The declared decoded byte length disagrees with the artifact bytes.
    DecodedByteLengthMismatch { declared: u64, actual: usize },
    /// The declared base64 is not the canonical unpadded base64 of the bytes.
    Base64CanonicalityMismatch { detail: String },
}

impl std::fmt::Display for DevicePayloadEncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Utf8CanonicalityMismatch { detail } => {
                write!(f, "text payload is not canonical UTF-8: {detail}")
            }
            Self::Utf8Bom { detail } | Self::Base64CanonicalityMismatch { detail } => {
                write!(f, "{detail}")
            }
            Self::EncodingTagEmpty => {
                write!(f, "binary payload carries an empty encoding tag")
            }
            Self::DecodedByteLengthMismatch { declared, actual } => write!(
                f,
                "binary payload declares {declared} decoded bytes but the artifact carries {actual}"
            ),
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
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
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
/// image version. Version 2 carries the A4S semantic record; the old v1
/// layout is retained only as a legacy carrier and is never default-filled
/// into the v2 record.
pub const WIRE_SESSION_SECTION_VERSION: u32 = 2;

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
    /// The legacy v1 typed KV-cache layout carriage. A v2 semantic record is
    /// carried by [`FmirSessionSectionV2`] instead of being synthesized from
    /// this smaller layout.
    pub kv_layout: WireKvCacheLayout,
}

/// The v2 session-section carrier for the static A4S semantic record.
///
/// This is intentionally a separate shape from [`FmirSessionSection`]. The
/// v1 layout cannot be deserialized into this carrier by defaulting absent
/// semantic facts, and no v1 field is treated as a v2 runtime fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirSessionSectionV2 {
    /// The session section's own v2 ratchet.
    pub v: u32,
    /// Declared session/host-input slots with their per-invocation cadence.
    pub inputs: Vec<WireSessionInput>,
    /// The workload modes this session executes.
    pub invocation_modes: Vec<WireInvocationMode>,
    /// The declared observation cadence of the session's observation.
    pub observation_cadence: WireSessionObservationCadence,
    /// The complete static KV-cache semantic record from A4S.
    pub semantic: WireKvCacheSemanticV2,
}

impl FmirSessionSectionV2 {
    /// Wrap the v2 static semantic record without filling or deriving fields.
    #[must_use]
    pub fn from_semantic(
        inputs: Vec<WireSessionInput>,
        invocation_modes: Vec<WireInvocationMode>,
        observation_cadence: WireSessionObservationCadence,
        semantic: WireKvCacheSemanticV2,
    ) -> Self {
        Self {
            v: WIRE_SESSION_SECTION_VERSION,
            inputs,
            invocation_modes,
            observation_cadence,
            semantic,
        }
    }

    /// Return whether this carrier has the exact v2 section version. A
    /// mismatch is rejected rather than decoded through the legacy layout.
    #[must_use]
    pub const fn has_supported_version(&self) -> bool {
        self.v == WIRE_SESSION_SECTION_VERSION
    }
}
