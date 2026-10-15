//! NAMEJOIN NJ2 close-out (risks R1/R2): REAL lowered source, not hand-built
//! MIR. A device program's buffers are wired by the VALUE the host passes
//! (one tensor as one kernel's `mut` output and the next kernel's input),
//! never by kernel parameter names.
//!
//! R1: real lowering must not route the host value through a copy between the
//! two calls (the chain would then be invisible). R2: the call-site argument
//! position must be the kernel parameter position (no hidden environment
//! parameter shifts it).

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::{KernelLowering, MirLoweringOptions, lower_analyzed_unit};
use crate::program::analyze_prepared_program_name;
use radix_mir::device::MirCompanionMap;
use radix_mir::device_program::{BufferId, DeviceProgram, DeviceProgramLifetime, build_device_program};
use radix_mir::device_semantics::DeviceSemantics;
use radix_mir::dump::dump_program;
use std::collections::BTreeSet;

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "device-value-join.fab",
        &format!("+++\nlocale = \"en\"\n+++\n{source}"),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

/// Two lane kernels and a host `main` that creates the tensor `t`, passes it
/// as `first`'s `mut` output and as `second`'s input. The parameter names of
/// both kernels are the arguments.
fn chain_source(first: [&str; 2], second: [&str; 2]) -> String {
    format!(
        r"
@ kernel
@ public
fn first(tf32[8] {a}, mut tf32[8] {b}) → void {{
    for from {a} at [i] thread lane let c {{
        {b}[i] ← {a}[i] * 2.0
    }}
}}
@ kernel
@ public
fn second(tf32[8] {c}, mut tf32[8] {d}) → void {{
    for from {c} at [i] thread lane let c2 {{
        {d}[i] ← {c}[i] + 1.0
    }}
}}
main {{
    let tf32[8] a0 ← tf32[8] {{}}.fill(1.5)
    mut tf32[8] t ← tf32[8] {{}}
    mut tf32[8] r ← tf32[8] {{}}
    first(a0, mut t)
    second(t, mut r)
    print r[3]
}}
",
        a = first[0],
        b = first[1],
        c = second[0],
        d = second[1],
    )
}

fn build(source: &str) -> (DeviceProgram, DeviceSemantics, String) {
    let unit = analyze(source);
    let options = MirLoweringOptions { kernel_lowering: KernelLowering::Device, ..Default::default() };
    let lowered = lower_analyzed_unit(unit.application_view(), options)
        .unwrap_or_else(|errors| panic!("device lowering must succeed: {errors:?}"));
    let dump = dump_program(lowered.validated.program());
    let (program, semantics, _steps) = build_device_program(
        &lowered.validated,
        &lowered.interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
        0,
        &lowered.device_function_metadata,
    )
    .unwrap_or_else(|error| panic!("the device program must build: {error:?}\n{dump}"));
    (program, semantics, dump)
}

fn blank_names(program: &mut DeviceProgram, semantics: &mut DeviceSemantics) {
    for kernel in &mut program.kernels {
        for resource in &mut kernel.resources {
            resource.buffer.name.clear();
        }
    }
    for result in &mut program.results {
        result.buffer.name.clear();
    }
    for value in &mut semantics.values {
        value.name.clear();
    }
}

fn buffer_ids(program: &DeviceProgram, entry: &str) -> Vec<BufferId> {
    let kernel = program
        .kernels
        .iter()
        .find(|kernel| kernel.entry == entry)
        .unwrap_or_else(|| panic!("kernel {entry} is in the program"));
    kernel
        .resources
        .iter()
        .map(|resource| resource.buffer.id)
        .collect()
}

#[test]
fn real_source_chain_shares_one_buffer_by_host_value_and_survives_renaming() {
    let (program, semantics, dump) = build(&chain_source(["x", "y"], ["p", "q"]));

    // R1: the host `main` calls the kernels with the SAME bare local; real
    // lowering inserts no copy between the two calls.
    let main_start = dump
        .find("function f2 ")
        .unwrap_or_else(|| panic!("no host function:\n{dump}"));
    let main = &dump[main_start..];
    // Real lowering calls a kernel through `MirCallee::Definition`
    // (`call def#N`), and passes the tensor local itself: `_1` is the first
    // call's output and the second call's input, with no copy in between.
    let calls: Vec<&str> = main
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("call def#"))
        .collect();
    assert_eq!(calls.len(), 2, "{dump}");
    assert!(calls[0].ends_with("(_0, _1)") && calls[1].ends_with("(_1, _2)"), "{dump}");

    // R2: the `mut` output is bound at argument position 1 of `first`; the
    // consumer's input is argument position 0 of `second`. Entry names are
    // compiler-owned `k<id>` (ca2499dc-3.1), in program order.
    let first = buffer_ids(&program, "k0");
    let second = buffer_ids(&program, "k1");
    assert_eq!(first.len(), 2, "first(a, mut o): {first:?}\n{dump}");
    assert_eq!(second.len(), 2, "second(src, mut dst): {second:?}\n{dump}");
    assert_eq!(
        first[1], second[0],
        "first's output and second's input are ONE buffer (the host value t):\n{dump}"
    );
    let all: BTreeSet<BufferId> = first.iter().chain(&second).copied().collect();
    assert_eq!(all.len(), 3, "a, t (shared) and r: {first:?} {second:?}\n{dump}");
    assert_eq!(
        semantics.dependencies.len(),
        1,
        "one producer→consumer edge: {:?}",
        semantics.dependencies
    );
    assert_eq!(program.launches.len(), 2);

    // Renaming the second kernel's parameters changes nothing but the names.
    let (mut renamed, mut renamed_semantics, _) = build(&chain_source(["x", "y"], ["in_", "out_"]));
    let (mut original, mut original_semantics) = (program, semantics);
    blank_names(&mut original, &mut original_semantics);
    blank_names(&mut renamed, &mut renamed_semantics);
    assert_eq!(original, renamed);
    assert_eq!(original_semantics, renamed_semantics);
}
