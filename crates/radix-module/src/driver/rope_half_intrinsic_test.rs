//! Unit test for the `rope_half<dim>(pos, rope_table)` intrinsic: the source
//! spelling of the half-split (Qwen / GPT-NeoX `rotate_half`) RoPE pairing.
//!
//! The registry row added for `rope_half` maps it to
//! `MirCollectionOp::TensorRopeHalf`; this proves, through the full language
//! frontend (lex → parse → semantic → HIR → MIR) plus the CPU runner, that:
//! - `x.rope_half<4>(0, table)` lowers to `TensorRopeHalf` and the runner
//!   rotates the half-split pairs `(x[k], x[k + dim/2])`: with cos = 0 and
//!   sin = 1 the unit at `x[0]` lands at `x[2]` (the pair partner for `k = 0`
//!   at `dim = 4`), not at `x[1]`;
//! - the same input through `rope_norm<4>` rotates the consecutive pairs
//!   `(x[2k], x[2k+1])` and lands the unit at `x[1]`, so the two spellings
//!   select distinct pairings.
//!
//! The values are hand-chosen: the table row is `(angle, cos, sin) =
//! (0, 0, 1)` for both pairs, so the result is exactly `[0, 0, 1, 0]` for the
//! half-split and `[0, 1, 0, 0]` for the consecutive-pair form.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

/// Run an `incipit` block over the rank-1 fixture tensor `[1, 0, 0, 0]` with
/// the pinned `(0, 0, 1)` table, apply `x.<method><4>(0, table)`, and return
/// the `nota` result.
fn run_unit_rope(method: &str) -> Vec<f64> {
    let source = format!(
        "incipit {{
    sit lista<f32> flat ← [1.0, 0.0, 0.0, 0.0]
    sit tensor<f32, [4]> x ← flat ↦ tensor<f32, [4]>
    sit lista<f32> table_flat ← [0.0, 0.0, 1.0, 0.0, 0.0, 1.0]
    sit tensor<f32, [1, 2, 3]> rope_table ← table_flat ↦ tensor<f32, [1, 2, 3]>
    sit tensor<f32, [4]> y ← x.{method}<4>(0, rope_table)
    nota y
}}"
    );
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    let mut host = BufferHost::default();
    match run_source(&session, "rope_half.fab", &source, &mut host) {
        Ok(()) => {}
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("rope_half source should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("rope_half source should lower cleanly: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => {
            panic!("runner should run the pinned rope_half: {errors:?}")
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

/// Element-wise numeric assertion within the fixture tolerance.
fn assert_tensor(name: &str, observed: &[f64], expected: &[f64]) {
    const TOL: f64 = 1e-6;
    assert_eq!(observed.len(), expected.len(), "{name}: length mismatch");
    for (i, (&obs, &exp)) in observed.iter().zip(expected).enumerate() {
        assert!((obs - exp).abs() <= TOL, "{name}[{i}]: observed {obs}, expected {exp}");
    }
}

/// The half-split pairing (Qwen / GPT-NeoX `rotate_half`): the unit at `x[0]`
/// rotates into its pair partner `x[2]` for `dim = 4` and stays out of `x[1]`.
#[test]
fn rope_half_rotates_half_split_pairs() {
    let observed = run_unit_rope("rope_half");
    assert_tensor("rope_half", &observed, &[0.0, 0.0, 1.0, 0.0]);
    assert!(
        observed[2] > 0.5,
        "rope_half must rotate the unit at x[0] into x[2] (pair (0, 2)); got {observed:?}"
    );
    assert!(
        observed[1].abs() < 1e-6,
        "rope_half must leave x[1] untouched; got {observed:?}"
    );
}

/// The same fixture through `rope_norm` lands at `x[1]` (consecutive pair
/// `(0, 1)`), proving the two spellings select different pairings.
#[test]
fn rope_norm_on_same_input_rotates_consecutive_pairs() {
    let observed = run_unit_rope("rope_norm");
    assert_tensor("rope_norm", &observed, &[0.0, 1.0, 0.0, 0.0]);
    assert!(
        observed[1] > 0.5,
        "rope_norm must rotate the unit at x[0] into x[1] (pair (0, 1)); got {observed:?}"
    );
    assert!(
        observed[2].abs() < 1e-6,
        "rope_norm must leave x[2] untouched; got {observed:?}"
    );
}
