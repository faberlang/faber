//! A package that declares a bodiless `@ nondum` function or `genus` method
//! emits cleanly and omits it: no text backend and no MIR function carries the
//! declaration, while the Faber re-emitter keeps it so formatting and
//! conversion never drop it.

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
    let session = Session::new(Config::default().with_target(target).with_latin_locale());
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
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let unit = crate::driver::analyze_source(&session, "nondum.fab", SOURCE, AnalyzeRequest::default())
        .expect("analysis succeeds");
    let dump = crate::mir::dump_analyzed_unit(unit.application_view()).expect("MIR lowering succeeds");
    // Two MIR functions: `nunc` and the entry block. The two declarations
    // lower to nothing.
    assert_eq!(
        dump.matches("function f").count(),
        2,
        "no MIR function for a declaration:\n{dump}"
    );
}

const GENUS_SOURCE: &str = r#"
genus Nexus {
    varia textus nomen

    @ nondum "pending host dispatch"
    functio mitte_postea(textus data) → vacuum

    functio lege() → textus {
        redde ego.nomen
    }
}

incipit {
    varia Nexus n ← Nexus { nomen = "salve" }
    nota n.lege()
}
"#;

fn emit_genus(target: Target) -> String {
    let result = compile(target, GENUS_SOURCE);
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

fn assert_omits_method_declaration(target: Target) {
    let code = emit_genus(target);
    assert!(
        !code.to_lowercase().contains("mitte_postea") && !code.contains("MittePostea"),
        "{target} must not emit a declaration-only method:\n{code}"
    );
    assert!(
        code.to_lowercase().contains("lege"),
        "{target} still emits the ordinary method:\n{code}"
    );
}

#[test]
fn rust_omits_a_bodiless_nondum_method() {
    assert_omits_method_declaration(Target::HirRust);
}

#[test]
fn typescript_omits_a_bodiless_nondum_method() {
    assert_omits_method_declaration(Target::HirTypeScript);
}

#[test]
fn go_omits_a_bodiless_nondum_method() {
    assert_omits_method_declaration(Target::HirGo);
}

#[test]
fn swift_omits_a_bodiless_nondum_method() {
    assert_omits_method_declaration(Target::HirSwift);
}

#[test]
fn python_omits_a_bodiless_nondum_method() {
    assert_omits_method_declaration(Target::HirPython);
}

#[test]
fn haskell_omits_a_bodiless_nondum_method() {
    assert_omits_method_declaration(Target::HirHaskell);
}

#[test]
fn faber_re_emission_keeps_the_method_declaration() {
    let code = emit_genus(Target::HirFaber);
    assert!(code.contains("mitte_postea("), "re-emission keeps the method:\n{code}");
    assert!(code.contains("pending host dispatch"), "{code}");
}

#[test]
fn mir_lowering_skips_a_bodiless_nondum_method() {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let unit = crate::driver::analyze_source(&session, "nondum.fab", GENUS_SOURCE, AnalyzeRequest::default())
        .expect("analysis succeeds");
    let dump = crate::mir::dump_analyzed_unit(unit.application_view()).expect("MIR lowering succeeds");
    // Two MIR functions: `lege` and the entry block. The declaration lowers
    // to nothing.
    assert_eq!(
        dump.matches("function f").count(),
        2,
        "no MIR function for a method declaration:\n{dump}"
    );
}
