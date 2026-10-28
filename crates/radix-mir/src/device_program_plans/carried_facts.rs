//! Carried-fact agreement between a program and its plan.

use super::{RecipeKind, library_entry_for_kernel};
use crate::abi::{MirKernelResource, MirKernelResourceKind, MirKernelSignature};
use crate::device_program::{BufferRole, DeviceResource, KernelUnit};
use crate::kernel_plan::CollectionKernelPlan;
use handroll::FxHashMap;

// ---------------------------------------------------------------------------
// CARRIED-FACT AGREEMENT
// ---------------------------------------------------------------------------

/// Whether a device program's carried plan agrees with the plan re-derived
/// from the kernel function's typed facts.
///
/// `derived == None` (no recipe on the function) agrees only with the
/// no-recipe [`CollectionKernelPlan::Elementwise`] carried plan; a carried
/// recipe must equal the derived plan exactly.
#[must_use]
pub fn carried_plan_agrees(
    carried: &CollectionKernelPlan,
    derived: Option<&CollectionKernelPlan>,
) -> bool {
    match derived {
        // A lane-form function derives no collection plan; the constructor
        // selects `LaneGrid` from its Lane parameter (`lane_grid_plan`).
        None => matches!(
            carried,
            CollectionKernelPlan::Elementwise
                | CollectionKernelPlan::LaneGrid { .. }
                | CollectionKernelPlan::SingleLane
                | CollectionKernelPlan::GridReduce { .. }
        ),
        Some(derived) => derived == carried,
    }
}

fn carried_resources_agree_impl(
    carried: &[DeviceResource],
    signature: &MirKernelSignature,
    allow_extra: bool,
) -> bool {
    let mut expected: FxHashMap<(u32, u32), &MirKernelResource> = FxHashMap::default();
    for resource in signature
        .resources()
        .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
    {
        expected.insert((resource.group, resource.binding), resource);
    }
    for resource in carried {
        let key = (resource.binding.group, resource.binding.binding);
        let Some(signature_resource) = expected.remove(&key) else {
            if !allow_extra {
                return false;
            }
            // A binding outside the carrier signature is a host-dispatch
            // fact of the library body.
            continue;
        };
        if !carried_slot_agrees(resource, signature_resource) {
            return false;
        }
    }
    expected.is_empty()
}

/// Whether a device program's carried resource list agrees with the
/// signature re-derived from the kernel function's typed facts.
///
/// The ABI role/access pair must be compatible with the carried program
/// role. Exact agreement holds for per-kernel roles; a **unified**
/// intermediate (S2-5) carries the program role `InOut` on slots whose own
/// ABI is (Output, Write) or (Input, Read) — the program role is a
/// cross-kernel identity fact (N2.6 data-flow identity), not a per-slot
/// derivation. `InOut` accepts any coherent ABI pair; incoherent pairs (e.g.
/// writing an input) still fail closed because `from_abi_role` yields
/// `None` for them.
#[must_use]
pub fn carried_resources_agree(carried: &[DeviceResource], signature: &MirKernelSignature) -> bool {
    carried_resources_agree_impl(carried, signature, false)
}

/// Whether a library-dispatch kernel's carried resource list agrees with the
/// carrier signature re-derived from the kernel function's typed facts.
///
/// A fused library dispatch (M2 library v0) carries the COMPLETE
/// host-dispatch resource set on the one launch — e.g. a `QkvProjection`
/// launch binds the K/V weights and outputs alongside the carrier ABI slots
/// for the host library dispatcher. Every signature slot must agree under
/// the same per-slot law as [`carried_resources_agree`]; carried resources
/// on bindings outside the signature are host-dispatch facts of the library
/// body, not kernel ABI slots, and no signature slot may be missing.
#[must_use]
pub fn carried_resources_agree_for_library_dispatch(
    carried: &[DeviceResource],
    signature: &MirKernelSignature,
) -> bool {
    carried_resources_agree_impl(carried, signature, true)
}

/// One carried slot against one signature slot — the shared per-slot law of
/// both agreement predicates (role/access coherence plus exact typed facts).
fn carried_slot_agrees(carried: &DeviceResource, signature: &MirKernelResource) -> bool {
    let Some(derived_role) = BufferRole::from_abi_role(signature.role, signature.access) else {
        return false;
    };
    (derived_role == carried.buffer.role || carried.buffer.role == BufferRole::InOut)
        && signature.element_ty == carried.version.element_ty
        && signature.element_count == carried.version.element_count
        // The carried reduced-resource projection (council-8 CB-3) must
        // agree with the version's — a stale or dropped projection is a
        // carried-fact disagreement, never silently tolerated. A
        // signature that DERIVES a projection (the kernel's own body
        // contains the producing SumAxis) must carry the same fact as
        // the version. A signature that derives NO projection is a
        // cross-function consumer (the producing SumAxis lives in
        // another kernel's source); it cannot reconstruct the producer's
        // fact and trusts the carried version — the mapping is carried,
        // never re-derived by a consumer.
        && (signature.reduced_projection.is_none()
            || signature.reduced_projection == carried.version.reduced_projection)
}

/// Whether a kernel unit dispatches a library body whose launch may carry
/// host-dispatch resources beyond the carrier signature (the same
/// library-entry derivation the executor plan uses; a derived library entry
/// means the host dispatcher owns the body).
#[must_use]
pub fn dispatches_library_body(kernel: &KernelUnit) -> bool {
    library_entry_for_kernel(kernel, RecipeKind::from_plan(&kernel.plan)).is_some()
}
