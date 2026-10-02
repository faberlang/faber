//! Shifted tensor optionality and limes policy admission.

fn accepts(source: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        result.is_ok(),
        "expected source to typecheck: {:?}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected source to fail typechecking"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.is_error() && diagnostic.issue() == Some(issue)),
        "expected `{issue}`, got {:?}",
        result.diagnostics
    );
}

fn tensor_seed() -> &'static str {
    "const tf32[] seed ← empty\nconst tf32[3] base ← seed.from_flat([1.0, 2.0, 3.0], [3])"
}

#[test]
fn shifted_optional_view_resolves_after_a_local_binding_or_coalesce() {
    accepts(&format!(
        "main {{\n{}\nconst _ pending ← base.shift([1])\nconst tf32[3] wrapped ← pending.limes<wrap>()\nconst tf32[3] filled ← base.shift([1]) coalesce 0.0\nprint wrapped.flatten()\nprint filled.flatten()\n}}",
        tensor_seed()
    ));
}

#[test]
fn shifted_optional_view_cannot_be_re_shifted_or_flattened_before_resolution() {
    rejects_with_issue(
        &format!(
            "main {{\n{}\nconst tf32[3] twice ← base.shift([1]).shift([1])\n}}",
            tensor_seed()
        ),
        "tensor_shift_nullable_source_unsupported",
    );
    rejects_with_issue(
        &format!(
            "main {{\n{}\nconst _ pending ← base.shift([1])\nconst _ flat ← pending.flatten()\n}}",
            tensor_seed()
        ),
        "tensor_edge_view_requires_resolution",
    );
}

#[test]
fn limes_requires_a_shifted_view() {
    rejects_with_issue(
        &format!(
            "main {{\n{}\nconst tf32[3] invalid ← base.limes<clamp>()\n}}",
            tensor_seed()
        ),
        "tensor_limes_requires_shifted_view",
    );
}

#[test]
fn custom_limes_policy_requires_plain_instance_callbacks() {
    accepts(&format!(
        "interface EdgePolicy<C> {{ fn remap(C coord) → C }}\nclass ZeroGhost implements EdgePolicy<vector<int, 1>> {{ fn remap(vector<int, 1> coord) → vector<int, 1> {{ return [0] ↦ vector<int, 1> }} }}\nmain {{\n{}\nconst tf32[3] mapped ← base.shift([1]).limes<ZeroGhost>()\n}}",
        tensor_seed()
    ));

    rejects_with_issue(
        &format!(
            "class InvalidPolicy {{ fn remap(vector<int, 1> coord) → numerus {{ return 0 }} }}\nmain {{\n{}\nconst tf32[3] invalid ← base.shift([1]).limes<InvalidPolicy>()\n}}",
            tensor_seed()
        ),
        "tensor_limes_policy_invalid",
    );
}
