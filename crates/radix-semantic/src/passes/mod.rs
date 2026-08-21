//! Semantic pass registry and execution-order map.
//!
//! This module is the index for the compiler phases that run after parsing and
//! before backend code generation. The first two passes, [`collect`] and
//! [`resolve`], operate directly on the parsed AST: they establish the symbol
//! universe, lexical scope tree, and type-alias entries needed before HIR
//! lowering can attach stable semantic identities. The later passes consume HIR
//! and progressively add type information, target-sensitive analysis, and
//! non-fatal diagnostics.
//!
//! PASS ORDER
//! ==========
//! 1. [`collect`] registers top-level AST declarations and imported module
//!    bindings in the global resolver scope.
//! 2. [`resolve`] walks AST bodies, checks lexical name/type availability, and
//!    lowers type aliases into the shared type table.
//! 3. HIR lowering happens outside this module, after collect/resolve errors
//!    have been reported.
//! 4. [`typecheck`] assigns and verifies expression/statement types on HIR.
//! 5. [`air_unroll`] rewrites eligible AIR-lane concrete-`N` `itera ab` range
//!    loops into straight-line bindings before purity admission.
//! 6. [`air_purity`] is the fail-closed AIR-lane subset gate.
//! 7. [`borrow`] runs target-gated ownership checks where required, currently
//!    for Rust output.
//! 8. [`definite_assignment`] rejects reads of uninitialized non-`sponte`
//!    bindings with conservative branch precision.
//! 9. [`exhaustive`] reports pattern coverage issues.
//! 10. [`return_path`] rejects value-returning functions whose bodies can fall
//!     through without returning.
//! 11. [`fragment_definition`] checks `@ nucleum fragment` bodies against the
//!     device-safe effect subset and records a per-definition effect summary.
//! 12. [`lint`] reports quality diagnostics that may be warnings rather than
//!     hard semantic errors.
//!
//! ERROR POLICY
//! ============
//! Collect and resolve return accumulated hard errors; callers stop before HIR
//! lowering when those errors exist because unresolved names would make later
//! phase identities unreliable. HIR-phase passes can contribute warnings as
//! well as errors, and the driver decides success from the final diagnostic
//! severities rather than from pass execution alone.

pub mod air_purity;
pub mod air_unroll;
pub mod borrow;
pub mod collect;
pub mod companion_predeclare;
pub mod companion_signature;
pub mod definite_assignment;
pub mod exhaustive;
pub mod fragment_definition;
pub mod import_path;
pub mod import_seam;
pub mod kernel_namespace;
pub mod lint;
pub mod live_tuus_cursor_escape;
pub mod resolve;
pub mod return_path;
pub(crate) mod typecheck;
pub mod visibility_annotation;

#[cfg(test)]
pub use crate::{
    DefId, Primitive, Resolver, SemanticError, SemanticErrorKind, SymbolKind, Type, TypeId,
    TypeTable, WarningKind,
};

#[cfg(test)]
#[path = "air_purity_test.rs"]
mod air_purity_test;

#[cfg(test)]
#[path = "air_unroll_test.rs"]
mod air_unroll_test;

#[cfg(test)]
#[path = "borrow_test.rs"]
mod borrow_test;

#[cfg(test)]
#[path = "collect_test.rs"]
mod collect_test;

#[cfg(test)]
#[path = "companion_predeclare_test.rs"]
mod companion_predeclare_test;

#[cfg(test)]
#[path = "companion_signature_test.rs"]
mod companion_signature_test;

#[cfg(test)]
#[path = "definite_assignment_test.rs"]
mod definite_assignment_test;

#[cfg(test)]
#[path = "exhaustive_test.rs"]
mod exhaustive_test;

#[cfg(test)]
#[path = "fragment_definition_test.rs"]
mod fragment_definition_test;

#[cfg(test)]
#[path = "import_path_test.rs"]
mod import_path_test;

#[cfg(test)]
#[path = "typecheck/iuncta_hole_test.rs"]
mod iuncta_hole_test;

#[cfg(test)]
#[path = "kernel_namespace_test.rs"]
mod kernel_namespace_test;

#[cfg(test)]
#[path = "lint_test.rs"]
mod lint_test;

#[cfg(test)]
#[path = "mutability_baseline_test.rs"]
mod mutability_baseline_test;

#[cfg(test)]
#[path = "mutability_markers_test.rs"]
mod mutability_markers_test;

#[cfg(test)]
#[path = "mutability_ref_test.rs"]
mod mutability_ref_test;

#[cfg(test)]
#[path = "mutability_own_test.rs"]
mod mutability_own_test;

#[cfg(test)]
#[path = "mutability_copy_test.rs"]
mod mutability_copy_test;

#[cfg(test)]
#[path = "mutability_mut_test.rs"]
mod mutability_mut_test;

#[cfg(test)]
#[path = "param_mode_test.rs"]
mod param_mode_test;

#[cfg(test)]
#[path = "resolve_test.rs"]
mod resolve_test;

#[cfg(test)]
#[path = "return_path_test.rs"]
mod return_path_test;

#[cfg(test)]
#[path = "schema_resolve_test.rs"]
mod schema_resolve_test;

#[cfg(test)]
#[path = "typecheck_test.rs"]
mod typecheck_test;

#[cfg(test)]
#[path = "typecheck/iuncta_label_test.rs"]
mod iuncta_label_test;

#[cfg(test)]
#[path = "typecheck/ratio_test.rs"]
mod ratio_test;

#[cfg(test)]
#[path = "typecheck/ratio_hole_test.rs"]
mod ratio_hole_test;

#[cfg(test)]
#[path = "visibility_annotation_test.rs"]
mod visibility_annotation_test;
