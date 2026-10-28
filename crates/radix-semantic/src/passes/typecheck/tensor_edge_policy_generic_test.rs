//! Generic forwarding and concrete discharge for `limes<P>` obligations.

use crate::ResolvedTensorEdgePolicy;
use radix_types::BuiltinTensorEdgePolicy;

fn analyze(source: &str) -> crate::test_support::AnalyzedFixture {
    let pack =
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("English locale pack");
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    crate::test_support::analyze_source_with_config(source, config)
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = analyze(source);
    assert!(
        !result.is_ok(),
        "expected `{issue}` failure, got {:?}",
        result.diagnostics
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
    "let tf32[3] base ← [1.0, 2.0, 3.0] ↦ tf32[3]"
}

#[test]
fn one_hop_forwarding_discharges_builtin_policy() {
    let result = analyze(&format!(
        "fn forward<P>(tf32[3] base) → tf32[3] {{\n    return base.shift([1]).edge<P>().materialize()\n}}\nmain {{\n{}\nlet tf32[3] mapped ← forward<wrap>(base)\n}}",
        tensor_seed()
    ));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);

    let policies = &result.tensor_edge_policies;
    assert_eq!(policies.function_requirements.len(), 1);
    assert!(policies.provider_impls.iter().any(|provider| {
        provider.policy == ResolvedTensorEdgePolicy::Builtin(BuiltinTensorEdgePolicy::Wrap)
            && provider.rank == 1
    }));
}

#[test]
fn two_hop_forwarding_propagates_and_discharges_the_same_obligation() {
    let result = analyze(&format!(
        "fn inner<P>(tf32[3] base) → tf32[3] {{\n    return base.shift([1]).edge<P>().materialize()\n}}\nfn middle<Q>(tf32[3] base) → tf32[3] {{\n    return inner<Q>(base)\n}}\nfn outer<R>(tf32[3] base) → tf32[3] {{\n    return middle<R>(base)\n}}\nmain {{\n{}\nlet tf32[3] mapped ← outer<reflect>(base)\n}}",
        tensor_seed()
    ));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);

    let policies = &result.tensor_edge_policies;
    assert_eq!(policies.function_requirements.len(), 3);
    assert!(policies.provider_impls.iter().any(|provider| {
        provider.policy == ResolvedTensorEdgePolicy::Builtin(BuiltinTensorEdgePolicy::Reflect)
            && provider.rank == 1
    }));
}

#[test]
fn generic_method_forwarding_keeps_the_method_definition_identity() {
    let result = analyze(&format!(
        "class Router {{\n    fn forward<P>(tf32[3] base) → tf32[3] {{\n        return base.shift([1]).edge<P>().materialize()\n    }}\n}}\nmain {{\n{}\nlet Router router ← Router {{}}\nlet tf32[3] mapped ← router.forward<wrap>(base)\n}}",
        tensor_seed()
    ));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    assert_eq!(result.tensor_edge_policies.function_requirements.len(), 1);
    assert!(
        result
            .tensor_edge_policies
            .provider_impls
            .iter()
            .any(|provider| {
                provider.policy == ResolvedTensorEdgePolicy::Builtin(BuiltinTensorEdgePolicy::Wrap)
                    && provider.rank == 1
            })
    );
}

#[test]
fn forwarded_custom_policy_retains_checked_callback_identities() {
    let result = analyze(&format!(
        "class ZeroGhost {{\n    fn remap(vector<int, size 1> coord) → vector<int, size 1> {{ return coord }}\n    fn transform(vector<int, size 1> coord, f32 value) → f32 {{ return value }}\n}}\nfn forward<P>(tf32[3] base) → tf32[3] {{\n    return base.shift([1]).edge<P>().materialize()\n}}\nmain {{\n{}\nlet tf32[3] mapped ← forward<ZeroGhost>(base)\n}}",
        tensor_seed()
    ));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);

    assert!(
        result
            .tensor_edge_policies
            .provider_impls
            .iter()
            .any(|provider| {
                matches!(
                    provider.policy,
                    ResolvedTensorEdgePolicy::Custom {
                        remap: _,
                        transform: Some(_),
                        ..
                    }
                ) && provider.rank == 1
            })
    );
}

#[test]
fn forwarded_policy_rejects_a_concrete_class_with_an_invalid_callback() {
    rejects_with_issue(
        &format!(
            "class InvalidPolicy {{\n    fn remap(vector<int, size 1> coord) → int {{ return 0 }}\n}}\nfn forward<P>(tf32[3] base) → tf32[3] {{\n    return base.shift([1]).edge<P>()\n}}\nmain {{\n{}\nlet tf32[3] mapped ← forward<InvalidPolicy>(base)\n}}",
            tensor_seed()
        ),
        "tensor_limes_policy_invalid",
    );
}

#[test]
fn forwarded_policy_rejects_a_class_with_runtime_state() {
    rejects_with_issue(
        &format!(
            "class StatefulPolicy {{\n    mut int state\n    fn remap(vector<int, size 1> coord) → vector<int, size 1> {{ return coord }}\n}}\nfn forward<P>(tf32[3] base) → tf32[3] {{\n    return base.shift([1]).edge<P>()\n}}\nmain {{\n{}\nlet tf32[3] mapped ← forward<StatefulPolicy>(base)\n}}",
            tensor_seed()
        ),
        "tensor_limes_policy_invalid",
    );
}

#[test]
fn forwarded_builtin_marker_cannot_bind_a_runtime_parameter() {
    rejects_with_issue(
        &format!(
            "fn stored<P>(P value, tf32[3] base) → tf32[3] {{\n    return base.shift([1]).edge<P>()\n}}\nmain {{\n{}\nlet tf32[3] mapped ← stored<wrap>(base, base)\n}}",
            tensor_seed()
        ),
        "tensor_edge_policy_marker_not_runtime_value",
    );
}

#[test]
fn custom_policy_parameter_cannot_carry_a_runtime_value() {
    rejects_with_issue(
        &format!(
            "class ZeroGhost {{\n    fn remap(vector<int, size 1> coord) → vector<int, size 1> {{ return coord }}\n}}\nfn stored<P>(P value, tf32[3] base) → tf32[3] {{\n    return base.shift([1]).edge<P>()\n}}\nmain {{\n{}\nlet tf32[3] mapped ← stored<ZeroGhost>(ZeroGhost {{}}, base)\n}}",
            tensor_seed()
        ),
        "tensor_limes_policy_invalid",
    );
}

#[test]
fn policy_type_names_do_not_reserve_value_function_names() {
    let result = analyze(&format!(
        "fn wrap(list<int> values) → list<int> ∪ none {{ return values }}\nmain {{\n{}\nlet list<int> ∪ none values ← wrap([1, 2])\nprint values!.length()\nlet tf32[3] mapped ← base.shift([1]).edge<wrap>()\n}}",
        tensor_seed()
    ));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    assert!(
        result
            .tensor_edge_policies
            .provider_impls
            .iter()
            .any(|provider| {
                provider.policy == ResolvedTensorEdgePolicy::Builtin(BuiltinTensorEdgePolicy::Wrap)
            })
    );
}
