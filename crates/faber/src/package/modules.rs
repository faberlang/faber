//! Module-segment derivation for package sources (forwarded to `radix-package`).
//!
//! RP2b: module/member segment mapping is compiler-owned and lives in
//! `radix-package`; this module forwards the moved surface so faber package
//! consumers keep their existing import paths.

pub(super) use radix_package::modules::module_segments_for_file;
