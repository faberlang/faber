//! Structured fail-closed error for FMIR image decode and validation.

use std::fmt;

/// Structured error for FMIR image decode and validation failures.
///
/// Every variant carries enough information for a caller (the `faber` package
/// layer) to render its own diagnostic with the established issue codes.
#[derive(Debug)]
pub enum FmirImageError {
    /// The payload is not valid TOML (text image) or postcard (binary image)
    /// for the image schema.
    Parse(String),

    /// The image carries an artifact version the loader does not support.
    /// Version ratchets are clean breaks; there is no compatibility decoder.
    VersionMismatch {
        /// Version encoded in the image.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// The image `target` tag does not match the format it was decoded as.
    TargetMismatch {
        /// Target tag encoded in the image.
        found: String,
        /// Target tag the format requires (`fmir-text` or `fmir`).
        expected: &'static str,
    },

    /// The toolchain version recorded in the image does not match the
    /// expected compiler version (faber's crate version). A device payload is
    /// admitted only from the exact producing toolchain version (N1.7).
    ToolchainMismatch {
        /// Toolchain version recorded in the image.
        found: String,
        /// Toolchain version the loader expects.
        expected: String,
    },

    /// A backend artifact's stored identity digest disagrees with the
    /// recomputed digest (contract §`IdentityDomains` rule 5; B3): the
    /// `content_sha256` is re-verified against the canonical decoded raw
    /// payload bytes, the `packet_sha256` against the versioned artifact
    /// metadata + `content_sha256`. Provenance is verified, not decorative.
    ArtifactDigestMismatch {
        /// Backend the mismatched artifact declares.
        backend: String,
        /// The digest field at fault (`content_sha256` or `packet_sha256`).
        field: &'static str,
        /// Digest stored in the image.
        stored: String,
        /// Digest recomputed from the artifact.
        computed: String,
    },

    /// The stage/provenance rule is violated: a `FinalizedBinary` artifact
    /// must carry its hash-bound parent `compiler_input_packet_sha256`
    /// (never silently replaced), a `CompilerInput` artifact must not.
    CompilerInputProvenance {
        /// Backend of the offending artifact.
        backend: String,
        /// The provenance rule violated.
        detail: String,
    },

    /// An artifact's typed target feature is not known to the compiled-in
    /// leaves (DDCP2-U3). Unknown features are rejected honestly, never
    /// guessed, never silently dropped.
    NonAdmittedTargetFeature {
        /// The unknown feature id.
        feature: String,
    },

    /// A compiled-in leaf artifact declares a target id that is not its
    /// leaf's target (exhaustive compiled-in validation).
    TargetIdMismatch {
        /// Backend of the offending artifact.
        backend: String,
        /// Target id the artifact declares.
        target: String,
        /// The compiled-in leaf's expected target id.
        expected: String,
    },

    /// An explicit backend selection names no artifact of the selected
    /// backend (DDCP2-U3): a missing artifact fails closed with the named
    /// rule.
    AbsentArtifactForSelection {
        /// The selected backend id.
        selection: String,
    },

    /// A backend artifact's explicit payload-encoding declaration is
    /// inconsistent with its canonical raw bytes (contract §Payload
    /// encoding): text payloads must be canonical UTF-8 (no BOM), binary
    /// payloads must carry the declared
    /// `binary:<encoding-tag>:<decoded-byte-length>:<canonical-unpadded-base64>`
    /// spelling. No binary is smuggled through UTF-8 or base64 without this
    /// schema-level declaration.
    ArtifactEncodingMismatch {
        /// Backend the mismatched artifact declares.
        backend: String,
        /// The payload-encoding rule violated.
        detail: String,
    },

    /// A backend artifact declares a backend id that is not a compiled-in
    /// leaf. Representable on the packet and preserved-until-selection
    /// (DDCP3-U0 (f)): admission no longer rejects it at load; selection use
    /// of an unknown backend fails with
    /// [`FmirImageError::NonAdmittedSelectedBackend`]. **Retained as a
    /// same-boundary compatibility shim** — faber's landed diagnostic
    /// mapping (`mir/image.rs` §Unknown) still matches this variant; removal
    /// routes through DDPP1-U5/U6 (recorded residual, not removed here).
    NonAdmittedArtifactBackend {
        /// The unknown backend id.
        backend: String,
    },

    /// An artifact's declared `abi_version` is not in the accepted set
    /// (DDCP3-U0 (c) / CTO9-6 c): the default admission rule is an
    /// **exact-match clean break** against [`crate::schema::ARTIFACT_ABI_VERSION`],
    /// which rides the coordinated boundary
    /// (`HOST_CALL_ABI_VERSION == DEVICE_RUN_PLAN_VERSION`). A deliberate
    /// future minor-compatibility lane is an explicit decision, never a
    /// silent addition.
    ArtifactAbiVersionMismatch {
        /// Backend of the offending artifact.
        backend: String,
        /// ABI/schema version the artifact declares.
        actual: u32,
        /// The accepted ABI version.
        accepted: u32,
    },

    /// An artifact's reflection carrier declares a nested `v` that is not
    /// [`crate::schema::ARTIFACT_REFLECTION_VERSION`]. The carrier's own
    /// version is admitted independently of the artifact ABI version and the
    /// outer image versions, before any reflected kernel is used: it is an
    /// **exact-match clean break**, never an alias for an older or newer
    /// reflection layout.
    ArtifactReflectionVersionMismatch {
        /// Backend of the offending artifact.
        backend: String,
        /// Reflection version the artifact declares.
        actual: u32,
        /// The accepted reflection version.
        expected: u32,
    },

    /// A compiled-in leaf artifact declares a (backend, format, stage,
    /// feature) combination its backend cannot carry (DDCP3-U0 (d) /
    /// CTO9-6 d): a known backend cannot carry an incompatible known format
    /// or an unrelated known feature. The detail names the violated
    /// combination.
    IncompatibleArtifactCombination {
        /// Backend of the offending artifact.
        backend: String,
        /// The violated combination, named.
        detail: String,
    },

    /// An artifact carries a required target feature more than once
    /// (DDCP3-U0 (b) / CTO9-6 b): duplicate features are rejected at
    /// admission with the named rule (the packet identity is the canonical
    /// SET — duplicates cannot have a well-defined set identity).
    DuplicateRequiredFeature {
        /// The duplicated feature id.
        feature: String,
    },

    /// An artifact's entrypoint symbol map repeats an entrypoint identity
    /// (DDCP3-U0 (b) / CTO9-6 b): duplicate entrypoint identities are
    /// rejected at admission with the named rule.
    DuplicateEntrypoint {
        /// The duplicated entrypoint identity.
        entry: String,
    },

    /// Backend-selection resolution named an unknown backend (DDCP3-U0 (f) /
    /// CTO9-6 f): an explicit selection of an unknown backend fails at
    /// selection resolution with the named rule — aligned with faber's
    /// landed fail-closed `FmirDeviceSelection::Unknown(_)` arm. Unknown
    /// backend ARTIFACTS are preserved-until-selection (admitted at load,
    /// validated as far as their declared fields allow).
    NonAdmittedSelectedBackend {
        /// The selected backend id.
        backend: String,
    },

    /// The nested device-program wire version is unsupported. The nested
    /// version is admitted independently of the outer FMIR image version
    /// (Stage 3R S8/F7), before any field-level interpretation of the device
    /// section.
    DeviceProgramVersionMismatch {
        /// Version encoded in the device section.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// The separately versioned executor-plan section is unsupported.
    ExecutorPlanSectionVersionMismatch {
        /// Version encoded in the executor-plan section.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// The executor-plan section contradicts the carried device program or
    /// contains incomplete typed selection facts.
    ExecutorPlanSectionInvalid {
        /// Focused fail-closed detail.
        detail: String,
    },

    /// The device program's carried semantic facts violate a fail-closed
    /// rule (Stage 3R F1–F7): identity aliasing, generation errors, graph
    /// cycles, illegal roots, missing dependencies, or duplicate/
    /// contradictory relation rows. The detail carries the focused
    /// diagnostic.
    WireProgramInvalid {
        /// The focused rule violation.
        detail: String,
    },

    /// The nested distributed-section version is unsupported (MD2-W1 A7).
    /// The distributed section carries its OWN version ratchet, admitted
    /// fail-closed before any field-level interpretation — independently of
    /// the nested device-program wire version and the outer image version.
    DistributedSectionVersionMismatch {
        /// Version encoded in the distributed section.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// A distributed section violates a fail-closed rule (MD2-W1): a
    /// wire-level section check (facts that do not resolve against the
    /// single-device program, an unknown element-type spelling) or the
    /// fail-before-launch validation gate (MD2-V1, run on the reconstructed
    /// logical plan). The detail carries the focused diagnostic.
    DistributedSectionInvalid {
        /// The focused rule violation.
        detail: String,
    },

    /// The equivalence gate failed (MD2-W1, exit-gate bullet 4): admission
    /// re-derived `(program, declared placement)` and the canonical bytes do
    /// NOT match the actually delivered logical plan (reconstructed from the
    /// section). The delivered plan is stale or tampered — the gate inspects
    /// the delivered bytes, never a self-declared hash.
    DistributedSectionEquivalenceMismatch {
        /// The focused stale/tampered diagnostic.
        detail: String,
    },

    /// The section's declared `logical_distributed_plan_hash` does not match
    /// the hash recomputed over the delivered logical plan (MD2-W1
    /// cross-checks MD2-H1's hash domain). A stale self-declared hash fails
    /// closed.
    DistributedSectionHashMismatch {
        /// The hash stored in the section.
        expected: String,
        /// The hash recomputed from the delivered logical plan.
        recomputed: String,
    },

    /// The nested session-section version is unsupported (GI4-2 A7). The
    /// cadence/session section carries its OWN version ratchet, admitted
    /// fail-closed before any field-level interpretation — independently of
    /// the nested device-program wire version and the outer image version.
    SessionSectionVersionMismatch {
        /// Version encoded in the session section.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// A cadence/session section violates a fail-closed rule (GI4-2): a
    /// wire-level section check — a zero KV-layout dimension, a dtype outside
    /// the closed surface, a missing or duplicate workload mode, an
    /// unresolvable or duplicate session-input slot, a cadence that
    /// contradicts the carried program's buffer facts, or an observation
    /// cadence that contradicts a decode invocation mode. The detail carries
    /// the focused diagnostic naming the violated class and the exact
    /// failing fact.
    SessionSectionInvalid {
        /// The focused rule violation.
        detail: String,
    },
}

impl fmt::Display for FmirImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(detail) => write!(f, "{detail}"),
            Self::VersionMismatch { actual, expected } => {
                fmt_version_mismatch(f, *actual, *expected)
            }
            Self::TargetMismatch { found, expected } => fmt_target_mismatch(f, found, expected),
            Self::ToolchainMismatch { found, expected } => {
                fmt_toolchain_mismatch(f, found, expected)
            }
            Self::ArtifactDigestMismatch {
                backend,
                field,
                stored,
                computed,
            } => fmt_artifact_digest_mismatch(f, backend, field, stored, computed),
            Self::CompilerInputProvenance { backend, detail } => write!(
                f,
                "FMIR device artifact `{backend}` provenance rule violated: {detail}"
            ),
            Self::NonAdmittedTargetFeature { feature } => write!(
                f,
                "FMIR device artifact declares target feature `{feature}`, which is not known to the compiled-in leaves — representable and rejected honestly, never guessed (DDCP2-U3 typed target features)"
            ),
            Self::TargetIdMismatch {
                backend,
                target,
                expected,
            } => fmt_target_id_mismatch(f, backend, target, expected),
            Self::AbsentArtifactForSelection { selection } => fmt_absent_artifact(f, selection),
            Self::ArtifactEncodingMismatch { backend, detail } => {
                fmt_artifact_encoding_mismatch(f, backend, detail)
            }
            Self::NonAdmittedArtifactBackend { backend } => fmt_non_admitted_backend(f, backend),
            Self::ArtifactAbiVersionMismatch {
                backend,
                actual,
                accepted,
            } => fmt_artifact_abi_version_mismatch(f, backend, *actual, *accepted),
            Self::ArtifactReflectionVersionMismatch {
                backend,
                actual,
                expected,
            } => write!(
                f,
                "unsupported artifact reflection version {actual} for backend `{backend}`; expected {expected}"
            ),
            Self::IncompatibleArtifactCombination { backend, detail } => write!(
                f,
                "FMIR device artifact `{backend}` combination is incompatible: {detail} (DDCP3-U0 (d) backend/format/stage/feature combination validation)"
            ),
            Self::DuplicateRequiredFeature { feature } => {
                fmt_duplicate_required_feature(f, feature)
            }
            Self::DuplicateEntrypoint { entry } => fmt_duplicate_entrypoint(f, entry),
            Self::NonAdmittedSelectedBackend { backend } => write!(
                f,
                "FMIR device backend selection `{backend}` is unknown — selection resolution fails closed (DDCP3-U0 (f) preserve-until-selection; unknown backend artifacts are admitted at load and rejected at selection use)"
            ),
            Self::DeviceProgramVersionMismatch { actual, expected } => {
                fmt_device_program_version_mismatch(f, *actual, *expected)
            }
            Self::ExecutorPlanSectionVersionMismatch { actual, expected } => write!(
                f,
                "unsupported executor-plan section version {actual}; expected {expected}"
            ),
            Self::ExecutorPlanSectionInvalid { detail } => {
                write!(f, "invalid executor-plan section: {detail}")
            }
            Self::WireProgramInvalid { detail } => {
                write!(f, "invalid device program: {detail}")
            }
            Self::DistributedSectionVersionMismatch { actual, expected } => {
                fmt_distributed_section_version_mismatch(f, *actual, *expected)
            }
            Self::DistributedSectionInvalid { detail } => {
                write!(f, "invalid distributed section: {detail}")
            }
            Self::DistributedSectionEquivalenceMismatch { detail } => {
                write!(f, "distributed plan mismatch: {detail}")
            }
            Self::DistributedSectionHashMismatch {
                expected,
                recomputed,
            } => fmt_distributed_hash_mismatch(f, expected, recomputed),
            Self::SessionSectionVersionMismatch { actual, expected } => {
                fmt_session_section_version_mismatch(f, *actual, *expected)
            }
            Self::SessionSectionInvalid { detail } => {
                write!(f, "invalid session section: {detail}")
            }
        }
    }
}

fn fmt_version_mismatch(f: &mut fmt::Formatter<'_>, actual: u32, expected: u32) -> fmt::Result {
    write!(
        f,
        "unsupported FMIR image version {actual}; expected {expected}"
    )
}

fn fmt_target_mismatch(f: &mut fmt::Formatter<'_>, found: &str, expected: &str) -> fmt::Result {
    write!(
        f,
        "FMIR image target must be `{expected}` (found `{found}`)"
    )
}

fn fmt_toolchain_mismatch(f: &mut fmt::Formatter<'_>, found: &str, expected: &str) -> fmt::Result {
    write!(
        f,
        "unsupported FMIR image toolchain {found}; expected {expected}"
    )
}

fn fmt_artifact_digest_mismatch(
    f: &mut fmt::Formatter<'_>,
    backend: &str,
    field: &str,
    stored: &str,
    computed: &str,
) -> fmt::Result {
    write!(
        f,
        "FMIR device artifact `{backend}` {field} mismatch (stored {stored}, recomputed {computed}) — digests cover decoded bytes, never the transport spelling (contract §IdentityDomains rule 5)"
    )
}

fn fmt_target_id_mismatch(
    f: &mut fmt::Formatter<'_>,
    backend: &str,
    target: &str,
    expected: &str,
) -> fmt::Result {
    write!(
        f,
        "FMIR device artifact `{backend}` declares target id `{target}`, but the compiled-in leaf's target is `{expected}` — exhaustive compiled-in validation fails closed"
    )
}

fn fmt_absent_artifact(f: &mut fmt::Formatter<'_>, selection: &str) -> fmt::Result {
    write!(
        f,
        "FMIR device backend selection `{selection}` has no artifact of that backend — a missing artifact fails closed (DDCP2-U3 selection validation)"
    )
}

fn fmt_artifact_encoding_mismatch(
    f: &mut fmt::Formatter<'_>,
    backend: &str,
    detail: &str,
) -> fmt::Result {
    write!(
        f,
        "FMIR device artifact `{backend}` payload-encoding mismatch: {detail} (contract §Payload encoding)"
    )
}

fn fmt_non_admitted_backend(f: &mut fmt::Formatter<'_>, backend: &str) -> fmt::Result {
    write!(
        f,
        "FMIR device artifact backend `{backend}` is not a compiled-in leaf — representable on the packet and preserved-until-selection (DDCP3-U0 (f)); selection use fails closed with `NonAdmittedSelectedBackend`, never silently dropped, remapped, or fallback-laned (contract §Unknown future backends)"
    )
}

fn fmt_artifact_abi_version_mismatch(
    f: &mut fmt::Formatter<'_>,
    backend: &str,
    actual: u32,
    accepted: u32,
) -> fmt::Result {
    write!(
        f,
        "FMIR device artifact `{backend}` declares ABI version {actual}; the accepted version is {accepted} — exact-match clean break (DDCP3-U0 (c), riding `HOST_CALL_ABI_VERSION == DEVICE_RUN_PLAN_VERSION`)"
    )
}

fn fmt_duplicate_required_feature(f: &mut fmt::Formatter<'_>, feature: &str) -> fmt::Result {
    write!(
        f,
        "FMIR device artifact declares target feature `{feature}` more than once — duplicate required features fail admission (DDCP3-U0 (b) canonical set identity)"
    )
}

fn fmt_duplicate_entrypoint(f: &mut fmt::Formatter<'_>, entry: &str) -> fmt::Result {
    write!(
        f,
        "FMIR device artifact declares entrypoint `{entry}` more than once — duplicate entrypoint identities fail admission (DDCP3-U0 (b))"
    )
}

fn fmt_device_program_version_mismatch(
    f: &mut fmt::Formatter<'_>,
    actual: u32,
    expected: u32,
) -> fmt::Result {
    write!(
        f,
        "incompatible device-program wire version {actual}; this loader accepts wire version {expected} — the serialized device program was produced by a different (incompatible) radix wire revision"
    )
}

fn fmt_distributed_section_version_mismatch(
    f: &mut fmt::Formatter<'_>,
    actual: u32,
    expected: u32,
) -> fmt::Result {
    write!(
        f,
        "unsupported distributed-section version {actual}; expected {expected}"
    )
}

fn fmt_distributed_hash_mismatch(
    f: &mut fmt::Formatter<'_>,
    expected: &str,
    recomputed: &str,
) -> fmt::Result {
    write!(
        f,
        "distributed-section logical-plan hash {expected} does not match the delivered plan (recomputed {recomputed})"
    )
}

fn fmt_session_section_version_mismatch(
    f: &mut fmt::Formatter<'_>,
    actual: u32,
    expected: u32,
) -> fmt::Result {
    write!(
        f,
        "unsupported session-section version {actual}; expected {expected}"
    )
}
