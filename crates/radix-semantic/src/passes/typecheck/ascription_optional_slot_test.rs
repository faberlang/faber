//! Regression coverage for need: a `∷` ascription on a literal with
//! `nihil` elements types the literal against the optional slot exactly as a
//! declaration cell does, instead of failing SEM010.

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
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
