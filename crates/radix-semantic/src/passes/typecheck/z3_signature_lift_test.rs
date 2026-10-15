//! Size-type Z3 law 1: a value parameter's name in a signature param/return
//! figura is a remote binding (the evidence crosses through the value slot)
//! and is refused with a did-you-mean at the typecheck.
//!
//! Law text: `docs/factory/size-type/goal.md`, Proposal row "signature
//! param/return term" and Z3 done-when 3 (`fn f(int n, tf32[n] x)` and
//! `→ tf32[h, w]` over `int` parameters).

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
fn signature_param_value_name_extent_is_refused_with_did_you_mean() {
    let result = analyze("fn f(int n, tf32[n] x) → int { return 1 }\nmain {}\n");
    assert!(result.is_err(), "remote value binding must refuse");
    assert_eq!(
        issue_args(&result, "signature_extent_bare_value_name", "param"),
        vec!["n".to_owned()],
        "{:?}",
        result.err()
    );
}

#[test]
fn signature_return_value_name_extents_are_refused_with_did_you_mean() {
    // g14 as originally written: the return figura names the int parameters.
    let result = analyze("fn make(int h, int w) → tf32[h, w] { return tf32[2, 3] {} }\nmain {}\n");
    assert!(result.is_err(), "remote return binding must refuse");
    let names = issue_args(&result, "signature_extent_bare_value_name", "param");
    assert!(
        names.contains(&"h".to_owned()) && names.contains(&"w".to_owned()),
        "did-you-mean must name each value parameter: {names:?}"
    );
}

#[test]
fn value_param_name_in_param_and_return_is_reported_per_figura() {
    let result = analyze("fn f(int n, tf32[n] x) → tf32[n] { return x }\nmain {}\n");
    assert!(result.is_err());
    // One refusal per figura position (the parameter term and the return
    // term), each naming the value parameter.
    assert_eq!(
        issue_args(&result, "signature_extent_bare_value_name", "param"),
        vec!["n".to_owned(), "n".to_owned()],
        "{:?}",
        result.err()
    );
}

#[test]
fn size_typed_generic_slot_stays_admissible() {
    // The did-you-mean's target spelling: `<size H>` is the size-typed
    // generic slot, and `H` reads as the extent term.
    let result = analyze("fn width<size H, size W>(tf32[H, W] t) → int { return H }\nmain {}\n");
    assert!(
        result.is_ok(),
        "size-typed generic slot must stay green: {:?}",
        result.err()
    );
}

#[test]
fn literal_extent_signature_stays_admissible() {
    let result = analyze("fn f(tf32[2, 3] x) → int { return 1 }\nmain {}\n");
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn kernel_launch_boundary_shapes_stay_admissible() {
    // g10's form: size-typed generics over tensor parameters at a kernel.
    let source = r#"@ kernel
fn mx<size H, size W>(tensor<f32, [H, W]> a, mut tensor<f32, [1]> out) → void {
    out[[0]] ← reduce via max from a at [i, j] thread lane let c { return c } coalesce 0.0
}
main {}
"#;
    let result = analyze(source);
    assert!(result.is_ok(), "{:?}", result.err());
}
