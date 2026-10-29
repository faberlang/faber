//! Control-flow lowering behavior: enum-member iteration, `or` alternatives
//! in `switch` and `match` arms, the kernel `⊥` default, and the grouped
//! (`per [...]`) grid reduction with its key-read proof.
//!
//! Each test analyzes real source and either runs it on the MIR runner (the
//! observable output is the contract) or lowers a kernel and checks the MIR
//! structure or the named refusal. Refusals are asserted by `issue`, never by
//! message text.

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::{BufferHost, KernelLowering, MirError, MirLoweringOptions, lower_analyzed_unit, run_analyzed};
use crate::program::analyze_prepared_program_name;
use radix_mir::dump::dump_program;

fn english(source: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{source}")
}

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "control-coverage.fab",
        &english(source),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

fn run(source: &str) -> Vec<String> {
    let mut unit = analyze(source);
    let mut host = BufferHost::default();
    run_analyzed(&mut unit, &mut host).unwrap_or_else(|errors| panic!("source must run: {errors:?}"));
    host.stdout_lines
}

fn lower(source: &str, kernel_lowering: KernelLowering) -> Result<String, Vec<MirError>> {
    let unit = analyze(source);
    let options = MirLoweringOptions { kernel_lowering, ..Default::default() };
    lower_analyzed_unit(unit.application_view(), options).map(|lowered| dump_program(lowered.validated.program()))
}

fn issues(errors: &[MirError]) -> Vec<&str> {
    errors.iter().map(|error| error.issue.as_str()).collect()
}

#[test]
fn iterating_an_enum_walks_its_members_in_declaration_order() {
    let source = r"
enum Status {
    pending = 10,
    active,
    done = -3
}

main {
    for from Status let s {
        switch s {
            case pending { print 1 }
            case active { print 2 }
            case done { print 3 }
        }
    }
}
";
    assert_eq!(run(source), ["1", "2", "3"]);
}

#[test]
fn switch_literal_arms_fall_through_to_the_default() {
    let source = r#"
fn bucket(int n) → string {
    switch n {
        case 1 { return "low" }
        default { return "other" }
    }
}

main {
    print bucket(1)
    print bucket(2)
    print bucket(9)
}
"#;
    assert_eq!(run(source), ["low", "other", "other"]);
}

#[test]
fn match_arm_alternatives_over_variants_share_one_body() {
    let source = r#"
union T {
    A,
    B,
    C { int n },
    D
}

fn f(T t) → string {
    match t {
        case A or B { return "ab" }
        case C let n { return "c§"(n) }
        case D { return "d" }
    }
}

main {
    print f(variant A { })
    print f(variant B { })
    print f(variant C { n = 3 })
    print f(variant D { })
}
"#;
    assert_eq!(run(source), ["ab", "ab", "c3", "d"]);
}

#[test]
fn match_arm_alternatives_over_payload_variants_bind_the_shared_name() {
    let source = r#"
union Shape {
    Circle { int size },
    Square { int size },
    Dot
}

fn measure(Shape s) → int {
    match s {
        case Circle let c or Square let c { return 10 }
        case Dot { return 0 }
    }
}

main {
    print measure(variant Circle { size = 2 })
    print measure(variant Square { size = 3 })
    print measure(variant Dot { })
}
"#;
    assert_eq!(run(source), ["10", "10", "0"]);
}

const CALL_DEFAULT_KERNEL: &str = r"
@ kernel
@ public
fn k(tensor<f32, [_]> src, mut tensor<f32, [_]> dst) → void {
    for from src at [i] thread lane let v {
        dst[[i]] ← src.get([i + 1]) ⊥ 0.0
    }
}
main {}
";

const CALL_DEFAULT_RANK2_KERNEL: &str = r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane let v {
        dst[[i, j]] ← src.get([i, j + 1]) ⊥ 0.0
    }
}
main {}
";

#[test]
fn kernel_call_default_selects_by_range_on_device_and_host() {
    for source in [CALL_DEFAULT_KERNEL, CALL_DEFAULT_RANK2_KERNEL] {
        for lowering in [KernelLowering::Device, KernelLowering::Host] {
            let dump = lower(source, lowering).unwrap_or_else(|errors| panic!("{lowering:?}: {errors:?}"));
            assert!(dump.contains("branch"), "{lowering:?}:\n{dump}");
        }
    }
}

const GROUPED_KERNEL: &str = r"
@ kernel
@ public
fn k(tensor<f32, [32]> weights, tensor<i32, [32]> rows, tensor<i32, [32]> cols, mut tensor<f32, [2, 3]> out) → void {
    let tensor<f32, [2, 3]> grouped ← reduce via sum from weights at [i] per [rows[i], cols[i]] thread lane let s { return s }
    out ⇇ grouped
}
main {}
";

#[test]
fn grouped_grid_reduction_lowers_one_key_function_per_result_axis() {
    let dump = lower(GROUPED_KERNEL, KernelLowering::Device).unwrap_or_else(|errors| panic!("{errors:?}"));
    assert!(dump.contains("filum_reduce op=sum form=grid"), "{dump}");
}

#[test]
fn grouped_grid_reduction_by_the_loop_coordinates_lowers() {
    let source = r"
@ kernel
@ public
fn k(tensor<f32, [2, 2, 32]> cube, mut tensor<f32, [2, 2]> out) → void {
    let tensor<f32, [2, 2]> grouped ← reduce via sum from cube at [i, j, l] per [i, j] thread lane let s { return s }
    out ⇇ grouped
}
main {}
";
    let dump = lower(source, KernelLowering::Device).unwrap_or_else(|errors| panic!("{errors:?}"));
    assert!(dump.contains("filum_reduce op=sum form=grid"), "{dump}");
}

fn unproven_key_kernel(key: &str, key_param: &str, source_shape: &str, coordinates: &str) -> String {
    format!(
        r"
@ kernel
@ public
fn k(tensor<f32, {source_shape}> weights, {key_param} rows, mut tensor<f32, [2]> out) → void {{
    let tensor<f32, [2]> grouped ← reduce via sum from weights at {coordinates} per [{key}] thread lane let s {{ return s }}
    out ⇇ grouped
}}
main {{}}
"
    )
}

#[test]
fn grouped_key_read_whose_axis_differs_from_the_source_axis_is_refused_by_name() {
    // `rows` has 32 entries; the key reads it with the second source axis (64).
    let source = unproven_key_kernel("rows[j]", "tensor<i32, [32]>", "[32, 64]", "[i, j]");
    let errors = lower(&source, KernelLowering::Device).expect_err("the key read is not proven in range");
    assert!(
        issues(&errors).contains(&"grouped_grid_reduction_key_index_unproven"),
        "{errors:?}"
    );
}

#[test]
fn grouped_key_read_with_a_computed_index_is_refused_by_name() {
    let source = unproven_key_kernel("rows[i + 1]", "tensor<i32, [32]>", "[32]", "[i]");
    let errors = lower(&source, KernelLowering::Device).expect_err("the key read is not proven in range");
    assert!(
        issues(&errors).contains(&"grouped_grid_reduction_key_index_unproven"),
        "{errors:?}"
    );
}

#[test]
fn grouped_key_read_through_a_key_tensor_of_another_extent_is_refused_by_name() {
    let source = unproven_key_kernel("rows[i]", "tensor<i32, [64]>", "[32]", "[i]");
    let errors = lower(&source, KernelLowering::Device).expect_err("the key read is not proven in range");
    assert!(
        issues(&errors).contains(&"grouped_grid_reduction_key_index_unproven"),
        "{errors:?}"
    );
}

#[test]
fn a_thread_first_reduction_in_a_value_function_stays_a_sequential_fold() {
    let source = r"
@ kernel
@ public
fn k(tensor<f32, [32]> a) → f32 ∪ none {
    return reduce via first from a at [i] thread lane let s { return s ≻ 0.5 }
}
main {}
";
    let dump = lower(source, KernelLowering::Device).unwrap_or_else(|errors| panic!("{errors:?}"));
    assert!(!dump.contains("filum_reduce"), "{dump}");
}
