//! JSON valor literal semantic validation and tabula-ascription tests.
//! See docs/factory/inline-json-valor/contract.md.

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::DiagnosticArg;
use crate::driver::ModuleCompileRequest;
use crate::test_support::{assert_compile_fails_with_args, assert_compile_fails_with_issue, test_session_la};

// ── JSON valor literal typecheck ──

#[cfg(feature = "hir-rust")]
#[test]
fn json_valor_literal_typechecks_as_valor_without_errors() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
    sit spec ← {
        "name": "openapi",
        "version": 3,
        "ratio": 1.5,
        "stable": true,
        "legacy": false,
        "gap": null,
        "tags": ["a", "b"],
        "nested": { "deep": { "value": 42 } }
    }
    nota spec
}"#;
    let result = compile(&session, "json.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "expected clean typecheck, got: {:?}", result.diagnostics);
}

#[test]
fn json_valor_literal_empty_object_without_cell_is_named_error() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "empty.fab",
        "incipit {\n    sit e ← {}\n    nota e\n}",
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn json_valor_literal_rejects_duplicate_keys_at_top_level() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_args(
        &session,
        "dup.fab",
        "incipit {\n    sit d ← { \"a\": 1, \"b\": 2, \"a\": 3 }\n    nota d\n}",
        &[
            DiagnosticArg::new("issue", "json_duplicate_key"),
            DiagnosticArg::new("key", "a"),
        ],
    );
}

#[test]
fn json_valor_literal_rejects_duplicate_keys_in_nested_object() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_args(
        &session,
        "dup2.fab",
        "incipit {\n    sit d ← { \"outer\": { \"x\": 1, \"x\": 2 } }\n    nota d\n}",
        &[
            DiagnosticArg::new("issue", "json_duplicate_key"),
            DiagnosticArg::new("key", "x"),
        ],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn json_valor_literal_distinct_keys_in_different_objects_are_not_duplicates() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n    sit d ← { \"a\": { \"x\": 1 }, \"b\": { \"x\": 2 } }\n    nota d\n}";
    let result = compile(&session, "siblings.fab", source, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "sibling-object keys must not be flagged duplicate: {:?}",
        result.diagnostics
    );
}

#[test]
#[cfg(feature = "hir-faber")]
fn json_valor_literal_round_trips_through_faber_codegen() {
    let source = "incipit {\n    sit d ← { \"a\": 1, \"b\": true }\n    nota d\n}";
    let first = super::faber_emit("rt1.fab", source);
    assert!(first.success(), "first compile failed: {:?}", first.diagnostics);
    let Some(out) = first.output else {
        panic!("expected faber output")
    };
    assert!(out.code.contains("{ \"a\": 1"), "expected JSON round-trip, got:\n{}", out.code);
    assert!(out.code.contains("true"), "expected JSON true wire token, got:\n{}", out.code);
}

// ── JSON valor → tabula<K,V> ascribe ──

#[cfg(feature = "hir-rust")]
#[test]
fn json_valor_ascribed_to_tabula_emits_real_hashmap_constant() {
    let session = test_session_la(Target::HirRust);
    let source =
        "incipit {\n    sit tabula<textus, numerus> puncta ← { \"aelia\": 95, \"balbus\": 87 }\n    nota puncta\n}";
    let result = compile(&session, "tabula.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "expected success, got: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("HashMap::<String, i64>::new()"),
        "expected real HashMap, got:\n{code}"
    );
    assert!(
        code.contains("__m.insert(\"aelia\".to_string(), 95i64)"),
        "expected typed insert, got:\n{code}"
    );
    assert!(
        !code.contains("faber::Valor::Tabula"),
        "must not emit faber::Valor wrapper for ascribed tabula"
    );
}

#[test]
fn json_valor_ascribed_to_tabula_rejects_value_type_mismatch() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "mismatch.fab",
        "incipit {\n    sit tabula<textus, textus> bad ← { \"a\": 1 }\n    nota bad\n}",
        "json_value_type_mismatch",
    );
}

#[test]
fn json_valor_ascribed_to_tabula_rejects_null_for_non_valor_value_type() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "null-mismatch.fab",
        "incipit {\n    sit tabula<textus, numerus> bad ← { \"a\": null }\n    nota bad\n}",
        "json_value_type_mismatch",
    );
}

#[test]
fn json_valor_ascribed_to_tabula_rejects_non_textus_key_type() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "key-mismatch.fab",
        "incipit {\n    sit tabula<numerus, numerus> bad ← { \"a\": 1 }\n    nota bad\n}",
        "json_map_key_must_be_textus",
    );
}

#[test]
fn json_valor_ascribed_to_tabula_rejects_nested_value_when_v_is_not_valor() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "nested.fab",
        "incipit {\n    sit tabula<textus, numerus> bad ← { \"a\": { \"x\": 1 } }\n    nota bad\n}",
        "json_nested_value_requires_valor",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn json_valor_ascribed_to_tabula_valor_values_lower_to_fabervalue() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n    sit tabula<textus, valor> nested ← { \"a\": { \"x\": 1 } }\n    nota nested\n}";
    let result = compile(&session, "nested_valor.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "expected success, got: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("HashMap::<String, faber::Valor>::new()") || code.contains("HashMap::<String, valor>::new()"),
        "expected valor-typed map, got:\n{code}"
    );
    assert!(
        code.contains("faber::Valor::Tabula"),
        "expected nested faber::Valor::Tabula value"
    );
}
