//! `target ⇇ value` through the HIR-backed source emitters: the Faber source
//! emitter prints it back (the `faber format` round trip); every other source
//! target other than Rust fails closed by name instead of emitting a
//! wrong program; Rust emits the runtime's `write_values` call.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "functio double(tensor<f32, [2, 2]> source, in tensor<f32, [2, 2]> out) → vacuum {
    out ⇇ source + source
}

incipit {
fixum tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
varia tensor<f32, [2, 2]> b ← [[0.0, 0.0], [0.0, 0.0]] ↦ tensor<f32, [2, 2]>
double(a, in b)
}
";

fn compile(target: Target, source: &str) -> crate::CompileResult {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    crate::driver::compile(&session, "<test>", source, ModuleCompileRequest::default())
}

fn issues(result: &crate::CompileResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .flat_map(|diagnostic| diagnostic.args.iter())
        .filter(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .collect()
}

#[cfg(feature = "hir-faber")]
fn faber_text(source: &str) -> String {
    let result = compile(Target::HirFaber, source);
    assert!(result.output.is_some(), "faber emission failed: {:?}", result.diagnostics);
    match result.output {
        Some(crate::Output::Faber(output)) => output.code,
        _ => panic!("expected Faber output"),
    }
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_format_keeps_the_copy_into_and_is_idempotent() {
    let first = faber_text(SOURCE);
    assert!(
        first.contains("out ⇇ source + source"),
        "the copy-into survives re-emission:\n{first}"
    );
    assert_eq!(first.matches('⇇').count(), 1, "one glyph, no chain:\n{first}");
    let second = faber_text(&first);
    assert_eq!(first, second, "re-formatting the formatted source changes nothing");
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_fails_closed_by_name() {
    let result = compile(Target::HirTypeScript, SOURCE);
    assert!(result.output.is_none(), "TypeScript must not emit a copy-into");
    assert!(
        issues(&result)
            .iter()
            .any(|issue| issue == "ts_copy_into_unsupported"),
        "{:?}",
        issues(&result)
    );
}

#[cfg(feature = "hir-go")]
#[test]
fn go_fails_closed_by_name() {
    let result = compile(Target::HirGo, SOURCE);
    assert!(result.output.is_none(), "Go must not emit a copy-into");
    assert!(
        issues(&result)
            .iter()
            .any(|issue| issue == "go_copy_into_unsupported"),
        "{:?}",
        issues(&result)
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_the_write_values_carrier() {
    let result = compile(Target::HirRust, SOURCE);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("Rust must emit a copy-into: {:?}", result.diagnostics);
    };
    assert!(
        output.code.contains("(*p_out).write_values(&("),
        "the copy-into is the runtime's write_values call:\n{}",
        output.code
    );
}

#[cfg(feature = "hir-python")]
#[test]
fn python_fails_closed() {
    let result = compile(Target::HirPython, SOURCE);
    assert!(result.output.is_none(), "Python must not emit a copy-into");
}
