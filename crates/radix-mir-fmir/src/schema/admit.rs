use super::device::FmirDeviceSection;
use super::distributed::{
    expected_plan_for_section, wire_plan_to_plan, FmirDistributedExecutionSection,
};
use super::hash::fnv1a64_blob_hash;
use super::requirements::is_known_device_requirement;
use super::wire::WireDeviceProgram;
use super::{
    logical_distributed_plan_hash, validate_wire_distributed_section, validate_wire_program,
    PACKAGE_MIR_ARTIFACT_VERSION, WIRE_DEVICE_PROGRAM_VERSION, WIRE_DISTRIBUTED_SECTION_VERSION,
};

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
    // MD2-W1: the optional distributed execution section is admitted
    // fail-closed at load — its own version ratchet, wire-level section
    // validation, MD2-V1's gate on the reconstructed plan, the equivalence
    // gate, and the H1 hash cross-check. Absent for single-device packages.
    if let Some(distributed) = &device.distributed {
        admit_distributed_section(distributed, &device.device_program.program)?;
    }
    Ok(())
}

/// Fail-closed admission of the optional distributed execution section
/// (MD2-W1). Runs when the section is present, in order:
///
/// 1. **version ratchet** (A7) — `WIRE_DISTRIBUTED_SECTION_VERSION` is
///    checked before any field interpretation (the `admit_device_section`
///    pattern, FC4);
/// 2. **wire-level section validation** — the declared placement facts
///    resolve against the carried single-device program; the declared hash
///    is the canonical spelling;
/// 3. **MD2-V1's gate** — the reconstructed delivered plan passes the
///    fail-before-launch checks (stale versions, topology, owners, replicas,
///    completeness, cycles, admissible collectives);
/// 4. **equivalence gate** — re-derive `(program, declared placement)` and
///    compare canonical bytes with the actually delivered plan (stale /
///    tampered fails with a focused diagnostic);
/// 5. **H1 hash cross-check** — the section's declared
///    `logical_distributed_plan_hash` equals the hash recomputed over the
///    delivered plan.
pub(crate) fn admit_distributed_section(
    section: &FmirDistributedExecutionSection,
    wire_program: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    // A7: the section's own version ratchet fails closed BEFORE any field
    // interpretation — independently of the nested device-program wire
    // version and the outer image version.
    if section.v != WIRE_DISTRIBUTED_SECTION_VERSION {
        return Err(
            crate::error::FmirImageError::UnsupportedDistributedSectionVersion {
                actual: section.v,
                expected: WIRE_DISTRIBUTED_SECTION_VERSION,
            },
        );
    }
    validate_wire_distributed_section(section, wire_program)?;
    let delivered = wire_plan_to_plan(&section.logical_plan)?;
    delivered.validate().map_err(|error| {
        crate::error::FmirImageError::DistributedSectionInvalid {
            detail: format!("delivered logical plan fails the fail-before-launch gate: {error}"),
        }
    })?;
    equivalence_gate(section, wire_program)?;
    // MD2-H1 cross-check: the declared hash must equal the hash recomputed
    // over the delivered plan (reconstructed from the wire — the section
    // mirrors D1's classes, calls V1's gate, and cross-checks H1's hash).
    let recomputed = logical_distributed_plan_hash(&delivered);
    if recomputed != section.logical_distributed_plan_hash {
        return Err(
            crate::error::FmirImageError::DistributedSectionHashMismatch {
                expected: section.logical_distributed_plan_hash.clone(),
                recomputed,
            },
        );
    }
    Ok(())
}

/// The equivalence gate (MD2-W1, exit-gate bullet 4): re-derive
/// `(program, declared placement)` from the carried single-device program and
/// the declared placement facts and compare **canonical bytes** with the
/// **actually delivered plan** (reconstructed from the section). A mutated
/// delivered plan (partition owner, graph edge, transfer byte contract) or a
/// mutated declared placement fails with a focused stale/tampered diagnostic.
/// The gate inspects the delivered bytes — it never trusts the section's
/// self-declared `logical_distributed_plan_hash`.
pub(crate) fn equivalence_gate(
    section: &FmirDistributedExecutionSection,
    wire_program: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    let delivered = wire_plan_to_plan(&section.logical_plan)?;
    let expected = expected_plan_for_section(section, wire_program)?;
    let delivered_bytes = radix_mir::distributed::encode_logical_plan(&delivered);
    let expected_bytes = radix_mir::distributed::encode_logical_plan(&expected);
    if delivered_bytes != expected_bytes {
        return Err(crate::error::FmirImageError::DistributedSectionEquivalenceMismatch {
            detail: format!(
                "the delivered logical plan ({} partitions, {} operations) is not the deterministic derivation of the declared program + placement — stale or tampered; admission re-derives (program, declared placement) and compares canonical bytes with the delivered plan",
                delivered.partitions.len(),
                delivered.graph.operations.len(),
            ),
        });
    }
    Ok(())
}
