//! K8-R nested composition and reduction exemption, through parse and the
//! semantic pipeline.
//!
//! Latin fixtures use the test parser's explicit Latin surface. Rejections
//! must be independence diagnostics, never incidental parse/type errors.

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
            .any(|arg| arg.name == "reason" && arg.value.to_string() == reason)),
        "missing reason {reason}: {errors:?}"
    );
}

/// A rank-2 grid written by an outer row loop and an inner column loop.
fn nest(inner: &str) -> String {
    format!(
        r"incipit {{
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    varia tensor<numerus, [2, 3]> grid ← vacua
    itera ex rows apud [i] filum outer sit row {{
        itera ex cols apud [j] filum inner sit col {{
            {inner}
        }}
    }}
}}"
    )
}

#[test]
fn legal_nested_filum_accepted() {
    accepted(&nest("grid[[i, j]] ← row + col"));
    accepted(&nest("grid.ponde([i, j], row + col)"));
    // The outer iteration may also write its own coordinate outside the inner
    // loop, and read what it wrote there.
    accepted(
        r"incipit {
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    varia tensor<numerus, [2]> sums ← vacua
    varia tensor<numerus, [2, 3]> grid ← vacua
    itera ex rows apud [i] filum outer sit row {
        sums[i] ← row
        itera ex cols apud [j] filum inner sit col {
            grid[[i, j]] ← sums[i] + col
        }
    }
}",
    );
}

#[test]
fn nested_neighbor_write_rejected() {
    // An inner iteration cannot write an outer-loop neighbor.
    rejected(
        &nest("grid[[i + 1, j]] ← row + col"),
        "write_not_own_coordinate",
    );
    // Nor a cell every inner iteration shares.
    rejected(
        &nest("grid[[i, 0]] ← row + col"),
        "write_not_own_coordinate",
    );
    // A write at only the outer coordinate repeats across inner iterations.
    rejected(
        r"incipit {
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    varia tensor<numerus, [2]> sums ← vacua
    itera ex rows apud [i] filum outer sit row {
        itera ex cols apud [j] filum inner sit col {
            sums[i] ← col
        }
    }
}",
        "write_not_own_coordinate",
    );
    // Reading a written neighbor through the nest is the same dependency.
    rejected(
        &nest("grid[[i, j]] ← grid[[i - 1, j]]"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn inner_loop_local_mutation_only_where_independent() {
    // A sequential inner loop may accumulate into its outer iteration's local.
    accepted(
        r"incipit {
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    itera ex rows apud [i] filum outer sit row {
        varia numerus total ← row
        itera ex cols apud [j] sit col {
            total ← total + col
        }
        rows[i] ← total
    }
}",
    );
    // An inner filum iteration owns only the locals its own body declares.
    accepted(&nest(
        "varia numerus local ← col\n local ← local + row\n grid[[i, j]] ← local",
    ));
    // Updating the outer iteration's local from an inner filum is loop-carried.
    rejected(
        r"incipit {
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    itera ex rows apud [i] filum outer sit row {
        varia numerus total ← row
        itera ex cols apud [j] filum inner sit col {
            total ← total + col
        }
        rows[i] ← total
    }
}",
        "outer_local_mutation",
    );
}

#[test]
fn k3_reduction_exempt_from_loop_carried_mutation() {
    // The fold's accumulator is internal to the reduction; only the store
    // into the iteration's own coordinate leaves it.
    accepted(
        r"incipit {
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    itera ex rows apud [i] filum outer sit row {
        rows[i] ← reducta via summa ex cols apud [j] sit s { redde s + row }
    }
}",
    );
    // The reduction's source is read at its own coordinates, so reducing over
    // storage the iteration writes is still a neighbor read.
    rejected(
        r"incipit {
    varia tensor<numerus, [3]> cols ← vacua
    itera ex cols apud [i] filum outer sit col {
        cols[i] ← reducta via summa ex cols apud [j]
    }
}",
        "written_storage_neighbor_read",
    );
    // The exemption is not a license to carry the result in an outer local.
    rejected(
        r"incipit {
    varia tensor<numerus, [2]> rows ← vacua
    varia tensor<numerus, [3]> cols ← vacua
    varia numerus total ← 0
    itera ex rows apud [i] filum outer sit row {
        total ← reducta via summa ex cols apud [j]
    }
}",
        "outer_local_mutation",
    );
}
