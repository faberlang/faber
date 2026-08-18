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
//! 5. [`borrow`] runs target-gated ownership checks where required, currently
//!    for Rust output.
//! 6. [`definite_assignment`] rejects reads of uninitialized non-`sponte`
//!    bindings with conservative branch precision.
//! 7. [`exhaustive`] reports pattern coverage issues.
//! 8. [`return_path`] rejects value-returning functions whose bodies can fall
//!    through without returning.
//! 9. [`fragment_definition`] checks `@ nucleum fragment` bodies against the
//!    device-safe effect subset and records a per-definition effect summary.
//! 10. [`lint`] reports quality diagnostics that may be warnings rather than
//!    hard semantic errors.
//!
//! ERROR POLICY
//! ============
//! Collect and resolve return accumulated hard errors; callers stop before HIR
//! lowering when those errors exist because unresolved names would make later
//! phase identities unreliable. HIR-phase passes can contribute warnings as
//! well as errors, and the driver decides success from the final diagnostic
//! severities rather than from pass execution alone.

pub mod air_purity;
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
pub mod typecheck;
pub mod visibility_annotation;
