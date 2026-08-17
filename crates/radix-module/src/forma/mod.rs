//! Faber source formatter and re-emitter.
//!
//! The `forma` module owns all Faber→Faber output. Radix supplies parse, analyze, and HIR;
//! forma supplies author and reader-locale (HIR-backed) formatting modes.
//!
//! LEGACY(canonical): `canonical::` / `emit_canonical` / `compile_canonical*`
//! are legacy names from before reader locales; see `compile.rs` module note.
//! Rename in a later sweep.

pub mod author;
#[cfg(feature = "hir-faber")]
pub mod canonical;
pub mod compile;
pub mod options;
pub mod pretty;

pub use author::{emit as emit_author, AuthorEmitter};
#[cfg(feature = "hir-faber")]
pub use canonical::{emit as emit_canonical, FaberCodegen, FaberOutput};
pub use compile::{
    compile_author, compile_author_str, compile_author_with_policy, compile_author_with_policy_str,
    compile_pretty_author, compile_pretty_author_str, FormatCompileResult,
};
#[cfg(feature = "hir-faber")]
pub use compile::{compile_canonical, compile_canonical_str, emit_from_analyzed};
pub use options::{FormatOptions, FormatPolicy, PolicyError};

#[doc(hidden)]
pub mod test_gate;

#[cfg(all(test, feature = "hir-faber"))]
#[path = "convert_regression_test.rs"]
mod convert_regression_test;
