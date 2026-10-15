//! A typed-zero scratch tensor inside a `filum` body is fresh iteration-local
//! storage, whichever way it is spelled (`vacua`/`empty`, or `Type {}`).
//!
//! The checker rewrites `empty` at a dense tensor slot into the typed-zero
//! construction (`crea` on a pre-typed `vacua` receiver). Before the rewrite a
//! bare `vacua` was fresh storage; the independence pass must give the typed
//! zero the same proof, and nothing wider than that exact shape.

use crate::test_support::analyze_source;

fn kernel(scratch_init: &str, statement: &str) -> String {
    format!(
        r"
@ nucleum
@ publica
functio k(varia tensor<f32, [4]> out, de tensor<f32, [4]> src) → vacuum {{
    itera ex src apud [i] filum lane sit value {{
        varia tensor<f32, [4]> scratch ← {scratch_init}
        {statement}
        out[i] ← scratch[i]
    }}
}}
"
    )
}

fn accepted(source: &str) {
    let result = analyze_source(source);
    assert!(result.success(), "{:?}", result.diagnostics);
}

fn rejected(source: &str, issue: &str) {
    let result = analyze_source(source);
    let issues: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .filter_map(|d| d.issue())
        .collect();
    assert!(issues.contains(&issue), "expected {issue}, got {issues:?}");
}

#[test]
fn vacua_scratch_is_iteration_local_storage() {
    accepted(&kernel("vacua", "scratch[0] ← value"));
}

#[test]
fn typed_zero_scratch_is_iteration_local_storage() {
    accepted(&kernel("tensor<f32, [4]> {}", "scratch[0] ← value"));
}

#[test]
fn both_spellings_certify_the_same_body() {
    for init in ["vacua", "tensor<f32, [4]> {}"] {
        accepted(&kernel(init, "scratch[i] ← value"));
    }
}

#[test]
fn scratch_aliasing_outer_storage_is_still_declined() {
    // The same write that a fresh scratch tolerates is a loop-carried write
    // when the local is a view of the outer tensor.
    rejected(
        &kernel("out", "scratch[0] ← value"),
        "filum_independence_unproven",
    );
}

#[test]
fn crea_on_a_real_tensor_is_not_a_fresh_typed_zero() {
    // An authored `.crea(..)` on a tensor value is not the typed-zero shape
    // (its receiver is a path, not the synthesized `vacua`): the checker
    // refuses it before the independence pass could mistake it for one.
    rejected(
        &kernel("src.crea(0.0, [4])", "scratch[0] ← value"),
        "unknown_method",
    );
}
