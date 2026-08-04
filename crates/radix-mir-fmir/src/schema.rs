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
//!
//! A `5 → 6` clean break (Stage 3R U1, the F1–F7 frozen contract) carries the
//! repaired semantic facts on the wire: a **semantic value identity** per
//! buffer (F1), explicit **value generations** per write slot (F2), carried
//! **roots + producer/consumer dependencies** (F3), the lossless
//! **primal/companion relation** (F4), and independent **initialization +
//! observation axes** (F5/F6). The nested device-program wire version
//! ([`WIRE_DEVICE_PROGRAM_VERSION`]) and the outer image version
//! ([`PACKAGE_MIR_ARTIFACT_VERSION`]) fail closed **independently** at the
//! radix decode boundary (F7/S8), before host construction or launch.

use radix_mir::MirProgram;
use radix_types::TypeTableSnapshot;
use serde::{Deserialize, Serialize};

/// Image schema version. Ratchets as clean breaks; there is no compatibility
/// decoder (A7). `3 → 4` clean break adds the optional `device` section
/// (canonical device-program payload, selection, backend artifacts, device
/// runtime requirements) for the differentiable-GPU campaign (N1.7). `4 → 5`
/// clean break (operator memo; S3-A4) makes the device section a
/// TYPED complete program (the opaque payload string is replaced) and moves
/// CUDA symbols + host inputs to metadata. `5 → 6` clean break (Stage 3R U1)
/// carries the F1–F7 semantic facts (semantic value identity, generations,
/// roots, dependencies, lossless relation, initialization/observation axes).
/// Old version-5 images fail closed with
/// [`UnsupportedVersion`](crate::error::FmirImageError::UnsupportedVersion).
pub const PACKAGE_MIR_ARTIFACT_VERSION: u32 = 6;

/// The faber-owned wire/codec version of the typed device-program section
/// (the nested `v` field of [`FmirDeviceProgramSection`]). Bumped in lockstep
/// with the `DEVICE_RUN_PLAN_VERSION` constructor ratchet; bumped `3 → 4` by
/// the Stage 3R U1 clean break (F7/S8). Admitted independently of the outer
/// image version: [`admit_device_section`] rejects a mismatched nested
/// version before any field-level interpretation, with a focused diagnostic
/// ([`UnsupportedDeviceProgramVersion`](crate::error::FmirImageError::UnsupportedDeviceProgramVersion)).
pub const WIRE_DEVICE_PROGRAM_VERSION: u32 = 4;

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
//
// Stage 3R (U1, S3–S7) extends the typed program with the F1–F7 carried
// facts: a semantic-value table + per-buffer identity (F1), explicit value
// generations per slot (F2), carried roots + dependency edges (F3), the
// lossless companion relation (F4), and the initialization / observation axes
// (F5/F6). `deny_unknown_fields` admission means an old consumer never
// silently ignores these sections.

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
    /// Carried semantic-value table (F1): one row per stable value identity.
    pub semantic_values: Vec<WireSemanticValue>,
    /// Declared legal execution roots — launch ids the host may start from
    /// (F3). Never inferred from kernel declaration order.
    pub roots: Vec<u32>,
    /// Carried producer/consumer dependency edges (F3).
    pub dependencies: Vec<WireDependencyEdge>,
    /// Lossless primal/companion relation rows (F4/S7).
    pub relations: Vec<WireCompanionRelation>,
}

/// One carried semantic value (F1): the stable identity plus the origin fact
/// it is derived from. Two unrelated same-name/same-shape values are distinct
/// rows; two rows minted from one origin would alias and fail admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSemanticValue {
    /// Program-unique stable identity.
    pub id: u32,
    /// Diagnostic name (never an identity fact).
    pub name: String,
    /// The carried fact this identity is derived from.
    pub origin: WireSemanticValueOrigin,
}

/// The carried MIR/value fact a [`WireSemanticValue`] identity derives from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireSemanticValueOrigin {
    /// The value flows from a specific MIR local of a specific function.
    MirLocal { function: u32, local: u32 },
    /// A host-provided input value.
    HostInput,
    /// A synthetic value minted by the materializer (e.g. a gradient slot);
    /// the label discriminates distinct synthetic values.
    Synthetic { label: String },
}

/// One carried producer/consumer dependency edge (F3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDependencyEdge {
    /// The launch that produces the value consumed downstream.
    pub producer: u32,
    /// The launch that consumes it.
    pub consumer: u32,
    /// The buffer whose content flows across the edge.
    pub buffer: u32,
    /// The content version that flows.
    pub version: u32,
}

/// One lossless primal/companion relation row (F4/S7): selected inputs with
/// the gradient-to-primal identity, selected outputs, and device-residency.
/// Duplicate or contradictory rows fail admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCompanionRelation {
    /// Primal function id.
    pub primal: u32,
    /// Companion function id.
    pub companion: u32,
    /// Derivative kind.
    pub derivative: WireCompanionDerivativeKind,
    /// The primal is an explicitly device-resident forward kernel.
    pub device_resident: bool,
    /// The selected differentiable inputs.
    pub selected_inputs: Vec<WireCompanionSelectedInput>,
    /// The selected primal outputs.
    pub selected_outputs: Vec<WireCompanionSelectedOutput>,
}

/// The derivative kind a companion computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireCompanionDerivativeKind {
    /// Reverse-mode vector-Jacobian product.
    ReverseModeVjp,
}

/// One selected differentiable input (F4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCompanionSelectedInput {
    /// The primal parameter this input is.
    pub param: u32,
    /// Parameter position in the primal signature.
    pub position: u32,
    /// The primal parameter's type id.
    pub ty: u32,
    /// The companion result-tuple slot carrying this input's gradient.
    pub gradient_slot: u32,
}

/// One selected primal output (F4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCompanionSelectedOutput {
    /// Output position in the primal result tuple.
    pub position: u32,
    /// The output's type id.
    pub ty: u32,
    /// The upstream gradient's type id.
    pub upstream_gradient_ty: u32,
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
/// field from role/lifetime, N3.4), plus the F2 generation this slot
/// consumes/produces and the F5 initialization policy that governs the
/// buffer's storage.
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
    /// Explicit value generation (F2): a write slot produces this generation;
    /// a read slot consumes it; a read-write slot produces it and consumes
    /// the previous one. Generation 0 (unset) is rejected.
    pub generation: u32,
    /// Independent initialization axis (F5): how the buffer's storage is
    /// brought to its first defined state — never derived from role/lifetime.
    pub initialization: WireInitializationPolicy,
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
    /// The stable semantic value identity this buffer holds (F1).
    pub semantic_value: u32,
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
    /// The explicit observation fact (F5/F6): this row is a DECLARED
    /// observation point. A writable intermediate or persistent state is a
    /// result only through this fact — never because it is writable.
    pub observation: WireObservationFact,
}

/// The explicit observation fact of a result row (F5/F6): the value is read
/// back at a declared completion boundary, decoupled from role and lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireObservationFact {
    /// The launch whose completion boundary makes the observation valid.
    pub at_launch: u32,
}

/// Independent initialization axis (F5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireInitializationPolicy {
    /// Storage is zero-filled at allocation.
    ZeroFill,
    /// Storage is uploaded from host-provided values.
    HostProvided,
    /// Storage is fully written by a device kernel before any read.
    KernelInitialized,
}

impl WireInitializationPolicy {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::ZeroFill => "zero-fill",
            Self::HostProvided => "host-provided",
            Self::KernelInitialized => "kernel-initialized",
        }
    }
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
    /// Rank-2 transpose recipe (S5-U1).
    Transpose(WireTransposePlan),
}

/// Rank-2 transpose recipe (`[M,N] → [N,M]`), the wire mirror of
/// `kernel_plan::TransposePlan` (S5-U1): one thread per output element with
/// a column-major index remap, no shared memory, no barriers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireTransposePlan {
    /// Source rows `M` (output columns).
    pub m: u64,
    /// Source columns `N` (output rows).
    pub n: u64,
    /// Workgroup lane count on `x` (the elementwise-style 1D contract).
    pub workgroup_x: u32,
    /// Dispatch extent on `x` (`M·N` output elements).
    pub dispatch_x: u64,
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
///
/// Order (Stage 3R F7/S8): the **nested** device-program wire version is
/// admitted independently of the outer image version and before any field
/// interpretation; then the carried F1–F7 semantic facts are validated
/// fail-closed; then the runtime requirements and artifact provenance.
pub(crate) fn admit_device_section(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    // S8: the nested device-program version fails closed independently of the
    // outer FMIR version, with a focused diagnostic.
    if device.device_program.v != WIRE_DEVICE_PROGRAM_VERSION {
        return Err(
            crate::error::FmirImageError::UnsupportedDeviceProgramVersion {
                actual: device.device_program.v,
                expected: WIRE_DEVICE_PROGRAM_VERSION,
            },
        );
    }
    validate_wire_program(&device.device_program.program)?;
    check_device_requirements(device)?;
    check_artifact_hashes(device)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire-level semantic admission (Stage 3R F1–F7)
// ---------------------------------------------------------------------------
//
// The radix-mir `DeviceSemantics` rules, run on the serialized wire form at
// the decode boundary: semantic identity (F1), generations (F2), roots +
// dependencies (F3), the lossless relation (F4), and the independent
// initialization/observation axes (F5/F6). Fail-closed: the first violation
// fails admission before host construction or launch.

/// Validate the carried wire facts fail-closed (F1–F7).
pub(crate) fn validate_wire_program(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    let launches: Vec<u32> = wire.launches.iter().map(|launch| launch.id).collect();
    validate_wire_semantic_values(wire)?;
    validate_wire_generations(wire)?;
    validate_wire_graph(wire, &launches)?;
    validate_wire_relations(wire)?;
    validate_wire_axes(wire, &launches)?;
    Ok(())
}

/// Rule F1: unique semantic-value identities; never two origins for one id,
/// never two ids for one origin; every buffer reference names a declared
/// value; one buffer holds one value.
fn validate_wire_semantic_values(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    for left in &wire.semantic_values {
        for right in &wire.semantic_values {
            if left.id == right.id && left != right {
                return Err(wire_invalid(format!(
                    "semantic value id {} is declared with conflicting facts ({} vs {})",
                    left.id, left.name, right.name
                )));
            }
            if left.id != right.id && left.origin == right.origin {
                return Err(wire_invalid(format!(
                    "semantic values {} and {} both derive from origin {left:?}; distinct values must derive from distinct carried facts",
                    left.id, right.id
                )));
            }
        }
    }
    for buffer in wire_buffers(wire) {
        let value = buffer.semantic_value;
        if !wire.semantic_values.iter().any(|row| row.id == value) {
            return Err(wire_invalid(format!(
                "buffer {} references semantic value {value}, which is never declared",
                buffer.id
            )));
        }
    }
    for left in wire_buffers(wire) {
        for right in wire_buffers(wire) {
            if left.id == right.id && left.semantic_value != right.semantic_value {
                return Err(wire_invalid(format!(
                    "buffer {} holds semantic values {} and {}; one buffer holds one value",
                    left.id, left.semantic_value, right.semantic_value
                )));
            }
        }
    }
    Ok(())
}

/// Rule F2: every slot carries an explicit generation (0 is the reserved
/// unset sentinel); a (value, generation) has exactly one producer; every
/// generation a slot consumes is produced elsewhere or covered by an
/// initialization fact (complete inputs).
fn validate_wire_generations(wire: &WireDeviceProgram) -> Result<(), crate::error::FmirImageError> {
    for kernel in &wire.kernels {
        for resource in &kernel.resources {
            if resource.generation == 0 {
                return Err(wire_invalid(format!(
                    "a slot on buffer {} carries generation 0 (the reserved unset sentinel)",
                    resource.buffer.id
                )));
            }
        }
    }
    // Single definition per value generation: two DISTINCT write/read-write
    // slots may not claim the same (value, generation).
    let producers: Vec<(u32, u32, usize, usize)> = wire
        .kernels
        .iter()
        .enumerate()
        .flat_map(|(kernel_index, kernel)| {
            kernel
                .resources
                .iter()
                .enumerate()
                .filter_map(move |(slot_index, resource)| {
                    if matches!(
                        resource.access,
                        WireResourceAccess::Write | WireResourceAccess::ReadWrite
                    ) {
                        Some((
                            resource.buffer.semantic_value,
                            resource.generation,
                            kernel_index,
                            slot_index,
                        ))
                    } else {
                        None
                    }
                })
        })
        .collect();
    for (index, (value, generation, kernel, slot)) in producers.iter().enumerate() {
        for (other_index, (other_value, other_generation, other_kernel, other_slot)) in
            producers.iter().enumerate()
        {
            if index < other_index && value == other_value && generation == other_generation {
                return Err(wire_invalid(format!(
                    "semantic value {value} generation {generation} has a second producer (kernels {kernel}/{other_kernel}, slots {slot}/{other_slot}); a later write is a new generation"
                )));
            }
        }
    }
    // Complete inputs: a read consumes its generation; a read-write consumes
    // the previous generation and produces its own. Every consumed generation
    // is produced by a DIFFERENT slot, or is the buffer's initial state
    // covered by its initialization policy.
    for (kernel_index, kernel) in wire.kernels.iter().enumerate() {
        for (slot_index, resource) in kernel.resources.iter().enumerate() {
            let value = resource.buffer.semantic_value;
            let consumed: Option<u32> = match resource.access {
                WireResourceAccess::Read => Some(resource.generation),
                WireResourceAccess::ReadWrite => Some(resource.generation - 1),
                WireResourceAccess::Write => None,
            };
            let Some(consumed) = consumed else {
                continue;
            };
            let produced_elsewhere = wire
                .kernels
                .iter()
                .enumerate()
                .flat_map(|(other_kernel, other_unit)| {
                    other_unit.resources.iter().enumerate().filter_map(
                        move |(other_slot, other)| {
                            if (other_kernel != kernel_index || other_slot != slot_index)
                                && other.buffer.semantic_value == value
                                && other.generation == consumed
                                && matches!(
                                    other.access,
                                    WireResourceAccess::Write | WireResourceAccess::ReadWrite
                                )
                            {
                                Some(())
                            } else {
                                None
                            }
                        },
                    )
                })
                .next()
                .is_some();
            if produced_elsewhere {
                continue;
            }
            let init_defined = consumed <= 1
                && wire_buffer_init_policy(wire, resource.buffer.id).is_some_and(|policy| {
                    matches!(
                        policy,
                        WireInitializationPolicy::HostProvided | WireInitializationPolicy::ZeroFill
                    )
                });
            if !init_defined {
                return Err(wire_invalid(format!(
                    "buffer {} reads semantic value {value} generation {consumed}, which no launch produces and no initialization defines (incomplete input)",
                    resource.buffer.id
                )));
            }
        }
    }
    Ok(())
}

/// Rule F3: legal, unique, non-empty roots; well-formed, duplicate-free,
/// acyclic dependencies; every generation producer reachable from a root.
fn validate_wire_graph(
    wire: &WireDeviceProgram,
    launches: &[u32],
) -> Result<(), crate::error::FmirImageError> {
    if wire.roots.is_empty() && !wire.launches.is_empty() {
        return Err(wire_invalid(
            "the program declares launches but no execution root (F3: roots are carried, never inferred)"
                .to_owned(),
        ));
    }
    for root in &wire.roots {
        if !launches.contains(root) {
            return Err(wire_invalid(format!(
                "root {root} is not a declared launch"
            )));
        }
        if wire
            .roots
            .iter()
            .filter(|candidate| *candidate == root)
            .count()
            > 1
        {
            return Err(wire_invalid(format!(
                "root {root} is declared more than once"
            )));
        }
    }
    for edge in &wire.dependencies {
        if edge.producer == edge.consumer {
            return Err(wire_invalid(format!(
                "dependency edge runs from launch {} to itself",
                edge.producer
            )));
        }
        if !launches.contains(&edge.producer) {
            return Err(wire_invalid(format!(
                "dependency edge names producer launch {}, which does not exist",
                edge.producer
            )));
        }
        if !launches.contains(&edge.consumer) {
            return Err(wire_invalid(format!(
                "dependency edge names consumer launch {}, which does not exist",
                edge.consumer
            )));
        }
        if wire
            .dependencies
            .iter()
            .filter(|candidate| {
                candidate.producer == edge.producer
                    && candidate.consumer == edge.consumer
                    && candidate.buffer == edge.buffer
                    && candidate.version == edge.version
            })
            .count()
            > 1
        {
            return Err(wire_invalid(format!(
                "dependency edge {} → {} is declared more than once",
                edge.producer, edge.consumer
            )));
        }
    }
    reject_wire_dependency_cycles(wire)?;
    // Missing dependency: every generation producer must be reachable from a
    // root; a producer hanging off the graph could never be scheduled.
    let producers: Vec<u32> = wire
        .launches
        .iter()
        .filter(|launch| {
            wire.kernels
                .get(launch.kernel_index as usize)
                .is_some_and(|kernel| {
                    kernel.resources.iter().any(|resource| {
                        matches!(
                            resource.access,
                            WireResourceAccess::Write | WireResourceAccess::ReadWrite
                        )
                    })
                })
        })
        .map(|launch| launch.id)
        .collect();
    for producer in &producers {
        if !wire_reachable_from_root(wire, *producer) {
            return Err(wire_invalid(format!(
                "launch {producer} produces a value but is not reachable from any root (missing dependency)"
            )));
        }
    }
    Ok(())
}

/// Whether a launch is reachable from a declared root via the dependency
/// edges.
fn wire_reachable_from_root(wire: &WireDeviceProgram, launch: u32) -> bool {
    let mut seen = wire.roots.clone();
    let mut frontier = wire.roots.clone();
    while let Some(current) = frontier.pop() {
        if current == launch {
            return true;
        }
        for edge in &wire.dependencies {
            if edge.producer == current && !seen.contains(&edge.consumer) {
                seen.push(edge.consumer);
                frontier.push(edge.consumer);
            }
        }
    }
    false
}

/// Reject a dependency cycle (F3) — depth-first back-edge detection.
fn reject_wire_dependency_cycles(
    wire: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    let mut visited: Vec<u32> = Vec::new();
    let mut on_stack: Vec<u32> = Vec::new();
    for edge in &wire.dependencies {
        if wire_cycle_through(wire, edge.producer, &mut visited, &mut on_stack) {
            return Err(wire_invalid(format!(
                "dependency graph contains a cycle through launch {}",
                edge.producer
            )));
        }
    }
    Ok(())
}

fn wire_cycle_through(
    wire: &WireDeviceProgram,
    launch: u32,
    visited: &mut Vec<u32>,
    on_stack: &mut Vec<u32>,
) -> bool {
    if on_stack.contains(&launch) {
        return true;
    }
    if visited.contains(&launch) {
        return false;
    }
    visited.push(launch);
    on_stack.push(launch);
    for edge in &wire.dependencies {
        if edge.producer == launch && wire_cycle_through(wire, edge.consumer, visited, on_stack) {
            return true;
        }
    }
    on_stack.retain(|seen| *seen != launch);
    false
}

/// Rule F4: relation rows are unique per primal and internally well-formed;
/// duplicate or contradictory rows fail admission.
fn validate_wire_relations(wire: &WireDeviceProgram) -> Result<(), crate::error::FmirImageError> {
    for relation in &wire.relations {
        if relation.primal == relation.companion {
            return Err(wire_invalid(format!(
                "relation row for primal {} declares the same function as companion",
                relation.primal
            )));
        }
        let mut positions: Vec<u32> = Vec::new();
        for selected in &relation.selected_inputs {
            if positions.contains(&selected.position) {
                return Err(wire_invalid(format!(
                    "relation row for primal {} repeats selected-input position {}",
                    relation.primal, selected.position
                )));
            }
            positions.push(selected.position);
        }
        let mut output_positions: Vec<u32> = Vec::new();
        for selected in &relation.selected_outputs {
            if output_positions.contains(&selected.position) {
                return Err(wire_invalid(format!(
                    "relation row for primal {} repeats selected-output position {}",
                    relation.primal, selected.position
                )));
            }
            output_positions.push(selected.position);
        }
    }
    for relation in &wire.relations {
        if wire
            .relations
            .iter()
            .filter(|candidate| candidate.primal == relation.primal)
            .count()
            > 1
        {
            return Err(wire_invalid(format!(
                "relation rows for primal {} are duplicate or contradictory",
                relation.primal
            )));
        }
    }
    Ok(())
}

/// Rule F5/F6: initialization facts agree per buffer (independent axis);
/// every observation names a real launch; results are declared observation
/// points — every result row carries an explicit observation fact whose
/// boundary is the producing launch (a result whose observation contradicts
/// its producer is "without an explicit observation fact" and fails closed).
fn validate_wire_axes(
    wire: &WireDeviceProgram,
    launches: &[u32],
) -> Result<(), crate::error::FmirImageError> {
    // Init policies per buffer must agree (F5: the axis is a single fact,
    // never derived from role/lifetime).
    for resource in wire
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
    {
        let policy = resource.initialization;
        if wire_buffer_init_policy(wire, resource.buffer.id).is_some_and(|first| first != policy) {
            return Err(wire_invalid(format!(
                "buffer {} declares conflicting initialization policies",
                resource.buffer.id
            )));
        }
    }
    for result in &wire.results {
        if !launches.contains(&result.observation.at_launch) {
            return Err(wire_invalid(format!(
                "result on buffer {} names observation launch {}, which does not exist",
                result.buffer.id, result.observation.at_launch
            )));
        }
        // F6: the observation fact is the result's explicit declaration —
        // its completion boundary must be the producing launch. A result
        // whose observation contradicts its producer is a writable
        // intermediate exposed without an explicit observation fact.
        if result.observation.at_launch != result.produced_by {
            return Err(wire_invalid(format!(
                "result on buffer {} names producing launch {}, but its observation fact names launch {}; a result is a declared observation point at its producing launch",
                result.buffer.id, result.produced_by, result.observation.at_launch
            )));
        }
    }
    Ok(())
}

/// The distinct buffers referenced by the program, in first-reference order.
fn wire_buffers(wire: &WireDeviceProgram) -> Vec<WireBufferIdentity> {
    let mut buffers: Vec<WireBufferIdentity> = Vec::new();
    for kernel in &wire.kernels {
        for resource in &kernel.resources {
            if !buffers.iter().any(|buffer| buffer.id == resource.buffer.id) {
                buffers.push(resource.buffer.clone());
            }
        }
    }
    for result in &wire.results {
        if !buffers.iter().any(|buffer| buffer.id == result.buffer.id) {
            buffers.push(result.buffer.clone());
        }
    }
    buffers
}

/// The first initialization policy declared for a buffer, if any.
fn wire_buffer_init_policy(
    wire: &WireDeviceProgram,
    buffer: u32,
) -> Option<WireInitializationPolicy> {
    wire.kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .find(|resource| resource.buffer.id == buffer)
        .map(|resource| resource.initialization)
}

/// A focused `WireProgramInvalid` admission error.
fn wire_invalid(detail: String) -> crate::error::FmirImageError {
    crate::error::FmirImageError::WireProgramInvalid { detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_version_is_6_for_the_carried_semantics_schema() {
        assert_eq!(PACKAGE_MIR_ARTIFACT_VERSION, 6);
    }

    #[test]
    fn device_program_wire_version_is_4_after_the_u1_break() {
        // Stage 3R U1 bumps the nested device-program wire version 3 → 4
        // (S1) and admits it independently of the outer FMIR version (S8).
        assert_eq!(WIRE_DEVICE_PROGRAM_VERSION, 4);
        assert_ne!(WIRE_DEVICE_PROGRAM_VERSION, PACKAGE_MIR_ARTIFACT_VERSION);
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
