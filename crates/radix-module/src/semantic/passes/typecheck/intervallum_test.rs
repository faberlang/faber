//! Typecheck and Rust-codegen probes for `intervallum<T>`.

use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirRust),
    )
    .compile_str("test.fab", source)
}

fn rust_code(result: crate::CompileResult) -> String {
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };
    output.code
}

fn accepts(result: &crate::CompileResult) {
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(result: &crate::CompileResult, issue: &str) {
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_typed_binding_accepts() {
    let source = r"incipit {
  sit intervallum<numerus> fines ← 0‥10
}";
    accepts(&compile(source));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_typed_parameter_accepts() {
    let source = r"functio probe(intervallum<numerus> fines) → vacuum { }";
    accepts(&compile(source));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_inferred_binding_accepts() {
    let source = r"incipit {
  sit _ fines ← 0‥10
}";
    accepts(&compile(source));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_zero_length_accepts() {
    let source = r"incipit {
  sit intervallum<numerus> fines ← 0‥0
}";
    accepts(&compile(source));
}

// The file's `compile` helper targets `HirRust`, so the accept probes compile
// out of builds without the rust leaf (the public cut), like their siblings.
#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_value_step_in_a_binding_accepts() {
    // A stored range carries its step in the type (operator ruling: the step
    // is a compile-time constant).
    let source = r"incipit {
  sit intervallum<numerus, 2> fines ← 0‥10 per 2
}";
    accepts(&compile(source));
}

#[test]
fn intervallum_runtime_step_in_a_binding_rejects() {
    // A step known only at run time stays inline-only: it is refused, never
    // silently dropped to step 1.
    let source = r"incipit {
  varia numerus k ← 2
  sit _ fines ← 0‥10 per k
}";
    rejects_with_issue(&compile(source), "intervallum_value_step_unsupported");
}

#[test]
fn intervallum_step_binding_literal_mismatch_rejects() {
    // A literal step must match the declared `S` either way; it is a type
    // error, never a silent conversion.
    let source = r"incipit {
  sit intervallum<numerus, 2> evens ← 0‥10 per 3
}";
    rejects_with_issue(&compile(source), "initializer_annotation_mismatch");
}

#[test]
fn intervallum_step_binding_missing_step_rejects() {
    // A step-less value is `intervallum<T, 1>`: declaring step 2 mismatches.
    let source = r"incipit {
  sit intervallum<numerus, 2> evens ← 0‥10
}";
    rejects_with_issue(&compile(source), "initializer_annotation_mismatch");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_step_binding_emits_the_stride() {
    let source = r"incipit {
  sit intervallum<numerus, 2> evens ← 0‥10 per 2
  sit intervallum<numerus> fines ← 0‥10
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(
        code.contains("faber::Intervallum::exclusive_per(0, 10, 2)"),
        "stepped binding should carry its stride: {code}"
    );
    assert!(
        code.contains("faber::Intervallum::exclusive(0, 10)"),
        "a step-less binding keeps the step-one constructor: {code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_step_remains_valid_for_itera_ab() {
    let source = r"incipit {
  itera ab 0‥10 per 2 sit i {
    nota i
  }
}";
    accepts(&compile(source));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_step_remains_valid_for_textus_slice() {
    let source = r#"incipit {
  sit _ even ← "abcdef"[0‥6 per 2]
}"#;
    accepts(&compile(source));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_stored_intra_accepts() {
    let source = r#"incipit {
  sit intervallum<numerus> fines ← 0‥10
  si 5 ∈ fines { nota "ok" }
}"#;
    accepts(&compile(source));
}

#[test]
fn textus_intra_range_rejects() {
    let source = r#"incipit { si "a" ∈ 0‥3 { nota "bad" } }"#;
    rejects_with_issue(&compile(source), "range_operand_must_be_numeric");
}

#[test]
fn membership_scalar_rhs_is_not_a_range() {
    // The keyword form `1 intra 2` retired with MG-1c; its glyph form takes
    // the collection rule, so a scalar right operand is not a range.
    let source = r#"incipit { si 1 ∈ 2 { nota "bad" } }"#;
    rejects_with_issue(&compile(source), "membership_requires_collection");
}

#[test]
fn intervallum_unknown_type_rejects() {
    let source = r"incipit {
  sit intervallum<textus> fines ← 0‥10
}";
    rejects_with_issue(&compile(source), "intervallum_bound");
}

#[test]
fn intervallum_fractus_rejects() {
    let source = r"incipit {
  sit intervallum<fractus> fines ← 0‥10
}";
    rejects_with_issue(&compile(source), "intervallum_bound");
}

#[test]
fn intervallum_sized_fractus_rejects() {
    let source = r"incipit {
  sit intervallum<f32> fines ← 0‥10
}";
    rejects_with_issue(&compile(source), "intervallum_bound");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intra_half_open_codegen_honors_exclusive_end() {
    let source = r#"incipit { si 5 ∈ 0‥10 { nota "ok" } }"#;
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("Intervallum::exclusive"));
    assert!(code.contains(".continet(&"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intra_inclusive_codegen_honors_inclusive_end() {
    let source = r#"incipit { si 10 ∈ 0…10 { nota "ok" } }"#;
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("Intervallum::inclusive"));
    assert!(code.contains(".continet(&"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_literal_codegen_emits_carrier() {
    let source = r"incipit {
  sit _ fines ← 0‥10
  nota fines
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("faber::Intervallum::exclusive"));
    assert!(!code.contains("Valor::from((0, 10))"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_value_clamp_half_open_codegen() {
    let source = r"incipit {
  sit _ clamped ← 15 ↦ 0‥10
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("Intervallum::exclusive(0, 10).coercere(15)"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_value_clamp_inclusive_codegen() {
    let source = r"incipit {
  sit _ clamped ← 15 ↦ 0…10
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("Intervallum::inclusive(0, 10).coercere(15)"));
}

#[ignore = "slow: spawns cargo check of generated Rust; run with --ignored"]
#[test]
fn intervallum_materialize_half_open_codegen() {
    let source = r"incipit {
  sit lista<numerus> xs ← 0‥10 ↦ lista<numerus>
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("faber::Intervallum::exclusive(0, 10).ad_lista()"));
    crate::backend_smoke::assert_hir_emitted_smoke(crate::Target::HirRust, &code, "intervallum-materialize-lista");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_materialize_inclusive_codegen() {
    let source = r"incipit {
  sit lista<numerus> xs ← 0…10 ↦ lista<numerus>
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("faber::Intervallum::inclusive(0, 10).ad_lista()"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_materialize_tensor_half_open_codegen() {
    let source = r"incipit {
  sit tensor<numerus, [10]> grid ← 0‥10 ↦ tensor<numerus, [10]>
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("Intervallum::exclusive(0, 10).ad_tensor()"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_materialize_tensor_inclusive_codegen() {
    let source = r"incipit {
  sit tensor<numerus, [4]> grid ← 0…3 ↦ tensor<numerus, [4]>
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("Intervallum::inclusive(0, 3).ad_tensor()"));
}

#[test]
fn intervallum_materialize_tensor_element_mismatch_rejects() {
    let source = r"incipit {
  sit tensor<fractus, [10]> grid ← 0‥10 ↦ tensor<fractus, [10]>
}";
    rejects_with_issue(&compile(source), "intervallum_materialize_tensor_element_mismatch");
}

#[test]
fn intervallum_sized_numerus_materialize_rejects_until_runtime_helpers_exist() {
    let source = r"incipit {
  sit u8 lo ← 1 ∷ u8
  sit u8 hi ← 4 ∷ u8
  sit intervallum<u8> fines ← lo‥hi
  sit lista<u8> xs ← fines ↦ lista<u8>
}";
    rejects_with_issue(&compile(source), "intervallum_helper_bound_unsupported");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_range_clamp_codegen_inherits_target_inclusivity() {
    let source = r"incipit {
  sit intervallum<numerus> wide ← 0‥100
  sit intervallum<numerus> narrow ← wide ↦ 10…50
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains(".coercere_intervallum(&"));
    assert!(code.contains("Intervallum::inclusive(10, 50)"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_continet_method_codegen() {
    let source = r"incipit {
  sit intervallum<numerus> fines ← 0‥10
  sit _ ok ← fines.continet(5)
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("fines.continet(&5)"));
}

#[test]
fn intervallum_clamp_recovery_rejects() {
    let source = r"incipit {
  sit _ clamped ← 15 ↦ 0‥10 ⊥ 0
}";
    rejects_with_issue(&compile(source), "intervallum_clamp_recovery_unsupported");
}

#[test]
fn intervallum_sized_numerus_clamp_rejects_until_runtime_helpers_exist() {
    let source = r"incipit {
  sit u8 x ← 7 ∷ u8
  sit u8 lo ← 1 ∷ u8
  sit u8 hi ← 4 ∷ u8
  sit intervallum<u8> fines ← lo‥hi
  sit u8 clamped ← x ↦ fines
}";
    rejects_with_issue(&compile(source), "intervallum_helper_bound_unsupported");
}

#[test]
fn intervallum_sized_numerus_range_clamp_rejects_until_runtime_helpers_exist() {
    let source = r"incipit {
  sit u8 zero ← 0 ∷ u8
  sit u8 one ← 1 ∷ u8
  sit u8 four ← 4 ∷ u8
  sit u8 ten ← 10 ∷ u8
  sit intervallum<u8> wide ← zero‥ten
  sit intervallum<u8> narrow ← one‥four
  sit intervallum<u8> clipped ← wide ↦ narrow
}";
    rejects_with_issue(&compile(source), "intervallum_helper_bound_unsupported");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_inter_overlapping_codegen() {
    let source = r"incipit {
  sit intervallum<numerus> left ← 0‥10
  sit intervallum<numerus> right ← 5‥15
  sit _ hit ← left.inter(right)
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains(".inter("));
}

/// The method `inter` (en `intersect`) is untouched by the retirement of the
/// membership words: the Latin spelling checks, and the English reader knows
/// only `intersect` (`inter` is not an en intervallum method).
#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_intersection_method_spellings_survive_the_membership_retirement() {
    let latin = r"incipit {
  sit intervallum<numerus> left ← 0‥10
  sit intervallum<numerus> right ← 5‥15
  sit _ hit ← left.inter(right)
}";
    accepts(&compile(latin));
    let en = |method: &str| {
        format!(
            "+++\nlocale = \"en\"\n+++\nmain {{\n    let range<int> a ← 0‥10\n    let range<int> b ← 5‥15\n    let _ x ← a.{method}(b)\n    print x\n}}\n"
        )
    };
    accepts(&compile(&en("intersect")));
    rejects_with_issue(&compile(&en("inter")), "unknown_method");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_union_adjacent_codegen() {
    let source = r"incipit {
  sit intervallum<numerus> left ← 0‥5
  sit intervallum<numerus> right ← 5‥10
  sit _ merged ← left.union(right)
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains(".union("));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_longitudo_codegen() {
    let source = r"incipit {
  sit intervallum<numerus> fines ← 0‥10
  sit _ span ← fines.longitudo()
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains(".longitudo()"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_instans_typed_parameter_accepts() {
    let source = r"functio probe(instans initium, instans finis, intervallum<instans> fines) → vacuum { }";
    accepts(&compile(source));
}

#[test]
fn intervallum_instans_longitudo_rejects() {
    let source = r"functio probe(intervallum<instans> fines) → numerus {
  redde fines.longitudo()
}";
    rejects_with_issue(&compile(source), "intervallum_longitudo_bound");
}

#[test]
fn intervallum_sized_numerus_longitudo_rejects_until_runtime_helpers_exist() {
    let source = r"functio probe(intervallum<u8> fines) → numerus {
  redde fines.longitudo()
}";
    rejects_with_issue(&compile(source), "intervallum_longitudo_bound");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_step_type_parameter_accepts() {
    accepts(&compile(r"functio probe(intervallum<numerus, 2> fines) → vacuum { }"));
    accepts(&compile(r"functio probe(intervallum<numerus, 1> fines) → vacuum { }"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_step_type_alias_accepts() {
    let source = r"typus Stepped = intervallum<numerus, 2>
functio probe(Stepped fines) → vacuum { }";
    accepts(&compile(source));
}

#[test]
fn intervallum_step_zero_rejects_not_positive() {
    let source = r"functio probe(intervallum<numerus, 0> fines) → vacuum { }";
    rejects_with_issue(&compile(source), "range_step_not_positive");
}

#[test]
fn intervallum_step_name_rejects_must_be_constant() {
    let source = r"functio probe(intervallum<numerus, n> fines) → vacuum { }";
    rejects_with_issue(&compile(source), "intervallum_step_must_be_constant");
}

#[test]
fn intervallum_step_instans_bound_rejects_not_integer() {
    let source = r"functio probe(intervallum<instans, 2> fines) → vacuum { }";
    rejects_with_issue(&compile(source), "intervallum_step_bound_not_integer");
}

#[test]
fn intervallum_step_arity_still_rejects_three_args() {
    let source = r"functio probe(intervallum<numerus, 2, 3> fines) → vacuum { }";
    rejects_with_issue(&compile(source), "intervallum_arity");
}

// The file's `compile` helper targets `HirRust`, so the accept probes compile
// out of builds without the rust leaf (the public cut), like their siblings.
#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_step_binding_initializer_accepts() {
    // The stored step is part of the value's type, so a literal step that
    // matches the declared `S` compiles.
    let source = r"incipit {
  sit intervallum<numerus, 2> evens ← 0‥10 per 2
}";
    accepts(&compile(source));
}

/// A value clamped into a stepped range has no ruled meaning (the stride
/// cannot survive as a `T` result), so it is refused by name.
#[test]
fn intervallum_clamp_stepped_target_rejects() {
    let source = r"incipit {
  sit numerus x ← 15 ↦ 0…10 per 2
}";
    rejects_with_issue(&compile(source), "intervallum_value_step_unsupported");
}

#[test]
fn intervallum_stepped_receiver_coercere_rejects() {
    let source = r"incipit {
  sit intervallum<numerus, 2> evens ← 0‥10 per 2
  sit _ clamped ← evens.coercere(3)
}";
    rejects_with_issue(&compile(source), "intervallum_value_step_unsupported");
}

#[test]
fn intervallum_stepped_receiver_inter_rejects() {
    let source = r"incipit {
  sit intervallum<numerus, 2> evens ← 0‥10 per 2
  sit _ hit ← evens.inter(evens)
}";
    rejects_with_issue(&compile(source), "intervallum_value_step_unsupported");
}

#[test]
fn intervallum_stepped_receiver_union_rejects() {
    let source = r"incipit {
  sit intervallum<numerus, 2> evens ← 0‥10 per 2
  sit _ merged ← evens.union(evens)
}";
    rejects_with_issue(&compile(source), "intervallum_value_step_unsupported");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_stepped_receiver_query_accepts() {
    // `longitudo` and `continet` read the stride, so a stepped receiver stays
    // valid for both.
    let source = r"incipit {
  sit intervallum<numerus, 2> evens ← 0‥10 per 2
  sit _ span ← evens.longitudo()
  sit _ has ← evens.continet(4)
}";
    accepts(&compile(source));
}
