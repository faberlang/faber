//! Typecheck pipeline tests — driver/codegen proofs that cannot live in the leaf.
//!
//! Production typecheck is crate-private in `radix-semantic`. This module
//! hosts remaining integration proofs that need the module compiler.

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
#[path = "bounded_textus_test.rs"]
mod bounded_textus_test;

#[cfg(test)]
#[path = "bounded_octeti_test.rs"]
mod bounded_octeti_test;

#[cfg(test)]
#[path = "modular_word_width_test.rs"]
mod modular_word_width_test;
