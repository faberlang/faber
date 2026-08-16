//! `+++` frontmatter validation and test-selection merge (forwarded to
//! `radix-package`).
//!
//! RP2b: frontmatter validation, manifest-path probing, and test-selection
//! merging are compiler-owned and live in `radix-package`; this module
//! forwards the moved surface so faber package consumers keep their existing
//! import paths.

pub use radix_package::frontmatter::RustTestSelection;
#[allow(unused_imports)] // test-selection merge consumed by the moved Rust assembly until RP7
pub(super) use radix_package::frontmatter::{
    manifest_path_for_spec, merge_entry_test_selection,
};
