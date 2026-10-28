//! The store's float width reaches an unsized float literal receiver of a
//! float method, the way it reaches the operands of `2.0 * 3.0`: math has no
//! width until the store, and the store's type decides.

use crate::test_support;
use radix_types::MirMathFunction;

fn issue_is_expression_mismatch(source: &str) {
    test_support::assert_rejects_with_issue(source, "expression_type_mismatch");
}

#[test]
fn every_scalar_function_of_a_literal_stores_into_f32_and_f64() {
    for function in MirMathFunction::SCALAR {
        let name = function.latin_name();
        for ty in ["f32", "f64"] {
            test_support::assert_accepts(&format!(
                "functio lit() → {ty} {{\n    sit {ty} y ← (8.0).{name}()\n    redde y\n}}"
            ));
        }
    }
}

#[test]
fn a_negated_literal_receiver_takes_the_store_width() {
    test_support::assert_accepts(
        "functio lit() → f32 {\n    sit f32 y ← (-2.0).absolutum()\n    redde y\n}",
    );
}

#[test]
fn glyph_radicals_of_a_literal_store_into_f32() {
    for glyph in ["√", "∛", "∜"] {
        test_support::assert_accepts(&format!(
            "functio lit() → f32 {{\n    sit f32 y ← {glyph} 81.0\n    redde y\n}}"
        ));
    }
}

#[test]
fn a_method_chain_over_a_literal_takes_the_store_width() {
    test_support::assert_accepts(
        "functio lit() → f32 {\n    sit f32 y ← (64.0).radix().radix_cubica()\n    redde y\n}",
    );
}

#[test]
fn the_width_reaches_a_literal_receiver_through_return_assignment_and_argument() {
    test_support::assert_accepts("functio lit() → f32 {\n    redde (8.0).radix_cubica()\n}");
    test_support::assert_accepts(
        "functio lit() → f32 {\n    varia f32 y ← 0.0\n    y ← (81.0).radix()\n    redde y\n}",
    );
    test_support::assert_accepts(
        "functio take(f32 x) → f32 { redde x }\nfunctio lit() → f32 {\n    redde take((81.0).radix())\n}",
    );
}

#[test]
fn a_literal_receiver_with_no_expected_width_still_defaults_to_f64() {
    // No store width reaches the receiver: it defaults as before to an f64, so
    // its result lands in an f64 slot.
    test_support::assert_accepts("functio lit() {\n    (8.0).radix_cubica()\n}");
    test_support::assert_accepts(
        "functio lit() → f64 {\n    sit f64 y ← (8.0).radix_cubica() + 1.0\n    redde y\n}",
    );
}

#[test]
fn a_typed_receiver_is_never_reshaped_by_the_store() {
    // Only an unsized literal takes the store's width; an f64 value stored into
    // an f32 slot is still a mismatch.
    issue_is_expression_mismatch(
        "functio lit(f64 x) → f32 {\n    sit f32 y ← x.radix_cubica()\n    redde y\n}",
    );
    issue_is_expression_mismatch(
        "functio lit() → f32 {\n    sit f64 x ← 8.0\n    sit f32 y ← x.radix_cubica()\n    redde y\n}",
    );
}
