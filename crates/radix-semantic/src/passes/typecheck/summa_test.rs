//! SFR-3: `summa ex` typecheck admission laws — shared apud arity rule,
//! numeric-scalar terms, exactly-one-`redde` pure body (fragment-compatible
//! effect subset), `filum` placement inside `@ nucleum` kernels, and the v1
//! tail gate (`K % W ≠ 0` fails closed at admission).
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

fn assert_rejects_with_issue(source: &str, issue: &str) -> crate::SemanticError {
    let result = test_support::analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected rejection {issue:?}, got: {:?}",
        result.diagnostics
    );
    let err = find_error(&result, issue)
        .unwrap_or_else(|| panic!("missing {issue:?} in {:?}", result.diagnostics));
    assert_eq!(
        crate::semantic_spec(err.kind).code,
        "SEM010",
        "issue {issue:?}"
    );
    err
}

fn assert_accepts(source: &str) {
    test_support::assert_accepts(source);
}

// ---------------------------------------------------------------------------
// Admission: sequential form in host and device context; filum in a kernel
// ---------------------------------------------------------------------------

#[test]
fn sequential_form_admits_in_host_function() {
    assert_accepts(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s { redde s }
}",
    );
}

#[test]
fn sequential_form_admits_in_nucleum_kernel() {
    assert_accepts(
        r"
@ nucleum
functio k(tensor<f32, [256]> a) → f32 {
    redde summa ex a apud [i] fixum s { redde s }
}",
    );
}

#[test]
fn sequential_form_without_apud_admits_rank1() {
    assert_accepts(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a fixum s { redde s }
}",
    );
}

#[test]
fn filum_form_admits_inside_nucleum_kernel() {
    assert_accepts(
        r"
@ nucleum
functio k(tensor<f32, [256]> a) → f32 {
    redde summa ex a apud [i] filum f fixum s { redde s }
}",
    );
}

#[test]
fn filum_form_admits_matrix_source_with_aligned_count() {
    // matrix<f32, [4, 8]>: K = 32, a full lane — admitted.
    assert_accepts(
        r"
@ nucleum
functio k(matrix<f32, [4, 8]> m) → f32 {
    redde summa ex m apud [i, j] filum f fixum s { redde s }
}",
    );
}

#[test]
fn sequential_form_admits_host_with_partial_lane_count() {
    // No filum means no lane schedule; K = 100 is not a tail here.
    assert_accepts(
        r"
functio total(tensor<f32, [100]> a) → f32 {
    redde summa ex a apud [i] fixum s { redde s }
}",
    );
}

#[test]
fn term_body_may_mutate_its_own_locals() {
    assert_accepts(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s {
        varia f32 t ← s
        t ← t + 1.0
        redde t
    }
}",
    );
}

// ---------------------------------------------------------------------------
// Placement: filum requires an enclosing @ nucleum kernel
// ---------------------------------------------------------------------------

#[test]
fn filum_form_outside_kernel_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [32]> a) → f32 {
    redde summa ex a apud [i] filum f fixum s { redde s }
}",
        "summa_filum_requires_nucleum_kernel",
    );
}

#[test]
fn filum_form_in_fragment_declines() {
    assert_rejects_with_issue(
        r"
@ nucleum fragment
functio helper(tensor<f32, [256]> a) → f32 {
    redde summa ex a apud [i] filum f fixum s { redde s }
}",
        "summa_filum_requires_nucleum_kernel",
    );
}

// ---------------------------------------------------------------------------
// Shared apud arity rule
// ---------------------------------------------------------------------------

#[test]
fn rank2_single_coordinate_declines_with_shared_arity_rule() {
    let err = assert_rejects_with_issue(
        r"
functio total(tensor<f32, [4, 4]> m) → f32 {
    redde summa ex m apud [i] fixum s { redde s }
}",
        "itera_apud_arity_mismatch",
    );
    assert_code_and_arg(&err, "SEM010", "expected", "2");
    assert_code_and_arg(&err, "SEM010", "actual", "1");
}

#[test]
fn rank2_without_apud_declines_with_shared_arity_rule() {
    let err = assert_rejects_with_issue(
        r"
functio total(tensor<f32, [4, 4]> m) → f32 {
    redde summa ex m fixum s { redde s }
}",
        "itera_apud_arity_mismatch",
    );
    assert_code_and_arg(&err, "SEM010", "expected", "2");
    assert_code_and_arg(&err, "SEM010", "actual", "0");
}

#[test]
fn rank2_three_coordinates_declines_with_shared_arity_rule() {
    let err = assert_rejects_with_issue(
        r"
functio total(tensor<f32, [4, 4]> m) → f32 {
    redde summa ex m apud [i, j, k] fixum s { redde s }
}",
        "itera_apud_arity_mismatch",
    );
    assert_code_and_arg(&err, "SEM010", "expected", "2");
    assert_code_and_arg(&err, "SEM010", "actual", "3");
}

// ---------------------------------------------------------------------------
// Iterable surface and term type
// ---------------------------------------------------------------------------

#[test]
fn non_tensor_iterable_declines() {
    assert_rejects_with_issue(
        r"
functio total(lista<numerus> xs) → numerus {
    redde summa ex xs fixum s { redde s }
}",
        "summa_requires_shaped_iterable",
    );
}

#[test]
fn non_tensor_iterable_with_apud_declines() {
    assert_rejects_with_issue(
        r"
functio total(lista<numerus> xs) → numerus {
    redde summa ex xs apud [i] fixum s { redde s }
}",
        "summa_requires_shaped_iterable",
    );
}

#[test]
fn non_scalar_term_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<textus, [8]> t) → textus {
    redde summa ex t apud [i] fixum s { redde s }
}",
        "summa_term_type_non_scalar",
    );
}

// ---------------------------------------------------------------------------
// Body law: exactly one valued redde
// ---------------------------------------------------------------------------

#[test]
fn body_without_redde_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s { tacet }
}",
        "summa_body_requires_single_redde",
    );
}

#[test]
fn body_with_two_redde_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s {
        redde s
        redde s
    }
}",
        "summa_body_requires_single_redde",
    );
}

#[test]
fn body_with_bare_redde_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s { redde }
}",
        "summa_body_requires_single_redde",
    );
}

// ---------------------------------------------------------------------------
// Body law: no rumpe / cede / cape / iace
// ---------------------------------------------------------------------------

#[test]
fn body_with_rumpe_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s {
        fac {
            redde s
            rumpe
        } dum verum
    }
}",
        "summa_body_forbids_rumpe",
    );
}

#[test]
fn body_with_cede_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s {
        cede s
        redde s
    }
}",
        "summa_body_forbids_cede",
    );
}

#[test]
fn body_with_cape_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s {
        fac { redde s } cape err { redde 0 }
    }
}",
        "summa_body_forbids_cape",
    );
}

#[test]
fn body_with_iace_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s {
        iace 0
        redde s
    }
}",
        "summa_body_forbids_iace",
    );
}
// ---------------------------------------------------------------------------
// Body law: fragment-compatible pure effect subset
// ---------------------------------------------------------------------------

#[test]
fn body_may_call_nucleum_fragment() {
    assert_accepts(
        r"
@ nucleum fragment
functio duplum(f32 x) → f32 {
    redde x + x
}

@ nucleum
functio k(tensor<f32, [256]> a) → f32 {
    redde summa ex a apud [i] filum f fixum s { redde duplum(s) }
}",
    );
}

#[test]
fn body_calling_host_function_declines() {
    assert_rejects_with_issue(
        r"
functio plus_one(f32 x) → f32 {
    redde x + 1
}

functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s { redde plus_one(s) }
}",
        "summa_body_context",
    );
}

#[test]
fn body_host_io_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s {
        nota s
        redde s
    }
}",
        "summa_body_effect",
    );
}

#[test]
fn body_captured_mutation_declines() {
    assert_rejects_with_issue(
        r"
functio total(tensor<f32, [8]> a) → f32 {
    varia f32 acc ← 0
    redde summa ex a apud [i] fixum s {
        acc ← s
        redde s
    }
}",
        "summa_body_effect",
    );
}

// ---------------------------------------------------------------------------
// Tail gate: K % W ≠ 0 fails closed at admission (filum form only)
// ---------------------------------------------------------------------------

#[test]
fn filum_form_with_partial_lane_count_declines() {
    assert_rejects_with_issue(
        r"
@ nucleum
functio k(tensor<f32, [100]> a) → f32 {
    redde summa ex a apud [i] filum f fixum s { redde s }
}",
        "summa_tail_not_aligned",
    );
}
