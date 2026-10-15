//! audit1-a2 emission pins: `value!.method(..)` is a method call on the
//! asserted payload. Before the HIR normalisation the Rust emit was
//! `s.length.clone()()` (a field call), TS kept the Faber method name, and Go
//! emitted `s.Length()` on a `*string`; every backend now sees an ordinary
//! `MethodCall` whose receiver is `NonNull(value, Value)`.
//!
//! Targets that cannot express the assertion reject it by name.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = r"
functio involve(lista<numerus> v) → lista<numerus> ∪ nihil { redde v }
functio salve() → textus ∪ nihil { redde 'salve' }

incipit {
    sit lista<numerus> base ← [1, 2]
    varia lista<numerus> ∪ nihil xs ← involve(base)
    xs!.appende(3)
    nota xs!.longitudo()
    nota xs![0]
    sit textus ∪ nihil t ← salve()
    nota t!.longitudo()
}
";

fn compile(target: Target) -> crate::CompileResult {
    let session = Session::new(Config::default().with_target(target).with_latin_locale());
    crate::driver::compile(&session, "non-null-call.fab", SOURCE, ModuleCompileRequest::default())
}

fn emit(target: Target) -> String {
    let result = compile(target);
    assert!(result.success(), "{target} compilation failed: {:?}", result.diagnostics);
    match result.output.expect("output") {
        crate::Output::Rust(out) => out.code,
        crate::Output::Go(out) => out.code,
        crate::Output::TypeScript(out) => out.code,
        crate::Output::Faber(out) => out.code,
        crate::Output::Swift(out) => out.code,
        _ => panic!("unexpected output for {target}"),
    }
}

fn main_body(code: &str, marker: &str) -> String {
    code.split(marker).nth(1).expect("main marker").to_owned()
}

#[test]
fn rust_emits_method_calls_on_the_asserted_payload() {
    let code = emit(Target::HirRust);
    let main = main_body(&code, "fn main()");
    assert!(main.contains("let mut l_xs: Option<Vec<i64>>"), "{main}");
    // A mutating receiver borrows the stored value; a read borrows shared.
    assert!(
        main.contains(r"faber::trap::some((l_xs).as_mut(), faber::Trap::Nonnull).push(3);"),
        "{main}"
    );
    assert!(
        main.contains(r"faber::trap::some((l_xs).as_ref(), faber::Trap::Nonnull).len() as i64"),
        "{main}"
    );
    assert!(
        main.contains(r"faber::trap::some((l_t).as_ref(), faber::Trap::Nonnull).chars().count() as i64"),
        "{main}"
    );
    assert!(!main.contains("length.clone()()"), "{main}");
}

#[test]
fn ts_emits_method_calls_on_the_asserted_payload() {
    let code = emit(Target::HirTypeScript);
    assert!(code.contains("l_xs!.push(3);"), "{code}");
    // A list length stays `.length`; a text length counts Unicode characters
    // on every target, so the asserted text payload is walked by code point.
    assert!(code.contains("l_xs!.length"), "{code}");
    assert!(code.contains("Array.from(l_t!).length"), "{code}");
    assert!(!code.contains("l_t!.length"), "{code}");
    assert!(!code.contains(".length()"), "{code}");
}

#[test]
fn go_emits_method_calls_through_the_pointer_storage() {
    let code = emit(Target::HirGo);
    assert!(code.contains("(*l_xs) = append((*l_xs), 3)"), "{code}");
    assert!(code.contains("len((*l_xs))"), "{code}");
    assert!(code.contains("len([]rune((*l_t)))"), "{code}");
    // Indexing an optional list dereferences the pointer too.
    assert!(code.contains("(*l_xs)[0]"), "{code}");
    assert!(!code.contains(".Length()"), "{code}");
}

#[test]
fn swift_emits_the_assertion_as_a_force_unwrap() {
    // Operator ruling 2026-10-02: an explicit source assertion is Swift's `!`,
    // including a mutating receiver (`xs!.append(3)` mutates in place).
    let code = emit(Target::HirSwift);
    assert!(code.contains("xs!.append(3)"), "{code}");
    assert!(code.contains("xs!.count"), "{code}");
    assert!(code.contains("xs![0]"), "{code}");
    assert!(code.contains("t!.unicodeScalars.count"), "{code}");
}

#[test]
fn faber_round_trips_the_non_null_method_spelling() {
    let code = emit(Target::HirFaber);
    assert!(code.contains("xs!.appende(3)"), "{code}");
    assert!(code.contains("xs!.longitudo()"), "{code}");
    assert!(code.contains("t!.longitudo()"), "{code}");
}

#[test]
fn go_rejects_a_non_null_receiver_it_cannot_write_through() {
    // `inverte` has only a binding-place statement form in Go; a `!` receiver
    // is rejected by name instead of emitting a non-compiling method call.
    let source = r"
functio involve(lista<numerus> v) → lista<numerus> ∪ nihil { redde v }
incipit {
    varia lista<numerus> ∪ nihil xs ← involve([1, 2])
    nota xs!.inverte()
}
";
    let session = Session::new(
        Config::default()
            .with_target(Target::HirGo)
            .with_latin_locale(),
    );
    let result = crate::driver::compile(&session, "non-null-go-reject.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| format!("{d:?}").contains("go_non_null_mutating_receiver_unsupported")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn unsupported_targets_reject_the_assertion_by_name() {
    // The Python leaf lowers the assertion (a `_faber_nonnull` guard), so only
    // the targets with no lowering remain here.
    for (target, issue) in [(Target::HirHaskell, "haskell_target_unsupported")] {
        let result = compile(target);
        assert!(!result.success(), "{target} must not accept `!.`");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| format!("{d:?}").contains(issue)),
            "{target}: {:?}",
            result.diagnostics
        );
    }
}
