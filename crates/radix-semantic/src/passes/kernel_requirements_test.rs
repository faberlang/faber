//! Per-target width, range, call-graph, and callback requirements.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::passes::kernel_requirements::{
    KernelExtentCondition, KernelNumericPolicy, KernelRangeExpr, KernelRequirementFailure,
    KernelTargetRequirement, KernelTrappingStore, KernelValueRange, KernelWidthRequirement,
    merge_seeded_target_report,
};
use crate::test_support::{analyze_source, analyze_source_with_config};
use crate::{NumericWidth, Primitive};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_target::Target;

fn analyze_en(source: &str) -> crate::test_support::AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn target_row<'a>(
    fixture: &'a crate::test_support::AnalyzedFixture,
    name: &str,
    target: Target,
) -> &'a crate::passes::kernel_requirements::KernelTargetRequirement {
    let kernel = fixture
        .kernel_requirements
        .kernels
        .iter()
        .find(|kernel| kernel.name == name)
        .unwrap_or_else(|| panic!("missing kernel requirement row for {name}"));
    kernel
        .target_reports
        .iter()
        .find(|row| row.target == target)
        .unwrap_or_else(|| panic!("missing {target:?} row for {name}"))
}

fn params_interface(
    whole: Option<crate::file_interface::InterfaceConstValue>,
    frac: Option<crate::file_interface::InterfaceConstValue>,
) -> crate::file_interface::FileInterface {
    use crate::file_interface::{FileExport, FileExportKind, InterfaceTypeSnapshot};
    let mut interface = crate::file_interface::FileInterface::new();
    interface.insert(FileExport {
        name: "CLK_T_WHOLE".to_owned(),
        kind: FileExportKind::Const(
            InterfaceTypeSnapshot::Primitive(crate::Primitive::Numerus),
            whole,
        ),
    });
    interface.insert(FileExport {
        name: "CLK_T_FRAC".to_owned(),
        kind: FileExportKind::Const(
            InterfaceTypeSnapshot::Primitive(crate::Primitive::Numerus),
            frac,
        ),
    });
    interface
}

fn analyze_en_with_params(
    source: &str,
    whole: Option<crate::file_interface::InterfaceConstValue>,
    frac: Option<crate::file_interface::InterfaceConstValue>,
) -> crate::test_support::AnalyzedFixture {
    use crate::passes::visibility_annotation::VisibilityTier::Publica;
    use std::collections::BTreeMap;
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config.namespace_exports = BTreeMap::from([(
        "params".to_owned(),
        BTreeMap::from([
            ("CLK_T_WHOLE".to_owned(), Publica),
            ("CLK_T_FRAC".to_owned(), Publica),
        ]),
    )]);
    config.file_interfaces = BTreeMap::from([("params".to_owned(), params_interface(whole, frac))]);
    analyze_source_with_config(source, config)
}

#[test]
fn imported_const_without_folded_value_keeps_out_of_range() {
    let fixture = analyze_en_with_params(
        r#"
import from "./params" params

@ kernel
@ public
fn now(tensor<f32, [24]> clock) → f32 {
    return clock[params.CLK_T_WHOLE] + clock[params.CLK_T_FRAC]
}
"#,
        None,
        None,
    );
    let now = target_row(&fixture, "now", Target::MirWgsl);
    assert!(
        now.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::IntermediateOutOfRange { .. }
        )),
        "{:?}",
        now.failures
    );
}

#[test]
fn imported_const_index_folds_to_its_exact_value() {
    let fixture = analyze_en_with_params(
        r#"
import from "./params" params

@ kernel
@ public
fn now(tensor<f32, [24]> clock) → f32 {
    return clock[params.CLK_T_WHOLE] + clock[params.CLK_T_FRAC]
}
"#,
        Some(crate::file_interface::InterfaceConstValue::Int(0)),
        Some(crate::file_interface::InterfaceConstValue::Int(1)),
    );
    let now = target_row(&fixture, "now", Target::MirWgsl);
    assert!(
        !now.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::IntermediateOutOfRange { .. }
        )),
        "{:?}",
        now.failures
    );
}

#[test]
fn module_const_index_folds_to_its_exact_value() {
    let fixture = analyze_en(
        r"
const i32 K = 4

@ kernel
@ public
fn pick(tensor<f32, [8]> x) → f32 {
    return x[[K]]
}

@ kernel
@ public
fn bad(tensor<f32, [8]> x, int j) → f32 {
    return x[[j + 0]]
}
",
    );

    let pick = target_row(&fixture, "pick", Target::MirWgsl);
    assert!(pick.admitted, "{:?}", pick.failures);
    assert_eq!(
        pick.failures,
        [] as [crate::passes::kernel_requirements::KernelRequirementFailure; 0]
    );

    let bad = target_row(&fixture, "bad", Target::MirWgsl);
    assert!(
        bad.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::IntermediateOutOfRange { .. }
        )),
        "{:?}",
        bad.failures
    );
}

#[test]
fn wrapping_i32_is_admitted_only_on_implemented_device_leaves() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn wrapping_sum(wrapping<i32> left, wrapping<i32> right) → wrapping<i32> {
    return left + right
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    for target in [Target::MirMetal, Target::MirWgsl] {
        let report = target_row(&fixture, "wrapping_sum", target);
        assert!(report.admitted, "{target:?}: {:?}", report.failures);
        assert_eq!(
            report.trapping_stores,
            [] as [crate::passes::kernel_requirements::KernelTrappingStore; 0]
        );
        assert!(report.widths.iter().any(|width| {
            width.policy == crate::passes::kernel_requirements::KernelNumericPolicy::Wrapping
                && width.width == crate::NumericWidth::I32
        }));
    }

    let amd = target_row(&fixture, "wrapping_sum", Target::MirAmd);
    // MirAmd lacks a wrapping integer kernel path: that capability fact stays
    // in the target row, so `fixture.is_ok()` above still holds.
    assert!(!amd.admitted);
    assert!(amd.failures.iter().any(|failure| matches!(
        failure,
        KernelRequirementFailure::UnsupportedIntegerWidth {
            width: crate::NumericWidth::I32,
            ..
        }
    )));
}

#[test]
fn sub32_integer_widths_and_bf16_are_admitted() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn add8(u8 a, u8 b) → u8 {
    return (a + b) / 2
}

@ kernel
@ public
fn mix(i16 a, u16 b) → i32 {
    return a ↦ i32
}

@ kernel
@ public
fn bf(tensor<bf16, [4]> x) → bf16 {
    return x[[0]]
}

@ kernel
@ public
fn wrap8(wrapping<u8> a, wrapping<u8> b) → wrapping<u8> {
    return a + b
}

main {}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    let exact_widths = [
        ("add8", crate::NumericWidth::U8),
        ("mix", crate::NumericWidth::I16),
        ("mix", crate::NumericWidth::U16),
        ("bf", crate::NumericWidth::BF16),
    ];
    for (name, width) in exact_widths {
        for target in [Target::MirMetal, Target::MirWgsl] {
            let report = target_row(&fixture, name, target);
            assert!(
                report.admitted,
                "{target:?} refused {name}: {:?}",
                report.failures
            );
            assert!(
                report.widths.iter().any(|row| row.width == width),
                "{target:?} {name} missing a widths row for {width:?}"
            );
        }
    }

    let metal = target_row(&fixture, "wrap8", Target::MirMetal);
    assert!(
        metal.admitted,
        "MirMetal refused wrap8: {:?}",
        metal.failures
    );
    assert!(metal.widths.iter().any(|row| {
        row.policy == crate::passes::kernel_requirements::KernelNumericPolicy::Wrapping
            && row.width == crate::NumericWidth::U8
    }));

    let amd = target_row(&fixture, "wrap8", Target::MirAmd);
    assert!(!amd.admitted);
    assert!(amd.failures.iter().any(|failure| matches!(
        failure,
        KernelRequirementFailure::UnsupportedIntegerWidth {
            width: crate::NumericWidth::U8,
            ..
        }
    )));
}

#[test]
fn wrapping_word_intermediates_observed_before_store_must_fit_their_carrier() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn signed_sub(wrapping<i32> left, wrapping<i32> right) → bool {
    return left - right ≺ left
}

@ kernel
@ public
fn signed_mul(wrapping<i32> left, wrapping<i32> right) → bool {
    return left * right ≻ left
}

@ kernel
@ public
fn signed_neg(wrapping<i32> value, wrapping<i32> bound) → bool {
    return -value ≻ bound
}

@ kernel
@ public
fn unsigned_sub(wrapping<u32> left, wrapping<u32> right) → bool {
    return left - right ≺ left
}

@ kernel
@ public
fn nested_add_div(wrapping<i32> left, wrapping<i32> right) → bool {
    return (left + right) / 2 ≻ left
}
",
    );
    let wide_intermediates = fixture
        .diagnostics
        .iter()
        .filter(|error| error.issue() == Some("nucleum_wide_intermediate"))
        .count();
    assert_eq!(
        wide_intermediates, 5,
        "one check error per kernel: {:?}",
        fixture.diagnostics
    );
    assert!(
        fixture
            .diagnostics
            .iter()
            .filter(|error| error.issue() == Some("nucleum_wide_intermediate"))
            .all(|error| error.args.iter().any(|arg| arg.name == "extent")),
        "every wide intermediate names its extent: {:?}",
        fixture.diagnostics
    );

    let cases = [
        ("signed_sub", -4_294_967_295, 4_294_967_295),
        (
            "signed_mul",
            -4_611_686_016_279_904_256,
            4_611_686_018_427_387_904,
        ),
        ("signed_neg", -2_147_483_647, 2_147_483_648),
        ("unsigned_sub", -4_294_967_295, 4_294_967_295),
        ("nested_add_div", -4_294_967_296, 4_294_967_294),
    ];
    for (name, expected_minimum, expected_maximum) in cases {
        for target in [Target::MirMetal, Target::MirWgsl] {
            let report = target_row(&fixture, name, target);
            assert!(
                !report.admitted,
                "{target:?} admitted {name}: {:?}",
                report.failures
            );
            let range = report
                .failures
                .iter()
                .find_map(|failure| match failure {
                    KernelRequirementFailure::IntermediateOutOfRange { range, .. } => Some(range),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{target:?} missing exact {name} intermediate failure: {:?}",
                        report.failures
                    )
                });
            assert_eq!(
                range.minimum,
                crate::passes::kernel_requirements::KernelRangeExpr::Constant(expected_minimum),
                "{target:?} {name}"
            );
            assert_eq!(
                range.maximum,
                crate::passes::kernel_requirements::KernelRangeExpr::Constant(expected_maximum),
                "{target:?} {name}"
            );
        }
    }
}

#[test]
fn symbolic_apud_coordinate_is_retained_as_a_store_condition() {
    let fixture = analyze_source(
        r"
@ nucleum
@ publica
functio copy_coordinate<magnitudo N>(
    tensor<f32, [N]> input,
    varia tensor<u32, [N]> output
) → vacuum {
    itera ex input apud [i] sit value {
        output[i] ← i
    }
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    let report = target_row(&fixture, "copy_coordinate", Target::MirMetal);
    // Operator ruling: a launch-time extent condition counts as proof, so the
    // symbolic coordinate stays admitted and the condition is recorded.
    assert!(report.admitted, "{:?}", report.failures);
    assert!(
        report.extent_conditions.iter().any(|condition| {
            condition.minimum == 0 && condition.maximum == i128::from(u32::MAX)
        })
    );
}

#[test]
fn transitive_kernel_width_failure_closes_the_calling_kernel() {
    let fixture = analyze_source(
        r"
@ nucleum
functio wide_helper(numerus value) → vacuum {
    tacet
}

@ nucleum
@ publica
functio caller(f32 value) → f32 {
    wide_helper(1)
    redde value
}
",
    );
    assert!(
        fixture
            .diagnostics
            .iter()
            .any(|error| error.issue() == Some("nucleum_wide_integer")),
        "wide_helper is a check error: {:?}",
        fixture.diagnostics
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_wide_integer", "width"),
        Some("i64"),
        "{:?}",
        fixture.diagnostics
    );

    let helper = target_row(&fixture, "wide_helper", Target::MirMetal);
    assert!(!helper.admitted);
    let caller = target_row(&fixture, "caller", Target::MirMetal);
    assert!(!caller.admitted);
    assert!(
        caller
            .failures
            .iter()
            .any(|failure| matches!(failure, KernelRequirementFailure::CalleeNotAdmitted { .. }))
    );
}

#[test]
fn tensor_division_requires_a_proof_that_each_divisor_lane_is_nonzero() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn tensor_division(
    tensor<wrapping<i32>, [4]> dividend,
    tensor<wrapping<i32>, [4]> divisor
) → void {
    let tensor<wrapping<i32>, [4]> quotient ← dividend.divided(divisor)
}
",
    );
    assert!(
        fixture
            .diagnostics
            .iter()
            .any(|error| error.issue() == Some("nucleum_extent_unproven")),
        "the unproven divisor extent is a check error: {:?}",
        fixture.diagnostics
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_extent_unproven", "extent"),
        Some("unknown"),
        "{:?}",
        fixture.diagnostics
    );

    let report = target_row(&fixture, "tensor_division", Target::MirMetal);
    assert!(!report.admitted);
    assert!(report.failures.iter().any(|failure| matches!(
        failure,
        KernelRequirementFailure::DivideByZeroPossible { .. }
    )));
}

#[test]
fn recursion_is_a_per_target_requirement_failure() {
    let fixture = analyze_source(
        r"
@ nucleum
@ publica
functio recursive(f32 value) → f32 {
    redde recursive(value)
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    let report = target_row(&fixture, "recursive", Target::MirMetal);
    assert!(report.recursive);
    assert!(!report.admitted);
    assert!(
        report
            .failures
            .contains(&KernelRequirementFailure::RecursiveCall)
    );
}

#[test]
fn mutually_recursive_kernels_stay_a_per_target_failure_not_a_check_error() {
    // A recursive kernel pair is a capability fact, not a language error: the
    // call cycle is reported per target row, and the check itself stays ok.
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn ping(f32 value) → f32 {
    return pong(value)
}

@ kernel
@ public
fn pong(f32 value) → f32 {
    return ping(value)
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    for name in ["ping", "pong"] {
        let report = target_row(&fixture, name, Target::MirMetal);
        assert!(report.recursive, "{name} sits on the call cycle");
        assert!(!report.admitted, "{name} is refused on Metal");
        assert!(
            report
                .failures
                .contains(&KernelRequirementFailure::RecursiveCall),
            "{name}: {:?}",
            report.failures
        );
    }
}

#[test]
fn limes_callback_gets_shift_and_symbolic_extent_range_facts() {
    let fixture = analyze_en(
        r"
interface EdgePolicy<C> { fn remap(C coord) → C }
class ZeroGhost implements EdgePolicy<vector<int, 1>> {
    fn remap(vector<int, 1> coord) → vector<int, 1> {
        return coord
    }
}

@ kernel
@ public
fn map_edges<size N>(
    tensor<f32, [N]> input,
    mut tensor<f32, [N]> output
) → void {
    let tensor<f32, [N]> mapped ← input.shift([1]).limes<ZeroGhost>()
    for from mapped at [i] let value {
        output[i] ← value
    }
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    let report = target_row(&fixture, "map_edges", Target::MirMetal);
    assert!(report.admitted, "{:?}", report.failures);
    assert!(
        report
            .extent_conditions
            .iter()
            .any(|condition| condition.minimum == i128::from(i32::MIN)
                && condition.maximum == i128::from(i32::MAX)),
        "limes must retain the i32 callback narrowing condition: {:?}",
        report.extent_conditions
    );
}

#[test]
fn untyped_float_constants_adopt_the_float_width_they_meet() {
    // K15b: `0.0` has no width of its own, so an f32 kernel that compares or
    // scales by constants must not demand f64.
    let fixture = analyze_source(
        r"
@ nucleum
@ publica
functio relu(f32 value) → f32 {
    si value ≻ 0.0 {
        redde value * 2.0 - 1.5
    }
    redde -0.5
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    for target in [Target::MirMetal, Target::MirWgsl] {
        let report = target_row(&fixture, "relu", target);
        assert!(report.admitted, "{target:?}: {:?}", report.failures);
    }
}

#[test]
fn explicit_f64_conversion_of_a_float_constant_still_requires_f64() {
    // K15b guard: only an *untyped* constant is width-free. A conversion to
    // a sized float carries that float's width.
    let fixture = analyze_source(
        r"
@ nucleum
@ publica
functio widen(f32 value) → f64 {
    redde value ↦ f64
}
",
    );
    assert!(
        fixture
            .diagnostics
            .iter()
            .any(|error| error.issue() == Some("nucleum_f64")),
        "f64 is a check error: {:?}",
        fixture.diagnostics
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_f64", "width"),
        Some("f64"),
        "{:?}",
        fixture.diagnostics
    );
    let report = target_row(&fixture, "widen", Target::MirMetal);
    assert!(!report.admitted, "f64 must still be rejected on Metal");
}

#[test]
fn kernel_with_static_extents_records_no_entry_shape_relation() {
    // K15a: a statically sized kernel parameter is checked at the typed call
    // site; the kernel itself carries no entry relation.
    let fixture = analyze_source(
        r"
@ nucleum
@ publica
functio scale(tensor<f32, [5]> input, varia tensor<f32, [5]> output) → vacuum {
    itera ex input apud [i] sit value {
        output[i] ← value * 2.0
    }
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    assert!(
        fixture.result.shape_obligations.functions.is_empty(),
        "{:?}",
        fixture.result.shape_obligations.functions
    );
    let report = target_row(&fixture, "scale", Target::MirMetal);
    assert!(report.admitted, "{:?}", report.failures);
}

#[test]
fn host_function_with_a_repeated_size_symbol_keeps_its_entry_relation() {
    let fixture = analyze_source(
        r"
functio same<magnitudo N>(tensor<f32, [N]> left, tensor<f32, [N]> right) → f32 {
    redde left.media() + right.media()
}

incipit {
    sit tensor<f32, [3]> a ← vacua
    nota same(a, a)
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    assert_eq!(fixture.result.shape_obligations.functions.len(), 1);
}

#[test]
fn vector_literal_over_untyped_float_constants_requires_no_f64() {
    // K15b: `[x, 2.0, 3.0, 4.0] ↦ vf32[4]` is f32 end to end; the list
    // literal's untyped constants must not demand f64.
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn productum(f32 x) → f32 {
    let vf32[4] a ← [x, 2.0, 3.0, 4.0] ↦ vf32[4]
    let vf32[4] b ← [1.0, 1.0, 1.0, 1.0] ↦ vf32[4]
    return a.dot(b)
}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    for target in [Target::MirMetal, Target::MirWgsl] {
        let report = target_row(&fixture, "productum", target);
        assert!(report.admitted, "{target:?}: {:?}", report.failures);
    }
}

#[test]
fn explicit_f64_array_literal_still_requires_f64() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn wide(f64 x) → f64 {
    let vf64[2] a ← [x, 2.0] ↦ vf64[2]
    return a.dot(a)
}
",
    );
    assert!(
        fixture
            .diagnostics
            .iter()
            .any(|error| error.issue() == Some("nucleum_f64")),
        "f64 is a check error: {:?}",
        fixture.diagnostics
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_f64", "width"),
        Some("f64"),
        "{:?}",
        fixture.diagnostics
    );
    assert!(!target_row(&fixture, "wide", Target::MirMetal).admitted);
}

#[test]
fn lane_loop_copy_is_admitted_under_the_extent_product_condition() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn copy(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane let v {
        dst[[i, j]] ← v
    }
}
main {}
",
    );
    let report = target_row(&fixture, "copy", Target::MirMetal);
    assert!(report.admitted, "{:?}", report.failures);
    assert!(
        report.extent_conditions.iter().any(|condition| {
            condition.minimum == i128::from(i32::MIN) && condition.maximum == i128::from(u32::MAX)
        }),
        "the linear lane address is bounded by a recorded extent condition: {:?}",
        report.extent_conditions
    );
}

#[test]
fn arbitrary_scalar_coordinates_into_a_tensor_stay_rejected() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn pick(tf32[_, _] input, mut tf32[_, _] output, u32 y, u32 x) → void {
    output[[y, x]] ← input[[y, x]]
}
main {}
",
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_extent_unproven", "extent"),
        Some("unknown"),
        "arbitrary coordinates are an unproven extent: {:?}",
        fixture.diagnostics
    );

    let report = target_row(&fixture, "pick", Target::MirMetal);
    assert!(!report.admitted);
    assert!(
        report.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::UnsupportedIntegerRange { .. }
        )),
        "{:?}",
        report.failures
    );
}

fn reduction_store_report(
    source: &str,
    name: &str,
) -> (
    crate::test_support::AnalyzedFixture,
    crate::passes::kernel_requirements::KernelTargetRequirement,
) {
    let fixture = analyze_en(source);
    let report = target_row(&fixture, name, Target::MirMetal).clone();
    (fixture, report)
}

#[test]
fn rank_one_argmax_over_a_literal_extent_stores_into_i32() {
    let (fixture, report) = reduction_store_report(
        r"
@ kernel
@ public
fn peak(tensor<f32, [32]> src, mut tensor<i32, [1]> out) → void {
    out[0] ← reduce via argmax from src at [i] thread lane let v { return v } coalesce 0
}
main {}
",
        "peak",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    assert!(report.admitted, "{:?}", report.failures);
    assert_eq!(
        report.trapping_stores,
        [] as [crate::passes::kernel_requirements::KernelTrappingStore; 0]
    );
}

#[test]
fn rank_one_argmin_over_a_symbolic_extent_is_admitted_under_an_extent_condition() {
    let (fixture, report) = reduction_store_report(
        r"
@ kernel
@ public
fn trough(tensor<f32, [_]> src, mut tensor<i32, [1]> out) → void {
    out[0] ← reduce via argmin from src at [i] thread lane let v { return v } coalesce 0
}
main {}
",
        "trough",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    // Operator ruling: a launch-time extent condition counts as proof, so a
    // symbolic extent is admitted rather than an unproven integer range.
    assert!(report.admitted, "{:?}", report.failures);
    assert!(
        report
            .extent_conditions
            .iter()
            .any(|condition| condition.maximum == i128::from(i32::MAX)),
        "the symbolic extent is bounded by the cell: {:?}",
        report.extent_conditions
    );
}

#[test]
fn count_over_a_literal_extent_stores_into_i32() {
    let (fixture, report) = reduction_store_report(
        r"
@ kernel
@ public
fn tally(tensor<f32, [4, 32]> src, mut tensor<i32, [1]> out) → void {
    out[0] ← reduce via count from src at [i, j] thread lane let v { return v ≻ 1.0 }
}
main {}
",
        "tally",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    assert!(report.admitted, "{:?}", report.failures);
}

#[test]
fn an_integer_max_reduction_into_i32_is_still_out_of_range() {
    let (fixture, report) = reduction_store_report(
        r"
@ kernel
@ public
fn widest(tensor<int, [32]> src, mut tensor<i32, [1]> out) → void {
    out[0] ← reduce via max from src at [i] thread lane let v { return v } coalesce 0
}
main {}
",
        "widest",
    );
    assert!(
        fixture
            .diagnostics
            .iter()
            .any(|error| error.issue() == Some("nucleum_wide_integer")),
        "the i64 tensor<int, [32]> parameter is a check error: {:?}",
        fixture.diagnostics
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_wide_integer", "width"),
        Some("i64"),
        "{:?}",
        fixture.diagnostics
    );
    assert!(!report.admitted);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| matches!(failure, KernelRequirementFailure::StoreOutOfRange { .. })),
        "{:?}",
        report.failures
    );
}

/// Named argument of the first diagnostic carrying `issue`.
fn issue_arg<'a>(
    fixture: &'a crate::test_support::AnalyzedFixture,
    issue: &str,
    name: &str,
) -> Option<&'a str> {
    fixture
        .diagnostics
        .iter()
        .find(|error| error.issue() == Some(issue))
        .and_then(|error| error.args.iter().find(|arg| arg.name == name))
        .map(|arg| arg.value.as_str())
}

#[test]
fn wide_integer_kernel_values_are_check_errors() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn k(int x) → int {
    return x + 1
}
main {}
",
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_wide_integer", "width"),
        Some("i64"),
        "{:?}",
        fixture.diagnostics
    );

    let twin = analyze_en(
        r"
@ kernel
@ public
fn k(u64 x) → u64 {
    return x
}
main {}
",
    );
    assert_eq!(
        issue_arg(&twin, "nucleum_wide_integer", "width"),
        Some("u64"),
        "{:?}",
        twin.diagnostics
    );
}

#[test]
fn f64_and_d64_kernel_values_are_check_errors() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn k(f64 x) → f64 {
    return x * 2.0
}
main {}
",
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_f64", "width"),
        Some("f64"),
        "{:?}",
        fixture.diagnostics
    );

    let decimal = analyze_en(
        r"
@ kernel
@ public
fn k(d64 x) → d64 {
    return x
}
main {}
",
    );
    assert_eq!(
        issue_arg(&decimal, "nucleum_d64", "width"),
        Some("d64"),
        "{:?}",
        decimal.diagnostics
    );
}

#[test]
fn unprovable_trapping_store_is_a_check_error() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn k(tensor<i32, [4]> src) → void {
    mut i32 y ← 0
    y ← src[[0]] * 3
}
main {}
",
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_trapping_store", "width"),
        Some("i32"),
        "{:?}",
        fixture.diagnostics
    );
}

#[test]
fn wide_exact_intermediate_is_a_check_error() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn half(wrapping<i32> left, wrapping<i32> right) → bool {
    return (left + right) / 2 ≺ left
}
main {}
",
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_wide_intermediate", "extent"),
        Some("-4294967296..4294967294"),
        "{:?}",
        fixture.diagnostics
    );
}

#[test]
fn signed_gather_coordinates_fit_the_address_envelope() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn gather(tensor<i32, [4]> ids, tensor<f32, [8]> data, mut tensor<f32, [4]> out) → void {
    for from ids at [i] thread lane let row {
        out[[i]] ← data[[row]]
    }
}
main {}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    let row = target_row(&fixture, "gather", Target::MirMetal);
    assert!(row.admitted, "{:?}", row.failures);
}

#[test]
fn shape_metadata_recipe_places_no_wide_integer_requirement() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn expand_mask<size C>(tensor<f32, [5, 3, 1, C]> scores, tensor<f32, [1, C]> mask, mut tensor<f32, [5, 3, 1, C]> output, u32 id) → void {
    output ⇇ scores + mask.expanded(scores.shape())
}
main {}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    assert!(issue_arg(&fixture, "nucleum_wide_integer", "width").is_none());
    let row = target_row(&fixture, "expand_mask", Target::MirMetal);
    assert!(row.admitted, "{:?}", row.failures);
}

#[test]
fn tensor_addressing_does_not_hide_wide_coordinate_arithmetic() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn gather(tensor<i32, [4]> ids, tensor<f32, [8]> data, mut tensor<f32, [4]> out) → void {
    for from ids at [i] thread lane let row {
        out[[i]] ← data[[row * 4]]
    }
}
main {}
",
    );
    assert!(issue_arg(&fixture, "nucleum_wide_intermediate", "extent").is_some());
    assert!(!target_row(&fixture, "gather", Target::MirMetal).admitted);
}

#[test]
fn remainder_accepts_a_positive_variable_divisor_but_not_a_possible_zero() {
    for (guard, admitted) in [("d ≻ 0", true), ("d ≥ 0", false)] {
        let fixture = analyze_en(&format!(
            r"
@ kernel
@ public
fn rem(i32 x, i32 d) → void {{
    if {guard} {{
        let i32 y ← x % d
    }}
}}
main {{}}
"
        ));
        let row = target_row(&fixture, "rem", Target::MirMetal);
        assert_eq!(row.admitted, admitted, "{:?}", row.failures);
    }
}

#[test]
fn scalar_clamps_preserve_portable_sampler_coordinate_bounds() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn sample<size H, size W>(tensor<f32, [H, W]> grid, i32 x, i32 y, i32 width, i32 height, i32 step) → f32 {
    let i32 cx ← (x ⤒ 0) ⤓ ((width ⤒ 1) - 1)
    let i32 cy ← (y ⤒ 0) ⤓ ((height ⤒ 1) - 1)
    if step ≻ 0 {
        let i32 qx ← cx / step
        let i32 qy ← cy / step
        let i32 x0 ← qx ⤓ ((width ⤒ 2) - 2)
        let i32 y0 ← qy ⤓ ((height ⤒ 2) - 2)
        return grid[[y0 + 1, x0 + 1]]
    }
    return 0.0
}
@ kernel
@ public
fn bad(i32 x) → i32 {
    return ((x ⤒ 0) + 1) * 3
}
",
    );
    let sample = target_row(&fixture, "sample", Target::MirMetal);
    assert!(sample.admitted, "{:?}", sample.failures);
    assert_eq!(
        sample.failures,
        [] as [crate::passes::kernel_requirements::KernelRequirementFailure; 0]
    );
    let bad = target_row(&fixture, "bad", Target::MirMetal);
    assert!(bad.failures.iter().any(|failure| matches!(
        failure,
        KernelRequirementFailure::IntermediateOutOfRange { .. }
    )));
}

#[test]
fn grouped_reduction_keys_have_coordinate_facts_and_count_cells_keep_their_bound() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn grouped(tf32[6] a, tensor<i32, [6]> keys) → tf32[3] {
    return reduce via sum from a at [i] per keys[i]
}
@ kernel
@ public
fn too_many(tf32[65536, 65536] a) → tensor<i32, [1]> {
    return reduce via count from a at [i, j] per 0 thread lane let s { return s ≻ 0.0 }
}
",
    );
    let grouped = target_row(&fixture, "grouped", Target::MirMetal);
    assert!(grouped.admitted, "{:?}", grouped.failures);
    let too_many = target_row(&fixture, "too_many", Target::MirMetal);
    assert!(
        too_many
            .failures
            .iter()
            .any(|failure| matches!(failure, KernelRequirementFailure::StoreOutOfRange { .. })),
        "{:?}",
        too_many.failures
    );
}

#[test]
fn remainder_by_a_positive_size_class_parameter_is_provable() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn mod_kernel(i32 j, i32 span) → i32 {
    let i32 hf ← span
    return j % hf
}

@ kernel
@ public
fn control_kernel(i32 j, i32 k) → i32 {
    return j % k
}
",
    );

    let proven = target_row(&fixture, "mod_kernel", Target::MirWgsl);
    assert!(proven.admitted, "{:?}", proven.failures);
    assert!(proven.failures.is_empty(), "{:?}", proven.failures);
    assert_eq!(proven.extent_conditions.len(), 1, "{proven:?}");
    let condition = &proven.extent_conditions[0];
    assert_eq!(condition.minimum, 1, "{condition:?}");
    assert!(
        matches!(condition.range.minimum, KernelRangeExpr::Parameter(_)),
        "{condition:?}"
    );
    assert!(
        matches!(condition.range.maximum, KernelRangeExpr::Parameter(_)),
        "{condition:?}"
    );

    let control = target_row(&fixture, "control_kernel", Target::MirWgsl);
    assert!(!control.admitted);
    assert!(
        control.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::DivideByZeroPossible { .. }
        )),
        "{:?}",
        control.failures
    );
}

#[test]
fn division_by_a_positive_size_class_parameter_is_provable() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn div_kernel(i32 j, i32 span) → i32 {
    let i32 hf ← span
    return j / hf
}

@ kernel
@ public
fn control_kernel(i32 j, i32 k) → i32 {
    return j / k
}
",
    );

    let proven = target_row(&fixture, "div_kernel", Target::MirWgsl);
    assert!(proven.admitted, "{:?}", proven.failures);
    assert!(proven.failures.is_empty(), "{:?}", proven.failures);
    assert_eq!(proven.extent_conditions.len(), 1, "{proven:?}");
    let condition = &proven.extent_conditions[0];
    assert_eq!(condition.minimum, 1, "{condition:?}");
    assert!(
        matches!(condition.range.minimum, KernelRangeExpr::Parameter(_)),
        "{condition:?}"
    );
    assert!(
        matches!(condition.range.maximum, KernelRangeExpr::Parameter(_)),
        "{condition:?}"
    );

    let control = target_row(&fixture, "control_kernel", Target::MirWgsl);
    assert!(!control.admitted);
    assert!(
        control.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::DivideByZeroPossible { .. }
        )),
        "{:?}",
        control.failures
    );
}

#[test]
fn lane_quotient_by_a_positive_size_class_parameter_is_provable() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn base_of<size H>(tensor<f32, [H]> dom, mut tensor<i32, [H]> out, i32 group) → void {
    for from dom at [h] thread lane let e {
        let i32 g ← group
        let i32 b ← h / g
        out[[h]] ← b
    }
}
main {}
",
    );
    let row = target_row(&fixture, "base_of", Target::MirWgsl);
    for failure in &row.failures {
        assert!(
            !matches!(
                failure,
                KernelRequirementFailure::DivideByZeroPossible { .. }
                    | KernelRequirementFailure::UnsupportedIntegerRange { .. }
                    | KernelRequirementFailure::OptionFRequired { .. }
            ),
            "lane-coordinate quotient must be proven: {failure:?}"
        );
    }
    assert!(
        row.extent_conditions
            .iter()
            .any(|condition| matches!(condition.range.minimum, KernelRangeExpr::Parameter(_))),
        "the quotient pushes a launch-contract extent condition: {:?}",
        row.extent_conditions
    );
}

#[test]
fn constant_arguments_seed_a_private_kernel_callee() {
    let fixture = analyze_en(
        r"
@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, 8)
}
",
    );
    let helper = target_row(&fixture, "helper", Target::MirWgsl);
    assert!(helper.admitted, "{:?}", helper.failures);
    let caller = target_row(&fixture, "caller", Target::MirWgsl);
    assert!(caller.admitted, "{:?}", caller.failures);

    let dynamic = analyze_en(
        r"
@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return helper(clock, base)
}
",
    );
    let helper = target_row(&dynamic, "helper", Target::MirWgsl);
    assert!(!helper.admitted, "{:?}", helper.failures);
    assert!(
        helper.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::IntermediateOutOfRange { .. }
        )),
        "a runtime argument keeps the conservative row: {:?}",
        helper.failures
    );
}

#[test]
fn imported_const_call_argument_seeds_a_private_kernel_callee() {
    let fixture = analyze_en_with_params(
        r#"
import from "./params" params

@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, params.CLK_T_WHOLE)
}
"#,
        Some(crate::file_interface::InterfaceConstValue::Int(0)),
        Some(crate::file_interface::InterfaceConstValue::Int(1)),
    );
    let helper = target_row(&fixture, "helper", Target::MirWgsl);
    assert!(helper.admitted, "{:?}", helper.failures);
    let caller = target_row(&fixture, "caller", Target::MirWgsl);
    assert!(caller.admitted, "{:?}", caller.failures);

    let unfolded = analyze_en_with_params(
        r#"
import from "./params" params

@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, params.CLK_T_WHOLE)
}
"#,
        None,
        None,
    );
    let helper = target_row(&unfolded, "helper", Target::MirWgsl);
    assert!(!helper.admitted, "{:?}", helper.failures);
    assert!(
        helper.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::IntermediateOutOfRange { .. }
        )),
        "an unfolded imported value keeps the conservative row: {:?}",
        helper.failures
    );
}

#[test]
fn module_const_call_argument_seeds_a_private_kernel_callee() {
    let fixture = analyze_en(
        r"
const i32 K = 4

@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, K + 4)
}
",
    );
    let helper = target_row(&fixture, "helper", Target::MirWgsl);
    assert!(helper.admitted, "{:?}", helper.failures);
    let caller = target_row(&fixture, "caller", Target::MirWgsl);
    assert!(caller.admitted, "{:?}", caller.failures);
}

#[test]
fn disagreeing_call_sites_merge_into_one_admitted_row() {
    let fixture = analyze_en_with_params(
        r#"
import from "./params" params

@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, 3>
}

@ kernel
@ public
fn caller_whole(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, params.CLK_T_WHOLE)
}

@ kernel
@ public
fn caller_frac(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, params.CLK_T_FRAC)
}
"#,
        Some(crate::file_interface::InterfaceConstValue::Int(0)),
        Some(crate::file_interface::InterfaceConstValue::Int(1)),
    );
    let helper = target_row(&fixture, "helper", Target::MirWgsl);
    assert!(
        helper.admitted,
        "distinct constant tuples merge into one admitted row: {:?}",
        helper.failures
    );
}

#[test]
fn merge_seeded_target_report_intersects_failures_and_unions_requirements() {
    let span = Span::default();
    let exact = KernelWidthRequirement {
        span,
        primitive: Primitive::Numerus,
        width: NumericWidth::I32,
        policy: KernelNumericPolicy::Exact,
    };
    let wrapping = KernelWidthRequirement {
        policy: KernelNumericPolicy::Wrapping,
        ..exact.clone()
    };
    let extent = KernelExtentCondition {
        span,
        range: KernelValueRange {
            minimum: KernelRangeExpr::Constant(0),
            maximum: KernelRangeExpr::Constant(7),
        },
        minimum: 0,
        maximum: 7,
    };
    let store = KernelTrappingStore {
        span,
        width: NumericWidth::I32,
    };
    let conservative = KernelTargetRequirement {
        target: Target::MirWgsl,
        admitted: true,
        recursive: false,
        callees: vec![DefId(11)],
        widths: vec![exact.clone()],
        extent_conditions: Vec::new(),
        trapping_stores: Vec::new(),
        failures: vec![KernelRequirementFailure::DivideByZeroPossible { span }],
    };
    let seed_a = KernelTargetRequirement {
        target: Target::MirWgsl,
        admitted: false,
        recursive: false,
        callees: vec![DefId(21)],
        widths: vec![exact.clone(), wrapping.clone()],
        extent_conditions: vec![extent.clone()],
        trapping_stores: vec![store.clone()],
        failures: vec![
            KernelRequirementFailure::DivideByZeroPossible { span },
            KernelRequirementFailure::CalleeNotAdmitted { callee: DefId(3) },
        ],
    };
    let seed_b = KernelTargetRequirement {
        target: Target::MirWgsl,
        admitted: false,
        recursive: false,
        callees: Vec::new(),
        widths: vec![exact.clone()],
        extent_conditions: Vec::new(),
        trapping_stores: Vec::new(),
        failures: vec![KernelRequirementFailure::DivideByZeroPossible { span }],
    };

    let merged = merge_seeded_target_report(&conservative, &[seed_a, seed_b]);

    assert_eq!(merged.target, Target::MirWgsl);
    assert!(!merged.recursive);
    assert_eq!(merged.failures.len(), 1, "{:?}", merged.failures);
    assert!(matches!(
        merged.failures.first(),
        Some(KernelRequirementFailure::DivideByZeroPossible { .. })
    ));
    assert!(!merged.admitted, "a shared failure keeps the row closed");
    assert_eq!(merged.widths.len(), 2, "{:?}", merged.widths);
    assert!(merged.widths.contains(&exact));
    assert!(merged.widths.contains(&wrapping));
    assert_eq!(merged.extent_conditions, vec![extent]);
    assert_eq!(merged.trapping_stores, vec![store]);
    assert_eq!(merged.callees.first(), Some(&DefId(11)));
    assert!(merged.callees.contains(&DefId(21)));

    let range_failure = |minimum, maximum| KernelRequirementFailure::IntermediateOutOfRange {
        span,
        range: KernelValueRange {
            minimum: KernelRangeExpr::Constant(minimum),
            maximum: KernelRangeExpr::Constant(maximum),
        },
    };
    let range_report_a = KernelTargetRequirement {
        admitted: false,
        failures: vec![range_failure(8_000_000_000, 8_000_000_000)],
        ..conservative.clone()
    };
    let range_report_b = KernelTargetRequirement {
        admitted: false,
        failures: vec![range_failure(27_000_000_000, 27_000_000_000)],
        ..conservative.clone()
    };
    let merged_ranges =
        merge_seeded_target_report(&conservative, &[range_report_a, range_report_b]);
    assert!(
        !merged_ranges.admitted,
        "each specialization exceeds the envelope"
    );
    assert_eq!(
        merged_ranges.failures.len(),
        2,
        "retain both concrete ranges"
    );

    let clean = KernelTargetRequirement {
        admitted: true,
        failures: Vec::new(),
        ..conservative.clone()
    };
    let merged_clean = merge_seeded_target_report(&conservative, &[clean.clone(), clean]);
    assert!(merged_clean.admitted);
    assert_eq!(
        merged_clean.failures,
        [] as [crate::passes::kernel_requirements::KernelRequirementFailure; 0]
    );

    assert_eq!(
        merge_seeded_target_report(&conservative, &[]),
        conservative,
        "no seeded report leaves the conservative row alone"
    );
}

#[test]
fn out_of_range_specializations_with_different_ranges_keep_callee_closed() {
    let fixture = analyze_en(
        r"
@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → f32 {
    return clock[base * base * base]
}

@ kernel
@ public
fn caller_a(tensor<f32, [24]> clock) → f32 {
    return helper(clock, 2000)
}

@ kernel
@ public
fn caller_b(tensor<f32, [24]> clock) → f32 {
    return helper(clock, 3000)
}
",
    );
    let helper = target_row(&fixture, "helper", Target::MirWgsl);
    assert!(
        !helper.admitted,
        "both specialized intermediate ranges exceed the envelope: {:?}",
        helper.failures
    );
    assert!(helper.failures.iter().any(|failure| matches!(
        failure,
        KernelRequirementFailure::IntermediateOutOfRange { .. }
    )));
}

#[test]
fn distinct_constant_tuples_merge_into_one_admitted_callee_row() {
    let fixture = analyze_en(
        r"
@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, 3>
}

@ kernel
@ public
fn caller_om(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, 8)
}

@ kernel
@ public
fn caller_al(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, 11)
}
",
    );
    let helper = target_row(&fixture, "helper", Target::MirWgsl);
    assert!(helper.admitted, "{:?}", helper.failures);
    for caller in ["caller_om", "caller_al"] {
        let row = target_row(&fixture, caller, Target::MirWgsl);
        assert!(row.admitted, "{caller}: {:?}", row.failures);
    }
}

#[test]
fn mixed_constant_and_dynamic_call_sites_keep_the_conservative_row_when_merging() {
    let fixture = analyze_en(
        r"
@ kernel
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, 3>
}

@ kernel
@ public
fn caller_const(tensor<f32, [24]> clock) → vector<f32, 3> {
    return helper(clock, 8)
}

@ kernel
@ public
fn caller_dynamic(tensor<f32, [24]> clock, i32 base) → vector<f32, 3> {
    return helper(clock, base)
}
",
    );
    let helper = target_row(&fixture, "helper", Target::MirWgsl);
    assert!(!helper.admitted, "{:?}", helper.failures);
    assert!(
        helper.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::IntermediateOutOfRange { .. }
        )),
        "one non-constant argument keeps the conservative row: {:?}",
        helper.failures
    );
}
