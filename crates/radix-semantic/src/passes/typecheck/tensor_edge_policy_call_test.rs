//! Shifted tensor optionality and limes policy admission.

fn analyze_english(source: &str) -> crate::test_support::AnalyzedFixture {
    let pack =
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("English locale pack");
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    crate::test_support::analyze_source_with_config(source, config)
}

fn accepts(source: &str) {
    let result = analyze_english(source);
    assert!(
        result.is_ok(),
        "expected source to typecheck: {:?}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = analyze_english(source);
    assert!(!result.is_ok(), "expected source to fail typechecking");
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
    "let tf32[3] base ← [1.0, 2.0, 3.0] ↦ tf32[3]"
}

#[test]
fn shifted_view_into_a_plain_tensor_initializer_is_rejected() {
    rejects_with_issue(
        &format!(
            "main {{\n{}\nlet tf32[3] s ← base.shift([1])\n }}",
            tensor_seed()
        ),
        "tensor_edge_view_requires_resolution",
    );
}

#[test]
fn shifted_view_into_an_optional_element_slot_stays_pending() {
    accepts(&format!(
        "main {{\n{}\nlet tensor<f32 ∪ none, [3]> s ← base.shift([1])\nlet tf32[3] w ← s.edge<wrap>()\nprint w.flatten()\n}}",
        tensor_seed()
    ));
}

#[test]
fn shifted_optional_view_resolves_after_a_local_binding_or_coalesce() {
    accepts(&format!(
        "main {{\n{}\nlet _ pending ← base.shift([1])\nlet tf32[3] wrapped ← pending.edge<wrap>()\nlet tf32[3] filled ← base.shift([1]) coalesce 0.0\nprint wrapped.flatten()\nprint filled.flatten()\n}}",
        tensor_seed()
    ));
}

#[test]
fn shifted_optional_view_cannot_be_re_shifted_or_flattened_before_resolution() {
    rejects_with_issue(
        &format!(
            "main {{\n{}\nlet tf32[3] twice ← base.shift([1]).shift([1])\n}}",
            tensor_seed()
        ),
        "tensor_shift_nullable_source_unsupported",
    );
    rejects_with_issue(
        &format!(
            "main {{\n{}\nlet _ pending ← base.shift([1])\nlet _ flat ← pending.flatten()\n}}",
            tensor_seed()
        ),
        "tensor_edge_view_requires_resolution",
    );
}

#[test]
fn limes_requires_a_shifted_view() {
    rejects_with_issue(
        &format!(
            "main {{\n{}\nlet tf32[3] invalid ← base.edge<clamp>()\n}}",
            tensor_seed()
        ),
        "tensor_limes_requires_shifted_view",
    );
}

#[test]
fn custom_limes_policy_requires_plain_instance_callbacks() {
    accepts(&format!(
        "interface EdgePolicy<C> {{ fn remap(C coord) → C }}\nclass ZeroGhost implements EdgePolicy<vector<int, size 1>> {{ fn remap(vector<int, size 1> coord) → vector<int, size 1> {{ return [0] ↦ vector<int, size 1> }} }}\nmain {{\n{}\nlet tf32[3] mapped ← base.shift([1]).edge<ZeroGhost>()\n}}",
        tensor_seed()
    ));

    rejects_with_issue(
        &format!(
            "class InvalidPolicy {{ fn remap(vector<int, size 1> coord) → int {{ return 0 }} }}\nmain {{\n{}\nlet tf32[3] invalid ← base.shift([1]).edge<InvalidPolicy>()\n}}",
            tensor_seed()
        ),
        "tensor_limes_policy_invalid",
    );
}
