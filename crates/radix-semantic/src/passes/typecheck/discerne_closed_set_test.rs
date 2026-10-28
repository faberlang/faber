//! `discerne` matches a closed set (union members, enum variants) only.
//!
//! Operator ruling 2026-10-02: numeric-width and length-shape type patterns are
//! not set discrimination. A type pattern whose target is not a member of the
//! scrutinee's closed set is `discerne_pattern_not_in_closed_set`.

use crate::test_support;

const ISSUE: &str = "discerne_pattern_not_in_closed_set";
const LITERAL_ISSUE: &str = "discerne_literal_arm_not_in_closed_set";
const SUBJECT_ISSUE: &str = "discerne_subject_not_in_closed_set";

#[test]
fn bounded_list_pattern_over_unbounded_list_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"functio probe(lista<numerus> values) → vacuum {
    discerne values {
        casu lista<numerus, magnitudo 4> { tacet }
        casu lista<numerus> { tacet }
    }
}",
        ISSUE,
    );
}

#[test]
fn derived_width_list_pattern_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"functio probe(lista<numerus> values) → vacuum {
    discerne values {
        casu lista<numerus, N+1> { tacet }
        casu lista<numerus> { tacet }
    }
}",
        ISSUE,
    );
}

#[test]
fn numeric_width_pattern_over_numerus_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"functio probe(numerus values) → vacuum {
    discerne values {
        casu i32 { tacet }
        casu numerus { tacet }
    }
}",
        ISSUE,
    );
}

#[test]
fn rejection_names_pattern_and_scrutinee_types() {
    let diagnostics = test_support::analyze_and_get_diagnostics(
        r"functio probe(lista<numerus> values) → vacuum {
    discerne values {
        casu lista<numerus, magnitudo 4> { tacet }
        casu lista<numerus> { tacet }
    }
}",
    );
    let error = diagnostics
        .iter()
        .find(|err| test_support::semantic_error_has_issue(err, ISSUE))
        .expect("closed-set rejection");
    let names: Vec<&str> = error.args.iter().map(|arg| arg.name).collect();
    assert!(names.contains(&"pattern"), "args: {:?}", error.args);
    assert!(names.contains(&"scrutinee"), "args: {:?}", error.args);
}

#[test]
fn family_pattern_over_a_union_that_lacks_it_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"functio probe(lista<numerus, magnitudo 4> ∪ lista<numerus, magnitudo 8> values) → vacuum {
    discerne values {
        casu lista<numerus> { tacet }
    }
}",
        ISSUE,
    );
}

#[test]
fn union_member_type_pattern_is_accepted() {
    test_support::assert_accepts(
        r"functio probe(lista<numerus, magnitudo 4> ∪ lista<numerus, magnitudo 8> values) → vacuum {
    discerne values {
        casu lista<numerus, magnitudo 4> { tacet }
        casu lista<numerus, magnitudo 8> { tacet }
    }
}",
    );
}

#[test]
fn numeric_member_of_a_union_is_accepted() {
    test_support::assert_accepts(
        r"functio probe(i32 ∪ textus value) → vacuum {
    discerne value {
        casu i32 { tacet }
        casu textus { tacet }
    }
}",
    );
}

#[test]
fn literal_arm_over_numeric_subject_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"functio probe(numerus n) → vacuum {
    discerne n {
        casu 1 { tacet }
        casu _ { tacet }
    }
}",
        LITERAL_ISSUE,
    );
}

#[test]
fn literal_arm_refusal_row_carries_a_hint_in_en_and_la() {
    // The hint is the issue's own locale row, keyed by the issue slug. The
    // test pins that both reader packs ship the row and that the row has a
    // hint; the hint's prose is the pack's contract, not asserted here.
    for id in ["en", "la"] {
        let pack = crate::locale::LocalePack::from_toml_path(format!(
            "{}/../../locale/{id}/pack.toml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_else(|err| panic!("load reader pack `{id}`: {err:?}"));
        let row = pack
            .diagnostics
            .get("SEM010")
            .and_then(|code| code.issues.get(LITERAL_ISSUE))
            .unwrap_or_else(|| panic!("{id}: missing SEM010.{LITERAL_ISSUE} row"));
        row.help
            .as_ref()
            .unwrap_or_else(|| panic!("{id}: SEM010.{LITERAL_ISSUE} row has no hint"));
    }
}

#[test]
fn multi_subject_match_over_a_numeric_subject_is_rejected_by_name() {
    let diagnostics = test_support::analyze_and_get_diagnostics(
        r"discretio Forma { Circulus, Quadratum }

functio probe(Forma a, numerus n) → vacuum {
    discerne a et n {
        casu _ et _ { tacet }
    }
}",
    );
    let error = diagnostics
        .iter()
        .find(|err| test_support::semantic_error_has_issue(err, SUBJECT_ISSUE))
        .expect("multi-subject closed-set rejection");
    let position = error.args.iter().find(|arg| arg.name == "position");
    assert_eq!(position.map(|arg| arg.value.as_str()), Some("2"));
}

#[test]
fn multi_subject_match_over_text_subjects_is_rejected_by_name() {
    test_support::assert_rejects_with_issue(
        r"functio probe(textus a, textus b) → vacuum {
    discerne a et b {
        casu _ et _ { tacet }
    }
}",
        SUBJECT_ISSUE,
    );
}

#[test]
fn multi_subject_match_over_two_closed_sets_is_accepted() {
    test_support::assert_accepts(
        r"discretio Forma { Circulus, Quadratum }

functio probe(Forma a, bivalens b) → vacuum {
    discerne a et b {
        casu Circulus et verum { tacet }
        casu _ et _ { tacet }
    }
}",
    );
}

#[test]
fn single_subject_numeric_match_with_a_catchall_is_not_a_subject_refusal() {
    let diagnostics = test_support::analyze_and_get_diagnostics(
        r"functio probe(numerus n) → vacuum {
    discerne n {
        casu _ { tacet }
    }
}",
    );
    assert!(
        !diagnostics
            .iter()
            .any(|err| test_support::semantic_error_has_issue(err, SUBJECT_ISSUE)),
        "{diagnostics:?}"
    );
}

#[test]
fn bool_literal_arms_over_bivalens_are_accepted() {
    test_support::assert_accepts(
        r"functio probe(bivalens b) → vacuum {
    discerne b {
        casu verum { tacet }
        casu falsum { tacet }
    }
}",
    );
}

#[test]
fn same_variant_in_two_subject_positions_is_accepted() {
    test_support::assert_accepts(
        r"discretio Forma { Circulus, Quadratum }

functio probe(Forma a, Forma b) → vacuum {
    discerne a et b {
        casu Circulus et Circulus { tacet }
        casu _ et _ { tacet }
    }
}",
    );
}
