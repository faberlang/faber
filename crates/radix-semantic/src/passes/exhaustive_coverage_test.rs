//! Source-level coverage tests for D4.8: `bivalens` is the closed set
//! `{verum, falsum}`, and multi-scrutinee `discerne` gets product coverage.
//!
//! These run the full driver (parse → lower → typecheck → exhaustive →
//! `return_path`) and assert on diagnostic kind, `issue`, and the `missing`
//! witness argument — never on rendered prose.

use crate::SemanticError;
use crate::SemanticErrorKind;
use crate::WarningKind;
use crate::test_support::analyze_source;

fn arg<'a>(diag: &'a SemanticError, name: &str) -> Option<&'a str> {
    diag.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn non_exhaustive(source: &str) -> Vec<SemanticError> {
    analyze_source(source)
        .diagnostics
        .iter()
        .filter(|diag| diag.kind == SemanticErrorKind::NonExhaustiveMatch)
        .cloned()
        .collect()
}

fn assert_clean(source: &str) {
    let result = analyze_source(source);
    let facts: Vec<_> = result
        .diagnostics
        .iter()
        .map(|diag| (diag.kind, diag.issue().map(str::to_owned)))
        .collect();
    assert!(result.is_ok(), "expected clean analysis, got {facts:?}");
    assert!(
        !result.diagnostics.iter().any(|diag| matches!(
            diag.kind,
            SemanticErrorKind::Warning(WarningKind::CoverageUnchecked)
        )),
        "expected coverage to be computed, got {facts:?}"
    );
}

/// A literal arm in `discerne` is refused: `discerne` matches closed sets only.
fn assert_literal_arm_refused(source: &str) {
    let result = analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|diag| {
            diag.kind == SemanticErrorKind::TypeMismatch
                && diag.issue() == Some("discerne_literal_arm_not_in_closed_set")
        }),
        "expected discerne_literal_arm_not_in_closed_set, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.kind, diag.issue()))
            .collect::<Vec<_>>()
    );
}

fn assert_missing(source: &str, issue: &str, missing: &str) {
    let errors = non_exhaustive(source);
    assert!(
        errors
            .iter()
            .any(|diag| diag.issue() == Some(issue) && arg(diag, "missing") == Some(missing)),
        "expected SEM040 {issue} missing {missing:?}, got {:?}",
        errors
            .iter()
            .map(|diag| (diag.issue(), arg(diag, "missing")))
            .collect::<Vec<_>>()
    );
}

// ── probe 1: a bool match covering verum and falsum definitely returns ──────

#[test]
fn bool_match_covering_both_values_definitely_returns() {
    assert_clean(
        "functio f(bivalens b) → numerus {
            discerne b {
                casu verum { redde 1 }
                casu falsum { redde 0 }
            }
        }",
    );
}

// ── probe 2: two bool scrutinees with all four combinations ─────────────────

#[test]
fn two_bool_scrutinees_with_all_four_combinations_definitely_return() {
    assert_clean(
        "functio g(bivalens a, bivalens b) → numerus {
            discerne a et b {
                casu verum et verum { redde 3 }
                casu verum et falsum { redde 2 }
                casu falsum et verum { redde 1 }
                casu falsum et falsum { redde 0 }
            }
        }",
    );
}

#[test]
fn two_bool_scrutinees_with_wildcard_rows_definitely_return() {
    assert_clean(
        "functio g(bivalens a, bivalens b) → numerus {
            discerne a et b {
                casu verum et _ { redde 1 }
                casu _ et verum { redde 2 }
                casu falsum et falsum { redde 0 }
            }
        }",
    );
}

// ── probe 3: a statement-level bool match missing falsum ────────────────────

#[test]
fn statement_bool_match_missing_falsum_is_rejected() {
    assert_missing(
        "incipit {
            fixum bivalens b ← verum
            discerne b {
                casu verum { nota \"sic\" }
            }
        }",
        "missing_cases",
        "falsum",
    );
}

#[test]
fn two_bool_scrutinees_missing_one_combination_names_it() {
    assert_missing(
        "functio g(bivalens a, bivalens b) → numerus {
            discerne a et b {
                casu verum et verum { redde 3 }
                casu verum et falsum { redde 2 }
                casu falsum et verum { redde 1 }
            }
            redde 0
        }",
        "missing_cases",
        "(falsum, falsum)",
    );
}

#[test]
fn enum_by_bool_product_missing_combination_names_it() {
    assert_missing(
        "discretio Lumen { Rubeum, Viride }
        functio h(Lumen l, bivalens b) → numerus {
            discerne l et b {
                casu Rubeum et _ { redde 1 }
                casu Viride et verum { redde 2 }
            }
            redde 0
        }",
        "missing_cases",
        "(Viride, falsum)",
    );
}

#[test]
fn enum_by_bool_product_fully_covered_definitely_returns() {
    assert_clean(
        "discretio Lumen { Rubeum, Viride }
        functio h(Lumen l, bivalens b) → numerus {
            discerne l et b {
                casu Rubeum et _ { redde 1 }
                casu Viride et verum { redde 2 }
                casu Viride et falsum { redde 3 }
            }
        }",
    );
}

#[test]
fn two_enum_subjects_with_every_cell_covered_definitely_return() {
    assert_clean(
        "discretio Lumen { Rubeum, Viride }
        discretio Forma { Circulus, Quadratum }
        functio h(Lumen l, Forma f) → numerus {
            discerne l et f {
                casu Rubeum et Circulus { redde 1 }
                casu Rubeum et Quadratum { redde 2 }
                casu Viride et _ { redde 3 }
            }
        }",
    );
}

#[test]
fn two_enum_subjects_missing_one_cell_names_it() {
    assert_missing(
        "discretio Lumen { Rubeum, Viride }
        discretio Forma { Circulus, Quadratum }
        functio h(Lumen l, Forma f) → numerus {
            discerne l et f {
                casu Rubeum et Circulus { redde 1 }
                casu Rubeum et Quadratum { redde 2 }
                casu Viride et Circulus { redde 3 }
            }
            redde 0
        }",
        "missing_cases",
        "(Viride, Quadratum)",
    );
}

#[test]
fn single_enum_missing_variant_names_it() {
    assert_missing(
        "discretio Lumen { Rubeum, Viride, Caeruleum }
        functio h(Lumen l) → numerus {
            discerne l {
                casu Rubeum { redde 1 }
                casu Caeruleum { redde 3 }
            }
            redde 0
        }",
        "missing_variants",
        "Viride",
    );
}

#[test]
fn literal_match_over_open_domain_without_catchall_is_rejected() {
    assert_literal_arm_refused(
        "functio k(numerus n) → numerus {
            discerne n {
                casu 1 { redde 10 }
                casu 2 { redde 20 }
            }
            redde 0
        }",
    );
}

#[test]
fn multi_scrutinee_literal_rows_with_ceterum_definitely_return() {
    assert_clean(
        "functio k(bivalens a, bivalens b) → numerus {
            discerne a et b {
                casu verum et verum { redde 10 }
                ceterum { redde 0 }
            }
        }",
    );
}

#[test]
fn multi_scrutinee_missing_row_is_no_longer_silent() {
    // Before D4.8 a multi-scrutinee statement match was skipped outright.
    assert_missing(
        "incipit {
            fixum bivalens a ← verum
            fixum bivalens b ← verum
            discerne a et b {
                casu verum et verum { nota \"unum\" }
                casu _ et falsum { nota \"falsum\" }
            }
        }",
        "missing_cases",
        "(falsum, verum)",
    );
}

// ── elige: implicit no-op default over open spaces only ─────────────────────

#[test]
fn elige_over_open_space_without_ceterum_keeps_switch_default() {
    assert_clean(
        "incipit {
            fixum numerus n ← 1
            elige n {
                casu 1 { nota \"unum\" }
                casu 2 { nota \"duo\" }
            }
        }",
    );
}

#[test]
fn elige_over_bivalens_missing_falsum_is_rejected() {
    assert_missing(
        "incipit {
            fixum bivalens b ← verum
            elige b {
                casu verum { nota \"sic\" }
            }
        }",
        "missing_cases",
        "falsum",
    );
}

#[test]
fn negated_literal_rows_are_open_space_values() {
    assert_literal_arm_refused(
        "functio s(numerus n) → numerus {
            discerne n {
                casu -1 { redde 1 }
                casu 0 { redde 0 }
            }
            redde 2
        }",
    );
}
