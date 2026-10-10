//! Regression-prevention tests: closures, namespace inference, structured control
//! flow, proba lowering, destructuring, struct/optional/nullable, and other
//! feature-area regression guards.

use super::compile;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, ModuleCompileRequest, Session};
use crate::test_support::test_session;
use std::path::PathBuf;

// ── faber roundtrip / annotation ──

#[test]
#[cfg(feature = "hir-faber")]
fn faber_emit_preserves_nondum_interface_method_annotation() {
    let source = r#"
implendum Drawable {
  @ nondum
  functio draw() → vacuum
}

genus Circle implet Drawable {
  functio draw() {
    nota "ok"
  }
}
"#;
    let result = super::faber_emit("nondum.fab", source);

    assert!(
        result.success(),
        "expected nondum interface method to compile with nondum annotation preserved: {:?}",
        result.diagnostics
    );
    let Some(output) = result.output else {
        panic!("expected faber output");
    };
    assert!(output.code.contains("@ nondum"), "output must preserve @ nondum annotation");
}

#[test]
#[cfg(feature = "hir-faber")]
fn annotation_strings_with_quotes_and_newlines_roundtrip_through_faber_codegen() {
    let source = r#"@ verte "a \"hello\" world"
incipit {}"#;
    let result = super::faber_emit("test.fab", source);

    assert!(result.success(), "expected compile success: {:?}", result.diagnostics);
}

#[test]
#[cfg(feature = "hir-faber")]
fn template_verte_annotation_roundtrips_through_faber_codegen() {
    let source = r#"@ verte "auto"
incipit {
  fixum _ x ← 42
  fixum _ y ← x ∷ textus
}"#;
    let result = super::faber_emit("test.fab", source);

    assert!(result.success(), "expected compile success: {:?}", result.diagnostics);
}

// ── faber roundtrip (tuple types/literals, tensor long form) ──

#[test]
#[cfg(feature = "hir-faber")]
fn tuple_return_and_literal_roundtrip_losslessly_through_faber_codegen() {
    // Before fix: the canonical emitter collapsed `iuncta<...>` return types
    // to `ignotum` and rendered tuple literals as `(a, b)` parens, which the
    // parser rejects (PARSE001). The typed construction form must survive.
    let source = r#"functio pair() → iuncta<numerus, textus> {
  redde iuncta<numerus, textus> [1, "salve"]
}"#;
    let output = super::faber_roundtrip(source);
    assert!(
        output.contains("→ iuncta<numerus, textus>"),
        "tuple return type must be preserved, got:\n{output}"
    );
    assert!(
        output.contains("redde iuncta<numerus, textus> [1, \"salve\"]"),
        "typed tuple literal must be preserved, got:\n{output}"
    );
    assert!(
        !output.contains("ignotum"),
        "tuple types must not collapse to ignotum, got:\n{output}"
    );
    assert!(
        !output.contains("(1, \"salve\")"),
        "paren tuple literal is not parseable source, got:\n{output}"
    );
}

#[test]
#[cfg(feature = "hir-faber")]
fn tensor_type_roundtrips_in_long_form_through_faber_codegen() {
    // The width sugar `tf32[2, 2]` is valid input but is not emitted
    // canonically: the long form `tensor<f32, [2, 2]>` is the documented
    // default surface and keeps canonical output re-parseable everywhere.
    let source = r"functio forward(tensor<f32, [2,2]> input) → tensor<f32, [2,2]> {
  redde input
}";
    let output = super::faber_roundtrip(source);
    assert!(
        output.contains("→ tensor<f32, [2, 2]>"),
        "tensor must round-trip in long form, got:\n{output}"
    );
    assert!(
        !output.contains("tf32"),
        "canonical emit must not use the width sugar, got:\n{output}"
    );
}

// ── faber roundtrip (fac, discerne, ad) ──

#[test]
#[cfg(feature = "hir-faber")]
fn fac_do_while_roundtrips_through_faber_codegen() {
    let source = r"incipit {
  varia _ counter ← 0
  fac {
    counter ← counter + 1
    nota counter
  } dum counter ≺ 3
}";
    let output = super::faber_roundtrip(source);
    assert!(output.contains("fac {"));
    assert!(output.contains("} dum counter ≺ 3"));
}

#[test]
#[cfg(feature = "hir-faber")]
fn discerne_alias_and_multi_subject_roundtrip_through_faber_codegen() {
    let source = r#"discretio Res {
  Click { numerus x numerus y },
  Quit
}

functio compare(Res left, Res right) → vacuum {
  discerne left et right {
    casu Click ut l et Quit ut r {
      nota l
      nota r
    }
    ceterum {
      nota "other"
    }
  }
}"#;
    let output = super::faber_roundtrip(source);
    assert!(output.contains("discerne left et right"));
    assert!(output.contains("casu Click ut l et Quit ut r"));
}

#[test]
fn statement_level_ad_does_not_roundtrip_through_faber_codegen() {
    let source = r#"incipit {
  ad "fasciculus:lege" ("hello.txt") → textus content ⇥ textus {
    nota content
  } cape err {
    nota err
  }
}"#;
    let result = compile(
        &test_session(Target::HirRust),
        "test.fab",
        source,
        ModuleCompileRequest::default(),
    );
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("PARSE020") && d.issue() == Some("ad_expression_only"))
    );
}

#[test]
fn rejects_old_verte_aliases_as_postfix_operators() {
    let session = test_session(Target::HirRust);
    for alias in &["qua", "innatum", "novum"] {
        let source = format!("incipit {{ nota 42 {alias} textus }}");
        let result = compile(&session, "test.fab", &source, ModuleCompileRequest::default());

        assert!(!result.success(), "source using old postfix alias '{alias}' must fail");
        assert!(
            result
                .diagnostics
                .iter()
                .any(crate::diagnostics::Diagnostic::is_error),
            "expected at least one error diagnostic for old alias '{alias}'"
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.is_error() && d.span.is_some_and(|s| s.start > 10 && s.start < 30)),
            "error should be reported near the alias token for '{alias}'"
        );
    }
}

// ── closure syntax (compact + legacy) ──

#[test]
#[cfg(feature = "hir-faber")]
fn faber_output_prefers_compact_closure_syntax() {
    let source = r"incipit {
  fixum _ double ← clausura numerus x: x * 2
}";
    let result = super::faber_emit("test.fab", source);

    let Some(output) = result.output else {
        panic!("expected Faber output");
    };
    assert!(output.code.contains("numerus x ∴ x * 2"));
    assert!(!output.code.contains("clausura numerus x"));
}

#[test]
fn compact_fac_closure_body_typechecks_with_redde() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ predicate ← numerus n → bivalens ∴ fac {
    redde n ≻ 1
  }
  nota predicate(2)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

#[test]
fn compact_fac_closure_rejects_redde_without_explicit_return_type() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ predicate ← numerus n ∴ fac {
    redde n ≻ 1
  }
}";
    let result = compile(&session, "closure-implicit-return.fab", source, ModuleCompileRequest::default());

    assert!(result.output.is_none());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM033") && d.issue() == Some("redde_requires_return_type"))
    );
}

#[test]
fn contextual_compact_fac_closure_rejects_redde_without_local_return_type() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum (numerus) → bivalens predicate ← numerus n ∴ fac {
    redde n ≻ 1
  }
}";
    let result = compile(
        &session,
        "closure-contextual-implicit-return.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(result.output.is_none());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM033") && d.issue() == Some("redde_requires_return_type"))
    );
}

#[test]
fn compact_fac_closure_accepts_iace_with_explicit_error_channel() {
    let session = test_session(Target::HirTypeScript);
    let source = r#"incipit {
  fixum _ failer ← numerus n ⇥ textus ∴ fac {
    iace "bad"
  }
}"#;
    let result = compile(
        &session,
        "closure-explicit-error-channel.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(result.success(), "{:?}", result.diagnostics);
}

#[test]
fn compact_fac_closure_rejects_iace_without_local_error_channel() {
    let session = test_session(Target::HirTypeScript);
    let source = r#"functio outer() ⇥ textus {
  fixum _ failer ← numerus n ∴ fac {
    iace "bad"
  }
}"#;
    let result = compile(
        &session,
        "closure-missing-error-channel.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(result.output.is_none());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM010") && d.issue() == Some("iace_requires_alternate_exit"))
    );
}

#[test]
fn compact_fac_closure_accepts_local_cape_without_error_channel() {
    let session = Session::new(Config::default().with_dev_locale_root());
    let source = r#"incipit {
  fixum _ handled ← numerus n ∴ fac {
    iace "bad"
  } cape err {
    nota err
  }
}"#;
    let result =
        crate::driver::analyze_source(&session, "closure-local-cape-error.fab", source, AnalyzeRequest::default());

    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn compact_fac_closure_cape_body_typechecks_with_redde() {
    let session = Session::new(Config::default().with_dev_locale_root());
    let source = r#"functio parseFlag(textus value) → bivalens ⇥ textus {
  si value ≡ "bad" ergo iace "bad"
  redde verum
}

incipit {
  fixum _ predicate ← textus value → bivalens ∴ fac {
    redde parseFlag(value)
  } cape err {
    redde falsum
  }
  nota predicate("bad")
}"#;
    let result = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default());

    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn legacy_block_closure_rejects_redde_without_explicit_return_type() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ predicate ← clausura numerus n {
    redde n ≻ 1
  }
}";
    let result = compile(
        &session,
        "legacy-closure-implicit-return.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(result.output.is_none());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM033") && d.issue() == Some("redde_requires_return_type"))
    );
}

#[test]
fn legacy_block_closure_body_typechecks_with_redde() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ predicate ← clausura numerus n → bivalens {
    redde n ≻ 1
  }
  nota predicate(2)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

// ── namespace / inference regressions ──

#[test]
fn namespace_function_call_in_condition_no_longer_leaves_infer_type() {
    let session = test_session(Target::HirRust);
    let source = r#"importa ex "../../norma/consolum" consolum

incipit {
  si consolum.audit() {
    nota "Interactive mode"
  }
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "cannot_infer_expression_type")
    }));
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("condition_must_be_bivalens"))
    );
}

#[test]
fn namespace_function_call_statements_no_longer_leave_infer_type() {
    let session = test_session(Target::HirRust);
    let source = r#"importa ex "../../norma/consolum" consolum

incipit {
  consolum.fundeLineam("x")
  consolum.errorLineam("y")
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "cannot_infer_expression_type")
    }));
}

#[test]
fn itera_de_array_index_no_longer_leaves_infer_types() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ xs ← [10, 20, 30]
  itera de xs fixum idx {
    nota xs[idx]
  }
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "cannot_infer_expression_type")
    }));
}

#[test]
fn itera_ab_range_no_longer_leaves_infer_types() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  itera ab 0‥5 fixum i {
    nota i
  }
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "cannot_infer_expression_type")
    }));
}

#[test]
fn itera_ab_inclusive_range_glyph_compiles() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  itera ab 0…5 fixum i {
    nota i
  }
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "cannot_infer_expression_type")
    }));
}

#[test]
fn deferred_local_assignment_can_drive_inference() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  varia value
  value = 42
  nota value
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("cannot_infer_variable_type"))
    );
}

#[test]
fn compile_supports_extended_binary_operators() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ a ← 1
  fixum _ b ← 2
  fixum numerus ∪ nihil maybe ← nulla

  nota a ≡ b
  nota a ≢ b
  nota maybe est nihil
  nota a ∈ 0‥3
  nota a ∈ [1, 2, 3]
  nota maybe vel 0
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

#[test]
fn compile_supports_extended_unary_operators() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ flag ← verum
  fixum textus ∪ nihil maybe ← nulla
  fixum _ n ← -3

  nota non flag
  nota maybe est nihil
  nota maybe non est nihil
  nota n ≺ 0
  nota n ≻ 0
  nota flag ≡ verum
  nota flag ≢ verum
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

// ── proba lowering ──

#[test]
fn compile_lowers_top_level_proba_to_rust_test_function() {
    let session = test_session(Target::HirRust);
    let source = r#"proba "one plus one equals two" {
  adfirma 1 + 1 ≡ 2
}

incipit {}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "unhandled_statement_kind")
    }));
    let output = super::rust_output(result);
    assert!(output.code.contains("#[test]"));
    assert!(output.code.contains("fn proba_"));
}

#[test]
fn compile_lowers_proba_omit_and_futurum_as_ignored_tests() {
    let session = test_session(Target::HirRust);
    let source = r#"proba "case one" omitte "blocked" {
  adfirma falsum
}

proba "case two" futurum "todo" {
  adfirma verum
}

incipit {}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "unhandled_statement_kind")
    }));
    let output = super::rust_output(result);
    assert!(
        output
            .code
            .contains("#[ignore = \"faber: omitte - blocked\"]")
    );
    assert!(
        output
            .code
            .contains("#[ignore = \"faber: futurum - todo\"]")
    );
}

#[test]
fn compile_lowers_probandum_nested_cases_without_lowering_errors() {
    let session = test_session(Target::HirRust);
    let source = r#"probandum "suite" {
  praepara omnia {
    fixum _ x ← 1
    nota x
  }

  proba "case one" {
    adfirma 1 ≡ 1
  }

  probandum "nested" {
    proba "case two" {
      adfirma verum
    }
  }
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "unhandled_statement_kind")
    }));
}

#[test]
fn compile_probandum_hooks_scope_local_to_suite_and_omnia_to_nested() {
    let session = test_session(Target::HirRust);
    let source = r#"probandum "outer" {
  praepara omnia {
    nota "HOOK_ALL"
  }

  praepara {
    nota "HOOK_LOCAL"
  }

  proba "own case" {
    nota "BODY_OWN"
  }

  probandum "inner" {
    proba "nested case" {
      nota "BODY_NESTED"
    }
  }
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "expected suite hooks to compile, got {:?}",
        result.diagnostics
    );
    let code = super::rust_output(result).code;

    let own_start = code.find("BODY_OWN").expect("own case body emitted");
    let own_case = &code[code[..own_start].rfind("fn ").expect("own case fn")..];
    let own_end = own_case.find("BODY_NESTED").unwrap_or(own_case.len());
    let own_case = &own_case[..own_end];
    let all = own_case.find("HOOK_ALL").expect("omnia hook in own case");
    let local = own_case.find("HOOK_LOCAL").expect("local hook in own case");
    let body = own_case.find("BODY_OWN").expect("body in own case");
    assert!(all < local && local < body, "hooks precede body: {own_case}");

    let nested_start = code.find("BODY_NESTED").expect("nested body emitted");
    let nested_case = &code[code[..nested_start].rfind("fn ").expect("nested case fn")..];
    assert!(
        nested_case.contains("HOOK_ALL"),
        "omnia hook reaches nested case: {nested_case}"
    );
    assert!(
        !nested_case.contains("HOOK_LOCAL"),
        "local hook must not reach nested case: {nested_case}"
    );
}

// ── struct / constructors / destructuring ──

#[test]
fn ex_destructured_object_fields_can_be_used_in_arithmetic() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ point ← Point { x = 4, y = 6 }
  ex point fixum x, y
  fixum numerus sum ← x + y
  nota sum
}

genus Point {
  varia numerus x = 0
  varia numerus y = 0
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("expression_type_mismatch"))
    );
}

#[test]
fn typed_constructor_uses_type_namespace_after_value_shadowing() {
    let session = test_session(Target::HirRust);
    let source = r#"genus persona {
  varia textus nomen
}

functio main() → vacuum {
  fixum persona persona ← persona { nomen = "Bob" }
  fixum persona alia ← persona { nomen = "Alice" }
  nota persona.nomen, alia.nomen
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "expected typed constructors to resolve through the type namespace, got {:?}",
        result.diagnostics
    );
}

#[test]
fn object_destructured_aliases_can_be_used_in_arithmetic() {
    let session = test_session(Target::HirRust);
    let source = r"functio totalis(Point point) → numerus {
  fixum { x ut left, y } ← point
  redde left + y
}

genus Point {
  varia numerus x = 0
  varia numerus y = 0
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "expected object destructuring aliases to compile, got {:?}",
        result.diagnostics
    );
}

#[test]
fn array_destructured_vars_can_be_used_in_arithmetic() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ coords ← [100, 200]
  varia [x, y] ← coords
  x ← x + 50
  y ← y + 50
  nota x, y
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("numeric_operands_required"))
    );
}

#[test]
fn method_return_values_can_participate_in_numeric_comparisons() {
    let session = test_session(Target::HirRust);
    let source = r#"functio accessArray(lista<numerus> items, numerus index) → numerus {
  si index ≺ 0 aut index ≥ items.longitudo() {
    mori "Index out of bounds"
  }
  redde items[index]
}

incipit {
  fixum _ nums ← [1, 2, 3]
  nota accessArray(nums, 1)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("numeric_operands_required"))
    );
}

#[test]
fn repeated_owned_calls_no_longer_report_use_after_move() {
    let session = test_session(Target::HirRust);
    let source = r"functio sumArray(lista<numerus> nums) → numerus {
  varia numerus total ← 0
  itera ex nums fixum n {
    total ← total + n
  }
  redde total
}

functio maxValue(lista<numerus> nums) → numerus {
  varia numerus max ← nums[0]
  itera ex nums fixum n {
    si n ≻ max {
      max ← n
    }
  }
  redde max
}

incipit {
  # 7562d0cf: annotate so the bounded literal widens explicitly.
  fixum lista<numerus> numbers ← [1, 2, 3, 4, 5]
  nota sumArray(numbers)
  nota maxValue(numbers)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("use_after_move"))
    );
}

#[test]
fn assignment_and_tempta_flow_no_longer_report_use_after_move() {
    let session = test_session(Target::HirRust);
    let source = r#"functio process(textus name) → vacuum {
  varia _ resource ← "pending"
  fac {
    resource ← name
    si name ≡ "" {
      iace "Empty name"
    }
    nota resource
  } cape err {
    nota err
  }
}

incipit {
  process("")
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("use_after_move"))
    );
}

#[test]
fn empty_array_and_spread_literals_no_longer_report_annotation_or_type_errors() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum lista<numerus> empty_list ← []
  fixum _ first ← [1, 2, 3]
  fixum _ extended ← [0, sparge first, 99]
  nota empty_list, extended
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("common_type_mismatch"))
    );

    let context_free_session = test_session(Target::HirRust);
    let context_free_source = r"incipit {
  fixum _ empty_list ← []
}";
    let context_free = compile(
        &context_free_session,
        "context-free-empty.fab",
        context_free_source,
        ModuleCompileRequest::default(),
    );

    assert!(!context_free.success(), "{:?}", context_free.diagnostics);
    assert!(context_free.diagnostics.iter().any(|diagnostic| {
        diagnostic.is_error()
            && diagnostic.code == Some("SEM014")
            && diagnostic.issue() == Some("cannot_infer_empty_list_element_type")
    }));
}

// ── typed vacua / context-free vacua ──

#[test]
fn typed_vacua_inhabits_declared_empty_collections() {
    let session = test_session(Target::HirRust);
    let source = r"functio empty_numbers() → lista<numerus> {
  fixum lista<numerus> xs ← vacua
  redde xs
}

functio empty_counts() → tabula<textus, numerus> {
  fixum tabula<textus, numerus> counts ← vacua
  redde counts
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "expected typed vacua collections to compile, got {:?}",
        result.diagnostics
    );
}

#[test]
fn context_free_vacua_requires_declared_type() {
    let session = test_session(Target::HirRust);
    let source = r"functio empty() → vacuum {
  fixum _ xs ← vacua
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.is_error()
            && diagnostic.code == Some("SEM014")
            && diagnostic.issue() == Some("vacua_requires_declared_type")
    }));
}

#[test]
fn cursor_iteration_accumulator_from_empty_array_no_longer_reports_inference_error() {
    let session = test_session(Target::HirRust);
    let source = r"@ cursor
functio rangeSync(numerus n) → numerus {
  itera ab 0‥n fixum i {
    cede i
  }
}

incipit {
  varia lista<numerus> syncResults ← vacua
  itera ex rangeSync(5) fixum num {
    syncResults.appende(num * 2)
  }
  nota syncResults
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "cannot_infer_expression_type")
    }));
    assert!(result.success(), "{:?}", result.diagnostics);
}

// ── optional chain / nullable ──

#[test]
fn optional_chain_no_longer_reports_lowering_stub() {
    let session = test_session(Target::HirRust);
    let source = r#"genus User {
  varia textus name = "Anon"
  varia lista<numerus> nums = [1, 2, 3]
}

functio id(textus x) → textus {
  redde x
}

incipit {
  fixum User ∪ nihil maybeUser ← nulla
  fixum _ a ← maybeUser?.name
  fixum _ b ← maybeUser?.nums?[0]
  fixum (textus) → textus ∪ nihil maybeFn ← id
  fixum _ c ← maybeFn?("x")
  nota a, b, c
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

#[test]
fn rust_output_emits_option_map_and_and_then_for_optional_chain() {
    let session = test_session(Target::HirRust);
    let source = r#"genus User {
  varia textus name = "Anon"
}

functio id(textus x) → textus {
  redde x
}

incipit {
  fixum User ∪ nihil maybeUser ← nulla
  fixum _ a ← maybeUser?.name
  fixum (textus) → textus ∪ nihil maybeFn ← id
  fixum _ c ← maybeFn?("x")
  nota a, c
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".as_ref().map(|o0| o0."));
    assert!(output.code.contains("o0.name.clone()"));
    assert!(output.code.contains(".and_then(|o0| Some(o0("));
}

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_output_translates_stdlib_lista_methods() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  varia _ xs ← [1, 2]
  xs.appende(3)
  fixum _ n ← xs.longitudo()
  nota n
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".push(3)"));
    assert!(output.code.contains(".len() as i64"));
    super::compile_rust_source_with_rustc(&output.code, "stdlib-lista-methods");
}

// ── struct construction validation ──

#[test]
fn struct_construction_requires_fields_and_checks_defaults() {
    let session = test_session(Target::HirRust);
    let missing = r#"genus User {
  varia textus name
  varia textus email sponte
}

incipit {
  fixum _ u ← User { email = "a@example.com" }
}"#;
    let missing_result = compile(&session, "missing.fab", missing, ModuleCompileRequest::default());
    assert!(!missing_result.success());
    assert!(
        missing_result.diagnostics.iter().any(|d| {
            d.is_error() && d.code == Some("SEM004") && d.issue() == Some("missing_required_struct_field")
        })
    );

    let bad_default = r#"genus User {
  varia numerus score = "bad"
}

incipit {
  fixum _ u ← { } ∷ User
}"#;
    let default_result = compile(&session, "default.fab", bad_default, ModuleCompileRequest::default());
    assert!(!default_result.success());
    assert!(
        default_result
            .diagnostics
            .iter()
            .any(|d| { d.is_error() && d.code == Some("SEM010") && d.issue() == Some("field_default_type_mismatch") })
    );
}

// ── optional coalesce / nullable fields ──

#[test]
fn rust_output_emits_coalesce_unwrap() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum textus ∪ nihil name ← nulla
  fixum _ display ← name vel "Anonymous"
  nota display
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".unwrap_or("));
}

#[test]
fn rust_output_wraps_sponte_fields_in_verte_struct_literals() {
    let session = test_session(Target::HirRust);
    let source = r#"genus User {
  varia textus name
  varia textus email sponte
  varia numerus score sponte
}

incipit {
  fixum _ a ← User { name = "Ada" }
  fixum _ b ← User { name = "Lin", email = "lin@example.com" }
  nota a.name
  nota b.name
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("pub email: Option<String>,"));
    assert!(output.code.contains("pub score: Option<i64>,"));
    assert!(output.code.contains("email: None,"));
    assert!(output.code.contains("score: None,"));
    assert!(
        output
            .code
            .contains("email: Some(\"lin@example.com\".to_string()),")
    );
}

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_output_applies_nullable_field_defaults_in_verte_struct_literals() {
    let session = test_session(Target::HirRust);
    let source = r#"genus User {
  varia textus name
  varia textus nickname = "Anonymous"
  varia textus ∪ nihil email = "Anonymous"
}

incipit {
  fixum _ a ← User { name = "Ada" }
  fixum _ b ← User { name = "Lin", email = nulla }
  fixum _ c ← User { name = "Ken", email = "ken@example.com" }
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("nickname: \"Anonymous\".to_string(),"));
    assert!(output.code.contains("pub email: Option<String>,"));
    assert!(
        output
            .code
            .contains("email: Some(\"Anonymous\".to_string()),")
    );
    assert!(output.code.contains("email: None,"));
    assert!(
        output
            .code
            .contains("email: Some(\"ken@example.com\".to_string()),")
    );
    assert!(!output.code.contains(".to_string().to_string()"));
    super::compile_rust_source_with_rustc(&output.code, "nullable-field-default");
}

// ── ignotum / type system ──

#[test]
fn ignotum_callee_no_longer_reports_not_callable() {
    let session = test_session(Target::HirRust);
    let source = r"functio invoke(ignotum callee) → ignotum {
  redde callee(1)
}

incipit {
  nota invoke
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("callee_not_callable"))
    );
}

#[test]
fn elige_accepts_enum_variant_case_values() {
    let session = test_session(Target::HirRust);
    let source = r#"ordo Color { rubrum, viridis, caeruleum }

incipit {
  fixum _ color ← Color.rubrum
  elige color {
    casu Color.rubrum { nota "R" }
    casu Color.viridis { nota "G" }
    casu Color.caeruleum { nota "B" }
  }
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "elige_case_value_shape")
    }));
}

#[test]
fn ordo_exemplum_no_longer_reports_elige_literal_error() {
    let session = test_session(Target::HirRust);
    // Uses relative path from crate root
    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/ordo/ordo.fab");
    let source = std::fs::read_to_string(&source_path).expect("read ordo exemplum");
    let result = compile(&session, "ordo.fab", &source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "elige_case_value_shape")
    }));
}

#[test]
fn optional_params_no_longer_require_all_arguments() {
    let session = test_session(Target::HirRust);
    let source = r#"functio greet(textus nomen, textus titulus sponte) → textus {
  si titulus est nihil {
    redde nomen
  }
  redde titulus
}

functio paginate(numerus pagina sponte vel 1, numerus per_pagina sponte vel 10) → numerus {
  redde pagina + per_pagina
}

functio analyze(textus source, de numerus depth sponte) → numerus {
  si depth est nihil {
    redde source.longitudo()
  }
  redde depth
}

incipit {
  nota greet("Marcus")
  nota greet("Marcus", "Dominus")
  nota paginate()
  nota paginate(2)
  nota paginate(2, 25)
  nota analyze("code")
  nota analyze("code", 5)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("wrong_number_of_arguments"))
    );
}

#[test]
fn nullable_union_type_forms_are_canonicalized() {
    let session = test_session(Target::HirRust);
    let source = r#"typus MaybeValue = textus ∪ numerus ∪ nihil
typus EitherValue = textus ∪ numerus

functio nullable_alias() → MaybeValue {
  redde 1
}

functio plain_alias() → EitherValue {
  redde "ok"
}

functio duplicate_nullable() → textus ∪ textus ∪ nihil {
  redde "ok"
}

functio nullable_direct() → textus ∪ nihil {
  redde nulla
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "expected canonical union forms to compile: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn plain_union_does_not_accept_nihil_without_nullable_member() {
    let session = test_session(Target::HirRust);
    let source = r"functio badplain() → textus ∪ numerus {
  redde nulla
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.code == Some("SEM010") && diag.issue() == Some("return_type_mismatch"))
    );
}

#[test]
fn degenerate_nihil_only_union_is_rejected() {
    let session = test_session(Target::HirRust);
    let source = r"functio bad() → nihil ∪ nihil {
  redde nulla
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.code == Some("SEM008") && diag.issue() == Some("union_only_nihil"))
    );
}

// ── additional regression / unused/feature-gated tests ──

#[test]
fn array_method_call_no_longer_reports_non_struct_member_error() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ numbers ← [1, 2, 3]
  fixum _ doubled ← numbers.mappata(numerus x ∴ x * 2)
  nota doubled
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

#[test]
fn interface_method_call_no_longer_reports_non_struct_member_error() {
    let session = test_session(Target::HirRust);
    let source = r#"implendum Drawable {
  functio draw() → vacuum
}

genus Circle implet Drawable {
  functio draw() {
    nota "ok"
  }
}

incipit {
  fixum _ c ← Circle {}
  c.draw()
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

#[test]
fn compact_inferred_closure_argument_uses_expected_method_signature() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ numbers ← [1, 2, 3]
  nota numbers.mappata(_ x ∴ x * 2)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

// ── features marked as incomplete (kept as #[ignore] with reason) ──

#[ignore = "PARSE001: json_quoted_key errors from config[\"name\"] syntax; feature incomplete, placeholder until index chain syntax is implemented"]
#[test]
fn object_member_and_index_chains_no_longer_report_index_errors() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum _ config ← { name = "test", value = 42 }
  nota config["name"]

  fixum _ data ← { items = ["first", "second"] }
  nota data.items[0]
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "array_index_must_be_numerus")
    }));
    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "indexing_requires_array_or_map")
    }));
}

#[ignore = "SEM014: cannot_infer_expression_type for compact closure; feature incomplete, placeholder until closure inference is implemented"]
#[test]
fn array_method_closure_argument_no_longer_reports_argument_type_mismatch() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ numbers ← [1, 2, 3]
  nota numbers.map(clausura numerus x: x * 2)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("argument_type_mismatch"))
    );
}

#[ignore = "PARSE001: syntax errors in genus definition prevent compilation; feature incomplete, placeholder until ego field assignment is implemented"]
#[test]
fn ego_field_assignment_no_longer_reports_assignment_type_mismatch() {
    let session = test_session(Target::HirRust);
    let source = r"genus Circulus {
  varia numerus diameter = 1
  functio crescere(numerus factor) → vacuum {
    ego.diameter ← ego.diameter * factor
  }
}

incipit {
  fixum _ c ← {} ∷ Circulus
  c.crescere(2)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("assignment_type_mismatch"))
    );
}

#[ignore = "PARSE001: json_quoted_key in object literal; feature incomplete, placeholder until objectum return type is implemented"]
#[test]
fn objectum_return_type_no_longer_reports_unknown_type() {
    let session = test_session(Target::HirRust);
    let source = r#"functio getResponse() → objectum {
  redde { body = "ok" }
}

incipit {
  fixum _ response ← getResponse()
  nota response
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("unknown_type"))
    );
}

// ── misc: quidlibet, conversio type params, verte_vel ──

#[test]
fn quidlibet_container_annotation_no_longer_reports_unknown_type() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum lista<quidlibet> docs ← [] ∷ lista<quidlibet>
  nota docs
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("unknown_type"))
    );
}

#[test]
fn conversio_type_params_no_longer_report_unknown_type() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum _ n ← "ff" ↦ i32 via Hex
  nota n
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("unknown_type"))
    );
}

#[test]
fn verte_vel_no_longer_reports_invalid_cast() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum _ data ← 42
  fixum _ asText ← data ∷ textus
  fixum _ parsed ← "invalid" ↦ numerus ⊥ 0
  fixum tabula<textus, numerus> cache ← { "alice": 95, "bob": 87 }
  fixum lista<textus> items ← vacua
  nota asText, parsed, cache, items
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("invalid_cast"))
    );
    assert!(result.success(), "{:?}", result.diagnostics);
}

// ── Rust output patterns ──

#[test]
fn rust_output_converts_textus_literals_to_owned_strings() {
    let session = test_session(Target::HirRust);
    let source = r#"functio pick(bivalens flag) → textus {
  si flag {
    redde "yes"
  }
  redde "no"
}

incipit {
  varia textus name ← "Marcus"
  fixum lista<textus> words ← ["a", "b"]
  fixum textus ∪ nihil maybe ← nulla
  fixum _ fallback ← maybe vel "x"
  nota pick(verum), name, words, fallback
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(
        output
            .code
            .contains("let name: String = \"Marcus\".to_string();")
    );
    assert!(output.code.contains("return \"yes\".to_string();"));
    assert!(
        output
            .code
            .contains("vec![\"a\".to_string(), \"b\".to_string()]")
    );
    assert!(output.code.contains("unwrap_or(\"x\".to_string())"));
}

#[test]
fn rust_output_wraps_optional_local_initializers_in_some() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum textus ∪ nihil actualName ← "Marcus"
  fixum textus ∪ nihil maybe ← nulla
  nota actualName, maybe
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(
        output
            .code
            .contains("let actualName: Option<String> = Some(\"Marcus\".to_string());")
    );
    assert!(output.code.contains("let maybe: Option<String> = None;"));
}

#[test]
fn rust_output_uses_or_for_option_coalesce() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum textus ∪ nihil a ← nulla
  fixum textus ∪ nihil b ← nulla
  fixum _ first ← a vel b
  nota first
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".or("));
}

#[test]
fn rust_output_uses_debug_format_for_collection_scribe() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum lista<textus> names ← ["a", "b"]
  nota names
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("println!(\"{names:?}\");"));
}

#[test]
fn rust_output_uses_scalar_display_helpers_for_scribe() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum fractus f ← 1.0
  fixum bivalens b ← verum
  fixum ascii a ← 'ok'
  nota f, b, a
  nota "§ § §"(f, b, a)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("faber::display::fractus(f)"));
    assert!(output.code.contains("faber::display::bivalens(b)"));
    assert!(
        output
            .code
            .contains("faber::display::text_payload((a).as_ref())")
    );
}

#[test]
fn rust_output_avoids_redundant_parentheses_in_common_contexts() {
    let session = test_session(Target::HirRust);
    let source = r"functio sum(numerus a, numerus b) → numerus {
  redde a + b
}

incipit {
  varia numerus x ← 1
  x ← x + 10
  dum x ≻ 0 {
    si x ≻ 5 {
      nota sum(x * 2, x + 1)
      rumpe
    }
    x ← x - 1
  }
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    // The emitted integer-helper prelude legitimately parenthesizes a range
    // before `.contains(..)` in a match guard; the guarantee is about the
    // lowered user code, so check from the first user item onward.
    let user_code = &output.code[output.code.find("fn sum").expect("user fn sum")..];
    assert!(!user_code.contains("if ("), "{user_code}");
    assert!(!user_code.contains("while ("));
    assert!(!user_code.contains("return (a + b);"));
    assert!(!user_code.contains("sum((x * 2), (x + 1))"));
}

#[test]
fn rust_output_normalizes_relative_import_paths() {
    let session = test_session(Target::HirRust);
    let source = r#"importa ex "./norma/consolum" publica consolum
importa ex "./commands/greet" publica greet

incipit {
  nota "ok"
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(
        output
            .code
            .contains("use crate::norma::consolum::consolum;")
    );
    assert!(output.code.contains("use crate::commands::greet::greet;"));
    assert!(!output.code.contains("..::"));
    assert!(!output.code.contains(".::"));
}

#[test]
fn ego_field_access_no_longer_reports_non_struct_member_error() {
    let session = test_session(Target::HirRust);
    let source = r"genus Counter {
  varia numerus count = 0
  functio inc() → numerus {
    ego.count ← ego.count + 1
    redde ego.count
  }
}

incipit {
  varia _ c ← Counter {}
  nota c.inc()
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
}

#[test]
fn typed_array_index_assignment_no_longer_reports_assignment_type_mismatch() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  varia lista<numerus> numbers ← [3, 7]
  numbers[0] ← 9
  nota numbers[0]
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("assignment_type_mismatch"))
    );
}

// ── format/scriptum emission ──

#[test]
fn rust_output_uses_format_macro_for_scriptum() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum _ value ← 7
  nota scriptum("valor: §", value)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("println!(\"valor: {value}\");"));
}

#[test]
fn rust_output_uses_zero_based_numbered_scriptum_placeholders() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum _ first ← "prima"
  fixum _ second ← "secunda"
  nota scriptum("ordo: §1, §0", first, second)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(
        output
            .code
            .contains("println!(\"ordo: {second}, {first}\");")
    );
}

#[test]
fn rust_output_uses_string_literal_format_application() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum _ value ← 7
  nota "valor: §"(value)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains("println!(\"valor: {value}\");"));
}

#[test]
fn rust_output_uses_numbered_string_literal_format_application() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum _ first ← "prima"
  fixum _ second ← "secunda"
  nota "ordo: §1, §0"(first, second)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(
        output
            .code
            .contains("println!(\"ordo: {second}, {first}\");")
    );
}

#[test]
fn rust_output_supports_unicode_textus_index_and_range_slice() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum _ ch ← "Salve, §!"[7]
  fixum _ prefix ← "hello world"[0‥5]
  fixum _ whole ← "hello world"[0 usque 10]
  fixum _ stepped ← "abcdef"[0‥6 per 2]
  nota ch, prefix, whole, stepped
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    assert!(output.code.contains(".chars().nth("));
    assert!(output.code.contains(".step_by("));
    assert!(output.code.contains("collect::<String>()"));
}

#[test]
fn rust_compile_accepts_explicit_infer_type_marker() {
    let session = test_session(Target::HirRust);
    // Ruling g bans `→ _` signature holes; the return is pinned to `numerus`
    // and the legal binding holes (`fixum _` / `varia _`) stay the test's
    // subject.
    let source = r#"functio answer() → numerus {
  redde 42
}

incipit {
  fixum _ name ← "Marcus"
  varia _ count ← answer()
  count ← count + 1
  nota "§: §"(name, count)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "expected compile success, got diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let output = super::rust_output(result);
    assert!(output.code.contains("fn answer() -> i64"));
    assert!(output.code.contains("let name: String"));
    assert!(output.code.contains("let mut count: i64"));
}

// ── smoke guards (slow, spawn external toolchains) ──

#[ignore = "slow: spawns external toolchain smoke (cargo/go/tsc); run with --ignored"]
#[test]
fn typescript_output_supports_unicode_textus_index_and_range_slice() {
    let session = test_session(Target::HirTypeScript);
    let source = r#"incipit {
  fixum _ ch ← "Salve, §!"[7]
  fixum _ prefix ← "hello world"[0‥5]
  fixum _ whole ← "hello world"[0 usque 10]
  fixum _ stepped ← "abcdef"[0‥6 per 2]
  nota ch, prefix, whole, stepped
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let Some(crate::Output::TypeScript(output)) = result.output else {
        panic!("expected TypeScript output");
    };
    assert!(output.code.contains("Array.from("));
    assert!(output.code.contains(".filter((_, n0)"));
    assert!(output.code.contains(".join(\"\")"));
    crate::backend_smoke::assert_hir_emitted_smoke(Target::HirTypeScript, &output.code, "ts-textus-slice");
}

#[ignore = "slow: spawns external toolchain smoke (cargo/go/tsc); run with --ignored"]
#[test]
fn go_output_supports_unicode_textus_index_and_range_slice() {
    let session = test_session(Target::HirGo);
    let source = r#"incipit {
  fixum _ ch ← "Salve, §!"[7]
  fixum _ prefix ← "hello world"[0‥5]
  fixum _ whole ← "hello world"[0 usque 10]
  fixum _ stepped ← "abcdef"[0‥6 per 2]
  nota ch, prefix, whole, stepped
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let Some(crate::Output::Go(output)) = result.output else {
        panic!("expected Go output");
    };
    assert!(output.code.contains("[]rune("));
    assert!(output.code.contains("d0 :="));
    assert!(output.code.contains("return string(out0)"));
    crate::backend_smoke::assert_hir_emitted_smoke(Target::HirGo, &output.code, "go-textus-slice");
}

#[ignore = "slow: spawns external toolchain smoke (cargo/go/tsc); run with --ignored"]
#[test]
fn hir_smoke_regression_guard_rust_rejects_mangled_emit() {
    if !crate::backend_smoke::rustc_available() {
        eprintln!("rustc not found on PATH; skipping rust smoke regression-guard");
        return;
    }
    let session = test_session(Target::HirRust);
    let result = compile(
        &session,
        "guard.fab",
        "incipit { nota \"salve\" }",
        ModuleCompileRequest::default(),
    );
    assert!(result.success());
    let emitted = super::rust_output(result).code;
    let mangled = crate::backend_smoke::mangle_emitted_text(&emitted);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::backend_smoke::assert_hir_emitted_smoke(Target::HirRust, &mangled, "regression-guard-rust");
    }));
    assert!(failed.is_err(), "mangled Rust emit must fail smoke when rustc is present");
}

#[ignore = "slow: spawns external toolchain smoke (cargo/go/tsc); run with --ignored"]
#[test]
fn hir_smoke_regression_guard_go_rejects_mangled_emit() {
    if !crate::backend_smoke::go_available() {
        eprintln!("go not found on PATH; skipping go smoke regression-guard");
        return;
    }
    let session = test_session(Target::HirGo);
    let result = compile(&session, "guard.fab", "incipit { nota 42 }", ModuleCompileRequest::default());
    assert!(result.success());
    let Some(crate::Output::Go(output)) = result.output else {
        panic!("expected Go output");
    };
    let mangled = crate::backend_smoke::mangle_emitted_text(&output.code);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::backend_smoke::assert_hir_emitted_smoke(Target::HirGo, &mangled, "regression-guard-go");
    }));
    assert!(failed.is_err(), "mangled Go emit must fail smoke when go is present");
}

#[ignore = "slow: spawns external toolchain smoke (cargo/go/tsc); run with --ignored"]
#[test]
fn hir_smoke_regression_guard_typescript_rejects_mangled_emit() {
    if !crate::backend_smoke::ts_typechecker_available() {
        eprintln!("tsc/deno not found on PATH; skipping typescript smoke regression-guard");
        return;
    }
    let session = test_session(Target::HirTypeScript);
    let result = compile(&session, "guard.fab", "incipit { nota 42 }", ModuleCompileRequest::default());
    assert!(result.success());
    let Some(crate::Output::TypeScript(output)) = result.output else {
        panic!("expected TypeScript output");
    };
    let mangled = crate::backend_smoke::mangle_emitted_text(&output.code);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::backend_smoke::assert_hir_emitted_smoke(Target::HirTypeScript, &mangled, "regression-guard-ts");
    }));
    assert!(
        failed.is_err(),
        "mangled TypeScript emit must fail smoke when tsc/deno is present"
    );
}

// ── tensor-returning calls in itera loops (SEM010 gate) ──

/// SEM010 regression pin: a `functio` returning `tensor<f32, [2,2]>` may be
/// called inside an `itera` body. The training exempla used to inline the
/// forward pass because calling a tensor-returning function inside a loop was
/// rejected by the then-current compiler (`SEM010`). The restriction no longer
/// exists; this test pins the positive behavior so the gate stays open and a
/// factored `forward` call remains legal inside a training loop.
#[test]
fn tensor_returning_function_call_in_itera_loop_compiles() {
    let source = r"
functio forward(
    tensor<f32, [2,2]> input,
    tensor<f32, [2,2]> weight,
    tensor<f32, [2,2]> bias
) → tensor<f32, [2,2]> {
    fixum tensor<f32, [2,2]> prediction ← input.matmul(weight)
    redde prediction + bias
}

incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    fixum tensor<f32, [2,2]> x ← flat ↦ tensor<f32, [2,2]>
    fixum tensor<f32, [2,2]> w ← flat ↦ tensor<f32, [2,2]>
    fixum tensor<f32, [2,2]> b ← flat ↦ tensor<f32, [2,2]>
    fixum numerus steps ← 2
    itera ab 0‥steps fixum step {
        fixum tensor<f32, [2,2]> pred ← forward(x, w, b)
        nota pred
    }
}
";
    let result = compile(
        &test_session(Target::MirLlvm),
        "loop_call.fab",
        source,
        ModuleCompileRequest::default(),
    );
    assert!(
        result.success(),
        "tensor-returning function call inside itera body should compile; diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn recovered_parse_co_reports_parse_and_semantic_errors() {
    // em-u3 continuation at the driver entry: a module whose parse recovers
    // keeps its successfully-parsed top-level items for semantic analysis, so
    // the corrupted item's PARSE030 and the clean item's SEM001 co-report in
    // one pass instead of the parse error masking the module. The corrupted
    // item (`broken`) is dropped (its span contains the parse-error spans);
    // the clean item (`uses_undefined`) is analyzed and emits its use-site
    // resolve error.
    let session = test_session(Target::HirRust);
    let source = r"
functio broken() → numerus {
    redde ∷ numerus
    redde ∷ numerus
}

functio uses_undefined() → numerus {
    redde definitely_not_defined
}
";
    let Err(diagnostics) =
        crate::driver::analyze_source(&session, "recovered-continuation.fab", source, AnalyzeRequest::default())
    else {
        panic!("recovered parse must fail analysis");
    };

    assert!(
        diagnostics.iter().any(|d| d.code == Some("PARSE030")),
        "corrupted item's parse error must be reported, got: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == Some("SEM001") && d.issue() == Some("unknown_identifier")),
        "clean item's semantic error must co-report, got: {diagnostics:?}"
    );
}
