//! NR-2 (nary-range-walk): driver-level proofs for the Cartesian
//! `itera ab` range product.
//!
//! The semantic laws are proven in the `radix-semantic` leaf; these probes
//! run the same statements through the full module driver so the product is
//! checked exactly as user packages meet it — parse, lower, typecheck — and
//! the Rust target stays fail-closed on the product until NR-4 lands.

use super::test_support::{assert_analysis_accepts, assert_analysis_rejects_with_issue, compile_and_get_diagnostics};

#[test]
fn n2_range_product_typechecks_through_the_driver() {
    assert_analysis_accepts(
        r"
functio grid(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n fixum i, j {
        nota i
    }
}
",
    );
}

#[test]
fn n3_range_product_typechecks_through_the_driver() {
    assert_analysis_accepts(
        r"
functio cube(numerus m, numerus n, numerus k) → vacuum {
    itera ab 0‥m, 0‥n, 0‥k fixum i, j, p {
        nota p
    }
}
",
    );
}

#[test]
fn n1_range_keeps_the_shared_itera_path_through_the_driver() {
    assert_analysis_accepts(
        r"
functio line(numerus m) → vacuum {
    itera ab 0‥m fixum i {
        nota i
    }
}
",
    );
}

#[test]
fn driver_rejects_range_product_count_mismatch() {
    assert_analysis_rejects_with_issue(
        r"
functio short(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n fixum i {
        nota i
    }
}
",
        "itera_range_source_binder_arity_mismatch",
    );
}

#[test]
fn driver_rejects_range_product_extent_binder_shadowing() {
    assert_analysis_rejects_with_issue(
        r"
functio shadow(numerus m, numerus k) → vacuum {
    itera ab 0‥m, 0‥k fixum i, k {
        nota i
    }
}
",
        "itera_range_extent_binder_shadowing",
    );
}

#[test]
fn driver_rejects_range_product_mixed_member() {
    assert_analysis_rejects_with_issue(
        r"
functio mixed(numerus m, numerus n) → vacuum {
    itera ab 0‥m, n fixum i, j {
        nota i
    }
}
",
        "itera_range_mixed_source",
    );
}

#[test]
fn driver_rejects_range_product_apud_clause() {
    assert_analysis_rejects_with_issue(
        r"
functio coord(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n apud [c] fixum i, j {
        nota i
    }
}
",
        "itera_apud_requires_ex",
    );
}

#[test]
fn driver_keeps_lockstep_laws_off_the_range_product_route() {
    // The zip form (`ex`) keeps its own identity on the same mismatch shape:
    // the product never reaches the for-from lockstep checker.
    assert_analysis_rejects_with_issue(
        r"
functio zip(lista<numerus> xs, lista<numerus> ys) → vacuum {
    itera ex xs, ys fixum x {
        nota x
    }
}
",
        "itera_lockstep_source_binder_arity_mismatch",
    );
}

#[test]
fn rust_target_stays_fail_closed_on_the_range_product() {
    // NR-2 transitional red window: typecheck admits the product, the Rust
    // target declines it by name until the ordered nested-loop emission
    // lands with NR-4. No guessed product code may be emitted meanwhile.
    let diagnostics = compile_and_get_diagnostics(
        r"
functio grid(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n fixum i, j {
        nota i
    }
}
",
    );
    assert!(
        !diagnostics.is_empty(),
        "Rust target must decline the range product until NR-4 lands"
    );
    assert!(
        diagnostics.iter().any(|diag| diag.is_error()),
        "expected a hard decline, got: {diagnostics:?}"
    );
}

#[test]
fn analysis_diagnostics_render_catalog_prose_for_product_laws() {
    // The three dedicated SEM010 rows must render through the catalog, not
    // leak bare issue strings.
    use crate::semantic::SemanticErrorKind;
    use radix_diagnostics::DiagnosticArg;
    use radix_semantic::catalog_semantic::semantic_message;

    let arity = [
        DiagnosticArg::new("issue", "itera_range_source_binder_arity_mismatch"),
        DiagnosticArg::new("expected", "2"),
        DiagnosticArg::new("actual", "1"),
    ];
    let rendered = semantic_message(SemanticErrorKind::TypeMismatch, &arity).expect("arity law renders catalog prose");
    assert_eq!(
        rendered,
        "for-range binds one binder per range: expected 2 binders, found 1; the counts must match exactly"
    );

    for issue in ["itera_range_extent_binder_shadowing", "itera_range_mixed_source"] {
        let args = [DiagnosticArg::new("issue", issue)];
        let rendered = semantic_message(SemanticErrorKind::TypeMismatch, &args)
            .unwrap_or_else(|| panic!("issue {issue} must render catalog prose"));
        assert!(
            !rendered.is_empty() && rendered != issue,
            "issue {issue} must render catalog prose, got: {rendered:?}"
        );
    }
}
