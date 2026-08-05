use super::device::FmirDeviceSection;
use super::hash::fnv1a64_blob_hash;
use super::requirements::is_known_device_requirement;
use super::{PACKAGE_MIR_ARTIFACT_VERSION, WIRE_DEVICE_PROGRAM_VERSION};
use crate::schema::validate_wire_program;

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
