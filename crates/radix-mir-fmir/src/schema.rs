//! FMIR portable package-image schema.
//!
//! Owns the wire structs, the artifact-version constant (clean-break ratchets;
//! no compatibility decoder — A7), the `fmir-text` / `fmir` target tags, the
//! FNV-1a hash used for source identities, runner fingerprints, and backend
//! artifact provenance, the SHA-256 same-package identity, and the host +
//! device runtime-requirement allowlists.
//!
//! # Device payload (N1.7 / architecture record §7)
//!
//! A `3 → 4` clean break adds the optional `device` section to
//! [`FmirTextImageFile`] / [`FmirBinaryImageFile`] for the differentiable-GPU
//! campaign. A `4 → 5` clean break (operator-authorized package-seam break,
//! S3-A4 / N3.3) replaces the section's opaque payload string with the
//! **typed complete device program** (kernels, launches, results,
//! per-resource access + version, lifetime) and moves CUDA symbols + host
//! input values to per-artifact / declared-inputs metadata, so the canonical
//! program bytes (the A10 identity substrate) are semantics-only. The section
//! carries the backend selection request, the declared backend artifacts
//! (Metal MSL + CUDA PTX) each with its own FNV-1a provenance hash, and the
//! device runtime requirements (`device:metal`, `device:cuda`). A device
//! payload is admitted only from the exact producing toolchain version, and
//! unknown `device:` requirements fail admission at package load.

use radix_mir::MirProgram;
use radix_types::TypeTableSnapshot;
use serde::{Deserialize, Serialize};

/// Image schema version. Ratchets as clean breaks; there is no compatibility
/// decoder (A7). `3 → 4` clean break adds the optional `device` section
/// (canonical device-program payload, selection, backend artifacts, device
/// runtime requirements) for the differentiable-GPU campaign (N1.7). `4 → 5`
/// clean break (operator memo; S3-A4) makes the device section a
/// TYPED complete program (the opaque payload string is replaced) and moves
/// CUDA symbols + host inputs to metadata. Old version-4 images fail closed
/// with [`UnsupportedVersion`](crate::error::FmirImageError::UnsupportedVersion).
pub const PACKAGE_MIR_ARTIFACT_VERSION: u32 = 5;

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
    /// First wire field in both formats. Both decoders gate on it before any
    /// payload interpretation: the text decoder checks it after the TOML
    /// parse and before the target/toolchain/device admission checks; the
    /// binary decoder pre-scans the first postcard varint so a version
    /// mismatch fails with
    /// [`UnsupportedVersion`](crate::error::FmirImageError::UnsupportedVersion)
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

/// postcard binary image schema. Field order is the serialized layout
/// (`version` first) and must not be reordered — a silent byte break.
#[derive(Debug, Serialize, Deserialize)]
pub struct FmirBinaryImageFile {
    /// First wire field (postcard varint). [`decode_binary_image`](crate::decode_binary_image)
    /// pre-scans this varint so a legacy-layout image (e.g. version 3
    /// without the device field, N1.7) fails with
    /// [`UnsupportedVersion`](crate::error::FmirImageError::UnsupportedVersion)
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
    /// [`is_known_host_requirement`] allowlist with `device:metal`,
    /// `device:cuda`. Unknown `device:` requirements fail admission at
    /// package load ([`check_device_requirements`]).
    pub runtime_requirements: Vec<String>,
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
    /// lockstep with [`PACKAGE_MIR_ARTIFACT_VERSION`]. Fail-closed admission:
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

// ---------------------------------------------------------------------------
// The typed complete device program (S3-A4 / N3.3)
// ---------------------------------------------------------------------------
//
// Mirrors the radix-mir `device_program::DeviceProgram` schema field-for-field
// (the S1-1 shared-schema authority) so the wire carries the COMPLETE program:
// kernels (function id, entry, plan, typed resources, launch), launches
// (ordered; may be >1 per kernel), the program lifetime, and the explicit
// result buffers. Per-resource access and the content version are distinct
// fields (the 4-axis separation, N3.4); data-flow edges derive
// deterministically from the complete access + launches (the source facts are
// on the wire), so the host never re-derives topology from launch order or a
// slot-role string.

/// The complete device program (wire form).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDeviceProgram {
    /// Ordered kernel declarations.
    pub kernels: Vec<WireKernelUnit>,
    /// Ordered launch sequence (a kernel may be launched more than once).
    pub launches: Vec<WireLaunchUnit>,
    /// Program execution-lifetime regime.
    pub lifetime: WireProgramLifetime,
    /// Explicit multi-output result buffers.
    pub results: Vec<WireResultBuffer>,
}

/// One ordered kernel declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireKernelUnit {
    /// MIR function id this kernel is lowered from.
    pub function: u32,
    /// Target-neutral logical entry name.
    pub entry: String,
    /// Shared algorithmic recipe (elementwise, matmul, reduction).
    pub plan: WireCollectionKernelPlan,
    /// Typed buffer slots bound by this kernel.
    pub resources: Vec<WireDeviceResource>,
    /// Workgroup / dispatch / launch plan.
    pub launch: WireKernelLaunchPlan,
}

/// A typed buffer slot bound to a kernel: identity + content version +
/// binding + slot access (the 4-axis separation keeps access a distinct
/// field from role/lifetime, N3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDeviceResource {
    /// The buffer's program-level identity facts.
    pub buffer: WireBufferIdentity,
    /// The content version (and shape) this slot binds.
    pub version: WireBufferVersion,
    /// Target-neutral binding indices.
    pub binding: WireBinding,
    /// Slot read/write access.
    pub access: WireResourceAccess,
}

/// Stable identity facts of one device buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBufferIdentity {
    /// Program-unique identity key.
    pub id: u32,
    /// Logical name (diagnostics; target-neutral — never a backend binding).
    pub name: String,
    /// Program-level role (input / output / in-out).
    pub role: WireBufferRole,
    /// Where the storage lives (`DeviceHandle` / `HostOwned`).
    pub storage: WireStorageLayout,
    /// How long the storage lives.
    pub lifetime: WireBufferLifetime,
}

/// One content version of a buffer (a shape snapshot; version ≥ 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBufferVersion {
    /// Monotonic version number within the buffer (≥ 1).
    pub version: u32,
    /// Element-type spelling of this version's shape (the campaign dtype
    /// surface; "f32" for this delivery).
    pub element_ty: String,
    /// Element count of this version's shape.
    pub element_count: u64,
}

/// Target-neutral binding indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBinding {
    /// First-level binding index.
    pub group: u32,
    /// Second-level binding index within the group.
    pub binding: u32,
}

/// The workgroup / dispatch / launch plan of one kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireKernelLaunchPlan {
    /// Threads per workgroup.
    pub workgroup: WireWorkgroupSize,
    /// Dispatch extent in workgroups for each axis.
    pub dispatch_size: WireDispatchSize,
    /// Derived workgroup count the host launches.
    pub workgroup_count: WireWorkgroupCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireWorkgroupSize {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDispatchSize {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireWorkgroupCount {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

/// One entry in the ordered launch sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLaunchUnit {
    /// Program-unique launch id.
    pub id: u32,
    /// Index into `kernels`.
    pub kernel_index: u32,
}

/// An explicit result buffer (multi-output readback).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireResultBuffer {
    /// Identity of the observed buffer.
    pub buffer: WireBufferIdentity,
    /// Observed content version and shape.
    pub version: WireBufferVersion,
    /// Role of the observation (output or in-out).
    pub role: WireBufferRole,
    /// The launch that produced the observed version.
    pub produced_by: u32,
}

/// Program-level buffer role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireBufferRole {
    /// Host-provided input; every slot accesses it read-only.
    Input,
    /// Device-produced result; read back at an observation point.
    Output,
    /// Device-resident intermediate shared across kernels.
    InOut,
}

impl WireBufferRole {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::InOut => "in-out",
        }
    }
}

/// Storage-lifetime class (the frozen three-class enum, N3.4 — no fourth
/// class by convenience).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireBufferLifetime {
    /// Allocated once for the whole program; persists across steps.
    PerProgram,
    /// Live within one training step; recycled between iterations.
    PerStep,
    /// Read back at a declared observation point.
    ObservationPoint,
}

impl WireBufferLifetime {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::PerProgram => "per-program",
            Self::PerStep => "per-step",
            Self::ObservationPoint => "observation-point",
        }
    }
}

/// Slot read/write access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireResourceAccess {
    Read,
    Write,
    ReadWrite,
}

/// Where the storage lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireStorageLayout {
    HostOwned,
    DeviceHandle,
}

/// Program execution-lifetime regime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireProgramLifetime {
    /// One-shot program run.
    #[default]
    SingleRun,
    /// Repeating training step; per-step buffers recycle between iterations.
    RepeatingStep,
}

/// The shared algorithmic recipe set (mirror of `CollectionKernelPlan`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum WireCollectionKernelPlan {
    /// No collection-op recipe (elementwise kernel).
    Elementwise,
    /// Tiled-matmul recipe.
    TiledMatMul(WireMatMulPlan),
    /// Sequential tree-reduction recipe.
    TreeReduction(WireReductionPlan),
}

/// Tiled-matmul recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireMatMulPlan {
    /// Left matrix rows `M`.
    pub m: u64,
    /// Contracted inner dimension `K`.
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
    /// Square tile edge.
    pub tile: u32,
    /// Workgroup lane count on `x`.
    pub workgroup_x: u32,
    /// Workgroup lane count on `y`.
    pub workgroup_y: u32,
    /// The two shared-memory operand tiles.
    pub shared_memory: WireMatMulSharedMemory,
    /// Per-K-tile barrier schedule.
    pub barriers: Vec<WireBarrierPoint>,
    /// Zero-fill policy for the operand tail tiles.
    pub oob_padding: WireOobPaddingPolicy,
}

/// The two shared-memory operand tiles of the tiled-matmul recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireMatMulSharedMemory {
    /// Left-matrix tile.
    pub shared_a: WireSharedMemoryLayout,
    /// Right-matrix tile.
    pub shared_b: WireSharedMemoryLayout,
}

/// Tree-reduction recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireReductionPlan {
    /// Sum or mean (mean divides by the true tensor length).
    pub op: WireReduceOp,
    /// True flat tensor length `n`.
    pub length: u64,
    /// Power-of-two workgroup lane count.
    pub workgroup_x: u32,
    /// Per-workgroup partial slot count.
    pub partials: u64,
    /// Shared-memory buffer the tree reduces through.
    pub shared_memory: WireSharedMemoryLayout,
    /// Ordered barrier schedule.
    pub barriers: Vec<WireBarrierPoint>,
    /// Zero-fill policy for tail elements beyond `length`.
    pub oob_padding: WireOobPaddingPolicy,
}

/// One shared-memory buffer layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSharedMemoryLayout {
    /// Byte width of one element.
    pub element_byte_width: u32,
    /// Number of slots.
    pub slot_count: u32,
    /// Logical buffer name (backends map to target syntax).
    pub buffer_name: String,
}

/// A barrier program point in a recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBarrierPoint {
    /// The phase whose writes this barrier publishes.
    pub after: WireBarrierPhase,
}

/// What shared-memory writes a barrier publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireBarrierPhase {
    SharedMemoryLoad,
    ReductionStep,
    InnerProductStep,
}

/// Out-of-bounds padding policy for collection-op recipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireOobPaddingPolicy {
    ZeroFill,
}

/// Reduction combination operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireReduceOp {
    Sum,
    Mean,
}

/// Deterministic canonical bytes of the COMPLETE device program — the A10
/// identity substrate. Both image routes carry the identical typed program
/// (Metal and CUDA images derive the same `WireDeviceProgram`), so both
/// derive identical bytes. Postcard serialization is deterministic for a
/// fixed struct shape and infallible for these scalar/string/enum fields.
#[must_use]
pub fn canonical_program_bytes(program: &WireDeviceProgram) -> Vec<u8> {
    postcard::to_allocvec(program).expect("wire program serialization is infallible")
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
    /// spelling (computed by [`fnv1a64_blob_hash`]). Verified on admission.
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

/// FNV-1a 64-bit per-blob provenance hash for a backend artifact (A9). Returns
/// the canonical `fnv64:<16-hex>` spelling used by source identities, so a
/// stored artifact hash is directly comparable across the text and binary
/// images. Reuses [`fnv1a64`]; backend blobs carry their own provenance and are
/// excluded from the same-package identity (§3.4).
#[must_use]
pub fn fnv1a64_blob_hash(blob: &[u8]) -> String {
    format!("fnv64:{:016x}", fnv1a64(blob))
}

/// SHA-256 same-package identity (A10 / §3.4): over (a) the source content
/// hashes and (b) the canonical backend-neutral device-program payload bytes.
/// Backend blobs are excluded (each carries its own FNV-1a provenance via
/// [`fnv1a64_blob_hash`]). Metal and CUDA routes derive identical semantic
/// bytes and therefore an identical identity hash.
///
/// # Canonicalization
///
/// The digest is taken over a length-prefixed, unambiguous byte stream so
/// distinct inputs never collide:
///
/// - for each source hash string (in image order): little-endian `u64` byte
///   length, then the UTF-8 bytes of the hash string;
/// - then the little-endian `u64` byte length of the canonical payload,
///   followed by the canonical payload bytes.
///
/// Returns the canonical `sha256:<64-hex>` spelling.
#[must_use]
pub fn device_identity_hash(source_hashes: &[&str], canonical_payload: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for source_hash in source_hashes {
        let bytes = source_hash.as_bytes();
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hasher.update((canonical_payload.len() as u64).to_le_bytes());
    hasher.update(canonical_payload);
    let digest = hasher.finalize();
    format!("sha256:{:x}", digest)
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

/// Whether a runtime requirement is a known `device:*` capability (N1.7).
/// `device:metal` and `device:cuda` are the frozen device requirements for the
/// differentiable-GPU campaign.
pub fn is_known_device_requirement(requirement: &str) -> bool {
    matches!(requirement, "device:metal" | "device:cuda")
}

/// Whether a runtime requirement is a known admitted capability (host **or**
/// device). This is the package-load admission allowlist: the
/// [`is_known_host_requirement`] allowlist extended with `device:metal` /
/// `device:cuda` (N1.7). A requirement that is neither a known `host:*` nor a
/// known `device:*` fails admission.
pub fn is_known_requirement(requirement: &str) -> bool {
    is_known_host_requirement(requirement) || is_known_device_requirement(requirement)
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

/// Fail-closed device runtime-requirement admission (N1.7): every requirement
/// in the device section must be a known `device:` capability
/// ([`is_known_device_requirement`]). Host requirements belong in the top-level
/// `runtime` section; an unknown `device:` (or misplaced `host:`) requirement
/// fails admission at package load.
pub(crate) fn check_device_requirements(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    for requirement in &device.runtime_requirements {
        if !is_known_device_requirement(requirement) {
            return Err(
                crate::error::FmirImageError::UnsupportedRuntimeRequirement {
                    requirement: requirement.clone(),
                },
            );
        }
    }
    Ok(())
}

/// Fail-closed backend-artifact admission (A9): each artifact's stored FNV-1a
/// hash must match its blob. Provenance is verified, not decorative — a
/// tampered or miscomputed hash fails admission at package load.
pub(crate) fn check_artifact_hashes(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    for artifact in &device.artifacts.artifact {
        let actual = fnv1a64_blob_hash(artifact.blob.as_bytes());
        if actual != artifact.hash {
            return Err(crate::error::FmirImageError::ArtifactHashMismatch {
                backend: artifact.backend,
                expected: artifact.hash.clone(),
                actual,
            });
        }
    }
    Ok(())
}

/// Combined device-section admission. Run after the version / target /
/// toolchain gates (a device payload is admitted only from the exact producing
/// toolchain version — N1.7).
pub(crate) fn admit_device_section(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    check_device_requirements(device)?;
    check_artifact_hashes(device)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_version_is_5_for_typed_device_program_schema() {
        assert_eq!(PACKAGE_MIR_ARTIFACT_VERSION, 5);
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

    #[test]
    fn device_requirements_allowlist_extends_host_allowlist() {
        // Frozen device requirements (N1.7).
        assert!(is_known_device_requirement("device:metal"));
        assert!(is_known_device_requirement("device:cuda"));
        assert!(!is_known_device_requirement("device:vulkan"));
        assert!(!is_known_device_requirement("device:"));
        assert!(!is_known_device_requirement("host:fs"));
        // Combined admission allowlist = host ∪ device.
        assert!(is_known_requirement("host:argv"));
        assert!(is_known_requirement("device:metal"));
        assert!(is_known_requirement("device:cuda"));
        assert!(!is_known_requirement("device:vulkan"));
        assert!(!is_known_requirement("kernel:foo.bar"));
    }

    #[test]
    fn fnv1a64_blob_hash_is_canonical_spelling() {
        // Empty blob → FNV offset basis, zero-padded to 16 hex.
        assert_eq!(fnv1a64_blob_hash(b""), "fnv64:cbf29ce484222325");
        // Single-byte "a" matches the reference vector in fnv1a64.
        assert_eq!(fnv1a64_blob_hash(b"a"), "fnv64:af63dc4c8601ec8c");
    }
}
