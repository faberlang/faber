//! D9.4 emission pins: every HIR backend lowers the typecheck-selected `ordo`
//! conversion row (discriminant, member name, value → member) instead of
//! rejecting the pair or emitting an unchecked cast.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "
ordo Status { pendens = 10, actum, finitum = -3 }

incipit {
    nota Status.actum ↦ numerus
    nota Status.finitum ↦ textus
    fixum Status s ← 11 ↦ Status ⊥ Status.pendens
    nota s ↦ numerus
}
";

fn emit(target: Target) -> String {
    let session = Session::new(Config::default().with_target(target).with_latin_locale());
    let result = crate::driver::compile(&session, "ordo.fab", SOURCE, ModuleCompileRequest::default());
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
fn rust_emits_ordo_rows_as_matches() {
    let code = emit(Target::HirRust);
    assert!(code.contains("Status::actum => 11i64"), "{code}");
    assert!(code.contains("Status::finitum => String::from(\"finitum\")"), "{code}");
    assert!(code.contains("11 => Some(Status::actum)"), "{code}");
    assert!(code.contains("-3 => Some(Status::finitum)"), "{code}");
    assert!(code.contains(".unwrap_or(Status::pendens)"), "{code}");
}

#[test]
fn go_emits_ordo_rows_as_switches() {
    let code = emit(Target::HirGo);
    assert!(code.contains("case actum: return 11;"), "{code}");
    assert!(code.contains("case finitum: return \"finitum\";"), "{code}");
    assert!(code.contains("case -3: return finitum{};"), "{code}");
    assert!(code.contains("return pendens{} }()"), "{code}");
}

#[test]
fn ts_emits_ordo_rows_as_switches() {
    let code = emit(Target::HirTypeScript);
    assert!(code.contains("case \"actum\": return 11;"), "{code}");
    assert!(code.contains("case \"finitum\": return \"finitum\";"), "{code}");
    assert!(code.contains("case 11: return actum;"), "{code}");
    assert!(code.contains("return pendens; })(11)"), "{code}");
}

#[test]
fn swift_emits_ordo_rows_as_switches() {
    let code = emit(Target::HirSwift);
    assert!(code.contains("case .actum: return 11;"), "{code}");
    assert!(code.contains("case .finitum: return \"finitum\";"), "{code}");
    assert!(code.contains("case 11: return Status.actum;"), "{code}");
    assert!(code.contains("default: return Status.pendens"), "{code}");
}
