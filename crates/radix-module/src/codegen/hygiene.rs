//! Codegen hygiene helpers — re-exported from `radix-codegen-shared`.
//!
//! Thin re-export stub. Canonical implementation lives in the sibling
//! `radix-codegen-shared` crate so all backends share one name- and
//! hygiene-collision policy.

#![allow(unused_imports, clippy::wildcard_imports)]
pub use radix_codegen_shared::hygiene::*;

#[cfg(test)]
#[path = "hygiene_test.rs"]
mod tests;
