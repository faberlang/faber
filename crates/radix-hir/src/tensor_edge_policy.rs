//! Resolved edge-policy identities retained from
//! `tensor.disloco(...).limes<P>()` calls.
//!
//! This semantic sidecar is neutral HIR input shared by every backend. A
//! custom policy is a compile-time dispatch selector; its checked ordinary
//! methods are retained here even though source syntax has no direct call
//! expression for them at the `limes` site.

use crate::{DefId, HirId};
use radix_types::{BuiltinTensorEdgePolicy, IndexId, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};

/// One statically selected policy for a shifted tensor view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedTensorEdgePolicy {
    /// Compiler-owned shape-aware policy selector.
    Builtin(BuiltinTensorEdgePolicy),
    /// Validated ordinary methods on a zero-field custom policy class.
    Custom {
        policy_def: DefId,
        remap: DefId,
        transform: Option<DefId>,
    },
    /// A generic type parameter whose concrete policy is known only after
    /// specialization. Consumers must resolve it at that boundary.
    Deferred { policy_type: radix_types::TypeId },
}

/// Shape and displacement source for an unresolved shifted tensor view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorShiftContext {
    /// Shape of the source tensor before the shift.
    pub source_shape: IndexId,
    /// HIR identity of the exact offset-list expression passed to `disloco`.
    pub shift_expression: HirId,
}

/// One generic function obligation to forward a statically ranked provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TensorEdgePolicyRequirement {
    /// Formal or concrete policy identity in this HIR unit's type arena.
    pub policy_type: TypeId,
    /// Tensor element identity carried by the limes operation.
    pub element_type: TypeId,
    /// Statically known tensor rank; runtime extents are deliberately absent.
    pub rank: usize,
}

/// One validated concrete provider dispatch retained for Rust codegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorEdgePolicyProviderImpl {
    /// Concrete policy identity in this unit's type arena.
    pub policy_type: TypeId,
    /// Tensor element identity accepted by the checked provider methods.
    pub element_type: TypeId,
    /// Statically known tensor rank.
    pub rank: usize,
    /// Resolved policy plus validated callback identities when custom.
    pub policy: ResolvedTensorEdgePolicy,
}

/// Per-call policy dispatch metadata and retained shifted-view provenance.
///
/// The table belongs in `radix-hir` because codegen consumes it alongside
/// HIR, while semantic analysis remains its producer. The semantic crate
/// re-exports these definitions to preserve its established API paths.
///
/// It also carries the resolved `ordo` order rows for enum `≺ ≻ ≤ ≥`
/// comparisons: this is the one analyzed sidecar this crate
/// threads from typecheck to MIR lowering, so an ordering decision travels the
/// same seam as the edge-policy facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TensorEdgePolicyTable {
    /// Policy dispatch metadata keyed by the outer `limes` HIR id.
    pub calls: FxHashMap<HirId, ResolvedTensorEdgePolicy>,
    /// HIR values whose physical tensor carrier still has an unresolved shift.
    /// This survives expression composition and local binding so `limes` can
    /// distinguish an edge view from an ordinary nullable-element tensor.
    pub shifted_expressions: FxHashSet<HirId>,
    /// Shift provenance keyed by the shifted expression identity.
    pub shifted_view_expressions: FxHashMap<HirId, TensorShiftContext>,
    /// Local bindings that carry an unresolved shifted tensor view.
    pub shifted_bindings: FxHashSet<DefId>,
    /// Shift provenance retained by local bindings.
    pub shifted_view_bindings: FxHashMap<DefId, TensorShiftContext>,
    /// Exact pre-shift shape and offset expression for each checked limes call.
    pub limes_contexts: FxHashMap<HirId, TensorShiftContext>,
    /// Generic provider obligations retained per function. Multiple rows are
    /// valid when independent policies, element types, or ranks are used.
    pub function_requirements: FxHashMap<DefId, Vec<TensorEdgePolicyRequirement>>,
    /// Concrete, validated provider implementations available to codegen.
    pub provider_impls: Vec<TensorEdgePolicyProviderImpl>,
    /// `ordo` ordering rows for enum `≺ ≻ ≤ ≥` comparisons.
    pub ordo_order: crate::OrdoOrderTable,
}
