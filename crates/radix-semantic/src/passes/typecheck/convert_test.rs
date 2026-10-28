//! Regression coverage for typecheck-level `↦` conversio and `↤` assignment.
//!
//! These tests lock the conversio matrix and conversion-directed assignment
//! contracts without depending on codegen emission.

use crate::passes::typecheck::test_support;

#[test]
fn conversio_rejects_struct_to_numerus() {
    test_support::assert_rejects_with_issue_la(
        r"
genus Foo { varia numerus n }

functio bad(Foo f) → numerus {
    redde f ↦ numerus
}
",
        "unsupported_conversio",
    );
}

#[test]
fn textus_to_regex_conversio_typechecks() {
    test_support::assert_accepts_la(r#"incipit { sit _ r ← "\\d+" ↦ regex }"#);
}

#[test]
fn ascii_to_regex_conversio_typechecks_via_textus_widening() {
    test_support::assert_accepts_la(r"incipit { sit _ r ← '(?g)\\d+' ↦ regex }");
}

#[test]
fn regex_conversio_rejects_recovery_slot() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit { sit _ r ← "\\d[" ↦ regex ⊥ "\\d+" }"#,
        "regex_recovery_unsupported",
    );
}

#[test]
fn regex_literal_pattern_has_no_length_cap() {
    // The conversion is the one place a pattern is compiled; there is no
    // check-time length policy on the literal.
    let pattern = "a".repeat(1100);
    let source = format!(r#"incipit {{ sit _ r ← "{pattern}" ↦ regex }}"#);
    test_support::assert_accepts_la(&source);
}

#[test]
fn regex_conversio_dynamic_source_with_text_channel_typechecks() {
    test_support::assert_accepts_la(
        r"functio f(textus pat) → regex ⇥ textus { redde pat ↦ regex }",
    );
}

#[test]
fn conversio_recovery_type_mismatch_rejects() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit { sit _ n ← "x" ↦ numerus ⊥ "string" }"#,
        "conversio_recovery_type_mismatch",
    );
}

#[test]
fn intervallum_clamp_operand_type_mismatch_rejects_with_structured_issue_arg() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit { sit _ clamped ← "x" ↦ 0‥10 }"#,
        "intervallum_clamp_operand_type_mismatch",
    );
}

#[test]
fn bare_fallible_conversio_requires_textus_alternate_exit() {
    test_support::assert_rejects_with_issue_la(
        r"
functio parse(valor v) → instans ⇥ numerus {
    redde v ↦ instans
}
",
        "conversio_failure_channel_textus_required",
    );
}

#[test]
fn inline_conversio_recovery_allows_non_textus_alternate_exit() {
    test_support::assert_accepts_la(
        r"
functio parse(valor v, instans epoch) → instans ⇥ numerus {
    redde v ↦ instans ⊥ epoch
}
",
    );
}

#[test]
fn valor_to_sized_numerus_direct_extraction_typechecks() {
    test_support::assert_accepts_la(
        r"
functio parse(valor v) → u8 ⇥ textus {
    redde v ↦ u8
}
",
    );
}

#[test]
fn valor_to_sized_fractus_direct_extraction_rejects_until_width_policy_exists() {
    test_support::assert_rejects_with_issue_la(
        r"
functio parse(valor v) → f32 {
    redde v ↦ f32
}
",
        "valor_to_sized_numeric_unsupported",
    );
}

#[test]
fn valor_to_sized_numeric_lista_extraction_rejects_until_runtime_trait_exists() {
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    sit valor v ← [1, 2]
    sit lista<u8> xs ← v ↦ lista<u8>
}
",
        "valor_to_sized_numeric_unsupported",
    );
}

#[test]
fn valor_to_genus_sized_fractus_field_rejects_until_width_policy_exists() {
    test_support::assert_rejects_with_issue_la(
        r#"
genus Sample {
    varia f32 ratio
}

incipit {
    sit valor v ← { "ratio": 1.5 } ↦ valor
    sit Sample s ← v ↦ Sample
}
"#,
        "valor_to_sized_numeric_unsupported",
    );
}

#[test]
fn infallible_conversio_allows_non_textus_alternate_exit() {
    test_support::assert_accepts_la(
        r"
functio widen(numerus n) → fractus ⇥ numerus {
    redde n ↦ fractus
}
",
    );
}

#[test]
fn truthiness_conversio_allows_non_textus_alternate_exit() {
    test_support::assert_accepts_la(
        r"
functio truthy(textus s) → bivalens ⇥ numerus {
    redde s ↦ bivalens
}
",
    );
}

#[test]
fn numeric_truthiness_conversio_rejects_recovery_slot() {
    for source in ["0 ↦ bivalens ⊥ verum", "0.0 ↦ bivalens ⊥ verum"] {
        test_support::assert_rejects_with_issue_la(
            &format!("incipit {{ sit _ b ← {source} }}"),
            "bivalens_recovery_unsupported",
        );
    }
}

/// Q-B4: a text or ASCII word parse is fallible, so an inline ⊥ default is legal even though numeric truthiness has nothing to recover.
#[test]
fn text_truthiness_conversio_accepts_recovery_slot() {
    for source in ["\"salve\" ↦ bivalens ⊥ falsum", "'no' ↦ bivalens ⊥ falsum"] {
        test_support::assert_accepts_la(&format!("incipit {{ sit _ b ← {source} }}"));
    }
}

#[test]
fn textus_to_octeti_conversio_typechecks() {
    test_support::assert_accepts_la(r#"incipit { sit _ b ← "hi" ↦ lista<b8> }"#);
}

#[test]
fn octeti_to_textus_conversio_typechecks() {
    test_support::assert_accepts_la(r"incipit { sit _ s ← |68 69| ↦ textus }");
}

#[test]
fn lista_numerus_to_textus_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit lista<numerus> a ← [72, 105]\n  sit _ s ← a ↦ textus\n}",
        "lista_to_textus_unsupported",
    );
}

#[test]
fn lista_textus_to_textus_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit lista<textus> a ← [\"x\", \"y\"]\n  sit _ s ← a ↦ textus\n}",
        "lista_to_textus_unsupported",
    );
}

#[test]
fn textus_to_lista_of_textus_is_a_named_reject() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit _ cs ← \"abc\" ↦ lista<textus>\n}",
        "textus_to_lista_unsupported",
    );
}

#[test]
fn textus_to_lista_of_numerus_is_a_named_reject() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit _ cs ← \"abc\" ↦ lista<numerus>\n}",
        "textus_to_lista_unsupported",
    );
}

#[test]
fn textus_to_lista_of_bits_stays_accepted() {
    test_support::assert_accepts_la("incipit {\n  sit _ b ← \"abc\" ↦ lista<b8>\n}");
}

#[test]
fn bounded_lista_to_bounded_textus_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit lista<numerus> base ← [1, 2]\n  sit lista<numerus, magnitudo 8> raw ← base ↦ lista<numerus, magnitudo 8>\n  sit textus<magnitudo 8> out ← raw ↦ textus<magnitudo 8>\n}",
        "lista_to_textus_unsupported",
    );
}

#[test]
fn bounded_textus_conversio_accepts_inline_recovery() {
    test_support::assert_accepts_la(
        "functio s(textus<magnitudo 4> t) → textus {\n  redde t ↦ textus ⊥ \"fallback\"\n}",
    );
}

#[test]
fn octeti_variable_to_textus_conversio_stays_legal() {
    test_support::assert_accepts_la(
        "incipit {\n  sit lista<b8> b ← |68 69|\n  sit _ s ← b ↦ textus\n}",
    );
}

#[test]
fn ascii_octeti_conversio_roundtrip_typechecks() {
    test_support::assert_accepts_la(
        "incipit {\n  sit _ b ← 'hi' ↦ lista<b8>\n  sit _ a ← b ↦ ascii\n}",
    );
}

#[test]
fn octeti_textus_recovery_typechecks() {
    test_support::assert_accepts_la(r#"incipit { sit _ s ← |ff| ↦ textus ⊥ "?" }"#);
}

#[test]
fn fractus_to_octeti_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ b ← 1.5 ↦ lista<b8> }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn bivalens_to_octeti_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ b ← verum ↦ lista<b8> }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn octeti_to_fractus_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ f ← |00| ↦ fractus }",
        "octeti_to_fractus_unspecified",
    );
}

enum MatrixExpect {
    Accepts,
    RejectsIssue(&'static str),
}

struct ConversioMatrixCase {
    name: &'static str,
    source: &'static str,
    expect: MatrixExpect,
}

fn primitive_conversio_matrix_cases() -> &'static [ConversioMatrixCase] {
    &[
        ConversioMatrixCase {
            name: "numerus_to_fractus",
            source: "incipit { sit _ f ← 42 ↦ fractus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "numerus_to_bivalens",
            source: "incipit { sit _ b ← 42 ↦ bivalens }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "numerus_to_textus",
            source: "incipit { sit _ s ← 42 ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "fractus_to_numerus",
            source: "incipit { sit _ n ← 1.5 ↦ numerus via Trunc }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "fractus_to_bivalens",
            source: "incipit { sit _ b ← 1.5 ↦ bivalens }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "fractus_to_textus",
            source: "incipit { sit _ s ← 1.5 ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "bivalens_to_numerus",
            source: "incipit { sit _ n ← verum ↦ numerus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "bivalens_to_fractus",
            source: "incipit { sit _ f ← verum ↦ fractus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "bivalens_to_textus",
            source: "incipit { sit _ s ← verum ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "textus_to_numerus",
            source: r#"incipit { sit _ n ← "42" ↦ numerus }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "textus_to_fractus",
            source: r#"incipit { sit _ f ← "1.5" ↦ fractus }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "textus_to_bivalens",
            source: r#"incipit { sit _ b ← "hi" ↦ bivalens }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "ascii_to_bivalens",
            source: "incipit { sit _ b ← 'hi' ↦ bivalens }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "ascii_to_textus",
            source: "incipit { sit _ s ← 'hi' ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "numerus_to_octeti",
            source: "incipit { sit _ b ← 42 ↦ lista<b8> }",
            expect: MatrixExpect::RejectsIssue("numeric_to_octeti_unspecified"),
        },
        ConversioMatrixCase {
            name: "octeti_to_numerus",
            source: "incipit { sit _ n ← |00| ↦ numerus }",
            expect: MatrixExpect::RejectsIssue("octeti_to_numerus_unspecified"),
        },
        ConversioMatrixCase {
            name: "octeti_to_bivalens",
            source: "incipit { sit _ b ← |00| ↦ bivalens }",
            expect: MatrixExpect::RejectsIssue("octeti_to_bivalens_unsupported"),
        },
        ConversioMatrixCase {
            name: "non_text_to_regex",
            source: "incipit { sit _ r ← 42 ↦ regex }",
            expect: MatrixExpect::RejectsIssue("regex_operand_unsupported"),
        },
        ConversioMatrixCase {
            name: "instans_widen_to_millis",
            source: "functio widen(instans t) → instans<ms> { redde t ↦ instans<ms> }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "instans_narrow_to_seconds",
            source: "functio narrow(instans<ns> t) → instans { redde t ↦ instans }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_instans",
            source: r#"incipit { sit valor v ← "1979-05-27T07:32:00Z" sit instans t ← v ↦ instans }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_instans_offset",
            source: r#"incipit { sit valor v ← "1979-05-27T07:32:00-04:00" sit instans t ← v ↦ instans }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "instans_to_textus",
            source: "functio wire(instans t) → textus { redde t ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_numerus",
            source: r"incipit { sit valor v ← 42 sit numerus n ← v ↦ numerus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_fractus_widen",
            source: r"incipit { sit valor v ← 7 sit fractus f ← v ↦ fractus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_textus_instans_wire",
            source: r#"incipit { sit valor v ← "1979-05-27T07:32:00Z" sit textus s ← v ↦ textus }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_bivalens",
            source: r"incipit { sit valor v ← verum sit bivalens b ← v ↦ bivalens }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_ascii",
            source: r"incipit { sit valor v ← 'ok' sit ascii a ← v ↦ ascii }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_numerus_recovery",
            source: r#"incipit { sit valor v ← "x" sit numerus n ← v ↦ numerus ⊥ 0 }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_lista",
            source: r"incipit { sit valor v ← [1, 2, 3] sit lista<numerus> xs ← v ↦ lista<numerus> }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_tabula",
            source: r#"incipit { sit valor v ← { "a": 1 } ↦ valor sit tabula<textus, numerus> m ← v ↦ tabula<textus, numerus> }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "lista_to_valor",
            source: r"incipit { sit lista<numerus> xs ← [1, 2] sit valor v ← xs ↦ valor }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "tensor_to_valor_compose",
            source: r"incipit { sit tensor<numerus, [2]> t ← [1, 2] ↦ tensor<numerus, [2]> sit valor v ← t ↦ valor }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_verte_numerus",
            source: r"incipit { sit valor v ← 42 sit numerus n ← v ∷ numerus }",
            expect: MatrixExpect::RejectsIssue("valor_verte_unsupported"),
        },
        ConversioMatrixCase {
            name: "valor_verte_tabula",
            source: r#"incipit { sit valor v ← { "a": 1 } ↦ valor sit _ m ← v ∷ tabula<textus, numerus> }"#,
            expect: MatrixExpect::RejectsIssue("valor_verte_unsupported"),
        },
        ConversioMatrixCase {
            name: "valor_to_cursor",
            source: r"incipit { sit valor v ← [1] sit _ c ← v ↦ cursor<numerus> }",
            expect: MatrixExpect::RejectsIssue("valor_to_cursor_boundary"),
        },
        ConversioMatrixCase {
            name: "valor_to_promissum",
            source: r"incipit { sit valor v ← 1 sit _ p ← v ↦ promissum<numerus> }",
            expect: MatrixExpect::RejectsIssue("valor_to_promissum_boundary"),
        },
        ConversioMatrixCase {
            name: "valor_to_regex",
            source: r#"incipit { sit valor v ← "x" sit _ r ← v ↦ regex }"#,
            expect: MatrixExpect::RejectsIssue("valor_to_regex_unsupported"),
        },
        ConversioMatrixCase {
            name: "promissum_to_valor",
            source: r"functio box(promissum<numerus> p) → valor { redde p ↦ valor }",
            expect: MatrixExpect::RejectsIssue("promissum_to_valor_boundary"),
        },
        ConversioMatrixCase {
            name: "cursor_to_valor",
            source: r"functio box(cursor<numerus> c) → valor { redde c ↦ valor }",
            expect: MatrixExpect::RejectsIssue("cursor_to_valor_boundary"),
        },
    ]
}

#[test]
fn instans_cross_precision_equality_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"
functio same(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ≡ coarse
}
",
        "equality_exact_types_required",
    );
}

#[test]
fn instans_cross_precision_ordering_typechecks() {
    test_support::assert_accepts_la(
        r"
functio before(instans fine, instans<ms> coarse) → bivalens {
    redde fine ≺ coarse
}
",
    );
}

#[test]
fn primitive_conversio_matrix_typecheck() {
    for case in primitive_conversio_matrix_cases() {
        let result = test_support::compile_and_get_result_la(case.source);
        match case.expect {
            MatrixExpect::Accepts => {
                assert!(result.is_ok(), "{}: {:?}", case.name, result.diagnostics);
            }
            MatrixExpect::RejectsIssue(issue) => {
                assert!(!result.is_ok(), "{}: {:?}", case.name, result.diagnostics);
                assert!(
                    result
                        .diagnostics
                        .iter()
                        .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
                    "{}: expected diagnostic issue {issue:?}, got: {:?}",
                    case.name,
                    result.diagnostics
                );
            }
        }
    }
}

#[test]
fn lista_to_copia_conversio_typechecks() {
    test_support::assert_accepts_la("incipit { sit _ s ← [1, 2, 2] ↦ copia<numerus> }");
}

#[test]
fn copia_to_lista_conversio_typechecks() {
    test_support::assert_accepts_la(
        "incipit {\n  varia copia<numerus> s ← vacua\n  s.adde(1)\n  sit _ xs ← s ↦ lista<numerus>\n}",
    );
}

#[test]
fn tensor_to_lista_conversio_typechecks() {
    test_support::assert_accepts_la(
        "incipit {\n  sit tensor<numerus, []> t ← vacua\n  sit _ xs ← t ↦ lista<numerus>\n}",
    );
}

#[test]
fn lista_to_tensor_conversio_typechecks() {
    test_support::assert_accepts_la("incipit { sit _ t ← [1, 2, 3] ↦ tensor<numerus, [3]> }");
}

#[test]
fn shaped_lista_to_tensor_conversio_typechecks() {
    test_support::assert_accepts_la(
        "incipit { sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]\n  sit tf32[2, 2] t ← flat ↦ tf32[2, 2] }",
    );
}

#[test]
fn rectangular_lista_literal_to_tensor_conversio_typechecks() {
    test_support::assert_accepts_la(
        "incipit { sit tf32[2, 2] t ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[2, 2] }",
    );
}

#[test]
fn rectangular_lista_literal_to_tensor_conversio_rejects_ragged_rows() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ t ← [[1.0, 2.0], [3.0]] ↦ tf32[2, 2] }",
        "rectangular_lista_literal_row_length",
    );
}

#[test]
fn rectangular_lista_literal_to_tensor_conversio_rejects_shape_mismatch() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ t ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[3, 3] }",
        "rectangular_lista_literal_shape",
    );
}

#[test]
fn sparse_tensor_conversio_typechecks_both_directions() {
    test_support::assert_accepts_la(
        r"incipit {
  sit tf32[2, 2] dense ← [[1.0, 0.0], [0.0, 2.0]] ↦ tf32[2, 2]
  sit sf32[2, 2] sparse ← dense ↦ sf32[2, 2]
  sit tf32[2, 2] roundtrip ← sparse ↦ tf32[2, 2]
  nota roundtrip.longitudo()
}",
    );
}

#[test]
fn sparse_tensor_conversio_rejects_shape_change() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
  sit tf32[2, 2] dense ← vacua
  sit sf32[4] sparse ← dense ↦ sf32[4]
  nota sparse.longitudo()
}",
        "sparsa_tensor_shape_mismatch",
    );
}

#[test]
fn sparse_tensor_conversio_rejects_element_change() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
  sit tf32[2] dense ← vacua
  sit sf64[2] sparse ← dense ↦ sf64[2]
  nota sparse.longitudo()
}",
        "sparsa_tensor_element_mismatch",
    );
}

#[test]
fn sparse_to_lista_conversio_rejects_as_ambiguous() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
  sit sf32[2] sparse ← vacua
  sit lista<f32> values ← sparse ↦ lista<f32>
  nota values.longitudo()
}",
        "sparsa_collection_unsupported",
    );
}

#[test]
fn nested_lista_variable_to_tensor_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit lista<lista<f32>> rows ← [[1.0, 2.0], [3.0, 4.0]]\n  sit _ t ← rows ↦ tf32[2, 2] }",
        "lista_to_tensor_nested",
    );
}

#[test]
fn lista_to_cursor_conversio_typechecks() {
    test_support::assert_accepts_la("incipit { sit _ c ← [1, 2, 3] ↦ cursor<numerus> }");
}

#[test]
fn cursor_to_lista_conversio_typechecks() {
    test_support::assert_accepts_la(
        "incipit { sit _ xs ← [1, 2, 3] ↦ cursor<numerus> ↦ lista<numerus> }",
    );
}

#[test]
fn tabula_to_lista_keys_conversio_typechecks() {
    test_support::assert_accepts_la(
        r#"incipit {
  sit tabula<textus, numerus> m ← { "a": 1, "b": 2 }
  sit _ ks ← m ↦ lista<textus>
}"#,
    );
}

#[test]
fn tabula_to_lista_values_conversio_typechecks() {
    test_support::assert_accepts_la(
        r#"incipit {
  sit tabula<textus, numerus> m ← { "a": 1, "b": 2 }
  sit _ vs ← m ↦ lista<numerus>
}"#,
    );
}

#[test]
fn lista_to_tabula_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ m ← [1, 2] ↦ tabula<textus, numerus> }",
        "lista_to_tabula_unsupported",
    );
}

#[test]
fn array_ascription_to_generic_map_key_rejects() {
    // Operator ruling (2026-09-21, total): an array is not a map, so an
    // array-to-map ascription is rejected whatever the key type is.
    test_support::assert_rejects_with_issue_la(
        r"functio selecta<K, V>() → tabula<K, V> {
  redde [] ∷ tabula<K, V>
}",
        "array_ascription_to_map",
    );
}

#[test]
fn generic_genus_field_array_ascription_to_generic_map_key_rejects() {
    // The norma `SortedMap<K, V>` shape: a generic class field default.
    test_support::assert_rejects_with_issue_la(
        r"genus SortedMap<K, V> {
  varia tabula<K, V> entries = [] ∷ tabula<K, V>
}",
        "array_ascription_to_map",
    );
}

#[test]
fn lista_source_ascription_to_generic_map_key_rejects() {
    // The rule keys on the source's container family, not on literal syntax.
    test_support::assert_rejects_with_issue_la(
        r"functio selecta<K, V>(lista<numerus> src) → tabula<K, V> {
  redde src ∷ tabula<K, V>
}",
        "array_ascription_to_map",
    );
}

#[test]
fn array_ascription_to_holed_map_key_rejects() {
    // `tabula<_, V>` leaves the key uninferred: same array-to-map family.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tabula<_, numerus> m ← [] ∷ tabula<_, numerus> }",
        "array_ascription_to_map",
    );
}

#[test]
fn index_keyed_array_ascription_to_map_rejects() {
    // The ruling's named shape: "even if technically it's a conversion that
    // should be allowed by treating the indexes as the key values, it's
    // still a conversion." Index-keyedness does not make the ascription
    // true.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tabula<numerus, numerus> m ← [] ∷ tabula<numerus, numerus> }",
        "array_ascription_to_map",
    );
}

#[test]
fn concrete_key_array_ascription_to_map_rejects() {
    // Totality is not about genericity: a concrete key is rejected too.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tabula<textus, numerus> m ← [] ∷ tabula<textus, numerus> }",
        "array_ascription_to_map",
    );
}

#[test]
fn fractus_key_array_ascription_to_map_rejects() {
    // Any concrete key family is the same rejection; the rule is not a
    // textus special case.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tabula<fractus, numerus> m ← [] ∷ tabula<fractus, numerus> }",
        "array_ascription_to_map",
    );
}

#[test]
fn sized_integer_key_array_ascription_to_map_rejects() {
    // A sized integer key is as index-keyed as bare numerus, and the ruling
    // rejects the index-keyed reading itself.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tabula<u32, numerus> m ← [] ∷ tabula<u32, numerus> }",
        "array_ascription_to_map",
    );
}

#[test]
fn shape_matching_array_ascription_to_tensor_rejects() {
    // The falsifier the second ruling names: rank and lengths agree, and it
    // is still a conversion — "it assumes that the internal structure of the
    // array matches with the internal structure of the tensor", which the
    // source does not establish (signed-to-unsigned cast analogy, operator
    // ruling 2026-09-21).
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tensor<fractus, [2, 2]> t ← [[1.0, 0.0], [0.0, 2.0]] ∷ tensor<fractus, [2, 2]> }",
        "array_ascription_to_tensor",
    );
}

#[test]
fn flat_array_ascription_to_tensor_rejects() {
    // Total over shapes: a flat, exact-length literal is the same
    // cross-container-kind conversion.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tensor<numerus, [3]> t ← [1, 2, 3] ∷ tensor<numerus, [3]> }",
        "array_ascription_to_tensor",
    );
}

#[test]
fn shape_mismatched_array_ascription_to_tensor_rejects() {
    // The rejection is unconditional: a disagreeing shape does not get a
    // gentler ruling than an agreeing one — both are conversions.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tensor<numerus, [8]> t ← [1, 2, 3] ∷ tensor<numerus, [8]> }",
        "array_ascription_to_tensor",
    );
}

#[test]
fn empty_array_ascription_to_tensor_rejects() {
    // The norma tensor.fab shape: `[]` is a definite array literal, so the
    // rule fires even with nothing to shape-match.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit tensor<fractus, [8]> t ← [] ∷ tensor<fractus, [8]> }",
        "array_ascription_to_tensor",
    );
}

#[test]
fn lista_source_ascription_to_tensor_rejects() {
    // As on the map side, the rule keys on the source's container family,
    // not on literal syntax: a typed lista identifier is a definite
    // cross-family source (bounded lista included).
    test_support::assert_rejects_with_issue_la(
        r"functio selecta(lista<fractus, magnitudo 4> src) → tensor<fractus, [4]> {
  redde src ∷ tensor<fractus, [4]>
}",
        "array_ascription_to_tensor",
    );
}

#[test]
fn unresolved_source_ascription_to_tensor_typechecks() {
    // The boundary that is not a loophole: an identifier or unresolved-type
    // source is exactly what ascription is for, so the rule must not fire.
    test_support::assert_accepts_la(
        r"functio asserta<T>(T raw) → tensor<fractus, [8]> {
  redde raw ∷ tensor<fractus, [8]>
}",
    );
}

#[test]
fn sanctioned_empty_and_keyed_map_construction_typechecks() {
    // `vacua` is the la-pack marker (`empty` is its en twin, covered by the
    // corpus); the keyed literal and the empty object literal stay the typed
    // construction paths.
    test_support::assert_accepts_la(
        r#"incipit {
  sit tabula<textus, numerus> a ← vacua
  sit tabula<textus, numerus> b ← { "alpha": 1 }
  sit tabula<textus, numerus> c ← {}
}"#,
    );
}

#[test]
fn array_ascription_to_lista_still_typechecks() {
    test_support::assert_accepts_la(
        "incipit { sit lista<numerus> xs ← [1, 2, 3] ∷ lista<numerus> }",
    );
}

#[test]
fn sync_to_promissum_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ p ← 42 ↦ promissum<numerus> }",
        "promissum_target_boundary",
    );
}

#[test]
fn promissum_to_sync_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"functio takesPromissum(promissum<numerus> p) → numerus {
  sit _ n ← p ↦ numerus
  redde n
}",
        "promissum_source_boundary",
    );
}

#[test]
fn copia_to_tabula_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  varia copia<numerus> s ← vacua\n  sit _ m ← s ↦ tabula<textus, numerus>\n}",
        "cross_collection_unsupported",
    );
}

struct CollectionMatrixCase {
    name: &'static str,
    source: &'static str,
    expect: MatrixExpect,
}

fn collection_conversio_matrix_cases() -> &'static [CollectionMatrixCase] {
    &[
        CollectionMatrixCase {
            name: "lista_to_copia",
            source: "incipit { sit _ s ← [1, 2] ↦ copia<numerus> }",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "copia_to_lista",
            source: "incipit {\n  sit copia<numerus> s ← [1, 2] ↦ copia<numerus>\n  sit _ xs ← s ↦ lista<numerus>\n}",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "tensor_to_lista",
            source: "incipit {\n  sit tensor<numerus, []> t ← vacua\n  sit _ xs ← t ↦ lista<numerus>\n}",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "lista_to_tensor",
            source: "incipit { sit _ t ← [1, 2, 3] ↦ tensor<numerus, [3]> }",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "cursor_to_lista",
            source: "incipit { sit _ xs ← [1, 2] ↦ cursor<numerus> ↦ lista<numerus> }",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "lista_to_cursor",
            source: "incipit { sit _ c ← [1, 2] ↦ cursor<numerus> }",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "tabula_to_lista_keys",
            source: r#"incipit {
  sit tabula<textus, numerus> m ← { "a": 1 }
  sit _ ks ← m ↦ lista<textus>
}"#,
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "tabula_to_lista_values",
            source: r#"incipit {
  sit tabula<textus, numerus> m ← { "a": 1 }
  sit _ vs ← m ↦ lista<numerus>
}"#,
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "lista_to_tabula",
            source: "incipit { sit _ m ← [1] ↦ tabula<textus, numerus> }",
            expect: MatrixExpect::RejectsIssue("lista_to_tabula_unsupported"),
        },
        CollectionMatrixCase {
            name: "sync_to_promissum",
            source: "incipit { sit _ p ← 1 ↦ promissum<numerus> }",
            expect: MatrixExpect::RejectsIssue("promissum_target_boundary"),
        },
        CollectionMatrixCase {
            name: "promissum_to_sync",
            source: r"functio takesPromissum(promissum<numerus> p) → numerus {
  sit _ n ← p ↦ numerus
  redde n
}",
            expect: MatrixExpect::RejectsIssue("promissum_source_boundary"),
        },
        CollectionMatrixCase {
            name: "copia_to_tabula",
            source: "incipit {\n  sit copia<numerus> s ← [1] ↦ copia<numerus>\n  sit _ m ← s ↦ tabula<textus, numerus>\n}",
            expect: MatrixExpect::RejectsIssue("cross_collection_unsupported"),
        },
        CollectionMatrixCase {
            name: "tabula_to_copia",
            source: r#"incipit {
  sit tabula<textus, numerus> m ← { "a": 1 }
  sit _ s ← m ↦ copia<numerus>
}"#,
            expect: MatrixExpect::RejectsIssue("cross_collection_unsupported"),
        },
        CollectionMatrixCase {
            name: "tensor_to_copia",
            source: "incipit {\n  sit tensor<numerus, []> t ← vacua\n  sit _ s ← t ↦ copia<numerus>\n}",
            expect: MatrixExpect::RejectsIssue("cross_collection_unsupported"),
        },
    ]
}

#[test]
fn tensor_numerus_to_fractus_conversio_typechecks() {
    test_support::assert_accepts_la(
        "incipit {\n  sit tensor<numerus, []> a ← vacua\n  sit _ b ← a ↦ tensor<fractus, []>\n}",
    );
}

#[test]
fn tensor_fractus_to_numerus_conversio_typechecks() {
    test_support::assert_accepts_la(
        "incipit {\n  sit tensor<fractus, []> b ← vacua\n  sit _ a ← b ↦ tensor<numerus, []>\n}",
    );
}

#[test]
fn tensor_to_scalar_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit tensor<numerus, []> t ← vacua\n  sit _ n ← t ↦ numerus\n}",
        "tensor_to_scalar_unsupported",
    );
}

#[test]
fn scalar_to_tensor_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit _ t ← 42 ↦ tensor<numerus, []>\n}",
        "scalar_to_tensor_unsupported",
    );
}

struct TensorMatrixCase {
    name: &'static str,
    source: &'static str,
    expect: MatrixExpect,
}

fn tensor_conversio_matrix_cases() -> &'static [TensorMatrixCase] {
    &[
        TensorMatrixCase {
            name: "lista_to_tensor",
            source: "incipit { sit _ t ← [1, 2, 3] ↦ tensor<numerus, [3]> }",
            expect: MatrixExpect::Accepts,
        },
        TensorMatrixCase {
            name: "tensor_to_lista",
            source: "incipit {\n  sit tensor<numerus, []> t ← vacua\n  sit _ xs ← t ↦ lista<numerus>\n}",
            expect: MatrixExpect::Accepts,
        },
        TensorMatrixCase {
            name: "tensor_numerus_to_fractus",
            source: "incipit {\n  sit tensor<numerus, []> a ← vacua\n  sit _ b ← a ↦ tensor<fractus, []>\n}",
            expect: MatrixExpect::Accepts,
        },
        TensorMatrixCase {
            name: "tensor_fractus_to_numerus",
            source: "incipit {\n  sit tensor<fractus, []> b ← vacua\n  sit _ a ← b ↦ tensor<numerus, []>\n}",
            expect: MatrixExpect::Accepts,
        },
        // Element-width lattice pairs: see `tensor_element_width_lattice_matrix_typecheck` (100 cells).
        TensorMatrixCase {
            name: "tensor_sized_to_scalar_rejects",
            source: "incipit {\n  sit tensor<i32, []> t ← vacua\n  sit _ n ← t ↦ i32\n}",
            expect: MatrixExpect::RejectsIssue("tensor_to_scalar_unsupported"),
        },
        TensorMatrixCase {
            name: "tensor_to_scalar",
            source: "incipit {\n  sit tensor<numerus, []> t ← vacua\n  sit _ n ← t ↦ numerus\n}",
            expect: MatrixExpect::RejectsIssue("tensor_to_scalar_unsupported"),
        },
        TensorMatrixCase {
            name: "scalar_to_tensor",
            source: "incipit { sit _ t ← 1 ↦ tensor<numerus, []> }",
            expect: MatrixExpect::RejectsIssue("scalar_to_tensor_unsupported"),
        },
    ]
}

#[test]
fn tensor_conversio_matrix_typecheck() {
    for case in tensor_conversio_matrix_cases() {
        let result = test_support::compile_and_get_result_la(case.source);
        match case.expect {
            MatrixExpect::Accepts => {
                assert!(result.is_ok(), "{}: {:?}", case.name, result.diagnostics);
            }
            MatrixExpect::RejectsIssue(issue) => {
                assert!(!result.is_ok(), "{}: {:?}", case.name, result.diagnostics);
                assert!(
                    result
                        .diagnostics
                        .iter()
                        .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
                    "{}: expected diagnostic issue {issue:?}, got: {:?}",
                    case.name,
                    result.diagnostics
                );
            }
        }
    }
}

#[test]
fn collection_conversio_matrix_typecheck() {
    for case in collection_conversio_matrix_cases() {
        let result = test_support::compile_and_get_result_la(case.source);
        match case.expect {
            MatrixExpect::Accepts => {
                assert!(result.is_ok(), "{}: {:?}", case.name, result.diagnostics);
            }
            MatrixExpect::RejectsIssue(issue) => {
                assert!(!result.is_ok(), "{}: {:?}", case.name, result.diagnostics);
                assert!(
                    result
                        .diagnostics
                        .iter()
                        .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
                    "{}: expected diagnostic issue {issue:?}, got: {:?}",
                    case.name,
                    result.diagnostics
                );
            }
        }
    }
}

#[test]
fn matrix_conversio_materialization_accepts_nested_lista_literal() {
    // Semantic-only: Rust host still rejects matrix codegen until a host
    // representation exists; materialize is admitted at typecheck/MIR.
    let result = crate::test_support::analyze_source_la(
        r"
functio identitas() → matrix<f32, [2, 2]> {
    redde [[1.0, 0.0], [0.0, 1.0]] ↦ matrix<f32, [2, 2]>
}
",
    );
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}",
        result.diagnostics
    );
}

#[test]
fn matrix_conversio_materialization_accepts_runtime_lista_source() {
    // S0-G9: a runtime `lista<T>` materializes into a literal matrix shape;
    // the element type is the static contract, the extent is runtime.
    test_support::assert_accepts_la(
        r"
functio cast(lista<f32> flat) → matrix<f32, [2, 2]> {
    redde flat ↦ matrix<f32, [2, 2]>
}
",
    );
}

#[test]
fn matrix_conversio_materialization_accepts_runtime_lista_parametric_shape() {
    // S0-G9: dimension-neutral construction — a parametric matrix shape has
    // no statically known extent, so a runtime lista source is the shape.
    test_support::assert_accepts_la(
        r"
functio cast<magnitudo R, magnitudo C>(lista<f32> flat) → matrix<f32, [R, C]> {
    redde flat ↦ matrix<f32, [R, C]>
}
",
    );
}

#[test]
fn matrix_conversio_materialization_rejects_nested_lista_element_mismatch() {
    // A nested `lista<lista<f32>>` source has row-list elements, not matrix
    // cells; the element mismatch rejects even though the source is a lista.
    test_support::assert_rejects_with_issue_la(
        r"
functio cast(lista<lista<f32>> cells) → matrix<f32, [2, 2]> {
    redde cells ↦ matrix<f32, [2, 2]>
}
",
        "lista_to_tensor_element_mismatch",
    );
}

#[test]
fn matrix_conversio_materialization_rejects_literal_source_parametric_shape() {
    // A literal source cannot be matched to unknown parametric dims.
    test_support::assert_rejects_with_issue_la(
        r"
functio cast<magnitudo R, magnitudo C>() → matrix<f32, [R, C]> {
    redde [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [R, C]>
}
",
        "matrix_materialize_unsupported",
    );
}

// ---- GSF-2: declared symbolic tensor-target pins ----

#[test]
fn symbolic_rank_two_lista_to_tensor_pin_typechecks() {
    // A declared `[T, D]` target is a known-shape pin, not an inference: the
    // generic gathers its result through `↦` at its own symbolic sizes.
    test_support::assert_accepts_la(
        r"
functio college<magnitudo V, magnitudo D, magnitudo T>(tensor<f32, [V, D]> mensa, lista<numerus> indices) → tensor<f32, [T, D]> ⇥ textus {
    redde (mensa ↦ lista<f32>) ↦ tensor<f32, [T, D]>
}
",
    );
}

#[test]
fn symbolic_rank_one_lista_to_tensor_pin_typechecks() {
    test_support::assert_accepts_la(
        r"
functio columna<magnitudo N>(lista<f32> plana) → tensor<f32, [N]> ⇥ textus {
    redde plana ↦ tensor<f32, [N]>
}
",
    );
}

#[test]
fn symbolic_lista_to_tensor_pin_rejects_element_mismatch() {
    test_support::assert_rejects_with_issue_la(
        r"
functio college<magnitudo T, magnitudo D>(lista<numerus> plana) → tensor<f32, [T, D]> {
    redde plana ↦ tensor<f32, [T, D]>
}
",
        "lista_to_tensor_element_mismatch",
    );
}

#[test]
fn literal_lista_to_symbolic_tensor_target_rejects() {
    // A rectangular literal cannot prove a parametric shape; the pin stays
    // reserved for flat sources whose count the runtime checks.
    test_support::assert_rejects_with_issue_la(
        r"
functio college<magnitudo T, magnitudo D>() → tensor<f32, [T, D]> {
    redde [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [T, D]>
}
",
        "rectangular_lista_literal_target",
    );
}

// ---- `sermo ↦ T` materialization conversio ----

#[test]
fn sermo_materialize_to_vacuum_typechecks() {
    test_support::assert_accepts_la(
        r#"incipit { sit _ v ← ad 'runtime:echo' ("salve") ↦ vacuum }"#,
    );
}

#[test]
fn sermo_materialize_to_textus_typechecks() {
    test_support::assert_accepts_la(
        r#"incipit { sit _ t ← ad 'runtime:echo' ("salve") ↦ textus }"#,
    );
}

#[test]
fn sermo_materialize_to_octeti_typechecks() {
    test_support::assert_accepts_la(
        r#"incipit { sit _ o ← ad 'runtime:echo' ("salve") ↦ lista<b8> }"#,
    );
}

#[test]
fn sermo_materialize_to_valor_typechecks() {
    test_support::assert_accepts_la(r#"incipit { sit _ v ← ad 'runtime:echo' ("salve") ↦ valor }"#);
}

#[test]
fn sermo_materialize_to_numerus_typechecks() {
    test_support::assert_accepts_la(r"incipit { sit _ n ← ad 'runtime:echo' (5) ↦ numerus }");
}

#[test]
fn sermo_materialize_to_bivalens_typechecks() {
    test_support::assert_accepts_la(r"incipit { sit _ b ← ad 'runtime:echo' (verum) ↦ bivalens }");
}

#[test]
fn sermo_materialize_to_lista_typechecks() {
    test_support::assert_accepts_la(
        r#"incipit { sit _ xs ← ad 'runtime:echo' ("salve") ↦ lista<textus> }"#,
    );
}

#[test]
fn sermo_materialize_to_type_param_typechecks() {
    test_support::assert_accepts_la(
        r#"
functio collige<T>(textus via) → T {
    redde ad 'runtime:echo' (via) ↦ T
}

incipit {
    sit textus corpus ← collige<textus>("README.md")
    nota corpus
}
"#,
    );
}

#[test]
fn sermo_materialize_rejects_tabula() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit { sit _ m ← ad 'runtime:echo' ("salve") ↦ tabula<textus, numerus> }"#,
        "sermo_materialize_target_unsupported",
    );
}

#[test]
fn sermo_materialize_rejects_tensor() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit { sit _ t ← ad 'runtime:echo' ("salve") ↦ tensor<numerus, [2]> }"#,
        "sermo_materialize_target_unsupported",
    );
}

#[test]
fn regex_literal_pattern_empty_accepts() {
    // Boundary: 0-byte pattern.
    test_support::assert_accepts_la(r#"incipit { sit _ r ← "" ↦ regex }"#);
}

#[test]
fn octeti_empty_literal_typechecks() {
    test_support::assert_accepts_la("incipit { sit _ b ← || ↦ lista<b8> }");
}

#[test]
fn sermo_materialize_rejects_copia() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit { sit _ s ← ad 'runtime:echo' ("salve") ↦ copia<textus> }"#,
        "sermo_materialize_target_unsupported",
    );
}

#[test]
fn sermo_materialization_in_textus_channel_is_rejected() {
    // A bare `sermo ↦ T` fails with the leaf record (`valor`), not text.
    test_support::assert_rejects_with_issue_la(
        r#"
functio materialize() → textus ⇥ textus {
    redde ad 'runtime:echo' ("salve") ↦ textus
}
"#,
        "conversio_failure_channel_value_required",
    );
}

#[test]
fn sermo_materialization_in_unknown_channel_checks() {
    test_support::assert_accepts_la(
        r#"
functio materialize() → textus ⇥ ignotum {
    redde ad 'runtime:echo' ("salve") ↦ textus
}
"#,
    );
}

#[test]
fn sermo_materialization_with_value_row_into_declared_error_type_checks() {
    test_support::assert_accepts_la(
        r#"
discretio Fault { Absens }

@ conversio
functio ex_valore(valor raw) → Fault {
    redde finge Absens {} ∷ Fault
}

functio materialize() → textus ⇥ Fault {
    redde ad 'runtime:echo' ("salve") ↦ textus
}
"#,
    );
}

// Conversion-directed assignment (`↤`)

#[test]
fn conversio_assign_accepts_mutable_local_place() {
    test_support::assert_accepts_la("incipit {\n  varia numerus count ← 0\n  count ↤ \"42\"\n}");
}

#[test]
fn conversio_assign_rejects_immutable_binding_after_init() {
    // A `fixum` local after its single initializer is not a valid place
    // (write-once); the definite-assignment `immutable_second_assignment`
    // issue is expected once the form routes through `Assign`.
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit numerus count ← 0\n  count ↤ \"42\"\n}",
        "immutable_second_assignment",
    );
}

#[test]
fn conversio_assign_rejects_inference_hole_destination() {
    // The destination type must be resolved before conversion checking;
    // `sit xs ← []` leaves an element inference hole that cannot name a
    // conversion target.
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit _ xs ← []\n  xs ↤ \"2\"\n}",
        "conversio_assign_inference_hole_destination",
    );
}

#[test]
fn conversio_assign_rejects_unsupported_route() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit json j ↤ \"x\"\n}",
        "unsupported_conversio",
    );
}

#[test]
fn conversio_assign_accepts_recovery_before_storage() {
    // `⊥ fallback` types before storage: the fallback satisfies the same
    // target-type rule as explicit `↦` recovery.
    test_support::assert_accepts_la("incipit {\n  varia numerus count ← 0\n  count ↤ \"x\" ⊥ 0\n}");
}

#[test]
fn conversio_assign_accepts_right_associated_chain() {
    // Inner converts to `b`'s type, then the inner result converts to `a`'s
    // type — both textus→numerus.
    test_support::assert_accepts_la(
        "incipit {\n  varia numerus a ← 0\n  varia numerus b ← 0\n  a ↤ b ↤ \"42\"\n}",
    );
}

#[test]
fn conversio_assign_accepts_field_place() {
    test_support::assert_accepts_la(
        r#"
genus Persona { varia textus nomen varia numerus aetas }
functio muta() → numerus ⇥ textus {
    varia Persona p ← Persona { nomen = "Ada", aetas = 36 }
    p.aetas ↤ "37"
    redde p.aetas
}
"#,
    );
}

#[test]
fn conversio_assign_accepts_indexed_place() {
    // Nontrivial place: an indexed element of a mutable list.
    test_support::assert_accepts_la(
        r#"
functio muta() → numerus ⇥ textus {
    varia lista<numerus> xs ← [1, 2]
    xs[0] ↤ "5"
    redde xs[0]
}
"#,
    );
}

#[test]
fn conversio_assign_typed_fixum_initializer_accepts() {
    // Typed declaration init: convert to the written type, then initialize
    // the binding. This is not post-declaration reassignment.
    test_support::assert_accepts_la("incipit {\n  sit numerus answer ↤ \"42\"\n}");
}

#[test]
fn conversio_assign_typed_fixum_item_scope_initializer_rejects_as_top_level_binding() {
    // D5.7: top-level `fixum`/`varia` is retired (SEM062 `top_level_binding`).
    // This used to prove item-scope typed `sit T x ↤ y` typechecked the
    // same conversion-directed initializer as the local form; that surface
    // no longer exists, so the same source must now be rejected outright,
    // and a conversion-directed initializer is not compile-time evaluable
    // (D5.9), so there is no `generis` static equivalent to redirect to.
    test_support::assert_rejects_with_issue_la(r#"sit numerus n ↤ "42""#, "top_level_binding");
}

#[test]
fn conversio_assign_untyped_fixum_initializer_rejects() {
    // `fixum _` has no written type → no concrete conversion target.
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit _ answer ↤ \"42\"\n}",
        "conversio_assign_requires_concrete_type",
    );
}

#[test]
fn conversio_assign_sit_initializer_rejects() {
    // `sit` is an inferred binding (≡ `fixum _`) → no concrete target.
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit _ answer ↤ \"42\"\n}",
        "conversio_assign_requires_concrete_type",
    );
}

#[test]
fn conversio_assign_untyped_destructuring_initializer_rejects() {
    // Untyped destructuring patterns have no concrete conversion target.
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  fixum { field } ↤ \"x\"\n}",
        "conversio_assign_requires_concrete_type",
    );
}

#[test]
fn conversio_assign_awaited_binding_initializer_rejects() {
    // `figendum`/`variandum` retain `←`; combining implicit conversion and
    // await ordering requires a separate language decision.
    for source in [
        "incipit {\n  figendum numerus n ↤ \"42\"\n}",
        "incipit {\n  variandum numerus n ↤ \"42\"\n}",
    ] {
        test_support::assert_rejects_with_issue_la(source, "conversio_assign_await_unsupported");
    }
}

#[test]
fn conversio_assign_recovery_typed_before_storage() {
    // `⊥ fallback` is typed against the resolved destination type through the
    // same target-type rule as explicit `↦` recovery — a wrong-typed fallback
    // rejects before any store.
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  varia numerus count ← 0\n  count ↤ \"x\" ⊥ \"fallback\"\n}",
        "conversio_recovery_type_mismatch",
    );
    test_support::assert_accepts_la("incipit {\n  varia numerus count ← 0\n  count ↤ \"x\" ⊥ 0\n}");
}

#[test]
fn conversio_assign_routes_through_live_tuus_cursor_escape_no_bypass() {
    // `↤` must not bypass the live-cursor escape pass. A live tuus cursor
    // destination reassigned via `↤` is the same `live_tuus_cursor_reassign`
    // error as ordinary `←`, and a live cursor source copied through `↤`
    // into a non-live path is the same `live_tuus_cursor_copy` error.
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit sermo s ← ad 'runtime:echo' (\"salve\")\n  sit tuus<textus> t ← s.tuus<textus>()\n  sit cursor<scrinium<textus>> c ← t.cursor()\n  c ↤ t.cursor()\n}",
        "live_tuus_cursor_reassign",
    );
    test_support::assert_rejects_with_issue_la(
        "incipit {\n  sit sermo s ← ad 'runtime:echo' (\"salve\")\n  sit tuus<textus> t ← s.tuus<textus>()\n  sit cursor<scrinium<textus>> c2\n  c2 ↤ t.cursor()\n}",
        "live_tuus_cursor_copy",
    );
}

#[test]
fn conversion_registry_duplicate_conflict_reports_dual_spans() {
    use crate::scope::{ConversionPair, ConversionRowConflict};
    use radix_lexer::Span;
    use radix_types::TypeId;

    let error = super::convert::conversion_registry_conflict_error(
        ConversionPair::new(TypeId(3), TypeId(9)),
        ConversionRowConflict::Duplicate {
            first_span: Span::new(4, 8),
            second_span: Span::new(40, 44),
        },
    );
    assert_eq!(error.issue(), Some("conversio_registry_duplicate_pair"));
    assert_eq!(error.span, Span::new(4, 44));
    assert!(error.args.iter().any(|arg| arg.name == "first_span"));
    assert!(error.args.iter().any(|arg| arg.name == "second_span"));
}

#[test]
fn conversion_registry_builtin_shadow_conflict_reports_dual_spans() {
    use crate::scope::{ConversionPair, ConversionRowConflict};
    use radix_lexer::Span;
    use radix_types::TypeId;

    let error = super::convert::conversion_registry_conflict_error(
        ConversionPair::new(TypeId(3), TypeId(9)),
        ConversionRowConflict::BuiltinShadow {
            builtin_span: Span::new(0, 0),
            user_span: Span::new(50, 54),
        },
    );
    assert_eq!(error.issue(), Some("conversio_registry_builtin_shadow"));
    assert_eq!(error.span, Span::new(0, 54));
    assert!(error.args.iter().any(|arg| arg.name == "first_span"));
    assert!(error.args.iter().any(|arg| arg.name == "second_span"));
}

// =============================================================================
// CTR-02: registered-row failure-channel composition
// =============================================================================
//
// The full `analyze_source_la` pipeline builds its own resolver with no enrolled
// rows (standalone-function enrollment lands with CTR-04), so the
// registry-backed contract below is proven by driving `TypeChecker` directly
// against a hand-seeded coherence registry. Under test (delivery CTR-02):
//
// - a row whose conversion function declares `⇥ E` composes that declared
//   channel through the function/use-site channel for explicit `↦` and
//   failable-call propagation;
// - inline `⊥` default stays target-typed, never channel-typed;
// - lookup is direct only: rows `A → B` and `B → C` never admit `A → C`;
// - local `fac/cape`, `iace` (hence `requirit`, which lowers to
//   `si non (cond) { iace err }`), and `↤` never consult the failable-call
//   seam, and a channel mismatch retains the SEM010-shaped
//   `failable_call_conversion_absent` diagnostic.

use super::TypeChecker;
use crate::scope::ConversionPair;
use crate::{FuncSig, ParamType, Primitive, Resolver, SemanticParamMode, TypeId, TypeTable};
use radix_hir::{DefId, HirConversioTarget, HirExpression, HirExpressionKind, HirId, HirLiteral};
use radix_lexer::{Interner, Span, Symbol};
use radix_target::Target;

/// Direct-checker harness with a hand-seeded coherence registry.
struct RowHarness {
    resolver: Resolver,
    types: TypeTable,
    interner: Interner,
}

impl RowHarness {
    fn new() -> Self {
        Self {
            resolver: Resolver::new(),
            types: TypeTable::new(),
            interner: Interner::new(),
        }
    }

    fn enroll_user(&mut self, source: TypeId, destination: TypeId, fn_id: DefId) {
        self.resolver
            .register_user_conversion(
                ConversionPair::new(source, destination),
                fn_id,
                Span::new(1, 2),
            )
            .expect("unique row enrollment");
    }

    fn checker(&mut self) -> TypeChecker<'_> {
        TypeChecker::new(
            &self.resolver,
            &mut self.types,
            Some(&self.interner),
            Target::HirRust,
        )
    }

    fn total_sig(&mut self) -> FuncSig {
        FuncSig {
            type_params: Vec::new(),
            params: Vec::new(),
            ret: self.types.primitive(Primitive::Vacuum),
            err: None,
            is_async: false,
            is_generator: false,
        }
    }

    fn conversion_fn_sig(&mut self, err: Option<TypeId>) -> FuncSig {
        FuncSig {
            type_params: Vec::new(),
            params: vec![ParamType {
                ty: self.types.primitive(Primitive::Vacuum),
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
            }],
            ret: self.types.primitive(Primitive::Vacuum),
            err,
            is_async: false,
            is_generator: false,
        }
    }
}

fn textus_literal_expr(id: u32) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::String(Symbol(7))),
        ty: None,
        span: crate::test_support::test_span(),
    }
}

fn int_literal_expr(id: u32, value: u64) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: None,
        span: crate::test_support::test_span(),
    }
}

fn bool_literal_expr(id: u32, value: bool) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Bool(value)),
        ty: None,
        span: crate::test_support::test_span(),
    }
}

fn has_issue(checker: &TypeChecker<'_>, issue: &str) -> bool {
    checker
        .errors
        .iter()
        .any(|diag| diag.is_error() && diag.issue() == Some(issue))
}

#[test]
fn failable_call_propagation_composes_declared_row_channel() {
    // A failable call throwing `textus` into a `⇥ numerus` function is
    // admitted by the direct row `textus → numerus` whose conversion function
    // declares `⇥ numerus` — the declared channel is assignable to the
    // use-site channel, and the row `DefId` is recorded as the conversion.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let row_fn = DefId(2100);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(numerus));
    let call_sig = h.total_sig();
    let call_sig = FuncSig {
        err: Some(textus),
        ..call_sig
    };

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Function(numerus));

    let admitted = checker.reject_failable_call(&call_sig, crate::test_support::test_span());
    assert!(!admitted, "registered row admits the failable call");
    assert_eq!(checker.pending_conversion, Some(row_fn));
    assert!(
        checker.errors.is_empty(),
        "no diagnostics expected: {:?}",
        checker.errors
    );
}

#[test]
fn failable_call_propagation_rejects_declared_channel_mismatch() {
    // The row's declared `⇥ bivalens` is not assignable to the use-site
    // `⇥ numerus` channel: the mismatch retains the SEM010-shaped
    // `failable_call_conversion_absent` diagnostic with the declared channel
    // as `source` and the use-site channel as `destination`.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let bivalens = h.types.primitive(Primitive::Bivalens);
    let row_fn = DefId(2101);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(bivalens));
    let mut call_sig = h.total_sig();
    call_sig.err = Some(textus);

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Function(numerus));

    let admitted = checker.reject_failable_call(&call_sig, crate::test_support::test_span());
    assert!(!admitted);
    assert_eq!(
        checker.pending_conversion, None,
        "a channel mismatch must not select the row"
    );
    let err = checker
        .errors
        .iter()
        .find(|diag| diag.issue() == Some("failable_call_conversion_absent"))
        .expect("SEM010-shaped conversion-absent diagnostic");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "source" && arg.value == "bivalens")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "destination" && arg.value == "numerus")
    );
}

#[test]
fn failable_call_propagation_requires_direct_row_only() {
    // Rows `textus → numerus` and `numerus → fractus` exist, but the call
    // throws `textus` into a `⇥ fractus` channel: direct-only lookup means
    // `textus → fractus` is a miss and the SEM010-shaped diagnostic fires.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let fractus = h.types.primitive(Primitive::Fractus);
    h.enroll_user(textus, numerus, DefId(2102));
    h.enroll_user(numerus, fractus, DefId(2103));
    let mut call_sig = h.total_sig();
    call_sig.err = Some(textus);

    let mut checker = h.checker();
    checker.current_error = Some(super::ErrorSink::Function(fractus));

    let admitted = checker.reject_failable_call(&call_sig, crate::test_support::test_span());
    assert!(!admitted);
    assert_eq!(checker.pending_conversion, None);
    let err = checker
        .errors
        .iter()
        .find(|diag| diag.issue() == Some("failable_call_conversion_absent"))
        .expect("SEM010-shaped conversion-absent diagnostic");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "source" && arg.value == "textus")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "destination" && arg.value == "fractus")
    );
}

#[test]
fn local_handler_keeps_original_error_with_registered_row_present() {
    // A `fac/cape` local handler typed as the source error admits the call
    // directly; the registered row for `textus → numerus` with a declared
    // channel is never consulted.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let bivalens = h.types.primitive(Primitive::Bivalens);
    let row_fn = DefId(2104);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(bivalens));
    let mut call_sig = h.total_sig();
    call_sig.err = Some(textus);

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Local(textus));

    let admitted = checker.reject_failable_call(&call_sig, crate::test_support::test_span());
    assert!(!admitted, "local handler admits the call");
    assert_eq!(
        checker.pending_conversion, None,
        "the local handler must not select the registry row"
    );
    assert!(
        checker.errors.is_empty(),
        "original-error handler type needs no conversion: {:?}",
        checker.errors
    );
}

#[test]
fn local_handler_mismatch_keeps_handler_issue_not_registry_seam() {
    // A mismatched local handler reports the handler issue; the registered
    // row for `textus → numerus` does not turn the mismatch into a
    // failable-call conversion lookup.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let bivalens = h.types.primitive(Primitive::Bivalens);
    let row_fn = DefId(2105);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(bivalens));
    let mut call_sig = h.total_sig();
    call_sig.err = Some(textus);

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Local(numerus));

    let admitted = checker.reject_failable_call(&call_sig, crate::test_support::test_span());
    assert!(!admitted);
    assert_eq!(checker.pending_conversion, None);
    assert!(has_issue(
        &checker,
        "failable_call_handler_error_type_mismatch"
    ));
    assert!(
        !has_issue(&checker, "failable_call_conversion_absent"),
        "the local sink must not consult the failable-call seam"
    );
}

#[test]
fn iace_throw_never_consults_registry_seam() {
    // `iace` (and `requirit`, which lowers to `si non (cond) { iace err }`)
    // throws the payload straight into the channel: with a registered row
    // `textus → numerus` present, throwing a `textus` value into a
    // `⇥ numerus` channel still reports `iace_value_type_mismatch`, never
    // `failable_call_conversion_absent`.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let bivalens = h.types.primitive(Primitive::Bivalens);
    let row_fn = DefId(2106);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(bivalens));

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Function(numerus));

    let thrown = textus_literal_expr(1);
    let mut throw_expr = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Throw(Box::new(thrown)),
        ty: None,
        span: crate::test_support::test_span(),
    };
    let result = checker.check_expr(&mut throw_expr);
    assert_eq!(result, checker.vacuum_type());
    assert!(has_issue(&checker, "iace_value_type_mismatch"));
    assert!(
        !has_issue(&checker, "failable_call_conversion_absent"),
        "iace must not use the failable-call seam: {:?}",
        checker.errors
    );
}

#[test]
fn bare_arrow_through_registered_row_composes_declared_channel() {
    // A bare `textus ↦ numerus` through the row admits the pair and composes
    // the row's declared `⇥ bivalens` against the use-site `⇥ numerus`
    // channel: the mismatch retains the SEM010 shape.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let bivalens = h.types.primitive(Primitive::Bivalens);
    let row_fn = DefId(2107);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(bivalens));

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Function(numerus));

    let mut source = textus_literal_expr(1);
    let mut target = HirConversioTarget::Type(numerus);
    let result = checker.check_conversio(
        &mut source,
        &mut target,
        &[],
        None,
        &mut None,
        crate::test_support::test_span(),
    );
    assert_eq!(result, numerus);
    let err = checker
        .errors
        .iter()
        .find(|diag| diag.issue() == Some("failable_call_conversion_absent"))
        .expect("SEM010-shaped conversion-absent diagnostic");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "source" && arg.value == "bivalens")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "destination" && arg.value == "numerus")
    );
}

#[test]
fn bare_arrow_through_registered_row_accepts_assignable_declared_channel() {
    // The same bare conversion is admitted without diagnostics when the
    // declared channel (`⇥ numerus`) is assignable to the use-site channel.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let row_fn = DefId(2108);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(numerus));

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Function(numerus));

    let mut source = textus_literal_expr(1);
    let mut target = HirConversioTarget::Type(numerus);
    let result = checker.check_conversio(
        &mut source,
        &mut target,
        &[],
        None,
        &mut None,
        crate::test_support::test_span(),
    );
    assert_eq!(result, numerus);
    assert!(
        checker.errors.is_empty(),
        "declared channel assignable to the use-site channel: {:?}",
        checker.errors
    );
}

#[test]
fn inline_recovery_stays_target_typed() {
    // Inline `⊥` default is typed against the conversion target, not the
    // row's declared channel: a `numerus` recovery is accepted even though
    // the row declares `⇥ bivalens`, and a `bivalens` recovery rejects with
    // `conversio_recovery_type_mismatch`.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let bivalens = h.types.primitive(Primitive::Bivalens);
    let row_fn = DefId(2109);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(bivalens));

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Function(numerus));

    // Target-typed recovery: `⊥ 0` (numerus) admits, despite the declared channel.
    let mut source = textus_literal_expr(1);
    let mut target = HirConversioTarget::Type(numerus);
    let mut recovery = int_literal_expr(2, 0);
    let result = checker.check_conversio(
        &mut source,
        &mut target,
        &[],
        Some(&mut recovery),
        &mut None,
        crate::test_support::test_span(),
    );
    assert_eq!(result, numerus);
    assert!(
        checker.errors.is_empty(),
        "target-typed recovery: {:?}",
        checker.errors
    );

    // Channel-typed recovery would be the wrong seam: `⊥ verum` (bivalens) is
    // rejected against the numerus target.
    let mut source = textus_literal_expr(3);
    let mut target = HirConversioTarget::Type(numerus);
    let mut recovery = bool_literal_expr(4, true);
    checker.check_conversio(
        &mut source,
        &mut target,
        &[],
        Some(&mut recovery),
        &mut None,
        crate::test_support::test_span(),
    );
    assert!(has_issue(&checker, "conversio_recovery_type_mismatch"));
    assert!(
        !has_issue(&checker, "failable_call_conversion_absent"),
        "recovery stays on the conversion seam: {:?}",
        checker.errors
    );
}

#[test]
fn conversio_assign_composes_declared_channel_through_conversion_seam() {
    // `↤` routes through `check_conversio` (the conversion seam): a
    // `numerus` local receiving a `textus` value is admitted by the direct
    // row whose declared channel matches the use-site channel.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let row_fn = DefId(2110);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(numerus));

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Function(numerus));
    checker.push_scope();
    checker.insert_binding(DefId(500), numerus);

    let mut target = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Path(DefId(500)),
        ty: None,
        span: crate::test_support::test_span(),
    };
    let mut source = textus_literal_expr(2);
    let result = checker.check_conversio_assign(
        &mut target,
        &mut source,
        None,
        &[],
        crate::test_support::test_span(),
    );
    assert_eq!(result, numerus);
    assert!(
        checker.errors.is_empty(),
        "↤ admits through the conversion seam: {:?}",
        checker.errors
    );
}

#[test]
fn conversio_assign_channel_mismatch_keeps_sem010_shape() {
    // A `↤` whose row declares `⇥ bivalens` against a `⇥ numerus` use-site
    // channel reports the SEM010-shaped mismatch — the conversion seam, not a
    // different failable-call diagnostic.
    let mut h = RowHarness::new();
    let textus = h.types.primitive(Primitive::Textus);
    let numerus = h.types.primitive(Primitive::Numerus);
    let bivalens = h.types.primitive(Primitive::Bivalens);
    let row_fn = DefId(2111);
    h.enroll_user(textus, numerus, row_fn);
    let row_sig = h.conversion_fn_sig(Some(bivalens));

    let mut checker = h.checker();
    checker.functions.insert(row_fn, row_sig);
    checker.current_error = Some(super::ErrorSink::Function(numerus));
    checker.push_scope();
    checker.insert_binding(DefId(501), numerus);

    let mut target = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Path(DefId(501)),
        ty: None,
        span: crate::test_support::test_span(),
    };
    let mut source = textus_literal_expr(2);
    checker.check_conversio_assign(
        &mut target,
        &mut source,
        None,
        &[],
        crate::test_support::test_span(),
    );
    let err = checker
        .errors
        .iter()
        .find(|diag| diag.issue() == Some("failable_call_conversion_absent"))
        .expect("SEM010-shaped conversion-absent diagnostic");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "source" && arg.value == "bivalens")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "destination" && arg.value == "numerus")
    );
}

#[test]
fn verte_ascription_rejects_runtime_narrowing_into_sized_numerus() {
    // DECISIONS D1.11 probe: `n ∷ int<u8>` on a run-time `n` compiled and
    // printed 300 — a u8 holding 300. F3: a value change is a `↦`
    // conversion, never a `∷` fact.
    test_support::assert_rejects_with_issue_la(
        r"
functio probe(numerus n) → u8 {
    redde n ∷ u8
}
",
        "numeric_ascription_requires_conversion",
    );
}

#[test]
fn verte_ascription_rejects_literal_over_target_width() {
    // D1.11: a literal the target cannot hold (`300 ∷ int<u8>`) is a
    // compile error, not a silent cast.
    test_support::assert_rejects_with_issue_la(
        r"incipit { sit u8 b ← 300 ∷ u8 }",
        "numeric_ascription_requires_conversion",
    );
}

#[test]
fn verte_ascription_rejects_float_precision_loss() {
    test_support::assert_rejects_with_issue_la(
        r"
functio narrow(fractus x) → f32 {
    redde x ∷ f32
}
",
        "numeric_ascription_requires_conversion",
    );
}

#[test]
fn verte_ascription_rejects_int_float_crossing() {
    test_support::assert_rejects_with_issue_la(
        r"
functio relabel(numerus n) → fractus {
    redde n ∷ fractus
}
",
        "numeric_ascription_requires_conversion",
    );
}

#[test]
fn verte_ascription_rejects_runtime_float_into_integer() {
    // D1.11 (fi4): a run-time float cannot be proven to be an integer in
    // range; the conversion is `x ↦ numerus`, which fails or takes `⊥`.
    test_support::assert_rejects_with_issue_la(
        r"
functio truncate(fractus x) → numerus {
    redde x ∷ numerus
}
",
        "numeric_ascription_requires_conversion",
    );
    test_support::assert_rejects_with_issue_la(
        r"
functio truncate(fractus x) → u8 {
    redde x ∷ u8
}
",
        "numeric_ascription_requires_conversion",
    );
}

#[test]
fn verte_ascription_rejects_float_literal_into_integer() {
    // Ruling 20's constant rule: a float literal states a float, even when
    // it is an exact integer; `3.0 ∷ numerus` is a conversion.
    test_support::assert_rejects_with_issue_la(
        r"incipit { sit numerus n ← 3.0 ∷ numerus }",
        "numeric_ascription_requires_conversion",
    );
}

#[test]
fn verte_ascription_accepts_float_literal_rounding_to_target_width() {
    test_support::assert_accepts_la(
        r"incipit {
    sit f32 rounded ← 0.1 ∷ f32
    sit f32 precise ← 0.7978845608028654 ∷ f32
}",
    );
}

#[test]
fn verte_ascription_rejects_float_literal_overflow_to_infinity() {
    test_support::assert_rejects_with_issue_la(
        r"incipit { sit f32 overflow ← 1e39 ∷ f32 }",
        "numeric_ascription_requires_conversion",
    );
}

#[test]
fn verte_ascription_keeps_provable_numeric_facts() {
    // The F3 fact cases: an in-range literal, i32→i64, f32→f64, and a
    // float literal the target width holds exactly.
    test_support::assert_accepts_la(r"incipit { sit u8 port ← 42 ∷ u8 }");
    test_support::assert_accepts_la(
        r"
functio widen(i32 n) → numerus {
    redde n ∷ numerus
}
",
    );
    test_support::assert_accepts_la(
        r"
functio widen_fractus(f32 x) → fractus {
    redde x ∷ fractus
}
",
    );
    test_support::assert_accepts_la(r"incipit { sit f32 two ← 2.5 ∷ f32 }");
}
#[test]
fn verte_ascription_keeps_modular_widening() {
    test_support::assert_accepts_la(
        r"
functio probe(modulus<u8> m) → modulus<u32> {
    redde m ∷ modulus<u32>
}
",
    );
}

// D1.11: two nominal genus types with no declared relation are not a provable
// `∷` fact — the ascription is a conversion (`↦`), and the diagnostic names
// both sides.

#[test]
fn verte_ascription_rejects_unrelated_genus_pair() {
    test_support::assert_rejects_with_args_la(
        r"
genus P { varia numerus x }

genus Q { varia numerus y }

functio probe(P p) → Q {
    redde p ∷ Q
}
",
        &[
            radix_diagnostics::DiagnosticArg::new("issue", "nominal_ascription_not_provable"),
            radix_diagnostics::DiagnosticArg::new("found", "P"),
            radix_diagnostics::DiagnosticArg::new("expected", "Q"),
        ],
    );
}

#[test]
fn verte_ascription_rejects_foreign_interface() {
    test_support::assert_rejects_with_issue_la(
        r"
implendum Nominatum {
    functio plenum() → textus
}

genus Civis {
    varia textus nomen
}

functio probe(Civis civis) → Nominatum {
    redde civis ∷ Nominatum
}
",
        "nominal_ascription_not_provable",
    );
}

#[test]
fn verte_ascription_accepts_same_genus() {
    test_support::assert_accepts_la(
        r"
genus P { varia numerus x }

functio probe(P p) → P {
    redde p ∷ P
}
",
    );
}

// A generic constructor-like call can leave its result carrying an inference
// hole (`mk(1)` is `Box<?>` until something constrains `?`). Ascribing the
// result to `Box<numerus>` states the missing witness; it is a refinement of
// the same genus, not an unprovable nominal pair.

#[test]
fn verte_ascription_accepts_inferred_generic_result() {
    test_support::assert_accepts_la(
        r"
genus Box<T> {
    varia T v
}

functio mk<T>(T x) → Box<T> {
    redde Box<T> { v = x }
}

incipit {
    sit Box<numerus> b ← mk(1) ∷ Box<numerus>
    nota b
}
",
    );
}

// The no-argument form leaves the witness entirely to the ascription:
// `mk()` has no argument to solve `T`, so `∷ Box<numerus>` is the only
// statement of the fact.

#[test]
fn verte_ascription_accepts_inferred_generic_result_no_argument() {
    test_support::assert_accepts_la(
        r"
genus Box<T> { }

functio mk<T>() → Box<T> {
    redde Box<T> { }
}

incipit {
    sit Box<numerus> b ← mk() ∷ Box<numerus>
    nota b
}
",
    );
}

#[test]
fn verte_ascription_accepts_narrowed_variant() {
    test_support::assert_accepts_la(
        r"
discretio Forma {
    Circulus { numerus r },
    Quadratum { numerus s }
}

functio probe(Forma f) → Forma {
    discerne f {
        casu Circulus ut c { redde c ∷ Forma }
        casu _ { redde f }
    }
}
",
    );
}

// A variant narrowed by `est` is the variant's own nominal
// (`Type::Struct(variant_def)`), while its parent is the `discretio`
// (`Type::Enum(enum_def)`). Ascribing the narrowed binding back to the parent
// restates the membership the test already proved; it is not an unprovable
// nominal pair (D1.11).

#[test]
fn verte_ascription_accepts_narrowed_variant_est_refinement() {
    test_support::assert_accepts_la(
        r"
discretio Forma {
    Circulus { numerus r },
    Quadratum { numerus s }
}

functio probe(Forma f) → Forma {
    adfirma f est Circulus
    redde f ∷ Forma
}
",
    );
}

#[test]
fn verte_ascription_accepts_declared_interface_conformance() {
    test_support::assert_accepts_la(
        r#"
implendum Nominatum {
    functio plenum() → textus
}

genus Civis implet Nominatum {
    varia textus nomen

    functio plenum() → textus { redde "§"(ego.nomen) }
}

incipit {
    sit Civis civis ← Civis { nomen = "Aurelia" }
    sit Nominatum boxed ← civis ∷ Nominatum
    sit Nominatum built ← Civis { nomen = "Iulia" } ∷ Nominatum
    nota boxed.plenum()
    nota built.plenum()
}
"#,
    );
}

#[test]
fn verte_ascription_accepts_alias_to_same_genus() {
    test_support::assert_accepts_la(
        r"
genus Civis { varia textus nomen }

typus Cives = Civis

functio probe(Civis civis) → Cives {
    redde civis ∷ Cives
}
",
    );
}

// D11.6 ruling 24: `↦` into `wrapping`/`saturating` applies the policy and
// cannot fail — no `⊥`, no failure channel, integer and `d64` sources only.

#[test]
fn policy_word_conversio_needs_no_recovery_or_channel() {
    test_support::assert_accepts_la(
        r"
functio narrow(numerus x, d64 d) → modulus<u8> {
    sit modulus<u8> a ← x ↦ modulus<u8>
    sit saturatus<u8> b ← x ↦ saturatus<u8>
    sit modulus<u8> c ← b ↦ modulus<u8>
    sit saturatus<i8> e ← a ↦ saturatus<i8>
    sit modulus<u8> f ← d ↦ modulus<u8> via HalfEven
    sit saturatus<u8> g ← d ↦ saturatus<u8> via HalfEven
    sit modulus<u32> h ← 300 ↦ modulus<u32>
    redde a
}
",
    );
}

#[test]
fn policy_word_conversio_rejects_recovery_slot() {
    for target in ["modulus<u8>", "saturatus<u8>"] {
        test_support::assert_rejects_with_issue_la(
            &format!("incipit {{ sit numerus x ← 300\nsit _ b ← x ↦ {target} ⊥ 0 }}"),
            "word_conversio_recovery_unsupported",
        );
    }
}

#[test]
fn policy_word_conversio_rejects_float_and_text_sources() {
    // Need: a float source names its rounding; text stays unsupported.
    for (source, target, issue) in [
        ("1.5", "modulus<u8>", "float_to_integer_requires_rounding"),
        ("1.5", "saturatus<u8>", "float_to_integer_requires_rounding"),
        ("\"7\"", "modulus<u8>", "unsupported_conversio"),
        ("\"7\"", "saturatus<u8>", "unsupported_conversio"),
    ] {
        test_support::assert_rejects_with_issue_la(
            &format!("incipit {{ sit _ b ← {source} ↦ {target} }}"),
            issue,
        );
    }
}

#[test]
fn policy_word_conversio_accepts_a_rounded_float_source() {
    for target in ["modulus<u8>", "saturatus<u8>"] {
        test_support::assert_accepts_la(&format!(
            "incipit {{ sit _ b ← 1.5 ↦ {target} via Floor }}"
        ));
    }
}

#[test]
fn via_lowercase_policy_words_accepted() {
    for source in [
        r#"incipit { sit _ n ← "ff" ↦ u32 via hex }"#,
        "functio f(lista<b8> corpus) → f32 { redde corpus[0‥4] ↦ f32 via le }",
    ] {
        test_support::assert_accepts_la(source);
    }
    for target in ["modulus<u8>", "saturatus<u8>"] {
        test_support::assert_accepts_la(&format!(
            "incipit {{ sit _ b ← 1.5 ↦ {target} via floor }}"
        ));
        test_support::assert_accepts_la(&format!(
            "incipit {{ sit _ c ← 1.5 ↦ {target} via half_even }}"
        ));
    }
}

#[test]
fn numeric_narrowing_conversio_requires_textus_alternate_exit() {
    test_support::assert_rejects_with_issue_la(
        r"
functio narrow(numerus n) → u8 ⇥ numerus {
    redde n ↦ u8
}
",
        "conversio_failure_channel_textus_required",
    );
}

#[test]
fn float_to_integer_conversio_requires_textus_alternate_exit() {
    test_support::assert_rejects_with_issue_la(
        r"
functio trunc(fractus x) → numerus ⇥ numerus {
    redde x ↦ numerus via Trunc
}
",
        "conversio_failure_channel_textus_required",
    );
}

#[test]
fn numeric_narrowing_conversio_accepts_textus_alternate_exit() {
    test_support::assert_accepts_la(
        r"
functio narrow(numerus n) → u8 ⇥ textus {
    redde n ↦ u8
}
",
    );
}

#[test]
fn numeric_narrowing_conversio_with_recovery_accepts_non_textus_exit() {
    test_support::assert_accepts_la(
        r"
functio narrow(numerus n) → u8 ⇥ numerus {
    redde n ↦ u8 ⊥ 0
}
",
    );
}

#[test]
fn wrapping_conversio_accepts_non_textus_alternate_exit() {
    test_support::assert_accepts_la(
        r"
functio wrap(numerus n) → modulus<u8> ⇥ numerus {
    redde n ↦ modulus<u8>
}
",
    );
}

#[test]
fn numeric_widening_conversio_accepts_non_textus_alternate_exit() {
    test_support::assert_accepts_la(
        r"
functio widen(u8 n) → numerus ⇥ numerus {
    redde n ↦ numerus
}
",
    );
}

#[test]
fn constant_fitting_conversio_accepts_non_textus_alternate_exit() {
    test_support::assert_accepts_la(
        r"
functio konst() → u8 ⇥ numerus {
    redde 200 ↦ u8
}
",
    );
}

#[test]
fn empty_list_conversio_to_valor_requires_element_annotation() {
    let source = r#"incipit {
    varia tabula<textus, valor> raw ← {}
    raw.pone("headers", [] ↦ valor)
}"#;
    let analyzed = test_support::analyze_source_la(source);
    let errors: Vec<_> = analyzed
        .diagnostics
        .iter()
        .filter(|error| error.issue() == Some("cannot_infer_empty_list_element_type"))
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", analyzed.diagnostics);
    assert_eq!(
        errors[0].kind,
        crate::SemanticErrorKind::TypeAnnotationAbsent
    );
    assert_eq!(
        &source[errors[0].span.start as usize..errors[0].span.end as usize],
        "[]"
    );
}

#[test]
fn empty_list_conversio_typed_source_accepts() {
    test_support::assert_accepts_la(
        r"incipit {
    sit lista<valor> headers ← vacua
    sit valor packed ← headers ↦ valor
    nota packed
}",
    );
}

#[test]
fn empty_list_conversio_element_ascription_accepts() {
    test_support::assert_accepts_la(
        r"incipit {
    sit valor packed ← ([] ∷ lista<textus>) ↦ valor
    nota packed
}",
    );
}

#[test]
fn empty_list_conversio_later_element_constraint_accepts() {
    test_support::assert_accepts_la(
        r"incipit {
    varia lista<numerus> headers ← []
    headers.appende(1)
    sit valor packed ← headers ↦ valor
    nota packed
}",
    );
}

#[test]
fn empty_list_in_a_hole_cell_is_a_named_error_even_with_a_later_append() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
    varia _ headers ← []
    headers.appende(1)
    sit valor packed ← headers ↦ valor
    nota packed
}",
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn empty_list_conversio_nonempty_source_accepts() {
    test_support::assert_accepts_la(
        r#"incipit {
    sit valor packed ← ["content-type"] ↦ valor
    nota packed
}"#,
    );
}

// Ruling 16: `tensor ↔ vector` and `tensor ↔ matrix` are family crossings, so
// they always need the explicit `↦` — and the crossing is exact (equal element
// type, equal static extents).

#[test]
fn tensor_family_conversio_tensor_to_vector_accepts() {
    test_support::assert_accepts_la(
        r"incipit {
  sit tf32[3] t ← vacua
  sit vector<f32, magnitudo 3> v ← t ↦ vector<f32, magnitudo 3>
  nota v
}",
    );
}

#[test]
fn tensor_family_conversio_vector_to_tensor_accepts() {
    test_support::assert_accepts_la(
        r"incipit {
  sit vector<f32, magnitudo 3> v ← [1.0, 2.0, 3.0] ↦ vector<f32, magnitudo 3>
  sit tf32[3] t ← v ↦ tf32[3]
  nota t
}",
    );
}

#[test]
fn tensor_family_conversio_tensor_to_matrix_accepts() {
    test_support::assert_accepts_la(
        r"incipit {
  sit tf32[2, 3] t ← vacua
  sit matrix<f32, [2, 3]> m ← t ↦ matrix<f32, [2, 3]>
  nota m
}",
    );
}

#[test]
fn tensor_family_conversio_matrix_to_tensor_accepts() {
    test_support::assert_accepts_la(
        r"incipit {
  sit matrix<f32, [2, 3]> m ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ matrix<f32, [2, 3]>
  sit tf32[2, 3] t ← m ↦ tf32[2, 3]
  nota t
}",
    );
}

#[test]
fn tensor_family_conversio_width_mismatch_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
  sit tf32[4] t ← vacua
  sit vector<f32, magnitudo 3> v ← t ↦ vector<f32, magnitudo 3>
  nota v
}",
        "tensor_shape_mismatch",
    );
}

#[test]
fn tensor_family_conversio_element_mismatch_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
  sit tu32[3] t ← vacua
  sit vector<f32, magnitudo 3> v ← t ↦ vector<f32, magnitudo 3>
  nota v
}",
        "tensor_element_mismatch",
    );
}
