//! Re-exports instans-precision types and assignability rules.
//!
//! This file is the re-export boundary for InstansPrecision, InstansPrecisionRank,
//! InstansPrecisionError, and the instans_assignable helper. The canonical source
//! is `radix-types::instans_precision`.

pub use radix_types::instans_precision::*;
pub use radix_types::{instans_assignable, InstansPrecision};
