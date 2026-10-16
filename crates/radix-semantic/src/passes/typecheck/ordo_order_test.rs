//! Operator ruling 2026-10-15: `≺ ≻ ≤ ≥` order two values of
//! one `ordo` by their declared discriminants, not by declaration position.
//!
//! Typecheck records the ordered `(variant, declared value)` rows keyed by the
//! comparison's left operand so lowering converts both operands to those
//! integers. Different enum definitions keep the ordinary mismatch path.

use crate::test_support::{analyze_ok_la, assert_rejects_with_issue_la};

const STATUS: &str = "ordo Status { pendens = 10, actum, finitum = -3, deletum }\n";

fn only_order_rows(source: &str) -> Vec<(radix_hir::DefId, i64)> {
    let fixture = analyze_ok_la(source);
    let comparisons = &fixture.result.tensor_edge_policies.ordo_order.comparisons;
    assert_eq!(comparisons.len(), 1, "comparisons: {comparisons:?}");
    comparisons.values().next().expect("one row set").clone()
}

#[test]
fn ordo_order_compare_typechecks() {
    let rows = only_order_rows(&format!(
        "{STATUS}incipit {{ nota Status.pendens ≺ Status.actum }}"
    ));
    let values: Vec<i64> = rows.iter().map(|(_, value)| *value).collect();
    assert_eq!(values, vec![10, 11, -3, -2]);
}

#[test]
fn ordo_order_compare_different_enums_rejects() {
    assert_rejects_with_issue_la(
        "ordo Color { rubrum, viridis }\nordo Gradus { primus, secundus }\nincipit { nota Color.rubrum ≺ Gradus.primus }",
        "numeric_operands_required",
    );
}
