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
