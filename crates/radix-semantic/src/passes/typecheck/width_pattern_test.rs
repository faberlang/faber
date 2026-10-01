//! N2 width and shape pattern proofs.
//!
//! Width patterns are carrier predicates. A bounded arm is more specific than
//! its unbounded family arm, and a family arm is the exhaustiveness catch-all.

use crate::test_support;

#[test]
fn bounded_before_unbounded_family_is_reachable() {
    test_support::assert_accepts(
        r"functio probe(lista<numerus> values) → vacuum {
    discerne values {
        casu lista<numerus, 4> { tacet }
        casu lista<numerus> { tacet }
    }
}",
    );
}

#[test]
fn unbounded_family_before_bounded_arm_is_unreachable() {
    test_support::assert_rejects_with_issue(
        r"functio probe(lista<numerus> values) → vacuum {
    discerne values {
        casu lista<numerus> { tacet }
        casu lista<numerus, 4> { tacet }
    }
}",
        "after_width_pattern",
    );
}

#[test]
fn capacity_hole_is_a_family_catchall() {
    test_support::assert_accepts(
        r"functio probe(lista<numerus, 4> values) → vacuum {
    discerne values {
        casu lista<numerus, _> { tacet }
    }
}",
    );
}

#[test]
fn family_catchall_covers_bounded_union_members() {
    test_support::assert_accepts(
        r"functio probe(lista<numerus, 4> ∪ lista<numerus, 8> values) → vacuum {
    discerne values {
        casu lista<numerus> { tacet }
    }
}",
    );
}

#[test]
fn bounded_pattern_accepts_a_union_member_and_preserves_specificity() {
    test_support::assert_accepts(
        r"functio probe(lista<numerus, 4> ∪ lista<numerus, 8> values) → vacuum {
    discerne values {
        casu lista<numerus, 4> { tacet }
        casu lista<numerus> { tacet }
    }
}",
    );
}

#[test]
fn tensor_figure_pattern_checks_against_tensor_carrier() {
    test_support::assert_accepts(
        r"functio probe(tensor<f32, [2, 4]> values) → vacuum {
    discerne values {
        casu tensor<f32, [2, 4]> { tacet }
    }
}",
    );
}

#[test]
fn numerus_width_is_checkable_but_not_a_family_catchall() {
    test_support::assert_accepts(
        r"functio probe(numerus values) → vacuum {
    discerne values {
        casu i32 { tacet }
        casu numerus { tacet }
    }
}",
    );
}
