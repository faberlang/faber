use crate::error::FmirImageError;
use crate::schema::{DeviceTargetId, FmirDeviceBackend};

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

/// Whether a target feature id is known to the compiled-in leaves
/// (DDCP2-U3 — the typed replacement for the `device:metal`/`device:cuda`
/// string allowlist). The known features are the compiled-in leaves'
/// declared target/feature ids. An unknown feature is representable on the
/// packet and rejected honestly by validation — never guessed.
#[must_use]
pub fn is_known_target_feature(feature: &str) -> bool {
    matches!(
        feature,
        "metal" | "cuda" | "macos-arm64" | "sm_80" | "sm_120"
    )
}

/// Validate an artifact's typed target id + required features (DDCP2-U3).
/// Every required feature must be a known target feature — an unknown
/// feature fails closed with a stable structured diagnostic naming the
/// feature (never guessed, never silently dropped).
///
/// # Errors
///
/// Returns [`FmirImageError::UnsupportedTargetFeature`] on the first unknown
/// required feature.
pub fn validate_target_features(
    target: &DeviceTargetId,
) -> Result<(), FmirImageError> {
    for feature in &target.required_features {
        if !is_known_target_feature(feature) {
            return Err(FmirImageError::UnsupportedTargetFeature {
                feature: feature.clone(),
            });
        }
    }
    Ok(())
}

/// Exhaustive target validation for the compiled-in leaves (contract
/// §Unknown future backends: compiled-in leaves keep exhaustive validation
/// of their own artifact fields). A Metal artifact's target id must be the
/// Metal leaf's target (`macos-arm64`); a CUDA artifact's target id must be
/// the CUDA leaf's target (`sm_120`). An incompatible target id fails closed
/// with the named rule. Unknown-backend artifacts are handled by the
/// unknown-backend admission (representable + rejected honestly).
///
/// # Errors
///
/// Returns [`FmirImageError::IncompatibleTargetId`] when a compiled-in leaf
/// artifact declares a target id that is not its leaf's target.
pub fn validate_compiled_in_target(
    backend: &FmirDeviceBackend,
    target: &DeviceTargetId,
) -> Result<(), FmirImageError> {
    let expected = match backend {
        FmirDeviceBackend::Metal => "macos-arm64",
        FmirDeviceBackend::Cuda => "sm_120",
        FmirDeviceBackend::Unknown(_) => return Ok(()),
    };
    if target.id != expected {
        return Err(FmirImageError::IncompatibleTargetId {
            backend: backend.spelling(),
            target: target.id.clone(),
            expected: expected.to_owned(),
        });
    }
    Ok(())
}
