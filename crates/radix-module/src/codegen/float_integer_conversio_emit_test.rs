//! D1.11 / D11.8 emission pins: `fractus ↦ numerus<W>` truncates toward zero
//! and fails (never saturates or wraps natively) on NaN, ±Inf, or a truncation
//! outside `W`; `⊥` supplies the recovery. Every HIR backend that lowers the
//! row carries the runner's failure text.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const MESSAGE: &str = "fractus to numerus conversion out of range";

const SOURCE: &str = "
incipit {
    fixum fractus d ← 300.0
    fixum u8 r ← d ↦ u8 via Trunc ⊥ 7
    nota r
    fixum u8 t ← d ↦ u8 via Trunc
    nota t
}
";

fn emit(target: Target) -> String {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let result = crate::driver::compile(&session, "float-int.fab", SOURCE, ModuleCompileRequest::default());
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

#[test]
fn rust_checks_float_truncation_and_honors_recovery() {
    let code = emit(Target::HirRust);
    assert!(code.contains("x0.is_finite()"), "{code}");
    assert!(code.contains("<u8 as TryFrom<i128>>::try_from(x0.trunc() as i128)"), "{code}");
    assert!(code.contains(".unwrap_or(7"), "{code}");
    assert!(
        code.contains("faber::trap::some(") && code.contains("faber::Trap::OutOfRange(\"fractus\", \"numerus\")"),
        "{code}"
    );
    assert!(!code.contains("d as u8"), "no saturating `as` cast: {code}");
}

#[test]
fn go_checks_float_truncation_and_honors_recovery() {
    let code = emit(Target::HirGo);
    assert!(code.contains("math.Trunc(float64(d))"), "{code}");
    assert!(
        code.contains("math.IsNaN(v) || math.IsInf(v, 0) || v < 0.0 || v >= 256.0"),
        "{code}"
    );
    assert!(code.contains("{ return uint8(7) }; return uint8(v) }()"), "{code}");
    assert!(code.contains(&format!("panic(\"{MESSAGE}\")")), "{code}");
    assert!(code.contains("\"math\""), "{code}");
}

#[test]
fn ts_checks_float_truncation_and_honors_recovery() {
    let code = emit(Target::HirTypeScript);
    assert!(
        code.contains("!Number.isFinite(__v) || __v < 0 || __v >= 256) ? 7 : __v)(Math.trunc(d))"),
        "{code}"
    );
    assert!(code.contains(&format!("throw new Error(\"{MESSAGE}\")")), "{code}");
}

#[test]
fn swift_checks_float_truncation_and_honors_recovery() {
    let code = emit(Target::HirSwift);
    assert!(
        code.contains("let t = v.rounded(.towardZero); guard t.isFinite, t >= 0.0, t < 256.0"),
        "{code}"
    );
    assert!(code.contains("else { return 7 }; return UInt8(t) })(d)"), "{code}");
    assert!(code.contains(&format!("fatalError(\"{MESSAGE}\")")), "{code}");
    assert!(!code.contains("UInt8(d)"), "no trapping native initializer: {code}");
}

#[test]
fn python_checks_float_truncation_and_honors_recovery() {
    let code = emit(Target::HirPython);
    assert!(
        code.contains("def _faber_float_int(value, lo, hi, recover, mode=None):"),
        "{code}"
    );
    assert!(code.contains("_faber_float_int(d, 0, 255, lambda: 7, \"trunc\")"), "{code}");
    assert!(code.contains("_faber_float_int(d, 0, 255, None, \"trunc\")"), "{code}");
    assert!(code.contains(&format!("raise FaberError(\"{MESSAGE}\")")), "{code}");
    assert!(code.contains("class FaberError(Exception):"), "{code}");
}
