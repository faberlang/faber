//! D17.8 (fi6c) emission pins: an unhandled failable call directly in `main`
//! is a panic on every HIR backend that runs programs; the same call inside a
//! handler still propagates or recovers, and a failable call in a helper is
//! still `SEM011:failable_call_requires_handler`.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const DECLS: &str = r#"
functio risky(numerus n) → numerus ⇥ textus {
    si n ≺ 0 { iace "negative input" }
    redde n + 1
}

functio check(numerus n) → vacuum ⇥ textus {
    si n ≺ 0 { iace "check failed" }
}

genus Konto {
    varia numerus saldo

    functio nimm(numerus n) → numerus ⇥ textus {
        si n ≻ ego.saldo { iace "insufficient funds" }
        redde ego.saldo - n
    }
}
"#;

const MAIN: &str = r"
incipit {
    check(1)
    fixum numerus x ← risky(2)
    nota x + risky(3)
    fixum Konto k ← Konto { saldo = 5 }
    nota k.nimm(1)
}
";

const HANDLED: &str = r"
incipit {
    fac {
        fixum numerus x ← risky(-1)
        nota x
    } cape err {
        nota err
    }
}
";

fn compile(target: Target, source: &str) -> crate::CompileResult {
    let session = Session::new(Config::default().with_latin_locale().with_target(target));
    crate::driver::compile(&session, "unhandled.fab", source, ModuleCompileRequest::default())
}

fn emit(target: Target, source: &str) -> String {
    let result = compile(target, source);
    assert!(result.success(), "{target} compilation failed: {:?}", result.diagnostics);
    match result.output.expect("output") {
        crate::Output::Rust(out) => out.code,
        crate::Output::Go(out) => out.code,
        crate::Output::TypeScript(out) => out.code,
        crate::Output::Swift(out) => out.code,
        crate::Output::Python(out) => out.code,
        _ => panic!("unexpected output for {target}"),
    }
}

fn main_body(code: &str, marker: &str) -> String {
    code.split(marker).nth(1).expect("main marker").to_owned()
}

#[test]
fn rust_aborts_on_an_unhandled_failable_call_in_main() {
    let code = emit(Target::HirRust, &format!("{DECLS}{MAIN}"));
    let main = main_body(&code, "fn main()");
    assert!(main.contains("faber::trap::ok(check(1), faber::Trap::FailableCall)"), "{main}");
    assert!(main.contains("faber::trap::ok(risky(2), faber::Trap::FailableCall)"), "{main}");
    assert!(
        main.contains("faber::trap::ok(l_k.nimm(1), faber::Trap::FailableCall)"),
        "{main}"
    );
}

#[test]
fn ts_throws_on_an_unhandled_failable_call_in_main() {
    let code = emit(Target::HirTypeScript, &format!("{DECLS}{MAIN}"));
    let main = main_body(&code, "(() => {\n");
    let unwrap = "if (!r.ok) { throw new Error(typeof r.error === \"string\" ? r.error";
    assert_eq!(main.matches(unwrap).count(), 4, "check, two risky, one method:\n{main}");
    assert!(main.contains("})(check(1))"), "{main}");
    assert!(main.contains("})(risky(2))"), "{main}");
    assert!(main.contains("})(l_k.nimm(1))"), "{main}");
}

#[test]
fn go_panics_on_an_unhandled_failable_call_in_main() {
    let code = emit(Target::HirGo, &format!("{DECLS}{MAIN}"));
    let main = main_body(&code, "func main()");
    assert!(
        main.contains("func() { err := check(1); if err != nil { panic(err) } }()"),
        "{main}"
    );
    assert!(
        main.contains("l_x := func() int { v, err := risky(2); if err != nil { panic(err) }; return v }()"),
        "{main}"
    );
    assert!(
        main.contains("func() int { v, err := l_k.Nimm(1); if err != nil { panic(err) }; return v }()"),
        "{main}"
    );
}

#[test]
fn swift_aborts_on_an_unhandled_failable_call_in_main() {
    let code = emit(Target::HirSwift, &format!("{DECLS}{MAIN}"));
    let main = main_body(&code, "struct Main");
    assert!(main.contains("try! check(n: 1)"), "{main}");
    assert!(main.contains("try! risky(n: 2)"), "{main}");
    assert!(main.contains("try! l_k.nimm(n: 1)"), "{main}");
}

#[test]
fn python_raises_the_error_from_an_unhandled_failable_call_in_main() {
    let code = emit(Target::HirPython, &format!("{DECLS}{MAIN}"));
    let main = main_body(&code, "if __name__ == \"__main__\":");
    assert!(main.contains("check(1)"), "{main}");
    assert!(main.contains("x = risky(2)"), "{main}");
    assert!(code.contains("raise FaberError(\"negative input\")"), "{code}");
}

#[test]
fn a_handled_call_in_main_keeps_its_handler_on_every_backend() {
    let source = format!("{DECLS}{HANDLED}");
    let ts = emit(Target::HirTypeScript, &source);
    assert!(!ts.contains("throw new Error(typeof r.error"), "{ts}");
    assert!(ts.contains("const t"), "{ts}");
    let go = emit(Target::HirGo, &source);
    assert!(!go.contains("panic(err)"), "{go}");
    assert!(go.contains("x, err := risky(int(-1))"), "{go}");
    let swift = emit(Target::HirSwift, &source);
    assert!(!swift.contains("try!"), "{swift}");
    assert!(!swift.contains("fatalError(\"\\(e)\")"), "{swift}");
    assert!(swift.contains("} catch {"), "{swift}");
}

#[test]
fn a_failable_call_in_a_helper_is_still_rejected() {
    let helper = "
functio helper() → numerus {
    redde risky(1)
}
incipit { nota helper() }
";
    for target in [Target::HirRust, Target::HirTypeScript, Target::HirGo, Target::HirSwift] {
        let result = compile(target, &format!("{DECLS}{helper}"));
        assert!(!result.success(), "{target} must reject a failable call in a helper");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| format!("{diagnostic:?}").contains("failable_call_requires_handler")),
            "{target}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn swift_uses_try_bang_for_a_throwing_callee_in_main() {
    let source = r#"
discretio GuardError {
    Invalid { textus causa }
}

functio guarded(numerus value) → numerus ⇥ GuardError {
    si value ≺ 0 { iace finge Invalid { causa = "negative" } }
    redde value
}

incipit {
    nota guarded(2)
}
"#;
    let code = emit(Target::HirSwift, source);
    assert!(code.contains("try! guarded(value: 2)"), "{code}");
}
