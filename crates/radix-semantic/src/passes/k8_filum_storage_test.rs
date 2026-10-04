//! K8 storage identity of tensors built by the factory intrinsics.
//!
//! `crea` and `strue` allocate new storage, so a tensor they build is its own
//! storage root. Any other call result keeps the named unknown-identity
//! decline.

use crate::test_support::analyze_source;

const ISSUE: &str = "filum_independence_unproven";

fn accepted(source: &str) {
    let result = analyze_source(source);
    assert!(result.success(), "{:?}", result.diagnostics);
}

fn rejected(source: &str, reason: &str) {
    let result = analyze_source(source);
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

fn kernel(grid: &str, statement: &str) -> String {
    format!(
        r"incipit {{
    varia tensor<numerus, []> seed ← vacua
    varia tensor<numerus, [4]> grid ← {grid}
    itera ex grid apud [i] filum lane fixum value {{
        {statement}
    }}
}}"
    )
}

#[test]
fn created_tensor_is_fresh_storage() {
    accepted(&kernel("seed.crea(0, [4])", "grid[i] ← value + 1"));
    rejected(
        &kernel("seed.crea(0, [4])", "grid[i] ← grid[i + 1]"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn flat_built_tensor_is_fresh_storage() {
    accepted(&kernel(
        "seed.strue([1, 2, 3, 4], [4])",
        "grid[i] ← value + 1",
    ));
    rejected(
        &kernel("seed.strue([1, 2, 3, 4], [4])", "grid[i] ← grid[i + 1]"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn other_call_result_keeps_unknown_identity() {
    rejected(
        &kernel(
            "seed.crea(0, [4]).addita(seed.crea(1, [4]))",
            "grid[i] ← value",
        ),
        "unknown_storage_identity",
    );
}
