//! K8 storage identity of tensors built by the factory intrinsics.
//!
//! `crea` and `strue` allocate new storage, so a tensor they build is its own
//! storage root. Any other call result keeps the named unknown-identity
//! decline. Tensor parameters carry storage roots by mode (see the section
//! below).

use crate::test_support::{analyze_source_la, assert_rejects_with_issue_la};

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

fn kernel(grid: &str, statement: &str) -> String {
    format!(
        r"incipit {{
    varia tensor<numerus, [4]> grid ← {grid}
    itera ex grid apud [i] filum lane fixum value {{
        {statement}
    }}
}}"
    )
}

#[test]
fn created_tensor_is_fresh_storage() {
    accepted(&kernel("ti64[4] {}", "grid[i] ← value + 1"));
    rejected(
        &kernel("ti64[4] {}", "grid[i] ← grid[i + 1]"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn flat_built_tensor_is_fresh_storage() {
    accepted(&kernel(
        "[1, 2, 3, 4] ↦ tensor<numerus, [4]>",
        "grid[i] ← value + 1",
    ));
    rejected(
        &kernel(
            "[1, 2, 3, 4] ↦ tensor<numerus, [4]>",
            "grid[i] ← grid[i + 1]",
        ),
        "written_storage_neighbor_read",
    );
}

#[test]
fn other_call_result_keeps_unknown_identity() {
    // The factory constructors only sit in store position, so the unknown
    // root rides an `addita` result over a fresh local.
    rejected(
        &format!(
            r"incipit {{
    varia tensor<numerus, [4]> fresh ← ti64[4] {{}}
    varia tensor<numerus, [4]> grid ← fresh.addita(fresh)
    itera ex grid apud [i] filum lane fixum value {{
        grid[i] ← value
    }}
}}"
        ),
        "unknown_storage_identity",
    );
}

// =============================================================================
// Tensor parameters
//
// The call-site borrow check judges overlap on storage, so a `mut` (`in`)
// parameter is distinct from every other parameter; `ref` (`de`) and unmarked
// parameters may name one another's tensor and share one root class.
// =============================================================================

const GRID: &str = "tensor<numerus, [4]>";

/// `statement` runs once per lane of `src` inside a `filum` body; `params` are
/// the kernel's parameters.
fn kernel_fn(params: &str, statement: &str) -> String {
    format!(
        r"functio kernel({params}) → vacuum {{
    itera ex src apud [i] filum lane fixum value {{
        {statement}
    }}
}}
incipit {{
}}"
    )
}

fn with_src(extra: &str, statement: &str) -> String {
    kernel_fn(&format!("{extra}, de {GRID} src"), statement)
}

/// A kernel with a `varia` alias of its `out` parameter declared before the loop.
fn with_out_alias(statement: &str) -> String {
    with_src(&format!("in {GRID} out"), statement).replace(
        "    itera ex src",
        &format!("    varia {GRID} alias ← out\n    itera ex src"),
    )
}

#[test]
fn mut_param_written_at_own_coordinate_is_accepted() {
    accepted(&with_src(&format!("in {GRID} out"), "out[i] ← value + 1"));
    // The checked store is failable unless its index is proven in range (K11); a filum binder's
    // range proof lives in the K8 passes, not in typecheck, so the function declares `⇥`.
    accepted(
        &with_src(&format!("in {GRID} out"), "out.ponde([i], value + 1)")
            .replace("→ vacuum {", "→ vacuum ⇥ textus {"),
    );
}

#[test]
fn mut_param_written_while_read_param_is_read_at_a_neighbour_is_accepted() {
    accepted(&with_src(&format!("in {GRID} out"), "out[i] ← src[i + 1]"));
}

#[test]
fn two_read_params_at_neighbours_and_one_mut_written_is_accepted() {
    accepted(&kernel_fn(
        &format!("in {GRID} out, de {GRID} src, de {GRID} other"),
        "out[i] ← src[i + 1] + other[i + 2]",
    ));
}

#[test]
fn mut_param_written_then_read_at_a_neighbour_is_rejected() {
    rejected(
        &with_src(&format!("in {GRID} out"), "out[i] ← out[i + 1]"),
        "written_storage_neighbor_read",
    );
    rejected(
        &with_src(
            &format!("in {GRID} out"),
            "out[i] ← value\n        sit numerus next ← out[i + 1]",
        ),
        "written_storage_neighbor_read",
    );
}

#[test]
fn read_param_read_at_neighbours_and_never_written_is_accepted() {
    accepted(&kernel_fn(
        &format!("de {GRID} src"),
        "sit numerus next ← src[i + 1]",
    ));
    // Paired with a local that owns its storage.
    accepted(&format!(
        r"functio kernel(de {GRID} src) → vacuum {{
    varia {GRID} grid ← ti64[4] {{}}
    itera ex src apud [i] filum lane fixum value {{
        grid[i] ← src[i + 1]
    }}
}}
incipit {{
}}"
    ));
}

#[test]
fn two_mut_params_written_at_own_coordinates_are_accepted() {
    accepted(&with_src(
        &format!("in {GRID} left, in {GRID} right"),
        "left[i] ← value\n        right[i] ← value + 1",
    ));
}

#[test]
fn bind_alias_of_a_mut_param_joins_its_root() {
    rejected(
        &with_out_alias("alias[i] ← value\n        sit numerus next ← out[i + 1]"),
        "written_storage_neighbor_read",
    );
    accepted(&with_out_alias("alias[i] ← src[i + 1]"));
}

#[test]
fn unmarked_params_are_read_only_and_never_overlap_a_mut_param() {
    // Reading A: an unmarked param is shared read-only, so it can only be
    // read; the old "write through one unmarked param, read through another"
    // shape is gone. Reads of unmarked params at neighbours are fine, a `mut`
    // param never overlaps an unmarked one (the call-site rule), and an
    // attempt to write an unmarked param is rejected as SEM056.
    accepted(&kernel_fn(
        &format!("in {GRID} out, {GRID} src"),
        "out[i] ← src[i + 1]",
    ));
    accepted(&kernel_fn(
        &format!("in {GRID} out, {GRID} left, {GRID} src"),
        "out[i] ← left[i + 1] + src[i + 2]",
    ));
    assert_rejects_with_issue_la(
        &kernel_fn(
            &format!("{GRID} left, {GRID} src"),
            "left[i] ← value + src[i]",
        ),
        "assign_to_nonmut_tensor_param",
    );
    // The mut param itself still meets the neighbour-read rule.
    rejected(
        &kernel_fn(&format!("in {GRID} out, {GRID} src"), "out[i] ← out[i + 1]"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn own_param_has_no_known_storage() {
    // `k(own a, mut b)` is not rejected at the call site for a non-exclusive
    // argument, so an `own` parameter keeps the unknown-identity decline.
    rejected(
        &with_src(&format!("own {GRID} out"), "out[i] ← value"),
        "unknown_storage_identity",
    );
}

#[test]
fn list_param_write_keeps_unknown_identity() {
    rejected(
        &with_src("in lista<numerus> xs", "xs[i] ← value"),
        "unknown_storage_identity",
    );
}

#[test]
fn call_from_the_body_stays_declined_with_param_roots() {
    rejected(
        &format!(
            r"functio inner(in {GRID} out) → vacuum {{
}}
functio kernel(in {GRID} out, de {GRID} src) → vacuum {{
    itera ex src apud [i] filum lane fixum value {{
        inner(varia out)
        out[i] ← value
    }}
}}
incipit {{
}}"
        ),
        "unproved_call_effect",
    );
}

#[test]
fn write_through_a_read_param_is_a_borrow_error_not_an_independence_one() {
    let result = analyze_source_la(&with_src(&format!("in {GRID} out"), "src[i] ← value"));
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors
            .iter()
            .any(|d| d.issue() == Some("assign_to_de_parameter")),
        "{errors:?}"
    );
    assert!(
        errors.iter().all(|d| d.issue() != Some(ISSUE)),
        "filum owns a borrow error: {errors:?}"
    );
}

#[test]
fn aliasing_call_is_a_borrow_error_while_the_kernel_is_certified() {
    let source = format!(
        r"functio kernel(in {GRID} out, de {GRID} src) → vacuum {{
    itera ex src apud [i] filum lane fixum value {{
        out[i] ← src[i + 1]
    }}
}}
incipit {{
    varia {GRID} grid ← ti64[4] {{}}
    varia {GRID} alias ← grid
    kernel(varia grid, alias)
}}"
    );
    let result = analyze_source_la(&source);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors
            .iter()
            .any(|d| d.issue() == Some("shared_borrow_conflict")),
        "{errors:?}"
    );
    assert!(
        errors.iter().all(|d| d.issue() != Some(ISSUE)),
        "kernel should be certified: {errors:?}"
    );
}
