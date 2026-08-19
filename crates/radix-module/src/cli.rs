//! `@ cli` source model — re-exported from `radix-semantic::cli` (test host).

pub use radix_semantic::cli::*;

#[cfg(test)]
#[path = "cli_test.rs"]
mod tests;
