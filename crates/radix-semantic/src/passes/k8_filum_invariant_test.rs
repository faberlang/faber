//! K8-R lane-invariant index components.
//!
//! A write whose index is the lanes' own coordinates (each once, unchanged)
//! plus components every lane shares (a constant, or a def declared outside
//! the loop) writes distinct cells per lane. Latin fixtures use the test
//! parser's explicit Latin surface.

use crate::test_support::analyze_source_la;

const ISSUE: &str = "filum_independence_unproven";

fn accepted(source: &str) {
    let result = analyze_source_la(source);
    assert!(result.success(), "{:?}", result.diagnostics);
}

fn rejected(source: &str, reason: &str) {
    let result = analyze_source_la(source);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(!errors.is_empty(), "body unexpectedly certified");
    assert!(
        errors.iter().all(|d| d.issue() == Some(ISSUE)),
        "not independence errors: {errors:?}"
    );
    assert!(
        errors.iter().any(|d| d
            .args
            .iter()
            .any(|arg| arg.name == "reason" && arg.value == reason)),
        "missing reason {reason}: {errors:?}"
    );
}

/// A matrix row written by one lane loop; `statement` is the lane body.
fn row(statement: &str) -> String {
    format!(
        r"incipit {{
    varia tensor<numerus, [3]> row ← vacua
    varia tensor<numerus, [2, 3]> cache ← vacua
    sit numerus pos ← 1
    itera ex row apud [j] filum lane sit value {{
        {statement}
    }}
}}"
    )
}

/// A cube plane written by a nest of two lane loops.
fn plane(statement: &str) -> String {
    format!(
        r"incipit {{
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    varia tensor<numerus, [2, 2, 3]> cube ← vacua
    sit numerus pos ← 1
    itera ex rows apud [i] filum outer sit r {{
        itera ex cols apud [j] filum inner sit c {{
            {statement}
        }}
    }}
}}"
    )
}

#[test]
fn flattened_lane_row_needs_a_proven_inner_range() {
    accepted(
        r"incipit {
    varia tensor<i32, [3]> lanes ← vacua
    varia tensor<i32, [906]> scratch ← vacua
    itera ex lanes apud [k] filum lane sit value {
        sit i32 base ← k * 301
        sit i32 lo ← 1
        sit i32 hi ← 301
        itera ab lo‥hi sit j {
            scratch[[base + j]] ← value
        }
    }
}",
    );
}

#[test]
fn flat_lane_offset_without_a_covering_stride_is_refused() {
    rejected(
        r"incipit {
    varia tensor<i32, [3]> lanes ← vacua
    varia tensor<i32, [12]> scratch ← vacua
    itera ex lanes apud [k] filum lane sit value {
        sit i32 hi ← 4
        itera ab 0‥hi sit j {
            scratch[[k + j]] ← value
        }
    }
}",
        "write_not_own_coordinate",
    );
    rejected(
        r"incipit {
    varia tensor<i32, [3]> lanes ← vacua
    varia tensor<i32, [12]> scratch ← vacua
    itera ex lanes apud [k] filum lane sit value {
        sit i32 pitch ← 2
        sit i32 base ← k * pitch
        sit i32 hi ← 4
        itera ab 0‥hi sit j {
            scratch[[base + j]] ← value
        }
    }
}",
        "write_not_own_coordinate",
    );
}

#[test]
fn flat_lane_row_may_not_cross_its_stride_boundary() {
    rejected(
        r"incipit {
    varia tensor<i32, [3]> lanes ← vacua
    varia tensor<i32, [12]> scratch ← vacua
    itera ex lanes apud [k] filum lane sit value {
        sit i32 pitch ← 4
        sit i32 base ← k * pitch
        sit i32 hi ← 5
        itera ab 0‥hi sit j {
            scratch[[base + j]] ← value
        }
    }
}",
        "write_not_own_coordinate",
    );
}

#[test]
fn invariant_components_beside_lane_coordinates_accepted() {
    accepted(&row("cache[[pos, j]] ← value"));
    accepted(&row("cache[[1, j]] ← value"));
    accepted(&row("cache.ponde([pos, j], value)"));
    accepted(&plane("cube[[pos, i, j]] ← r + c"));
    accepted(&plane("cube[[i, pos, j]] ← r + c"));
    accepted(&plane("cube[[i, j, 0]] ← r + c"));
}

#[test]
fn transposed_column_write_accepted() {
    accepted(
        r"incipit {
    varia tensor<numerus, [2]> col ← vacua
    varia tensor<numerus, [2, 3]> cache ← vacua
    sit numerus pos ← 1
    itera ex col apud [j] filum lane sit value {
        cache[[j, pos]] ← value
    }
}",
    );
}

#[test]
fn lane_coordinate_must_appear_once_unchanged() {
    rejected(&row("cache[[j, j]] ← value"), "write_not_own_coordinate");
    rejected(
        &row("cache[[pos, j + 1]] ← value"),
        "write_not_own_coordinate",
    );
    rejected(
        &row("cache[[pos, j * 2]] ← value"),
        "write_not_own_coordinate",
    );
}

#[test]
fn every_lane_coordinate_is_required() {
    rejected(&row("cache[[pos, 0]] ← value"), "write_not_own_coordinate");
    rejected(&row("cache[[0, 1]] ← value"), "write_not_own_coordinate");
    rejected(
        &plane("cube[[pos, i, 0]] ← r + c"),
        "write_not_own_coordinate",
    );
    rejected(
        &plane("cube[[pos, j, j]] ← r + c"),
        "write_not_own_coordinate",
    );
}

#[test]
fn lane_local_components_are_not_invariant() {
    // A local declared inside the lane body differs per lane.
    rejected(
        &row("sit numerus q ← value\n cache[[q, j]] ← value"),
        "write_not_own_coordinate",
    );
    // So does a value read from the lane's element.
    rejected(
        &row("cache[[value, j]] ← value"),
        "write_not_own_coordinate",
    );
    // The outer lane's local is not shared across the inner nest.
    rejected(
        r"incipit {
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    varia tensor<numerus, [2, 2, 3]> cube ← vacua
    itera ex rows apud [i] filum outer sit r {
        sit numerus q ← r
        itera ex cols apud [j] filum inner sit c {
            cube[[q, i, j]] ← c
        }
    }
}",
        "write_not_own_coordinate",
    );
}

#[test]
fn outer_local_still_cannot_be_written_in_the_body() {
    rejected(
        &row("pos ← pos + 1\n cache[[pos, j]] ← value")
            .replace("sit numerus pos", "varia numerus pos"),
        "outer_local_mutation",
    );
}

#[test]
fn invariant_write_keeps_the_neighbor_and_consistency_checks() {
    // Reading another cell of the written storage is still a neighbor read.
    rejected(
        &row("cache[[pos, j]] ← cache[[pos, j + 1]]"),
        "written_storage_neighbor_read",
    );
    rejected(
        &row("cache[[pos, j]] ← cache[[0, j]]"),
        "written_storage_neighbor_read",
    );
    // Reading the lane's own cell is fine.
    accepted(&row("cache[[pos, j]] ← cache[[pos, j]] + value"));
    // Two different invariant rows are two addresses of one write set.
    rejected(
        &row("cache[[pos, j]] ← value\n cache[[0, j]] ← value"),
        "inconsistent_written_coordinate",
    );
}
