//! GT19 product-boundary AD acceptance receipts, la locale.
//!
//! GT19 AD acceptance — the rope_norm backward companion, end-to-end through
//! the driver.
//!
//! The single-file runner must install AIR backward companions just like the
//! package MIR path. This pins an annotated mean-of-product primal and its
//! interprocedural unit residual through the public source runner.

use crate::codegen::Target;
use crate::driver::{
    AnalyzeRequest, Config, Session, SpannedCodegenError, analyze_source, lower_mir_with_context_for_target,
};
use crate::mir::{BufferHost, RunSourceError, run_source};
use crate::test_support::test_session_la;

/// Unwrap a driver result, panicking with the codegen error message
/// (`SpannedCodegenError` carries no `Debug` impl).
fn expect_codegen_ok<T>(result: Result<T, SpannedCodegenError>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what}: {}", error.message),
    }
}

/// Names of every lowered MIR function, resolved through the lowering
/// interner.
fn mir_function_names(program: &crate::mir::MirProgram, interner: &crate::lexer::Interner) -> Vec<String> {
    program
        .functions
        .iter()
        .filter_map(|f| f.name.map(|symbol| interner.resolve(symbol).to_owned()))
        .collect()
}

fn gt19_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::MirFmirBinary)
            .with_dev_locale_root(),
    )
}

fn run_gt19_la(name: &str, source: &str) -> Vec<String> {
    let session = gt19_session();
    let mut host = BufferHost::default();
    match run_source(&session, name, source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("frontend should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("lowering should be clean: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("runner should run: {errors:?}"),
    }
}

fn parse_fractus_list(line: &str) -> Vec<f64> {
    let inner = line.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<f64>()
                .expect("list element must parse as fractus")
        })
        .collect()
}

/// Pinned `[2, 8]` fixture rows (mirrors `X_ROW_MAJOR` in
/// `rms_norm_intrinsic_test.rs`): row 0 is `[1.0..=8.0]`, row 1 is
/// `[0.5..=7.5]`. Both rows have non-zero mean.
const GT19_X_ROW_MAJOR: [f32; 16] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, //
    0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5,
];

/// Pinned affine gamma `[8]` (mirrors `GAMMA` in `rms_norm_intrinsic_test.rs`).
const GT19_GAMMA: [f32; 8] = [2.0, 1.0, 0.5, 1.5, 0.75, 1.25, 1.0, 0.5];

/// Analytic f64 reference for the pinned primal
/// `loss = media(x.rms_norm(axis 1, 1e-5, gamma))`. The forward mirrors
/// `rms_norm_reference` (per-row sum-of-squares mean, `r = 1/sqrt(mean+eps)`,
/// `y = x·r·gamma`); the chain rule through the full-tensor mean gives the
/// uniform upstream `1/(B·D)`, and per row with `s = mean(x²)+eps`:
///
/// - `dL/dx_i = gamma_i·u·r − x_i·r³·(Σ_j u·gamma_j·x_j)/n`
/// - `dL/dgamma_i = u·x_i·r`
fn gt19_rms_norm_mean_backward_reference(rows: usize, cols: usize) -> (f64, Vec<f64>, Vec<f64>) {
    const EPS: f64 = 1e-5;
    let n = f64::from(cols as u32);
    let upstream = 1.0 / f64::from(rows as u32 * cols as u32);
    let mut loss_sum = 0.0;
    let mut dx = vec![0.0; rows * cols];
    let mut dgamma = vec![0.0; cols];
    for row in 0..rows {
        let base = row * cols;
        let mean = GT19_X_ROW_MAJOR[base..base + cols]
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
            / n;
        let r = 1.0 / (mean + EPS).sqrt();
        let dot = (0..cols)
            .map(|j| upstream * f64::from(GT19_GAMMA[j]) * f64::from(GT19_X_ROW_MAJOR[base + j]))
            .sum::<f64>();
        for (j, cell) in dx[base..base + cols].iter_mut().enumerate() {
            let xi = f64::from(GT19_X_ROW_MAJOR[base + j]);
            *cell = f64::from(GT19_GAMMA[j]) * upstream * r - xi * r * r * r * dot / n;
            dgamma[j] += upstream * xi * r;
            loss_sum += xi * r * f64::from(GT19_GAMMA[j]);
        }
    }
    let loss = loss_sum / f64::from(rows as u32 * cols as u32);
    (loss, dx, dgamma)
}

/// Per-element close check (`atol = rtol = 1e-4`, the numeric-policy
/// gradient-row form used by `check_close` in `rms_norm_intrinsic_test.rs`).
fn gt19_check_close(name: &str, observed: &[f64], expected: &[f64]) {
    const ATOL: f64 = 1e-4;
    const RTOL: f64 = 1e-4;
    assert_eq!(observed.len(), expected.len(), "{name}: length mismatch");
    for (index, (observed, expected)) in observed.iter().zip(expected).enumerate() {
        assert!(
            (observed - expected).abs() <= ATOL + RTOL * expected.abs(),
            "{name}[{index}]: observed {observed} vs expected {expected}"
        );
    }
}

/// The GT19 rope backward companion fixture: an annotated `rope_norm` primal
/// over an interleaved `[rows, dim/2, 3]` table with
/// `theta_k = 8 * 100000^(-2k/8)`, cos and sin per pair, both rows identical.
/// `x` mirrors the pinned `rope_norm` fixture rows.
const GT19_ROPE_SOURCE: &str = r#"
@ radix lane "air"
@ radix backward "rope_backward"
functio rot(tf32[2, 8] x, tf32[2, 4, 3] table) → fractus {
    sit tensor<f32, [2, 8]> y ← x.rope_norm<8>(8, table)
    redde y.media()
}

functio unitas() → vacuum {
    tacet
}

incipit {
    sit lista<f32> flat ← [0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, -1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, -1.5]
    sit tensor<f32, [2, 8]> x ← flat ↦ tensor<f32, [2, 8]>
    sit lista<f32> table_flat ← [8.0, -0.1455000338, 0.9893582466, 0.4498730602, 0.9005023096, 0.434851228, 0.0252982213, 0.9996800171, 0.0252955229, 0.0014226235, 0.9999989881, 0.001422623, 8.0, -0.1455000338, 0.9893582466, 0.4498730602, 0.9005023096, 0.434851228, 0.0252982213, 0.9996800171, 0.0252955229, 0.0014226235, 0.9999989881, 0.001422623]
    sit tensor<f32, [2, 4, 3]> rope_table ← table_flat ↦ tensor<f32, [2, 4, 3]>
    sit [gx, gt] ← rope_backward(x, rope_table, unitas(), 1.0)
    nota gx
    nota gt
}
"#;

#[test]
fn gt19_rope_backward_companion_compiles() {
    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "gt19_rope_backward.fab", GT19_ROPE_SOURCE, AnalyzeRequest::default())
        .expect("rope backward should analyze cleanly");
    let lowered = expect_codegen_ok(
        lower_mir_with_context_for_target(&mut analysis, false),
        "rope backward should lower after bundle application",
    );
    let names = mir_function_names(lowered.validated.program(), &lowered.interner);
    assert!(
        names.iter().any(|name| name == "rope_backward"),
        "rope backward companion must be installed; got functions: {names:?}"
    );
}

#[test]
fn gt19_rms_norm_backward_runs_on_runner_path() {
    let source = r#"
functio unitas() → vacuum { tacet }

@ radix lane "air"
@ radix backward "norm_backward"
functio norm(tf32[2, 8] x, tf32[8] gamma) → fractus {
    redde x.rms_norm(axis 1, 1e-5, gamma).media()
}

incipit {
    sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5]
    sit lista<f32> gamma_flat ← [2.0, 1.0, 0.5, 1.5, 0.75, 1.25, 1.0, 0.5]
    sit tf32[2, 8] x ← flat ↦ tf32[2, 8]
    sit tf32[8] gamma ← gamma_flat ↦ tf32[8]
    sit iuncta<tf32[2, 8], tf32[8]> gs ← norm_backward(x, gamma, unitas(), 1.0)
    nota gs[0] ↦ lista<f32>
    nota gs[1] ↦ lista<f32>
}
"#;

    let stdout = run_gt19_la("gt19_rms_norm_backward.fab", source);
    assert_eq!(stdout.len(), 2, "both gradients should be printed");
    let grad_x = parse_fractus_list(&stdout[0]);
    let grad_gamma = parse_fractus_list(&stdout[1]);
    let (_, expected_dx, expected_dgamma) = gt19_rms_norm_mean_backward_reference(2, 8);
    gt19_check_close("grad_x", &grad_x, &expected_dx);
    gt19_check_close("grad_gamma", &grad_gamma, &expected_dgamma);
}

#[test]
fn gt19_mul_mean_backward_runs_on_runner_path() {
    let source = r#"
functio unitas() → vacuum { tacet }

@ radix lane "air"
@ radix backward "lin_backward"
functio lin(tf32[2] a, tf32[2] b) → fractus {
    sit tf32[2] p ← a ⊙ b
    redde p.media()
}

incipit {
    sit lista<f32> a_flat ← [2.0, 4.0]
    sit lista<f32> b_flat ← [1.0, 3.0]
    sit tf32[2] a ← a_flat ↦ tf32[2]
    sit tf32[2] b ← b_flat ↦ tf32[2]
    sit fractus loss ← lin(a, b)
    nota loss
    sit iuncta<tf32[2], tf32[2]> gs ← lin_backward(a, b, unitas(), 1.0)
    nota gs[0] ↦ lista<f32>
    nota gs[1] ↦ lista<f32>
}
"#;

    let stdout = run_gt19_la("gt19_mul_mean_backward.fab", source);
    assert_eq!(stdout.len(), 3, "primal and both gradients should be printed");
    let loss = stdout[0]
        .parse::<f64>()
        .expect("loss should render as a number");
    assert!((loss - 7.0).abs() < 1e-6, "mean product should be 7.0, got {loss}");
    let grad_a = parse_fractus_list(&stdout[1]);
    let grad_b = parse_fractus_list(&stdout[2]);
    assert_eq!(grad_a.len(), 2);
    assert_eq!(grad_b.len(), 2);
    for (observed, expected) in grad_a.iter().zip([0.5, 1.5]) {
        assert!((observed - expected).abs() < 1e-6, "grad_a: {grad_a:?}");
    }
    for (observed, expected) in grad_b.iter().zip([1.0, 2.0]) {
        assert!((observed - expected).abs() < 1e-6, "grad_b: {grad_b:?}");
    }
}
