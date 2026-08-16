//! Faber build lockfile (`faber.lock`) consumption.
//!
//! RP2a: lock consumption is compiler-owned and lives in `radix-package`; this
//! module forwards the moved surface so faber package consumers keep their
//! existing import paths.

#[allow(unused_imports)] // LockedPackage consumed by the moved artifact plan until RP7
pub(crate) use radix_package::lockfile::{
    lock_index, read_lock, validate_dependencies_against_lock, LockedPackage, LOCK_FILE,
};
