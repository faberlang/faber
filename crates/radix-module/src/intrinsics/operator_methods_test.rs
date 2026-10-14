//! Tests for intrinsic operator method registration and codegen.
//! Proves: numeric, fractus, bitwise, shift operators are registered;
//! imperativus methods reject fixum receivers and allow `in` parameters;
//! codegen emits Rust glyph operations (not method calls).

use super::{
    IntrinsicReceiver, is_numeric_bitwise_operator, is_numeric_imperativus_operator, lookup_method,
    lookup_method_for_type,
};
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::semantic::{Primitive, Type, TypeTable};

#[cfg(feature = "hir-rust")]
fn compile_rust(name: &str, source: &str) -> String {
    let compiler = crate::Compiler::new(crate::Config::default().with_latin_locale());
    let result = compiler.compile_str(name, source);
    let Some(crate::Output::Rust(rust)) = result.output else {
        panic!("expected Rust output, got diagnostics: {:?}", result.diagnostics);
    };
    rust.code
}

fn rejects_imperativus_on_fixum(source: &str) {
    let session = Session::new(Config::default().with_latin_locale());
    let result = analyze_source(&session, "operator_methods.fab", source, AnalyzeRequest::default());
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
    assert!(code.contains("l_a + 2.0") || code.contains("l_a+2.0"));
    assert!(code.contains("l_a > 1.0") || code.contains("l_a>1.0"));
    assert!(code.contains("l_a <= 2.0") || code.contains("l_a<=2.0"));
    // ss1: `acc.adde(5.0)` is the assignment `acc ← acc + 5.0`.
    assert!(code.contains("l_acc = l_acc + 5.0"), "{code}");
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
    // ss1: `acc.coniunge(8)` is the assignment `acc ← acc ∧ 8`.
    assert!(
        code.contains("l_acc = faber::exact::fit::<i64>(faber::exact::int(Some((l_acc as i128) & 8_i128)))"),
        "{code}"
    );
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
    let session = Session::new(Config::default().with_latin_locale());
    let source = r"functio bump(in fractus acc) → vacuum {
    acc.adde(1.0)
}

incipit {
    varia fractus x ← 2.0
    bump(in x)
}";
    let result = analyze_source(&session, "in-imperativus.fab", source, AnalyzeRequest::default());
    assert!(result.is_ok(), "expected in-parameter imperativus to pass: {:?}", result.err());
}

#[cfg(feature = "hir-rust")]
#[test]
fn potentia_receiver_codegen_uses_ieee_powf() {
    let session = Session::new(Config::default().with_latin_locale());
    let source = r"incipit {
    fixum fractus b ← -2.0
    fixum fractus r ← b.potentia(2.0)
    nota r
}";
    assert!(
        analyze_source(&session, "potentia-guard.fab", source, AnalyzeRequest::default()).is_ok(),
        "potentia receiver should typecheck"
    );

    // IEEE 754 pow (operator ruling 2026-09-24): no silent `base ≤ 0 → 0` guard.
    let code = compile_rust("potentia-guard.fab", source);
    assert!(code.contains(".powf("), "fractus potentia must use Rust powf:\n{code}");
    assert!(!code.contains("<= 0.0") && !code.contains("<=0.0"), "no base guard:\n{code}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn numerus_potentia_codegen_uses_integer_pow_without_float_guard() {
    let source = r"incipit {
    fixum numerus base ← -2
    fixum numerus result ← base.potentia(3)
    nota result
}";
    let code = compile_rust("potentia-numerus.fab", source);
    assert!(
        code.contains("faber::exact::pow(l_base as i128, 3_i128)"),
        "integer potentia must use the exact i128 power helper:\n{code}"
    );
    assert!(
        !code.contains("<= 0.0"),
        "integer potentia must not carry the FLOAT guard:\n{code}"
    );
    assert!(
        !code.contains(".ln()"),
        "integer potentia must not use the float identity:\n{code}"
    );
    assert!(!code.contains(" as f64"), "integer potentia must not cast through f64:\n{code}");
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
    // The exact shifts are `faber::exact::shl` / `shr` (their bodies live in
    // the runtime crate); `sinistrata`/`sinistra` and `dextrata`/`dextra` each
    // emit one call.
    // ss1: `acc.sinistra(1)` is the assignment `acc ← acc ⇐ 1`.
    assert!(code.contains("acc = "), "{code}");
    assert!(code.matches("faber::exact::shl(").count() >= 2, "{code}");
    assert!(code.matches("faber::exact::shr(").count() >= 2, "{code}");
    assert!(!code.contains(".sinistrata("));
    assert!(!code.contains(".sinistra("));
}

#[test]
fn rejects_shift_operator_on_fractus_receiver() {
    let session = Session::new(Config::default().with_latin_locale());
    let result = analyze_source(
        &session,
        "shift-fractus.fab",
        r"incipit {
    fixum fractus x ← 1.0
    fixum _ y ← x.sinistrata(1.0)
}",
        AnalyzeRequest::default(),
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
    let session = Session::new(Config::default().with_latin_locale());
    let result = analyze_source(
        &session,
        "bitwise-fractus.fab",
        r"incipit {
    fixum fractus x ← 1.0
    fixum _ y ← x.coniuncta(1.0)
}",
        AnalyzeRequest::default(),
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

#[cfg(feature = "hir-rust")]
#[test]
fn sized_integer_absolutum_and_potentia_emit_exact_i128_values() {
    let source = r"incipit {
    fixum i8 n ← -128
    fixum u8 m ← n.absolutum()
    fixum i32 p ← 100000
    nota m, p.potentia(2)
}";
    let code = compile_rust("abs-power-sized.fab", source);
    assert!(
        code.contains("faber::exact::fit::<u8>(faber::exact::int(Some((l_n as i128).abs())))"),
        "sized absolutum must compute |x| exactly in i128:\n{code}"
    );
    assert!(
        code.contains("faber::exact::pow(l_p as i128, 2_i128)"),
        "sized potentia must use the exact i128 power helper:\n{code}"
    );
    assert!(
        !code.contains("n.abs()") && !code.contains(".pow("),
        "no native narrow ops:\n{code}"
    );
}
