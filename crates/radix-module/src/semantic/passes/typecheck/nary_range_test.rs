//! NR-2 (nary-range-walk): driver-level proofs for the Cartesian
//! `itera ab` range product.
//!
//! The semantic laws are proven in the `radix-semantic` leaf; these probes
//! run the same statements through the full module driver so the product is
//! checked exactly as user packages meet it — parse, lower, typecheck — and,
//! since NR-4 (e55457166), the Rust target emits the product.

use super::test_support::{assert_accepts, assert_analysis_accepts, assert_analysis_rejects_with_issue};

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

#[cfg(feature = "hir-rust")]
#[test]
fn rust_target_emits_the_range_product_through_the_driver() {
    // NR-4 settled contract: the Rust target emits the Cartesian product as
    // ordered nested range loops (extent/walk shape byte-pinned in the
    // `radix-hir-rust` leaf). Through the driver that is a clean compile —
    // codegen output present, no error diagnostics. The malformed-product
    // laws above keep failing closed at typecheck.
    assert_accepts(
        r"
functio grid(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n fixum i, j {
        nota i
    }
}
",
    );
}
