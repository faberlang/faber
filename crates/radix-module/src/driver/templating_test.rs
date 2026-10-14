//! Template/scriptum, block strings, ASCII literals, octeti literals, valor
//! assignment policy, and related literal-emission tests.

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::DiagnosticArg;
use crate::driver::ModuleCompileRequest;
use crate::test_support::{assert_compile_fails_with_args, assert_compile_fails_with_issue, test_session_la};

// ── template / forma / scriptum ──

#[test]
fn backtick_template_captures_forma_without_rendering() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  fixum textus accountId ← "acct-1"
  fixum textus plan ← "pro"
  fixum _ q ← `select * from accounts where id = § and plan = §`(accountId, plan)
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

#[test]
fn bare_backtick_template_produces_empty_params() {
    let session = test_session_la(Target::HirRust);
    let source = r"incipit {
  fixum _ q ← `select 1`
  nota q.template
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = super::rust_output(result);
    assert!(output.code.contains("vec![]") || output.code.contains("Vec::new()"));
}

#[test]
fn forma_reused_textus_param_is_borrowed_not_moved() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  fixum textus t ← "x"
  fixum _ first ← `§`(t)
  fixum _ second ← `§`(t)
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

#[test]
fn quoted_template_still_renders_via_scriptum() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  fixum textus name ← "Mundus"
  fixum textus message ← "Hello, §!"(name)
  nota message
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("format!"));
}

#[test]
fn compile_lowers_scriptum_expression() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  fixum _ name ← "Marcus"
  fixum _ msg ← scriptum("salve, §!", name)
  nota msg
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
}

// ── block strings (guillemets) ──

#[test]
fn block_string_literal_uses_guillemets() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  fixum _ quote ← «he said \"salve\"»\n  nota quote\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("he said \\\"salve\\\""));
}

#[test]
fn block_string_opener_glyphs_are_rejected() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  fixum _ quote ← ❝he said \"salve\"❞\n  nota quote\n}";
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

#[test]
fn guillemet_block_string_template_renders_via_scriptum() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  fixum _ code ← 200\n  fixum _ label ← \"ok\"\n  fixum _ msg ← «status: § (§)»(code, label)\n  nota msg\n}";
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
    let source = "incipit {\n  fixum _ route ← 'pg:query'\n  nota route\n}";
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
        "incipit {\n  fixum _ bad ← 'café'\n  nota bad\n}",
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
        "incipit {\n  fixum _ id ← 1\n  fixum _ bad ← 'route'(id)\n  nota bad\n}",
        &[DiagnosticArg::new("issue", "ascii_literal_template_application")],
    );
}

#[test]
fn ascii_widens_to_textus() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  fixum _ route ← 'pg:query'\n  fixum textus shown ← route\n  nota shown\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".to_textus()"));
}

#[test]
fn textus_to_ascii_conversio_emits_try_from_textus() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  fixum _ raw ← \"ok\"\n  fixum ascii token ← raw ↦ ascii\n  nota token\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("faber::Ascii::try_from_textus"));
}

// ── octeti literal ──

#[test]
fn octeti_literal_compiles_to_vec_u8() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit {\n  fixum octeti sig ← |de ad be ef|\n  fixum octeti empty_octeti ← ||\n  nota sig\n}";
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
        "incipit {\n  fixum octeti bad ← |abc|\n  nota bad\n}",
        &[DiagnosticArg::new("issue", "octeti_literal_odd_digit_count")],
    );
}

#[test]
fn octeti_literal_rejects_invalid_hex_character() {
    let session = test_session_la(Target::HirRust);
    assert_compile_fails_with_args(
        &session,
        "test.fab",
        "incipit {\n  fixum octeti bad ← |de ad g0|\n  nota bad\n}",
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
        "incipit {\n  fixum _ id ← 1\n  fixum _ bad ← |00|(id)\n  nota bad\n}",
        &[DiagnosticArg::new("issue", "octeti_literal_template_application")],
    );
}

#[test]
fn octeti_literal_matches_in_elige_pattern() {
    let session = test_session_la(Target::HirRust);
    let source = r#"incipit {
  fixum _ bytes ← |deadbeef|
  elige bytes {
    casu |deadbeef| ergo nota "hit"
    casu |00| ergo nota "miss"
    ceterum ergo nota "other"
  }
}"#;
    let result = compile(&session, "pattern.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "expected octeti pattern match, got: {:?}", result.diagnostics);
}

// ── octetus builtin (retired: need, ruling B8) ──
//
// `octetus` (en `byte`) was `numerus<u8>`. The word is retired with the old
// `octeti` primitive: the element of `octeti` is the raw-bits `b8`, a number
// is spelled `u8`, and the two convert explicitly (`↦ u8`). The test keeps its
// id; it now proves the u8 lowering of the number and the retirement of the
// word.

#[test]
fn octetus_builtin_resolves_to_numerus_u8_and_lowers_to_u8() {
    let session = test_session_la(Target::HirRust);
    // `accipe` is the optional read (`b8 ∪ nihil`), and an optional into a plain
    // cell is SEM010 optional_into_plain_cell (sb300 1d034eee1e, need),
    // so each read is coalesced with a b8 default.
    let source = "functio firstByte(octeti buf) → u8 {\n    fixum b8 b ← buf.accipe(0) vel (0 ∷ b8)\n    redde b ↦ u8\n}\n\nincipit {\n    varia octeti sig ← |de ad be ef|\n    fixum u8 a ← firstByte(sig)\n    fixum b8 raw ← sig.accipe(1) vel (0 ∷ b8)\n    sig.appende(raw)\n    nota a\n}\n";
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
    assert_compile_fails_with_issue(
        &session,
        "octetus_retired.fab",
        "functio firstByte(octeti buf) → octetus {\n    redde 0\n}\n",
        "retired_byte_word",
    );
}

#[test]
fn octetus_builtin_rejects_width_parameters() {
    let session = test_session_la(Target::HirRust);
    let source = "incipit { fixum octetus<i32> x ← 1 }";
    let result = compile(&session, "octet_params.fab", source, ModuleCompileRequest::default());
    assert!(!result.success(), "expected octetus<i32> to be rejected, got success");
}

// ── valor unification ──

#[test]
fn bare_assignment_rejects_valor_to_concrete_targets() {
    let session = test_session_la(Target::HirRust);
    for (name, source) in [
        ("valor-to-numerus.fab", "incipit {\n  fixum numerus n ← { \"a\": 1 }\n}"),
        ("valor-to-textus.fab", "incipit {\n  fixum textus s ← { \"a\": 1 }\n}"),
        ("valor-to-bivalens.fab", "incipit {\n  fixum bivalens b ← { \"a\": 1 }\n}"),
        ("valor-to-fractus.fab", "incipit {\n  fixum fractus f ← { \"a\": 1 }\n}"),
        ("valor-to-lista.fab", "incipit {\n  fixum lista<numerus> xs ← { \"a\": 1 }\n}"),
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
        "incipit {\n  fixum numerus n ← 1.5\n}",
        "expression_type_mismatch",
    );
    assert_compile_fails_with_issue(
        &session,
        "textus-to-ascii.fab",
        "incipit {\n  fixum ascii a ← \"uber\"\n}",
        "expression_type_mismatch",
    );
}

#[test]
fn bare_assignment_rejects_json_tabula_shape_mismatches() {
    let session = test_session_la(Target::HirRust);
    for (name, source, issue) in [
        (
            "f8-null-to-numerus.fab",
            "incipit {\n  fixum tabula<textus, numerus> x ← { \"a\": null }\n}",
            "json_value_type_mismatch",
        ),
        (
            "f9-textus-to-numerus.fab",
            "incipit {\n  fixum tabula<textus, numerus> x ← { \"a\": \"95\" }\n}",
            "json_value_type_mismatch",
        ),
        (
            "f10-array-to-numerus.fab",
            "incipit {\n  fixum tabula<textus, numerus> x ← { \"a\": [1, 2] }\n}",
            "json_nested_value_requires_valor",
        ),
        (
            "f11-wrong-key-type.fab",
            "incipit {\n  fixum tabula<numerus, numerus> x ← { \"a\": 1 }\n}",
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

#[test]
fn bare_assignment_keeps_implicit_widening() {
    let session = test_session_la(Target::HirRust);
    for (name, source) in [
        ("numerus-to-valor.fab", "incipit {\n  fixum valor v ← 5\n}"),
        ("textus-to-valor.fab", "incipit {\n  fixum valor v ← \"str\"\n}"),
        ("bivalens-to-valor.fab", "incipit {\n  fixum valor v ← verum\n}"),
        ("lista-to-valor.fab", "incipit {\n  fixum valor v ← [1, 2, 3]\n}"),
        ("json-to-valor.fab", "incipit {\n  fixum valor v ← { \"a\": 1 } ↦ valor\n}"),
        ("numerus-to-fractus.fab", "incipit {\n  fixum fractus f ← 1\n}"),
        ("ascii-to-textus.fab", "incipit {\n  fixum textus s ← 'ascii'\n}"),
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
        "incipit {\n  fixum valor v ← { \"a\": 1 }\n}",
        "initializer_annotation_mismatch",
    );
}
