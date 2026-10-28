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
#[path = "membership_test.rs"]
mod membership_test;

#[cfg(test)]
#[path = "membership_glyph_test.rs"]
mod membership_glyph_test;

#[cfg(test)]
#[path = "bounded_textus_test.rs"]
mod bounded_textus_test;

#[cfg(test)]
#[path = "bounded_octeti_test.rs"]
mod bounded_octeti_test;

#[cfg(test)]
#[path = "bounded_ops_test.rs"]
mod bounded_ops_test;

#[cfg(test)]
#[path = "textus_index_warn_test.rs"]
mod textus_index_warn_test;

#[cfg(test)]
#[path = "modular_word_width_test.rs"]
mod modular_word_width_test;

#[cfg(test)]
#[path = "saturating_word_test.rs"]
mod saturating_word_test;

#[cfg(test)]
#[path = "trapping_word_test.rs"]
mod trapping_word_test;

#[cfg(test)]
#[path = "word_literal_equality_test.rs"]
mod word_literal_equality_test;

#[cfg(test)]
#[path = "policy_float_store_test.rs"]
mod policy_float_store_test;

#[cfg(test)]
#[path = "via_hint_test.rs"]
mod via_hint_test;

#[cfg(test)]
#[path = "nary_range_test.rs"]
mod nary_range_test;

#[cfg(test)]
#[path = "inf_type_test.rs"]
mod inf_type_test;

#[cfg(test)]
#[path = "ranged_list_test.rs"]
mod ranged_list_test;

#[cfg(test)]
#[path = "capacity_size_test.rs"]
mod capacity_size_test;

#[cfg(test)]
#[path = "ranged_policy_test.rs"]
mod ranged_policy_test;
