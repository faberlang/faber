//! Semantic passes — re-exported from `radix-semantic::passes`.
//!
//! Pipeline-shaped tests that need the driver or a backend stay hosted here.

pub use radix_semantic::passes::*;

#[cfg(test)]
#[path = "import_seam_test.rs"]
mod import_seam_test;

#[cfg(test)]
#[path = "member_privacy_seam_test.rs"]
mod member_privacy_seam_test;

#[cfg(test)]
#[path = "typecheck/mod.rs"]
pub mod typecheck;
