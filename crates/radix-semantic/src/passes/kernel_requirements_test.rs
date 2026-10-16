//! Per-target width, range, call-graph, and callback requirements.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::passes::kernel_requirements::KernelRequirementFailure;
use crate::test_support::{analyze_source, analyze_source_with_config};
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
        assert!(report.trapping_stores.is_empty());
        assert!(report.widths.iter().any(|width| {
            width.policy == crate::passes::kernel_requirements::KernelNumericPolicy::Wrapping
                && width.width == crate::NumericWidth::I32
        }));
    }

    let amd = target_row(&fixture, "wrapping_sum", Target::MirAmd);
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
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

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
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

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
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

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
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
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
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
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
) -> crate::passes::kernel_requirements::KernelTargetRequirement {
    let fixture = analyze_en(source);
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    target_row(&fixture, name, Target::MirMetal).clone()
}

#[test]
fn rank_one_argmax_over_a_literal_extent_stores_into_i32() {
    let report = reduction_store_report(
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
    assert!(report.admitted, "{:?}", report.failures);
    assert!(report.trapping_stores.is_empty());
}

#[test]
fn rank_one_argmin_over_a_symbolic_extent_is_admitted_under_an_extent_condition() {
    let report = reduction_store_report(
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
    let report = reduction_store_report(
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
    assert!(report.admitted, "{:?}", report.failures);
}

#[test]
fn an_integer_max_reduction_into_i32_is_still_out_of_range() {
    let report = reduction_store_report(
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
