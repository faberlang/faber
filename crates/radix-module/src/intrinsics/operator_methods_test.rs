//! Tests for intrinsic operator method registration and codegen.
//! Proves: numeric, fractus, bitwise, shift operators are registered;
//! imperativus methods reject fixum receivers and allow `in` parameters;
//! codegen emits Rust glyph operations (not method calls).

use super::{
    is_numeric_bitwise_operator, is_numeric_imperativus_operator, lookup_method, lookup_method_for_type,
    IntrinsicReceiver,
};
use crate::driver::{analyze_source, Config, Session};
use crate::semantic::{Primitive, Type, TypeTable};

#[cfg(feature = "hir-rust")]
fn compile_rust(name: &str, source: &str) -> String {
    let compiler = crate::Compiler::new(crate::Config::default());
    let result = compiler.compile_str(name, source);
    let Some(crate::Output::Rust(rust)) = result.output else {
        panic!("expected Rust output, got diagnostics: {:?}", result.diagnostics);
    };
    rust.code
}

fn rejects_imperativus_on_fixum(source: &str) {
    let session = Session::new(Config::default());
    let result = analyze_source(&session, "operator_methods.fab", source);
    let Err(diagnostics) = result else {
        panic!("expected typecheck failure for fixum imperativus receiver");
    };
    assert!(
        diagnostics
            .iter()
            .any(|d| d.args.iter().any(|arg| {
                arg.name == "issue" && arg.value == "imperativus_operator_method_requires_varia_receiver"
            })),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn numeric_methods_addita_adde_etc_are_registered() {
    assert!(lookup_method(IntrinsicReceiver::Numeric, "addita", 1).is_some());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "adde", 1).is_some());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "minor", 1).is_some());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "coniuncta", 1).is_some());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "sinistrata", 1).is_some());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "dextra", 1).is_some());
}

#[test]
fn imperativus_operators_adde_and_sinistra_are_registered() {
    assert!(is_numeric_imperativus_operator("adde"));
    assert!(is_numeric_imperativus_operator("sinistra"));
}

#[test]
fn bitwise_operators_coniuncta_and_sinistrata_are_registered() {
    assert!(is_numeric_bitwise_operator("coniuncta"));
    assert!(is_numeric_bitwise_operator("sinistrata"));
}

#[test]
fn lookup_method_with_wrong_arity_returns_none() {
    assert!(lookup_method(IntrinsicReceiver::Numeric, "addita", 0).is_none());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "addita", 2).is_none());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "adde", 0).is_none());
    assert!(lookup_method(IntrinsicReceiver::Numeric, "coniuncta", 0).is_none());
    assert!(lookup_method(IntrinsicReceiver::Fractus, "addita", 0).is_none());
}

#[test]
fn fractus_receiver_resolves_numeric_operator_rows() {
    let types = TypeTable::new();
    let receiver = types.get(types.primitive(Primitive::Fractus));
    assert!(lookup_method_for_type(receiver, "addita", 1).is_some());
    assert!(lookup_method_for_type(receiver, "minime", 1).is_some());
    assert!(lookup_method_for_type(receiver, "rotunda", 0).is_some());
}

#[test]
fn modular_word_receiver_resolves_only_explicitly_lowered_numeric_rows() {
    let mut types = TypeTable::new();
    let receiver_id = types.intern(Type::ModularWord(crate::semantic::NumericWidth::U32));
    let receiver = types.get(receiver_id);
    assert!(lookup_method_for_type(receiver, "addita", 1).is_some());
    assert!(lookup_method_for_type(receiver, "sinistrata", 1).is_some());
    assert!(lookup_method_for_type(receiver, "dextra", 1).is_some());
    assert!(lookup_method_for_type(receiver, "potentia", 1).is_none());
}

#[cfg(feature = "hir-rust")]
#[test]
fn emits_numeric_operator_methods_as_rust_glyph_operations() {
    let source = r"
incipit {
    fixum fractus a ← 1.5
    fixum _ sum ← a.addita(2.0)
    fixum _ gt ← a.maior(1.0)
    fixum _ le ← a.minime(2.0)
    varia fractus acc ← 10.0
    acc.adde(5.0)
    nota sum, gt, le, acc
}
";

    let code = compile_rust("numeric-operator-methods.fab", source);
    assert!(code.contains("a + 2.0") || code.contains("a+2.0"));
    assert!(code.contains("a > 1.0") || code.contains("a>1.0"));
    assert!(code.contains("a <= 2.0") || code.contains("a<=2.0"));
    assert!(code.contains("acc += 5.0") || code.contains("acc+=5.0"));
    assert!(!code.contains(".addita("));
    assert!(!code.contains(".adde("));
}

#[test]
fn rejects_imperativus_addita_on_fixum_receiver() {
    rejects_imperativus_on_fixum(
        r"incipit {
    fixum fractus x ← 1.0
    x.adde(2.0)
}",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn emits_bitwise_operator_methods_as_rust_glyph_operations() {
    let source = r"
incipit {
    fixum numerus mask ← 12
    fixum _ bits ← mask.coniuncta(10)
    fixum _ inv ← mask.complementata()
    varia numerus acc ← 15
    acc.coniunge(8)
    nota bits, inv, acc
}
";

    let code = compile_rust("bitwise-operator-methods.fab", source);
    assert!(code.contains(" & ") || code.contains('&'));
    assert!(code.contains('!'));
    assert!(code.contains("&=") || code.contains("& ="));
    assert!(!code.contains(".coniuncta("));
    assert!(!code.contains(".coniunge("));
}

#[test]
fn rejects_imperativus_nega_on_fixum_receiver() {
    rejects_imperativus_on_fixum(
        r"incipit {
    fixum fractus x ← 1.0
    x.nega()
}",
    );
}

#[test]
fn rejects_imperativus_coniunge_on_fixum_receiver() {
    rejects_imperativus_on_fixum(
        r"incipit {
    fixum numerus x ← 1
    x.coniunge(2)
}",
    );
}

#[test]
fn accepts_imperativus_on_in_parameter_receiver() {
    let session = Session::new(Config::default());
    let source = r"functio bump(in fractus acc) → vacuum {
    acc.adde(1.0)
}

incipit {
    varia fractus x ← 2.0
    bump(x)
}";
    let result = analyze_source(&session, "in-imperativus.fab", source);
    assert!(result.is_ok(), "expected in-parameter imperativus to pass: {:?}", result.err());
}

#[cfg(feature = "hir-rust")]
#[test]
fn potentia_receiver_codegen_applies_non_positive_base_guard() {
    let session = Session::new(Config::default());
    let source = r"incipit {
    fixum fractus b ← -2.0
    fixum fractus r ← b.potentia(2.0)
    nota r
}";
    assert!(
        analyze_source(&session, "potentia-guard.fab", source).is_ok(),
        "potentia receiver should typecheck"
    );

    let code = compile_rust("potentia-guard.fab", source);
    assert!(code.contains("<= 0.0") || code.contains("<=0.0"));
    assert!(!code.contains(".powf("));
}

#[cfg(feature = "hir-rust")]
#[test]
fn emits_shift_operator_methods_as_rust_glyph_operations() {
    let source = r"
incipit {
    fixum numerus n ← 1
    fixum _ left ← n.sinistrata(4)
    fixum _ right ← n.dextrata(2)
    varia numerus acc ← 16
    acc.sinistra(1)
    acc.dextra(1)
    nota left, right, acc
}
";

    let code = compile_rust("shift-operator-methods.fab", source);
    assert!(code.contains("<<"));
    assert!(code.contains(">>"));
    assert!(code.contains("<<=") || code.contains("<< ="));
    assert!(code.contains(">>=") || code.contains(">> ="));
    assert!(!code.contains(".sinistrata("));
    assert!(!code.contains(".sinistra("));
}

#[test]
fn rejects_shift_operator_on_fractus_receiver() {
    let session = Session::new(Config::default());
    let result = analyze_source(
        &session,
        "shift-fractus.fab",
        r"incipit {
    fixum fractus x ← 1.0
    fixum _ y ← x.sinistrata(1.0)
}",
    );
    let Err(diagnostics) = result else {
        panic!("expected shift fractus rejection");
    };
    assert!(
        diagnostics.iter().any(|d| d
            .args
            .iter()
            .any(|arg| { arg.name == "issue" && arg.value == "bitwise_method_requires_numerus_receiver" })),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn rejects_bitwise_operator_on_fractus_receiver() {
    let session = Session::new(Config::default());
    let result = analyze_source(
        &session,
        "bitwise-fractus.fab",
        r"incipit {
    fixum fractus x ← 1.0
    fixum _ y ← x.coniuncta(1.0)
}",
    );
    let Err(diagnostics) = result else {
        panic!("expected bitwise fractus rejection");
    };
    assert!(
        diagnostics.iter().any(|d| d
            .args
            .iter()
            .any(|arg| { arg.name == "issue" && arg.value == "bitwise_method_requires_numerus_receiver" })),
        "diagnostics: {diagnostics:?}"
    );
}
