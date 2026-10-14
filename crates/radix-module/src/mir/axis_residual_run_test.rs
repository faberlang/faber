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
