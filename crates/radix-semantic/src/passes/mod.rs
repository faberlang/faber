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
//!    [`filum_independence`] then admits independent iteration on every target.
//! 5. [`air_unroll`] rewrites eligible AIR-lane concrete-`N` `itera ab` range
//!    loops into straight-line bindings before purity admission.
//! 6. [`air_purity`] is the fail-closed AIR-lane subset gate.
//! 7. [`borrow_region`] builds the HIR CFG and computes conservative
//!    flow-sensitive region facts from the shared access-fact table.
//! 8. [`borrow`] runs target-gated ownership checks where required, currently
//!    for Rust output.
//! 9. [`definite_assignment`] rejects reads of uninitialized non-`sponte`
//!    bindings with conservative branch precision.
//! 10. [`exhaustive`] reports pattern coverage issues.
//! 11. [`return_path`] rejects value-returning functions whose bodies can fall
//!     through without returning.
//! 12. [`fragment_definition`] checks `@ nucleum fragment` bodies against the
//!     device-safe effect subset and records a per-definition effect summary.
//! 13. [`kernel_legality`] decides `@ nucleum` body legality (gpu-reset
//!     rule 12) and call legality between kernels and host code.
//! 14. [`lint`] reports quality diagnostics that may be warnings rather than
//!     hard semantic errors.
//!
//! ERROR POLICY
//! ============
//! Collect and resolve return accumulated hard errors; callers stop before HIR
//! lowering when those errors exist because unresolved names would make later
//! phase identities unreliable. HIR-phase passes can contribute warnings as
//! well as errors, and the driver decides success from the final diagnostic
//! severities rather than from pass execution alone.

pub mod ad_routes;
pub mod air_closure_lift;
pub mod air_purity;
pub mod air_specialize;
pub mod air_tensor_intrinsics;
pub mod air_unroll;
pub mod borrow;
pub mod borrow_region;
pub mod borrow_region_cfg;
pub mod collect;
pub mod companion_predeclare;
pub mod companion_signature;
pub mod complexity;
pub(crate) mod coverage;
pub mod decimal_integer_literal;
pub mod definite_assignment;
pub mod device_region;
pub mod exhaustive;
pub(crate) mod filum_independence;
pub mod float_literal_fold;
pub mod fragment_definition;
pub mod import_path;
pub mod import_seam;
pub mod import_target;
pub mod insere_fold;
pub mod kernel_legality;
mod kernel_legality_body;
mod kernel_legality_types;
pub mod kernel_namespace;
pub mod kernel_requirements;
pub mod kernel_tier;
pub mod leading_underscore;
pub mod ledger_gate;
pub mod lint;
pub mod live_tuus_cursor_escape;
pub mod local_constants;
mod method_receiver_mutation;
pub mod praefixum;
pub mod resolve;
pub mod return_path;
pub mod statica;
pub(crate) mod typecheck;
pub use typecheck::integer_range::{const_int_exact, const_int_value, is_const_int};
pub use typecheck::{
    binary_twin_name, in_place_binary_twin_name, in_place_unary_twin_name, unary_twin_name,
};
pub mod visibility_annotation;

#[cfg(test)]
pub use crate::{
    DefId, Primitive, Resolver, SemanticError, SemanticErrorKind, SymbolKind, Type, TypeId,
    TypeTable, WarningKind,
};

#[cfg(test)]
#[path = "air_closure_lift_test.rs"]
mod air_closure_lift_test;

#[cfg(test)]
#[path = "air_tensor_intrinsics_test.rs"]
mod air_tensor_intrinsics_test;

#[cfg(test)]
#[path = "air_purity_test.rs"]
mod air_purity_test;

#[cfg(test)]
#[path = "inf_type_test.rs"]
mod inf_type_test;

#[cfg(test)]
#[path = "air_unroll_test.rs"]
mod air_unroll_test;

#[cfg(test)]
#[path = "borrow_test.rs"]
mod borrow_test;

#[cfg(test)]
#[path = "borrow_alias_test.rs"]
mod borrow_alias_test;

#[cfg(test)]
#[path = "borrow_nonmut_tensor_write_test.rs"]
mod borrow_nonmut_tensor_write_test;

#[cfg(test)]
#[path = "borrow_unmarked_alias_test.rs"]
mod borrow_unmarked_alias_test;

#[cfg(test)]
#[path = "borrow_mut_argument_test.rs"]
mod borrow_mut_argument_test;

#[cfg(test)]
#[path = "borrow_receiver_write_test.rs"]
mod borrow_receiver_write_test;

#[cfg(test)]
#[path = "lint_mut_arg_test.rs"]
mod lint_mut_arg_test;

#[cfg(test)]
#[path = "lint_tensor_use_test.rs"]
mod lint_tensor_use_test;

#[cfg(test)]
#[path = "borrow_non_null_receiver_test.rs"]
mod borrow_non_null_receiver_test;

#[cfg(test)]
#[path = "kernel_arg_exclusivity_test.rs"]
mod kernel_arg_exclusivity_test;

#[cfg(test)]
#[path = "borrow_alias_call_test.rs"]
mod borrow_alias_call_test;

#[cfg(test)]
#[path = "borrow_alias_call_types_test.rs"]
mod borrow_alias_call_types_test;

#[cfg(test)]
#[path = "borrow_alias_container_test.rs"]
mod borrow_alias_container_test;

#[cfg(test)]
#[path = "borrow_alias_symmetry_test.rs"]
mod borrow_alias_symmetry_test;

#[cfg(test)]
#[path = "borrow_alias_register_option_test.rs"]
mod borrow_alias_register_option_test;

#[cfg(test)]
#[path = "borrow_region_test.rs"]
mod borrow_region_test;

#[cfg(test)]
#[path = "borrow_region_stress_test.rs"]
mod borrow_region_stress_test;

#[cfg(test)]
#[path = "borrow_region_parity_test.rs"]
mod borrow_region_parity_test;

#[cfg(test)]
#[path = "borrow_region_coverage_test.rs"]
mod borrow_region_coverage_test;

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
#[path = "complexity_test.rs"]
mod complexity_test;

#[cfg(test)]
#[path = "copy_into_test.rs"]
mod copy_into_test;

#[cfg(test)]
#[path = "sink_into_test.rs"]
mod sink_into_test;

#[cfg(test)]
#[path = "definite_assignment_test.rs"]
mod definite_assignment_test;
#[cfg(test)]
#[path = "method_receiver_mutation_test.rs"]
mod method_receiver_mutation_test;

#[cfg(test)]
#[path = "exhaustive_coverage_test.rs"]
mod exhaustive_coverage_test;
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
#[path = "itera_binder_test.rs"]
mod itera_binder_test;

#[cfg(test)]
#[path = "itera_binder_mutability_test.rs"]
mod itera_binder_mutability_test;

#[cfg(test)]
#[path = "typecheck/iuncta_hole_test.rs"]
mod iuncta_hole_test;

#[cfg(test)]
#[path = "kernel_legality_test.rs"]
mod kernel_legality_test;

#[cfg(test)]
#[path = "device_region_test.rs"]
mod device_region_test;

#[cfg(test)]
#[path = "kernel_requirements_test.rs"]
mod kernel_requirements_test;

#[cfg(test)]
#[path = "kernel_tier_test.rs"]
mod kernel_tier_test;

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
#[path = "value_marker_test.rs"]
mod value_marker_test;

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
#[path = "own_param_writable_test.rs"]
mod own_param_writable_test;
#[cfg(test)]
#[path = "ref_local_test.rs"]
mod ref_local_test;

#[cfg(test)]
#[path = "call_marker_test.rs"]
mod call_marker_test;
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
#[path = "ledger_gate_test.rs"]
mod ledger_gate_test;

#[cfg(test)]
#[path = "local_constants_test.rs"]
mod local_constants_test;

#[cfg(test)]
#[path = "module_constant_test.rs"]
mod module_constant_test;

#[cfg(test)]
#[path = "praefixum_local_test.rs"]
mod praefixum_local_test;
#[cfg(test)]
#[path = "praefixum_test.rs"]
mod praefixum_test;
#[cfg(test)]
#[path = "statica_test.rs"]
mod statica_test;
#[cfg(test)]
#[path = "summa_disambiguation_test.rs"]
mod summa_disambiguation_test;

#[cfg(test)]
#[path = "visibility_annotation_test.rs"]
mod visibility_annotation_test;

#[cfg(test)]
#[path = "k8_filum_calls_test.rs"]
mod k8_filum_calls_test;

#[cfg(test)]
#[path = "k8_filum_foreign_test.rs"]
mod k8_filum_foreign_test;

#[cfg(test)]
#[path = "k8_filum_independence_test.rs"]
mod k8_filum_independence_test;

#[cfg(test)]
#[path = "k8_filum_methods_test.rs"]
mod k8_filum_methods_test;

#[cfg(test)]
#[path = "filum_typed_zero_scratch_test.rs"]
mod filum_typed_zero_scratch_test;

#[cfg(test)]
#[path = "filum_ends_test.rs"]
mod filum_ends_test;

#[cfg(test)]
#[path = "k8_filum_nested_test.rs"]
mod k8_filum_nested_test;

#[cfg(test)]
#[path = "k_for_range_test.rs"]
mod k_for_range_test;

#[cfg(test)]
#[path = "k8_filum_storage_test.rs"]
mod k8_filum_storage_test;

#[cfg(test)]
#[path = "k8_filum_invariant_test.rs"]
mod k8_filum_invariant_test;

#[cfg(test)]
#[path = "borrow_own_arg_test.rs"]
mod borrow_own_arg_test;
