//! Proba/test source path filters (forwarded to `radix-package`).
//!
//! RP2b: the filter implementation is compiler-owned and lives in
//! `radix-package`; this module forwards the moved surface. The focused
//! `test_source_filter_test.rs` stays faber-side and exercises the forwarded
//! types.

pub use radix_package::test_source_filter::TestSourceFilter;

#[cfg(test)]
pub(crate) use radix_package::test_source_filter::path_pattern_matches;

#[cfg(test)]
#[path = "test_source_filter_test.rs"]
mod test_source_filter_test;
