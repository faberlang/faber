//! Typecheck — re-exported from `radix-semantic::passes::typecheck` (test host).

pub use radix_semantic::passes::typecheck::*;
pub use radix_semantic::*;

#[cfg(test)]
#[path = "type_hole_union_warnings_test.rs"]
mod type_hole_union_warnings_test;

#[cfg(test)]
#[path = "tensor_bracket_test.rs"]
mod tests;

#[cfg(test)]
#[path = "access_operand_test.rs"]
mod access_operand_tests;

#[cfg(test)]
#[path = "convert_test.rs"]
mod convert_host_tests;

#[cfg(all(test, feature = "hir-rust"))]
#[path = "tensor_lattice_matrix_test.rs"]
mod tensor_lattice_matrix_tests;

#[cfg(test)]
#[path = "tensor_index_test.rs"]
mod tensor_index_host_tests;

#[cfg(test)]
#[path = "tensor_type_directed_test.rs"]
mod tensor_type_directed_host_tests;

#[cfg(test)]
#[path = "bounded_identity_test.rs"]
mod bounded_identity_test;

#[cfg(test)]
#[path = "bounded_capacity_slot_test.rs"]
mod bounded_capacity_slot_test;

#[cfg(test)]
#[path = "generic_params_test.rs"]
mod generic_params_test;

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "generic_construction_test.rs"]
mod generic_construction_test;

#[cfg(all(test, feature = "hir-rust"))]
#[path = "scalar_longitudo_test.rs"]
mod scalar_longitudo_test;

#[cfg(test)]
#[path = "type_hole_union_infer_test.rs"]
mod type_hole_union_infer_test;

#[cfg(test)]
#[path = "null_narrowing_test.rs"]
mod null_narrowing_tests;

#[cfg(test)]
#[path = "octeti_index_test.rs"]
mod octeti_index_test;

#[cfg(test)]
#[path = "octeti_endian_conversio_test.rs"]
mod octeti_endian_conversio_test;

#[cfg(test)]
mod test_support;

#[cfg(test)]
#[path = "../typecheck_test.rs"]
mod typecheck_host_tests;

#[cfg(test)]
#[path = "element_relaxation_test.rs"]
mod element_relaxation_tests;

#[cfg(test)]
#[path = "scalar_order_test.rs"]
mod scalar_order_test;

#[cfg(test)]
#[path = "equality_exact_types_test.rs"]
mod equality_exact_types_test;

#[cfg(test)]
#[path = "ascii_index_test.rs"]
mod ascii_index_test;

#[cfg(test)]
#[path = "ascii_cursor_test.rs"]
mod ascii_cursor_test;

#[cfg(test)]
#[path = "textus_cursor_test.rs"]
mod textus_cursor_test;

#[cfg(all(test, feature = "hir-rust"))]
#[path = "numeric_value_equality_test.rs"]
mod numeric_value_equality_test;

#[cfg(test)]
#[path = "scalar_numeric_width_test.rs"]
mod scalar_numeric_width_test;

#[cfg(test)]
#[path = "union_pattern_test.rs"]
mod union_pattern_test;

#[cfg(test)]
#[path = "union_construction_test.rs"]
mod union_construction_test;

#[cfg(test)]
#[path = "namespace_shadow_test.rs"]
mod namespace_shadow_test;

#[cfg(test)]
#[path = "union_value_test.rs"]
mod union_value_test;

#[cfg(test)]
#[path = "unsigned_integer_test.rs"]
mod unsigned_integer_test;

#[cfg(test)]
#[path = "modular_word_width_test.rs"]
mod modular_word_width_test;

#[cfg(test)]
#[path = "literal_inference_test.rs"]
mod literal_inference_test;

#[cfg(test)]
#[path = "glyph_binary_test.rs"]
mod glyph_binary_test;

#[cfg(test)]
#[path = "tensor_plus_test.rs"]
mod tensor_plus_test;

#[cfg(test)]
#[path = "gradient_test.rs"]
mod gradient_test;

#[cfg(test)]
#[path = "infer_test.rs"]
mod infer_host_tests;

#[cfg(test)]
#[path = "type_hole_union_join_test.rs"]
mod type_hole_union_join_test;

#[cfg(test)]
#[path = "tensor_index_width_test.rs"]
mod tensor_index_width_host_tests;

#[cfg(test)]
#[path = "matrix_intrinsic_test.rs"]
mod matrix_intrinsic_tests;

#[cfg(test)]
#[path = "atomic_intrinsic_test.rs"]
mod atomic_intrinsic_tests;

#[cfg(test)]
#[path = "intervallum_test.rs"]
mod intervallum_host_tests;

#[cfg(test)]
#[cfg(feature = "mir-core")]
#[path = "intrinsic_locale_test.rs"]
mod intrinsic_locale_tests;

#[cfg(test)]
#[path = "../typecheck_test.rs"]
mod typecheck_test_host;
