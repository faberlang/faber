//! N0 enum variant refinement and access-honesty proofs.
//!
//! Variant members use the existing nominal `Type::Struct` carrier with the
//! variant `DefId`; this keeps the refinement lattice closed over `TypeId`s
//! without adding a new shared type-table node.

use crate::test_support;

const CARRIER: &str = r"genus ErrorDetail {
    textus body
}

discretio Carrier {
    DetailCarrier { ErrorDetail detail },
    TextCarrier { textus text }
}

";

#[test]
fn enum_variant_payload_refines_through_return() {
    test_support::assert_accepts(&format!(
        "{CARRIER}functio probe(Carrier r) → textus {{\n    adfirma r est DetailCarrier\n    redde r.detail.body\n}}"
    ));
}

#[test]
fn enum_variant_access_without_refinement_is_diagnostic() {
    test_support::assert_rejects_with_issue(
        &format!("{CARRIER}functio probe(Carrier r) → textus {{ redde r.detail.body }}"),
        "union_member_not_common",
    );
}

#[test]
fn enum_variant_non_est_refines_the_other_member() {
    test_support::assert_accepts(&format!(
        "{CARRIER}functio probe(Carrier r) → textus {{\n    adfirma r non est TextCarrier\n    redde r.detail.body\n}}"
    ));
}
