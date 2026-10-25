//! FMIR portable package-image schema.
//!
//! Owns the wire structs, the artifact-version constant (clean-break ratchets;
//! no compatibility decoder — A7), the `fmir-text` / `fmir` target tags, the
//! SHA-256 same-package identity + the artifact content/packet digests, and
//! the host runtime-requirement allowlist + the typed device target-feature
//! surface.
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
//! carries the backend selection request and the declared backend artifacts
//! (Metal MSL + CUDA PTX) each with its own content/packet identity digest
//! and its typed device target features.
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

mod admit;
mod device;
mod distributed;
mod distributed_identity;
mod executor_plan;
mod executor_plan_admission;
mod hash;
mod host_execution;
mod host_execution_project;
mod image;
mod requirements;
mod validate;
mod wire;

pub use admit::admit_device_section_with_compat;
pub(crate) use admit::*;
pub use device::*;
pub use distributed::*;
pub use distributed_identity::*;
pub use executor_plan::*;
pub use executor_plan_admission::*;
pub use hash::*;
pub use host_execution::*;
pub use host_execution_project::*;
pub use image::*;
pub use requirements::*;
pub(crate) use validate::*;
pub use wire::*;

#[cfg(test)]
#[path = "distributed_identity_test.rs"]
mod distributed_identity_test;

#[cfg(test)]
#[path = "distributed_test.rs"]
mod distributed_test;

#[cfg(test)]
#[path = "md3h_distributed_test.rs"]
mod md3h_distributed_test;

#[cfg(test)]
#[path = "md3j_distributed_test.rs"]
mod md3j_distributed_test;

#[cfg(test)]
#[path = "session_test.rs"]
mod session_test;

#[cfg(test)]
#[path = "wire_test.rs"]
mod wire_test;

#[cfg(test)]
#[path = "host_execution_test.rs"]
mod host_execution_test;

#[cfg(test)]
#[path = "host_execution_project_test.rs"]
mod host_execution_project_test;

/// Image schema version. Ratchets as clean breaks; there is no compatibility
/// decoder (A7). `3 → 4` clean break adds the optional `device` section
/// (canonical device-program payload, selection, backend artifacts, device
/// runtime requirements) for the differentiable-GPU campaign (N1.7). `4 → 5`
/// clean break (operator memo; S3-A4) makes the device section a
/// TYPED complete program (the opaque payload string is replaced) and moves
/// CUDA symbols + host inputs to metadata. `5 → 6` clean break (Stage 3R U1)
/// carries the F1–F7 semantic facts (semantic value identity, generations,
/// roots, dependencies, lossless relation, initialization/observation axes).
/// `6 → 7` clean break (DDCP2-U2) migrates `FmirDeviceArtifact` to the frozen
/// **`DeviceArtifact` packet** (typed backend id with the unknown-representable
/// case, explicit format id, materialization stage, typed target id +
/// required features, ABI/schema version, canonical raw bytes, explicit
/// text/binary payload encoding, the entrypoint symbol map, and the
/// `content_sha256`/`packet_sha256` identity fields; FNV artifact provenance
/// removed). `7 → 8` adds the ordered `MirIntrinsic::Scan` payload to the
/// serialized MIR program; old version-7 decoders cannot interpret that enum
/// variant. Old images fail closed with
/// [`UnsupportedVersion`](crate::error::FmirImageError::VersionMismatch).
pub const PACKAGE_MIR_ARTIFACT_VERSION: u32 = 8;

/// The accepted artifact ABI/schema version of the `DeviceArtifact` packet
/// (DDCP3-U0 (c) / CTO9-6 c): the **exact-match** admission set for an
/// artifact's declared `abi_version`. Rides the coordinated artifact
/// boundary; the nested device-program wire has its own exact-match ratchet
/// and may advance independently. A deliberate
/// future minor-compatibility lane is an explicit decision, never a silent
/// addition; a packet whose `abi_version` is not this value fails admission
/// with [`UnsupportedArtifactAbiVersion`](crate::error::FmirImageError::ArtifactAbiVersionMismatch).
///
/// NOTE (rebaselined at DDCP3-U0; amended at the fire-12/fire-13 cleanup):
/// faber's landed `DEVICE_ARTIFACT_ABI_VERSION` is **8** (aligned with this
/// coordinated value — faber `0109453` bumped it as part of the U0 consumer
/// migration; the earlier record of a same-boundary `1` divergence is
/// superseded). DDPP1-U5/U6 re-verify the landed value ("summaries are
/// claims"); any future drift is recorded with a `fix:` marker, never
/// silently patched radix-side.
pub const ARTIFACT_ABI_VERSION: u32 = 8;

/// The faber-owned wire/codec version of the typed device-program section
/// (the nested `v` field of [`FmirDeviceProgramSection`]). Bumped in lockstep
/// with the `DEVICE_RUN_PLAN_VERSION` constructor ratchet; bumped `3 → 4` by
/// the Stage 3R U1 clean break (F7/S8), `4 → 5` by the S5-U5b clean break
/// (the declared `RepeatingStep` step count rides the lifetime variant),
/// `5 → 6` by the Stage 5A U1 clean break (S5A-U1: every result row's
/// observation fact carries the explicit **observation cadence** —
/// `PerStep`/`EndOfRun` — so the canonical wire bytes change when the
/// readback cadence changes), `6 → 7` by the S6-C2 clean break (the
/// S6-N1-frozen recipe variants `AxisReduction`/`RowSoftmax`/
/// `LayerNormalization` gain real wire mirrors and every kernel carries the
/// typed rank-extension **broadcast fact** — both change the canonical wire
/// bytes; admission validates the carried broadcast fail-closed, A7), and
/// `7 → 8` by the council-7/council-8 clean break (CB-2/CB-3): the
/// GI3-1 shape changes become version-visible (`WireRopePlan.per_row`/`rows`
/// and the `MirUnOp::F16Round` unary variant landed inside wire-7 without a
/// ratchet — wire-7 is an **unpublished clean break**: no pre-GI3-1 wire-7
/// artifact escaped the repo, ephemeral same-revision tests only — so the
/// bump records them retroactively), and every `AxisReduction` plan +
/// reduced buffer version carries the producer-defined **reduced-resource
/// projection** (`axis_extent` + `inner_stride`) so consumers never
/// reconstruct the reduced-buffer mapping from element count), and `8 → 9`
/// by GEA3-A2f: the fixed composed matmul wire variant was carried as a
/// transitional shape; `9 → 10` (kernel-codegen) makes generic multi-child
/// programs the sole representation.
/// The current kernel-codegen clean break is `10 → 11` (S2): the batched
/// plan variants — `BatchedTiledMatMul` (U1), `BatchedRowSoftmax` (U2),
/// `BatchedTranspose` (U3) — landed as new wire enum variants inside
/// wire-10 without a ratchet; an older wire-10 consumer cannot interpret
/// them, so the bump records them. A stale wire-10 device section fails
/// nested admission before field interpretation — no compat projection.
/// The current clean break is `13 → 14` (ordered Scan plans): Scan adds its
/// three-stage workgroup/chunk/group geometry, summary layout, prefix element
/// width, and traversal direction. A stale wire-13 consumer cannot interpret
/// these stage semantics. The preceding clean break is `12 → 13` (grouped `GridReduce` shapes): a grouped
/// reduction carries its literal bucket shape so its workgroup-by-bucket
/// scratch and second-stage launch are checked from the wire facts. A stale
/// wire-12 device section fails nested admission before field interpretation.
/// The preceding clean break was `11 → 12` (the wire-shape widening): every
/// buffer version carries the producer-stamped **parameter shape vector**
/// (`shape: [dim, …]`, declaration-order literal dims) beside the flattened
/// `element_count`, so consumers never reconstruct rank or dims from the
/// count (`[4,4]` and `[16]` share one); admission rejects a carried shape
/// whose product disagrees with the count. A stale wire-11 device section
/// fails nested admission before field interpretation — no compat
/// projection.
/// Admitted independently of the outer image version:
/// [`admit_device_section`] rejects a mismatched nested version before any
/// field-level interpretation, with a focused diagnostic
/// ([`UnsupportedDeviceProgramVersion`](crate::error::FmirImageError::DeviceProgramVersionMismatch)).
pub const WIRE_DEVICE_PROGRAM_VERSION: u32 = 14;

/// The version ratchet of the optional distributed execution section
/// (MD2-W1 A7). The section carries its OWN nested version, admitted
/// fail-closed **before field interpretation** (the `admit_device_section`
/// pattern, FC4) — independently of the nested device-program wire version
/// and the outer image version. The first distributed-section shape is v1;
/// the nested `WIRE_DEVICE_PROGRAM_VERSION` and the outer package version
/// stayed unchanged for that MD2 boundary (FC1); later clean breaks ratchet
/// them independently.
pub const WIRE_DISTRIBUTED_SECTION_VERSION: u32 = 1;

/// Text image `target` tag.
pub const FMIR_TEXT_TARGET_NAME: &str = "fmir-text";

/// Binary image `target` tag.
pub const FMIR_TARGET_NAME: &str = "fmir";

/// Deterministic canonical bytes of the COMPLETE device program — the A10
/// identity substrate. Both image routes carry the identical typed program
/// (Metal and CUDA images derive the same `WireDeviceProgram`), so both
/// derive identical bytes. Postcard serialization is deterministic for a
/// fixed struct shape and infallible for these scalar/string/enum fields.
///
/// # Panics
///
/// Panics if postcard serialization of the wire program fails; the field
/// shape is fixed and scalar/string/enum only, so this never happens in
/// practice.
#[must_use]
pub fn canonical_program_bytes(program: &WireDeviceProgram) -> Vec<u8> {
    postcard::to_allocvec(program).expect("wire program serialization is infallible")
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
