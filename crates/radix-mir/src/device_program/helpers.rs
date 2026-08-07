use super::error::DeviceProgramError;
use super::registry::{RegistryBuffer, RegistryVersion};
use super::types::{
    BufferId, BufferIdentity, BufferRole, BufferVersion, DeviceProgram, ResultBuffer,
};
use crate::abi::MirKernelResourceAccess;
use crate::MirType;

/// Whether a slot access is consistent with the buffer's program role.
///
/// Inputs are read-only; outputs are written (a read-write output slot is
/// also coherent — an in-place result); in-out intermediates may be read,
/// written, or both.
pub(super) fn role_permits_access(role: BufferRole, access: MirKernelResourceAccess) -> bool {
    match role {
        BufferRole::Input => access == MirKernelResourceAccess::Read,
        BufferRole::Output => matches!(
            access,
            MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
        ),
        BufferRole::InOut => true,
    }
}

/// Record the first-seen identity for a buffer id; reject conflicting repeats.
pub(super) fn record_identity(
    seen: &mut Vec<BufferIdentity>,
    identity: &BufferIdentity,
) -> Result<(), DeviceProgramError> {
    let Some(first) = seen.iter().find(|first| first.id == identity.id) else {
        seen.push(identity.clone());
        return Ok(());
    };
    if first.name != identity.name {
        return Err(DeviceProgramError::BufferIdentityConflict {
            buffer: identity.id,
            field: "name",
            first: first.name.clone(),
            second: identity.name.clone(),
        });
    }
    if first.role != identity.role {
        return Err(DeviceProgramError::BufferIdentityConflict {
            buffer: identity.id,
            field: "role",
            first: first.role.spelling().to_owned(),
            second: identity.role.spelling().to_owned(),
        });
    }
    if first.storage != identity.storage {
        return Err(DeviceProgramError::BufferIdentityConflict {
            buffer: identity.id,
            field: "storage",
            first: format!("{:?}", first.storage),
            second: format!("{:?}", identity.storage),
        });
    }
    if first.lifetime != identity.lifetime {
        return Err(DeviceProgramError::BufferIdentityConflict {
            buffer: identity.id,
            field: "lifetime",
            first: first.lifetime.spelling().to_owned(),
            second: identity.lifetime.spelling().to_owned(),
        });
    }
    Ok(())
}

/// Record the first-seen shape for a (buffer, version); reject conflicting
/// shapes (a shape change must be a new version — no in-place
/// reinterpretation).
pub(super) fn record_shape(
    seen: &mut Vec<ShapeRecord>,
    buffer: BufferId,
    version: &BufferVersion,
) -> Result<(), DeviceProgramError> {
    if let Some(first) = seen
        .iter()
        .find(|seen| seen.buffer == buffer && seen.version == version.version)
    {
        if first.element_ty != version.element_ty
            || first.element_count != version.element_count
            // The reduced-resource projection (council-8 CB-3) is part of
            // the version's shape — two references to one version must
            // agree on the producer-defined projection too.
            || first.reduced_projection != version.reduced_projection
        {
            return Err(DeviceProgramError::VersionShapeConflict {
                buffer,
                version: version.version,
                first: shape_description(first.element_ty, first.element_count),
                second: shape_description(version.element_ty, version.element_count),
            });
        }
    } else {
        seen.push(ShapeRecord {
            buffer,
            version: version.version,
            element_ty: version.element_ty,
            element_count: version.element_count,
            reduced_projection: version.reduced_projection,
        });
    }
    Ok(())
}

/// The recorded shape identity of one (buffer, version): element type +
/// count + the carried reduced-resource projection (council-8 CB-3).
pub(super) struct ShapeRecord {
    pub(super) buffer: BufferId,
    pub(super) version: u32,
    pub(super) element_ty: MirType,
    pub(super) element_count: u64,
    pub(super) reduced_projection: Option<crate::abi::ReducedProjection>,
}

/// Compact shape description for diagnostics.
fn shape_description(element_ty: MirType, element_count: u64) -> String {
    format!(
        "{element_count} elements of type {:?}",
        element_ty.semantic_id()
    )
}

/// Whether the named producing launch actually writes the result's version.
pub(super) fn result_is_produced(program: &DeviceProgram, result: &ResultBuffer) -> bool {
    program
        .launches
        .iter()
        .find(|launch| launch.id == result.produced_by)
        .and_then(|launch| program.kernels.get(launch.kernel_index))
        .is_some_and(|kernel| {
            kernel.resources.iter().any(|resource| {
                resource.buffer.id == result.buffer.id
                    && resource.version.version == result.version.version
                    && matches!(
                        resource.access,
                        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                    )
            })
        })
}

/// Find or insert the registry entry for a buffer id.
pub(super) fn registry_buffer_mut<'a>(
    buffers: &'a mut Vec<RegistryBuffer>,
    identity: &BufferIdentity,
) -> &'a mut RegistryBuffer {
    if let Some(index) = buffers
        .iter()
        .position(|entry| entry.identity.id == identity.id)
    {
        return &mut buffers[index];
    }
    buffers.push(RegistryBuffer {
        identity: identity.clone(),
        versions: Vec::new(),
    });
    let index = buffers.len() - 1;
    &mut buffers[index]
}

/// Find or insert the registry entry for a buffer version.
pub(super) fn registry_version_mut<'a>(
    versions: &'a mut Vec<RegistryVersion>,
    version: &BufferVersion,
) -> &'a mut RegistryVersion {
    if let Some(index) = versions
        .iter()
        .position(|entry| entry.version.version == version.version)
    {
        return &mut versions[index];
    }
    versions.push(RegistryVersion {
        version: version.clone(),
        produced_by: Vec::new(),
        consumed_by: Vec::new(),
    });
    let index = versions.len() - 1;
    &mut versions[index]
}
