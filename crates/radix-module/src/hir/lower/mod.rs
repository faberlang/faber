//! AST→HIR lowering — re-exported from `radix-semantic::lower` (test host).

pub use crate::hir::{HirAnnotation, HirAnnotationField, HirAnnotationValue, HirAvailability};
pub use radix_semantic::lower::annotation::*;
pub use radix_semantic::lower::*;

#[cfg(test)]
#[path = "annotation_test.rs"]
mod tests;

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "schema_test.rs"]
mod schema_test;

#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_host_tests;

#[cfg(test)]
#[path = "instans_type_test.rs"]
mod instans_type_tests;
