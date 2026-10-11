//! Shim: the `place` lowering module moved to `radix-mir-lower`; this copy
//! hosts its companion test so the test's module path and `#[path]` stay
//! unchanged.

pub use radix_mir_lower::lower::place::*;

// The moved file began with `use super::*;`; its companion test relied on that
// scope, so the shim re-exports it.
pub use super::*;

#[cfg(test)]
#[path = "place_test.rs"]
mod tests;
