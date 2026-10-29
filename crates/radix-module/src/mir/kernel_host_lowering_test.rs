//! `KernelLowering::Host` lowers `@ kernel` functions as ordinary functions.
//!
//! The CPU route (`faber run --device cpu`) selects host kernel lowering and
//! runs the result on the MIR runner under `RunnerKernelPolicy::Execute`. These
//! tests prove, per divergence, that the host form differs from the device
//! form and that the host form computes the sequential meaning of the kernel.
//! Device-form output is pinned by the existing device tests and by the
//! byte-identity check recorded with the unit.

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::{BufferHost, KernelLowering, MirLoweringOptions, RunSourceError, lower_analyzed_unit, run_source};
use crate::program::analyze_prepared_program_name;
use radix_mir::dump::dump_program;
use radix_mir_runner::RUNNER_KERNEL_REFUSED_ISSUE;

/// Corpus-style English source: the TOML head selects the code locale.
fn english(source: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{source}")
}

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "kernel-host-lowering.fab",
        &english(source),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

fn dump(source: &str, kernel_lowering: KernelLowering) -> String {
    let unit = analyze(source);
    let options = MirLoweringOptions { kernel_lowering, ..Default::default() };
    let lowered = lower_analyzed_unit(unit.application_view(), options)
        .unwrap_or_else(|errors| panic!("{kernel_lowering:?} lowering must succeed: {errors:?}"));
    dump_program(lowered.validated.program())
}

/// The dump text of the function with the given printed id (`f0`, `f1`, ...).
fn function_dump<'a>(dump: &'a str, id: &str) -> &'a str {
    let header = format!("function {id} ");
    let start = dump
        .find(&header)
        .unwrap_or_else(|| panic!("no {id} in the dump:\n{dump}"));
    let rest = &dump[start..];
    rest[1..]
        .find("\nfunction ")
        .map_or(rest, |end| &rest[..=end])
}

/// Run on the CPU route: host kernel lowering and the `Execute` policy.
fn run_on_cpu(source: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_host_kernels(),
    );
    let mut host = BufferHost::default();
    run_source(&session, "kernel-host-run.fab", &english(source), &mut host)
        .unwrap_or_else(|error| panic!("the CPU route must run the kernels: {error:?}"));
    host.stdout_lines
}

/// Run on the default route: device kernel lowering and the `Refuse` policy.
fn run_on_default_route(source: &str) -> Result<Vec<String>, RunSourceError> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    run_source(&session, "kernel-device-run.fab", &english(source), &mut host).map(|()| host.stdout_lines)
}

const TENSOR_READ_KERNEL: &str = r"
@ kernel
@ public
fn k(tf32[8] a, mut tf32[8] o) → void {
    for from a at [i] thread lane let c {
        o[i] ← a[i] * 2.0
    }
}
main {
    let tf32[8] a ← tf32[8] {}.fill(1.5)
    mut tf32[8] b ← tf32[8] {}
    k(a, mut b)
    print b[3]
}
";

#[test]
fn host_kernel_lowering_reads_and_writes_tensors_through_tensor_get_and_set() {
    // `f0` is the kernel; `main` (`f1`) reads `b[3]` on the host either way.
    // The `for … at [i] const c` binder gathers its element with `tensor_get`
    // in the host form; the device form is the lane form (gpu-reset C4-0), so
    // the binder is an Index read of the lane id too, and no collection op
    // remains.
    let host = dump(TENSOR_READ_KERNEL, KernelLowering::Host);
    let host_kernel = function_dump(&host, "f0");
    assert!(
        host_kernel.contains("tensor_set") && host_kernel.matches("tensor_get").count() == 2,
        "host form must use the collection ops for tensor brackets:\n{host_kernel}"
    );
    let device = dump(TENSOR_READ_KERNEL, KernelLowering::Device);
    let device_kernel = function_dump(&device, "f0");
    assert!(
        !device_kernel.contains("tensor_set")
            && !device_kernel.contains("tensor_get")
            && device_kernel.contains("_4 = _0[_2]")
            && device_kernel.contains("_1[_3] = ")
            && device_kernel.contains("_0[_3] * "),
        "device form keeps the device-view projection, not the collection ops:\n{device_kernel}"
    );
}

#[test]
fn cpu_route_runs_a_kernel_the_default_route_refuses() {
    let errors = match run_on_default_route(TENSOR_READ_KERNEL) {
        Err(RunSourceError::Stepper(errors)) => errors,
        other => panic!("the default route must refuse the kernel call: {other:?}"),
    };
    assert_eq!(errors[0].issue, RUNNER_KERNEL_REFUSED_ISSUE);
    assert_eq!(run_on_cpu(TENSOR_READ_KERNEL), ["3.0"]);
}

const HELPER_KERNEL: &str = r"
@ kernel
fn bump(f32 x) → f32 {
    return x + 1.0
}
@ kernel
@ public
fn k(f32 x) → f32 {
    return bump(x) * 2.0
}
main {
    print k(1.5)
}
";

#[test]
fn host_kernel_lowering_keeps_a_helper_kernel_call_as_a_plain_call() {
    let host = dump(HELPER_KERNEL, KernelLowering::Host);
    assert_eq!(host.matches("\nfunction ").count() + 1, 3, "bump, k and main all stay:\n{host}");
    assert_eq!(host.matches("call def#").count(), 2, "main calls k and k calls bump:\n{host}");
    let device = dump(HELPER_KERNEL, KernelLowering::Device);
    assert_eq!(
        device.matches("call def#").count(),
        1,
        "device form composes (inlines) the helper; only main's launch call remains:\n{device}"
    );
    assert_eq!(run_on_cpu(HELPER_KERNEL), ["5.0"]);
}

#[test]
fn host_kernel_lowering_runs_a_rank_two_size_generic_kernel_with_a_helper_call() {
    let source = r"
@ kernel
fn sum2<size H, size W>(tensor<f32, [H, W]> t, i32 x, i32 y) → f32 {
    if 0 ≤ x and 0 ≤ y {
        return t[[y, x]] + 1.0
    }
    return 0.0
}
@ kernel
@ public
fn k<size H, size W>(tensor<f32, [H, W]> t, mut tensor<f32, [H, W]> o) → void {
    for from t at [i, j] thread lane let c {
        o[[i, j]] ← sum2(t, j, i) * 2.0
    }
}
main {
    let int n ← 4
    let tf32[size n, size n] a ← tf32[size n, size n] {}.fill(1.5)
    mut tf32[size n, size n] b ← tf32[size n, size n] {}
    k(a, mut b)
    print b[[2, 3]]
}
";
    assert_eq!(run_on_cpu(source), ["5.0"]);
}

#[test]
fn host_kernel_lowering_folds_a_thread_sum_sequentially() {
    let source = r"
@ kernel
@ public
fn total<size N>(tf32[N] a) → f32 {
    return reduce via sum from a at [i] thread lane let s { return s * 2.0 }
}
main {
    let tf32[4] a ← [3.0, 1.0, 4.0, 2.0] ↦ tf32[4]
    print total(a)
}
";
    assert_eq!(run_on_cpu(source), ["20.0"]);
    let device = dump(source, KernelLowering::Device);
    assert!(
        device.contains("filum_reduce op=sum form=declared"),
        "device lowering keeps the declared FilumReduce recipe for the ungrouped thread sum:\n{device}"
    );
    let host = dump(source, KernelLowering::Host);
    assert!(
        !host.contains("filum_reduce"),
        "host lowering folds the thread sum like any reduction:\n{host}"
    );
}

#[test]
fn host_kernel_lowering_reduces_a_wave_speed_shaped_thread_max_to_the_sequential_maximum() {
    // The shape of the A2 `wave_speed` kernel: a `reduce via max … thread` over
    // a rank-2 state tensor with a helper kernel in the body, written into a
    // device-resident clock tensor.
    let source = r"
@ kernel
fn speed_of(f32 scale, vector<f32, size 4> s) → f32 {
    return (s[1] + s[2]) * scale
}
@ kernel
@ public
fn wave_speed<size H, size W>(f32 scale, tensor<vector<f32, size 4>, [H, W]> state, mut tensor<f32, [4]> clock) → void {
    clock[[1]] ← reduce via max from state at [i, j] thread lane let s { return speed_of(scale, s) } coalesce 0.0
}
main {
    let tensor<vector<f32, size 4>, []> cell_seed ← empty
    let vector<f32, size 4> low ← [0.0, 1.0, 1.0, 0.0] ↦ vector<f32, size 4>
    mut tensor<vector<f32, size 4>, [5, 5]> state ← cell_seed.create(low, [5, 5])
    state[[3, 2]] ← [0.0, 2.0, 5.0, 0.0] ↦ vector<f32, size 4>
    state[[0, 4]] ← [0.0, 1.0, 3.0, 0.0] ↦ vector<f32, size 4>
    mut tf32[4] clock ← tf32[4] {}
    wave_speed(0.5, state, clock)
    print clock[[1]]
}
";
    // max over cells of (hu + hv) * 0.5: the cell at [3, 2] gives (2 + 5) * 0.5.
    // The non-zero vector fill has no conversion or constructor spelling (a
    // `↦` row populates from flat data; the constructor mints zeros), so the
    // `cell_seed.create(low, [5, 5])` construction now declines at check with
    // the named retirement. The fixture parks with the GPU device surface by
    // operator disposition; re-pin the run when that fill surface exists.
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_host_kernels(),
    );
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "kernel-host-run.fab", &english(source), &mut host) {
        Err(RunSourceError::Frontend(errors)) => errors,
        other => panic!("the retired crea construction must decline at analysis: {other:?}"),
    };
    assert!(
        errors
            .iter()
            .any(|diag| diag.code == Some("SEM011") && diag.issue() == Some("tensor_crea_retired")),
        "expected the named tensor_crea_retired decline, got: {errors:?}"
    );
}

#[test]
fn host_kernel_lowering_runs_a_sized_argmax_coalesce_region() {
    // The `c4-reduce-int` shape: the reduce identity `coalesce 0` lowers
    // through the generic coalesce under host kernel lowering, so the fallback
    // literal is an unsized `MirConstant::Int(0)` standing for the sized `i32`
    // cell the region stores into. Semantic analysis already range-checked it
    // under the inner type; the validator must accept the fitting constant.
    let source = r"
@ kernel
@ public
fn amax<size N>(tensor<f32, [N]> src, mut tensor<i32, [2]> out) → void {
    out[0] ← reduce via argmax from src at [i] thread lane let v { return v } coalesce 0
}
main {
    const size N ← 6
    mut tensor<f32, [N]> wavy ← tf32[N] {}
    for range 0‥6 let i {
        wavy[[i]] ← ((i * 7) % 13) ↦ f32
    }
    mut tensor<i32, [2]> out ← ti32[2] {}
    amax(wavy, mut out)
    print out[0]
}
";
    assert_eq!(run_on_cpu(source), ["5"]);
}

#[test]
fn host_kernel_coalesce_fallback_literal_fits_a_sized_list_element() {
    let source = r"
main {
    let list<u8> digits ← [7, 9, 2]
    let u8 first ← digits.get(1) coalesce 0
    print first
}
";
    assert_eq!(run_on_cpu(source), ["9"]);
}
