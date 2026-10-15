//! Size-type Z3 law 3 (Q3): `_` extent parameters are anonymous size
//! parameters chosen per call — one call's witness never binds another's,
//! and at entry the hole falls through to the carrier's runtime axis.
//!
//! Law text: `docs/factory/size-type/goal.md`, consequences row "`_` extent
//! parameters are anonymous size params chosen per call" and Z3 done-when 1
//! (the g06/c1 first-call-binding defect is gone).

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

#[test]
fn two_calls_with_different_shapes_both_check() {
    // The c1 probe: pre-Z3 the first call bound the shared inference hole
    // program-wide and the second call was incompatible_tensor_index. The
    // call-boundary argument check alone exercises the per-call binding.
    let source = r#"fn rows(tensor<f32, [_, _]> t) → int { return 1 }

main {
    let tf32[2, 3] a ← tf32[2, 3] {}
    let tf32[4, 5] b ← tf32[4, 5] {}
    print rows(a)
    print rows(b)
}
"#;
    let result = analyze(source);
    assert!(
        result.is_ok(),
        "each call binds its own anonymous sizes: {:?}",
        result.err()
    );
}

#[test]
fn anonymous_and_literal_axes_stay_distinct_within_one_call() {
    // A declared extent stays checked at the call while the `_` axes bind
    // per call from their own argument.
    let source = r#"fn pick(tensor<f32, [_, 3]> t) → int { return 1 }

main {
    let tf32[2, 3] a ← tf32[2, 3] {}
    let tf32[4, 3] b ← tf32[4, 3] {}
    print pick(a)
    print pick(b)
}
"#;
    let result = analyze(source);
    assert!(result.is_ok(), "{:?}", result.err());

    let mismatched = analyze(
        "fn pick(tensor<f32, [_, 3]> t) → int { return 1 }\nmain {\n    let tf32[2, 4] bad ← tf32[2, 4] {}\n    print pick(bad)\n}\n",
    );
    assert!(
        mismatched.is_err(),
        "a declared extent still rejects its mismatch"
    );
    assert!(
        mismatched
            .result
            .diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("incompatible_tensor_index")),
        "{:?}",
        mismatched.err()
    );
}

#[test]
fn underscore_kernel_parameters_fall_through_to_the_carrier() {
    // g06: `_` extents in a kernel signature with an `at [i, j]` loop — the
    // loop reads the launch carrier instead of failing cannot_infer.
    let source = r#"@ kernel
@ public
fn k(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane let c {
        dst[[i, j]] ← c
    }
}
main {}
"#;
    let result = analyze(source);
    assert!(
        result.is_ok(),
        "the `_` hole falls through to runtime at entry: {:?}",
        result.err()
    );
}

#[test]
fn reduce_over_underscore_parameter_checks() {
    // Z3 done-when 2: a reduce body over a `_` parameter checks.
    let source = r#"@ kernel
@ public
fn fold(tensor<f32, [_, _]> a, mut tensor<f32, [1]> out) → void {
    out[[0]] ← reduce via max from a at [i, j] thread lane let c { return c } coalesce 0.0
}
main {}
"#;
    let result = analyze(source);
    assert!(result.is_ok(), "{:?}", result.err());
}
