//! A non-empty brace literal ascribed (`∷`) to a genus constructs the genus
//! like `Genus { … }`, or is rejected at check with a named code. It never
//! passes check and fails late in MIR.

use crate::test_support;
use crate::{Type, TypeTable};
use radix_hir::{HirExpression, HirExpressionKind, HirStatementKind};

const GENUS: &str = "genus P { varia numerus x = 0 varia numerus y = 0 }";

fn entry_init(unit: &test_support::AnalyzedFixture) -> &HirExpression {
    let hir = unit.hir.as_ref().expect("hir");
    let entry = hir.entry.as_ref().expect("entry");
    entry
        .statements
        .iter()
        .find_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => local.init.as_ref(),
            _ => None,
        })
        .expect("entry local initializer")
}

fn is_genus(types: &TypeTable, ty: crate::TypeId) -> bool {
    matches!(types.get(ty), Type::Struct(_))
}

fn assert_constructs_genus(source: &str, fields: usize) {
    let unit = test_support::analyze_ok(source);
    let init = entry_init(&unit);
    let HirExpressionKind::Verte {
        target,
        entries: Some(entries),
        ..
    } = &init.kind
    else {
        panic!("expected a genus construction, got {:?}", init.kind);
    };
    assert!(is_genus(&unit.types, *target), "target is not the genus");
    assert_eq!(entries.len(), fields, "construction entries: {entries:?}");
}

#[test]
fn keyed_literal_ascribed_to_genus_constructs_the_genus() {
    assert_constructs_genus(
        &format!("{GENUS} incipit {{ fixum P p ← {{ x = 1, y = 2 }} ∷ P nota p.x }}"),
        2,
    );
}

#[test]
fn inferred_constructor_ascribed_to_genus_constructs_the_genus() {
    assert_constructs_genus(
        &format!("{GENUS} incipit {{ fixum P p ← _ {{ x = 1 }} ∷ P nota p.x }}"),
        1,
    );
}

#[test]
fn unknown_field_literal_ascribed_to_genus_is_named_error() {
    test_support::assert_rejects_with_issue(
        &format!("{GENUS} incipit {{ fixum P p ← {{ bogus = 1 }} ∷ P nota p.x }}"),
        "unknown_field",
    );
}

#[test]
fn missing_required_field_literal_ascribed_to_genus_is_named_error() {
    const REQUIRED: &str = "genus Q { varia numerus x varia numerus y = 0 }";
    test_support::assert_rejects_with_issue(
        &format!("{REQUIRED} incipit {{ fixum Q q ← {{ y = 1 }} ∷ Q nota q.y }}"),
        "missing_required_struct_field",
    );
}

#[test]
fn quoted_key_literal_ascribed_to_genus_is_named_error() {
    test_support::assert_rejects_with_issue(
        &format!("{GENUS} incipit {{ fixum P p ← {{ \"x\": 1 }} ∷ P nota p.x }}"),
        "inferred_constructor_entries_need_map",
    );
}

#[test]
fn quoted_key_literal_ascribed_to_map_is_still_accepted() {
    test_support::assert_accepts(
        "incipit { fixum tabula<textus, numerus> m ← { \"a\": 1 } ∷ tabula<textus, numerus> nota m }",
    );
}
