//! Parsed Faber syntax tree surface (foundation crate).
//!
//! Package `radix` re-exports this crate as `radix::syntax`.
//!
//! This module is the contract between token-level parsing and every later
//! compiler phase. The parser builds these nodes as a faithful, untyped record
//! of the source grammar; semantic analysis, lowering, diagnostics, and tooling
//! consume the same data without needing to know parser internals.
//!
//! The boundary is deliberately syntactic. A node may carry parse-time shape,
//! source spans, annotations, and stable [`NodeId`]s, but it does not assert
//! name resolution, inferred types, backend legality, or target meaning. Those
//! facts belong to later phase tables keyed by the syntax tree.
//!
//! INVARIANTS
//! ==========
//! - Statements and expressions have `NodeId`s suitable for phase-local side
//!   tables.
//! - Spans always describe source locations for diagnostics, not ownership or
//!   semantic extent.
//! - Annotation syntax is preserved here even when its meaning is interpreted by
//!   lowering, package tooling, or backend codegen.
//! - Visitor helpers are traversal utilities, not a completeness guarantee for
//!   semantic analysis.

mod annotation_promotion;
mod ast;
mod braced_annotation;
mod span;
mod trivia;
mod visit;

pub use annotation_promotion::{
    AnnotationView, InvalidAnnotationView, NondumView, RadixLaneView, RadixTypusView, VerteView,
    annotation_has_protecta, braced_nondum_view, braced_radix_lane_view,
    braced_radix_lane_view_strict, braced_radix_typus_view, braced_radix_typus_view_strict,
    braced_verte_view, nondum_from_stmt, radix_lane_from_stmt, verte_from_stmt,
    verte_rust_methodus,
};
pub use ast::*;
pub use braced_annotation::{
    BracedAnnotationError, BracedOperandusView, BracedOptioView, BracedView, braced_cli_nomen,
    braced_cli_nomen_strict, braced_family_is, braced_imperium_nomen, braced_imperium_nomen_strict,
    braced_operandus_view, braced_operandus_view_strict, braced_optio_view,
    braced_optio_view_strict,
};
pub use span::Spanned;
pub use trivia::{CommentLine, Newline, Trivia};
pub use visit::{DepthExceeded, MAX_VISIT_DEPTH, Visitor, walk_expr, walk_stmt};
