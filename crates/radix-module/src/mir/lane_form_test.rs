//! Lane-form lowering of a top-level `thread lane` loop (gpu-reset C4-0).

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::lower::lane_form::{LANE_FORM_POST_LOOP_ISSUE, LANE_FORM_SECOND_REGION_ISSUE};
use crate::mir::{KernelLowering, MirError, MirLoweringOptions, lower_analyzed_unit};
use crate::program::analyze_prepared_program_name;
use radix_mir::dump::dump_program;
use radix_mir::{MirLocalId, MirParamMode};

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
    lower_analyzed_unit(unit.application_view(), options).map(|lowered| {
        let program = lowered.validated.program();
        let modes = program.functions[0]
            .params
            .iter()
            .map(|param| param.mode)
            .collect();
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
        .map(|line| {
            line.split(": ty#")
                .next()
                .unwrap_or(line)
                .trim_end()
                .to_owned()
        })
        .collect()
}

const COPY: &str = r"
@ kernel
@ public
fn copy(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane let v {
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
fn lowered_lane_metadata_keeps_the_for_from_source_local() {
    let unit = analyze(
        r#"
@ kernel
@ public
fn copy(tensor<f32, [3]> other, tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane let v {
        dst[[i, j]] ← v + other[0]
    }
}
main {}
"#,
    );
    let lowered = lower_analyzed_unit(
        unit.application_view(),
        MirLoweringOptions { kernel_lowering: KernelLowering::Device, ..Default::default() },
    )
    .expect("device lane form lowers");
    let function = &lowered.validated.program().functions[0];
    let metadata = lowered
        .device_function_metadata
        .get(&function.id)
        .expect("device metadata");
    assert_eq!(metadata.lane_domain_local, Some(MirLocalId(1)));
}

#[test]
fn host_form_of_the_same_kernel_is_unchanged() {
    let (dump, modes) = lower(COPY, KernelLowering::Host).expect("host lowering validates");
    assert_eq!(
        modes,
        [MirParamMode::Owned, MirParamMode::MutRef],
        "host form has no lane parameter"
    );
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
    for from src at [i, j] thread lane let v {
        dst[[i, j]] ← v
    }
    for from src at [i, j] thread lane let v {
        dst[[i, j]] ← v + 1.0
    }
}
main {}
";

const POST_LOOP: &str = r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane let v {
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

/// Gpu-reset C4-2g: the lane id and the grid arithmetic run in the `u32`
/// device word (the 32-bit GlobalInvocationX), not in `numerus` (i64). The
/// program-visible binders keep their `int` meaning (F9 ruling 27).
#[test]
fn lane_id_and_grid_arithmetic_are_u32_while_binders_stay_int() {
    use radix_types::{NumericWidth, Primitive, Type};
    let unit = analyze(COPY);
    let options = MirLoweringOptions { kernel_lowering: KernelLowering::Device, ..Default::default() };
    let lowered = lower_analyzed_unit(unit.application_view(), options).expect("device lowering validates");
    let types = &lowered.validated.validation().types;
    let function = &lowered.validated.program().functions[0];
    let u32_ty = types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
        .expect("device lowering interns the u32 word");
    let int_ty = types.primitive(Primitive::Numerus);
    assert!(matches!(
        types.get(u32_ty),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
    ));
    let lane = function
        .params
        .iter()
        .find(|param| param.mode == MirParamMode::Lane)
        .expect("lane parameter");
    assert_eq!(lane.ty.semantic_id(), u32_ty, "the Lane builtin is the u32 invocation id");
    // %0 %1 extents, %2 product, %3 guard (bool), %4 lane % E1, %5 lane / E1.
    let temp = |index: usize| function.temps[index].ty.semantic_id();
    for index in [0, 1, 2, 4, 5] {
        assert_eq!(temp(index), u32_ty, "temp %{index} runs in the u32 word");
    }
    let binders: Vec<_> = function
        .locals
        .iter()
        .filter(|local| local.name.is_some())
        .collect();
    assert!(
        binders.iter().any(|local| local.ty.semantic_id() == int_ty),
        "the coordinate binders keep their `int` meaning"
    );
}

/// Gpu-reset C4-2g: the lane-form `copy` passes the device admission gate
/// (its linear address is bounded by an extent-product condition) and reaches
/// the Metal lane emitter (C4-2f), which emits it.
#[cfg(feature = "mir-metal")]
#[test]
fn lane_form_copy_is_admitted_and_emits_on_metal() {
    let msl = crate::mir::test_support::compile_metal_text(&english(COPY))
        .unwrap_or_else(|error| panic!("lane-form copy must emit: {error}"));
    // ca2499dc-3.1: the compiler owns the device entry name; the author's
    // `copy` is decoration only and the emitted entry is `k<function id>`.
    let entry = msl
        .split_once("kernel void ")
        .map(|(_, rest)| rest)
        .expect("the Metal source declares a kernel entry");
    assert!(
        entry
            .strip_prefix('k')
            .is_some_and(|id| id.starts_with(|c: char| c.is_ascii_digit())),
        "the entry is k<id>: {msl}"
    );
    assert!(msl.contains("thread_position_in_grid"), "{msl}");
    assert!(msl.contains("b0_extent0"), "extent resources are declared: {msl}");
}

#[test]
fn coordinate_float_conversions_use_checked_views_of_int_binders() {
    use radix_mir::{MirIntrinsic, MirStatementKind};
    use radix_types::{NumericWidth, Primitive};
    let source = COPY.replace("dst[[i, j]] ← v", "dst[[i, j]] ← (i ↦ f32) + (j ↦ f32)");
    let unit = analyze(&source);
    let options = MirLoweringOptions { kernel_lowering: KernelLowering::Device, ..Default::default() };
    let lowered = lower_analyzed_unit(unit.application_view(), options)
        .expect("coordinate conversions must agree with their operand types");
    let types = &lowered.validated.validation().types;
    let function = &lowered.validated.program().functions[0];
    let int_ty = types.primitive(Primitive::Numerus);
    let i32_ty = types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::I32)
        .expect("checked i32 coordinate");
    assert_eq!(
        function
            .locals
            .iter()
            .filter(|local| local.name.is_some() && local.ty.semantic_id() == int_ty)
            .count(),
        2
    );
    let conversions: Vec<_> = function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => match &call.intrinsic {
                MirIntrinsic::Convert(conversion) => Some(conversion),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(conversions.len(), 2);
    assert!(
        conversions
            .iter()
            .all(|conversion| conversion.source_ty.semantic_id() == i32_ty)
    );
}
