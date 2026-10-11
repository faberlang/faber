//! Shim: the `expr` lowering module moved to `radix-mir-lower`; this copy hosts
//! its companion test so the test's module path and `#[path]` stay unchanged.

pub use radix_mir_lower::lower::expr::*;

// The moved file began with `use super::*;`; its companion test relied on that
// scope, so the shim re-exports it.
pub use super::*;
pub use radix_hir::HirCallArg;

#[cfg(test)]
#[path = "expr_test.rs"]
mod tests;
