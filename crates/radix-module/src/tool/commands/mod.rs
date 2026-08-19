//! Command handlers for the developer `radix` tool and shared helpers.
//!
//! Submodules group inspection `JSON`, semantic check, compile/emit, targets,
//! and artifact writing. The parent [`crate::tool`] module re-exports the public
//! surface for `radix_module::tool::*` callers.

mod abi;
mod check;
pub(crate) mod compile;
mod diagnostics_locale_pack;
mod emit;
mod inspect;
mod json;
mod package;
#[cfg(feature = "mir-core")]
mod reflection;
mod source;
mod targets;
mod verify;

pub use abi::*;
pub use check::*;
pub use compile::*;
pub use diagnostics_locale_pack::*;
pub use emit::*;
pub use inspect::*;
pub use json::escape_json;
#[cfg(feature = "hir-fhir")]
pub use package::{
    nucleum_dispatch_reflection_names, nucleum_omitted_target_symbols, signature_only_fragment_import_diagnostic,
};
#[cfg(feature = "mir-core")]
pub use reflection::*;
pub use source::*;
pub use targets::*;
pub use verify::*;

#[cfg(test)]
#[cfg(feature = "mir-core")]
#[path = "../../tool_test.rs"]
mod tests;
