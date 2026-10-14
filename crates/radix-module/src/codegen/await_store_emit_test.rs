//! d11-6 §3(a) row 2: a narrowing await-binding checks the awaited value
//! against the slot after the await, on the HIR backends that carry
//! `promissum` (Rust, Go, TypeScript).

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session, compile};

const NARROWING: &str = r"
functio fetch() fiet → numerus {
    redde 300
}

functio caller() fiet → vacuum {
    fixum u8 v ↢ fetch()
    nota v
}";

fn emit(target: Target, source: &str) -> String {
    let session = Session::new(Config::default().with_target(target));
    let result = compile(&session, "await-store.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "{:?}", result.diagnostics);
    match result.output {
        Some(crate::Output::Rust(output)) => output.code,
        Some(crate::Output::Go(output)) => output.code,
        Some(crate::Output::TypeScript(output)) => output.code,
        _ => panic!("expected HIR backend output"),
    }
}

#[test]
fn rust_checks_the_awaited_value_after_the_await() {
    let code = emit(Target::HirRust, NARROWING);
    assert!(
        code.contains("faber::exact::store::<u8>(fetch().await as i128, \"await-binding `v`\""),
        "{code}"
    );
}

#[test]
fn go_checks_the_awaited_value() {
    let code = emit(Target::HirGo, NARROWING);
    assert!(
        code.contains("v := uint8(rt.ExactStore[uint8](rt.ExactI(int64(fetch())), \"await-binding `v`\""),
        "{code}"
    );
}

#[test]
fn typescript_checks_the_value_after_await() {
    let code = emit(Target::HirTypeScript, NARROWING);
    assert!(
        code.contains("exact.store(await fetch(), 0, 255, \"u8\", \"await-binding `v`\""),
        "{code}"
    );
    // The range check lives in the runtime package's `exact` namespace, not
    // inline in the file.
    assert!(code.contains("import { "), "{code}");
    assert!(!code.contains("v < lo || v > hi"), "{code}");
}

#[test]
fn an_assignable_await_bind_emits_no_check() {
    let source = r"
functio fetch() fiet → u8 {
    redde 5
}

functio caller() fiet → vacuum {
    fixum u8 v ↢ fetch()
    nota v
}";
    let code = emit(Target::HirRust, source);
    assert!(code.contains("let l_v: u8 = fetch().await;"), "{code}");
}
