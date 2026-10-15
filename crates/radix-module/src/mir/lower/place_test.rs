//! A multi-element device tensor index lowers to one row-major linear index.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};
use crate::mir::{MirLoweringOptions, dump_analyzed_unit, lower_analyzed_unit, validate_program};

fn dump_validated(source: &str) -> String {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let unit = crate::driver::analyze_source(&session, "place.fab", source, AnalyzeRequest::default())
        .expect("analysis succeeds");
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the linearized device index must validate");
    dump_analyzed_unit(unit.application_view()).expect("MIR dump succeeds")
}

#[test]
fn device_rank2_bracket_index_lowers_to_one_linear_index() {
    let dump = dump_validated(
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn pick(tf32[4, 6] input, mut tf32[4, 6] output, u32 y, u32 x) → void {
    output[[y, x]] ← input[[y, x]]
}
"#,
    );
    assert!(dump.contains("%0 = _2 * const int 6"), "missing row stride multiply: {dump}");
    assert!(dump.contains("%1 = %0 + _3"), "missing column add: {dump}");
    assert!(dump.contains("_1[%1] = _0[%3]"), "access must use one linear index: {dump}");
    assert!(!dump.contains("runtime collection tensor_get"), "{dump}");
}

/// A rank-1 `v[[i]]` is the integer index `i` itself: the device-form MIR is
/// identical to the scalar spelling `v[i]` (same blocks), and it validates (the index is not
/// a one-element list).
#[test]
fn device_rank1_bracket_index_lowers_like_the_scalar_index() {
    let kernel = |read: &str, write: &str| {
        format!(
            r#"+++
locale = "en"
+++
@ kernel
@ public
fn pick(tf32[8] input, mut tf32[8] output, u32 x) → void {{
    output{write} ← input{read}
}}
"#
        )
    };
    let listed = dump_validated(&kernel("[[x]]", "[[x]]"));
    let scalar = dump_validated(&kernel("[x]", "[x]"));
    // Type ids differ (the list spelling interns an extra type); the blocks must not.
    let blocks = |dump: &str| dump.split("bb0:").nth(1).map(str::to_owned);
    assert_eq!(
        blocks(&listed),
        blocks(&scalar),
        "rank-1 list and scalar index must lower identically"
    );
    assert!(listed.contains("_1[_2] = _0[_2]"), "access must index by the integer: {listed}");
    assert!(!listed.contains("runtime collection tensor_get"), "{listed}");
}

#[test]
fn device_rank3_bracket_index_lowers_to_one_linear_index() {
    let dump = dump_validated(
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn pick(tf32[2, 3, 5] input, mut tf32[2, 3, 5] output, u32 z, u32 y, u32 x) → void {
    output[[z, y, x]] ← input[[z, y, x]]
}
"#,
    );
    assert!(dump.contains("%0 = _2 * const int 3"), "missing axis-1 stride: {dump}");
    assert!(dump.contains("%1 = %0 + _3"), "missing axis-1 add: {dump}");
    assert!(dump.contains("%2 = %1 * const int 5"), "missing axis-2 stride: {dump}");
    assert!(dump.contains("%3 = %2 + _4"), "missing axis-2 add: {dump}");
}

#[test]
fn device_rank2_runtime_extent_index_reads_tensor_extent() {
    let dump = dump_validated(
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn pick(tf32[_, _] input, mut tf32[_, _] output, u32 y, u32 x) → void {
    output[[y, x]] ← input[[y, x]]
}
"#,
    );
    assert!(dump.contains("%0 = extent _1 axis 1"), "missing output axis-1 extent: {dump}");
    assert!(dump.contains("%1 = _2 * %0"), "missing row stride multiply: {dump}");
    assert!(dump.contains("%2 = %1 + _3"), "missing column add: {dump}");
    assert!(dump.contains("%3 = extent _0 axis 1"), "missing input axis-1 extent: {dump}");
    assert!(!dump.contains("axis 0"), "the leading axis extent is never read: {dump}");
    assert!(!dump.contains("run-time extents"), "{dump}");
}

#[test]
fn device_rank3_mixed_extent_index_reads_runtime_axes_only() {
    let dump = dump_validated(
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn pick(tf32[2, _, 5] input, mut tf32[2, _, 5] output, u32 z, u32 y, u32 x) → void {
    output[[z, y, x]] ← input[[z, y, x]]
}
"#,
    );
    assert!(dump.contains("extent _1 axis 1"), "missing runtime axis-1 extent: {dump}");
    assert!(
        !dump.contains("axis 2") && !dump.contains("axis 0"),
        "literal axes must not read an extent: {dump}"
    );
    assert!(dump.contains("* const int 5"), "literal axis-2 stride: {dump}");
}

/// Size-type rule 11: a `_` extent is a wildcard bound at run time, so a call
/// passing tensors of the same rank validates whether the argument extent is
/// a literal or a run-time value (it is not instantiated per concrete size).
#[test]
fn underscore_kernel_call_accepts_literal_and_runtime_extents() {
    let _ = dump_validated(
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn bump(tf32[_] input, mut tf32[_] output) → void {
    for from input at [i] thread lane let x {
        output[i] ← x + 1.0
    }
}

fn runtime_extent(int value) → int {
    return value
}

main {
    let int n ← runtime_extent(3)
    let tf32[4] fixed ← tf32[4] {}
    mut tf32[4] fixed_out ← tf32[4] {}
    let tf32[size n] sized ← tf32[size n] {}
    mut tf32[size n] sized_out ← tf32[size n] {}
    bump(fixed, mut fixed_out)
    bump(sized, mut sized_out)
}
"#,
    );
}
