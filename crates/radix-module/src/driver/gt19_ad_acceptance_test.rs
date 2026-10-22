//! GT19 product-boundary AD acceptance receipts, la locale.
//!
//! The single-file runner must install AIR backward companions just like the
//! package MIR path. This pins an annotated mean-of-product primal and its
//! interprocedural unit residual through the public source runner.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

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
