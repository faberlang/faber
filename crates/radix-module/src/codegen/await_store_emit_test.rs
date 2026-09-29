//! d11-6 §3(a) row 2: a narrowing await-binding checks the awaited value
//! against the slot after the await, on the HIR backends that carry
//! `promissum` (Rust, Go, TypeScript).

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session, compile};

const NARROWING: &str = r#"
functio fetch() fiet → numerus {
    redde 300
}

functio caller() fiet → vacuum {
    fixum numerus<u8> v ↢ fetch()
    nota v
}"#;

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
    assert!(code.contains("<u8 as TryFrom<i64>>::try_from(fetch().await).expect("), "{code}");
}

#[test]
fn go_checks_the_awaited_value() {
    let code = emit(Target::HirGo, NARROWING);
    assert!(code.contains("var v int = fetch()"), "{code}");
    assert!(code.contains("v < 0 || v > 255"), "{code}");
}

#[test]
fn typescript_checks_the_value_after_await() {
    let code = emit(Target::HirTypeScript, NARROWING);
    assert!(code.contains("(await fetch())"), "{code}");
    assert!(code.contains("__v < 0 || __v > 255"), "{code}");
}

#[test]
fn an_assignable_await_bind_emits_no_check() {
    let source = r#"
functio fetch() fiet → numerus<u8> {
    redde 5
}

functio caller() fiet → vacuum {
    fixum numerus<u8> v ↢ fetch()
    nota v
}"#;
    let code = emit(Target::HirRust, source);
    assert!(code.contains("let v: u8 = fetch().await;"), "{code}");
}
