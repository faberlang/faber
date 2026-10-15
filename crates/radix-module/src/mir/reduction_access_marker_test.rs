//! A reduction whose source is an access-marked parameter (`mut`/`own`).
//!
//! The parameter's type carries the access marker; the reduction must read the
//! tensor carrier under it. Before the fix the reduction declined silently, the
//! `return` lowered to nothing, and the function ended in a no-value return
//! (`invalid MIR: no-value return in value-returning function`).

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_analyzed};
use crate::program::analyze_prepared_program_name;

fn run(source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("+++\nlocale = \"en\"\n+++\n{source}");
    let mut unit = analyze_prepared_program_name(&session, "reduction-access-marker.fab", &source)
        .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"));
    let mut host = BufferHost::default();
    run_analyzed(&mut unit, &mut host).unwrap_or_else(|errors| panic!("program must lower and run: {errors:?}"));
    host.stdout_lines
}

const PRELUDE: &str = r"
fn argmax_mut(mut tensor<f32, [4]> x) → int {
    return reduce via argmax from x at [i] coalesce 0
}

fn sum_mut(mut tensor<f32, [4]> x) → f32 {
    return reduce via sum from x at [i]
}

fn max_mut(mut tensor<f32, [4]> x) → f32 {
    return reduce via max from x at [i] coalesce 0.0 ∷ f32
}

fn argmax_own(own tensor<f32, [4]> x) → int {
    return reduce via argmax from x at [i] coalesce 0
}

fn argmax_plain(tensor<f32, [4]> x) → int {
    return reduce via argmax from x at [i] coalesce 0
}
";

#[test]
fn mut_parameter_reduction_return_runs() {
    let source = format!(
        "{PRELUDE}
main {{
    mut tensor<f32, [4]> t ← tf32[4] {{}}
    t[[2]] ← 5.0 ∷ f32
    t[[3]] ← 1.5 ∷ f32
    print argmax_mut(mut t)
}}
"
    );
    assert_eq!(run(&source), ["2"]);
}

#[test]
fn mut_parameter_sum_and_max_return_run() {
    let source = format!(
        "{PRELUDE}
main {{
    mut tensor<f32, [4]> t ← tf32[4] {{}}
    t[[2]] ← 5.0 ∷ f32
    t[[3]] ← 1.5 ∷ f32
    print sum_mut(mut t)
    print max_mut(mut t)
}}
"
    );
    assert_eq!(run(&source), ["6.5", "5.0"]);
}

#[test]
fn own_and_plain_parameter_argmax_return_run() {
    let source = format!(
        "{PRELUDE}
main {{
    mut tensor<f32, [4]> t ← tf32[4] {{}}
    t[[1]] ← 2.0 ∷ f32
    print argmax_plain(t)
    print argmax_own(own t)
}}
"
    );
    assert_eq!(run(&source), ["1", "1"]);
}
