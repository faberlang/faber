//! Canonical Faber source emission for analyzed HIR.
//!
//! LEGACY(canonical): module name predates reader locales — this is the
//! HIR-backed Faber re-emit used by `faber format --locale <X>`; there is no
//! single canonical language surface anymore. Rename in a later sweep.
//!
//! The HIR-backed canonical emitter lives in `crate::codegen::faber` so the
//! developer compiler CLI can expose `radix emit -t faber` without depending on
//! the higher-level `forma` crate. `forma` re-exports it for formatter callers.

pub use crate::codegen::faber::{emit, empty_presentation, FaberCodegen, FaberOutput};
