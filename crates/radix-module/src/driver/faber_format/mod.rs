//! The Faber format/convert workflow: analyze + canonical HIR re-emit.
//!
//! `faber format` and `faber convert` are workflows above the driver, not
//! code emission. They sequence driver analysis with the Faber HIR emitter,
//! prove the emitted program's meaning is unchanged, and refuse to write when
//! it is not.

pub mod compile;
mod meaning;
pub mod test_gate;

pub use compile::{FaberReemitResult, emit_from_analyzed, reemit_faber};
