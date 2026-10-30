//! K8 look-through of private kernel helper calls (gpu-reset rule 12.7).
//!
//! A call inside a `filum` body (or a reduction term) to a private `@ nucleum`
//! helper is analysed as if inlined: the helper's tensor parameters take the
//! argument storage, its coordinate parameters take the argument coordinates,
//! and its reads and writes meet the rules of the enclosing body. Anything
//! the analysis cannot see through keeps the named decline.

use crate::test_support::{analyze_source_la, assert_accepts_la, assert_rejects_with_issue_la};
use std::fmt::Write;

const ISSUE: &str = "filum_independence_unproven";
const GRID: &str = "tensor<f32, [4]>";

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

/// A public kernel whose `filum` body is `statement`; `out` is written, `src`
/// drives the loop, `value` is the element.
fn kernel(helpers: &str, statement: &str) -> String {
    format!(
        r"{helpers}
@ nucleum
@ publica
functio k(in {GRID} out, de {GRID} src) → vacuum {{
    itera ex src apud [i] filum lane sit value {{
        {statement}
    }}
}}
"
    )
}

// =============================================================================
// Writes and reads through a helper
// =============================================================================

const STORE: &str = r"
@ nucleum
functio store(varia tensor<f32, [4]> target, i32 at, f32 v) → vacuum {
    target[at] ← v
}
";

#[test]
fn helper_writing_through_a_mut_param_at_the_loop_coordinate_is_accepted() {
    accepted(&kernel(STORE, "store(varia out, i, value + 1.0)"));
}

#[test]
fn helper_scalar_value_arguments_and_returns_are_accepted() {
    accepted(&kernel(
        r"
@ nucleum
functio twice(f32 x) → f32 { redde x * 2.0 }
",
        "out[i] ← twice(value)",
    ));
}

#[test]
fn helper_reading_a_neighbour_of_the_written_tensor_is_rejected() {
    let peek = r"
@ nucleum
functio peek(de tensor<f32, [4]> t, i32 at) → f32 { redde t[at + 1] }
";
    rejected(
        &kernel(peek, "out[i] ← peek(out, i)"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn helper_reading_a_neighbour_of_an_unwritten_tensor_is_accepted() {
    let peek = r"
@ nucleum
functio peek(de tensor<f32, [4]> t, i32 at) → f32 { redde t[at + 1] }
";
    accepted(&kernel(peek, "out[i] ← peek(src, i)"));
}

#[test]
fn helper_reading_the_written_tensor_at_the_own_coordinate_is_accepted() {
    let read = r"
@ nucleum
functio read(de tensor<f32, [4]> t, i32 at) → f32 { redde t[at] }
";
    accepted(&kernel(read, "out[i] ← read(out, i) + value"));
}

#[test]
fn helper_writing_at_another_coordinate_is_rejected() {
    let poke = r"
@ nucleum
functio poke(varia tensor<f32, [4]> target, f32 v) → vacuum { target[0] ← v }
";
    rejected(
        &kernel(poke, "poke(varia out, value)"),
        "write_not_own_coordinate",
    );
    // A coordinate the analysis cannot map is not the own coordinate.
    let shifted = r"
@ nucleum
functio poke(varia tensor<f32, [4]> target, i32 at, f32 v) → vacuum { target[at + 1] ← v }
";
    rejected(
        &kernel(shifted, "poke(varia out, i, value)"),
        "write_not_own_coordinate",
    );
}

#[test]
fn helper_argument_that_is_not_the_loop_coordinate_is_not_an_own_write() {
    rejected(
        &kernel(STORE, "store(varia out, 0, value)"),
        "write_not_own_coordinate",
    );
}

#[test]
fn two_mut_params_given_one_tensor_are_a_borrow_error_not_an_independence_one() {
    let both = r"
@ nucleum
functio both(varia tensor<f32, [4]> left, varia tensor<f32, [4]> right, i32 at) → vacuum {
    left[at] ← 1.0
    right[at] ← 2.0
}
";
    let result = analyze_source_la(&kernel(both, "both(varia out, varia out, i)"));
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.iter().any(|d| d
            .issue()
            .is_some_and(|issue| issue.contains("borrow_conflict"))),
        "{errors:?}"
    );
    assert!(
        errors.iter().all(|d| d.issue() != Some(ISSUE)),
        "borrow owns the overlap: {errors:?}"
    );
}

#[test]
fn helper_reading_a_neighbour_through_an_alias_argument_is_rejected() {
    let peek = r"
@ nucleum
functio peek(de tensor<f32, [4]> t, i32 at) → f32 { redde t[at + 1] }
";
    let source = kernel(peek, "out[i] ← peek(alias, i)").replace(
        "    itera ex src",
        &format!("    varia {GRID} alias ← out\n    itera ex src"),
    );
    rejected(&source, "written_storage_neighbor_read");
}

#[test]
fn helper_local_alias_of_a_parameter_keeps_the_parameter_storage() {
    let aliased = r"
@ nucleum
functio peek(de tensor<f32, [4]> t, i32 at) → f32 {
    sit tensor<f32, [4]> view ← t
    redde view[at + 1]
}
";
    rejected(
        &kernel(aliased, "out[i] ← peek(out, i)"),
        "written_storage_neighbor_read",
    );
    accepted(&kernel(aliased, "out[i] ← peek(src, i)"));
}

#[test]
fn mut_helper_param_and_unmarked_reader_param_follow_the_caller_roots() {
    // Reading A: an unmarked helper parameter is a shared read-only view, so
    // only the `mut` (`in`) one is written. A `mut` caller parameter never
    // overlaps an unmarked one, so the write and the neighbour read are
    // independent; the helper's write still meets the neighbour-read rule.
    let copy = r"
@ nucleum
functio copy(varia tensor<f32, [4]> to, tensor<f32, [4]> from, i32 at) → vacuum {
    to[at] ← from[at + 1]
}
";
    accepted(&kernel(copy, "copy(varia out, src, i)"));
    // Reading the written parameter's own neighbour through the helper's
    // other parameter would need the same storage twice, which the call-site
    // borrow rule owns; the helper reading its own `mut` parameter does not.
    let self_read = r"
@ nucleum
functio shift(varia tensor<f32, [4]> to, i32 at) → vacuum {
    to[at] ← to[at + 1]
}
";
    rejected(
        &kernel(self_read, "shift(varia out, i)"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn unmarked_helper_param_cannot_be_written() {
    // The old scenario (a helper writing an unmarked parameter) is impossible
    // by construction under reading A: it is rejected at the helper itself.
    let copy = r"
@ nucleum
functio copy(tensor<f32, [4]> to, tensor<f32, [4]> from, i32 at) → vacuum {
    to[at] ← from[at + 1]
}
";
    assert_rejects_with_issue_la(
        &kernel(copy, "copy(out, src, i)"),
        "assign_to_nonmut_tensor_param",
    );
}

#[test]
fn a_write_and_a_neighbour_read_in_two_helper_calls_are_rejected() {
    let peek = r"
@ nucleum
functio peek(de tensor<f32, [4]> t, i32 at) → f32 { redde t[at + 1] }
";
    let both = format!("{STORE}{peek}");
    rejected(
        &kernel(
            &both,
            "store(varia out, i, value)\n        sit f32 next ← peek(out, i)",
        ),
        "written_storage_neighbor_read",
    );
}

// =============================================================================
// Depth, recursion and the callees that stay declined
// =============================================================================

#[test]
fn nested_helper_calls_are_accepted() {
    let nested = format!(
        "{STORE}
@ nucleum
functio outer(varia tensor<f32, [4]> target, i32 at, f32 v) → vacuum {{
    store(varia target, at, v + 1.0)
}}
"
    );
    accepted(&kernel(&nested, "outer(varia out, i, value)"));
}

#[test]
fn nested_helper_that_writes_elsewhere_is_rejected() {
    let nested = format!(
        "{STORE}
@ nucleum
functio outer(varia tensor<f32, [4]> target, f32 v) → vacuum {{
    store(varia target, 0, v)
}}
"
    );
    rejected(
        &kernel(&nested, "outer(varia out, value)"),
        "write_not_own_coordinate",
    );
}

#[test]
fn direct_recursion_is_declined_by_name() {
    let spin = r"
@ nucleum
functio spin(f32 x) → f32 { redde spin(x) }
";
    rejected(&kernel(spin, "out[i] ← spin(value)"), "recursive_call");
}

#[test]
fn mutual_recursion_is_declined_by_name() {
    let pingpong = r"
@ nucleum
functio ping(f32 x) → f32 { redde pong(x) }
@ nucleum
functio pong(f32 x) → f32 { redde ping(x) }
";
    rejected(&kernel(pingpong, "out[i] ← ping(value)"), "recursive_call");
}

#[test]
fn chain_past_the_inline_depth_is_declined_by_name() {
    let mut helpers = String::from("@ nucleum\nfunctio h0(f32 x) → f32 { redde x }\n");
    for level in 1..=9 {
        writeln!(
            helpers,
            "@ nucleum\nfunctio h{level}(f32 x) → f32 {{ redde h{}(x) }}",
            level - 1
        )
        .expect("write to a string");
    }
    accepted(&kernel(&helpers, "out[i] ← h7(value)"));
    rejected(
        &kernel(&helpers, "out[i] ← h9(value)"),
        "inline_depth_exceeded",
    );
}

#[test]
fn host_function_call_stays_declined() {
    let host = r"
functio host(f32 x) → f32 { redde x }
";
    rejected(
        &kernel(host, "out[i] ← host(value)"),
        "unproved_call_effect",
    );
}

#[test]
fn public_kernel_call_is_looked_through() {
    // Rule 12.7 (extended 2026-10-04): a public kernel called from a kernel is
    // just a call; its body is analysed as inlined.
    let public = r"
@ nucleum
@ publica
functio shown(f32 x) → f32 { redde x }
";
    accepted(&kernel(public, "out[i] ← shown(value)"));
}

#[test]
fn helper_with_a_host_call_inside_is_declined_at_that_call() {
    let leaky = r"
functio host(f32 x) → f32 { redde x }
@ nucleum
functio leaky(f32 x) → f32 { redde host(x) }
";
    rejected(
        &kernel(leaky, "out[i] ← leaky(value)"),
        "unproved_call_effect",
    );
}

#[test]
fn helper_with_a_parallel_loop_is_declined() {
    let looped = r"
@ nucleum
functio sweep(varia tensor<f32, [4]> target) → vacuum {
    itera ex target apud [j] filum inner sit cell { target[j] ← cell }
}
";
    rejected(&kernel(looped, "sweep(varia out)"), "unproved_loop_form");
}

// =============================================================================
// Callee locals
// =============================================================================

#[test]
fn helper_locals_are_iteration_local() {
    accepted(&kernel(
        r"
@ nucleum
functio ramp(f32 x) → f32 {
    varia f32 acc ← x
    acc ← acc + 1.0
    acc ← acc * 2.0
    redde acc
}
",
        "out[i] ← ramp(value)",
    ));
}

#[test]
fn helper_reassigning_a_tensor_parameter_is_declined() {
    let swap = r"
@ nucleum
functio swap(varia tensor<f32, [4]> target, de tensor<f32, [4]> other) → vacuum {
    target ← other
}
";
    let result = analyze_source_la(&kernel(swap, "swap(varia out, src)"));
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.iter().any(|d| {
            d.issue() == Some(ISSUE)
                && d.args
                    .iter()
                    .any(|arg| arg.name == "reason" && arg.value == "outer_local_mutation")
        }),
        "{errors:?}"
    );
}

// =============================================================================
// Reduction terms
// =============================================================================

const SQUARE: &str = r"
@ nucleum
functio square(f32 x) → f32 { redde x * x }
";

#[test]
fn reduction_term_may_call_a_pure_private_helper() {
    assert_accepts_la(&format!(
        r"{SQUARE}
@ nucleum
functio k(tensor<f32, [32]> xs) → f32 {{
    redde reducta via summa ex xs apud [i] filum f sit s {{ redde square(s) }}
}}
"
    ));
    assert_accepts_la(&format!(
        r"{SQUARE}
@ nucleum
functio k(tensor<f32, [4]> xs) → f32 {{
    redde reducta via summa ex xs apud [i] sit s {{ redde square(s) }}
}}
"
    ));
}

#[test]
fn reduction_term_may_call_a_helper_that_reads_a_tensor() {
    assert_accepts_la(
        r"
@ nucleum
functio at(de tensor<f32, [4]> t, i32 j) → f32 { redde t[j] }
@ nucleum
functio k(tensor<f32, [4]> xs, tensor<f32, [4]> ys) → f32 {
    redde reducta via summa ex xs apud [i] sit s { redde s + at(ys, i + 1) }
}
",
    );
}

#[test]
fn reduction_term_calling_a_host_function_is_still_rejected() {
    assert_rejects_with_issue_la(
        r"
functio host(f32 x) → f32 { redde x }
@ nucleum
functio k(tensor<f32, [32]> xs) → f32 {
    redde reducta via summa ex xs apud [i] filum f sit s { redde host(s) }
}
",
        "reducta_body_context",
    );
}

#[test]
fn reduction_term_may_call_a_public_kernel() {
    assert_accepts_la(
        r"
@ nucleum
@ publica
functio shown(f32 x) → f32 { redde x }
@ nucleum
functio k(tensor<f32, [32]> xs) → f32 {
    redde reducta via summa ex xs apud [i] filum f sit s { redde shown(s) }
}
",
    );
}

#[test]
fn reduction_term_may_call_a_helper_that_calls_a_helper() {
    assert_accepts_la(&format!(
        r"{SQUARE}
@ nucleum
functio speed(f32 x) → f32 {{ redde square(x) + 1.0 }}
@ nucleum
functio k(tensor<f32, [32]> xs) → f32 {{
    redde reducta via summa ex xs apud [i] filum f sit s {{ redde speed(s) }}
}}
"
    ));
}

#[test]
fn reduction_term_calling_a_failable_kernel_is_accepted_when_the_caller_can_fail() {
    // KTHROW (ruling 2026-10-08): a helper with `⇥ text` is inlinable; the
    // caller propagates through its own `⇥ text`.
    accepted(
        r"
@ nucleum
functio risky(f32 x) → f32 ⇥ textus { redde x }
@ nucleum
functio k(tensor<f32, [32]> xs) → f32 ⇥ textus {
    redde reducta via summa ex xs apud [i] filum f sit s { redde risky(s) }
}
",
    );
}

#[test]
fn reduction_term_helper_that_writes_is_rejected() {
    let result = analyze_source_la(
        r"
@ nucleum
functio dirty(varia tensor<f32, [32]> t, f32 x) → f32 {
    t[0] ← x
    redde x
}
@ nucleum
functio k(varia tensor<f32, [32]> xs) → f32 {
    redde reducta via summa ex xs apud [i] filum f sit s { redde dirty(varia xs, s) }
}
",
    );
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors
            .iter()
            .any(|d| d.issue() == Some("reducta_body_effect")
                && d.args
                    .iter()
                    .any(|arg| arg.name == "reason" && arg.value == "captured_mut_host")),
        "{errors:?}"
    );
}

#[test]
fn reduction_term_helper_with_a_host_call_inside_is_rejected() {
    let result = analyze_source_la(
        r"
functio host(f32 x) → f32 { redde x }
@ nucleum
functio leaky(f32 x) → f32 { redde host(x) }
@ nucleum
functio k(tensor<f32, [32]> xs) → f32 {
    redde reducta via summa ex xs apud [i] filum f sit s { redde leaky(s) }
}
",
    );
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors
            .iter()
            .any(|d| d.issue() == Some("reducta_body_effect")),
        "{errors:?}"
    );
}

// =============================================================================
// What a call outside the loop can disturb
// =============================================================================

fn host_function(statements: &str) -> String {
    format!(
        r"
functio log(f32 x) → vacuum {{ }}
functio log_grid(de {GRID} t) → vacuum {{ }}
genus Counter {{
    varia i32 count
    functio bump() → vacuum {{ ego.count ← ego.count + 1 }}
}}
functio kernel(in {GRID} out, de {GRID} src) → vacuum {{
    varia {GRID} scratch ← tf32[4] {{}}
    varia Counter counter ← Counter {{ count = 0 }}
    {statements}
    itera ex src apud [i] filum lane sit value {{
        out[i] ← value
    }}
}}
incipit {{
}}"
    )
}

#[test]
fn host_call_before_the_loop_keeps_parameter_roots() {
    accepted(&host_function("log(1.0)"));
    // Only the argument's own storage is in reach of the call.
    accepted(&host_function("log_grid(scratch)"));
}

#[test]
fn inert_helper_call_before_the_loop_keeps_parameter_roots() {
    let total = r"
@ nucleum
functio total(de tensor<f32, [4]> t) → f32 { redde t[0] }
";
    for argument in ["src", "out"] {
        accepted(&kernel(total, "out[i] ← value + z").replace(
            "    itera ex src",
            &format!("    sit f32 z ← total({argument})\n    itera ex src"),
        ));
    }
}

#[test]
fn host_call_taking_the_written_parameter_poisons_it() {
    rejected(&host_function("log_grid(out)"), "unknown_storage_identity");
}

#[test]
fn opaque_effect_before_the_loop_still_poisons_the_function() {
    rejected(&host_function("counter.bump()"), "unknown_storage_identity");
}

// =============================================================================
// Register-class components and views
// =============================================================================

#[test]
fn vector_component_of_the_element_binder_is_a_value_read() {
    accepted(
        r"
functio k(de tensor<vector<f32, magnitudo 4>, [4]> state, varia tensor<f32, [4]> depth) → vacuum {
    itera ex state apud [i] filum lane sit c {
        depth[i] ← c[0]
    }
}
incipit {
}",
    );
}

#[test]
fn lane_store_on_a_lane_local_vector_is_accepted() {
    accepted(
        r"
functio k(de tensor<vector<f32, magnitudo 4>, [4]> state, varia tensor<vector<f32, magnitudo 4>, [4]> out) → vacuum {
    itera ex state apud [i] filum lane sit c {
        varia vector<f32, magnitudo 4> s ← c
        s[1] ← 0.0
        out[i] ← s
    }
}
incipit {
}",
    );
}

#[test]
fn lane_store_on_an_outer_vector_local_is_outer_local_mutation() {
    rejected(
        r"
functio k(de tensor<vector<f32, magnitudo 4>, [4]> state, varia tensor<f32, [4]> out) → vacuum {
    varia vector<f32, magnitudo 4> acc ← state[0]
    itera ex state apud [i] filum lane sit c {
        acc[0] ← c[0]
        out[i] ← acc[0]
    }
}
incipit {
}",
        "outer_local_mutation",
    );
}

#[test]
fn vector_tensor_written_in_place_still_rejects_neighbour_reads() {
    rejected(
        r"
functio k(varia tensor<vector<f32, magnitudo 4>, [4]> state) → vacuum {
    itera ex state apud [i] filum lane sit c {
        state[i] ← state[i + 1]
    }
}
incipit {
}",
        "written_storage_neighbor_read",
    );
}

/// Unmarked tensor parameters: a shifted view needs a plain tensor receiver.
fn view_kernel(statement: &str) -> String {
    format!(
        r"
functio k(in {GRID} out, {GRID} src) → vacuum {{
    itera ex src apud [i] filum lane sit value {{
        {statement}
    }}
}}
incipit {{
}}"
    )
}

#[test]
fn edge_view_of_an_unwritten_tensor_is_accepted() {
    accepted(&view_kernel(
        "sit tensor<f32, [4]> west ← src.disloco([1]).limes<clampa>()\n        out[i] ← west[i] + value",
    ));
    accepted(
        r"
functio k(varia tensor<f32, [4]> out, tensor<f32, [4]> src) → vacuum {
    sit tensor<f32, [4]> west ← src.disloco([1]).limes<reflexa>()
    itera ex src apud [i] filum lane sit value {
        out[i] ← west[i] + value
    }
}
incipit {
}",
    );
}

#[test]
fn edge_view_of_the_written_tensor_is_rejected() {
    // The written parameter is `mut` (reading A), so the K8 check, not the
    // unmarked-write error, judges the view that reaches the written tensor.
    rejected(
        r"
functio k(varia tensor<f32, [4]> out, tensor<f32, [4]> src) → vacuum {
    varia tensor<f32, [4]> alias ← out
    sit tensor<f32, [4]> west ← alias.disloco([1]).limes<clampa>()
    itera ex src apud [i] filum lane sit value {
        out[i] ← west[i] + value
    }
}
incipit {
}",
        "written_storage_neighbor_read",
    );
}

#[test]
fn edge_view_of_an_unmarked_param_cannot_launder_a_write() {
    // Writing the unmarked tensor directly is rejected before any view matters.
    assert_rejects_with_issue_la(
        r"
functio k(tensor<f32, [4]> out, tensor<f32, [4]> src) → vacuum {
    itera ex src apud [i] filum lane sit value {
        out[i] ← value
    }
}
incipit {
}",
        "assign_to_nonmut_tensor_param",
    );
}

// =============================================================================
// A helper with a `⇥ text` error channel (KTHROW, ruling 2026-10-08)
// =============================================================================

const FAILABLE: &str = r"
@ nucleum
functio h(f32 x) → f32 ⇥ textus {
    si x ≺ 0.0 {
        iace 'negative'
    }
    redde x
}
";

/// A public kernel that can itself fail, so a failable helper call
/// propagates through its own `⇥ text`.
fn failable_kernel(helpers: &str, statement: &str) -> String {
    kernel(helpers, statement).replace("→ vacuum {", "→ vacuum ⇥ textus {")
}

#[test]
fn failable_helper_is_admitted_in_a_lane_body() {
    accepted(&failable_kernel(FAILABLE, "out[i] ← h(value)"));
}

#[test]
fn failable_helper_without_a_failing_caller_stays_a_named_error() {
    // The failure has no handler in the lane body and the kernel cannot
    // propagate it: it must not disappear.
    assert_rejects_with_issue_la(
        &kernel(FAILABLE, "out[i] ← h(value)"),
        "failable_call_requires_handler",
    );
}

#[test]
fn failable_helper_is_still_held_to_the_independence_rules() {
    let peek = r"
@ nucleum
functio peek(de tensor<f32, [4]> t, i32 at) → f32 ⇥ textus {
    si at ≺ 0 {
        iace 'negative'
    }
    redde t[at + 1]
}
";
    rejected(
        &failable_kernel(peek, "out[i] ← peek(out, i)"),
        "written_storage_neighbor_read",
    );
}
