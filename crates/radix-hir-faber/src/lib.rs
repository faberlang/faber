//! Canonical Faber HIR re-emission backend (leaf crate).
//!
//! Package `radix` re-exports the façade and supplies reader-locale surfaces.

// structural: re-emission methods keep &self for a uniform visitor surface.
#![allow(clippy::unused_self)]

pub mod annotation;
pub mod decl;
pub mod expr;
pub mod figura;
pub mod literal;
pub mod module;
pub mod names;
pub mod ops;
pub mod pattern;
pub mod stmt;
pub mod surface;
pub mod trivia;
pub mod types;

#[cfg(test)]
mod trivia_test;

pub use module::{emit, empty_presentation, FaberCodegen, FaberOutput};
pub use surface::{KeywordSurface, TypeBindingKind};
