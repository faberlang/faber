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
    /// single-device packages (MD-A15) — wire-7 single-device images keep
    /// the independent single-device path (the A10 identity and the nested
    /// `WIRE_DEVICE_PROGRAM_VERSION = 7` bytes are untouched). `Some` for
    /// distributed images: the section carries its OWN version ratchet
    /// ([`crate::schema::WIRE_DISTRIBUTED_SECTION_VERSION`]) and is admitted
    /// fail-closed at load; the single-device run route rejects it
    /// ("distributed execution requires MD3") until MD3. Old consumers
    /// reject the new field via `deny_unknown_fields` (fail-closed, A7).
    pub distributed: Option<FmirDistributedExecutionSection>,
    /// The optional cadence/session section (GI4-2). `None` for packages
    /// without an inference-session surface — single-device packages do not
    /// require the section (the MD-A15 precedent; the wire-7 single-device
    /// path, the A10 identity, and the nested
    /// `WIRE_DEVICE_PROGRAM_VERSION = 7` bytes are untouched). `Some` for
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

/// One logical-entry → backend-symbol identity for the CUDA artifact
/// (per-artifact metadata, NOT a program semantic). Metal artifacts carry no
/// mapping: the logical entry IS the launch name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceSymbol {
    /// Logical entry (a kernel-unit fact of the program).
    pub entry: String,
    /// Emitted NVVM/PTX `.entry` symbol the host launches by.
    pub symbol: String,
}
/// Backend selection request recorded in the image (N1.1). Fail-closed: an
/// unknown value fails at parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FmirDeviceSelection {
    /// Automatic backend resolution at run time.
    Auto,
    /// Apple Metal.
    Metal,
    /// NVIDIA CUDA.
    Cuda,
}

/// Declared backend artifacts list (N1.7 / §3.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceArtifactsSection {
    pub artifact: Vec<FmirDeviceArtifact>,
}

/// One declared backend artifact: a backend blob (MSL source for Metal, PTX
/// text for CUDA) plus its FNV-1a provenance hash. Both packaged artifacts are
/// text; there is no binary blob in Stages 0–6 (N1.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDeviceArtifact {
    /// Backend the blob targets. `auto` is not a backend.
    pub backend: FmirDeviceBackend,
    /// Backend blob text (MSL for `metal`, PTX for `cuda`).
    pub blob: String,
    /// FNV-1a 64-bit provenance hash of `blob`, canonical `fnv64:<16-hex>`
    /// spelling (computed by [`crate::schema::fnv1a64_blob_hash`]). Verified on admission.
    pub hash: String,
    /// Per-artifact logical-entry → symbol mapping (N3.3 / S3-A4): the CUDA
    /// artifact's `.entry` symbol identities the host launches by. Empty for
    /// Metal (the logical entry IS the launch name). This is artifact
    /// metadata, NOT program semantics — absent from the canonical program
    /// bytes.
    pub symbols: Vec<FmirDeviceSymbol>,
}

/// Backend a declared artifact targets (N1.3). `auto` is a selection, not a
/// backend. Fail-closed: an unknown value fails at parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FmirDeviceBackend {
    /// Apple Metal (MSL source).
    Metal,
    /// NVIDIA CUDA (PTX text).
    Cuda,
}

/// The version ratchet of the optional cadence/session section (GI4-2; CTO
/// S12). The section carries its OWN nested version, admitted fail-closed
/// **before field interpretation** (the `admit_device_section` pattern, FC4)
/// — independently of the nested device-program wire version and the outer
/// image version. The first session-section shape is v1; the nested
/// `WIRE_DEVICE_PROGRAM_VERSION = 7` and the outer
/// `PACKAGE_MIR_ARTIFACT_VERSION = 6` stay untouched (the MD2-W1
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
