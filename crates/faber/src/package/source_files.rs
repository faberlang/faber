//! Faber package source discovery (forwarded to `radix-package`).
//!
//! RP2b: source loading is compiler-owned and lives in `radix-package`; this
//! module forwards the moved surface so faber package consumers keep their
//! existing import paths.

pub(super) use radix_package::source_files::{
    is_proba_source_path, load_package_source, package_source_files,
};
