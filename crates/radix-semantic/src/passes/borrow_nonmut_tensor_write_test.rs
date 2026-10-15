//! G20: a write through a tensor parameter that is not marked `mut` is a
//! check-time error, for ordinary functions and `@ kernel` functions alike.
//!
//! Three write shapes are rejected: an index store, a copy-into `⇇`, and
//! passing the parameter to a `mut` parameter of another function. Each
//! negative fixture has a positive twin that differs only by `mut`.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

const ISSUE: &str = "assign_to_nonmut_tensor_param";

/// The fixtures are written in the English surface.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn assert_accepts(source: &str) {
    let result = analyze_en(source);
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "expected no error, got: {:?}",
        result.diagnostics
    );
}

fn assert_rejects_naming(source: &str, param: &str) {
    let result = analyze_en(source);
    let found = result.diagnostics.iter().find(|d| {
        d.is_error()
            && d.kind == SemanticErrorKind::AssignToImmutableBorrow
            && issue(d) == Some(ISSUE)
    });
    let found = found.unwrap_or_else(|| panic!("expected {ISSUE}, got: {:?}", result.diagnostics));
    assert!(
        found
            .args
            .iter()
            .any(|a| a.name == "name" && a.value.to_string() == param),
        "diagnostic does not name `{param}`: {found:?}"
    );
}

#[test]
fn index_store_through_a_nonmut_tensor_param_is_rejected() {
    assert_rejects_naming(
        r"fn bump(tf32[2, 3] dst) → void {
    dst[[0, 0]] ← 1.0
}
main {
}",
        "dst",
    );
}

#[test]
fn index_store_through_a_mut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn bump(mut tf32[2, 3] dst) → void {
    dst[[0, 0]] ← 1.0
}
main {
}",
    );
}

#[test]
fn copy_into_a_nonmut_tensor_param_is_rejected() {
    assert_rejects_naming(
        r"fn put(tf32[2, 3] dst, tf32[2, 3] src) → void {
    dst ⇇ src
}
main {
}",
        "dst",
    );
}

#[test]
fn copy_into_a_mut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn put(mut tf32[2, 3] dst, tf32[2, 3] src) → void {
    dst ⇇ src
}
main {
}",
    );
}

#[test]
fn passing_a_nonmut_tensor_param_to_a_mut_param_is_rejected() {
    assert_rejects_naming(
        r"fn inner(mut tf32[2, 3] dst) → void {
    dst[[0, 0]] ← 1.0
}
fn outer(tf32[2, 3] dst) → void {
    inner(mut dst)
}
main {
}",
        "dst",
    );
}

#[test]
fn passing_a_mut_tensor_param_to_a_mut_param_is_accepted() {
    assert_accepts(
        r"fn inner(mut tf32[2, 3] dst) → void {
    dst[[0, 0]] ← 1.0
}
fn outer(mut tf32[2, 3] dst) → void {
    inner(mut dst)
}
main {
}",
    );
}

#[test]
fn kernel_write_through_a_nonmut_tensor_param_is_rejected() {
    assert_rejects_naming(
        r"@ kernel
fn bump(tf32[2, 3] dst) → void {
    dst[[0, 0]] ← 1.0
}
main {
}",
        "dst",
    );
}

#[test]
fn kernel_write_through_a_mut_tensor_param_is_accepted() {
    assert_accepts(
        r"@ kernel
fn bump(mut tf32[2, 3] dst) → void {
    dst[[0, 0]] ← 1.0
}
main {
}",
    );
}

#[test]
fn reading_a_nonmut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn first(tf32[2, 3] src) → f32 {
    return src[[0, 0]]
}
main {
}",
    );
}

// Method calls and `←` aliases of a non-mut tensor parameter.

#[test]
fn fill_statement_on_a_nonmut_tensor_param_is_rejected() {
    assert_rejects_naming(
        r"fn clear(tf32[2, 3] dst) → void {
    dst.fill(0.0)
}
main {
}",
        "dst",
    );
}

#[test]
fn fill_statement_on_a_mut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn clear(mut tf32[2, 3] dst) → void {
    dst.fill(0.0)
}
main {
}",
    );
}

#[test]
fn bound_fill_of_a_nonmut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn cleared(tf32[2, 3] src) → tf32[2, 3] {
    return src.fill(0.0)
}
main {
}",
    );
}

#[test]
fn element_set_method_on_a_nonmut_tensor_param_is_rejected() {
    assert_rejects_naming(
        r"fn put(tf32[2, 3] dst) → void {
    dst.set([0, 0], 1.0)
}
main {
}",
        "dst",
    );
}

#[test]
fn element_set_method_on_a_mut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn put(mut tf32[2, 3] dst) → void {
    dst.set([0, 0], 1.0)
}
main {
}",
    );
}

#[test]
fn index_store_through_an_alias_of_a_nonmut_tensor_param_is_rejected() {
    assert_rejects_naming(
        r"fn bump(tf32[2, 3] dst) → void {
    let tf32[2, 3] c ← dst
    c[[0, 0]] ← 1.0
}
main {
}",
        "dst",
    );
}

#[test]
fn index_store_through_an_alias_of_an_alias_is_rejected() {
    assert_rejects_naming(
        r"fn bump(tf32[2, 3] dst) → void {
    let tf32[2, 3] c ← dst
    let tf32[2, 3] d ← c
    d[[0, 0]] ← 1.0
}
main {
}",
        "dst",
    );
}

#[test]
fn index_store_through_an_alias_of_a_mut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn bump(mut tf32[2, 3] dst) → void {
    mut tf32[2, 3] c ← dst
    c[[0, 0]] ← 1.0
}
main {
}",
    );
}

#[test]
fn copy_into_and_mut_call_through_an_alias_of_a_nonmut_tensor_param_are_rejected() {
    assert_rejects_naming(
        r"fn put(tf32[2, 3] dst, tf32[2, 3] src) → void {
    let tf32[2, 3] c ← dst
    c ⇇ src
}
main {
}",
        "dst",
    );
    assert_rejects_naming(
        r"fn inner(mut tf32[2, 3] dst) → void {
    dst[[0, 0]] ← 1.0
}
fn outer(tf32[2, 3] dst) → void {
    let tf32[2, 3] c ← dst
    inner(mut c)
}
main {
}",
        "dst",
    );
}

#[test]
fn fill_statement_through_an_alias_of_a_nonmut_tensor_param_is_rejected() {
    assert_rejects_naming(
        r"fn clear(tf32[2, 3] dst) → void {
    let tf32[2, 3] c ← dst
    c.fill(0.0)
}
main {
}",
        "dst",
    );
}

#[test]
fn mut_alias_of_a_nonmut_tensor_param_is_rejected() {
    assert_rejects_naming(
        r"fn bump(tf32[2, 3] dst) → void {
    mut mut tf32[2, 3] c ← dst
    c[[0, 0]] ← 1.0
}
main {
}",
        "dst",
    );
}

#[test]
fn mut_alias_of_a_mut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn bump(mut tf32[2, 3] dst) → void {
    mut mut tf32[2, 3] c ← dst
    c[[0, 0]] ← 1.0
}
main {
}",
    );
}

#[test]
fn reading_through_an_alias_of_a_nonmut_tensor_param_is_accepted() {
    assert_accepts(
        r"fn first(tf32[2, 3] src) → f32 {
    let tf32[2, 3] c ← src
    return c[[0, 0]]
}
main {
}",
    );
}
