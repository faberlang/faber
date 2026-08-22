//! AC-5: `apud` coordinate iteration typechecking — tensor/matrix element
//! dispatch, arity-vs-rank rule, mode rule, and fail-closed decline shapes.
//!
//! Assertions are structured (`code` + `issue` + args), never rendered prose.

use crate::test_support::{self, AnalyzedFixture};

/// First error diagnostic carrying `issue`, if any.
fn find_error(result: &AnalyzedFixture, issue: &str) -> Option<crate::SemanticError> {
    result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && err.issue() == Some(issue))
        .cloned()
}

/// Assert the diagnostic's catalog code and a named string arg value.
fn assert_code_and_arg(err: &crate::SemanticError, code: &str, name: &str, value: &str) {
    assert_eq!(
        crate::semantic_spec(err.kind).code,
        code,
        "diagnostic code for issue {:?}",
        err.issue()
    );
    assert_eq!(
        err.args
            .iter()
            .find(|arg| arg.name == name)
            .map(|arg| arg.value.as_str()),
        Some(value),
        "diagnostic arg {name:?} for issue {:?}",
        err.issue()
    );
}

fn assert_rejects_arity(source: &str, expected: &str, actual: &str) {
    let result = test_support::analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected arity rejection, got: {:?}",
        result.diagnostics
    );
    let err = find_error(&result, "itera_apud_arity_mismatch").unwrap_or_else(|| {
        panic!(
            "missing itera_apud_arity_mismatch in {:?}",
            result.diagnostics
        )
    });
    assert_code_and_arg(&err, "SEM010", "expected", expected);
    assert_code_and_arg(&err, "SEM010", "actual", actual);
}

#[test]
fn rank2_tensor_apud_typechecks_element_and_numerus_coordinates() {
    test_support::assert_accepts(
        r"
functio grid(tensor<f32, [2, 3]> m) → vacuum {
    itera ex m apud [i, j] fixum cell {
        fixum f32 x ← cell
        fixum numerus a ← i
        fixum numerus b ← j
        nota x, a, b
    }
}",
    );
}

#[test]
fn rank3_tensor_apud_typechecks_three_coordinates() {
    test_support::assert_accepts(
        r"
functio cube(tensor<f32, [2, 3, 4]> t) → vacuum {
    itera ex t apud [i, j, k] fixum cell {
        fixum f32 x ← cell
        fixum numerus a ← i
        fixum numerus b ← j
        fixum numerus c ← k
        nota x, a, b, c
    }
}",
    );
}

#[test]
fn rank1_tensor_apud_single_coordinate_typechecks() {
    test_support::assert_accepts(
        r"
functio line(tensor<f32, [8]> t) → vacuum {
    itera ex t apud [i] fixum cell {
        fixum f32 x ← cell
        fixum numerus a ← i
        nota x, a
    }
}",
    );
}

#[test]
fn matrix_apud_typechecks_element_and_two_coordinates() {
    test_support::assert_accepts(
        r"
functio grid(matrix<f32, [2, 3]> m) → vacuum {
    itera ex m apud [i, j] fixum cell {
        fixum f32 x ← cell
        fixum numerus a ← i
        fixum numerus b ← j
        nota x, a, b
    }
}",
    );
}

#[test]
fn rank1_tensor_without_apud_keeps_element_binding() {
    // Route T: coordinate-free rank-1 literal tensor iteration keeps binding
    // the element type (the existing behavior must not regress).
    test_support::assert_accepts(
        r"
functio line(tensor<f32, [8]> t) → vacuum {
    itera ex t fixum cell {
        fixum f32 x ← cell
        nota x
    }
}",
    );
}

#[test]
fn rank2_apud_single_coordinate_rejects_with_arity_issue() {
    assert_rejects_arity(
        r"
functio grid(tensor<f32, [2, 3]> m) → vacuum {
    itera ex m apud [i] fixum cell {
        tacet
    }
}",
        "2",
        "1",
    );
}

#[test]
fn rank2_tensor_without_apud_rejects_with_arity_issue() {
    assert_rejects_arity(
        r"
functio grid(tensor<f32, [2, 3]> m) → vacuum {
    itera ex m fixum cell {
        tacet
    }
}",
        "2",
        "0",
    );
}

#[test]
fn rank2_apud_three_coordinates_rejects_with_arity_issue() {
    assert_rejects_arity(
        r"
functio grid(tensor<f32, [2, 3]> m) → vacuum {
    itera ex m apud [i, j, k] fixum cell {
        tacet
    }
}",
        "2",
        "3",
    );
}

#[test]
fn matrix_apud_wrong_arity_rejects_with_arity_issue() {
    assert_rejects_arity(
        r"
functio grid(matrix<f32, [2, 3]> m) → vacuum {
    itera ex m apud [i] fixum cell {
        tacet
    }
}",
        "2",
        "1",
    );
}

#[test]
fn matrix_without_apud_rejects_with_arity_issue() {
    assert_rejects_arity(
        r"
functio grid(matrix<f32, [2, 3]> m) → vacuum {
    itera ex m fixum cell {
        tacet
    }
}",
        "2",
        "0",
    );
}

#[test]
fn apud_on_ab_rejects_with_mode_issue() {
    let result = test_support::analyze_source(
        r"
functio count() → vacuum {
    itera ab 0‥3 apud [i] fixum x {
        tacet
    }
}",
    );
    assert!(
        !result.is_ok(),
        "expected mode rejection, got: {:?}",
        result.diagnostics
    );
    let err = find_error(&result, "itera_apud_requires_ex")
        .unwrap_or_else(|| panic!("missing itera_apud_requires_ex in {:?}", result.diagnostics));
    assert_eq!(crate::semantic_spec(err.kind).code, "SEM010");
}

#[test]
fn apud_on_de_rejects_with_mode_issue() {
    let result = test_support::analyze_source(
        r"
functio indices(lista<numerus> xs) → vacuum {
    itera de xs apud [i] fixum k {
        tacet
    }
}",
    );
    assert!(
        !result.is_ok(),
        "expected mode rejection, got: {:?}",
        result.diagnostics
    );
    let err = find_error(&result, "itera_apud_requires_ex")
        .unwrap_or_else(|| panic!("missing itera_apud_requires_ex in {:?}", result.diagnostics));
    assert_eq!(crate::semantic_spec(err.kind).code, "SEM010");
}

#[test]
fn apud_on_ragged_nested_list_rejects_fail_closed() {
    let result = test_support::analyze_source(
        r"
functio ragged(lista<lista<numerus>> pairs) → vacuum {
    itera ex pairs apud [i, j] fixum cell {
        tacet
    }
}",
    );
    assert!(
        !result.is_ok(),
        "expected fail-closed rejection, got: {:?}",
        result.diagnostics
    );
    let err = find_error(&result, "itera_apud_requires_shaped_iterable").unwrap_or_else(|| {
        panic!(
            "missing itera_apud_requires_shaped_iterable in {:?}",
            result.diagnostics
        )
    });
    assert_eq!(crate::semantic_spec(err.kind).code, "SEM010");
}

#[test]
fn apud_on_plain_list_rejects_fail_closed() {
    let result = test_support::analyze_source(
        r"
functio flat(lista<numerus> xs) → vacuum {
    itera ex xs apud [i] fixum cell {
        tacet
    }
}",
    );
    assert!(
        !result.is_ok(),
        "expected fail-closed rejection, got: {:?}",
        result.diagnostics
    );
    let err = find_error(&result, "itera_apud_requires_shaped_iterable").unwrap_or_else(|| {
        panic!(
            "missing itera_apud_requires_shaped_iterable in {:?}",
            result.diagnostics
        )
    });
    assert_eq!(crate::semantic_spec(err.kind).code, "SEM010");
}

#[test]
fn apud_on_sparsa_rejects_fail_closed() {
    let result = test_support::analyze_source(
        r"
functio sparse(sparsa<f32, [2, 3]> s) → vacuum {
    itera ex s apud [i, j] fixum cell {
        tacet
    }
}",
    );
    assert!(
        !result.is_ok(),
        "expected fail-closed rejection, got: {:?}",
        result.diagnostics
    );
    let err = find_error(&result, "itera_apud_requires_shaped_iterable").unwrap_or_else(|| {
        panic!(
            "missing itera_apud_requires_shaped_iterable in {:?}",
            result.diagnostics
        )
    });
    assert_eq!(crate::semantic_spec(err.kind).code, "SEM010");
}
