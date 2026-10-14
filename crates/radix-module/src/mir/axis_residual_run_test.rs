//! Need, residual axis reduction: `mean` over an integer-element
//! tensor returns `fractus` like the scalar `.mean()`. The reduction sums
//! exactly in the element family and converts the summed tensor to `fractus`
//! (the ordinary tensor element `↦`) before dividing by the literal extent.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session, compile};
use crate::mir::{BufferHost, run_source};

/// `grid = [[1, 2, 3], [4, 5, 6]]` (shape [2, 3]); the caller appends the
/// statements under test and the closing brace.
const GRID: &str = r#"+++
locale = "en"
+++
main {
    const list<i32> flat ← [1, 2, 3, 4, 5, 6]
    const ti32[2, 3] grid ← flat ↦ ti32[2, 3]
"#;

fn ok(body: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("{GRID}{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "axis.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(other) => panic!("{body}: {other:?}"),
    }
}

/// The `tf32` grid of `corpus/tensor/axis-builtins.fab`, without the
/// enclosing `main`.
const TF32_GRID: &str = r#"    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    const tf32[2, 3] grid ← flat ↦ tf32[2, 3]
"#;

/// Run the exemplar `tf32` grid with `module` before `main` and `body` inside
/// it.
fn ok_tf32(module: &str, body: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("+++\nlocale = \"en\"\n+++\n{module}main {{\n{TF32_GRID}{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "axis.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(other) => panic!("{body}: {other:?}"),
    }
}

/// Compile the exemplar `tf32` grid and list its diagnostic issues.
fn issues_tf32(body: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("+++\nlocale = \"en\"\n+++\nmain {{\n{TF32_GRID}{body}\n}}\n");
    let result = compile(&session, "axis.fab", &source, ModuleCompileRequest::default());
    result
        .diagnostics
        .iter()
        .filter_map(|diag| diag.issue().map(str::to_owned))
        .collect()
}

fn issues(body: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("{GRID}{body}\n}}\n");
    let result = compile(&session, "axis.fab", &source, ModuleCompileRequest::default());
    result
        .diagnostics
        .iter()
        .filter_map(|diag| diag.issue().map(str::to_owned))
        .collect()
}

#[test]
fn integer_axis_mean_over_rows_prints_float_means() {
    assert_eq!(
        ok("    const tf64[2] means ← grid.mean(axis 1)\n    print means[0]\n    print means[1]"),
        ["2.0", "5.0"]
    );
}

#[test]
fn integer_axis_mean_over_columns_prints_float_means() {
    assert_eq!(
        ok("    const tf64[3] means ← grid.mean(axis 0)\n    print means[0]\n    print means[1]\n    print means[2]"),
        ["2.5", "3.5", "4.5"]
    );
}

#[test]
fn integer_axis_mean_accepts_the_tf64_result_and_refuses_ti32() {
    assert_eq!(ok("    const tf64[2] means ← grid.mean(axis 1)\n    print means[0]"), ["2.0"]);
    let rejected = issues("    const ti32[2] means ← grid.mean(axis 1)");
    assert!(
        rejected
            .iter()
            .any(|issue| issue == "initializer_annotation_mismatch"),
        "an integer result slot must fail the ordinary store mismatch: {rejected:?}"
    );
    assert!(
        !rejected
            .iter()
            .any(|issue| issue == "axis_reduction_mean_requires_float"),
        "integer mean must not raise the non-float element refusal: {rejected:?}"
    );
}

// Need, residual item 2: the axis may be a named compile-time
// constant (`const int K ← 1`), a literal, or a compile-time parameter; a
// run-time variable stays the named refusal.

#[test]
fn named_const_axis_matches_the_literal_axis() {
    let named = ok_tf32(
        "",
        "    const int K ← 1\n    const tf32[2] rows ← grid.sum(axis K)\n    print rows[0]\n    print rows[1]",
    );
    let literal = ok_tf32(
        "",
        "    const tf32[2] rows ← grid.sum(axis 1)\n    print rows[0]\n    print rows[1]",
    );
    assert_eq!(named, ["6.0", "15.0"]);
    assert_eq!(named, literal);
}

#[test]
fn mutable_named_axis_stays_not_constant() {
    let rejected =
        issues_tf32("    var int k ← 1\n    const tf32[2] rows ← grid.sum(axis k)");
    assert!(
        rejected
            .iter()
            .any(|issue| issue == "axis_reduction_not_constant"),
        "a `var` axis must be the named not-constant refusal: {rejected:?}"
    );
}

#[test]
fn module_level_const_axis_is_admitted() {
    assert_eq!(
        ok_tf32(
            "const int K = 1\n",
            "    const tf32[2] rows ← grid.sum(axis K)\n    print rows[0]\n    print rows[1]"
        ),
        ["6.0", "15.0"]
    );
}

#[test]
fn named_const_axis_out_of_range_is_refused() {
    let rejected = issues_tf32("    const int K ← 5\n    const tf32[3] rows ← grid.sum(axis K)");
    assert!(
        rejected
            .iter()
            .any(|issue| issue == "axis_reduction_out_of_range"),
        "a named axis outside the rank must be the named out-of-range refusal: {rejected:?}"
    );
}
