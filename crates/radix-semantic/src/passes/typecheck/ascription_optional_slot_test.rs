//! Regression coverage for need: a `∷` ascription on a literal with
//! `nihil` elements types the literal against the optional slot exactly as a
//! declaration cell does, instead of failing SEM010.

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

fn accepts(result: &crate::SemanticResult) {
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects(result: &crate::SemanticResult) {
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn declaration_form_list_with_nihil_elements_checks() {
    accepts(&compile(
        r#"incipit {
  fixum lista<numerus ∪ nihil> xs ← [nulla, 1, 2, nulla]
  nota xs
}"#,
    ));
}

#[test]
fn ascribed_list_with_nihil_elements_checks() {
    accepts(&compile(
        r#"incipit {
  fixum lista<numerus ∪ nihil> xs ← [nulla, 1, 2, nulla] ∷ lista<numerus ∪ nihil>
  nota xs
}"#,
    ));
}

#[test]
fn ascribed_map_with_nihil_value_checks() {
    accepts(&compile(
        r#"incipit {
  fixum tabula<textus, numerus ∪ nihil> m ← {"a": null} ∷ tabula<textus, numerus ∪ nihil>
  nota m
}"#,
    ));
}

#[test]
fn ascribed_nested_list_with_nihil_elements_checks() {
    accepts(&compile(
        r#"incipit {
  fixum lista<lista<numerus ∪ nihil>> n ← [[nulla, 1], [2]] ∷ lista<lista<numerus ∪ nihil>>
  nota n
}"#,
    ));
}

#[test]
fn ascribed_list_with_wrong_element_still_rejects() {
    rejects(&compile(
        r#"incipit {
  fixum lista<numerus ∪ nihil> xs ← [nulla, "x"] ∷ lista<numerus ∪ nihil>
  nota xs
}"#,
    ));
}

// Need: an ascribed JSON-object literal is typed against the map the
// way a declaration cell types it.

#[test]
fn declaration_form_nested_json_map_rejects_requires_valor() {
    crate::test_support::assert_rejects_with_issue_la(
        r#"incipit {
  fixum tabula<textus, tabula<textus, numerus>> n ← {"a": {"b": 1}}
  nota n
}"#,
        "json_nested_value_requires_valor",
    );
}

#[test]
fn ascribed_nested_json_map_rejects_requires_valor() {
    crate::test_support::assert_rejects_with_issue_la(
        r#"incipit {
  fixum _ n ← {"a": {"b": 1}} ∷ tabula<textus, tabula<textus, numerus>>
  nota n
}"#,
        "json_nested_value_requires_valor",
    );
}

#[test]
fn declaration_form_list_of_json_maps_checks() {
    accepts(&compile(
        r#"incipit {
  fixum lista<tabula<textus, numerus>> l ← [{"a": 1}]
  nota l
}"#,
    ));
}

#[test]
fn ascribed_list_of_json_maps_checks() {
    accepts(&compile(
        r#"incipit {
  fixum _ l ← [{"a": 1}] ∷ lista<tabula<textus, numerus>>
  nota l
}"#,
    ));
}

#[test]
fn ascribed_list_of_json_maps_with_wrong_value_rejects() {
    rejects(&compile(
        r#"incipit {
  fixum _ l ← [{"a": "x"}] ∷ lista<tabula<textus, numerus>>
  nota l
}"#,
    ));
}
