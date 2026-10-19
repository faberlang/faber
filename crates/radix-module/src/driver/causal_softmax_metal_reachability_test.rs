//! S05 family-2 Metal-column probe: `x.softmax_causal()` from source to
//! Metal text.
//!
//! The source spelling and its CPU-runner arithmetic proof are pinned in
//! `causal_softmax_intrinsic_test.rs`; the Metal emit body
//! (`radix-mir-metal` `emit_causal_masked_softmax`) predates the spelling
//! probe and was reached only through the additive-mask idiom. These two
//! probes close that gap through the driver's own `compile` route
//! (`Target::MirMetal`, `crate::Output::MetalText`):
//!
//! - Probe A `[8, 64]`: rows > 1, width 64 >= 32 and 64 % 4 == 0 admits
//!   `causal_softmax_row_reduction_admission`, so the kernel carries the
//!   `RowReduction` launch geometry and the emitted body is the threadgroup
//!   row recipe (one workgroup per row: `cs_red` tree reductions). The mask
//!   is the recipe's own semantics — the row index drives the column guard
//!   `masked = min(row + 1, cols)`; there is no mask tensor input.
//! - Probe B `[1, 64]`: the same function shape at T=1 (decode). Rows <= 1
//!   keeps the generic per-element 1D contract, so the emitted body carries
//!   the flat `row = o / cols` / `col = o % cols` addressing and no
//!   threadgroup reduction. This pins the planner answer: the causal offset
//!   is not a plan setting — `CausalMaskedSoftmaxPlan` is only `rows`/`cols`
//!   — and continuation decode rides the T=1 generic contract.

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::Diagnostic;
use crate::driver::{Config, ModuleCompileRequest, Session};

fn metal_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::MirMetal)
            .with_dev_locale_root(),
    )
}

/// The source shape of both probes: one `@ kernel` entry whose whole body is
/// the causal masked softmax of its rank-2 operand.
fn csm_source(rows: u64, cols: u64) -> String {
    format!(
        "+++
locale = \"en\"
+++

@ kernel
@ public
fn csm(tensor<f32, [{rows}, {cols}]> x) → tensor<f32, [{rows}, {cols}]> {{
    return x.softmax_causal()
}}
"
    )
}

/// The `issue` args of a failed compile, so a divergence is reported by
/// diagnostic identity (never by message text).
fn issue_facts(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .flat_map(|diagnostic| diagnostic.args.iter())
        .filter(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .collect()
}

/// Compile one probe to Metal text, failing with diagnostic identity facts.
fn compile_metal(rows: u64, cols: u64) -> String {
    let source = csm_source(rows, cols);
    let result = compile(&metal_session(), "csm.fab", &source, ModuleCompileRequest::default());
    if !result.success() {
        panic!(
            "softmax_causal [{rows}, {cols}] should emit Metal text; issues={:?}",
            issue_facts(&result.diagnostics)
        );
    }
    match result.output {
        Some(crate::Output::MetalText(output)) => output.code,
        other => panic!("expected MetalText output, got {}", other.is_some()),
    }
}

/// Probe A: the admitted multi-row shape emits the threadgroup row recipe —
/// one workgroup per row, the row index selecting the causal extent, and the
/// column guard that zeroes the future (above-diagonal) columns.
#[test]
fn causal_softmax_metal_row_reduction_admitted() {
    let metal = compile_metal(8, 64);
    // The row-reduction recipe's threadgroup scratch: absent from the generic
    // per-element 1D body (probe B pins its absence there).
    assert!(
        metal.contains("threadgroup float cs_red[32];"),
        "row-reduction scratch buffer (one 32-lane workgroup per row):\n{metal}"
    );
    // One workgroup per row: the thread id's row half comes from the
    // workgroup geometry, not from a flat `o / cols` quotient.
    assert!(
        metal.contains("uint row = id.x / 32u;"),
        "row index derived from the per-row workgroup:\n{metal}"
    );
    assert!(
        metal.contains("uint lane = id.x % 32u;"),
        "lane index derived from the per-row workgroup:\n{metal}"
    );
    // The causal semantics: the ROW index fixes the unmasked column extent
    // (the diagonal included) — there is no mask tensor input, the guard is
    // part of the recipe.
    assert!(
        metal.contains("uint masked = min(row + 1u, 64u);"),
        "row-driven causal column extent:\n{metal}"
    );
    // The masked columns are excluded before any load feeds an `exp`: the
    // scale pass guards the column (`if (j < masked)`) and writes zero above
    // the diagonal.
    assert!(
        metal.contains("if (j < masked) {"),
        "column-index mask guard precedes the scale-pass read:\n{metal}"
    );
    assert!(
        metal.contains("= 0.0;"),
        "above-diagonal columns are written as zero probability:\n{metal}"
    );
}

/// Probe B: the T=1 decode shape stays on the generic per-element 1D
/// contract — flat `o` addressing, per-element row scan, no threadgroup
/// reduction and therefore none of probe A's row-reduction markers.
#[test]
fn causal_softmax_metal_decode_t1_generic_contract() {
    let metal = compile_metal(1, 64);
    assert!(
        metal.contains("uint row = o / 64u;"),
        "generic 1D contract derives the row from the flat index:\n{metal}"
    );
    assert!(
        metal.contains("uint col = o % 64u;"),
        "generic 1D contract derives the column from the flat index:\n{metal}"
    );
    // The same causal semantics survive the generic body (rows <= 1 keeps
    // the per-element contract, not a mask-free softmax).
    assert!(
        metal.contains("uint masked = min(row + 1u, 64u);"),
        "T=1 keeps the row-driven causal column extent:\n{metal}"
    );
    // Probe A's row-reduction recipe must be entirely absent: the decode
    // shape does not admit it (`causal_softmax_row_reduction_admission`
    // returns `None` for rows <= 1).
    assert!(
        !metal.contains("cs_red"),
        "T=1 must not emit the row-reduction scratch buffer:\n{metal}"
    );
    assert!(
        !metal.contains("uint row = id.x / 32u;"),
        "T=1 must not emit the per-row workgroup row index:\n{metal}"
    );
    assert!(
        !metal.contains("threadgroup_barrier"),
        "T=1 must not emit threadgroup barriers:\n{metal}"
    );
}
