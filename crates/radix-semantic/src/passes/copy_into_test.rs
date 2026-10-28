//! `target ⇇ value` copy-into: rules R1-R7.
//!
//! `⇇` copies every value of `value` into the existing storage of `target`;
//! `←` only ever rebinds names. Each rule below has a positive fixture and one
//! negative fixture asserting the diagnostic kind and issue (never prose).
//!
//! - R1 exact element type and shape on both sides
//! - R2 the target is writable tensor storage (never a constant, a view, a
//!   scalar, or a list/matrix/vector)
//! - R3 the value does not alias the target
//! - R4/R5 a statement without a result, no chains, no use as a value
//! - R6 `←` of a whole tensor into a `mut` tensor parameter is rejected
//!   inside a kernel; on the host it is the ordinary rebind
//! - R7 the callable-sink form is retired

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

/// The fixtures are written in the English surface.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn assert_accepts(source: &str) {
    let result = analyze_en(source);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(super::super::error::SemanticError::is_error),
        "expected no error, got: {:?}",
        result.diagnostics
    );
}

fn assert_rejects(source: &str, kind: SemanticErrorKind, expected: &str) {
    let result = analyze_en(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.kind == kind && issue(d) == Some(expected)),
        "expected {kind:?} issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

fn assert_no_issue(source: &str, unexpected: &str) {
    let result = analyze_en(source);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| issue(d) == Some(unexpected)),
        "unexpected issue {unexpected:?}: {:?}",
        result.diagnostics
    );
}

// =============================================================================
// Accepted shapes
// =============================================================================

#[test]
fn kernel_shaped_copy_of_a_transpose_into_the_mut_output_typechecks() {
    assert_accepts(
        r"@ kernel
fn transposed(tf32[2, 3] input, mut tf32[3, 2] output) → void {
    output ⇇ input.transpose()
}
main {}",
    );
}

#[test]
fn a_generic_target_compares_against_a_transpose_in_declared_symbol_space() {
    assert_accepts(
        r"fn transposed<size T, size d>(tensor<f32, [T, d]> input, mut tensor<f32, [d, T]> output) → void {
    output ⇇ input.transpose()
}
main {}",
    );
}

#[test]
fn the_right_side_may_be_a_view_or_a_view_binding() {
    assert_accepts(
        r"fn copy_view(tf32[3, 2] input, mut tf32[2, 3] first, mut tf32[2, 3] second) → void {
    let tf32[2, 3] view ← input.transpose()
    first ⇇ view
    second ⇇ input.transpose()
}
main {}",
    );
}

#[test]
fn a_ref_parameter_view_is_a_legal_source() {
    assert_accepts(
        r"fn copy_ref(ref tf32[3, 2] input, mut tf32[2, 3] output) → void {
    output ⇇ input.transpose()
}
main {}",
    );
}

#[test]
fn a_field_root_is_writable_and_sibling_fields_do_not_alias() {
    assert_accepts(
        r"class Holder {
    mut tf32[3, 3] buf
    mut tf32[3, 3] other
}
fn fill(mut Holder holder, tf32[3, 3] source) → void {
    holder.buf ⇇ source
    holder.buf ⇇ holder.other
    holder.buf ⇇ holder.other.transpose()
}
main {}",
    );
}

#[test]
fn a_var_local_is_writable_and_still_rebinds_with_the_arrow() {
    assert_accepts(
        r"fn locals(tf32[3, 3] source) → void {
    mut tf32[3, 3] local ← source + source
    local ← source + source
    local ⇇ source
    local ⇇ source.transpose()
}
main {}",
    );
}

#[test]
fn distinct_mut_parameters_do_not_alias() {
    assert_accepts(
        r"fn two(mut tf32[3, 3] first, mut tf32[3, 3] second) → void {
    first ⇇ second.transpose()
}
main {}",
    );
}

#[test]
fn a_mut_parameter_written_only_by_copy_into_is_not_reported_unused() {
    let result = analyze_en(
        r"fn write(tf32[3, 3] source, mut tf32[3, 3] output) → void {
    output ⇇ source
}
main {}",
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| issue(d) == Some("unused_mut_parameter")),
        "the copy is a write of the parameter: {:?}",
        result.diagnostics
    );
}

#[test]
fn a_var_tensor_written_only_by_copy_into_is_not_an_unnecessary_var() {
    let source = r"main {
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    let tf32[2, 2] source ← flat ↦ tf32[2, 2]
    mut tf32[2, 2] target ← flat ↦ tf32[2, 2]
    target ⇇ source
}";
    assert_no_issue(source, "unnecessary_varia");
}

// =============================================================================
// R1 exact element type and shape
// =============================================================================

#[test]
fn r1_a_different_element_type_is_rejected() {
    assert_rejects(
        r"fn t(tf64[3, 2] input, mut tf32[3, 2] output) → void {
    output ⇇ input
}
main {}",
        SemanticErrorKind::TypeMismatch,
        "copy_into_type_mismatch",
    );
}

#[test]
fn r1_a_widening_element_is_not_accepted_as_exact() {
    assert_rejects(
        r"fn t(ti32[3, 2] input, mut ti64[3, 2] output) → void {
    output ⇇ input
}
main {}",
        SemanticErrorKind::TypeMismatch,
        "copy_into_type_mismatch",
    );
}

#[test]
fn r1_a_static_shape_mismatch_is_a_compile_error() {
    assert_rejects(
        r"fn t(tf32[2, 3] input, mut tf32[2, 3] output) → void {
    output ⇇ input.transpose()
}
main {}",
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
    );
}

#[test]
fn r1_a_shape_mismatch_through_a_ref_receiver_is_still_a_compile_error() {
    assert_rejects(
        r"fn t(ref tf32[2, 3] input, mut tf32[2, 3] output) → void {
    output ⇇ input.transpose()
}
main {}",
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
    );
}

#[test]
fn r1_a_generic_shape_mismatch_is_a_compile_error() {
    assert_rejects(
        r"fn t<size T, size d>(tensor<f32, [T, d]> input, mut tensor<f32, [T, d]> output) → void {
    output ⇇ input.transpose()
}
main {}",
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
    );
}

#[test]
fn r1_a_runtime_extent_target_accepts_the_same_symbolic_extent() {
    assert_accepts(
        r"fn t<size T>(tensor<f32, [T]> input, mut tensor<f32, [T]> output) → void {
    output ⇇ input
}
main {}",
    );
}

#[test]
fn r1_a_value_that_is_not_a_tensor_is_rejected() {
    assert_rejects(
        r"fn t(mut tf32[3, 3] output) → void {
    output ⇇ 5
}
main {}",
        SemanticErrorKind::TypeMismatch,
        "copy_into_type_mismatch",
    );
}

// =============================================================================
// R2 writable tensor storage on the left
// =============================================================================

#[test]
fn r2_a_constant_target_is_rejected() {
    assert_rejects(
        r"fn t(tf32[3, 3] source) → void {
    let tf32[3, 3] fixed ← source + source
    fixed ⇇ source
}
main {}",
        SemanticErrorKind::ImmutableAssignment,
        "copy_into_target_immutable",
    );
}

#[test]
fn r2_a_non_mut_parameter_target_is_rejected() {
    assert_rejects(
        r"fn t(tf32[3, 3] source, tf32[3, 3] plain) → void {
    plain ⇇ source
}
main {}",
        SemanticErrorKind::ImmutableAssignment,
        "copy_into_target_immutable",
    );
}

#[test]
fn r2_a_field_of_a_non_mut_parameter_is_rejected() {
    assert_rejects(
        r"class Holder {
    mut tf32[3, 3] buf
}
fn t(Holder holder, tf32[3, 3] source) → void {
    holder.buf ⇇ source
}
main {}",
        SemanticErrorKind::ImmutableAssignment,
        "copy_into_target_immutable",
    );
}

#[test]
fn r2_a_scalar_target_is_rejected() {
    assert_rejects(
        r"fn t(mut i32 x) → void {
    x ⇇ 3
}
main {}",
        SemanticErrorKind::OperandTypesInvalid,
        "copy_into_target_not_tensor",
    );
}

#[test]
fn r2_a_list_target_is_rejected() {
    assert_rejects(
        r"fn t(mut list<i32> target, list<i32> source) → void {
    target ⇇ source
}
main {}",
        SemanticErrorKind::OperandTypesInvalid,
        "copy_into_target_not_tensor",
    );
}

#[test]
fn r2_a_matrix_or_vector_target_is_rejected() {
    assert_rejects(
        r"fn t(mut matrix<f32, [2, 2]> target, matrix<f32, [2, 2]> source) → void {
    target ⇇ source
}
main {}",
        SemanticErrorKind::OperandTypesInvalid,
        "copy_into_target_not_tensor",
    );
    assert_rejects(
        r"fn t(mut vector<f32, size 4> target, vector<f32, size 4> source) → void {
    target ⇇ source
}
main {}",
        SemanticErrorKind::OperandTypesInvalid,
        "copy_into_target_not_tensor",
    );
}

#[test]
fn r2_a_view_binding_target_is_rejected() {
    assert_rejects(
        r"fn t(tf32[3, 2] source, mut tf32[2, 3] output) → void {
    let tf32[3, 2] view ← output.transpose()
    view ⇇ source
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_target_view",
    );
}

#[test]
fn r2_a_view_expression_target_is_rejected() {
    assert_rejects(
        r"fn t(tf32[1, 3] source, mut tf32[3, 3] output) → void {
    output.slice(0, 1) ⇇ source
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_target_view",
    );
}

#[test]
fn r2_a_call_result_is_not_storage() {
    assert_rejects(
        r"fn make() → tf32[3, 3] {
    return empty
}
fn t(tf32[3, 3] source) → void {
    make() ⇇ source
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_target_not_storage",
    );
}

// =============================================================================
// R3 the value does not alias the target
// =============================================================================

#[test]
fn r3_a_view_of_the_target_is_rejected() {
    assert_rejects(
        r"fn t(mut tf32[3, 3] a) → void {
    a ⇇ a.transpose()
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_alias",
    );
}

#[test]
fn r3_the_target_itself_is_rejected() {
    assert_rejects(
        r"fn t(mut tf32[3, 3] a) → void {
    a ⇇ a
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_alias",
    );
}

#[test]
fn r3_an_arrow_alias_of_the_target_counts() {
    assert_rejects(
        r"fn t(mut tf32[3, 3] a) → void {
    mut tf32[3, 3] c ← a
    c ⇇ a.transpose()
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_alias",
    );
}

#[test]
fn r3_an_arrow_alias_on_the_value_side_counts() {
    assert_rejects(
        r"fn t(mut tf32[3, 3] a) → void {
    mut tf32[3, 3] c ← a
    a ⇇ c.transpose()
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_alias",
    );
}

#[test]
fn r3_aliases_of_aliases_count() {
    assert_rejects(
        r"fn t(mut tf32[3, 3] a) → void {
    mut tf32[3, 3] c ← a
    mut tf32[3, 3] d ← c
    d ⇇ a.transpose()
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_alias",
    );
}

#[test]
fn r3_a_rebound_name_no_longer_aliases_the_value() {
    assert_no_issue(
        r"fn t(tf32[3, 3] source, mut tf32[3, 3] a) → void {
    mut tf32[3, 3] c ← source + source
    c ← source + source
    c ⇇ a.transpose()
}
main {}",
        "copy_into_alias",
    );
}

#[test]
fn r3_the_same_field_is_rejected() {
    assert_rejects(
        r"class Holder {
    mut tf32[3, 3] buf
    mut tf32[3, 3] other
}
fn t(mut Holder holder) → void {
    holder.buf ⇇ holder.buf.transpose()
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "copy_into_alias",
    );
}

// =============================================================================
// R4/R5 a statement without a result
// =============================================================================

#[test]
fn r4_a_copy_into_is_not_a_value_to_assign() {
    assert_rejects(
        r"fn t(tf32[3, 3] a, mut tf32[3, 3] b) → void {
    mut tf32[3, 3] c ← a + a
    c ← b ⇇ a
}
main {}",
        SemanticErrorKind::TypeMismatch,
        "copy_into_value_used",
    );
}

#[test]
fn r5_a_copy_into_is_not_a_value_to_initialize_with() {
    assert_rejects(
        r"fn t(tf32[3, 3] a, mut tf32[3, 3] b) → void {
    let _ c ← b ⇇ a
}
main {}",
        SemanticErrorKind::TypeMismatch,
        "copy_into_value_used",
    );
}

#[test]
fn r5_a_chain_is_a_named_parse_error() {
    let result = analyze_en(
        r"fn t(tf32[3, 3] a, mut tf32[3, 3] b, mut tf32[3, 3] c) → void {
    b ⇇ c ⇇ a
}
main {}",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| issue(d) == Some("copy_into_chain")),
        "expected copy_into_chain, got: {:?}",
        result.diagnostics
    );
}

// =============================================================================
// R6 `←` never writes a mut tensor parameter inside a kernel
// =============================================================================

#[test]
fn r6_a_host_arrow_into_a_mut_tensor_parameter_is_the_ordinary_rebind() {
    assert_accepts(
        r"fn t(tf32[3, 3] a, mut tf32[3, 3] b) → void {
    b ← a + a
}
main {}",
    );
}

/// 2026-10-13 ruling: a kernel parameter is a device buffer, so `←` cannot re-point it.
#[test]
fn r6_a_kernel_arrow_into_the_mut_output_is_rejected_by_ruling() {
    assert_rejects(
        r"@ kernel
fn t(tf32[3, 3] a, mut tf32[3, 3] b) → void {
    b ← a + a
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "mut_tensor_param_rebind",
    );
}

#[test]
fn r6_a_view_stored_into_a_mut_parameter_is_the_view_escape() {
    assert_rejects(
        r"fn t(tf32[3, 2] a, mut tf32[2, 3] b) → void {
    b ← a.transpose()
}
main {}",
        SemanticErrorKind::ModeMismatch,
        "tensor_view_escape",
    );
}

#[test]
fn r6_the_arrow_still_rebinds_a_var_local_and_writes_one_element() {
    assert_no_issue(
        r"fn t(tf32[3, 3] a, mut tf32[3, 3] b) → void {
    mut tf32[3, 3] c ← a + a
    c ← a + a
    b[0, 0] ← 1.0
}
main {}",
        "mut_tensor_param_rebind",
    );
}

#[test]
fn r6_a_scalar_mut_parameter_still_takes_the_arrow() {
    assert_no_issue(
        r"fn t(mut i32 x) → void {
    x ← 3
}
main {}",
        "mut_tensor_param_rebind",
    );
}

// =============================================================================
// R7 the callable-sink form is retired
// =============================================================================

#[test]
fn r7_a_callable_on_the_left_is_the_retired_sink_form() {
    assert_rejects(
        r#"fn sink(string value) → void {}
fn t() → void {
    sink ⇇ "salve"
}
main {}"#,
        SemanticErrorKind::NonCallable,
        "transfer_sink_retired",
    );
}
