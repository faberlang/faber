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
fn pick(tf32[4, 6] input, mut tf32[4, 6] output) → void {
    for from input at [y, x] thread lane let value {
        output[[y, x]] ← value
    }
}
"#,
    );
    assert!(dump.contains("%4 = _3 * const int 6"), "missing row stride multiply: {dump}");
    assert!(dump.contains("%5 = %4 + _4"), "missing column add: {dump}");
    assert!(dump.contains("_1[%5] = _5"), "access must use one linear index: {dump}");
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
fn pick(tf32[8] input, mut tf32[8] output) → void {{
    for from input at [x] thread lane let value {{
        output{write} ← input{read}
    }}
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
    assert!(listed.contains("_1[_3] = _0[_3]"), "access must index by the integer: {listed}");
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
fn pick(tf32[2, 3, 5] input, mut tf32[2, 3, 5] output) → void {
    for from input at [z, y, x] thread lane let value {
        output[[z, y, x]] ← value
    }
}
"#,
    );
    assert!(dump.contains("%7 = _3 * const int 3"), "missing axis-1 stride: {dump}");
    assert!(dump.contains("%8 = %7 + _4"), "missing axis-1 add: {dump}");
    assert!(dump.contains("%9 = %8 * const int 5"), "missing axis-2 stride: {dump}");
    assert!(dump.contains("%10 = %9 + _5"), "missing axis-2 add: {dump}");
    assert!(dump.contains("_1[%10] = _6"), "access must use one linear index: {dump}");
}

#[test]
fn device_rank2_runtime_extent_index_reads_tensor_extent() {
    // Kernel-subset ruling (2026-10-15, need; need): a
    // dynamic extent cannot prove this raw lane index's bounds, so check rejects it.
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let diagnostics = match crate::driver::analyze_source(
        &session,
        "place.fab",
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn pick(tf32[_, _] input, mut tf32[_, _] output, u32 y, u32 x) → void {
    output[[y, x]] ← input[[y, x]]
}
"#,
        AnalyzeRequest::default(),
    ) {
        Err(diagnostics) => diagnostics,
        Ok(_) => panic!("runtime tensor extents must not admit an unproven lane index"),
    };
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("nucleum_extent_unproven"))
        .expect("runtime lane index reports the named extent check error");
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "extent" && arg.value == "unknown"),
        "the check error must name the unproven extent: {diagnostic:?}"
    );
}

#[test]
fn device_rank3_mixed_extent_index_reads_runtime_axes_only() {
    let dump = dump_validated(
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn pick(tf32[2, _, 5] input, mut tf32[2, _, 5] output) → void {
    for from input at [z, y, x] thread lane let value {
        output[[z, y, x]] ← value
    }
}
"#,
    );
    assert!(
        dump.contains("extent _1 axis 1"),
        "missing output runtime axis-1 extent: {dump}"
    );
    assert!(dump.contains("extent _0 axis 1"), "missing input runtime axis-1 extent: {dump}");
    assert!(dump.contains("%13 = %12 * const int 5"), "literal axis-2 stride: {dump}");
    assert!(dump.contains("_1[%14] = _6"), "mixed-shape store uses one linear index: {dump}");
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
