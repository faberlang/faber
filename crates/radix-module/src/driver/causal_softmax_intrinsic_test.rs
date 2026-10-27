//! Driver-level test for the `softmax_causal` tensor intrinsic (GI3-1
//! causal-masked row softmax): the source spelling `x.softmax_causal()` runs
//! through the full language frontend (lex → parse → semantic → HIR → MIR)
//! plus the CPU runner.
//!
//! Proves, through the driver's `run_source` route:
//! - a 3×3 all-ones score matrix softmaxes each row `i` over columns `0..=i`
//!   (row 0 = [1], row 1 = [0.5, 0.5], row 2 = [1/3, 1/3, 1/3]) and the three
//!   entries above the diagonal are exactly zero;
//! - a 2×2 distinguishing input ([0, 0, 0, 1]) gives row 0 = [1, 0] and row 1
//!   = [1/(1+e), e/(1+e)], pinning the exp-shift softmax arithmetic.
//!
//! The source spelling is the registry row `softmax_causal`
//! (`crates/radix-intrinsics/src/registry.rs`), not `causal_softmax`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

/// Format an f64 fixture datum as a Faber `fractus` literal (always with a
/// decimal point so the parser treats it as a float literal).
fn f32_literal(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// Parse a `nota <tensor>` runner line (`[v0, v1, …]`) back to f64 values.
fn parse_tensor_line(line: &str) -> Vec<f64> {
    let line = line.trim();
    assert!(line.starts_with('[') && line.ends_with(']'), "tensor nota line: {line:?}");
    let inner = &line[1..line.len() - 1];
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<f64>()
                .unwrap_or_else(|err| panic!("tensor element {part:?}: {err}"))
        })
        .collect()
}

/// Run an `incipit` block that builds `x` from `flat`, applies
/// `x.softmax_causal()`, and `nota`s the result; returns the runner tensor as
/// f64 data.
fn run_causal_softmax_source(rows: usize, cols: usize, flat: &[f64]) -> Vec<f64> {
    assert_eq!(flat.len(), rows * cols, "fixture flat data must fill the shape");
    let flat_text: Vec<String> = flat.iter().map(|&v| f32_literal(v)).collect();
    let source = format!(
        "incipit {{
    sit lista<f32> flat ← [{}]
    sit tensor<f32, [{rows}, {cols}]> x ← flat ↦ tensor<f32, [{rows}, {cols}]>
    sit tensor<f32, [{rows}, {cols}]> y ← x.softmax_causal()
    nota y
}}",
        flat_text.join(", "),
    );
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    let mut host = BufferHost::default();
    match run_source(&session, "causal_softmax.fab", &source, &mut host) {
        Ok(()) => {}
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("causal_softmax source should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("causal_softmax source should lower cleanly: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => {
            panic!("runner should run the pinned softmax_causal: {errors:?}")
        }
    }
    assert_eq!(
        host.stdout_lines.len(),
        1,
        "expected one nota line, got {:?}",
        host.stdout_lines
    );
    parse_tensor_line(&host.stdout_lines[0])
}

/// Element-wise comparison within the 1e-6 fixture tolerance.
fn check_close(name: &str, observed: &[f64], expected: &[f64]) {
    const TOL: f64 = 1e-6;
    assert_eq!(observed.len(), expected.len(), "{name}: length mismatch");
    for (i, (&obs, &exp)) in observed.iter().zip(expected).enumerate() {
        let delta = (obs - exp).abs();
        assert!(
            delta <= TOL,
            "{name}[{i}]: runner={obs}, expected={exp}, delta={delta:.3e} over {TOL:.1e}"
        );
    }
}

/// 3×3 all-ones score matrix: row `i` is uniform over its `i + 1` visible
/// columns and the above-diagonal entries are exactly zero.
#[test]
fn softmax_causal_3x3_ones_masks_above_diagonal() {
    let observed = run_causal_softmax_source(3, 3, &[1.0; 9]);
    let third = 1.0 / 3.0;
    check_close(
        "softmax_causal 3x3 ones",
        &observed,
        &[1.0, 0.0, 0.0, 0.5, 0.5, 0.0, third, third, third],
    );
    assert_eq!(observed[1], 0.0, "row 0 x[1] is above the diagonal");
    assert_eq!(observed[2], 0.0, "row 0 x[2] is above the diagonal");
    assert_eq!(observed[5], 0.0, "row 1 x[2] is above the diagonal");
}

/// 2×2 distinguishing input: row 0 softmaxes the single score 0.0 to [1, 0];
/// row 1 softmaxes [0.0, 1.0] to [1/(1+e), e/(1+e)], pinning the exponent
/// arithmetic.
#[test]
fn softmax_causal_2x2_row_softmax_matches_exp() {
    let observed = run_causal_softmax_source(2, 2, &[0.0, 0.0, 0.0, 1.0]);
    let e = std::f64::consts::E;
    check_close("softmax_causal 2x2", &observed, &[1.0, 0.0, 1.0 / (1.0 + e), e / (1.0 + e)]);
    assert_eq!(observed[1], 0.0, "row 0 x[1] is above the diagonal");
}
