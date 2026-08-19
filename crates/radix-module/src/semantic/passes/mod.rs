//! Semantic passes — re-exported from `radix-semantic::passes` (test host).

pub use radix_semantic::passes::*;
pub use radix_semantic::*;

#[cfg(test)]
#[path = "visibility_annotation_test.rs"]
mod tests;

#[cfg(test)]
#[path = "air_unroll_test.rs"]
mod air_unroll_host_tests;

#[cfg(test)]
#[path = "resolve_test.rs"]
mod resolve_host_tests;

#[cfg(test)]
#[path = "schema_resolve_test.rs"]
mod schema_resolve_tests;

#[cfg(test)]
#[path = "import_path_test.rs"]
mod import_path_host_tests;

#[cfg(test)]
#[path = "air_purity_test.rs"]
mod air_purity_host_tests;

#[cfg(test)]
#[path = "collect_test.rs"]
mod collect_host_tests;

#[cfg(test)]
#[path = "borrow_test.rs"]
mod borrow_host_tests;

#[cfg(test)]
#[path = "lint_test.rs"]
mod lint_host_tests;

#[cfg(test)]
#[path = "return_path_test.rs"]
mod return_path_host_tests;

#[cfg(test)]
#[path = "companion_signature_test.rs"]
mod companion_signature_host_tests;

#[cfg(test)]
#[path = "import_seam_test.rs"]
mod import_seam_host_tests;

#[cfg(test)]
#[path = "kernel_namespace_test.rs"]
mod kernel_namespace_host_tests;

#[cfg(test)]
#[path = "exhaustive_test.rs"]
mod exhaustive_host_tests;

#[cfg(test)]
#[path = "fragment_definition_test.rs"]
mod fragment_definition_host_tests;

#[cfg(test)]
#[path = "companion_predeclare_test.rs"]
mod companion_predeclare_host_tests;

#[cfg(test)]
#[path = "definite_assignment_test.rs"]
mod definite_assignment_host_tests;

#[cfg(test)]
#[path = "typecheck/mod.rs"]
pub mod typecheck;
