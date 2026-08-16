//! Cross-unit file-interface extraction for the faber package surface
//! (forwarded).
//!
//! RP2c: file-interface snapshot extraction moved to `radix-package`; this
//! module forwards the moved surface so faber package consumers keep their
//! existing import paths.

pub use radix_package::file_interface::extract_callable_contracts;
#[allow(unused_imports)]
// consumed by `package_test.rs` analysis-shape assertions (cfg(test)).
pub use radix_package::file_interface::extract_file_interface_with_identity;
