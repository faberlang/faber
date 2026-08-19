//! Typecheck pipeline tests — driver/codegen proofs that cannot live in the leaf.

pub use radix_semantic::passes::typecheck::*;
pub use radix_semantic::*;

#[cfg(test)]
#[path = "test_support.rs"]
pub mod test_support;

#[cfg(test)]
#[path = "intrinsic_locale_test.rs"]
mod intrinsic_locale_test;

#[cfg(test)]
#[path = "unsigned_integer_test.rs"]
mod unsigned_integer_test;

#[cfg(test)]
#[path = "scalar_numeric_width_test.rs"]
mod scalar_numeric_width_test;

#[cfg(test)]
#[path = "intervallum_test.rs"]
mod intervallum_test;

#[cfg(test)]
#[path = "modular_word_width_test.rs"]
mod modular_word_width_test;
