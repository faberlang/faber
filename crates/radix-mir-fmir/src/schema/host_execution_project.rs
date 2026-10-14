//! Radix admission and typed-gap boundary for HEP-3 physical projection.
//!
//! A Host execution packet may be produced only from admitted physical
//! allocation, view, and runtime-window facts. The current FMIR device
//! section does not carry that plan, so this entrypoint verifies every prior
//! source fact it can verify and then fails closed with a typed gap. It never
//! derives allocation bytes, alignment, or physical identity from semantic
//! `WireDeviceProgram` fields.

use std::{error::Error, fmt};

use super::{
    FmirDeviceArtifact, FmirDeviceSection, FmirDeviceSelection, WireHostExecutionEnvelopeV1,
    admit_device_section,
};

/// A typed Radix-side failure to reduce an admitted device program to the
/// complete physical facts required by the Host v1 packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostExecutionProjectionError {
    /// The source device section failed its existing complete FMIR admission.
    DeviceAdmission {
        /// The source admission failure rendered by its owning schema.
        detail: String,
    },
    /// The caller supplied an artifact not declared by the admitted section.
    ArtifactNotDeclared,
    /// The supplied artifact disagrees with an explicit device selection.
    ArtifactSelectionMismatch {
        /// The explicitly selected backend.
        selection: String,
        /// The backend declared by the supplied artifact.
        artifact_backend: String,
    },
    /// A used logical entry has no final entry in the selected artifact.
    MissingArtifactEntry {
        /// The logical entry that requires a final artifact symbol.
        entry: String,
    },
    /// A logical entry maps to more than one final artifact symbol.
    AmbiguousArtifactEntry {
        /// The logical entry with conflicting final symbols.
        entry: String,
    },
    /// The admitted source contains no complete physical allocation/view plan.
    MissingPhysicalFact {
        /// The missing physical fact.
        fact: &'static str,
    },
}

impl fmt::Display for HostExecutionProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceAdmission { detail } => {
                write!(formatter, "device section does not admit: {detail}")
            }
            Self::ArtifactNotDeclared => write!(formatter, "selected artifact is not declared"),
            Self::ArtifactSelectionMismatch {
                selection,
                artifact_backend,
            } => write!(
                formatter,
                "selected backend `{selection}` does not match artifact backend `{artifact_backend}`"
            ),
            Self::MissingArtifactEntry { entry } => {
                write!(
                    formatter,
                    "selected artifact has no final entry for `{entry}`"
                )
            }
            Self::AmbiguousArtifactEntry { entry } => {
                write!(
                    formatter,
                    "selected artifact has ambiguous final entries for `{entry}`"
                )
            }
            Self::MissingPhysicalFact { fact } => {
                write!(formatter, "required physical fact is absent: {fact}")
            }
        }
    }
}

impl Error for HostExecutionProjectionError {}

/// Projects one admitted FMIR device section and one selected target artifact
/// into the immutable physical execution packet.
///
/// The current `FmirDeviceSection` has no admitted physical allocation/view
/// plan. Consequently, a structurally honest source always stops at the
/// typed gap after verifying source admission, target selection, and final
/// artifact entries. That is deliberate: it prevents both a Radix semantic
/// reconstruction and a Hosts-side fallback.
///
/// # Errors
/// Returns [`HostExecutionProjectionError`] when FMIR admission fails, the
/// artifact or final entry is unavailable, or the source lacks the required
/// physical allocation/view facts. No error permits a Hosts-side
/// reconstruction.
pub fn project_host_execution_v1(
    device: &FmirDeviceSection,
    artifact: &FmirDeviceArtifact,
) -> Result<WireHostExecutionEnvelopeV1, HostExecutionProjectionError> {
    admit_device_section(device).map_err(|error| {
        HostExecutionProjectionError::DeviceAdmission {
            detail: error.to_string(),
        }
    })?;
    if !device.artifacts.artifact.contains(artifact) {
        return Err(HostExecutionProjectionError::ArtifactNotDeclared);
    }
    if let Some(selection) = explicit_selection(&device.selection)
        && selection != artifact.backend.spelling()
    {
        return Err(HostExecutionProjectionError::ArtifactSelectionMismatch {
            selection,
            artifact_backend: artifact.backend.spelling(),
        });
    }
    for source_launch in &device.device_program.program.launches {
        let source_kernel = device
            .device_program
            .program
            .kernels
            .get(source_launch.kernel_index as usize)
            .ok_or(HostExecutionProjectionError::MissingPhysicalFact {
                fact: "declared launch kernel",
            })?;
        resolve_final_entry(artifact, source_kernel.function, &source_kernel.entry)?;
    }
    Err(HostExecutionProjectionError::MissingPhysicalFact {
        fact: "admitted physical allocation/view plan",
    })
}

fn explicit_selection(selection: &FmirDeviceSelection) -> Option<String> {
    match selection {
        FmirDeviceSelection::Auto => None,
        FmirDeviceSelection::Metal | FmirDeviceSelection::Cuda | FmirDeviceSelection::Amd => {
            Some(selection.spelling())
        }
        FmirDeviceSelection::Unknown(_) => Some(selection.spelling()),
    }
}

/// Resolve the final artifact symbol for one device-program kernel row.
///
/// The typed function id is the identity (G-06): the reflection path matches
/// `FmirReflectedKernel::function` against it. The logical entry text is
/// carried only for the artifact's entrypoint symbol table, whose rows are
/// resolved by the emitted logical entry name.
pub(super) fn resolve_final_entry(
    artifact: &FmirDeviceArtifact,
    function_id: u32,
    logical_entry: &str,
) -> Result<String, HostExecutionProjectionError> {
    if !artifact.entrypoints.is_empty() {
        let mut entries = artifact
            .entrypoints
            .iter()
            .filter(|entry| entry.entry == logical_entry)
            .map(|entry| entry.symbol.clone());
        let Some(entry) = entries.next() else {
            return Err(HostExecutionProjectionError::MissingArtifactEntry {
                entry: logical_entry.to_owned(),
            });
        };
        if entry.is_empty() {
            return Err(HostExecutionProjectionError::MissingArtifactEntry {
                entry: logical_entry.to_owned(),
            });
        }
        if entries.next().is_some() {
            return Err(HostExecutionProjectionError::AmbiguousArtifactEntry {
                entry: logical_entry.to_owned(),
            });
        }
        return Ok(entry);
    }
    let Some(reflection) = &artifact.reflection else {
        return Err(HostExecutionProjectionError::MissingArtifactEntry {
            entry: logical_entry.to_owned(),
        });
    };
    let mut entries = reflection
        .kernels
        .iter()
        .filter(|kernel| kernel.function == function_id)
        .map(|kernel| kernel.entry_name.clone());
    let Some(entry) = entries.next() else {
        return Err(HostExecutionProjectionError::MissingArtifactEntry {
            entry: logical_entry.to_owned(),
        });
    };
    if entry.is_empty() {
        return Err(HostExecutionProjectionError::MissingArtifactEntry {
            entry: logical_entry.to_owned(),
        });
    }
    if entries.next().is_some() {
        return Err(HostExecutionProjectionError::AmbiguousArtifactEntry {
            entry: logical_entry.to_owned(),
        });
    }
    Ok(entry)
}
