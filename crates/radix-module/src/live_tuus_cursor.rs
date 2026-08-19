//! Live tuus-cursor provenance — re-exported from `radix-semantic` (test host).

pub use radix_semantic::live_tuus_cursor::*;
pub use radix_semantic::*;

#[cfg(test)]
#[path = "live_tuus_cursor_test.rs"]
mod tests;
