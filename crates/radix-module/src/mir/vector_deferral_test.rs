//! Vector methods lower to one deferred `VectorOp` and expand after types are
//! concrete. Covers the wiring: literal widths, generic widths instantiated
//! at a literal, projected receivers, and the survivor rule never firing on a
//! valid program. The lane rules themselves are pinned in
//! `radix-mir`'s `vector_expand_test`.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};
use crate::mir::{BufferHost, RunSourceError, dump_analyzed_unit, run_source};

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    )
}

fn dump(source: &str) -> String {
    let analyzed = crate::driver::analyze_source(&session(), "test.fab", source, AnalyzeRequest::default())
        .expect("analysis succeeds");
    dump_analyzed_unit(analyzed.application_view()).expect("MIR lowering succeeds")
}

fn run(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    match run_source(&session(), "test.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("runner errors: {errors:?}"),
    }
}

#[test]
fn literal_width_methods_leave_no_deferred_op_in_the_program() {
    for width in 2..=4 {
        for (result, call) in [
            ("vf32", "a.addita(b)"),
            ("f32", "a.productum(b)"),
            ("f32", "a.norm()"),
            ("vf32", "a.normalizata()"),
            ("vf32", "a.crea()"),
        ] {
            let result_ty = if result == "vf32" {
                format!("vf32[{width}]")
            } else {
                result.to_owned()
            };
            let source =
                format!("functio f(vf32[{width}] a, vf32[{width}] b) → {result_ty} {{\n    redde {call}\n}}\n");
            let text = dump(&source);
            assert!(!text.contains("vector_op"), "w{width} {call}: {text}");
            let expanded = if call.contains("crea") {
                "const float 0.0"
            } else {
                "_0.lane0"
            };
            assert!(text.contains(expanded), "w{width} {call}: {text}");
        }
    }
}

#[test]
fn generic_width_method_expands_at_the_instance() {
    let lines = run(r"functio dot_self<magnitudo N>(vector<f32, magnitudo N> v) → f32 {
    redde v.productum(v)
}

incipit {
    sit vf32[3] v ← [3.0, 4.0, 0.0] ↦ vf32[3]
    nota dot_self(v)
}
");
    assert_eq!(lines, ["25.0"]);
}

#[test]
fn one_generic_template_instantiates_at_two_widths() {
    let lines = run(r"functio dot_self<magnitudo N>(vector<f32, magnitudo N> v) → f32 {
    redde v.productum(v)
}

incipit {
    sit vf32[2] a ← [3.0, 4.0] ↦ vf32[2]
    sit vf32[3] b ← [1.0, 2.0, 2.0] ↦ vf32[3]
    nota dot_self(a)
    nota dot_self(b)
}
");
    assert_eq!(lines, ["25.0", "9.0"]);
}

#[test]
fn projected_receiver_is_copied_to_a_temp_before_deferral() {
    let lines = run(r"incipit {
    sit vf32[3] v ← [3.0, 4.0, 0.0] ↦ vf32[3]
    sit lista<vf32[3]> xs ← [v]
    nota xs[0].norm()
}
");
    assert_eq!(lines, ["5.0"]);
}

/// Q-X1: `longitudo` is the lane count (the element count); the Euclidean
/// norm is `norm`.
#[test]
fn vector_longitudo_is_the_lane_count_and_norma_is_the_norm() {
    let lines = run(r"incipit {
    sit vf32[3] v ← [3.0, 4.0, 0.0] ↦ vf32[3]
    nota v.longitudo()
    nota v.norm()
}
");
    assert_eq!(lines, ["3", "5.0"]);
}
