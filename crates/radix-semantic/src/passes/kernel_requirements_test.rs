//! Per-target width, range, call-graph, and callback requirements.

use crate::passes::kernel_requirements::KernelRequirementFailure;
use crate::test_support::analyze_source;
use radix_target::Target;

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
    let fixture = analyze_source(
        r"
@ nucleum
@ publica
functio wrapping_sum(wrapping<i32> left, wrapping<i32> right) → wrapping<i32> {
    redde left + right
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
fn symbolic_apud_coordinate_is_retained_as_a_store_condition() {
    let fixture = analyze_source(
        r"
@ nucleum
@ publica
functio copy_coordinate<magnitudo N>(
    tensor<f32, [N]> input,
    tensor<u32, [N]> output
) → vacuum {
    itera ex input apud [i] fixum value {
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
functio wide_helper(int value) → vacuum {
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
    let fixture = analyze_source(
        r"
@ nucleum
@ publica
functio tensor_division(
    tensor<wrapping<i32>, [4]> dividend,
    tensor<wrapping<i32>, [4]> divisor
) → vacuum {
    fixum tensor<wrapping<i32>, [4]> quotient ← dividend.divisio(divisor)
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
    let fixture = analyze_source(
        r"
interface EdgePolicy<C> { fn remap(C coord) → C }
class ZeroGhost implements EdgePolicy<vector<int, 1>> {
    fn remap(vector<int, 1> coord) → vector<int, 1> {
        return coord
    }
}

@ nucleum
@ publica
functio map_edges<magnitudo N>(
    tensor<f32, [N]> input,
    tensor<f32, [N]> output
) → vacuum {
    fixum tensor<f32, [N]> mapped ← input.shift([1]).limes<ZeroGhost>()
    itera ex mapped apud [i] fixum value {
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
