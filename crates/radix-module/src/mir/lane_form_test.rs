//! Lane-form lowering of a top-level `thread lane` loop (gpu-reset C4-0).

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::lower::lane_form::{LANE_FORM_POST_LOOP_ISSUE, LANE_FORM_SECOND_REGION_ISSUE};
use crate::mir::{KernelLowering, MirError, MirLoweringOptions, lower_analyzed_unit};
use radix_mir::MirParamMode;
use crate::program::analyze_prepared_program_name;
use radix_mir::dump::dump_program;

fn english(source: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{source}")
}

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "lane-form.fab",
        &english(source),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

/// The dump of the lowered program plus the modes of `f0`'s parameters.
fn lower(source: &str, kernel_lowering: KernelLowering) -> Result<(String, Vec<MirParamMode>), Vec<MirError>> {
    let unit = analyze(source);
    let options = MirLoweringOptions { kernel_lowering, ..Default::default() };
    lower_analyzed_unit(&unit, options).map(|lowered| {
        let program = lowered.validated.program();
        let modes = program.functions[0].params.iter().map(|param| param.mode).collect();
        (dump_program(program), modes)
    })
}

/// The block lines of `f0`, with the `: ty#N` suffixes dropped so the
/// snapshot does not depend on type-arena numbering.
fn blocks_of_f0(dump: &str) -> Vec<String> {
    let start = dump.find("function f0 ").expect("f0 in the dump");
    let function = &dump[start..];
    let function = &function[..function.find("\n}\n").map_or(function.len(), |end| end + 2)];
    let blocks = &function[function.find("  bb0:").expect("f0 has blocks")..];
    blocks
        .lines()
        .filter(|line| *line != "}")
        .map(|line| line.split(": ty#").next().unwrap_or(line).trim_end().to_owned())
        .collect()
}

const COPY: &str = r"
@ kernel
@ public
fn copy(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane const v {
        dst[[i, j]] ← v
    }
}
main {}
";

#[test]
fn copy_lowers_to_the_lane_form() {
    let (dump, modes) = lower(COPY, KernelLowering::Device).expect("device lowering validates");
    assert_eq!(
        modes,
        [MirParamMode::Owned, MirParamMode::MutRef, MirParamMode::Lane],
        "the lane id is the one trailing Lane builtin parameter"
    );
    let expected = [
        "  bb0:",
        "    %0 = extent _0 axis 0",
        "    %1 = extent _0 axis 1",
        "    %2 = %0 * %1",
        "    %3 = _2 >= %2",
        "    branch %3 bb1 bb2",
        "  bb1:",
        "    return",
        "  bb2:",
        "    %4 = _2 % %1",
        "    %5 = _2 / %1",
        "    _3 = %5",
        "    _4 = %4",
        "    _5 = _0[_2]",
        "    %6 = extent _1 axis 1",
        "    %7 = _3 * %6",
        "    %8 = %7 + _4",
        "    _1[%8] = _5",
        "    goto bb3",
        "  bb3:",
        "    return",
    ];
    assert_eq!(blocks_of_f0(&dump), expected, "{dump}");
    assert!(!dump.contains("tensor_get") && !dump.contains("tensor_shape"), "{dump}");
}

#[test]
fn host_form_of_the_same_kernel_is_unchanged() {
    let (dump, modes) = lower(COPY, KernelLowering::Host).expect("host lowering validates");
    assert_eq!(modes, [MirParamMode::Owned, MirParamMode::MutRef], "host form has no lane parameter");
    let expected = [
        "  bb0:",
        "    %0 = runtime collection tensor_shape(_0) -> ty#38",
        "    %1 = runtime collection index(%0, const int 0) -> ty#39",
        "    %2 = option unwrap_assume(%1)",
        "    %3 = runtime collection index(%0, const int 1) -> ty#39",
        "    %4 = option unwrap_assume(%3)",
        "    _3 = const int 0",
        "    goto bb2",
        "  bb1:",
        "    return",
        "  bb2:",
        "    %5 = _3 < %2",
        "    branch %5 bb3 bb5",
        "  bb3:",
        "    _4 = const int 0",
        "    goto bb6",
        "  bb4:",
        "    %12 = _3 + const int 1",
        "    _3 = %12",
        "    goto bb2",
        "  bb5:",
        "    goto bb1",
        "  bb6:",
        "    %6 = _4 < %4",
        "    branch %6 bb7 bb9",
        "  bb7:",
        "    %7 = construct array",
        "    %8 = runtime collection tensor_get(_0, %7) -> ty#41",
        "    %9 = option unwrap_assume(%8)",
        "    _2 = %9",
        "    %10 = construct array",
        "    runtime collection tensor_set(_1, %10, _2) -> ty#5",
        "    goto bb8",
        "  bb8:",
        "    %11 = _4 + const int 1",
        "    _4 = %11",
        "    goto bb6",
        "  bb9:",
        "    goto bb4",
    ];
    // The host snapshot keeps its `-> ty#N` call suffixes out of the
    // comparison too: only the op names and operands are pinned.
    let strip = |line: &str| line.split(" -> ty#").next().unwrap_or(line).to_owned();
    let actual: Vec<String> = blocks_of_f0(&dump).iter().map(|line| strip(line)).collect();
    let expected: Vec<String> = expected.iter().map(|line| strip(line)).collect();
    assert_eq!(actual, expected, "{dump}");
}

const TWO_REGIONS: &str = r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane const v {
        dst[[i, j]] ← v
    }
    for from src at [i, j] thread lane const v {
        dst[[i, j]] ← v + 1.0
    }
}
main {}
";

const POST_LOOP: &str = r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane const v {
        dst[[i, j]] ← v
    }
    dst[[0, 0]] ← 1.0
}
main {}
";

fn issues(source: &str) -> Vec<String> {
    lower(source, KernelLowering::Device)
        .expect_err("the kernel must fail closed")
        .into_iter()
        .map(|error| error.issue)
        .collect()
}

#[test]
fn a_second_lane_region_fails_closed_with_a_named_issue() {
    assert_eq!(issues(TWO_REGIONS), [LANE_FORM_SECOND_REGION_ISSUE]);
}

#[test]
fn a_statement_after_the_lane_region_fails_closed_with_a_named_issue() {
    assert_eq!(issues(POST_LOOP), [LANE_FORM_POST_LOOP_ISSUE]);
}

#[test]
fn host_lowering_accepts_the_kernels_device_lowering_refuses() {
    for source in [TWO_REGIONS, POST_LOOP] {
        lower(source, KernelLowering::Host).expect("host form is the sequential oracle and has no lane regions");
    }
}
