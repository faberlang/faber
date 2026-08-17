//! Parser — re-exported from foundation crate `radix-parser`.
//!
//! The parser implementation lives in the `radix-parser` sibling crate. This
//! module re-exports it for path stability so `crate::parser::*` continues to
//! resolve for the driver, forma, diagnostics, and tests.

pub use radix_parser::*;

#[cfg(test)]
#[path = "integration_test.rs"]
mod integration_test;
