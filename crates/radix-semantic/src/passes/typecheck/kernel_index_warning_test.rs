//! The unproven-index warning inside a kernel (gpu-reset K11).
//!
//! Every case asserts the structured issue slug, never rendered prose.

use crate::test_support::analyze_source;
use crate::{SemanticErrorKind, WarningKind};

const UNPROVEN: &str = "unproven_kernel_index";

fn unproven_count(source: &str) -> usize {
    let analysis = analyze_source(source);
    let hits: Vec<_> = analysis
        .diagnostics
        .iter()
        .filter(|diag| diag.issue() == Some(UNPROVEN))
        .collect();
    for hit in &hits {
        assert_eq!(
            hit.kind,
            SemanticErrorKind::Warning(WarningKind::UnprovenKernelIndex)
        );
        assert!(!hit.is_error(), "the index warning must never be an error");
    }
    hits.len()
}

#[test]
fn kernel_index_warning_fires_for_arithmetic_and_parameter_indexes() {
    let arithmetic = r"
@ nucleum
functio k(tensor<f32, [8]> src, tensor<f32, [8]> dst) → vacuum {
    itera ex src apud [i] filum f sit v {
        dst[i + 1] ← v
    }
}
";
    assert_eq!(unproven_count(arithmetic), 1);
    let parameter = r"
@ nucleum
functio k(tensor<f32, [8]> src, numerus j) → f32 ∪ nihil {
    redde src[j]
}
";
    assert_eq!(unproven_count(parameter), 1);
}

#[test]
fn kernel_index_warning_is_silent_for_binders_and_in_range_literals() {
    let binder = r"
@ nucleum
functio k(tensor<f32, [8]> src, tensor<f32, [8]> dst) → vacuum {
    itera ex src apud [i] filum f sit v {
        dst[i] ← v
    }
}
";
    assert_eq!(unproven_count(binder), 0);
    let rank_two = r"
@ nucleum
functio k<magnitudo H, magnitudo W>(tensor<f32, [H, W]> src, tensor<f32, [H, W]> dst) → vacuum {
    itera ex src apud [i, j] filum f sit v {
        dst[[i, j]] ← v
    }
}
";
    assert_eq!(unproven_count(rank_two), 0);
    let literal = r"
@ nucleum
functio k(tensor<f32, [8]> src) → f32 ∪ nihil {
    redde src[3]
}
";
    assert_eq!(unproven_count(literal), 0);
}

#[test]
fn kernel_index_warning_does_not_trust_a_binder_of_a_different_extent() {
    let source = r"
@ nucleum
functio k(tensor<f32, [4]> small, tensor<f32, [8]> big) → vacuum {
    itera ex small apud [i] filum f sit v {
        big[i] ← v
    }
}
";
    assert_eq!(unproven_count(source), 1);
}

#[test]
fn kernel_index_warning_is_silent_outside_a_kernel() {
    let source = r"
functio k(tensor<f32, [8]> src, numerus j) → f32 ∪ nihil {
    redde src[j]
}
";
    assert_eq!(unproven_count(source), 0);
}

#[test]
fn kernel_index_warning_is_silent_for_a_constant_index_in_range() {
    // A module constant, and a constant expression over one, are proven
    // exactly like the literal they fold to.
    for index in ["K", "K + 1", "[K]"] {
        let source = format!(
            r"
fixum numerus K = 2

@ nucleum
functio k(tensor<f32, [8]> src) → f32 ∪ nihil {{
    redde src[{index}]
}}
"
        );
        assert_eq!(unproven_count(&source), 0, "src[{index}]");
    }
    let local = r"
@ nucleum
functio k(tensor<f32, [8]> src) → f32 ∪ nihil {
    fixum numerus K = 2
    redde src[K]
}
";
    assert_eq!(unproven_count(local), 0, "a local compile-time constant");
}

#[test]
fn kernel_index_warning_keeps_a_run_time_binding_unproven() {
    // `sit` is a run-time binding even with a literal initializer.
    let source = r"
@ nucleum
functio k(tensor<f32, [8]> src) → f32 ∪ nihil {
    sit numerus i ← 2
    redde src[i]
}
";
    assert_eq!(unproven_count(source), 1);
}

#[test]
fn a_constant_index_out_of_range_is_refused_at_compile_time() {
    let source = r"
fixum numerus K = 8

@ nucleum
functio k(tensor<f32, [8]> src) → f32 ∪ nihil {
    redde src[K]
}
";
    let analysis = analyze_source(source);
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.issue() == Some("index_provably_out_of_bounds")),
        "{:?}",
        analysis.diagnostics
    );
    assert_eq!(unproven_count(source), 0);
}
