//! Tensor uses the lints must see: a statement-form `fill` is a write through
//! a `mut` parameter (WARN008), and a scalar named in a tensor extent is a use
//! of that scalar (WARN001). Each fixture has a negative twin that keeps the
//! warning, so the fix cannot be a blanket suppression.
//! Fixtures use the English surface.

use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};
use crate::{SemanticErrorKind, WarningKind};

fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn count(source: &str, kind: WarningKind, issue_name: &str) -> usize {
    let result = analyze_en(source);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(super::super::error::SemanticError::is_error),
        "fixture must be error-free: {:?}",
        result.diagnostics
    );
    result
        .diagnostics
        .iter()
        .filter(|d| d.kind == SemanticErrorKind::Warning(kind) && issue(d) == Some(issue_name))
        .count()
}

fn unused_mut_params(source: &str) -> usize {
    count(
        source,
        WarningKind::UnusedMutRefParam,
        "unused_mut_parameter",
    )
}

fn unused_variables(source: &str) -> usize {
    count(source, WarningKind::UnusedVariable, "unused_variable")
}

#[test]
fn statement_fill_through_a_mut_param_is_a_write() {
    let source = r"fn zero(mut tf32[2, 3] t) {
    t.fill(4.0)
}
main {
}";
    assert_eq!(unused_mut_params(source), 0);
}

#[test]
fn unused_mut_tensor_param_still_warns() {
    let source = r"fn idle(mut tf32[2, 3] t) {
    print 1
}
main {
}";
    assert_eq!(unused_mut_params(source), 1);
}

#[test]
fn bound_fill_through_a_mut_param_is_not_a_write() {
    let source = r"fn fresh(mut tf32[2, 3] t) → tf32[2, 3] {
    let tf32[2, 3] filled ← t.fill(4.0)
    return filled
}
main {
}";
    assert_eq!(unused_mut_params(source), 1);
}

// A plain bind of a shareable value aliases its source (D-1), so the object a
// `mut` parameter names can be written through the alias. WARN008 must not
// call that `mut` unused: removing it turns the alias bind into SEM065
// (`const_shared_into_var`, the read-only-parameter source row).
const ALIAS_BOX: &str = "class Box {\n    mut int v = 0\n}\n";

#[test]
fn write_through_a_plain_bind_alias_is_a_mut_param_write() {
    // The probe: `mut Box b ← p` shares the parameter's object, so `b.v ← 2`
    // writes it even though the name `p` is never a write target.
    let source = format!(
        "{ALIAS_BOX}fn poke(mut Box p) {{\n    mut Box b ← p\n    b.v ← 2\n}}\nmain {{\n    mut Box x ← Box {{ v = 1 }}\n    poke(mut x)\n    print x.v\n}}"
    );
    assert_eq!(unused_mut_params(&source), 0);
}

#[test]
fn mut_param_shared_into_a_varia_cell_is_not_unused() {
    // Even without a write, the alias bind needs the parameter's `mut`:
    // `let` on `p` makes `mut Box b ← p` SEM065.
    let source = format!(
        "{ALIAS_BOX}fn poke(mut Box p) {{\n    mut Box b ← p\n    print b.v\n}}\nmain {{\n    mut Box x ← Box {{ v = 1 }}\n    poke(mut x)\n    print x.v\n}}"
    );
    assert_eq!(unused_mut_params(&source), 0);
}

#[test]
fn mut_param_without_alias_or_write_still_warns() {
    // Negative twin: the parameter is only read and never aliased, so its
    // `mut` really is unnecessary.
    let source = format!(
        "{ALIAS_BOX}fn peek(mut Box p) {{\n    print p.v\n}}\nmain {{\n    mut Box x ← Box {{ v = 1 }}\n    peek(mut x)\n    print x.v\n}}"
    );
    assert_eq!(unused_mut_params(&source), 1);
}

#[test]
fn scalar_named_in_a_tensor_extent_is_used() {
    let source = r"fn runtime_size(int value) → int {
    return value
}
main {
    let list<f32> xs ← [1.0, 2.0, 3.0]
    let int n ← runtime_size(4)
    let tensor<f32, [size (n - 1)]> dynamic ← xs ↦ tensor<f32, [size (n - 1)]>
    print dynamic.length()
}";
    assert_eq!(unused_variables(source), 0);
}

#[test]
fn scalar_named_in_a_tf32_extent_is_used() {
    let source = r"fn runtime_extent(int value) → int {
    return value
}
main {
    let int n ← runtime_extent(3)
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    let tf32[2, size n] a ← flat ↦ tf32[2, size n]
    print a.length()
}";
    assert_eq!(unused_variables(source), 0);
}

#[test]
fn genuinely_unused_scalar_still_warns() {
    let source = r"fn runtime_size(int value) → int {
    return value
}
main {
    let list<f32> xs ← [1.0, 2.0, 3.0]
    let int n ← runtime_size(4)
    let int dead ← runtime_size(2)
    let tensor<f32, [size (n - 1)]> dynamic ← xs ↦ tensor<f32, [size (n - 1)]>
    print dynamic.length()
}";
    assert_eq!(unused_variables(source), 1);
}
