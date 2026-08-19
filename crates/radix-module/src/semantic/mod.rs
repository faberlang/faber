//! Semantic analysis — re-exported from leaf `radix-semantic`.
//!
//! The production module tree (passes, typecheck, satellites) lives in the
//! `radix-semantic` crate. This skeleton keeps the `crate::semantic::*` path
//! stable for driver/codegen/MIR/services consumers and hosts the
//! driver-integration test suites that cannot live in the leaf.

pub use crate::tensor_goal_gate;
pub use radix_semantic::*;

// Local test-host modules shadow the glob-imported leaf modules so the
// hosted driver-integration suites compile; each host re-exports the leaf
// surface it shadows.
#[cfg(test)]
#[path = "passes/mod.rs"]
pub mod passes;

#[cfg(test)]
#[path = "annotation_contract_test.rs"]
mod tests;

#[cfg(test)]
#[path = "analyzed_application_test.rs"]
mod analyzed_application_host_tests;

#[cfg(test)]
#[path = "tensor_bracket_test.rs"]
mod tensor_bracket_host_tests;

#[cfg(test)]
#[path = "format_type_test.rs"]
mod format_type_host_tests;

#[cfg(test)]
#[path = "index_infer_test.rs"]
mod index_infer_host_tests;

#[cfg(test)]
#[path = "figura_test.rs"]
mod figura_host_tests;

#[cfg(test)]
#[path = "figura_literal_test.rs"]
mod literal_tests;

#[cfg(test)]
#[path = "types_test.rs"]
mod types_host_tests;

#[cfg(test)]
#[path = "broadcast_reduction_test.rs"]
mod broadcast_reduction_host_tests;

#[cfg(test)]
#[path = "error_test.rs"]
mod error_host_tests;

#[cfg(test)]
#[path = "function_facts_test.rs"]
mod function_facts_host_tests;

#[cfg(test)]
#[path = "function_facts_propagate_worklist_test.rs"]
mod propagate_failable_worklist_tests;

#[cfg(test)]
#[path = "index_test.rs"]
mod index_host_tests;

#[cfg(test)]
#[path = "numeric_width_test.rs"]
mod numeric_width_host_tests;

#[cfg(test)]
#[path = "radix_lane_test.rs"]
mod radix_lane_host_tests;

#[cfg(test)]
#[path = "scope_test.rs"]
mod scope_host_tests;

#[cfg(test)]
#[path = "init_state_test.rs"]
mod init_state_host_tests;

#[cfg(test)]
#[path = "g2_policy_test.rs"]
mod g2_policy_test;

#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_host_tests;

#[cfg(test)]
#[path = "lista_tensor_conversio_test.rs"]
mod lista_tensor_conversio_host_tests;

#[cfg(test)]
#[path = "def_id_closure_test.rs"]
mod def_id_closure_host_tests;

#[cfg(test)]
#[path = "vector_width_test.rs"]
mod vector_width_host_tests;

#[cfg(test)]
#[path = "index_unify_test.rs"]
mod index_unify_host_tests;

#[cfg(test)]
#[path = "tensor_element_lattice_test.rs"]
mod tensor_element_lattice_host_tests;

#[cfg(test)]
#[path = "instans_precision_test.rs"]
mod instans_precision_host_tests;
