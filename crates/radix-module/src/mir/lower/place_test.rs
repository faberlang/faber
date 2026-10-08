//! A multi-element device tensor index lowers to one row-major linear index.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};
use crate::mir::{MirLoweringOptions, dump_analyzed_unit, lower_analyzed_unit, validate_program};

fn dump_validated(source: &str) -> String {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut unit = crate::driver::analyze_source(&session, "place.fab", source, AnalyzeRequest::default())
        .expect("analysis succeeds");
    let lowered = lower_analyzed_unit(&mut unit, MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(&lowered.validated.program(), lowered.validated.validation())
        .expect("the linearized device index must validate");
    dump_analyzed_unit(&mut unit).expect("MIR dump succeeds")
}

#[test]
fn device_rank2_bracket_index_lowers_to_one_linear_index() {
    let dump = dump_validated(
        r"
@ nucleum
functio pick(tf32[4, 6] input, tf32[4, 6] output, u32 y, u32 x) → vacuum {
    output[[y, x]] ← input[[y, x]]
}
",
    );
    assert!(dump.contains("%0 = _2 * const int 6"), "missing row stride multiply: {dump}");
    assert!(dump.contains("%1 = %0 + _3"), "missing column add: {dump}");
    assert!(dump.contains("_1[%1] = _0[%3]"), "access must use one linear index: {dump}");
    assert!(!dump.contains("runtime collection tensor_get"), "{dump}");
}

#[test]
fn device_rank3_bracket_index_lowers_to_one_linear_index() {
    let dump = dump_validated(
        r"
@ nucleum
functio pick(tf32[2, 3, 5] input, tf32[2, 3, 5] output, u32 z, u32 y, u32 x) → vacuum {
    output[[z, y, x]] ← input[[z, y, x]]
}
",
    );
    assert!(dump.contains("%0 = _2 * const int 3"), "missing axis-1 stride: {dump}");
    assert!(dump.contains("%1 = %0 + _3"), "missing axis-1 add: {dump}");
    assert!(dump.contains("%2 = %1 * const int 5"), "missing axis-2 stride: {dump}");
    assert!(dump.contains("%3 = %2 + _4"), "missing axis-2 add: {dump}");
}
