//! Regression coverage for typecheck-level `↦` conversio and `↤` assignment.
//!
//! These tests lock the conversio matrix and conversion-directed assignment
//! contracts without depending on codegen emission.

use crate::passes::typecheck::test_support;

#[test]
fn conversio_rejects_struct_to_numerus() {
    test_support::assert_rejects_with_issue(
        r"
genus Foo { numerus n }

functio bad(Foo f) → numerus {
    redde f ↦ numerus
}
",
        "unsupported_conversio",
    );
}

#[test]
fn textus_to_regex_conversio_typechecks() {
    test_support::assert_accepts(r#"incipit { fixum _ r ← "\d+" ↦ regex }"#);
}

#[test]
fn ascii_to_regex_conversio_typechecks_via_textus_widening() {
    test_support::assert_accepts(r"incipit { fixum _ r ← '(?g)\d+' ↦ regex }");
}

#[test]
fn regex_conversio_rejects_recovery_slot() {
    test_support::assert_rejects_with_issue(
        r#"incipit { fixum _ r ← "\d[" ↦ regex ⇥ "\d+" }"#,
        "regex_recovery_unsupported",
    );
}

#[test]
fn regex_literal_pattern_at_cap_typechecks() {
    // REGEX_PATTERN_MAX_BYTES = 1024; exactly at the cap is accepted.
    let pattern = "a".repeat(1024);
    let source = format!(r#"incipit {{ fixum _ r ← "{pattern}" ↦ regex }}"#);
    test_support::assert_accepts(&source);
}

#[test]
fn regex_literal_pattern_over_cap_rejects() {
    let pattern = "a".repeat(1025);
    let source = format!(r#"incipit {{ fixum _ r ← "{pattern}" ↦ regex }}"#);
    test_support::assert_rejects_with_issue(&source, "regex_pattern_too_long");
}

#[test]
fn regex_conversio_dynamic_source_unaffected_by_cap() {
    // The cap applies to folded literals only; dynamic textus sources carry no
    // compile-time length and keep flowing.
    test_support::assert_accepts(r"functio f(textus pat) → regex { redde pat ↦ regex }");
}

#[test]
fn conversio_recovery_type_mismatch_rejects() {
    test_support::assert_rejects_with_issue(
        r#"incipit { fixum _ n ← "x" ↦ numerus ⇥ "string" }"#,
        "conversio_recovery_type_mismatch",
    );
}

#[test]
fn intervallum_clamp_operand_type_mismatch_rejects_with_structured_issue_arg() {
    test_support::assert_rejects_with_issue(
        r#"incipit { fixum _ clamped ← "x" ↦ 0‥10 }"#,
        "intervallum_clamp_operand_type_mismatch",
    );
}

#[test]
fn bare_fallible_conversio_requires_textus_alternate_exit() {
    test_support::assert_rejects_with_issue(
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
    test_support::assert_accepts(
        r"
functio parse(valor v, instans epoch) → instans ⇥ numerus {
    redde v ↦ instans ⇥ epoch
}
",
    );
}

#[test]
fn valor_to_sized_numerus_direct_extraction_typechecks() {
    test_support::assert_accepts(
        r"
functio parse(valor v) → numerus<u8> {
    redde v ↦ numerus<u8>
}
",
    );
}

#[test]
fn valor_to_sized_fractus_direct_extraction_rejects_until_width_policy_exists() {
    test_support::assert_rejects_with_issue(
        r"
functio parse(valor v) → fractus<f32> {
    redde v ↦ fractus<f32>
}
",
        "valor_to_sized_numeric_unsupported",
    );
}

#[test]
fn valor_to_sized_numeric_lista_extraction_rejects_until_runtime_trait_exists() {
    test_support::assert_rejects_with_issue(
        r"
incipit {
    fixum valor v ← [1, 2]
    fixum lista<numerus<u8>> xs ← v ↦ lista<numerus<u8>>
}
",
        "valor_to_sized_numeric_unsupported",
    );
}

#[test]
fn valor_to_genus_sized_fractus_field_rejects_until_width_policy_exists() {
    test_support::assert_rejects_with_issue(
        r#"
genus Sample {
    fractus<f32> ratio
}

incipit {
    fixum valor v ← { "ratio": 1.5 } ↦ valor
    fixum Sample s ← v ↦ Sample
}
"#,
        "valor_to_sized_numeric_unsupported",
    );
}

#[test]
fn infallible_conversio_allows_non_textus_alternate_exit() {
    test_support::assert_accepts(
        r"
functio widen(numerus n) → fractus ⇥ numerus {
    redde n ↦ fractus
}
",
    );
}

#[test]
fn truthiness_conversio_allows_non_textus_alternate_exit() {
    test_support::assert_accepts(
        r"
functio truthy(textus s) → bivalens ⇥ numerus {
    redde s ↦ bivalens
}
",
    );
}

#[test]
fn truthiness_conversio_rejects_recovery_slot() {
    for source in [
        "0 ↦ bivalens ⇥ verum",
        "0.0 ↦ bivalens ⇥ verum",
        "\"\" ↦ bivalens ⇥ verum",
        "'no' ↦ bivalens ⇥ verum",
    ] {
        test_support::assert_rejects_with_issue(
            &format!("incipit {{ fixum _ b ← {source} }}"),
            "bivalens_recovery_unsupported",
        );
    }
}

#[test]
fn textus_to_octeti_conversio_typechecks() {
    test_support::assert_accepts(r#"incipit { fixum _ b ← "hi" ↦ octeti }"#);
}

#[test]
fn octeti_to_textus_conversio_typechecks() {
    test_support::assert_accepts(r"incipit { fixum _ s ← |68 69| ↦ textus }");
}

#[test]
fn ascii_octeti_conversio_roundtrip_typechecks() {
    test_support::assert_accepts(
        "incipit {\n  fixum _ b ← 'hi' ↦ octeti\n  fixum _ a ← b ↦ ascii\n}",
    );
}

#[test]
fn octeti_textus_recovery_typechecks() {
    test_support::assert_accepts(r#"incipit { fixum _ s ← |ff| ↦ textus ⇥ "?" }"#);
}

#[test]
fn fractus_to_octeti_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ b ← 1.5 ↦ octeti }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn bivalens_to_octeti_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ b ← verum ↦ octeti }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn octeti_to_fractus_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ f ← |00| ↦ fractus }",
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
            source: "incipit { fixum _ f ← 42 ↦ fractus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "numerus_to_bivalens",
            source: "incipit { fixum _ b ← 42 ↦ bivalens }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "numerus_to_textus",
            source: "incipit { fixum _ s ← 42 ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "fractus_to_numerus",
            source: "incipit { fixum _ n ← 1.5 ↦ numerus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "fractus_to_bivalens",
            source: "incipit { fixum _ b ← 1.5 ↦ bivalens }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "fractus_to_textus",
            source: "incipit { fixum _ s ← 1.5 ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "bivalens_to_numerus",
            source: "incipit { fixum _ n ← verum ↦ numerus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "bivalens_to_fractus",
            source: "incipit { fixum _ f ← verum ↦ fractus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "bivalens_to_textus",
            source: "incipit { fixum _ s ← verum ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "textus_to_numerus",
            source: r#"incipit { fixum _ n ← "42" ↦ numerus }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "textus_to_fractus",
            source: r#"incipit { fixum _ f ← "1.5" ↦ fractus }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "textus_to_bivalens",
            source: r#"incipit { fixum _ b ← "hi" ↦ bivalens }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "ascii_to_bivalens",
            source: "incipit { fixum _ b ← 'hi' ↦ bivalens }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "ascii_to_textus",
            source: "incipit { fixum _ s ← 'hi' ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "numerus_to_octeti",
            source: "incipit { fixum _ b ← 42 ↦ octeti }",
            expect: MatrixExpect::RejectsIssue("numeric_to_octeti_unspecified"),
        },
        ConversioMatrixCase {
            name: "octeti_to_numerus",
            source: "incipit { fixum _ n ← |00| ↦ numerus }",
            expect: MatrixExpect::RejectsIssue("octeti_to_numerus_unspecified"),
        },
        ConversioMatrixCase {
            name: "octeti_to_bivalens",
            source: "incipit { fixum _ b ← |00| ↦ bivalens }",
            expect: MatrixExpect::RejectsIssue("octeti_to_bivalens_unsupported"),
        },
        ConversioMatrixCase {
            name: "non_text_to_regex",
            source: "incipit { fixum _ r ← 42 ↦ regex }",
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
            source: r#"incipit { fixum valor v ← "1979-05-27T07:32:00Z" fixum instans t ← v ↦ instans }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_instans_offset",
            source: r#"incipit { fixum valor v ← "1979-05-27T07:32:00-04:00" fixum instans t ← v ↦ instans }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "instans_to_textus",
            source: "functio wire(instans t) → textus { redde t ↦ textus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_numerus",
            source: r"incipit { fixum valor v ← 42 fixum numerus n ← v ↦ numerus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_fractus_widen",
            source: r"incipit { fixum valor v ← 7 fixum fractus f ← v ↦ fractus }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_textus_instans_wire",
            source: r#"incipit { fixum valor v ← "1979-05-27T07:32:00Z" fixum textus s ← v ↦ textus }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_bivalens",
            source: r"incipit { fixum valor v ← verum fixum bivalens b ← v ↦ bivalens }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_ascii",
            source: r"incipit { fixum valor v ← 'ok' fixum ascii a ← v ↦ ascii }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_numerus_recovery",
            source: r#"incipit { fixum valor v ← "x" fixum numerus n ← v ↦ numerus ⇥ 0 }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_lista",
            source: r"incipit { fixum valor v ← [1, 2, 3] fixum lista<numerus> xs ← v ↦ lista<numerus> }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_to_tabula",
            source: r#"incipit { fixum valor v ← { "a": 1 } ↦ valor fixum tabula<textus, numerus> m ← v ↦ tabula<textus, numerus> }"#,
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "lista_to_valor",
            source: r"incipit { fixum lista<numerus> xs ← [1, 2] fixum valor v ← xs ↦ valor }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "tensor_to_valor_compose",
            source: r"incipit { fixum tensor<numerus, [2]> t ← [1, 2] ↦ tensor<numerus, [2]> fixum valor v ← t ↦ valor }",
            expect: MatrixExpect::Accepts,
        },
        ConversioMatrixCase {
            name: "valor_verte_numerus",
            source: r"incipit { fixum valor v ← 42 fixum numerus n ← v ∷ numerus }",
            expect: MatrixExpect::RejectsIssue("valor_verte_unsupported"),
        },
        ConversioMatrixCase {
            name: "valor_verte_tabula",
            source: r#"incipit { fixum valor v ← { "a": 1 } ↦ valor fixum _ m ← v ∷ tabula<textus, numerus> }"#,
            expect: MatrixExpect::RejectsIssue("valor_verte_unsupported"),
        },
        ConversioMatrixCase {
            name: "valor_to_cursor",
            source: r"incipit { fixum valor v ← [1] fixum _ c ← v ↦ cursor<numerus> }",
            expect: MatrixExpect::RejectsIssue("valor_to_cursor_boundary"),
        },
        ConversioMatrixCase {
            name: "valor_to_promissum",
            source: r"incipit { fixum valor v ← 1 fixum _ p ← v ↦ promissum<numerus> }",
            expect: MatrixExpect::RejectsIssue("valor_to_promissum_boundary"),
        },
        ConversioMatrixCase {
            name: "valor_to_regex",
            source: r#"incipit { fixum valor v ← "x" fixum _ r ← v ↦ regex }"#,
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
    test_support::assert_rejects_with_issue(
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
    test_support::assert_accepts(
        r"
functio before(instans fine, instans<ms> coarse) → bivalens {
    redde fine < coarse
}
",
    );
}

#[test]
fn primitive_conversio_matrix_typecheck() {
    for case in primitive_conversio_matrix_cases() {
        let result = test_support::compile_and_get_result(case.source);
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
    test_support::assert_accepts("incipit { fixum _ s ← [1, 2, 2] ↦ copia<numerus> }");
}

#[test]
fn copia_to_lista_conversio_typechecks() {
    test_support::assert_accepts(
        "incipit {\n  varia copia<numerus> s ← vacua\n  s.adde(1)\n  fixum _ xs ← s ↦ lista<numerus>\n}",
    );
}

#[test]
fn tensor_to_lista_conversio_typechecks() {
    test_support::assert_accepts(
        "incipit {\n  fixum tensor<numerus, []> t ← vacua\n  fixum _ xs ← t ↦ lista<numerus>\n}",
    );
}

#[test]
fn lista_to_tensor_conversio_typechecks() {
    test_support::assert_accepts("incipit { fixum _ t ← [1, 2, 3] ↦ tensor<numerus, [3]> }");
}

#[test]
fn shaped_lista_to_tensor_conversio_typechecks() {
    test_support::assert_accepts(
        "incipit { fixum lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]\n  fixum tf32[2, 2] t ← flat ↦ tf32[2, 2] }",
    );
}

#[test]
fn rectangular_lista_literal_to_tensor_conversio_typechecks() {
    test_support::assert_accepts(
        "incipit { fixum tf32[2, 2] t ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[2, 2] }",
    );
}

#[test]
fn rectangular_lista_literal_to_tensor_conversio_rejects_ragged_rows() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ t ← [[1.0, 2.0], [3.0]] ↦ tf32[2, 2] }",
        "rectangular_lista_literal_row_length",
    );
}

#[test]
fn rectangular_lista_literal_to_tensor_conversio_rejects_shape_mismatch() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ t ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[3, 3] }",
        "rectangular_lista_literal_shape",
    );
}

#[test]
fn sparse_tensor_conversio_typechecks_both_directions() {
    test_support::assert_accepts(
        r"incipit {
  fixum tf32[2, 2] dense ← [[1.0, 0.0], [0.0, 2.0]] ↦ tf32[2, 2]
  fixum sf32[2, 2] sparse ← dense ↦ sf32[2, 2]
  fixum tf32[2, 2] roundtrip ← sparse ↦ tf32[2, 2]
  nota roundtrip.longitudo()
}",
    );
}

#[test]
fn sparse_tensor_conversio_rejects_shape_change() {
    test_support::assert_rejects_with_issue(
        r"incipit {
  fixum tf32[2, 2] dense ← vacua
  fixum sf32[4] sparse ← dense ↦ sf32[4]
  nota sparse.longitudo()
}",
        "sparsa_tensor_shape_mismatch",
    );
}

#[test]
fn sparse_tensor_conversio_rejects_element_change() {
    test_support::assert_rejects_with_issue(
        r"incipit {
  fixum tf32[2] dense ← vacua
  fixum sf64[2] sparse ← dense ↦ sf64[2]
  nota sparse.longitudo()
}",
        "sparsa_tensor_element_mismatch",
    );
}

#[test]
fn sparse_to_lista_conversio_rejects_as_ambiguous() {
    test_support::assert_rejects_with_issue(
        r"incipit {
  fixum sf32[2] sparse ← vacua
  fixum lista<f32> values ← sparse ↦ lista<f32>
  nota values.longitudo()
}",
        "sparsa_collection_unsupported",
    );
}

#[test]
fn nested_lista_variable_to_tensor_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum lista<lista<f32>> rows ← [[1.0, 2.0], [3.0, 4.0]]\n  fixum _ t ← rows ↦ tf32[2, 2] }",
        "lista_to_tensor_nested",
    );
}

#[test]
fn lista_to_cursor_conversio_typechecks() {
    test_support::assert_accepts("incipit { fixum _ c ← [1, 2, 3] ↦ cursor<numerus> }");
}

#[test]
fn cursor_to_lista_conversio_typechecks() {
    test_support::assert_accepts(
        "incipit { fixum _ xs ← [1, 2, 3] ↦ cursor<numerus> ↦ lista<numerus> }",
    );
}

#[test]
fn tabula_to_lista_keys_conversio_typechecks() {
    test_support::assert_accepts(
        r#"incipit {
  fixum tabula<textus, numerus> m ← { "a": 1, "b": 2 }
  fixum _ ks ← m ↦ lista<textus>
}"#,
    );
}

#[test]
fn tabula_to_lista_values_conversio_typechecks() {
    test_support::assert_accepts(
        r#"incipit {
  fixum tabula<textus, numerus> m ← { "a": 1, "b": 2 }
  fixum _ vs ← m ↦ lista<numerus>
}"#,
    );
}

#[test]
fn lista_to_tabula_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ m ← [1, 2] ↦ tabula<textus, numerus> }",
        "lista_to_tabula_unsupported",
    );
}

#[test]
fn sync_to_promissum_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ p ← 42 ↦ promissum<numerus> }",
        "promissum_target_boundary",
    );
}

#[test]
fn promissum_to_sync_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        r"functio takesPromissum(promissum<numerus> p) → numerus {
  fixum _ n ← p ↦ numerus
  redde n
}",
        "promissum_source_boundary",
    );
}

#[test]
fn copia_to_tabula_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit {\n  varia copia<numerus> s ← vacua\n  fixum _ m ← s ↦ tabula<textus, numerus>\n}",
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
            source: "incipit { fixum _ s ← [1, 2] ↦ copia<numerus> }",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "copia_to_lista",
            source: "incipit {\n  fixum copia<numerus> s ← [1, 2] ↦ copia<numerus>\n  fixum _ xs ← s ↦ lista<numerus>\n}",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "tensor_to_lista",
            source: "incipit {\n  fixum tensor<numerus, []> t ← vacua\n  fixum _ xs ← t ↦ lista<numerus>\n}",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "lista_to_tensor",
            source: "incipit { fixum _ t ← [1, 2, 3] ↦ tensor<numerus, [3]> }",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "cursor_to_lista",
            source: "incipit { fixum _ xs ← [1, 2] ↦ cursor<numerus> ↦ lista<numerus> }",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "lista_to_cursor",
            source: "incipit { fixum _ c ← [1, 2] ↦ cursor<numerus> }",
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "tabula_to_lista_keys",
            source: r#"incipit {
  fixum tabula<textus, numerus> m ← { "a": 1 }
  fixum _ ks ← m ↦ lista<textus>
}"#,
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "tabula_to_lista_values",
            source: r#"incipit {
  fixum tabula<textus, numerus> m ← { "a": 1 }
  fixum _ vs ← m ↦ lista<numerus>
}"#,
            expect: MatrixExpect::Accepts,
        },
        CollectionMatrixCase {
            name: "lista_to_tabula",
            source: "incipit { fixum _ m ← [1] ↦ tabula<textus, numerus> }",
            expect: MatrixExpect::RejectsIssue("lista_to_tabula_unsupported"),
        },
        CollectionMatrixCase {
            name: "sync_to_promissum",
            source: "incipit { fixum _ p ← 1 ↦ promissum<numerus> }",
            expect: MatrixExpect::RejectsIssue("promissum_target_boundary"),
        },
        CollectionMatrixCase {
            name: "promissum_to_sync",
            source: r"functio takesPromissum(promissum<numerus> p) → numerus {
  fixum _ n ← p ↦ numerus
  redde n
}",
            expect: MatrixExpect::RejectsIssue("promissum_source_boundary"),
        },
        CollectionMatrixCase {
            name: "copia_to_tabula",
            source: "incipit {\n  fixum copia<numerus> s ← [1] ↦ copia<numerus>\n  fixum _ m ← s ↦ tabula<textus, numerus>\n}",
            expect: MatrixExpect::RejectsIssue("cross_collection_unsupported"),
        },
        CollectionMatrixCase {
            name: "tabula_to_copia",
            source: r#"incipit {
  fixum tabula<textus, numerus> m ← { "a": 1 }
  fixum _ s ← m ↦ copia<numerus>
}"#,
            expect: MatrixExpect::RejectsIssue("cross_collection_unsupported"),
        },
        CollectionMatrixCase {
            name: "tensor_to_copia",
            source: "incipit {\n  fixum tensor<numerus, []> t ← vacua\n  fixum _ s ← t ↦ copia<numerus>\n}",
            expect: MatrixExpect::RejectsIssue("cross_collection_unsupported"),
        },
    ]
}

#[test]
fn tensor_numerus_to_fractus_conversio_typechecks() {
    test_support::assert_accepts(
        "incipit {\n  fixum tensor<numerus, []> a ← vacua\n  fixum _ b ← a ↦ tensor<fractus, []>\n}",
    );
}

#[test]
fn tensor_fractus_to_numerus_conversio_typechecks() {
    test_support::assert_accepts(
        "incipit {\n  fixum tensor<fractus, []> b ← vacua\n  fixum _ a ← b ↦ tensor<numerus, []>\n}",
    );
}

#[test]
fn tensor_to_scalar_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit {\n  fixum tensor<numerus, []> t ← vacua\n  fixum _ n ← t ↦ numerus\n}",
        "tensor_to_scalar_unsupported",
    );
}

#[test]
fn scalar_to_tensor_conversio_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit {\n  fixum _ t ← 42 ↦ tensor<numerus, []>\n}",
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
            source: "incipit { fixum _ t ← [1, 2, 3] ↦ tensor<numerus, [3]> }",
            expect: MatrixExpect::Accepts,
        },
        TensorMatrixCase {
            name: "tensor_to_lista",
            source: "incipit {\n  fixum tensor<numerus, []> t ← vacua\n  fixum _ xs ← t ↦ lista<numerus>\n}",
            expect: MatrixExpect::Accepts,
        },
        TensorMatrixCase {
            name: "tensor_numerus_to_fractus",
            source: "incipit {\n  fixum tensor<numerus, []> a ← vacua\n  fixum _ b ← a ↦ tensor<fractus, []>\n}",
            expect: MatrixExpect::Accepts,
        },
        TensorMatrixCase {
            name: "tensor_fractus_to_numerus",
            source: "incipit {\n  fixum tensor<fractus, []> b ← vacua\n  fixum _ a ← b ↦ tensor<numerus, []>\n}",
            expect: MatrixExpect::Accepts,
        },
        // Element-width lattice pairs: see `tensor_element_width_lattice_matrix_typecheck` (100 cells).
        TensorMatrixCase {
            name: "tensor_sized_to_scalar_rejects",
            source: "incipit {\n  fixum tensor<numerus<i32>, []> t ← vacua\n  fixum _ n ← t ↦ numerus<i32>\n}",
            expect: MatrixExpect::RejectsIssue("tensor_to_scalar_unsupported"),
        },
        TensorMatrixCase {
            name: "tensor_to_scalar",
            source: "incipit {\n  fixum tensor<numerus, []> t ← vacua\n  fixum _ n ← t ↦ numerus\n}",
            expect: MatrixExpect::RejectsIssue("tensor_to_scalar_unsupported"),
        },
        TensorMatrixCase {
            name: "scalar_to_tensor",
            source: "incipit { fixum _ t ← 1 ↦ tensor<numerus, []> }",
            expect: MatrixExpect::RejectsIssue("scalar_to_tensor_unsupported"),
        },
    ]
}

#[test]
fn tensor_conversio_matrix_typecheck() {
    for case in tensor_conversio_matrix_cases() {
        let result = test_support::compile_and_get_result(case.source);
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
        let result = test_support::compile_and_get_result(case.source);
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
    let result = crate::test_support::analyze_source(
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
    test_support::assert_accepts(
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
    test_support::assert_accepts(
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
    test_support::assert_rejects_with_issue(
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
    test_support::assert_rejects_with_issue(
        r"
functio cast<magnitudo R, magnitudo C>() → matrix<f32, [R, C]> {
    redde [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [R, C]>
}
",
        "matrix_materialize_unsupported",
    );
}

// ---- `sermo ↦ T` materialization conversio ----

#[test]
fn sermo_materialize_to_vacuum_typechecks() {
    test_support::assert_accepts(r#"incipit { fixum _ v ← ad 'runtime:echo' ("salve") ↦ vacuum }"#);
}

#[test]
fn sermo_materialize_to_textus_typechecks() {
    test_support::assert_accepts(r#"incipit { fixum _ t ← ad 'runtime:echo' ("salve") ↦ textus }"#);
}

#[test]
fn sermo_materialize_to_octeti_typechecks() {
    test_support::assert_accepts(r#"incipit { fixum _ o ← ad 'runtime:echo' ("salve") ↦ octeti }"#);
}

#[test]
fn sermo_materialize_to_valor_typechecks() {
    test_support::assert_accepts(r#"incipit { fixum _ v ← ad 'runtime:echo' ("salve") ↦ valor }"#);
}

#[test]
fn sermo_materialize_to_numerus_typechecks() {
    test_support::assert_accepts(r"incipit { fixum _ n ← ad 'runtime:echo' (5) ↦ numerus }");
}

#[test]
fn sermo_materialize_to_bivalens_typechecks() {
    test_support::assert_accepts(r"incipit { fixum _ b ← ad 'runtime:echo' (verum) ↦ bivalens }");
}

#[test]
fn sermo_materialize_to_lista_typechecks() {
    test_support::assert_accepts(
        r#"incipit { fixum _ xs ← ad 'runtime:echo' ("salve") ↦ lista<textus> }"#,
    );
}

#[test]
fn sermo_materialize_to_type_param_typechecks() {
    test_support::assert_accepts(
        r#"
functio collige<T>(textus via) → T {
    redde ad 'runtime:echo' (via) ↦ T
}

incipit {
    fixum textus corpus ← collige<textus>("README.md")
    nota corpus
}
"#,
    );
}

#[test]
fn sermo_materialize_rejects_tabula() {
    test_support::assert_rejects_with_issue(
        r#"incipit { fixum _ m ← ad 'runtime:echo' ("salve") ↦ tabula<textus, numerus> }"#,
        "sermo_materialize_target_unsupported",
    );
}

#[test]
fn sermo_materialize_rejects_tensor() {
    test_support::assert_rejects_with_issue(
        r#"incipit { fixum _ t ← ad 'runtime:echo' ("salve") ↦ tensor<numerus, [2]> }"#,
        "sermo_materialize_target_unsupported",
    );
}

#[test]
fn regex_literal_pattern_empty_accepts() {
    // Boundary: 0-byte pattern, well within REGEX_PATTERN_MAX_BYTES (1024).
    test_support::assert_accepts(r#"incipit { fixum _ r ← "" ↦ regex }"#);
}

#[test]
fn octeti_empty_literal_typechecks() {
    test_support::assert_accepts("incipit { fixum _ b ← || ↦ octeti }");
}

#[test]
fn sermo_materialize_rejects_copia() {
    test_support::assert_rejects_with_issue(
        r#"incipit { fixum _ s ← ad 'runtime:echo' ("salve") ↦ copia<textus> }"#,
        "sermo_materialize_target_unsupported",
    );
}

// Conversion-directed assignment (`↤`)

#[test]
fn conversio_assign_accepts_mutable_local_place() {
    test_support::assert_accepts("incipit {\n  varia numerus count ← 0\n  count ↤ \"42\"\n}");
}

#[test]
fn conversio_assign_rejects_immutable_binding_after_init() {
    // A `fixum` local after its single initializer is not a valid place
    // (write-once); the definite-assignment `immutable_second_assignment`
    // issue is expected once the form routes through `Assign`.
    test_support::assert_rejects_with_issue(
        "incipit {\n  fixum numerus count ← 0\n  count ↤ \"42\"\n}",
        "immutable_second_assignment",
    );
}

#[test]
fn conversio_assign_rejects_inference_hole_destination() {
    // The destination type must be resolved before conversion checking;
    // `fixum _ xs ← []` leaves an element inference hole that cannot name a
    // conversion target.
    test_support::assert_rejects_with_issue(
        "incipit {\n  fixum _ xs ← []\n  xs ↤ \"2\"\n}",
        "conversio_assign_inference_hole_destination",
    );
}

#[test]
fn conversio_assign_rejects_unsupported_route() {
    test_support::assert_rejects_with_issue(
        "incipit {\n  fixum json j ↤ \"x\"\n}",
        "unsupported_conversio",
    );
}

#[test]
fn conversio_assign_accepts_recovery_before_storage() {
    // `⇥ fallback` types before storage: the fallback satisfies the same
    // target-type rule as explicit `↦` recovery.
    test_support::assert_accepts("incipit {\n  varia numerus count ← 0\n  count ↤ \"x\" ⇥ 0\n}");
}

#[test]
fn conversio_assign_accepts_right_associated_chain() {
    // Inner converts to `b`'s type, then the inner result converts to `a`'s
    // type — both textus→numerus.
    test_support::assert_accepts(
        "incipit {\n  varia numerus a ← 0\n  varia numerus b ← 0\n  a ↤ b ↤ \"42\"\n}",
    );
}

#[test]
fn conversio_assign_accepts_field_place() {
    test_support::assert_accepts(
        r#"
genus Persona { textus nomen numerus aetas }
functio muta() → numerus {
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
    test_support::assert_accepts(
        r#"
functio muta() → numerus {
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
    test_support::assert_accepts("incipit {\n  fixum numerus answer ↤ \"42\"\n}");
}

#[test]
fn conversio_assign_typed_fixum_item_scope_initializer_accepts() {
    // Item-scope typed `fixum T x ↤ y` is the same conversion-directed
    // initializer as the local form, not SEM020 assignment to a constant.
    test_support::assert_accepts(r#"fixum numerus n ↤ "42""#);
    test_support::assert_rejects_with_issue(
        "fixum numerus n ↤ \"42\"\nincipit { n ← 1 }",
        "assignment_to_constant",
    );
}

#[test]
fn conversio_assign_untyped_fixum_initializer_rejects() {
    // `fixum _` has no written type → no concrete conversion target.
    test_support::assert_rejects_with_issue(
        "incipit {\n  fixum _ answer ↤ \"42\"\n}",
        "conversio_assign_requires_concrete_type",
    );
}

#[test]
fn conversio_assign_sit_initializer_rejects() {
    // `sit` is an inferred binding (≡ `fixum _`) → no concrete target.
    test_support::assert_rejects_with_issue(
        "incipit {\n  sit answer ↤ \"42\"\n}",
        "conversio_assign_requires_concrete_type",
    );
}

#[test]
fn conversio_assign_untyped_destructuring_initializer_rejects() {
    // Untyped destructuring patterns have no concrete conversion target.
    test_support::assert_rejects_with_issue(
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
        test_support::assert_rejects_with_issue(source, "conversio_assign_await_unsupported");
    }
}

#[test]
fn conversio_assign_recovery_typed_before_storage() {
    // `⇥ fallback` is typed against the resolved destination type through the
    // same target-type rule as explicit `↦` recovery — a wrong-typed fallback
    // rejects before any store.
    test_support::assert_rejects_with_issue(
        "incipit {\n  varia numerus count ← 0\n  count ↤ \"x\" ⇥ \"fallback\"\n}",
        "conversio_recovery_type_mismatch",
    );
    test_support::assert_accepts("incipit {\n  varia numerus count ← 0\n  count ↤ \"x\" ⇥ 0\n}");
}

#[test]
fn conversio_assign_routes_through_live_tuus_cursor_escape_no_bypass() {
    // `↤` must not bypass the live-cursor escape pass. A live tuus cursor
    // destination reassigned via `↤` is the same `live_tuus_cursor_reassign`
    // error as ordinary `←`, and a live cursor source copied through `↤`
    // into a non-live path is the same `live_tuus_cursor_copy` error.
    test_support::assert_rejects_with_issue(
        "incipit {\n  fixum sermo s ← ad 'runtime:echo' (\"salve\")\n  fixum tuus<textus> t ← s.tuus<textus>()\n  fixum cursor<scrinium<textus>> c ← t.cursor()\n  c ↤ t.cursor()\n}",
        "live_tuus_cursor_reassign",
    );
    test_support::assert_rejects_with_issue(
        "incipit {\n  fixum sermo s ← ad 'runtime:echo' (\"salve\")\n  fixum tuus<textus> t ← s.tuus<textus>()\n  fixum cursor<scrinium<textus>> c2\n  c2 ↤ t.cursor()\n}",
        "live_tuus_cursor_copy",
    );
}
