//! Type-conversion (conversio) codegen tests. Each compiles a small Faber program
//! to Rust and asserts the emitted conversion pattern.

use super::compile;
use crate::codegen::Target;
use crate::driver::ModuleCompileRequest;
use crate::test_support::test_session_la;

// ── conversio glyph form ──

#[cfg(feature = "hir-rust")]
#[test]
fn conversio_glyph_form_compiles_to_parse() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit textus raw ← "22"
  sit _ n ← raw ↦ numerus
  nota n
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".parse::<i64>().unwrap()"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn conversio_of_a_literal_emits_its_value() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit _ n ← "22" ↦ numerus
  nota n
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("let l_n: i64 = 22_i64;"));
    assert!(!output.code.contains(".parse::<i64>()"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn conversio_emits_parse_not_as_cast() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit textus raw ← "22"
  sit _ n ← raw ↦ numerus
  nota n
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".parse::<i64>().unwrap()"));
    assert!(!output.code.contains(" as i64"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn conversio_with_recovery_emits_unwrap_or() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit _ n ← "bad" ↦ numerus ⊥ 0
  nota n
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".parse::<i64>().unwrap_or(0)"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn conversio_to_textus_emits_to_string() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit _ n ← 42
  sit _ s ← n ↦ textus
  nota s
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".to_string()"));
}

// ── textus / octeti conversio ──

#[cfg(feature = "hir-rust")]
#[test]
fn textus_to_octeti_conversio_emits_into_bytes() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit _ s ← "hi"
  sit _ b ← s ↦ octeti
  nota b
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(code.contains(".into_bytes()"), "expected into_bytes, got:\n{code}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn octeti_to_textus_conversio_emits_from_utf8_with_expect() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit _ b ← |68 69|
  sit _ s ← b ↦ textus
  nota s
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(code.contains("std::str::from_utf8("), "expected from_utf8, got:\n{code}");
    assert!(
        code.contains("faber::trap::ok(std::str::from_utf8(")
            && code.contains("faber::Trap::Other(\"octeti to textus conversion failed: invalid UTF-8\")"),
        "expected expect, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn octeti_to_textus_conversio_with_recovery_emits_unwrap_or() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit _ b ← |ff|
  sit _ s ← b ↦ textus ⊥ "?"
  nota s
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".map(|s| s.to_owned()).unwrap_or("),
        "expected unwrap_or, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn ascii_octeti_conversio_emits_correct_lowering() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit _ a ← 'hi'
  sit _ b ← a ↦ octeti
  sit _ back ← b ↦ ascii
  nota back
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".to_textus().into_bytes()"),
        "expected ascii→octeti lowering, got:\n{code}"
    );
    assert!(
        code.contains("faber::Ascii::try_from_bytes("),
        "expected octeti→ascii lowering, got:\n{code}"
    );
}

// ── scalar conversio ──

#[cfg(feature = "hir-rust")]
#[test]
fn fractus_to_numerus_conversio_emits_checked_truncation() {
    // D1.11: float → integer `↦ via Trunc` truncates toward zero and fails when the value is
    // NaN, ±Inf, or out of the target range — never a saturating `as` cast.
    let session = test_session_la(Target::HirRust);
    let source = "incipit { sit _ n ← 3.7 ↦ numerus via Trunc }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(code.contains(".trunc()"), "expected truncation, got:\n{code}");
    assert!(code.contains("is_finite()"), "expected NaN/Inf check, got:\n{code}");
    assert!(
        code.contains("TryFrom<i128>"),
        "expected checked range conversion, got:\n{code}"
    );
    assert!(
        code.contains("faber::Trap::OutOfRange(\"fractus\", \"numerus\")"),
        "expected the runner's failure text, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn fractus_to_bivalens_conversio_emits_nonzero_check() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit { sit _ b ← 1.5 ↦ bivalens }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(code.contains(" != 0.0"), "expected nonzero check, got:\n{code}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn bivalens_to_numerus_conversio_emits_bool_cast() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit { sit _ n ← verum ↦ numerus }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(code.contains(" as i64"), "expected bool cast, got:\n{code}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn bivalens_to_fractus_conversio_emits_conditional_literals() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit { sit _ f ← verum ↦ fractus }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("if ") && code.contains("{ 1.0 as f64 } else { 0.0 as f64 }"),
        "expected conditional fractus lowering, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn ascii_to_bivalens_conversio_parses_the_locale_words() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit { sit _ b ← 'hi' ↦ bivalens }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("\"verum\" => true")
            && code.contains("\"falsum\" => false")
            && code.contains("panic!(\"text to bivalens conversion failed\")"),
        "expected the code locale's true/false words and failure arm, got:\n{code}"
    );
    assert!(!code.contains(".is_empty()"), "text truthiness is retired, got:\n{code}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn instans_narrow_conversio_emits_ad_praecisionem() {
    let session = test_session_la(Target::HirRust);
    let source = "functio narrow(instans<ns> t) → instans { redde t ↦ instans }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".ad_praecisionem(faber::InstansPraecisio::Secunda)"),
        "expected instans narrowing via ad_praecisionem, got:\n{code}"
    );
}

// ── valor conversio codegen ──

#[cfg(feature = "hir-rust")]
#[test]
fn valor_to_lista_conversio_emits_from_valor() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit valor v ← [1, 2, 3]
  sit lista<numerus> xs ← v ↦ lista<numerus>
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("<Vec<i64> as faber::FromValor>::from_valor"),
        "expected valor lista extraction, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn valor_to_tabula_conversio_emits_from_valor() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit valor v ← { "a": 1 } ↦ valor
  sit tabula<textus, numerus> m ← v ↦ tabula<textus, numerus>
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("<std::collections::HashMap<String, i64> as faber::FromValor>::from_valor"),
        "expected valor tabula extraction, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn genus_to_valor_conversio_emits_tabula_boxing() {
    let session = test_session_la(Target::HirRust);
    let source = r"genus Point { varia numerus x varia numerus y }
incipit {
  sit Point pt ← Point { x = 1, y = 2 }
  sit valor v ← pt ↦ valor
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("faber::Valor::Tabula") && code.contains("faber::Valor::from(__s.x"),
        "expected genus tabula boxing, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn lista_to_valor_conversio_emits_from() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit lista<numerus> xs ← [1, 2]
  sit valor v ← xs ↦ valor
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("faber::Valor::from("),
        "expected lista valor boxing, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn octeti_to_valor_conversio_emits_byte_list_boxing() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit octeti bytes ← |de ad|
  sit valor v ← bytes ↦ valor
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("into_iter().map(|__b| __b as i64).collect::<Vec<i64>>()"),
        "expected octeti valor byte-list boxing, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn valor_to_numerus_conversio_emits_from_valor() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit valor v ← 42
  sit numerus n ← v ↦ numerus
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("<i64 as faber::FromValor>::from_valor")
            && code.contains("faber::Trap::Conversion(\"valor\", \"numerus\")"),
        "expected valor numerus extraction, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn valor_to_fractus_conversio_emits_from_valor_with_widen() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit valor v ← 7
  sit fractus f ← v ↦ fractus
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("<f64 as faber::FromValor>::from_valor"),
        "expected valor fractus extraction, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn valor_to_octeti_conversio_emits_octeti_match() {
    let session = test_session_la(Target::HirRust);
    let source = r#"
incipit {
    # Ruling 2026-10-10 (222376cb-4): a bare empty `{}` at an infer slot is
    # the double hole, so the valor source carries a typed cell.
    sit valor body ← { "k": 1 } ↦ valor
    sit octeti bytes ← body ↦ octeti
    nota bytes.longitudo()
}
"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("faber::Valor::Octeti(__bytes)")
            && code.contains("faber::Trap::Conversion(\"valor\", \"octeti\")"),
        "expected valor octeti extraction, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn valor_to_textus_conversio_emits_from_valor() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit valor v ← "1979-05-27T07:32:00Z"
  sit textus s ← v ↦ textus
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("<String as faber::FromValor>::from_valor"),
        "expected valor textus extraction, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn valor_to_numerus_conversio_with_recovery_emits_unwrap_or() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit valor v ← "bad"
  sit numerus n ← v ↦ numerus ⊥ 0
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("<i64 as faber::FromValor>::from_valor") && code.contains(".unwrap_or("),
        "expected valor extraction with ⊥ recovery, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn valor_to_instans_conversio_emits_try_from_valor() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit valor v ← "1979-05-27T07:32:00Z"
  sit instans<ms> t ← v ↦ instans<ms>
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("faber::Instans::try_from_valor") && code.contains("faber::InstansPraecisio::Millisecunda"),
        "expected valor extraction with target precision, got:\n{code}"
    );
}

#[test]
fn instans_fine_to_coarse_assignment_rejects_without_conversio() {
    let session = test_session_la(Target::HirRust);
    crate::test_support::assert_compile_fails_with_issue(
        &session,
        "test.fab",
        "functio borrow(instans<ns> fine) → vacuum {\n  sit instans coarse ← fine\n}",
        "expression_type_mismatch",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn scalar_text_order_emits_str_compare() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit { si 'a' ≤ 'z' { nota \"ok\" } }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("AsRef::<str>::as_ref("),
        "expected str text compare emit, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn whole_string_text_order_emits_str_compare() {
    // D1.6: UTF-8 byte order is code-point order, so `&str` compare is exact.
    let session = test_session_la(Target::HirRust);
    let source = "functio prior(textus a, textus b) → bivalens { redde a ≺ b }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("AsRef::<str>::as_ref(") && !code.contains("unicode_scalar_value"),
        "expected str text compare emit, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn instans_cross_precision_compare_emits_partial_cmp_at_coarser() {
    let session = test_session_la(Target::HirRust);
    let source = "functio before(instans<ns> fine, instans<ms> coarse) → bivalens { redde fine ≺ coarse }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".partial_cmp_at_coarser("),
        "expected coarser-precision instans compare, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn instans_cross_precision_approx_eq_emits_partial_cmp_at_coarser() {
    let session = test_session_la(Target::HirRust);
    let source = "functio same(instans<ns> fine, instans<ms> coarse) → bivalens { redde fine ≅ coarse }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".partial_cmp_at_coarser(") && code.contains("std::cmp::Ordering::Equal"),
        "expected coarser-precision instans approx-eq compare, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn instans_cross_precision_approx_not_eq_emits_partial_cmp_at_coarser() {
    let session = test_session_la(Target::HirRust);
    let source = "functio differ(instans<ns> fine, instans<ms> coarse) → bivalens { redde fine ≇ coarse }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".partial_cmp_at_coarser(") && code.contains("!= Some(std::cmp::Ordering::Equal)"),
        "expected coarser-precision instans approx-not-eq compare, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn instans_to_textus_conversio_emits_to_rfc3339() {
    let session = test_session_la(Target::HirRust);
    let source = "functio wire(instans t) → textus { redde t ↦ textus }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".to_rfc3339()"),
        "expected precision-honoring instans wire emit, got:\n{code}"
    );
}

// ── collection conversio ──

#[cfg(feature = "hir-rust")]
#[test]
fn lista_to_copia_conversio_emits_hashset_collect() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit { sit _ s ← [1, 2, 2] ↦ copia<numerus> }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".into_iter().collect::<HashSet<_>>()"),
        "expected HashSet collect, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn copia_to_lista_conversio_emits_vec_collect() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  varia copia<numerus> s ← vacua\n  s.adde(1)\n  sit _ xs ← s ↦ lista<numerus>\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".into_iter().collect::<Vec<_>>()"),
        "expected Vec collect, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn named_lista_to_copia_conversio_clones_source_before_consuming() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit lista<numerus> xs ← [1, 2, 2]\n  sit _ s ← xs ↦ copia<numerus>\n  sit _ n ← xs.longitudo()\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("xs.clone().into_iter().collect::<HashSet<_>>()"),
        "a named lista source must be read, not moved, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn named_copia_to_lista_conversio_clones_source_before_consuming() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  varia copia<numerus> s ← vacua\n  s.adde(1)\n  sit _ xs ← s ↦ lista<numerus>\n  sit _ ys ← s ↦ lista<numerus>\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert_eq!(
        code.matches("s.clone().into_iter().collect::<Vec<_>>()")
            .count(),
        2,
        "a named copia source must be read, not moved, got:\n{code}"
    );
}

#[cfg(feature = "hir-swift")]
#[test]
fn lista_copia_conversio_emits_swift_set_and_array_initializers() {
    let session = test_session_la(Target::HirSwift);
    let source = "incipit {\n  sit lista<numerus> xs ← [1, 2, 2]\n  sit copia<numerus> s ← xs ↦ copia<numerus>\n  sit lista<numerus> ys ← s ↦ lista<numerus>\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(crate::Output::Swift(output)) = result.output else {
        panic!("expected Swift output");
    };
    let code = output.code;
    assert!(code.contains("= Set(l_xs)"), "expected Set(l_xs), got:\n{code}");
    assert!(code.contains("= Array(l_s)"), "expected Array(l_s), got:\n{code}");
    assert!(!code.contains("as!"), "no trapping cast, got:\n{code}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn rvalue_lista_to_copia_conversio_moves_without_clone() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit { sit _ s ← [1, 2, 2] ↦ copia<numerus> }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        !code.contains(".clone().into_iter()"),
        "an rvalue source needs no duplicate, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn tensor_to_lista_conversio_emits_planata() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit tensor<numerus, []> t ← vacua\n  sit _ xs ← t ↦ lista<numerus>\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(code.contains(".planata()"), "expected planata flatten, got:\n{code}");
}

fn assert_lista_to_tensor_conversio_rust_emit(source: &str, must_contain: &[&str], must_not_contain: &[&str]) {
    let session = test_session_la(Target::HirRust);
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    for needle in must_contain {
        assert!(code.contains(needle), "expected `{needle}` in:\n{code}");
    }
    for needle in must_not_contain {
        assert!(!code.contains(needle), "did not expect `{needle}` in:\n{code}");
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn lista_to_tensor_conversio_emits_structa() {
    assert_lista_to_tensor_conversio_rust_emit(
        "incipit { sit _ t ← [1, 2, 3] ↦ tensor<numerus, [3]> }",
        &["::structa(", "&[3]"],
        &[".len() as i64"],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn shaped_lista_to_tensor_conversio_emits_literal_shape() {
    assert_lista_to_tensor_conversio_rust_emit(
        "incipit { sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]\n  sit tf32[2, 2] t ← flat ↦ tf32[2, 2] }",
        &["::structa(", "&[2, 2]"],
        &[".len() as i64"],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rectangular_lista_literal_to_tensor_conversio_emits_flat_structa() {
    assert_lista_to_tensor_conversio_rust_emit(
        "incipit { sit tf32[2, 2] t ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[2, 2] }",
        &["::structa(vec![1.0, 2.0, 3.0, 4.0], &[2, 2])"],
        &[],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn tabula_to_lista_keys_conversio_emits_keys_cloned() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit tabula<textus, numerus> m ← { "a": 1, "b": 2 }
  sit _ ks ← m ↦ lista<textus>
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".keys().cloned().collect::<Vec<_>>()"),
        "expected keys projection, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn tabula_to_lista_values_conversio_emits_values_cloned() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit tabula<textus, numerus> m ← { "a": 1, "b": 2 }
  sit _ vs ← m ↦ lista<numerus>
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".values().cloned().collect::<Vec<_>>()"),
        "expected values projection, got:\n{code}"
    );
}

// ── tensor element width lattice ──

#[cfg(feature = "hir-rust")]
#[test]
fn tensor_numerus_to_fractus_conversio_emits_convert_elements() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit tensor<numerus, []> a ← vacua\n  sit _ b ← a ↦ tensor<fractus, []>\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".convert_elements(|v: i64| v as f64)"),
        "expected element-wise tensor widen, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn tensor_fractus_to_numerus_conversio_emits_convert_elements() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit tensor<fractus, []> b ← vacua\n  sit _ a ← b ↦ tensor<numerus, []>\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".convert_elements(|v: f64| v as i64)"),
        "expected element-wise tensor narrow, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn tensor_numerus_i32_to_i64_conversio_emits_convert_elements() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit tensor<i32, []> a ← vacua\n  sit _ b ← a ↦ tensor<i64, []>\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains(".convert_elements(|v: i32| v as i64)"),
        "expected sized element-wise tensor widen, got:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn tensor_element_width_lattice_codegen_matrix() {
    let session = test_session_la(Target::HirRust);
    for from in crate::semantic::tensor_element_lattice::SPECS {
        for to in crate::semantic::tensor_element_lattice::SPECS {
            if !crate::semantic::tensor_element_lattice::needs_convert_elements(
                &mut crate::semantic::TypeTable::new(),
                from,
                to,
            ) {
                continue;
            }
            let case = format!("{}_to_{}", from.label, to.label);
            let source = crate::semantic::tensor_element_lattice::conversio_source(from, to);
            let result = compile(&session, "test.fab", &source, ModuleCompileRequest::default());
            assert!(result.success(), "{case}: {:?}", result.diagnostics);
            let code = super::rust_output(result).code;
            let widen = format!(".convert_elements(|v: {}| v as {})", from.rust, to.rust);
            let store = format!(
                ".convert_elements(|v: {}| faber::exact::store::<{}>(i128::from(v), ",
                from.rust, to.rust
            );
            assert!(
                code.contains(&widen) || code.contains(&store),
                "{case}: expected {widen:?} or {store:?} in:\n{code}"
            );
        }
    }
}

// ── static ascription ──

#[cfg(feature = "hir-rust")]
#[test]
fn verte_static_ascription_emits_as_cast() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit f32 n ← 1.5
  sit _ f ← n ∷ fractus
  nota f
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = super::rust_output(result);
    assert!(output.code.contains(" as f64"));
}

// ── roundtrip through faber codegen ──

#[test]
#[cfg(feature = "hir-faber")]
fn conversio_glyph_form_roundtrips_through_faber_codegen() {
    let source = r#"incipit {
  sit _ n ← "22" ↦ numerus
  nota n
}"#;
    let result = super::faber_emit("test.fab", source);

    assert!(result.success());
    let Some(output) = result.output else {
        panic!("expected faber output");
    };
    assert!(output.code.contains("↦ numerus"));
}

#[test]
#[cfg(feature = "hir-faber")]
fn conversio_with_recovery_roundtrips_through_faber_codegen() {
    let source = r#"incipit {
  sit _ n ← "bad" ↦ numerus ⊥ 0
  nota n
}"#;
    let result = super::faber_emit("test.fab", source);

    assert!(result.success());
    let Some(output) = result.output else {
        panic!("expected faber output");
    };
    assert!(output.code.contains("↦ numerus ⊥ 0"));
}
