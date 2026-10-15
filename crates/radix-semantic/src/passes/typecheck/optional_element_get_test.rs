//! need: the optional read (`accipe`) over a container whose element
//! or value is already optional is one `T ∪ nihil`, not a nested optional. An
//! optional of an optional is the same optional, so a plain optional cell takes
//! the read directly, `vel` and flow narrowing apply once, and a plain cell is
//! rejected exactly once with `SEM010 optional_into_plain_cell`.
//!
//! The text read needs no case here: a text element is a one-character
//! `textus`, never optional, so `text_get_test` already pins its single level.
//!
//! Fixtures use the latin spellings (the unit-test harness has no en pack).

use crate::Primitive;
use crate::test_support::{
    analyze_ok_la, analyze_source_la, assert_accepts_la, assert_rejects_with_issue_la,
    entry_local_tys,
};

const ISSUE: &str = "optional_into_plain_cell";

/// The second `incipit` local must be a single-level optional of `numerus`.
fn assert_single_level_numerus_optional(source: &str, local: usize) {
    let unit = analyze_ok_la(source);
    let tys = entry_local_tys(&unit);
    let ty = tys[local];
    let inner = unit
        .types
        .nullable_inner(ty)
        .unwrap_or_else(|| panic!("read must be optional, got {:?}", unit.types.get(ty)));
    assert!(
        unit.types.nullable_inner(inner).is_none(),
        "read must be one optional level, got {:?} over {:?}",
        unit.types.get(ty),
        unit.types.get(inner)
    );
    assert!(
        matches!(
            unit.types.get(inner),
            crate::Type::Primitive(Primitive::Numerus)
        ),
        "payload must be numerus, got {:?}",
        unit.types.get(inner)
    );
}

#[test]
fn list_get_over_optional_element_is_one_optional() {
    assert_single_level_numerus_optional(
        r"incipit {
    sit lista<numerus ∪ nihil> xs ← [1, nulla, 3]
    sit _ found ← xs.accipe(0)
    nota found
}",
        1,
    );
}

#[test]
fn map_get_over_optional_value_is_one_optional() {
    assert_single_level_numerus_optional(
        r#"incipit {
    sit tabula<textus, numerus ∪ nihil> m ← { "a": 1 }
    sit _ found ← m.accipe("a")
    nota found
}"#,
        1,
    );
}

#[test]
fn list_get_over_optional_element_stores_into_optional_cell() {
    assert_accepts_la(
        r"incipit {
    sit lista<numerus ∪ nihil> xs ← [1, nulla, 3]
    varia numerus ∪ nihil a ← xs.accipe(0)
    a ← xs.accipe(1)
    nota a
}",
    );
}

#[test]
fn map_get_over_optional_value_stores_into_optional_cell() {
    assert_accepts_la(
        r#"incipit {
    sit tabula<textus, numerus ∪ nihil> m ← { "a": 1 }
    sit numerus ∪ nihil a ← m.accipe("a")
    nota a
}"#,
    );
}

#[test]
fn list_get_over_optional_element_into_plain_cell_is_rejected_once() {
    let result = analyze_source_la(
        r"incipit {
    sit lista<numerus ∪ nihil> xs ← [1, nulla, 3]
    sit numerus a ← xs.accipe(0)
    nota a
}",
    );
    let hits = result
        .diagnostics
        .iter()
        .filter(|diagnostic| format!("{diagnostic:?}").contains(ISSUE))
        .count();
    assert_eq!(hits, 1, "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn map_get_over_optional_value_into_plain_cell_is_rejected() {
    assert_rejects_with_issue_la(
        r#"incipit {
    sit tabula<textus, numerus ∪ nihil> m ← { "a": 1 }
    sit numerus a ← m.accipe("a")
    nota a
}"#,
        ISSUE,
    );
}

#[test]
fn list_get_over_optional_element_composes_with_vel() {
    assert_accepts_la(
        r"incipit {
    sit lista<numerus ∪ nihil> xs ← [1, nulla, 3]
    sit numerus a ← xs.accipe(1) vel 7
    nota a
}",
    );
}

#[test]
fn list_get_over_optional_element_narrows_once() {
    assert_accepts_la(
        r"incipit {
    sit lista<numerus ∪ nihil> xs ← [1, nulla, 3]
    sit _ a ← xs.accipe(0)
    si a non est nihil {
        sit numerus b ← a
        nota b
    }
}",
    );
}

#[test]
fn list_get_over_plain_element_stays_one_optional() {
    assert_single_level_numerus_optional(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3]
    sit _ found ← xs.accipe(0)
    nota found
}",
        1,
    );
}
