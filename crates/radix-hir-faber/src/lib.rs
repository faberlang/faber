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
pub mod wrap;

#[cfg(test)]
mod cape_attach_emit_test;
#[cfg(test)]
mod closure_body_emit_test;
#[cfg(test)]
mod fac_omnia_emit_test;

#[cfg(test)]
mod nondum_availability_emit_test;

#[cfg(test)]
mod comma_law_test;

#[cfg(test)]
mod size_decl_emit_test;

#[cfg(test)]
mod extent_lift_emit_test;

#[cfg(test)]
mod generic_default_emit_test;

#[cfg(test)]
mod commune_emit_test;

// Mirrors the lint-fix-1 cfg-gate landing on main: ungated test modules fail
// stage-2 clippy (unused/dead) in non-test builds.
#[cfg(test)]
mod bitnot_spacing_test;
#[cfg(test)]
mod instance_spacing_test;
#[cfg(test)]
mod intervallum_spacing_test;
#[cfg(test)]
mod radical_spacing_test;
#[cfg(test)]
mod static_field_read_test;

#[cfg(test)]
mod itera_range_emit_test;

#[cfg(test)]
mod receiver_paren_test;

#[cfg(test)]
mod inferred_bracket_emit_test;

#[cfg(test)]
mod lege_lineam_emit_test;

#[cfg(test)]
mod trivia_test;

#[cfg(test)]
mod type_check_emit_test;

#[cfg(test)]
mod wrap_test;

pub use module::{FaberCodegen, FaberOutput, empty_presentation};
pub use surface::{FrameFamily, KeywordSurface, TypeBindingKind};
pub use wrap::wrap_faber_source;
