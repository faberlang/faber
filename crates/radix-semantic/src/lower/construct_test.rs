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

fn issues(source: &str) -> Vec<String> {
    let result = analyze_source_with_config(source, config());
    result
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

#[test]
fn construct_named_form_checks_and_synthesizes_create() {
    let result = analyze_source_with_config(
        "fn main() {\n    const size N ← 2\n    let tf32[2, N] a ← tf32[2, N] {}\n}\n",
        config(),
    );
    assert!(
        result.is_ok(),
        "expected green, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn construct_inline_spelled_lift_checks() {
    let result = analyze_source_with_config(
        "fn main() {\n    mut int n ← 3\n    let tf32[_, _] b ← tf32[2, size n] {}\n}\n",
        config(),
    );
    assert!(
        result.is_ok(),
        "expected green, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn construct_literal_mismatch_stays_red() {
    let found = issues("fn main() {\n    let tf32[3, 2] a ← tf32[2, 3] {}\n}\n");
    assert!(
        found
            .iter()
            .any(|issue| issue == "initializer_annotation_mismatch"),
        "expected initializer mismatch, got: {found:?}"
    );
}

#[test]
fn construct_negative_spelled_lift_rejects() {
    let found = issues("fn main() {\n    let tf32[2, size -1] b ← tf32[2, size -1] {}\n}\n");
    assert!(
        found.iter().any(|issue| issue == "size_lift_negative"),
        "expected size_lift_negative, got: {found:?}"
    );
}

#[test]
fn construct_extent_hole_is_refused() {
    let found = issues("fn main() {\n    let tf32[2, 3] a ← tf32[2, _] {}\n}\n");
    assert!(
        found
            .iter()
            .any(|issue| issue == "constructor_extent_requires_term"),
        "expected constructor_extent_requires_term, got: {found:?}"
    );
}

#[test]
fn construct_non_extent_type_is_refused_by_name() {
    // The parser claim never fires for scalar type keywords (no `[`), so
    // `int x ← int` keeps its existing named refusal (`name_not_value`); a
    // bare width marker without a shape is not a constructor.
    let found = issues("fn main() {\n    let int x ← int\n}\n");
    assert!(
        found.iter().any(|issue| issue == "name_not_value"),
        "expected name_not_value for scalar type in value position, got: {found:?}"
    );
}
