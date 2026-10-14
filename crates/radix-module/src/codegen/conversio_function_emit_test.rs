//! D9.3 emission pins: an explicit `↦` through a registered `@ conversio`
//! pair calls the function on every HIR backend; a failable function's `⊥`
//! default recovers the failed result, and backends without an
//! expression-position failable carrier fail closed on propagation.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const DECLS: &str = "
genus Celsius { varia fractus gradus }
genus Fahrenheit { varia fractus gradus }
genus Lectio { varia numerus valor }
genus Mensura { varia numerus valor }

@ conversio
functio adFahrenheit(Celsius c) → Fahrenheit {
    redde Fahrenheit { gradus = c.gradus }
}

@ conversio
functio adMensuram(Lectio l) → Mensura ⇥ textus {
    si l.valor ≺ 0 { iace \"lectio negativa\" }
    redde Mensura { valor = l.valor }
}
";

const USES: &str = "
incipit {
    fixum Fahrenheit f ← Celsius { gradus = 1.0 } ↦ Fahrenheit
    nota f.gradus
    fixum Mensura m ← Lectio { valor = -4 } ↦ Mensura ⊥ Mensura { valor = 0 }
    nota m.valor
}
";

const PROPAGATES: &str = "
functio mensura(Lectio l) → Mensura ⇥ textus {
    redde l ↦ Mensura
}
incipit { nota 1 }
";

fn compile(target: Target, source: &str) -> crate::CompileResult {
    let session = Session::new(Config::default().with_target(target).with_latin_locale());
    crate::driver::compile(&session, "conversio.fab", source, ModuleCompileRequest::default())
}

fn emit(target: Target, source: &str) -> String {
    let result = compile(target, source);
    assert!(result.success(), "{target} compilation failed: {:?}", result.diagnostics);
    match result.output.expect("output") {
        crate::Output::Rust(out) => out.code,
        crate::Output::Go(out) => out.code,
        crate::Output::TypeScript(out) => out.code,
        crate::Output::Swift(out) => out.code,
        _ => panic!("unexpected output for {target}"),
    }
}

#[test]
fn rust_calls_the_conversio_function() {
    let code = emit(Target::HirRust, &format!("{DECLS}{USES}"));
    assert!(code.contains("let f: Fahrenheit = adFahrenheit(Celsius {"), "{code}");
    assert!(code.contains("}).unwrap_or_else(|_| Mensura {"), "{code}");
    let code = emit(Target::HirRust, &format!("{DECLS}{PROPAGATES}"));
    assert!(code.contains("adMensuram(l.clone())?"), "{code}");
}

#[test]
fn go_calls_the_conversio_function() {
    let code = emit(Target::HirGo, &format!("{DECLS}{USES}"));
    assert!(code.contains("f := adFahrenheit(&Celsius{"), "{code}");
    assert!(code.contains("faberConversio, faberConversioErr := adMensuram("), "{code}");
    let result = compile(Target::HirGo, &format!("{DECLS}{PROPAGATES}"));
    assert!(!result.success());
}

#[test]
fn ts_calls_the_conversio_function() {
    let code = emit(Target::HirTypeScript, &format!("{DECLS}{USES}"));
    assert!(code.contains("const f: Fahrenheit = adFahrenheit("), "{code}");
    assert!(code.contains("const __r = adMensuram("), "{code}");
    assert!(code.contains("return __r.ok ? __r.value : "), "{code}");
    let result = compile(Target::HirTypeScript, &format!("{DECLS}{PROPAGATES}"));
    assert!(!result.success());
}

#[test]
fn swift_calls_an_infallible_conversio_function() {
    let code = emit(
        Target::HirSwift,
        &format!(
            "{DECLS}incipit {{\n    fixum Fahrenheit f ← Celsius {{ gradus = 1.0 }} ↦ Fahrenheit\n    nota f.gradus\n}}\n"
        ),
    );
    assert!(code.contains("let f: Fahrenheit = adFahrenheit(c: Celsius("), "{code}");
}
