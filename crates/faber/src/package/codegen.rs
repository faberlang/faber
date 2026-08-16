//! HIR source-text assembly for generated package crates (forwarded).
//!
//! RP4: `assemble_crate` / `ModuleNode` moved to `radix-package` with the Rust
//! package assembly; this module forwards the moved surface so faber's
//! library-crate emitter (`library_link.rs`) and package tests keep their
//! existing import paths until RP7 removes the residue.

#[allow(unused_imports)] // consumed by package tests (hir-fhir parity) until RP7
pub use radix_package::codegen::{assemble_crate, ModuleNode};
