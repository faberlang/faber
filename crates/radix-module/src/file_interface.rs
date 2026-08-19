//! Portable file interfaces — re-exported from `radix-semantic::file_interface` (test host).

pub use radix_semantic::file_interface::*;

#[cfg(test)]
#[path = "file_interface_test.rs"]
mod tests;
