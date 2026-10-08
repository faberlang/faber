//! `empty` at a dense tensor, vector, or matrix slot is the typed zero
//! (operator ruling 2026-10-08). Checker side: which slots accept it, which
//! refuse it by name, and that rank-0, sparse, and collection slots keep
//! their existing behaviour.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::analyze_source_with_config;
use radix_target::Target;

fn config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config
}

fn check(source: &str) -> crate::test_support::AnalyzedFixture {
    analyze_source_with_config(source, config())
}

fn issues(source: &str) -> Vec<String> {
    check(source)
        .diagnostics
        .iter()
        .filter_map(|diag| {
            diag.args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

fn accepts(source: &str) {
    let result = check(source);
    assert!(
        result.is_ok(),
        "expected green for {source:?}, got: {:?}",
        result.diagnostics
    );
}

fn refuses_with(source: &str, issue: &str) {
    let found = issues(source);
    assert!(
        found.iter().any(|found| found == issue),
        "expected {issue} for {source:?}, got: {found:?}"
    );
}

const REFUSED: &str = "vacua_requires_resolved_extents";

#[test]
fn empty_accepts_dense_shorthand_tensor() {
    accepts("fn main() {\n    const tf32[2, 3] a ← empty\n}\n");
}

#[test]
fn empty_accepts_dense_long_form_tensor() {
    accepts("fn main() {\n    const tensor<f32, [2, 3]> a ← empty\n}\n");
}

#[test]
fn empty_accepts_vector_and_matrix() {
    accepts("fn main() {\n    const vf32[4] a ← empty\n    const mf32[2, 2] b ← empty\n}\n");
}

#[test]
fn empty_accepts_tensor_with_vector_element() {
    accepts("fn main() {\n    const tensor<vf32[2], [3]> a ← empty\n}\n");
}

#[test]
fn empty_accepts_declared_size_and_size_generic_extents() {
    accepts("fn main() {\n    const size K ← 5\n    const tf32[K, 2] a ← empty\n}\n");
    accepts("fn zeros<size N>() → tensor<f32, [N]> {\n    return empty\n}\n");
}

#[test]
fn empty_accepts_return_assignment_and_argument_positions() {
    accepts(
        "fn pick() → tf32[2] {\n    return empty\n}\n\
         fn take(tf32[2] x) → void {\n}\n\
         fn main() {\n    var tf32[2] w ← pick()\n    w ← empty\n    take(empty)\n}\n",
    );
}

#[test]
fn empty_refuses_unresolved_extent_by_name() {
    refuses_with("fn main() {\n    const tf32[2, _] a ← empty\n}\n", REFUSED);
    refuses_with("fn main() {\n    const tf32[_, _] a ← empty\n}\n", REFUSED);
}

#[test]
fn refused_empty_does_not_cascade_store_mismatches() {
    let found =
        issues("fn f<size N>() → vf32[N] {\n    const vf32[N] a ← empty\n    return a\n}\n");
    assert!(found.iter().any(|found| found == REFUSED), "got: {found:?}");
    for cascade in [
        "expression_type_mismatch",
        "initializer_annotation_mismatch",
    ] {
        assert!(
            !found.iter().any(|found| found == cascade),
            "{cascade} cascaded: {found:?}"
        );
    }
}

#[test]
fn empty_refuses_affine_extent_that_the_typed_zero_refuses() {
    let typed = "fn f<size N>() → tf32[N + 1] {\n    return tf32[N + 1] {}\n}\n";
    refuses_with(typed, "constructor_affine_extent_unsupported");
    refuses_with(
        "fn f<size N>() → tf32[N + 1] {\n    return empty\n}\n",
        REFUSED,
    );
}

#[test]
fn empty_refuses_register_extent_that_the_typed_zero_refuses() {
    let typed = "fn f<size N>() → vf32[N] {\n    return vf32[N] {}\n}\n";
    refuses_with(typed, "constructor_register_extent_requires_literal");
    refuses_with("fn f<size N>() → vf32[N] {\n    return empty\n}\n", REFUSED);
}

#[test]
fn empty_refuses_scalar_context_as_before() {
    refuses_with(
        "fn main() {\n    const int a ← empty\n}\n",
        "vacua_requires_collection_type",
    );
}

#[test]
fn empty_keeps_rank_zero_sparse_and_collection_slots() {
    accepts("fn main() {\n    const tensor<f32, []> a ← empty\n}\n");
    accepts("fn main() {\n    const sparsa<f32, [2, 3]> a ← empty\n}\n");
    accepts(
        "fn main() {\n    const list<int> a ← empty\n    const tabula<textus, int> b ← empty\n    const copia<int> c ← empty\n}\n",
    );
}
