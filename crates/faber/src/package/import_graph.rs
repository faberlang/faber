//! Import graph construction for the faber package surface (forwarded).
//!
//! RP2c: local/library import resolution, cycle detection, mount planning,
//! and import diagnostics moved to `radix-package`; this module forwards the
//! moved surface so faber package consumers keep their existing import paths.

pub use radix_package::import_graph::{
    build_mount_plan, detect_import_cycles, import_unsupported_diagnostic, library_import_binding,
    library_import_kind_diagnostic, resolve_import, ImportResolution,
};
