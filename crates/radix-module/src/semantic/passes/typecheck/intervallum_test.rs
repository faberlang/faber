//! Typecheck and Rust-codegen probes for `intervallum<T>`.

use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("test.fab", source)
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
  fixum intervallum<numerus> fines ← 0‥10
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
  fixum _ fines ← 0‥10
}";
    accepts(&compile(source));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_zero_length_accepts() {
    let source = r"incipit {
  fixum intervallum<numerus> fines ← 0‥0
}";
    accepts(&compile(source));
}

#[test]
fn intervallum_value_step_rejects() {
    let source = r"incipit {
  fixum _ fines ← 0‥10 per 2
}";
    rejects_with_issue(&compile(source), "intervallum_value_step_unsupported");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_step_remains_valid_for_itera_ab() {
    let source = r"incipit {
  itera ab 0‥10 per 2 fixum i {
    nota i
  }
}";
    accepts(&compile(source));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_step_remains_valid_for_textus_slice() {
    let source = r#"incipit {
  fixum _ even ← "abcdef"[0‥6 per 2]
}"#;
    accepts(&compile(source));
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_stored_intra_accepts() {
    let source = r#"incipit {
  fixum intervallum<numerus> fines ← 0‥10
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
  fixum intervallum<textus> fines ← 0‥10
}";
    rejects_with_issue(&compile(source), "intervallum_bound");
}

#[test]
fn intervallum_fractus_rejects() {
    let source = r"incipit {
  fixum intervallum<fractus> fines ← 0‥10
}";
    rejects_with_issue(&compile(source), "intervallum_bound");
}

#[test]
fn intervallum_sized_fractus_rejects() {
    let source = r"incipit {
  fixum intervallum<f32> fines ← 0‥10
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
  fixum _ fines ← 0‥10
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
  fixum _ clamped ← 15 ↦ 0‥10
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
  fixum _ clamped ← 15 ↦ 0…10
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
  fixum lista<numerus> xs ← 0‥10 ↦ lista<numerus>
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
  fixum lista<numerus> xs ← 0…10 ↦ lista<numerus>
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
  fixum tensor<numerus, [10]> grid ← 0‥10 ↦ tensor<numerus, [10]>
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
  fixum tensor<numerus, [4]> grid ← 0…3 ↦ tensor<numerus, [4]>
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("Intervallum::inclusive(0, 3).ad_tensor()"));
}

#[test]
fn intervallum_materialize_tensor_element_mismatch_rejects() {
    let source = r"incipit {
  fixum tensor<fractus, [10]> grid ← 0‥10 ↦ tensor<fractus, [10]>
}";
    rejects_with_issue(&compile(source), "intervallum_materialize_tensor_element_mismatch");
}

#[test]
fn intervallum_sized_numerus_materialize_rejects_until_runtime_helpers_exist() {
    let source = r"incipit {
  fixum u8 lo ← 1 ∷ u8
  fixum u8 hi ← 4 ∷ u8
  fixum intervallum<u8> fines ← lo‥hi
  fixum lista<u8> xs ← fines ↦ lista<u8>
}";
    rejects_with_issue(&compile(source), "intervallum_helper_bound_unsupported");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_range_clamp_codegen_inherits_target_inclusivity() {
    let source = r"incipit {
  fixum intervallum<numerus> wide ← 0‥100
  fixum intervallum<numerus> narrow ← wide ↦ 10…50
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
  fixum intervallum<numerus> fines ← 0‥10
  fixum _ ok ← fines.continet(5)
}";
    let result = compile(source);
    accepts(&result);
    let code = rust_code(result);
    assert!(code.contains("fines.continet(&5)"));
}

#[test]
fn intervallum_clamp_recovery_rejects() {
    let source = r"incipit {
  fixum _ clamped ← 15 ↦ 0‥10 ⊥ 0
}";
    rejects_with_issue(&compile(source), "intervallum_clamp_recovery_unsupported");
}

#[test]
fn intervallum_sized_numerus_clamp_rejects_until_runtime_helpers_exist() {
    let source = r"incipit {
  fixum u8 x ← 7 ∷ u8
  fixum u8 lo ← 1 ∷ u8
  fixum u8 hi ← 4 ∷ u8
  fixum intervallum<u8> fines ← lo‥hi
  fixum u8 clamped ← x ↦ fines
}";
    rejects_with_issue(&compile(source), "intervallum_helper_bound_unsupported");
}

#[test]
fn intervallum_sized_numerus_range_clamp_rejects_until_runtime_helpers_exist() {
    let source = r"incipit {
  fixum u8 zero ← 0 ∷ u8
  fixum u8 one ← 1 ∷ u8
  fixum u8 four ← 4 ∷ u8
  fixum u8 ten ← 10 ∷ u8
  fixum intervallum<u8> wide ← zero‥ten
  fixum intervallum<u8> narrow ← one‥four
  fixum intervallum<u8> clipped ← wide ↦ narrow
}";
    rejects_with_issue(&compile(source), "intervallum_helper_bound_unsupported");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_inter_overlapping_codegen() {
    let source = r"incipit {
  fixum intervallum<numerus> left ← 0‥10
  fixum intervallum<numerus> right ← 5‥15
  fixum _ hit ← left.inter(right)
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
  fixum intervallum<numerus> left ← 0‥10
  fixum intervallum<numerus> right ← 5‥15
  fixum _ hit ← left.inter(right)
}";
    accepts(&compile(latin));
    let en = |method: &str| {
        format!(
            "+++\nlocale = \"en\"\n+++\nmain {{\n    const intervallum<int> a ← 0‥10\n    const intervallum<int> b ← 5‥15\n    const _ x ← a.{method}(b)\n    print x\n}}\n"
        )
    };
    accepts(&compile(&en("intersect")));
    rejects_with_issue(&compile(&en("inter")), "unknown_method");
}

#[cfg(feature = "hir-rust")]
#[test]
fn intervallum_union_adjacent_codegen() {
    let source = r"incipit {
  fixum intervallum<numerus> left ← 0‥5
  fixum intervallum<numerus> right ← 5‥10
  fixum _ merged ← left.union(right)
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
  fixum intervallum<numerus> fines ← 0‥10
  fixum _ span ← fines.longitudo()
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
