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
    itera ex m apud [i, j] sit cell {
        sit f32 x ← cell
        sit numerus a ← i
        sit numerus b ← j
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
    itera ex t apud [i, j, k] sit cell {
        sit f32 x ← cell
        sit numerus a ← i
        sit numerus b ← j
        sit numerus c ← k
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
    itera ex t apud [i] sit cell {
        sit f32 x ← cell
        sit numerus a ← i
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
    itera ex m apud [i, j] sit cell {
        sit f32 x ← cell
        sit numerus a ← i
        sit numerus b ← j
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
    itera ex t sit cell {
        sit f32 x ← cell
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
    itera ex m apud [i] sit cell {
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
    itera ex m sit cell {
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
    itera ex m apud [i, j, k] sit cell {
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
    itera ex m apud [i] sit cell {
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
    itera ex m sit cell {
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
    itera ab 0‥3 apud [i] sit x {
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
    itera de xs apud [i] sit k {
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
fn apud_on_ragged_nested_list_rejects_with_rank1_arity() {
    // D3.1: a lista is rank 1 — `xs[i]` takes one index — so two names are
    // an arity mismatch, never a ragged rank-2 walk.
    assert_rejects_arity(
        r"
functio ragged(lista<lista<numerus>> pairs) → vacuum {
    itera ex pairs apud [i, j] sit cell {
        tacet
    }
}",
        "1",
        "2",
    );
}

#[test]
fn apud_on_plain_list_binds_numerus_index_and_element() {
    test_support::assert_accepts(
        r"
functio flat(lista<textus> xs) → vacuum {
    itera ex xs apud [i] sit cell {
        sit numerus n ← i
        sit textus t ← cell
        nota n, t
    }
}",
    );
}

#[test]
fn apud_on_tabula_binds_key_and_value() {
    test_support::assert_accepts(
        r"
functio pairs(tabula<textus, numerus> m) → vacuum {
    itera ex m apud [k] sit v {
        sit textus key ← k
        sit numerus value ← v
        nota key, value
    }
}",
    );
}

#[test]
fn apud_on_tuple_key_tabula_binds_each_key_part() {
    test_support::assert_accepts(
        r"
functio sales(tabula<iuncta<textus, numerus>, numerus> m) → vacuum {
    itera ex m apud [regio, annus] sit v {
        sit textus r ← regio
        sit numerus a ← annus
        sit numerus value ← v
        nota r, a, value
    }
}",
    );
}

#[test]
fn apud_on_tuple_key_tabula_with_one_name_rejects_arity() {
    assert_rejects_arity(
        r"
functio sales(tabula<iuncta<textus, numerus>, numerus> m) → vacuum {
    itera ex m apud [k] sit v {
        tacet
    }
}",
        "2",
        "1",
    );
}

fn assert_rejects_indexed(source: &str) {
    let result = test_support::analyze_source(source);
    let err = find_error(&result, "itera_apud_requires_indexed_iterable").unwrap_or_else(|| {
        panic!(
            "missing itera_apud_requires_indexed_iterable in {:?}",
            result.diagnostics
        )
    });
    assert_eq!(crate::semantic_spec(err.kind).code, "SEM010");
}

#[test]
fn apud_on_copia_rejects_as_not_indexed() {
    assert_rejects_indexed(
        r"
functio members(copia<numerus> s) → vacuum {
    itera ex s apud [i] sit x {
        tacet
    }
}",
    );
}

#[test]
fn apud_on_cursor_rejects_as_not_indexed() {
    assert_rejects_indexed(
        r"
functio stream(cursor<numerus> c) → vacuum {
    itera ex c apud [i] sit x {
        tacet
    }
}",
    );
}

#[test]
fn apud_on_sparsa_rejects_fail_closed() {
    assert_rejects_indexed(
        r"
functio sparse(sparsa<f32, [2, 3]> s) → vacuum {
    itera ex s apud [i, j] sit cell {
        tacet
    }
}",
    );
}
