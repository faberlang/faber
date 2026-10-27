//! Template/scriptum, block strings, ASCII literals, octeti literals, valor
//! assignment policy, and related literal-emission tests.

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::DiagnosticArg;
use crate::driver::ModuleCompileRequest;
use crate::test_support::{assert_compile_fails_with_args, assert_compile_fails_with_issue, test_session_la};

// ── template / forma / scriptum ──

#[cfg(feature = "hir-rust")]
#[test]
fn backtick_template_captures_forma_without_rendering() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit textus accountId ← "acct-1"
  sit textus plan ← "pro"
  sit _ q ← `select * from accounts where id = § and plan = §`(accountId, plan)
  nota q.template
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = super::rust_output(result);
    assert!(!output.code.contains("format!"), "forma capture must not render via scriptum");
    assert!(output.code.contains("struct forma"));
    assert!(output.code.contains("template:"));
    assert!(output.code.contains("params:"));
    assert!(output.code.contains("faber::Valor::from"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn bare_backtick_template_produces_empty_params() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  sit _ q ← `select 1`
  nota q.template
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = super::rust_output(result);
    assert!(output.code.contains("vec![]") || output.code.contains("Vec::new()"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn forma_reused_textus_param_is_borrowed_not_moved() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit textus t ← "x"
  sit _ first ← `§`(t)
  sit _ second ← `§`(t)
  nota first.template
  nota second.template
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = super::rust_output(result);
    assert!(
        output.code.contains(".as_str()"),
        "textus param must be borrowed to avoid a move"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn quoted_template_still_renders_via_scriptum() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit textus name ← "Mundus"
  sit textus message ← "Hello, §!"(name)
  nota message
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("format!"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_lowers_scriptum_expression() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit _ name ← "Marcus"
  sit _ msg ← scriptum("salve, §!", name)
  nota msg
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
}

// ── block strings (guillemets) ──

#[cfg(feature = "hir-rust")]
#[test]
fn block_string_literal_uses_guillemets() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit _ quote ← «he said \"salve\"»\n  nota quote\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("he said \\\"salve\\\""));
}

#[test]
fn block_string_opener_glyphs_are_rejected() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit _ quote ← ❝he said \"salve\"❞\n  nota quote\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    // `❝` is an unexpected character, not a block-string token.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| { d.is_error() && d.code == Some("LEX004") && d.issue() == Some("unexpected_character") })
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn guillemet_block_string_template_renders_via_scriptum() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit _ code ← 200\n  sit _ label ← \"ok\"\n  sit _ msg ← «status: § (§)»(code, label)\n  nota msg\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("format!"));
}

// ── ASCII literals ──

#[ignore = "slow: spawns external toolchain smoke (cargo/go/tsc); run with --ignored"]
#[test]
fn ascii_literal_compiles_to_faber_ascii() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit _ route ← 'pg:query'\n  nota route\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("faber::Ascii::new(\"pg:query\")"));
    crate::backend_smoke::assert_hir_emitted_smoke(Target::HirRust, &output.code, "ascii-literal");
}

#[test]
fn ascii_literal_rejects_non_ascii_content() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_args(
        &session,
        "test.fab",
        "incipit {\n  sit _ bad ← 'café'\n  nota bad\n}",
        &[
            DiagnosticArg::new("issue", "ascii_literal_non_ascii_scalar"),
            DiagnosticArg::new("character", "é"),
        ],
    );
}

#[test]
fn ascii_literal_rejects_template_application() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_args(
        &session,
        "test.fab",
        "incipit {\n  sit _ id ← 1\n  sit _ bad ← 'route'(id)\n  nota bad\n}",
        &[DiagnosticArg::new("issue", "ascii_literal_template_application")],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn ascii_widens_to_textus() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit _ route ← 'pg:query'\n  sit textus shown ← route\n  nota shown\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".to_textus()"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn textus_to_ascii_conversio_emits_try_from_textus() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit _ raw ← \"ok\"\n  sit ascii token ← raw ↦ ascii\n  nota token\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("faber::Ascii::try_from_textus"));
}

// ── octeti literal ──

#[cfg(feature = "hir-rust")]
#[test]
fn octeti_literal_compiles_to_vec_u8() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  sit lista<b8> sig ← |de ad be ef|\n  sit lista<b8> empty_octeti ← ||\n  nota sig\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("vec![0xde, 0xad, 0xbe, 0xef]"));
    assert!(output.code.contains("vec![]"));
}

#[test]
fn octeti_literal_rejects_odd_hex_digit_count() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_args(
        &session,
        "test.fab",
        "incipit {\n  sit lista<b8> bad ← |abc|\n  nota bad\n}",
        &[DiagnosticArg::new("issue", "octeti_literal_odd_digit_count")],
    );
}

#[test]
fn octeti_literal_rejects_invalid_hex_character() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_args(
        &session,
        "test.fab",
        "incipit {\n  sit lista<b8> bad ← |de ad g0|\n  nota bad\n}",
        &[
            DiagnosticArg::new("issue", "octeti_literal_invalid_character"),
            DiagnosticArg::new("character", "g"),
        ],
    );
}

#[test]
fn octeti_literal_rejects_template_application() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_args(
        &session,
        "test.fab",
        "incipit {\n  sit _ id ← 1\n  sit _ bad ← |00|(id)\n  nota bad\n}",
        &[DiagnosticArg::new("issue", "octeti_literal_template_application")],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn octeti_literal_matches_in_elige_pattern() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  sit _ bytes ← |deadbeef|
  elige bytes {
    casu |deadbeef| ergo nota "hit"
    casu |00| ergo nota "miss"
    ceterum ergo nota "other"
  }
}"#;
    let result = compile(&session, "pattern.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "expected octeti pattern match, got: {:?}", result.diagnostics);
}

// ── byte list with a `u8` return ──
//
// The element of `lista<b8>` is the raw-bits `b8`; a number is spelled `u8`,
// and the two convert explicitly (`↦ u8`).

#[cfg(feature = "hir-rust")]
#[test]
fn byte_list_param_and_u8_return_lower_to_vec_u8() {
    let session = test_session_la(Target::HirRust);
    // `accipe` is the optional read (`b8 ∪ nihil`), and an optional into a plain
    // cell is SEM010 optional_into_plain_cell (sb300 1d034eee1e, need),
    // so each read is coalesced with a b8 default.
    let source = "functio firstByte(lista<b8> buf) → u8 {\n    sit b8 b ← buf.accipe(0) vel (0 ∷ b8)\n    redde b ↦ u8\n}\n\nincipit {\n    varia lista<b8> sig ← |de ad be ef|\n    sit u8 a ← firstByte(sig)\n    sit b8 raw ← sig.accipe(1) vel (0 ∷ b8)\n    sig.appende(raw)\n    nota a\n}\n";
    let result = compile(&session, "octet_builtin.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "expected success, got: {:?}", result.diagnostics);
    let code = super::rust_output(result).code;
    assert!(
        code.contains("fn firstByte(p_buf: Vec<u8>) -> u8"),
        "expected the u8 return and the byte-list param to lower to u8, got:\n{code}"
    );
    assert!(
        code.contains("p_buf.get(0"),
        "expected accipe to lower to p_buf.get, got:\n{code}"
    );
    assert!(
        code.contains("l_sig.push(l_raw)"),
        "expected appende to lower to push, got:\n{code}"
    );
}

// ── valor unification ──

#[test]
fn bare_assignment_rejects_valor_to_concrete_targets() {
    let session = test_session_la(Target::HirRust);
    for (name, source) in [
        ("valor-to-numerus.fab", "incipit {\n  sit numerus n ← { \"a\": 1 }\n}"),
        ("valor-to-textus.fab", "incipit {\n  sit textus s ← { \"a\": 1 }\n}"),
        ("valor-to-bivalens.fab", "incipit {\n  sit bivalens b ← { \"a\": 1 }\n}"),
        ("valor-to-fractus.fab", "incipit {\n  sit fractus f ← { \"a\": 1 }\n}"),
        ("valor-to-lista.fab", "incipit {\n  sit lista<numerus> xs ← { \"a\": 1 }\n}"),
    ] {
        assert_compile_fails_with_issue(&session, name, source, "expression_type_mismatch");
    }
}

#[test]
fn bare_assignment_rejects_scalar_narrowing() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "fractus-to-numerus.fab",
        "incipit {\n  sit numerus n ← 1.5\n}",
        "expression_type_mismatch",
    );
    assert_compile_fails_with_issue(
        &session,
        "textus-to-ascii.fab",
        "incipit {\n  sit ascii a ← \"uber\"\n}",
        "expression_type_mismatch",
    );
}

#[test]
fn bare_assignment_rejects_json_tabula_shape_mismatches() {
    let session = test_session_la(Target::HirRust);
    for (name, source, issue) in [
        (
            "f8-null-to-numerus.fab",
            "incipit {\n  sit tabula<textus, numerus> x ← { \"a\": null }\n}",
            "json_value_type_mismatch",
        ),
        (
            "f9-textus-to-numerus.fab",
            "incipit {\n  sit tabula<textus, numerus> x ← { \"a\": \"95\" }\n}",
            "json_value_type_mismatch",
        ),
        (
            "f10-array-to-numerus.fab",
            "incipit {\n  sit tabula<textus, numerus> x ← { \"a\": [1, 2] }\n}",
            "json_nested_value_requires_valor",
        ),
        (
            "f11-wrong-key-type.fab",
            "incipit {\n  sit tabula<numerus, numerus> x ← { \"a\": 1 }\n}",
            "json_map_key_must_be_textus",
        ),
    ] {
        assert_compile_fails_with_issue(&session, name, source, issue);
    }
}

#[test]
fn explicit_assignment_rejects_numeric_narrowing() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "explicit-fractus-to-numerus.fab",
        "incipit {\n  varia numerus n ← 1\n  n ← n + 1.5\n}",
        "assignment_type_mismatch",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn bare_assignment_keeps_implicit_widening() {
    let session = test_session_la(Target::HirRust);
    for (name, source) in [
        ("numerus-to-valor.fab", "incipit {\n  sit valor v ← 5\n}"),
        ("textus-to-valor.fab", "incipit {\n  sit valor v ← \"str\"\n}"),
        ("bivalens-to-valor.fab", "incipit {\n  sit valor v ← verum\n}"),
        ("lista-to-valor.fab", "incipit {\n  sit valor v ← [1, 2, 3]\n}"),
        ("json-to-valor.fab", "incipit {\n  sit valor v ← { \"a\": 1 } ↦ valor\n}"),
        ("numerus-to-fractus.fab", "incipit {\n  sit fractus f ← 1\n}"),
        ("ascii-to-textus.fab", "incipit {\n  sit textus s ← 'ascii'\n}"),
    ] {
        let result = compile(&session, name, source, ModuleCompileRequest::default());
        assert!(
            result.success(),
            "expected widening success for {name}, got {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn bare_json_literal_does_not_implicitly_assign_to_valor() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "json-direct-valor.fab",
        "incipit {\n  sit valor v ← { \"a\": 1 }\n}",
        "initializer_annotation_mismatch",
    );
}
