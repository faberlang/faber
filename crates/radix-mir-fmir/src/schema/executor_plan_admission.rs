//! Focused admission for the KV-B executor-plan v2 payload.
//!
//! This helper is deliberately not wired into the shared device-section
//! admission yet. A4V owns the outer section-version carrier and A5R owns the
//! shared admission integration. Until then, this module proves the v2
//! boundary in isolation: version skew fails before field interpretation,
//! runtime sources are closed, and allocation/view arithmetic fails closed.

use super::executor_plan::{
    WireAllocationFact, WireExecutorPlanSource, WireExecutorPlanV2, WireRuntimeSource,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The focused errors produced by executor-plan v2 admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorPlanAdmissionError {
    /// The producer payload version is not the v2 version understood here.
    UnsupportedVersion {
        /// Version carried by the payload.
        actual: u16,
        /// Version this helper accepts.
        expected: u16,
    },
    /// The outer FMIR executor-plan section version is stale or skewed.
    UnsupportedSectionVersion {
        /// Version carried by the outer section.
        actual: u32,
        /// Version this helper accepts.
        expected: u32,
    },
    /// The number or order of selections contradicts the carried program.
    SelectionShape {
        /// Focused shape diagnostic.
        detail: String,
    },
    /// A runtime source is not one of the four typed invocation-state fields.
    UnknownRuntimeSource {
        /// The unknown source spelling carried by the payload.
        source: String,
    },
    /// A checked span calculation overflowed `u64`.
    ArithmeticOverflow {
        /// The span component whose checked operation overflowed.
        field: &'static str,
    },
    /// A v2 fact is structurally inconsistent without overflowing arithmetic.
    InvalidFact {
        /// Focused fact diagnostic.
        detail: String,
    },
}

impl fmt::Display for ExecutorPlanAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { actual, expected } => write!(
                formatter,
                "unsupported executor-plan v2 payload version {actual}; expected {expected}"
            ),
            Self::UnsupportedSectionVersion { actual, expected } => write!(
                formatter,
                "unsupported executor-plan v2 section version {actual}; expected {expected}"
            ),
            Self::SelectionShape { detail } => {
                write!(formatter, "invalid executor-plan shape: {detail}")
            }
            Self::UnknownRuntimeSource { source } => {
                write!(formatter, "unknown executor-plan runtime source `{source}`")
            }
            Self::ArithmeticOverflow { field } => {
                write!(
                    formatter,
                    "executor-plan arithmetic overflow while checking {field}"
                )
            }
            Self::InvalidFact { detail } => {
                write!(formatter, "invalid executor-plan fact: {detail}")
            }
        }
    }
}

impl std::error::Error for ExecutorPlanAdmissionError {}

/// Admit a v2 executor-plan payload against its program kernel count.
///
/// The version check is intentionally the first operation. In particular, a
/// v1 payload is never passed to the v2 field checks, and a v1 constant cannot
/// be reinterpreted as a runtime cursor.
///
/// # Errors
/// Returns [`ExecutorPlanAdmissionError`] when the payload version is stale,
/// the selection shape is inconsistent, a runtime source is unknown, or a
/// view/allocation span calculation overflows.
pub fn admit_executor_plan_v2(
    payload: &WireExecutorPlanV2,
    kernel_count: usize,
) -> Result<(), ExecutorPlanAdmissionError> {
    if payload.abi_version != super::executor_plan::EXECUTOR_PLAN_V2_VERSION {
        return Err(ExecutorPlanAdmissionError::UnsupportedVersion {
            actual: payload.abi_version,
            expected: super::executor_plan::EXECUTOR_PLAN_V2_VERSION,
        });
    }
    if payload.selections.len() != kernel_count {
        return Err(ExecutorPlanAdmissionError::SelectionShape {
            detail: format!(
                "selection count {} does not match kernel count {kernel_count}",
                payload.selections.len()
            ),
        });
    }
    for (position, selection) in payload.selections.iter().enumerate() {
        if usize::try_from(selection.kernel_index).ok() != Some(position) {
            return Err(ExecutorPlanAdmissionError::SelectionShape {
                detail: format!(
                    "selection index {} is not the ordered kernel index {position}",
                    selection.kernel_index
                ),
            });
        }
    }

    let allocations = admit_allocations(&payload.allocations)?;
    admit_views(payload, &allocations)?;
    admit_launch_bindings(payload, &allocations, kernel_count)?;
    Ok(())
}

/// Admit a v2 payload together with the outer FMIR section version.
///
/// A4V/A5R can call this at the package boundary after the outer carrier has
/// been ratcheted. The outer check runs before the payload check and before
/// any selection, allocation, or view field is interpreted.
pub fn admit_executor_plan_section_v2_with_version(
    section_version: u32,
    payload: &WireExecutorPlanV2,
    kernel_count: usize,
) -> Result<(), ExecutorPlanAdmissionError> {
    if section_version != super::executor_plan::EXECUTOR_PLAN_SECTION_V2_VERSION {
        return Err(ExecutorPlanAdmissionError::UnsupportedSectionVersion {
            actual: section_version,
            expected: super::executor_plan::EXECUTOR_PLAN_SECTION_V2_VERSION,
        });
    }
    admit_executor_plan_v2(payload, kernel_count)
}

/// Alias named for callers that refer to the package carrier as a section.
pub fn admit_executor_plan_section_v2(
    payload: &WireExecutorPlanV2,
    kernel_count: usize,
) -> Result<(), ExecutorPlanAdmissionError> {
    admit_executor_plan_section_v2_with_version(
        super::executor_plan::EXECUTOR_PLAN_SECTION_V2_VERSION,
        payload,
        kernel_count,
    )
}

fn admit_allocations(
    facts: &[WireAllocationFact],
) -> Result<BTreeMap<u32, &WireAllocationFact>, ExecutorPlanAdmissionError> {
    let mut allocations = BTreeMap::new();
    for allocation in facts {
        let id = allocation.buffer.id;
        if id == 0 || allocations.insert(id, allocation).is_some() {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: format!("allocation id {id} is zero or duplicated"),
            });
        }
        if allocation.capacity_bytes == 0
            || allocation.dtype.is_empty()
            || allocation.buffer.name.is_empty()
        {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: format!("allocation {id} has an incomplete static fact"),
            });
        }
        if allocation.lifetime != allocation.buffer.lifetime {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: format!("allocation {id} repeats a conflicting lifetime"),
            });
        }
    }
    Ok(allocations)
}

fn admit_views(
    payload: &WireExecutorPlanV2,
    allocations: &BTreeMap<u32, &WireAllocationFact>,
) -> Result<(), ExecutorPlanAdmissionError> {
    let mut view_keys = BTreeSet::new();
    for view in &payload.views {
        if !view_keys.insert((view.allocation, view.static_base, view.maximum_span)) {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: format!("allocation {} repeats an identical view", view.allocation),
            });
        }
        let Some(allocation) = allocations.get(&view.allocation) else {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: format!("view refers to unknown allocation {}", view.allocation),
            });
        };
        validate_view(view, allocation)?;
    }
    Ok(())
}

fn validate_view(
    view: &super::executor_plan::WireViewFact,
    allocation: &WireAllocationFact,
) -> Result<(), ExecutorPlanAdmissionError> {
    if view.logical_dims.is_empty()
        || view.logical_dims.len() != view.strides.len()
        || view.logical_dims.contains(&0)
        || view.strides.contains(&0)
        || view.maximum_span == 0
    {
        return Err(ExecutorPlanAdmissionError::InvalidFact {
            detail: format!("allocation {} has an invalid view shape", view.allocation),
        });
    }
    let dtype_width = dtype_width(&allocation.dtype);
    let required_elements = checked_view_elements(&view.logical_dims, &view.strides)?;
    let required_span = required_elements.checked_mul(dtype_width).ok_or(
        ExecutorPlanAdmissionError::ArithmeticOverflow {
            field: "view element count * dtype width",
        },
    )?;
    if view.maximum_span < required_span {
        return Err(ExecutorPlanAdmissionError::InvalidFact {
            detail: format!(
                "view maximum span {} is smaller than required span {required_span}",
                view.maximum_span
            ),
        });
    }
    let base_bytes = view.static_base.checked_mul(dtype_width).ok_or(
        ExecutorPlanAdmissionError::ArithmeticOverflow {
            field: "view static_base * dtype width",
        },
    )?;
    let end = base_bytes.checked_add(view.maximum_span).ok_or(
        ExecutorPlanAdmissionError::ArithmeticOverflow {
            field: "view base + maximum span",
        },
    )?;
    if end > allocation.capacity_bytes {
        return Err(ExecutorPlanAdmissionError::InvalidFact {
            detail: format!(
                "view end {end} exceeds allocation capacity {}",
                allocation.capacity_bytes
            ),
        });
    }
    Ok(())
}

fn checked_view_elements(dims: &[u64], strides: &[u64]) -> Result<u64, ExecutorPlanAdmissionError> {
    let max_element = dims
        .iter()
        .zip(strides)
        .try_fold(0u64, |acc, (dim, stride)| {
            dim.checked_sub(1)
                .and_then(|extent| extent.checked_mul(*stride))
                .and_then(|extent| acc.checked_add(extent))
        })
        .ok_or(ExecutorPlanAdmissionError::ArithmeticOverflow {
            field: "view dimension * stride",
        })?;
    max_element
        .checked_add(1)
        .ok_or(ExecutorPlanAdmissionError::ArithmeticOverflow {
            field: "view element span accumulation",
        })
}

fn dtype_width(dtype: &str) -> u64 {
    match dtype {
        "F16" => 2,
        // The producer's closed scalar surface uses F32/U32/I32. Unknown
        // future spellings are not guessed into a narrower width here; the
        // shared semantic admission owns the dtype allowlist.
        _ => 4,
    }
}

fn admit_launch_bindings(
    payload: &WireExecutorPlanV2,
    allocations: &BTreeMap<u32, &WireAllocationFact>,
    kernel_count: usize,
) -> Result<(), ExecutorPlanAdmissionError> {
    let mut binding_keys = BTreeSet::new();
    let mut has_runtime_source = false;
    for binding in &payload.launch_bindings {
        if usize::try_from(binding.kernel_index)
            .ok()
            .is_none_or(|index| index >= kernel_count)
        {
            return Err(ExecutorPlanAdmissionError::SelectionShape {
                detail: format!(
                    "launch binding references kernel {} outside {kernel_count} selections",
                    binding.kernel_index
                ),
            });
        }
        let key = (
            binding.kernel_index,
            binding.binding.group,
            binding.binding.binding,
        );
        if !binding_keys.insert(key) {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: format!(
                    "launch binding repeats kernel {} slot ({}, {})",
                    binding.kernel_index, binding.binding.group, binding.binding.binding
                ),
            });
        }
        let Some(allocation) = allocations.get(&binding.handle) else {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: format!(
                    "launch binding refers to unknown allocation {}",
                    binding.handle
                ),
            });
        };
        if binding.view_span == 0 {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: "launch binding view span must be nonzero".to_owned(),
            });
        }
        let end = binding.byte_offset.checked_add(binding.view_span).ok_or(
            ExecutorPlanAdmissionError::ArithmeticOverflow {
                field: "launch byte_offset + view_span",
            },
        )?;
        if end > allocation.capacity_bytes {
            return Err(ExecutorPlanAdmissionError::InvalidFact {
                detail: format!(
                    "launch binding end {end} exceeds allocation capacity {}",
                    allocation.capacity_bytes
                ),
            });
        }
        admit_source(&binding.runtime_source)?;
        has_runtime_source |= matches!(
            binding.runtime_source,
            WireExecutorPlanSource::Runtime { .. }
        );
    }
    if !has_runtime_source {
        return Err(ExecutorPlanAdmissionError::InvalidFact {
            detail: "v2 payload carries no tagged runtime source".to_owned(),
        });
    }
    Ok(())
}

fn admit_source(source: &WireExecutorPlanSource) -> Result<(), ExecutorPlanAdmissionError> {
    if let WireExecutorPlanSource::Runtime { source } = source {
        admit_runtime_source(source)?;
    }
    Ok(())
}

fn admit_runtime_source(source: &WireRuntimeSource) -> Result<(), ExecutorPlanAdmissionError> {
    if let WireRuntimeSource::Unknown(source) = source {
        return Err(ExecutorPlanAdmissionError::UnknownRuntimeSource {
            source: source.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::executor_plan::{
        WireAllocationFact, WireAllocationIdentity, WireBackendRequirement, WireExecutorPlan,
        WireExecutorPlanV2, WireLogicalSignature, WirePlanProvenance,
    };
    use crate::schema::wire::{
        WireBinding, WireBufferLifetime, WireBufferRole, WireInitializationPolicy,
        WireStorageLayout,
    };

    fn payload() -> WireExecutorPlanV2 {
        WireExecutorPlanV2 {
            abi_version: super::super::executor_plan::EXECUTOR_PLAN_V2_VERSION,
            selections: vec![WireExecutorPlan {
                kernel_index: 0,
                library_entry: Some("kv_append".to_owned()),
                logical_signature: WireLogicalSignature {
                    operation: "kv_append".to_owned(),
                    inputs: vec!["k".to_owned()],
                    outputs: vec!["cache_k".to_owned()],
                },
                primary_bind: super::super::executor_plan::WireBindDescriptor {
                    dims: vec![2, 4],
                    strides: vec![4, 1],
                    layout: "strided".to_owned(),
                    grid: [1, 1, 1],
                },
                binds: Vec::new(),
                uniforms: Vec::new(),
                offsets: Vec::new(),
                workspace: None,
                backend_requirements: vec![WireBackendRequirement {
                    backend: "metal".to_owned(),
                    feature: "buffer-offsets".to_owned(),
                }],
                provenance: WirePlanProvenance {
                    pass: "kv-cache-plan".to_owned(),
                    revision: 2,
                    source_nodes: vec![17],
                },
            }],
            allocations: vec![WireAllocationFact {
                buffer: WireAllocationIdentity {
                    id: 7,
                    name: "cache_k".to_owned(),
                    role: WireBufferRole::InOut,
                    storage: WireStorageLayout::DeviceHandle,
                    lifetime: WireBufferLifetime::PerProgram,
                },
                dtype: "F32".to_owned(),
                capacity_bytes: 4096,
                lifetime: WireBufferLifetime::PerProgram,
                initialization: WireInitializationPolicy::ZeroFill,
            }],
            views: vec![
                super::super::executor_plan::WireViewFact {
                    allocation: 7,
                    logical_dims: vec![2, 4],
                    strides: vec![4, 1],
                    static_base: 0,
                    maximum_span: 32,
                },
                super::super::executor_plan::WireViewFact {
                    allocation: 7,
                    logical_dims: vec![1, 4],
                    strides: vec![4, 1],
                    static_base: 4,
                    maximum_span: 16,
                },
            ],
            launch_bindings: vec![super::super::executor_plan::WireLaunchBinding {
                kernel_index: 0,
                handle: 7,
                binding: WireBinding {
                    group: 0,
                    binding: 3,
                },
                byte_offset: 0,
                view_span: 32,
                runtime_source: WireExecutorPlanSource::Runtime {
                    source: WireRuntimeSource::Position,
                },
            }],
        }
    }

    #[test]
    fn v2_runtime_sources_and_views_round_trip() {
        let original = payload();
        let bytes = postcard::to_allocvec(&original).expect("v2 payload serializes");
        let decoded: WireExecutorPlanV2 =
            postcard::from_bytes(&bytes).expect("v2 payload deserializes");
        assert_eq!(decoded, original);
        admit_executor_plan_v2(&decoded, 1).expect("round-tripped v2 payload admits");
    }

    #[test]
    fn producer_v2_facts_project_to_the_fmir_mirror() {
        let producer = radix_mir::device_program_plans::ExecutorPlanExtensionV2 {
            abi_version: radix_mir::device_program_plans::EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION,
            selections: Vec::new(),
            allocations: vec![radix_mir::device_program_plans::AllocationFact {
                buffer: radix_mir::BufferIdentity {
                    id: radix_mir::BufferId(7),
                    name: "cache_k".to_owned(),
                    role: radix_mir::BufferRole::InOut,
                    storage: radix_mir::MirTensorStorageLayout::DeviceHandle,
                    lifetime: radix_mir::BufferLifetime::PerProgram,
                },
                dtype: radix_mir::UniformType::F32,
                capacity_bytes: 4096,
                lifetime: radix_mir::BufferLifetime::PerProgram,
                initialization: radix_mir::device_semantics::InitializationPolicy::ZeroFill,
            }],
            views: vec![radix_mir::device_program_plans::ViewFact {
                allocation: radix_mir::BufferId(7),
                logical_dims: vec![2, 4],
                strides: vec![4, 1],
                static_base: 0,
                maximum_span: 32,
            }],
            launch_bindings: vec![radix_mir::device_program_plans::LaunchBinding::new(
                0,
                radix_mir::BufferId(7),
                radix_mir::Binding {
                    group: 0,
                    binding: 3,
                },
                0,
                32,
                radix_mir::device_program_plans::ExecutorPlanSource::runtime(
                    radix_mir::device_program_plans::RuntimeSource::Position,
                ),
            )],
        };
        let mirror = WireExecutorPlanV2::from_mir(&producer);
        assert_eq!(mirror.abi_version, 2);
        assert_eq!(mirror.allocations[0].buffer.id, 7);
        assert_eq!(mirror.views[0].maximum_span, 32);
        assert!(matches!(
            mirror.launch_bindings[0].runtime_source,
            WireExecutorPlanSource::Runtime {
                source: WireRuntimeSource::Position
            }
        ));
    }

    #[test]
    fn unknown_runtime_source_fails_closed() {
        let mut payload = payload();
        payload.launch_bindings[0].runtime_source = WireExecutorPlanSource::Runtime {
            source: WireRuntimeSource::Unknown("future_cursor".to_owned()),
        };
        assert!(matches!(
            admit_executor_plan_v2(&payload, 1),
            Err(ExecutorPlanAdmissionError::UnknownRuntimeSource { source })
                if source == "future_cursor"
        ));
    }

    #[test]
    fn overflowing_view_span_fails_closed() {
        let mut payload = payload();
        payload.views[0].logical_dims = vec![u64::MAX];
        payload.views[0].strides = vec![2];
        payload.views[0].maximum_span = u64::MAX;
        assert!(matches!(
            admit_executor_plan_v2(&payload, 1),
            Err(ExecutorPlanAdmissionError::ArithmeticOverflow { .. })
        ));
    }

    #[test]
    fn overflowing_launch_span_fails_closed() {
        let mut payload = payload();
        payload.launch_bindings[0].byte_offset = u64::MAX - 1;
        payload.launch_bindings[0].view_span = 2;
        assert!(matches!(
            admit_executor_plan_v2(&payload, 1),
            Err(ExecutorPlanAdmissionError::ArithmeticOverflow { .. })
        ));
    }

    #[test]
    fn payload_version_skew_fails_before_field_admission() {
        let mut payload = payload();
        payload.abi_version = 1;
        payload.selections.clear();
        assert!(matches!(
            admit_executor_plan_v2(&payload, 1),
            Err(ExecutorPlanAdmissionError::UnsupportedVersion {
                actual: 1,
                expected: 2
            })
        ));
    }

    #[test]
    fn outer_section_version_skew_fails_before_payload_admission() {
        let payload = payload();
        assert!(matches!(
            admit_executor_plan_section_v2_with_version(1, &payload, 1),
            Err(ExecutorPlanAdmissionError::UnsupportedSectionVersion {
                actual: 1,
                expected: 2
            })
        ));
    }

    #[test]
    fn v1_static_plan_has_no_runtime_source_shape() {
        let legacy = WireExecutorPlan {
            kernel_index: 0,
            library_entry: None,
            logical_signature: WireLogicalSignature {
                operation: "legacy".to_owned(),
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
            primary_bind: super::super::executor_plan::WireBindDescriptor {
                dims: vec![1],
                strides: vec![1],
                layout: "flat".to_owned(),
                grid: [1, 1, 1],
            },
            binds: Vec::new(),
            uniforms: vec![super::super::executor_plan::WireUniformBinding {
                name: "constant".to_owned(),
                ty: "U32".to_owned(),
                bits: 7,
            }],
            offsets: Vec::new(),
            workspace: None,
            backend_requirements: Vec::new(),
            provenance: WirePlanProvenance {
                pass: "legacy".to_owned(),
                revision: 1,
                source_nodes: vec![1],
            },
        };
        assert_eq!(legacy.uniforms[0].bits, 7);
    }
}
