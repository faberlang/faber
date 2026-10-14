//! D10.6 emission pins: every HIR backend lowers the `littera` text rows
//! (digits, digit parse, the only letter) and the `Code` hint as checked
//! conversions — never an unchecked cast or a type assertion.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "
incipit {
    fixum ascii a ← 42 ↦ ascii
    fixum littera d ← 7 ↦ littera ⊥ '?'
    fixum numerus n ← d ↦ numerus ⊥ 0
    fixum textus t ← \"x\"
    fixum littera u ← t ↦ littera ⊥ '?'
    fixum u32 c ← d ↦ u32 via Code
    fixum littera b ← 65 ↦ littera via Code ⊥ '?'
    fixum octeti raw ← |68 69|
    fixum ascii h ← raw[0‥1] ↦ ascii ⊥ 'nn'
    nota a
    nota n
    nota u
    nota c
    nota b
    nota h
}
";

fn emit(target: Target) -> String {
    let session = Session::new(Config::default().with_target(target).with_latin_locale());
    let result = crate::driver::compile(&session, "littera.fab", SOURCE, ModuleCompileRequest::default());
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
fn rust_emits_littera_rows() {
    let code = emit(Target::HirRust);
    assert!(code.contains("faber::Ascii::new(&42.to_string())"), "{code}");
    assert!(code.contains("char::from_digit(digit, 10)"), "{code}");
    assert!(code.contains(".to_digit(10).map(|digit| digit as i64)"), "{code}");
    assert!(code.contains("scalars.next().filter(|_| scalars.next().is_none())"), "{code}");
    assert!(code.contains("d as u32"), "{code}");
    assert!(code.contains("and_then(char::from_u32)"), "{code}");
}

#[test]
fn go_emits_littera_rows() {
    let code = emit(Target::HirGo);
    assert!(code.contains("fmt.Sprint(42)"), "{code}");
    assert!(code.contains("return rune('0' + v)"), "{code}");
    assert!(code.contains("return int(v - '0')"), "{code}");
    assert!(code.contains("v := []rune(l_t)"), "{code}");
    assert!(code.contains("uint32(l_d)"), "{code}");
    assert!(code.contains("(v >= 0xD800 && v <= 0xDFFF)"), "{code}");
    assert!(code.contains("append([]byte(nil), __xs[__s:__e]...)"), "{code}");
}

#[test]
fn ts_emits_littera_rows() {
    let code = emit(Target::HirTypeScript);
    assert!(code.contains("String(42)"), "{code}");
    assert!(code.contains("v > 9) { return"), "{code}");
    assert!(code.contains("/^[0-9]$/.test(c)"), "{code}");
    assert!(code.contains("const scalars = [...s]"), "{code}");
    assert!(code.contains(".codePointAt(0) as number"), "{code}");
    assert!(code.contains("String.fromCodePoint(v)"), "{code}");
    assert!(code.contains("return __o.slice(__s, __e)"), "{code}");
}

#[test]
fn swift_emits_littera_rows() {
    let code = emit(Target::HirSwift);
    assert!(code.contains("Array(String(42).utf8)"), "{code}");
    assert!(code.contains("Unicode.Scalar(UInt8(48 + v))"), "{code}");
    assert!(code.contains("guard v >= 48, v <= 57"), "{code}");
    assert!(code.contains(".unicodeScalars); guard scalars.count == 1"), "{code}");
    assert!(code.contains("(l_d).value"), "{code}");
    assert!(code.contains("let s = Unicode.Scalar(UInt32(v))"), "{code}");
}
