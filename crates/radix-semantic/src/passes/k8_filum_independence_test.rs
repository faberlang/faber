//! K8-I behavioral admission oracle, through parse and the semantic pipeline.
//!
//! Latin fixtures use the test parser's explicit Latin surface. Rejections
//! must be independence diagnostics, never incidental parse/type errors.

use crate::test_support::{analyze_source, analyze_source_for_target};
use radix_target::Target;

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

fn body(statements: &str) -> String {
    format!(
        "incipit {{\n varia tensor<numerus, [4]> grid ← vacua\n itera ex grid apud [i] filum lane fixum value {{\n {statements}\n }}\n }}"
    )
}

#[test]
fn own_coordinate_write_accepted() {
    let source = body("grid[i] ← value + 1");
    // The proof is not hidden behind the Rust borrow gate or a device context.
    for target in Target::ALL {
        let result = analyze_source_for_target(&source, *target);
        assert!(result.success(), "{target:?}: {:?}", result.diagnostics);
    }
    accepted(&source.replace("incipit", "functio host() → vacuum"));
    accepted(&body("grid.ponde([i], value + 1)"));
}

#[test]
fn non_own_coordinate_write_rejected() {
    rejected(&body("grid[0] ← value"), "write_not_own_coordinate");
    rejected(&body("grid.ponde([0], value)"), "write_not_own_coordinate");
}

#[test]
fn hidden_mutating_call_write_rejected() {
    rejected(
        r"
functio poke(in tensor<numerus, [4]> target) → vacuum { target[0] ← 9 }
incipit {
    varia tensor<numerus, [4]> grid ← vacua
    itera ex grid apud [i] filum lane fixum value {
        grid[i] ← value
        poke(in grid)
    }
}",
        "unproved_call_effect",
    );
    // A user method with a built-in-looking name is not an intrinsic proof.
    rejected(
        r"
genus Counter {
    varia numerus count
    functio accipe(numerus index) → numerus { ego.count ← ego.count + 1
        redde ego.count }
}
incipit {
    varia tensor<numerus, [4]> grid ← vacua
    varia Counter counter ← Counter { count = 0 }
    itera ex grid apud [i] filum lane fixum value {
        grid[i] ← value
        nota counter.accipe(i)
    }
}",
        "unproved_call_effect",
    );
}

#[test]
fn same_coordinate_read_write_accepted() {
    accepted(&body("grid[i] ← grid[i] + value"));
    accepted(&body("grid.ponde([i], grid.accipe([i]) + value)"));
}

#[test]
fn neighbor_read_of_written_tensor_rejected() {
    rejected(
        &body("fixum numerus prior ← grid[i + 1]\n grid[i] ← prior"),
        "written_storage_neighbor_read",
    );
    rejected(
        &body("grid[i] ← value\n nota grid[i + 1]"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn direct_alias_neighbor_rejected() {
    rejected(
        r"incipit {
    varia tensor<numerus, [4]> grid ← vacua
    varia tensor<numerus, [4]> alias ← grid
    itera ex grid apud [i] filum lane fixum value {
        grid[i] ← alias[i + 1]
    }
}",
        "written_storage_neighbor_read",
    );
    // A later rebind cannot erase a possible storage root, including branches.
    rejected(
        r"incipit {
    varia tensor<numerus, [4]> grid ← vacua
    varia tensor<numerus, [4]> other ← vacua
    varia tensor<numerus, [4]> alias ← other
    si verum { alias ← grid }
    itera ex grid apud [i] filum lane fixum value {
        grid[i] ← alias[i + 1]
    }
}",
        "written_storage_neighbor_read",
    );
}

#[test]
fn field_alias_neighbor_rejected() {
    rejected(
        r"
genus Holder { varia tensor<numerus, [4]> data }
incipit {
    varia tensor<numerus, [4]> grid ← vacua
    varia Holder holder ← Holder { data = grid }
    varia tensor<numerus, [4]> alias ← holder.data
    itera ex grid apud [i] filum lane fixum value {
        grid[i] ← alias[i + 1]
    }
}",
        "written_storage_neighbor_read",
    );
}

#[test]
fn transformed_view_neighbor_rejected() {
    rejected(
        r"incipit {
    varia tensor<numerus, [2, 2]> grid ← vacua
    itera ex grid apud [i, j] filum lane fixum value {
        fixum numerus neighbor ← grid.transpone().accipe([i, j])
        grid.ponde([i, j], neighbor)
    }
}",
        "written_storage_neighbor_read",
    );
    rejected(
        r"incipit {
    varia tensor<numerus, [4]> grid ← vacua
    itera ex grid apud [i] filum lane fixum value {
        fixum numerus neighbor ← grid.sectio(1, 4).accipe([i])
        grid[i] ← neighbor
    }
}",
        "written_storage_neighbor_read",
    );
}

#[test]
fn explicit_copied_storage_accepted() {
    accepted(
        r"incipit {
    varia tensor<numerus, [4]> grid ← vacua
    varia copy tensor<numerus, [4]> independent ← grid
    itera ex grid apud [i] filum lane fixum value {
        grid[i] ← independent[0]
    }
}",
    );
}

#[test]
fn tensor_conversio_initializer_proves_fresh_storage() {
    // `flat ↦ tensor` lowers through `TensorFromFlat` on a fresh aggregate,
    // so the conversion initializer proves like the factory methods do.
    accepted(
        r"incipit {
    fixum tensor<numerus, [4]> lanes ← [1, 2, 3, 4] ↦ tensor<numerus, [4]>
    varia tensor<numerus, [4]> grid ← vacua
    itera ex lanes apud [i] filum lane fixum value {
        grid[i] ← value
    }
}",
    );
}

#[test]
fn tensor_constructor_initializer_proves_fresh_storage() {
    // The `tf32[4]` constructor lowers to the synthesized `crea` call, so it
    // proves without the seed-method costume.
    accepted(
        r"incipit {
    varia tf32[4] grid ← tf32[4] {}
    itera ex grid apud [i] filum lane fixum value {
        grid[i] ← value
    }
}",
    );
}

#[test]
fn tensor_source_conversio_keeps_unknown_decline() {
    // A tensor source shares its buffer across the conversion, so the
    // result is not fresh and the unknown-identity decline stands.
    rejected(
        r"incipit {
    varia tensor<numerus, [4]> lanes ← vacua
    varia tensor<numerus, [4]> copy ← lanes ↦ tensor<numerus, [4]>
    itera ex lanes apud [i] filum lane fixum value {
        copy[i] ← value
    }
}",
        "unknown_storage_identity",
    );
}

#[test]
fn outer_local_update_rejected() {
    rejected(
        r"incipit {
    varia tensor<numerus, [4]> grid ← vacua
    varia numerus total ← 0
    itera ex grid apud [i] filum lane fixum value {
        total ← total + value
    }
}",
        "outer_local_mutation",
    );
}

#[test]
fn iteration_local_update_accepted() {
    accepted(&body(
        "varia numerus local ← value\n local ← local + 1\n local ↑\n grid[i] ← local",
    ));
}

#[test]
fn ordinary_non_filum_loop_unchanged() {
    accepted(
        r"incipit {
    varia tensor<numerus, [4]> grid ← vacua
    varia numerus total ← 0
    itera ex grid apud [i] fixum value {
        total ← total + grid[i + 1]
        grid[0] ← total
    }
}",
    );
}

#[test]
fn failable_accipe_with_inline_default_accepted() {
    // The `⊥` default makes the failure local: nothing escapes the lane.
    accepted(&body("grid[i] ← grid.accipe([i]) ⊥ 0"));
    accepted(&body("grid[i] ← grid.accipe([i]) ⊥ value"));
}

#[test]
fn failable_accipe_with_default_keeps_neighbor_rule() {
    // The default does not excuse a neighbor read of a written tensor.
    rejected(
        &body("grid[i] ← grid.accipe([i + 1]) ⊥ 0"),
        "written_storage_neighbor_read",
    );
}
