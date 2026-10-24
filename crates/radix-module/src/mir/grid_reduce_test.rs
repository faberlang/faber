//! Whole-grid reduction lane region (gpu-reset C4-8 U1): a top-level
//! `dest ← reduce via Op from t at [i, j] thread lane const v { ... }` lowers
//! to the Lane parameter, the extents, ONE `FilumReduce` node (form `grid`),
//! and the ordinary destination store; no grid guard.

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::lower::lane_form::LANE_FORM_NESTED_REDUCE_ISSUE;
use crate::mir::{KernelLowering, MirError, MirLoweringOptions, lower_analyzed_unit};
use crate::program::analyze_prepared_program_name;
use radix_mir::MirParamMode;
use radix_mir::dump::dump_program;

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "grid-reduce.fab",
        &format!("+++\nlocale = \"en\"\n+++\n{source}"),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

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

/// The block lines of `f0` with the `ty#N` suffixes dropped.
fn blocks_of_f0(dump: &str) -> Vec<String> {
    let start = dump.find("function f0 ").expect("f0 in the dump");
    let function = &dump[start..];
    let function = &function[..function.find("\n}\n").map_or(function.len(), |end| end + 2)];
    let blocks = &function[function.find("  bb0:").expect("f0 has blocks")..];
    blocks
        .lines()
        .filter(|line| *line != "}")
        .map(|line| {
            let line = line.split(" -> ty#").next().unwrap_or(line);
            line.split(": ty#")
                .next()
                .unwrap_or(line)
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn reduce_kernel(op: &str, element: &str) -> String {
    let coalesce = if op == "max" { " coalesce 0.0" } else { "" };
    format!(
        r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> state, mut tensor<{element}, [4]> out) → void {{
    out[0] ← reduce via {op} from state at [i, j] thread lane let s {{ return s * 2.0 }}{coalesce}
}}
main {{}}
"
    )
}

#[test]
fn max_over_rank_two_lowers_to_the_grid_node_without_a_guard() {
    let (dump, modes) = lower(&reduce_kernel("max", "f32"), KernelLowering::Device).expect("device lowering validates");
    assert_eq!(modes, [MirParamMode::Owned, MirParamMode::MutRef, MirParamMode::Lane]);
    let lines = blocks_of_f0(&dump);
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("filum_reduce"))
            .count(),
        1,
        "{dump}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("filum_reduce op=max form=grid lane_width=256")),
        "{dump}"
    );
    assert!(lines.iter().any(|line| line.contains("extent _0 axis 0")), "{dump}");
    assert!(lines.iter().any(|line| line.contains("extent _0 axis 1")), "{dump}");
    assert!(!lines.iter().any(|line| line.contains("branch")), "no grid guard:\n{dump}");
    let node = lines
        .iter()
        .position(|line| line.contains("filum_reduce"))
        .unwrap();
    assert!(
        lines[node..]
            .iter()
            .any(|line| line.contains("_1[") && line.contains(" = ")),
        "the destination store follows the node as an ordinary Index store:\n{dump}"
    );
}

#[test]
fn sum_count_and_argmax_lower_to_their_ops() {
    for (op, element, needle) in [
        ("sum", "f32", "op=sum form=grid"),
        ("count", "i32", "op=count form=grid"),
        ("argmax", "i32", "op=argmax form=grid"),
    ] {
        let source = if op == "argmax" {
            format!(
                r"
@ kernel
@ public
fn k(tensor<f32, [_]> state, mut tensor<{element}, [4]> out) → void {{
    out[0] ← reduce via {op} from state at [i] thread lane let s {{ return s }} coalesce 0
}}
main {{}}
"
            )
        } else if op == "count" {
            format!(
                r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> state, mut tensor<{element}, [4]> out) → void {{
    out[0] ← reduce via {op} from state at [i, j] thread lane let s {{ return s ≻ 0.0 }}
}}
main {{}}
"
            )
        } else {
            reduce_kernel(op, element)
        };
        let (dump, _) = lower(&source, KernelLowering::Device).unwrap_or_else(|errors| panic!("{op}: {errors:?}"));
        assert!(dump.contains(needle), "{op}:\n{dump}");
    }
}

#[test]
fn host_form_keeps_the_sequential_fold() {
    let (dump, modes) = lower(&reduce_kernel("max", "f32"), KernelLowering::Host).expect("host lowering validates");
    assert_eq!(
        modes,
        [MirParamMode::Owned, MirParamMode::MutRef],
        "host form has no lane parameter"
    );
    assert!(!dump.contains("filum_reduce"), "{dump}");
}

#[test]
fn a_reduce_nested_in_a_lane_loop_is_refused_by_name() {
    let source = r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> state, mut tensor<f32, [_, _]> out) → void {
    for from state at [i, j] thread lane let v {
        out[[i, j]] ← reduce via max from state at [a, b] thread lane let s { return s } coalesce 0.0
    }
}
main {}
";
    let errors = lower(source, KernelLowering::Device).expect_err("nested reduce is refused");
    assert!(
        errors
            .iter()
            // The issue is the refusal's identity (one producer, one prose);
            // the definition span points at the enclosing lane region.
            .any(|error| error.issue == LANE_FORM_NESTED_REDUCE_ISSUE && error.definition_span.is_some()),
        "{errors:?}"
    );
}

const SECOND_SOURCE: &str = r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> mask, tensor<f32, [_, _]> state, mut tensor<f32, [4]> out) → void {
    out[0] ← reduce via max from state at [i, j] thread lane let s { return s * 2.0 } coalesce 0.0
}
main {}
";

const LOOP_KERNEL: &str = r"
@ kernel
@ public
fn k(tensor<f32, [_, _]> src, mut tensor<f32, [_, _]> dst) → void {
    for from src at [i, j] thread lane let v {
        dst[[i, j]] ← v
    }
}
main {}
";

fn device_program(source: &str) -> (radix_mir::device_program::DeviceProgram, radix_mir::MirKernelSignature) {
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("device lowering validates");
    let (program, _semantics, _steps) = radix_mir::device_program::build_device_program(
        &lowered.validated,
        &lowered.interner,
        &radix_mir::device::MirCompanionMap::default(),
        None,
        radix_mir::device_program::DeviceProgramLifetime::SingleRun,
        0,
        &lowered.device_function_metadata,
    )
    .expect("the lane kernel materializes");
    let signature = radix_mir::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
        &lowered.validated.program().functions[0],
        lowered.validated.validation(),
        &lowered.interner,
    )
    .expect("the lane kernel has an ABI signature");
    (program, signature)
}

#[test]
fn the_plan_names_the_reduced_tensor_not_the_first_read_buffer() {
    use radix_mir::CollectionKernelPlan;
    let (program, _) = device_program(SECOND_SOURCE);
    assert_eq!(program.kernels.len(), 1);
    let CollectionKernelPlan::GridReduce {
        source_resource,
        rank,
        group_shape,
        workgroup,
        partials_cap,
        partial_layout,
    } = &program.kernels[0].plan
    else {
        panic!("expected GridReduce, got {:?}", program.kernels[0].plan);
    };
    assert_eq!((*source_resource, *rank, *workgroup, *partials_cap), (1, 2, 256, 1024));
    assert_eq!(*group_shape, None, "an ungrouped reduce keeps the scalar layout");
    assert_eq!(partial_layout.element_bytes(), 8, "f32 value plus the seen flag");
}

#[test]
fn a_loop_kernel_still_plans_a_lane_grid() {
    use radix_mir::CollectionKernelPlan;
    let (program, _) = device_program(LOOP_KERNEL);
    assert_eq!(
        program.kernels[0].plan,
        CollectionKernelPlan::LaneGrid { source_resource: 0, rank: 2 }
    );
}

#[test]
fn the_abi_carries_the_partials_scratch_between_the_bindings_and_the_extents() {
    use radix_mir::MirKernelResourceKind as Kind;
    let (_, signature) = device_program(SECOND_SOURCE);
    let kinds: Vec<(u32, Kind)> = signature
        .resources_by_binding()
        .map(|resource| (resource.binding, resource.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (0, Kind::StorageBuffer),
            (1, Kind::StorageBuffer),
            (2, Kind::StorageBuffer),
            (3, Kind::PartialsScratch { cap: 1024, element_bytes: 8 }),
            (4, Kind::TensorAxisExtent { tensor_binding: 1, axis: 0 }),
            (5, Kind::TensorAxisExtent { tensor_binding: 1, axis: 1 }),
        ],
        "declared bindings, then the partials scratch, then the extents"
    );
}

const HELPER_TERM_SOURCE: &str = r"
@ kernel
fn clampabs(f32 v) → f32 {
    if v ≺ 0.0 then return 0.0 - v
    return v
}

@ kernel
@ public
fn k(f32 bias, tensor<f32, [_, _]> state, mut tensor<f32, [4]> out) → void {
    out[0] ← reduce via max from state at [i, j] thread lane let s { return clampabs(s - bias) } coalesce 0.0
}
main {}
";

/// The lines of the dump's function `name` (`function f1 ...` up to the next
/// function).
fn function_lines(dump: &str, name: &str) -> Vec<String> {
    let start = dump
        .find(&format!("function {name} "))
        .unwrap_or_else(|| panic!("{name} in the dump:\n{dump}"));
    let body = &dump[start..];
    let end = body[1..]
        .find("\nfunction ")
        .map_or(body.len(), |at| at + 1);
    body[..end].lines().map(str::to_owned).collect()
}

#[test]
fn a_term_with_a_helper_call_and_a_branch_lowers_to_its_own_function() {
    let (dump, _) = lower(HELPER_TERM_SOURCE, KernelLowering::Device).expect("device lowering validates");
    // f0 `clampabs` was inlined into the term and pruned; f1 is the kernel,
    // f2 the term function.
    let kernel = function_lines(&dump, "f1");
    let node = kernel
        .iter()
        .find(|line| line.contains("filum_reduce"))
        .unwrap_or_else(|| panic!("node in f1:\n{dump}"));
    // The node names the term function (its last operand) and one capture
    // (`bias`) before it; the kernel carries no flat term body.
    assert!(
        node.contains("form=grid") && node.contains("term_fn_captures=1"),
        "{node}\n{dump}"
    );
    assert!(node.contains("_0, const function f2)"), "capture, then the function:\n{node}");
    assert!(!kernel.iter().any(|line| line.contains("branch")), "{dump}");
    // The term function: `bias`, two coordinates, and the element are its
    // parameters; composition inlined `clampabs` into it, so it holds the
    // branch and the early-return join itself, and returns the term value.
    let term = function_lines(&dump, "f2");
    assert!(term[0].starts_with("function f2 -> ty#25"), "{}", term[0]);
    assert!(
        term.iter().any(|line| line.contains("branch")),
        "the helper's branch is in the term:\n{dump}"
    );
    assert!(
        !term.iter().any(|line| line.contains("call")),
        "the helper call was inlined:\n{dump}"
    );
    assert!(
        term.iter()
            .any(|line| line.trim_start().starts_with("return %")),
        "{dump}"
    );
}
