//! K8-I behavioral admission oracle, through parse and the semantic pipeline.
//!
//! Latin fixtures use the test parser's explicit Latin surface. Rejections
//! must be independence diagnostics, never incidental parse/type errors.

use crate::test_support::{analyze_source_for_target_la, analyze_source_la};
use radix_target::Target;

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
            .any(|arg| arg.name == "reason" && arg.value.clone() == reason)),
        "missing reason {reason}: {errors:?}"
    );
}

fn body(statements: &str) -> String {
    format!(
        "incipit {{\n varia tensor<numerus, [4]> grid ← vacua\n itera ex grid apud [i] filum lane sit value {{\n {statements}\n }}\n }}"
    )
}

#[test]
fn own_coordinate_write_accepted() {
    let source = body("grid[i] ← value + 1");
    // The proof is not hidden behind the Rust borrow gate or a device context.
    for target in Target::ALL {
        let result = analyze_source_for_target_la(&source, *target);
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
functio poke(varia tensor<numerus, [4]> target) → vacuum { target[0] ← 9 }
incipit {
    varia tensor<numerus, [4]> grid ← vacua
    itera ex grid apud [i] filum lane sit value {
        grid[i] ← value
        poke(varia grid)
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
    itera ex grid apud [i] filum lane sit value {
        grid[i] ← value
        nota counter.accipe(i)
    }
}",
        "unproved_call_effect",
    );
}

#[test]
fn snapshot_of_a_later_mutated_source_is_not_its_live_alias() {
    // An immutable local keeps the value its initializer read at declaration:
    // `i1` is 0 forever and `i2` is 2 forever, whatever `m` is later. The
    // proof must not reason about either snapshot as the live `m`, or the
    // write through `i1` and the read through `i2` share one address.
    rejected(
        r"incipit {
    varia tensor<numerus, [3]> row ← vacua
    varia tensor<numerus, [3, 3]> cache ← vacua
    varia numerus m ← 0
    sit numerus i1 ← m
    m ← 2
    sit numerus i2 ← m
    itera ex row apud [j] filum lane sit value {
        cache[[i1, j]] ← value
        sit numerus x ← cache.accipe([i2, j])
    }
}",
        "written_storage_neighbor_read",
    );
    // A snapshot taken inside the lane keeps the same rule: the write through
    // it is not a write at the live mutable's address.
    rejected(
        r"incipit {
    varia tensor<numerus, [3]> row ← vacua
    varia tensor<numerus, [3, 3]> cache ← vacua
    varia numerus m ← 0
    m ← 2
    itera ex row apud [j] filum lane sit value {
        sit numerus i ← m
        cache[[i, j]] ← value
    }
}",
        "write_not_own_coordinate",
    );
    // The live mutable itself still names one lane-invariant component, and
    // a snapshot of a value-stable source keeps resolving through it.
    accepted(
        r"incipit {
    varia tensor<numerus, [3]> row ← vacua
    varia tensor<numerus, [3, 3]> cache ← vacua
    varia numerus m ← 2
    itera ex row apud [j] filum lane sit value {
        cache[[m, j]] ← value
        sit numerus jj ← j
        sit numerus x ← cache.accipe([m, jj])
    }
}",
    );
}

#[test]
fn initializer_expansion_work_is_memoized_and_capped() {
    // A chain that names its predecessor twice is exponential when each
    // local is re-expanded (`a₁ ← a₀ + a₀`, …): the assert is on visited
    // nodes, not on a timing. Memoized resolution walks each initializer
    // once, so the whole program stays far below the bound.
    let mut chain = String::from("sit i32 a0 ← k");
    for index in 1..24 {
        chain.push_str(&format!(
            "\n sit i32 a{index} ← a{} + a{}",
            index - 1,
            index - 1
        ));
    }
    let source = format!(
        "incipit {{\n varia tensor<i32, [3]> lanes ← vacua\n varia tensor<i32, [64]> scratch ← vacua\n itera ex lanes apud [k] filum lane sit value {{\n {chain}\n scratch[[a23]] ← value\n }}\n }}"
    );
    super::filum_independence::work::take();
    let result = analyze_source_la(&source);
    let nodes = super::filum_independence::work::take();
    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        nodes < 10_000,
        "initializer expansion visited {nodes} nodes; the memo or the cap regressed"
    );
}

#[test]
fn same_coordinate_read_write_accepted() {
    accepted(&body("grid[i] ← grid[i] + value"));
    accepted(&body("grid.ponde([i], grid.accipe([i]) + value)"));
}

#[test]
fn pure_value_method_call_is_not_an_access() {
    // A registered value method on a scalar receiver reads no storage: the
    // call must prove, not decline as an unproved effect.
    accepted(&body(
        "sit numerus magnitude ← value.absolutum()\n grid[i] ← value + magnitude",
    ));
}

#[test]
fn neighbor_read_of_written_tensor_rejected() {
    rejected(
        &body("sit numerus prior ← grid[i + 1]\n grid[i] ← prior"),
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
    itera ex grid apud [i] filum lane sit value {
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
    itera ex grid apud [i] filum lane sit value {
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
    itera ex grid apud [i] filum lane sit value {
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
    itera ex grid apud [i, j] filum lane sit value {
        sit numerus neighbor ← grid.transpone().accipe([i, j])
        grid.ponde([i, j], neighbor)
    }
}",
        "written_storage_neighbor_read",
    );
    rejected(
        r"incipit {
    varia tensor<numerus, [4]> grid ← vacua
    itera ex grid apud [i] filum lane sit value {
        sit numerus neighbor ← grid.sectio(1, 4).accipe([i])
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
    varia tensor<numerus, [4]> independent ← copy grid
    itera ex grid apud [i] filum lane sit value {
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
    sit tensor<numerus, [4]> lanes ← [1, 2, 3, 4] ↦ tensor<numerus, [4]>
    varia tensor<numerus, [4]> grid ← vacua
    itera ex lanes apud [i] filum lane sit value {
        grid[i] ← value
    }
}",
    );
}

#[test]
fn float_literal_conversio_initializer_proves_fresh_storage() {
    // An untyped float literal adopts the tensor element at the conversion
    // (`[1.0, 2.0] ↦ tf32[2]`): the list's own element type is not `f32`, but
    // the lowering still builds a fresh tensor, so the fact stands.
    accepted(
        r"incipit {
    sit tf32[2] lanes ← [1.0, 2.0] ↦ tf32[2]
    varia tf32[2] grid ← tf32[2] {}
    itera ex lanes apud [i] filum lane sit value {
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
    itera ex grid apud [i] filum lane sit value {
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
    itera ex lanes apud [i] filum lane sit value {
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
    itera ex grid apud [i] filum lane sit value {
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
    itera ex grid apud [i] sit value {
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
