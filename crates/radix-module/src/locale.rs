//! Reader-locale packs — re-exported from `radix-semantic::locale` (test host).

pub use radix_semantic::locale::*;

#[cfg(test)]
#[path = "locale_test.rs"]
mod tests;

#[cfg(test)]
#[path = "locale_intrinsics_test.rs"]
mod intrinsics_tests;

#[cfg(test)]
#[path = "intrinsic_pack_completeness_test.rs"]
mod completeness_tests;
