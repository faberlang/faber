//! Import graph construction for the faber package surface (forwarded).
//!
//! RP2c: local/library import resolution, cycle detection, mount planning,
//! and import diagnostics moved to `radix-package`; this module forwards the
//! moved surface so faber package consumers keep their existing import paths.
//!
//! RP3: the package loader and multi-unit analysis moved to `radix-package`
//! with the analysis half of `compile.rs`; only the resolution surface the
//! emission half still consumes is forwarded here.

pub use radix_package::import_graph::{resolve_import, ImportResolution};
