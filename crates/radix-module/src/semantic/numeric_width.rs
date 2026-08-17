//! Re-exports numeric-width types and helpers for the semantic module's public API.
//!
//! This file is the re-export boundary for NumericWidth, WidthError, and
//! width-marker sugar helpers. The canonical source is `radix-types::numeric_width`.

pub use radix_types::numeric_width::*;

#[cfg(test)]
#[path = "numeric_width_test.rs"]
mod tests;
