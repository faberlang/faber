//! Canonical Faber HIR re-emission backend (leaf crate).
//!
//! `radix-module` re-exports this leaf through its codegen surface and owns
//! module-level target dispatch; the `radix` façade re-exports that module API.

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
mod comma_law_test;

#[cfg(test)]
mod commune_emit_test;

#[cfg(test)]
mod intervallum_spacing_test;

#[cfg(test)]
mod itera_range_emit_test;

#[cfg(test)]
mod trivia_test;

pub use module::{emit, empty_presentation, FaberCodegen, FaberOutput};
pub use surface::{KeywordSurface, TypeBindingKind};
