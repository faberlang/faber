//! Runtime shape relations retained for compiler boundaries.
//!
//! Typechecking discharges relations that are statically known. Relations
//! whose equality depends on runtime extents remain here for MIR/backend
//! lowering, where they must use the callable's existing recoverable error
//! route.

use crate::IndexId;
use handroll::FxHashMap;
use radix_hir::{DefId, HirId};
use radix_lexer::Span;

/// Equality required between two dimension expressions at a runtime boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeRelation {
    /// Left dimension in the shared semantic index arena.
    pub left: IndexId,
    /// Right dimension in the shared semantic index arena.
    pub right: IndexId,
    /// Source site that introduced this boundary relation.
    pub span: Span,
    /// Boundary where this equation is checked.
    pub boundary: ShapeRelationBoundary,
}

/// Runtime source for one generic size parameter, kept separate from the
/// generic instance key. The key uses the formal parameter identity; this row
/// tells lowering where that parameter's value comes from for this invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeSizeBinding {
    /// Ordinal in the complete generic descriptor list (type and size params).
    pub parameter: u32,
    /// Runtime-origin dimension expression that witnesses this size.
    pub witness: IndexId,
    /// Declaration/call site that provides the witness.
    pub span: Span,
}

/// Runtime location for a retained shape equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeRelationBoundary {
    /// Before entering the callee, after evaluating call arguments.
    Call,
    /// At function entry, against formal carrier axes.
    FunctionEntry,
    /// Before returning an expression-produced value.
    FunctionReturn,
    /// At a declaration store pinning opaque initializer axes to the
    /// annotation's named size terms.
    Store,
}

/// Runtime shape checks produced by typechecking.
///
/// Call-site rows are keyed by the stable HIR expression identity. Function
/// rows cover parameter-entry, field/record, and return boundaries and are
/// keyed by the owning declaration identity. Dimension provenance stays in
/// `IndexExpr::Runtime` terms referenced by each relation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShapeObligationTable {
    /// Relations checked before a call is entered.
    pub calls: FxHashMap<HirId, Vec<ShapeRelation>>,
    /// Per-call runtime witnesses for generic size parameters.
    pub call_size_bindings: FxHashMap<HirId, Vec<RuntimeSizeBinding>>,
    /// Relations checked at function entry or before returning a value.
    pub functions: FxHashMap<DefId, Vec<ShapeRelation>>,
    /// Relations checked when a declaration store pins opaque initializer
    /// axes to the annotation's size terms; keyed by initializer expression.
    pub stores: FxHashMap<HirId, Vec<ShapeRelation>>,
    /// Function-generic size values witnessed by formal tensor/record axes.
    pub function_size_bindings: FxHashMap<DefId, Vec<RuntimeSizeBinding>>,
}
