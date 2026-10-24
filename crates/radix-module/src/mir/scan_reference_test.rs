//! Reference-runner outputs through the normal host-kernel lowering route.
//!
//! Scan admission and kernel metadata stay intact; `KernelLowering::Host`
//! chooses the existing sequential reference route for the interpreter.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, KernelLowering, run_analyzed_with_kernel_lowering};
use crate::program::analyze_prepared_program_name;

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root()
            .with_latin_locale(),
    )
}

fn latin(source: &str) -> String {
    format!("+++\nlocale = \"la\"\n+++\n{source}")
}

fn run(source: &str) -> Vec<String> {
    let mut unit = analyze_prepared_program_name(&session(), "scan-reference.fab", &latin(source))
        .unwrap_or_else(|errors| panic!("scan source must analyze: {errors:?}"));
    let mut host = BufferHost::default();
    run_analyzed_with_kernel_lowering(&mut unit, KernelLowering::Host, &mut host)
        .unwrap_or_else(|errors| panic!("reference scan must run: {errors:?}"));
    host.stdout_lines
}

#[test]
fn inclusive_exclusive_and_descending_sum_scans_keep_source_order() {
    let source = r"
@ nucleum
@ publica
functio inclusive(tensor<f32, [4]> xs) → tensor<f32, [4]> {
    redde scan via summa ex xs apud [i] filum lane sit value { redde value }
}
@ nucleum
@ publica
functio exclusive(tensor<f32, [4]> xs) → tensor<f32, [4]> {
    redde scan via summa total exclusive ex xs apud [i] filum lane sit value { redde value }
}
@ nucleum
@ publica
functio descending(tensor<f32, [4]> xs) → tensor<f32, [4]> {
    redde scan via summa ex xs apud [i] order descendens filum lane sit value { redde value }
}
@ nucleum
@ publica
functio descending_exclusive(tensor<f32, [4]> xs) → tensor<f32, [4]> {
    redde scan via summa total exclusive ex xs apud [i] order descendens filum lane sit value { redde value }
}
incipit {
    sit tensor<f32, [4]> xs ← [1.0, 2.0, 3.0, 4.0] ↦ tensor<f32, [4]>
    nota inclusive(xs)
    nota exclusive(xs)
    nota descending(xs)
    nota descending_exclusive(xs)
}
";
    assert_eq!(
        run(source),
        [
            "[1.0, 3.0, 6.0, 10.0]",
            "[0.0, 1.0, 3.0, 6.0]",
            "[10.0, 9.0, 7.0, 4.0]",
            "[9.0, 7.0, 4.0, 0.0]",
        ]
    );
}

#[test]
fn grouped_scan_keeps_one_accumulator_per_recurrent_key() {
    let source = r"
@ nucleum
@ publica
functio per_paritatem(tensor<f32, [6]> xs) → tensor<f32, [6]> {
    redde scan via summa ex xs apud [i] pro i % 2 filum lane sit value { redde value }
}
incipit {
    sit tensor<f32, [6]> xs ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] ↦ tensor<f32, [6]>
    nota per_paritatem(xs)
}
";
    assert_eq!(run(source), ["[1.0, 2.0, 4.0, 6.0, 9.0, 12.0]"]);
}

#[test]
fn empty_scan_returns_an_empty_source_shaped_tensor() {
    let source = r"
@ nucleum
@ publica
functio empty_scan(tf32[0] xs) → tf32[0] {
    redde scan via summa ex xs apud [i] filum lane sit value { redde value }
}
incipit {
    sit tf32[0] xs ← vacua
    nota empty_scan(xs)
}
";
    assert_eq!(run(source), ["[]"]);
}
