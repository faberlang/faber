//! Kernel namespace registry — re-exported from `radix-semantic::kernel` (test host).

pub use radix_semantic::kernel::*;

#[cfg(test)]
#[path = "kernel_namespace_test.rs"]
mod tests;
