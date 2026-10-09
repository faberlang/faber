//! A package that declares a bodiless `@ nondum` function emits cleanly and
//! omits it: no text backend and no MIR function carries the declaration, while
//! the Faber re-emitter keeps it so formatting and conversion never drop it.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, ModuleCompileRequest, Session};

const SOURCE: &str = r#"
@ nondum "pending host dispatch"
functio later(textus s) → textus

@ nondum "withdrawn"
functio later_pair(textus s, numerus n) → textus ⇥ textus

functio nunc(textus s) → textus {
    redde s
}

incipit {
    nota nunc("salve")
}
"#;

fn compile(target: Target, source: &str) -> crate::CompileResult {
    let session = Session::new(Config::default().with_target(target));
    crate::driver::compile(&session, "nondum.fab", source, ModuleCompileRequest::default())
}

fn emit(target: Target) -> String {
    let result = compile(target, SOURCE);
    assert!(result.success(), "{target} compilation failed: {:?}", result.diagnostics);
    match result.output.expect("output") {
        crate::Output::Rust(out) => out.code,
        crate::Output::Go(out) => out.code,
        crate::Output::TypeScript(out) => out.code,
        crate::Output::Swift(out) => out.code,
        crate::Output::Python(out) => out.code,
        crate::Output::Haskell(out) => out.code,
        crate::Output::Faber(out) => out.code,
        _ => panic!("unexpected output for {target}"),
    }
}

fn assert_omits_declarations(target: Target) {
    let code = emit(target);
    assert!(
        !code.contains("later"),
        "{target} must not emit a declaration-only function:\n{code}"
    );
    assert!(
        code.to_lowercase().contains("nunc"),
        "{target} still emits the ordinary function:\n{code}"
    );
}

#[test]
fn rust_omits_a_bodiless_nondum_function() {
    assert_omits_declarations(Target::HirRust);
}

#[test]
fn typescript_omits_a_bodiless_nondum_function() {
    assert_omits_declarations(Target::HirTypeScript);
}

#[test]
fn go_omits_a_bodiless_nondum_function() {
    assert_omits_declarations(Target::HirGo);
}

#[test]
fn swift_omits_a_bodiless_nondum_function() {
    assert_omits_declarations(Target::HirSwift);
}

#[test]
fn python_omits_a_bodiless_nondum_function() {
    assert_omits_declarations(Target::HirPython);
}

#[test]
fn haskell_omits_a_bodiless_nondum_function() {
    assert_omits_declarations(Target::HirHaskell);
}

#[test]
fn faber_re_emission_keeps_the_declaration() {
    let code = emit(Target::HirFaber);
    assert!(code.contains("later("), "re-emission keeps the declaration:\n{code}");
    assert!(code.contains("later_pair("), "{code}");
    assert!(code.contains("pending host dispatch"), "{code}");
    assert!(
        !code.contains("later(textus s) → textus {"),
        "the kept declaration stays bodiless:\n{code}"
    );
}

#[test]
fn mir_lowering_skips_a_bodiless_nondum_function() {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let unit = crate::driver::analyze_source(&session, "nondum.fab", SOURCE, AnalyzeRequest::default())
        .expect("analysis succeeds");
    let dump = crate::mir::dump_analyzed_unit(&unit).expect("MIR lowering succeeds");
    // Two MIR functions: `nunc` and the entry block. The two declarations
    // lower to nothing.
    assert_eq!(
        dump.matches("function f").count(),
        2,
        "no MIR function for a declaration:\n{dump}"
    );
}
