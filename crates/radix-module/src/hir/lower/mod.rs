//! AST→HIR lowering — re-exported from `radix-semantic::lower` (test host).

pub use radix_semantic::lower::*;

#[cfg(test)]
#[path = "annotation_test.rs"]
mod annotation_test;

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "schema_test.rs"]
mod schema_test;

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "instans_type_test.rs"]
mod instans_type_tests;
