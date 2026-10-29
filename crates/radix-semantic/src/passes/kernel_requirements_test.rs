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

    // A 64-bit index is a kernel value now, but the address it forms is not
    // proven to fit the device index path.
    let bad = target_row(&fixture, "bad", Target::MirWgsl);
    assert!(
        bad.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::UnsupportedIntegerRange { .. }
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
fn wrapping_word_intermediates_observed_before_store_live_in_the_64_bit_range() {
    // Ruling 2026-10-28: a wrapping word reduces once, at the store; the exact
    // value an observer reads lives in the host's 64-bit intermediate range.
    // Beyond the portable 32-bit envelope it needs a 64-bit lane, which no
    // device emitter computes yet: every target row refuses it by name, and it
    // is never a check error.
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
main {}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);

    for name in [
        "signed_sub",
        "signed_mul",
        "signed_neg",
        "unsigned_sub",
        "nested_add_div",
    ] {
        for target in Target::DEVICE_TARGETS {
            let report = target_row(&fixture, name, target);
            assert!(
                !report.admitted,
                "{target:?} admitted {name}: {:?}",
                report.failures
            );
            assert!(
                report.failures.iter().any(|failure| matches!(
                    failure,
                    KernelRequirementFailure::UnsupportedIntegerWidth {
                        width: NumericWidth::I64,
                        ..
                    }
                )),
                "{target:?} must refuse {name} by naming the 64-bit lane: {:?}",
                report.failures
            );
        }
    }
}

#[test]
fn wrapping_u64_intermediates_beyond_64_bits_are_check_errors() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn sub(wrapping<u64> left, wrapping<u64> right) → bool {
    return left - right ≺ left
}

@ kernel
@ public
fn neg(wrapping<u64> value, wrapping<u64> bound) → bool {
    return -value ≻ bound
}

@ kernel
@ public
fn add_div(wrapping<u64> left, wrapping<u64> right) → bool {
    return (left + right) / 2 ≻ left
}
main {}
",
    );
    let extents: Vec<_> = fixture
        .diagnostics
        .iter()
        .filter(|error| error.issue() == Some("nucleum_wide_intermediate"))
        .filter_map(|error| error.args.iter().find(|arg| arg.name == "extent"))
        .map(|arg| arg.value.as_str())
        .collect();
    assert_eq!(
        extents.len(),
        3,
        "one check error per kernel: {:?}",
        fixture.diagnostics
    );
    for name in ["sub", "neg", "add_div"] {
        for target in Target::DEVICE_TARGETS {
            let report = target_row(&fixture, name, target);
            assert!(
                report.failures.iter().any(|failure| matches!(
                    failure,
                    KernelRequirementFailure::IntermediateOutOfRange { .. }
                )),
                "{target:?} {name}: {:?}",
                report.failures
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
fn transitive_wide_width_refusal_closes_the_calling_kernel_on_narrow_targets() {
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
        !fixture.diagnostics.iter().any(|error| error.is_error()),
        "a 64-bit helper is a capability fact, not a check error: {:?}",
        fixture.diagnostics
    );

    for name in ["wide_helper", "caller"] {
        let metal = target_row(&fixture, name, Target::MirMetal);
        assert!(metal.admitted, "{name}: {:?}", metal.failures);
    }
    for target in [Target::MirWgsl, Target::MirAmd] {
        let helper = target_row(&fixture, "wide_helper", target);
        assert!(!helper.admitted);
        assert!(helper.failures.iter().any(|failure| matches!(
            failure,
            KernelRequirementFailure::UnsupportedIntegerWidth {
                width: NumericWidth::I64,
                ..
            }
        )));
        let caller = target_row(&fixture, "caller", target);
        assert!(!caller.admitted);
        assert!(
            caller.failures.iter().any(|failure| matches!(
                failure,
                KernelRequirementFailure::CalleeNotAdmitted { .. }
            ))
        );
    }
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
class ZeroGhost implements EdgePolicy<vector<int, size 1>> {
    fn remap(vector<int, size 1> coord) → vector<int, size 1> {
        return coord
    }
}

@ kernel
@ public
fn map_edges<size N>(
    tensor<f32, [N]> input,
    mut tensor<f32, [N]> output
) → void {
    let tensor<f32, [N]> mapped ← input.shift([1]).edge<ZeroGhost>()
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
    assert_eq!(
        issue_arg(&fixture, "nucleum_trapping_store", "width"),
        Some("i32"),
        "the i64 tensor<int, [32]> parameter is legal; the i32 store is not: {:?}",
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

/// Assert that `name` checks clean and carries `width` as a per-target fact:
/// admitted on Metal, refused by name (never a check error) on WGSL and AMD.
fn assert_wide_width_is_a_capability_fact(
    fixture: &crate::test_support::AnalyzedFixture,
    name: &str,
    width: NumericWidth,
) {
    assert!(fixture.is_ok(), "{name}: {:?}", fixture.diagnostics);
    let metal = target_row(fixture, name, Target::MirMetal);
    assert!(metal.admitted, "{name}: {:?}", metal.failures);
    assert!(
        metal.widths.iter().any(|row| row.width == width),
        "{name}: missing a {width:?} widths row"
    );
    for target in [Target::MirWgsl, Target::MirAmd] {
        let row = target_row(fixture, name, target);
        assert!(!row.admitted, "{target:?} admitted {name}");
        assert!(
            row.failures.iter().any(|failure| matches!(
                failure,
                KernelRequirementFailure::UnsupportedIntegerWidth { width: refused, .. }
                    if *refused == width
            )),
            "{target:?} {name} must refuse {width:?} by name: {:?}",
            row.failures
        );
    }
}

#[test]
fn wide_integer_kernel_values_pass_check_and_report_per_target() {
    // Ruling 2026-10-28: i64, u64 and wrapping<u64> are kernel values.
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn k_int(int x) → int {
    return x
}

@ kernel
@ public
fn k_i64(i64 x) → i64 {
    return x
}

@ kernel
@ public
fn k_u64(u64 x) → u64 {
    return x
}

@ kernel
@ public
fn k_wrap(wrapping<u64> a, wrapping<u64> b) → wrapping<u64> {
    return a + b
}

@ kernel
@ public
fn k_tensor(tensor<u64, [4]> src, mut tensor<u64, [4]> dst) → void {
    for from src at [i] thread lane let v {
        dst[[i]] ← v
    }
}
main {}
",
    );
    assert_wide_width_is_a_capability_fact(&fixture, "k_int", NumericWidth::I64);
    assert_wide_width_is_a_capability_fact(&fixture, "k_i64", NumericWidth::I64);
    assert_wide_width_is_a_capability_fact(&fixture, "k_u64", NumericWidth::U64);
    assert_wide_width_is_a_capability_fact(&fixture, "k_wrap", NumericWidth::U64);
    assert_wide_width_is_a_capability_fact(&fixture, "k_tensor", NumericWidth::U64);
}

#[test]
fn saturating_u64_passes_check_and_keeps_its_lowering_deferral() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn k(saturating<u64> x) → saturating<u64> {
    return x
}
main {}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    let metal = target_row(&fixture, "k", Target::MirMetal);
    assert!(
        metal.failures.iter().all(|failure| matches!(
            failure,
            KernelRequirementFailure::SaturatingLoweringDeferred { .. }
        )),
        "the only Metal blocker is the saturating deferral every width has: {:?}",
        metal.failures
    );
    let wgsl = target_row(&fixture, "k", Target::MirWgsl);
    assert!(wgsl.failures.iter().any(|failure| matches!(
        failure,
        KernelRequirementFailure::UnsupportedIntegerWidth {
            width: NumericWidth::U64,
            ..
        }
    )));
}

#[test]
fn d64_kernel_values_pass_check_and_report_per_target() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn k_id(d64 x) → d64 {
    return x
}

@ kernel
@ public
fn k_sum(tensor<d64, [4]> a, tensor<d64, [4]> b, mut tensor<d64, [4]> out) → void {
    for from a at [i] thread lane let x {
        out[[i]] ← x + b[[i]]
    }
}

@ kernel
@ public
fn k_mul(d64 a, d64 b) → d64 {
    return a * b
}
main {}
",
    );
    assert_wide_width_is_a_capability_fact(&fixture, "k_id", NumericWidth::D64);
    assert_wide_width_is_a_capability_fact(&fixture, "k_sum", NumericWidth::D64);
    assert_wide_width_is_a_capability_fact(&fixture, "k_mul", NumericWidth::D64);
}

#[test]
fn d64_kernel_carrier_bounds_defer_possible_overflow_to_runtime() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn decision_step(d64 dx, d64 dy, d64 dz) → d64 {
    return dx * dx + dy * dy + dz * dz
}

@ kernel
@ public
fn product(d64 a, d64 b) → d64 {
    return a * b
}
main {}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    for name in ["decision_step", "product"] {
        let metal = target_row(&fixture, name, Target::MirMetal);
        assert!(metal.admitted, "{name}: {:?}", metal.failures);
    }
}

#[test]
fn constant_d64_intermediate_proven_to_overflow_remains_a_check_error() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn constant_product() → d64 {
    return (90000000000 ↦ d64) * (90000000000 ↦ d64) * (90000000000 ↦ d64)
}
main {}
",
    );
    assert!(
        fixture
            .diagnostics
            .iter()
            .any(|error| error.issue() == Some("nucleum_wide_intermediate")),
        "{:?}",
        fixture.diagnostics
    );
}

#[test]
fn integer_intermediate_beyond_64_bits_is_a_check_error() {
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn sum(u64 a, u64 b) → u64 {
    return a + b
}
main {}
",
    );
    assert_eq!(
        issue_arg(&fixture, "nucleum_wide_intermediate", "extent"),
        Some("0..36893488147419103230"),
        "{:?}",
        fixture.diagnostics
    );
}

#[test]
fn f64_kernel_values_are_still_check_errors() {
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
fn exact_intermediate_beyond_32_bits_is_a_per_target_fact() {
    // `(left + right) / 2` over wrapping<i32> is 33 bits wide: exact in the
    // ether (ruling 17), so legal Faber; it needs a 64-bit lane, which every
    // target row refuses by name until its emitter computes one.
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
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    for target in Target::DEVICE_TARGETS {
        let row = target_row(&fixture, "half", target);
        assert!(
            row.failures.iter().any(|failure| matches!(
                failure,
                KernelRequirementFailure::UnsupportedIntegerWidth {
                    width: NumericWidth::I64,
                    ..
                }
            )),
            "{target:?}: {:?}",
            row.failures
        );
    }
}

#[test]
fn wide_intermediate_in_its_own_64_bit_type_is_native_on_metal_only() {
    // The masked sum is 33 bits wide and the expression is an `i64`: the
    // 64-bit lane is the expression's own type, so Metal computes it in a
    // `long` and admits it; WGSL and AMD have no 64-bit lane and refuse it.
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn sum(i64 left, i64 right) → i64 {
    return (left ∧ 4294967295) + (right ∧ 4294967295)
}

@ kernel
@ public
fn narrow(u32 left, u32 right) → u32 {
    return ((left ∧ 16777215) * (right ∧ 16777215)) / 1048576
}
main {}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    let metal = target_row(&fixture, "sum", Target::MirMetal);
    assert!(metal.admitted, "{:?}", metal.failures);
    for target in [Target::MirWgsl, Target::MirAmd] {
        let row = target_row(&fixture, "sum", target);
        assert!(!row.admitted, "{target:?} admitted a 64-bit lane");
    }
    // A narrow expression holding a 48-bit intermediate needs a widening no
    // emitter performs: every row, Metal included, refuses it by name.
    for target in Target::DEVICE_TARGETS {
        let row = target_row(&fixture, "narrow", target);
        assert!(
            row.failures.iter().any(|failure| matches!(
                failure,
                KernelRequirementFailure::UnsupportedIntegerWidth { .. }
            )),
            "{target:?}: {:?}",
            row.failures
        );
    }
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
    // The address stays on the 32-bit device index path: a 33-bit coordinate
    // is neither proven nor a value-width question.
    assert!(
        fixture
            .diagnostics
            .iter()
            .any(|error| error.issue() == Some("nucleum_extent_unproven")),
        "{:?}",
        fixture.diagnostics
    );
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
    // `((x ⤒ 0) + 1) * 3` is 33 bits: legal Faber, so it is no longer an
    // out-of-range intermediate; it needs a 64-bit lane (refused by name per
    // target) and its i32 return store is not proven.
    let bad = target_row(&fixture, "bad", Target::MirMetal);
    assert!(!bad.admitted);
    assert!(
        bad.failures
            .iter()
            .any(|failure| matches!(failure, KernelRequirementFailure::StoreOutOfRange { .. }))
    );
    assert!(bad.failures.iter().any(|failure| matches!(
        failure,
        KernelRequirementFailure::UnsupportedIntegerWidth { .. }
    )));
    assert!(!bad.failures.iter().any(|failure| matches!(
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
    return reduce via sum from a at [i] per [keys[i]]
}
@ kernel
@ public
fn too_many(tf32[65536, 65536] a) → tensor<i32, [1]> {
    return reduce via count from a at [i, j] per [0] thread lane let s { return s ≻ 0.0 }
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
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, size 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock) → vector<f32, size 3> {
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
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, size 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
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
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, size 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock) → vector<f32, size 3> {
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
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, size 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock) → vector<f32, size 3> {
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
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, size 3>
}

@ kernel
@ public
fn caller(tensor<f32, [24]> clock) → vector<f32, size 3> {
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
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, size 3>
}

@ kernel
@ public
fn caller_whole(tensor<f32, [24]> clock) → vector<f32, size 3> {
    return helper(clock, params.CLK_T_WHOLE)
}

@ kernel
@ public
fn caller_frac(tensor<f32, [24]> clock) → vector<f32, size 3> {
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
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, size 3>
}

@ kernel
@ public
fn caller_om(tensor<f32, [24]> clock) → vector<f32, size 3> {
    return helper(clock, 8)
}

@ kernel
@ public
fn caller_al(tensor<f32, [24]> clock) → vector<f32, size 3> {
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
fn helper(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
    return [clock[base + base], clock[base + base + 1], clock[base + base + 2]] ↦ vector<f32, size 3>
}

@ kernel
@ public
fn caller_const(tensor<f32, [24]> clock) → vector<f32, size 3> {
    return helper(clock, 8)
}

@ kernel
@ public
fn caller_dynamic(tensor<f32, [24]> clock, i32 base) → vector<f32, size 3> {
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

#[test]
fn size_name_read_in_a_kernel_is_an_i32_extent_not_a_wide_integer() {
    // A size name read in a kernel body is in the lane-coordinate carrier
    // (`i32`), bounded by the launch extent: it composes with the lane
    // binders in a guard, in arithmetic stored to an `i32`, and as a float.
    let fixture = analyze_en(
        r"
@ kernel
@ public
fn edges<size H, size W>(tensor<f32, [H, W]> src, mut tensor<f32, [H, W]> dst) → void {
    for from src at [i, j] thread lane let c {
        mut f32 r ← c
        if j ≥ W - 2 then r ← r + 1000.0
        if i ≡ H - 1 then r ← r + 10.0
        let i32 q ← (j * 3) ⤓ (W - 2)
        dst[[i, j]] ← r + (q ↦ f32) + (W ↦ f32)
    }
}
main {}
",
    );
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    let row = target_row(&fixture, "edges", Target::MirWgsl);
    assert!(
        row.admitted,
        "a size read is no 64-bit value, so even a 32-bit target admits it: {:?}",
        row.failures
    );
}
