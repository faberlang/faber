use crate::error::FmirImageError;
use crate::schema::{
    DeviceArtifactFormat, DeviceTargetId, FmirDeviceBackend, MaterializationStage,
};

/// Whether a runtime requirement is a known `host:*` capability.
///
/// `kernel:*` requirements are validated by the `faber` loading policy, which
/// has access to the kernel module/verb manifest resolution in the `radix`
/// package. The host surface is the S1 source/runtime surface — separate from
/// the typed device target-feature surface below.
#[must_use]
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

/// One declarative artifact-compatibility row (DDCP3-U0 (e) / CTO9-6 e): a
/// (backend, format, stage) combination a compiled-in leaf admits, its
/// allowed target id, and its permitted required features. The rows are the
/// table-driven replacement for the flat `is_known_target_feature`
/// allowlist + the hard-coded compiled-in target mapping, and the substrate
/// for the backend/format/stage/target/feature combination validation
/// (CTO9-6 d). Rows are `Copy` + const-constructible so the leaf crates
/// export their rows as `pub const fn` data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactCompatibilityRow {
    /// Backend id (the [`FmirDeviceBackend::spelling`] form, e.g. `"metal"`).
    pub backend: &'static str,
    /// Format id (the [`DeviceArtifactFormat::spelling`] form, e.g. `"msl"`).
    pub format: &'static str,
    /// Materialization stage (the [`MaterializationStage::spelling`] form).
    pub stage: &'static str,
    /// The allowed target id for this backend/format/stage row.
    pub target: &'static str,
    /// The required features permitted for this row (the typed feature ids
    /// the backend's artifacts may carry).
    pub features: &'static [&'static str],
}

/// The declarative artifact-compatibility table (DDCP3-U0 (e)): the rows the
/// caller assembles from the compiled-in leaves
/// (`radix-mir-metal::compat::metal_compatibility_rows` /
/// `radix-mir-llvm::compat::cuda_compatibility_rows`) and passes into
/// [`admit_device_section_with_compat`](crate::schema::admit_device_section_with_compat).
/// The FMIR default table keeps the mid-flight faber consumer admissible;
/// default-table removal + faber caller-supplied assembly is a recorded
/// same-boundary contract item for DDPP1-U5/U6 + DDCP4 (NOT removed here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactCompatibilityTable {
    /// The assembled rows.
    pub rows: &'static [ArtifactCompatibilityRow],
}

/// The FMIR default compatibility table (DDCP3-U0 (e)): the current
/// Metal/CUDA facts as data — Metal artifacts carry `msl` (compiler-input)
/// / `metallib` (finalized-binary) for target `macos-arm64` with the
/// `metal`/`macos-arm64` features; CUDA artifacts carry `ptx` for target
/// `sm_120` with the `cuda`/`sm_80`/`sm_120` features. Kept as the
/// mid-flight default so faber's `admit_device_section` decode path does not
/// break; the leaf-owned rows are the same facts exported by the leaf
/// crates for caller-supplied assembly (removal = recorded residual, not
/// this unit).
#[must_use]
pub fn default_compatibility_table() -> ArtifactCompatibilityTable {
    ArtifactCompatibilityTable {
        rows: &[
            ArtifactCompatibilityRow {
                backend: "metal",
                format: "msl",
                stage: "compiler-input",
                target: "macos-arm64",
                features: &["metal", "macos-arm64"],
            },
            ArtifactCompatibilityRow {
                backend: "metal",
                format: "metallib",
                stage: "finalized-binary",
                target: "macos-arm64",
                features: &["metal", "macos-arm64"],
            },
            ArtifactCompatibilityRow {
                backend: "cuda",
                format: "ptx",
                stage: "compiler-input",
                target: "sm_120",
                features: &["cuda", "sm_80", "sm_120"],
            },
            ArtifactCompatibilityRow {
                backend: "cuda",
                format: "ptx",
                stage: "finalized-binary",
                target: "sm_120",
                features: &["cuda", "sm_80", "sm_120"],
            },
        ],
    }
}

/// Whether a target feature id is known to the assembled compatibility table
/// (DDCP3-U0 (e) — the table-driven replacement for the flat allowlist): a
/// feature is known iff some row permits it. An unknown feature is
/// representable on the packet and rejected honestly by validation — never
/// guessed.
#[must_use]
pub fn is_known_target_feature_in(table: &ArtifactCompatibilityTable, feature: &str) -> bool {
    table
        .rows
        .iter()
        .any(|row| row.features.iter().any(|known| *known == feature))
}

/// Whether a target feature id is known to the FMIR default table (the
/// DDCP2-U3 flat allowlist as data: `metal`/`cuda`/`macos-arm64`/`sm_80`/
/// `sm_120`).
#[must_use]
pub fn is_known_target_feature(feature: &str) -> bool {
    is_known_target_feature_in(&default_compatibility_table(), feature)
}

/// Duplicate-only feature validation (DDCP3-U0 (f) preserve-until-selection):
/// an unknown-backend artifact is validated as far as its declared fields
/// allow — the feature ALLOWLIST is leaf-specific knowledge and does not
/// apply (the artifact is rejected at selection use instead), but a
/// duplicated required feature is structurally invalid regardless of
/// backend and fails closed with the named rule.
///
/// # Errors
///
/// Returns [`FmirImageError::DuplicateRequiredFeature`] on the first
/// duplicate.
pub fn validate_duplicate_target_features(target: &DeviceTargetId) -> Result<(), FmirImageError> {
    for (index, feature) in target.required_features.iter().enumerate() {
        if target.required_features[..index]
            .iter()
            .any(|seen| seen == feature)
        {
            return Err(FmirImageError::DuplicateRequiredFeature {
                feature: feature.clone(),
            });
        }
    }
    Ok(())
}

/// Validate an artifact's typed target id + required features against a
/// compatibility table (DDCP3-U0 (b)/(e)): every required feature must be a
/// known feature (an unknown feature fails closed with the named rule) and
/// **no required feature may appear twice** (a duplicate fails closed with
/// [`FmirImageError::DuplicateRequiredFeature`] — the CTO9-6 b duplicate
/// rejection, independent of hashing).
///
/// # Errors
///
/// Returns [`FmirImageError::UnsupportedTargetFeature`] on the first unknown
/// required feature and [`FmirImageError::DuplicateRequiredFeature`] on the
/// first duplicate.
pub fn validate_target_features_in(
    target: &DeviceTargetId,
    table: &ArtifactCompatibilityTable,
) -> Result<(), FmirImageError> {
    validate_duplicate_target_features(target)?;
    for feature in &target.required_features {
        if !is_known_target_feature_in(table, feature) {
            return Err(FmirImageError::UnsupportedTargetFeature {
                feature: feature.clone(),
            });
        }
    }
    Ok(())
}

/// Validate an artifact's typed target id + required features against the
/// FMIR default table (DDCP2-U3 wrapper).
///
/// # Errors
///
/// Returns [`FmirImageError::UnsupportedTargetFeature`] /
/// [`FmirImageError::DuplicateRequiredFeature`].
pub fn validate_target_features(target: &DeviceTargetId) -> Result<(), FmirImageError> {
    validate_target_features_in(target, &default_compatibility_table())
}

/// Exhaustive target validation for the compiled-in leaves against a
/// compatibility table (contract §Unknown future backends: compiled-in
/// leaves keep exhaustive validation of their own artifact fields). A known
/// backend's target id must be one of the rows' allowed targets. Unknown
/// backends are handled by the unknown-backend admission (representable +
/// preserved-until-selection).
///
/// # Errors
///
/// Returns [`FmirImageError::IncompatibleTargetId`] when a compiled-in leaf
/// artifact declares a target id that is not in its backend's rows.
pub fn validate_compiled_in_target_in(
    backend: &FmirDeviceBackend,
    target: &DeviceTargetId,
    table: &ArtifactCompatibilityTable,
) -> Result<(), FmirImageError> {
    let backend_spelling = backend.spelling();
    let rows: Vec<&ArtifactCompatibilityRow> = table
        .rows
        .iter()
        .filter(|row| row.backend == backend_spelling)
        .collect();
    if rows.is_empty() {
        // Unknown backend (no rows): no compiled-in exhaustive target rule.
        return Ok(());
    }
    if rows.iter().any(|row| row.target == target.id) {
        return Ok(());
    }
    let expected = rows[0].target;
    Err(FmirImageError::IncompatibleTargetId {
        backend: backend_spelling,
        target: target.id.clone(),
        expected: expected.to_owned(),
    })
}

/// Exhaustive target validation against the FMIR default table (DDCP2-U3
/// wrapper).
///
/// # Errors
///
/// Returns [`FmirImageError::IncompatibleTargetId`].
pub fn validate_compiled_in_target(
    backend: &FmirDeviceBackend,
    target: &DeviceTargetId,
) -> Result<(), FmirImageError> {
    validate_compiled_in_target_in(backend, target, &default_compatibility_table())
}

/// Combination validation for the compiled-in leaves (DDCP3-U0 (d) /
/// CTO9-6 d): a **known backend cannot carry an incompatible known format or
/// an unrelated known feature** — the (backend, format, stage) triple must
/// have a row in the table, and every required feature must be permitted for
/// that backend. Unknown-backend artifacts are validated as far as their
/// declared fields allow: the format/stage/feature combination rules are
/// skipped (leaf-specific knowledge), while the global known-feature check
/// still applies via [`validate_target_features_in`].
///
/// # Errors
///
/// Returns [`FmirImageError::IncompatibleArtifactCombination`] when the
/// backend cannot carry the declared format/stage/feature combination.
pub fn validate_artifact_combination(
    backend: &FmirDeviceBackend,
    format: &DeviceArtifactFormat,
    stage: MaterializationStage,
    target: &DeviceTargetId,
    table: &ArtifactCompatibilityTable,
) -> Result<(), FmirImageError> {
    let backend_spelling = backend.spelling();
    if matches!(backend, FmirDeviceBackend::Unknown(_)) {
        // Preserve-until-selection: unknown-backend artifacts are admitted as
        // far as their declared fields allow — no leaf-specific combination
        // knowledge applies.
        return Ok(());
    }
    let format_spelling = format.spelling();
    let stage_spelling = stage.spelling();
    let rows: Vec<&ArtifactCompatibilityRow> = table
        .rows
        .iter()
        .filter(|row| {
            row.backend == backend_spelling
                && row.format == format_spelling
                && row.stage == stage_spelling
        })
        .collect();
    if rows.is_empty() {
        return Err(FmirImageError::IncompatibleArtifactCombination {
            backend: backend_spelling.clone(),
            detail: format!(
                "backend `{backend_spelling}` cannot carry format `{format_spelling}` at stage `{stage_spelling}`"
            ),
        });
    }
    let permitted: Vec<&str> = rows
        .iter()
        .flat_map(|row| row.features.iter().copied())
        .collect();
    for feature in &target.required_features {
        if !permitted.contains(&feature.as_str()) {
            return Err(FmirImageError::IncompatibleArtifactCombination {
                backend: backend_spelling.clone(),
                detail: format!(
                    "backend `{backend_spelling}` does not permit the required feature `{feature}` for format `{format_spelling}` at stage `{stage_spelling}`"
                ),
            });
        }
    }
    Ok(())
}
