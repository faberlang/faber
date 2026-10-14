//! Regression coverage for the `expande([...], axis n)` probe validation.
//!
//! Need: the broadcast-back axis marker is the mirror of an axis
//! reduction. Before the rewrite lands, an invalid call must be refused with
//! the named diagnostic for its violation.

fn accepts(source: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected typecheck failure, but source was accepted"
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.issue() == Some(issue)),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

#[test]
fn expanded_axis_out_of_range_is_refused() {
    rejects_with_issue(
        r"incipit {
  fixum tf32[2, 3] s ← vacua
  fixum _ r ← s.expande([2, 4, 3], axis 5)
  nota r
}",
        "expanded_axis_out_of_range",
    );
}

#[test]
fn expanded_axis_non_constant_is_refused() {
    rejects_with_issue(
        r"incipit {
  fixum tf32[2, 3] s ← vacua
  varia numerus k ← 1
  fixum _ r ← s.expande([2, 4, 3], axis k)
  nota r
}",
        "expanded_axis_not_constant",
    );
}

#[test]
fn expanded_axis_rank_mismatch_is_refused() {
    rejects_with_issue(
        r"incipit {
  fixum tf32[2, 3] s ← vacua
  fixum _ r ← s.expande([2, 3], axis 0)
  nota r
}",
        "expanded_axis_rank_mismatch",
    );
}

#[test]
fn bare_expande_without_axis_stays_the_ordinary_intrinsic() {
    accepts(
        r"incipit {
  fixum tf32[2, 3] s ← vacua
  fixum _ r ← s.expande([2, 4, 3])
  nota r
}",
    );
}
