//! Semantic laws for the closed `reducta via Op` family.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

#[test]
fn all_nine_ops_admit_their_term_kinds_and_result_shapes() {
    assert_accepts(
        r"
functio summa(tensor<f32, [4]> xs) → f32 { redde reducta via summa ex xs }
functio productum(tensor<f32, [4]> xs) → f32 { redde reducta via factum ex xs }
functio maxima(tensor<f32, [4]> xs) → f32 { redde reducta via maxima ex xs }
functio minima(tensor<f32, [4]> xs) → f32 { redde reducta via minima ex xs }
functio index_max(matrix<f32, [2, 4]> xs) → iuncta<numerus, numerus> { redde reducta via argmaxima ex xs apud [i, j] }
functio index_min(tensor<f32, [4]> xs) → numerus { redde reducta via argminima ex xs apud [i] }
functio omnia(tensor<bivalens, [4]> xs) → bivalens { redde reducta via omnia ex xs }
functio aliqua(tensor<bivalens, [4]> xs) → bivalens { redde reducta via quilibet ex xs }
functio numerus(tensor<bivalens, [4]> xs) → numerus { redde reducta via numeratio ex xs }
",
    );
}

#[test]
fn empty_dynamic_extrema_are_nullable_or_take_an_identity() {
    assert_accepts(
        r"
functio maximum<magnitudo N>(tensor<f32, [N]> xs) → f32 ∪ nihil {
    redde reducta via maxima ex xs apud [i]
}
functio minimum<magnitudo N>(tensor<f32, [N]> xs, f32 identity) → f32 {
    redde reducta via minima ex xs vel identity
}
",
    );
    assert_rejects_with_issue(
        r"
functio maximum<magnitudo N>(tensor<f32, [N]> xs) → f32 {
    redde reducta via maxima ex xs
}
",
        "reducta_nullable_result_requires_coalesce",
    );
}

#[test]
fn filum_placement_and_alignment_are_checked() {
    assert_accepts(
        r"
@ nucleum
functio k(tensor<f32, [32]> xs) → f32 {
    redde reducta via summa ex xs apud [i] filum f fixum s { redde s }
}
",
    );
    assert_rejects_with_issue(
        r"
functio k(tensor<f32, [32]> xs) → f32 {
    redde reducta via summa ex xs apud [i] filum f fixum s { redde s }
}
",
        "reducta_filum_requires_nucleum_kernel",
    );
    assert_rejects_with_issue(
        r"
@ nucleum
functio k(tensor<f32, [33]> xs) → f32 {
    redde reducta via summa ex xs apud [i] filum f fixum s { redde s }
}
",
        "reducta_tail_not_aligned",
    );
}

#[test]
fn reduction_terms_enforce_numeric_and_boolean_families() {
    assert_rejects_with_issue(
        r"
functio sum(tensor<textus, [4]> xs) → textus {
    redde reducta via summa ex xs
}
",
        "reducta_term_type_non_scalar",
    );
    assert_rejects_with_issue(
        r"
functio all(tensor<numerus, [4]> xs) → bivalens {
    redde reducta via omnia ex xs
}
",
        "reducta_term_type_not_bool",
    );
}

#[test]
fn body_requires_one_value_return_and_rejects_captured_mutation() {
    assert_rejects_with_issue(
        r"
functio sum(tensor<numerus, [4]> xs) → numerus {
    redde reducta via summa ex xs fixum x { x }
}
",
        "reducta_body_requires_single_redde",
    );
    assert_rejects_with_issue(
        r"
functio sum(tensor<numerus, [4]> xs) → numerus {
    varia numerus captured ← 0
    redde reducta via summa ex xs fixum x { captured ← captured + 1 redde x }
}
",
        "reducta_body_effect",
    );
}

#[test]
fn coordinate_arity_matches_the_source_rank() {
    assert_rejects_with_issue(
        r"
functio sum(matrix<numerus, [2, 3]> xs) → numerus {
    redde reducta via summa ex xs apud [i]
}
",
        "itera_apud_arity_mismatch",
    );
}

#[test]
fn grouped_reductions_require_integer_keys_and_supported_operators() {
    assert_rejects_with_issue(
        r"
functio no_coordinates(tensor<numerus, [3]> xs, tensor<numerus, [3]> keys) → tensor<numerus, [2]> {
    redde reducta via summa ex xs pro keys[0]
}
",
        "reducta_group_requires_apud",
    );
    assert_rejects_with_issue(
        r"
functio non_integer_key(tensor<numerus, [3]> xs, tensor<fractus, [3]> keys) → tensor<numerus, [2]> {
    redde reducta via summa ex xs apud [i] pro keys[i]
}
",
        "reducta_group_key_not_integer",
    );
    assert_rejects_with_issue(
        r"
functio grouped_argmax(tensor<numerus, [3]> xs, tensor<numerus, [3]> keys) → tensor<numerus, [2]> {
    redde reducta via argmaxima ex xs apud [i] pro keys[i]
}
",
        "reducta_group_op_unsupported",
    );
    assert_rejects_with_issue(
        r"
functio grouped_max(tensor<numerus, [3]> xs, tensor<numerus, [3]> keys) → tensor<numerus, [2]> {
    redde reducta via maxima ex xs apud [i] pro keys[i]
}
",
        "reducta_group_op_unsupported",
    );
}

// Need: `pro [i, j]` is a bracketed coordinate list; the result is a
// rank-N tensor whose rank equals the key count and `pro k` is `pro [k]`.

#[test]
fn grouped_reduction_with_two_keys_checks_against_a_rank_two_result() {
    assert_accepts(
        r"
functio buckets(tensor<numerus, [3]> xs, tensor<numerus, [3]> rows, tensor<numerus, [3]> cols) → tensor<numerus, [2, 4]> {
    redde reducta via summa ex xs apud [i] pro [rows[i], cols[i]]
}
",
    );
}

#[test]
fn grouped_reduction_key_count_must_match_the_result_rank() {
    assert_rejects_with_issue(
        r"
functio too_few(tensor<numerus, [3]> xs, tensor<numerus, [3]> keys) → tensor<numerus, [2, 4]> {
    redde reducta via summa ex xs apud [i] pro [keys[i]]
}
",
        "reducta_group_rank_mismatch",
    );
    assert_rejects_with_issue(
        r"
functio too_many(tensor<numerus, [3]> xs, tensor<numerus, [3]> rows, tensor<numerus, [3]> cols) → tensor<numerus, [2]> {
    redde reducta via summa ex xs apud [i] pro [rows[i], cols[i]]
}
",
        "reducta_group_rank_mismatch",
    );
}

#[test]
fn grouped_reduction_refuses_non_integral_keys_in_a_coordinate_list() {
    assert_rejects_with_issue(
        r"
functio non_integer(tensor<numerus, [3]> xs, tensor<numerus, [3]> rows, tensor<fractus, [3]> cols) → tensor<numerus, [2, 4]> {
    redde reducta via summa ex xs apud [i] pro [rows[i], cols[i]]
}
",
        "reducta_group_key_not_integer",
    );
}

#[test]
fn grouped_reduction_refuses_spread_and_non_literal_key_lists() {
    assert_rejects_with_issue(
        r"
functio spread(tensor<numerus, [3]> xs, lista<numerus> ks) → tensor<numerus, [2, 4]> {
    redde reducta via summa ex xs apud [i] pro [sparge ks]
}
",
        "reducta_group_keys_not_literal",
    );
    assert_rejects_with_issue(
        r"
functio variable(tensor<numerus, [3]> xs, lista<numerus> ks) → tensor<numerus, [2, 4]> {
    redde reducta via summa ex xs apud [i] pro ks
}
",
        "reducta_group_keys_not_literal",
    );
}

#[test]
fn a_single_key_and_a_one_element_list_agree() {
    assert_accepts(
        r"
functio bare(tensor<numerus, [3]> xs, tensor<numerus, [3]> keys) → tensor<numerus, [2]> {
    redde reducta via summa ex xs apud [i] pro keys[i]
}
functio listed(tensor<numerus, [3]> xs, tensor<numerus, [3]> keys) → tensor<numerus, [2]> {
    redde reducta via summa ex xs apud [i] pro [keys[i]]
}
",
    );
    assert_rejects_with_issue(
        r"
functio bare(tensor<numerus, [3]> xs, tensor<numerus, [3]> keys) → tensor<numerus, [2, 4]> {
    redde reducta via summa ex xs apud [i] pro keys[i]
}
",
        "reducta_group_rank_mismatch",
    );
    assert_rejects_with_issue(
        r"
functio listed(tensor<numerus, [3]> xs, tensor<numerus, [3]> keys) → tensor<numerus, [2, 4]> {
    redde reducta via summa ex xs apud [i] pro [keys[i]]
}
",
        "reducta_group_rank_mismatch",
    );
}

// Need: `t.summa(axis n)` / `t.media(axis n)` on a rank-2 tensor
// expand to the grouped reduction and drop the named axis from the shape.

#[test]
fn axis_reduction_drops_the_named_axis_from_a_rank_two_shape() {
    assert_accepts(
        r"
functio rows(tensor<f32, [2, 3]> g) → tensor<f32, [2]> { redde g.summa(axis 1) }
functio columns(tensor<f32, [2, 3]> g) → tensor<f32, [3]> { redde g.summa(axis 0) }
functio means(tensor<f32, [2, 3]> g) → tensor<f32, [2]> { redde g.media(axis 1) }
",
    );
}

#[test]
fn axis_reduction_result_shape_must_match_the_kept_axis() {
    assert_rejects_with_issue(
        r"
functio rows(tensor<f32, [2, 3]> g) → tensor<f32, [3]> { redde g.summa(axis 1) }
",
        "return_type_mismatch",
    );
}

#[test]
fn axis_reduction_out_of_range_axis_is_named() {
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [2, 3]> g) → tensor<f32, [2]> { redde g.summa(axis 2) }
",
        "axis_reduction_out_of_range",
    );
}

#[test]
fn axis_reduction_negative_axis_is_out_of_range() {
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [2, 3]> g) → tensor<f32, [2]> { redde g.summa(axis 0 - 1) }
",
        "axis_reduction_out_of_range",
    );
}

#[test]
fn axis_reduction_run_time_axis_is_named() {
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [2, 3]> g, numerus a) → tensor<f32, [2]> { redde g.summa(axis a) }
",
        "axis_reduction_not_constant",
    );
}

#[test]
fn axis_reduction_named_integer_local_is_a_constant_axis() {
    assert_accepts(
        r"
functio rows(tensor<f32, [2, 3]> g) → tensor<f32, [2]> {
    fixum numerus k ← 1
    redde g.summa(axis k)
}
",
    );
}

#[test]
fn axis_reduction_named_modular_local_uses_the_reduced_value() {
    // `modulus<u8>` stores 256 as 0, so the axis is 0 and the result is [3].
    assert_accepts(
        r"
functio cols(tensor<f32, [2, 3]> g) → tensor<f32, [3]> {
    fixum modulus<u8> k ← 256
    redde g.summa(axis k)
}
",
    );
}

#[test]
fn axis_reduction_named_saturating_local_uses_the_clamped_value() {
    // `saturatus<u8>` stores 300 as 255, which is no axis of a rank-two tensor.
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [2, 3]> g) → tensor<f32, [2]> {
    fixum saturatus<u8> k ← 300
    redde g.summa(axis k)
}
",
        "axis_reduction_out_of_range",
    );
}

#[test]
fn axis_reduction_named_modular_local_in_range_keeps_its_value() {
    assert_accepts(
        r"
functio rows(tensor<f32, [2, 3]> g) → tensor<f32, [2]> {
    fixum modulus<u8> k ← 1
    redde g.summa(axis k)
}
",
    );
}

#[test]
fn axis_reduction_named_local_chained_through_another_local() {
    assert_accepts(
        r"
functio rows(tensor<f32, [2, 3]> g) → tensor<f32, [2]> {
    fixum numerus a ← 1
    fixum numerus k ← a
    redde g.summa(axis k)
}
",
    );
}

#[test]
fn axis_reduction_named_float_local_is_not_a_constant_axis() {
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [2, 3]> g) → tensor<f32, [2]> {
    fixum fractus k ← 1
    redde g.summa(axis k)
}
",
        "axis_reduction_not_constant",
    );
}

#[test]
fn axis_reduction_drops_the_named_axis_from_a_rank_three_shape() {
    assert_accepts(
        r"
functio first(tensor<f32, [2, 3, 4]> g) → tensor<f32, [3, 4]> { redde g.summa(axis 0) }
functio second(tensor<f32, [2, 3, 4]> g) → tensor<f32, [2, 4]> { redde g.summa(axis 1) }
functio third(tensor<f32, [2, 3, 4]> g) → tensor<f32, [2, 3]> { redde g.summa(axis 2) }
functio mean(tensor<f32, [2, 3, 4]> g) → tensor<f32, [2, 4]> { redde g.media(axis 1) }
",
    );
}

#[test]
fn axis_reduction_rank_three_result_shape_must_match_the_kept_axes() {
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [2, 3, 4]> g) → tensor<f32, [2, 3]> { redde g.summa(axis 1) }
",
        "return_type_mismatch",
    );
}

#[test]
fn axis_reduction_rank_three_out_of_range_axis_is_named() {
    assert_accepts(
        r"
functio last(tensor<f32, [2, 3, 4]> g) → tensor<f32, [2, 3]> { redde g.summa(axis 2) }
",
    );
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [2, 3, 4]> g) → tensor<f32, [2, 3]> { redde g.summa(axis 3) }
",
        "axis_reduction_out_of_range",
    );
}

#[test]
fn axis_reduction_above_rank_eight_is_a_named_refusal() {
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [1, 1, 1, 1, 1, 1, 1, 1, 2]> g) → tensor<f32, [1, 1, 1, 1, 1, 1, 1, 1]> { redde g.summa(axis 8) }
",
        "axis_reduction_rank_unsupported",
    );
}

#[test]
fn axis_reduction_on_a_rank_one_tensor_is_a_named_refusal() {
    assert_rejects_with_issue(
        r"
functio bad(tensor<f32, [4]> g) → f32 { redde g.summa(axis 0) }
",
        "axis_reduction_rank_unsupported",
    );
}

#[test]
fn axis_reduction_mean_needs_a_float_element() {
    assert_rejects_with_issue(
        r"
functio bad(tensor<textus, [2, 3]> g) → tensor<f32, [2]> { redde g.media(axis 1) }
",
        "axis_reduction_mean_requires_float",
    );
}
