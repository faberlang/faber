//! Size-type Z3 law 2: a bare value name in a local annotation extent is the
//! invisible int→size crossing and is refused with a did-you-mean; the
//! spelled `size` lift, declared size names, and the whole-shape list
//! reading stay admissible.
//!
//! Law text: `docs/factory/size-type/goal.md`, the same-page-test amendment
//! (Proposal row "local annotation extent") and Z3 done-when 4.

use crate::locale::LocalePack;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config};
use radix_target::Target;

fn analyze(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("installed English locale pack"),
    );
    analyze_source_with_config(source, config)
}

/// Structured facts only: issue identity plus the did-you-mean argument.
fn issue_args(result: &AnalyzedFixture, issue: &str, arg: &str) -> Vec<String> {
    result
        .result
        .diagnostics
        .iter()
        .filter(|diag| diag.issue() == Some(issue))
        .filter_map(|diag| {
            diag.args
                .iter()
                .find(|candidate| candidate.name == arg)
                .map(|candidate| candidate.value.clone())
        })
        .collect()
}

#[test]
fn bare_value_name_annotation_extent_is_refused_with_did_you_mean() {
    let source = r#"fn runtime_extent(int value) → int { return value }

main {
    let int n ← runtime_extent(2)
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    let tf32[2, n] a ← flat ↦ tf32[2, size n]
}
"#;
    let result = analyze(source);
    assert!(result.is_err(), "the bare annotation crossing must refuse");
    assert_eq!(
        issue_args(&result, "annotation_extent_bare_value_name", "name"),
        vec!["n".to_owned()],
        "{:?}",
        result.err()
    );
}

#[test]
fn spelled_size_lift_annotation_stays_green() {
    let source = r#"fn runtime_extent(int value) → int { return value }

main {
    let int n ← runtime_extent(2)
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    let tf32[2, size n] a ← flat ↦ tf32[2, size n]
    print a.shape().get(1)
}
"#;
    let result = analyze(source);
    assert!(
        result.is_ok(),
        "the spelled lift is the same-page store: {:?}",
        result.err()
    );
}

#[test]
fn bare_value_param_name_annotation_extent_is_refused_with_did_you_mean() {
    // The value name lowers through the parameter path (ScalarParameter);
    // the refusal must cover it identically.
    let source = r#"fn probe(int n) → int {
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    let tf32[2, n] a ← flat ↦ tf32[2, size n]
    return 1
}
main {}
"#;
    let result = analyze(source);
    assert!(result.is_err());
    assert_eq!(
        issue_args(&result, "annotation_extent_bare_value_name", "name"),
        vec!["n".to_owned()],
        "{:?}",
        result.err()
    );
}

#[test]
fn bare_affine_extent_is_refused_but_parenthesized_lift_stays_green() {
    let refused = analyze(
        "fn runtime_size(int value) → int { return value }\nmain {\n    let int n ← runtime_size(4)\n    let list<f32> flat ← [1.0, 2.0, 3.0]\n    let tensor<f32, [n - 1]> dynamic ← flat ↦ tensor<f32, [size (n - 1)]>\n}\n",
    );
    assert!(refused.is_err(), "bare affine extent must refuse");
    assert_eq!(
        issue_args(&refused, "annotation_extent_bare_value_name", "name"),
        vec!["n".to_owned()],
        "{:?}",
        refused.err()
    );

    let lifted = analyze(
        "fn runtime_size(int value) → int { return value }\nmain {\n    let int n ← runtime_size(4)\n    let list<f32> flat ← [1.0, 2.0, 3.0]\n    let tensor<f32, [size (n - 1)]> dynamic ← flat ↦ tensor<f32, [size (n - 1)]>\n}\n",
    );
    assert!(
        lifted.is_ok(),
        "the parenthesized spelled lift is the store: {:?}",
        lifted.err()
    );
}

#[test]
fn declared_size_names_stay_admissible_in_annotations() {
    // Static fold (Z1) and the runtime lift declaration both stay green.
    let static_size =
        analyze("main {\n    const size N = 3\n    mut tf32[N, 2] values ← tf32[N, 2] {}\n}\n");
    assert!(
        static_size.is_ok(),
        "declared static size stays a size term: {:?}",
        static_size.err()
    );

    let runtime_size = analyze(
        "fn probe(int h) {\n    const size H ← h\n    let list<f32> zeros ← [0.0, 0.0]\n    do {\n        mut tf32[H + 1] values ← zeros ↦ tf32[H + 1]\n    }\n    catch err {\n    }\n}\nmain {}\n",
    );
    assert!(
        runtime_size.is_ok(),
        "declared runtime size stays a size term: {:?}",
        runtime_size.err()
    );
}

#[test]
fn whole_shape_list_binding_stays_admissible() {
    // The whole-figura value name is the shape-list reading (conversions
    // track), not a per-extent int→size crossing.
    let source = r#"main {
  let list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  let list<int> shape ← [2, 2]
  let tf32[2, 2] data ← flat ↦ tf32[shape]
}
"#;
    let result = analyze(source);
    assert!(
        result.is_ok(),
        "whole-shape value reading stays green: {:?}",
        result.err()
    );
}
