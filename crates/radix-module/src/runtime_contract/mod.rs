//! Runtime contract types — re-exported from leaf `radix-runtime-contract`
//! (display, frame, instans, sparsa, tensor).

pub use radix_runtime_contract::{display, frame, instans, sparsa, tensor};

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
