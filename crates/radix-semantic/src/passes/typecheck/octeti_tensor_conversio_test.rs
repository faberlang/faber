//! Typecheck pins for `octeti ↦ tensor<E, [shape]> via Be|Le`: a faithful
//! bit reinterpretation of the bytes. Assertions are on code, issue, and
//! args; never on message text.

use crate::test_support::{
    self, analyze_source_la, assert_accepts_la, assert_rejects_with_issue_la,
};
use crate::{SemanticError, SemanticErrorKind};

const UNSPECIFIED: &str = "list_b8_to_tensor_unspecified";
const ELEMENT: &str = "list_b8_to_tensor_element_unsupported";
const LENGTH: &str = "list_b8_to_tensor_length_mismatch";
const RECORD_BIG_ENDIAN: &str = "list_b8_to_tensor_record_big_endian";
const HANDLER: &str = "failable_conversio_requires_handler";

fn arg<'a>(err: &'a SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn error_with_issue(source: &str, issue: &str) -> SemanticError {
    let result = analyze_source_la(source);
    result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && test_support::issue(err) == Some(issue))
        .unwrap_or_else(|| panic!("expected {issue}, got {:?}", result.diagnostics))
        .clone()
}

fn assert_no_issue(source: &str, unexpected: &str) {
    let result = analyze_source_la(source);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|err| test_support::issue(err) == Some(unexpected)),
        "unexpected issue {unexpected:?}: {:?}",
        result.diagnostics
    );
}

fn read(element: &str, shape: &str, hint: &str) -> String {
    format!("incipit {{ sit _ t ← |00 01 02 03| ↦ tensor<{element}, {shape}> via {hint} }}")
}

// --- accepted -------------------------------------------------------------

#[test]
fn f32_le_and_be_are_accepted() {
    assert_accepts_la(
        "functio w(lista<b8> b) → tensor<f32, [2, 3]> ⇥ textus { redde b ↦ tensor<f32, [2, 3]> via Le }",
    );
    assert_accepts_la(
        "functio w(lista<b8> b) → tensor<f32, [2, 3]> ⇥ textus { redde b ↦ tensor<f32, [2, 3]> via Be }",
    );
}

#[test]
fn every_element_width_is_accepted_with_a_matching_literal() {
    let eight = "00 01 02 03 04 05 06 07";
    for (element, count) in [
        ("u8", 8),
        ("i8", 8),
        ("i16", 4),
        ("u16", 4),
        ("i32", 2),
        ("u32", 2),
        ("i64", 1),
        ("u64", 1),
        ("f32", 2),
        ("f64", 1),
    ] {
        if matches!(element, "u8" | "i8") {
            // An 8-bit element has no byte order and takes no clause (B6).
            assert_accepts_la(&format!(
                "incipit {{ sit _ t ← |{eight}| ↦ tensor<{element}, [{count}]> }}"
            ));
            continue;
        }
        for hint in ["Le", "Be"] {
            assert_accepts_la(&format!(
                "incipit {{ sit _ t ← |{eight}| ↦ tensor<{element}, [{count}]> via {hint} }}"
            ));
        }
    }
}

#[test]
fn bounded_octeti_source_is_accepted() {
    assert_accepts_la(
        "functio w(lista<b8, magnitudo 8> b) → tensor<f32, [2]> ⇥ textus { redde b ↦ tensor<f32, [2]> via Le }",
    );
}

#[test]
fn runtime_length_source_is_accepted_without_a_static_check() {
    assert_accepts_la("incipit { sit _ t ← |00 01 02 03|[0‥4] ↦ tensor<f32, [1]> via Le }");
    assert_no_issue(
        "functio w(lista<b8> b) → tensor<f32, [4]> ⇥ textus { redde b ↦ tensor<f32, [4]> via Le }",
        LENGTH,
    );
}

#[test]
fn non_literal_extents_are_not_rejected_here() {
    assert_no_issue(
        "functio w(lista<b8> b, magnitudo N) → tensor<f32, [N]> ⇥ textus { redde b ↦ tensor<f32, [N]> via Le }",
        UNSPECIFIED,
    );
    assert_no_issue(
        "functio w(lista<b8> b, magnitudo N) → tensor<f32, [N]> ⇥ textus { redde b ↦ tensor<f32, [N]> via Le }",
        LENGTH,
    );
}

#[test]
fn handlers_cover_the_failable_conversion() {
    assert_accepts_la(concat!(
        "functio w(lista<b8> b) → numerus {\n",
        "    fac {\n        sit _ t ← b ↦ tensor<f32, [2]> via Le\n        redde 1\n",
        "    } cape err {\n        redde 0\n    }\n",
        "}\n",
    ));
    assert_accepts_la(
        "functio w(lista<b8> b) → tensor<f32, [2]> ⇥ textus {\n    redde b ↦ tensor<f32, [2]> via Le\n}\n",
    );
    assert_accepts_la(
        "functio w(lista<b8> b, tensor<f32, [2]> d) → tensor<f32, [2]> {\n    redde b ↦ tensor<f32, [2]> via Le ⊥ d\n}\n",
    );
}

#[test]
fn main_body_is_exempt_from_the_handler_rule() {
    assert_accepts_la("incipit { sit _ t ← |00 01 02 03 04 05 06 07| ↦ tensor<f32, [2]> via Le }");
}

// --- rejected: hint -------------------------------------------------------

#[test]
fn missing_hint_is_unspecified() {
    let err = error_with_issue(
        "incipit { sit _ t ← |00 01 02 03| ↦ tensor<f32, [1]> }",
        UNSPECIFIED,
    );
    assert_eq!(err.kind, SemanticErrorKind::ConversionInvalid);
}

#[test]
fn one_byte_elements_take_no_hint_and_refuse_one() {
    assert_accepts_la("incipit { sit _ t ← |00 01| ↦ tensor<u8, [2]> }");
    for hint in ["Le", "Be"] {
        assert_rejects_with_issue_la(
            &format!("incipit {{ sit _ t ← |00 01| ↦ tensor<u8, [2]> via {hint} }}"),
            "bits_via_clause_not_allowed",
        );
    }
}

#[test]
fn hex_hint_is_unspecified() {
    for hint in ["Hex"] {
        assert_rejects_with_issue_la(&read("f32", "[1]", hint), UNSPECIFIED);
    }
}

#[test]
fn code_hint_is_rejected_by_the_littera_code_gate() {
    // `Code` is claimed first by the littera row check, which names every
    // non-code-point pair `code_hint_pair_mismatch`.
    assert_rejects_with_issue_la(&read("f32", "[1]", "Code"), "code_hint_pair_mismatch");
}

#[test]
fn misspelled_endian_hint_is_rejected_as_an_unknown_hint() {
    // The lowering hint gate names it before typecheck sees the pair, as for
    // every other conversion row; the checker never admits the pair.
    assert_rejects_with_issue_la(&read("f32", "[1]", "LE"), "conversio_via_unknown_hint");
}

// --- rejected: element ----------------------------------------------------

#[test]
fn half_float_elements_are_unsupported() {
    for element in ["f16", "bf16"] {
        assert_rejects_with_issue_la(&read(element, "[2]", "Le"), ELEMENT);
    }
}

#[test]
fn vector_element_is_unsupported() {
    assert_rejects_with_issue_la(
        "incipit { sit _ t ← |00 01 02 03| ↦ tensor<vector<f32, magnitudo 4>, [2]> via Le }",
        ELEMENT,
    );
}

// --- rejected: static length ----------------------------------------------

#[test]
fn static_literal_length_mismatch_carries_expected_and_actual() {
    let err = error_with_issue(&read("f32", "[2]", "Le"), LENGTH);
    assert_eq!(err.kind, SemanticErrorKind::ConversionInvalid);
    assert_eq!(arg(&err, "expected"), Some("8"));
    assert_eq!(arg(&err, "actual"), Some("4"));
}

#[test]
fn static_length_mismatch_multiplies_every_extent() {
    let err = error_with_issue(&read("i16", "[2, 3]", "Be"), LENGTH);
    assert_eq!(arg(&err, "expected"), Some("12"));
    assert_eq!(arg(&err, "actual"), Some("4"));
}

#[test]
fn static_window_length_mismatch_is_rejected() {
    let err = error_with_issue(
        "incipit { sit _ t ← |00 01 02 03 04 05 06 07|[0‥8] ↦ tensor<f32, [1]> via Le }",
        LENGTH,
    );
    assert_eq!(arg(&err, "expected"), Some("4"));
    assert_eq!(arg(&err, "actual"), Some("8"));
}

// --- rejected: handler ----------------------------------------------------

#[test]
fn bare_conversion_outside_main_requires_a_handler() {
    assert_rejects_with_issue_la(
        "functio w(lista<b8> b) → tensor<f32, [2]> {\n    redde b ↦ tensor<f32, [2]> via Le\n}\n",
        HANDLER,
    );
}

// --- the lowering hint gate ------------------------------------------------

#[test]
fn tensor_target_takes_a_via_hint() {
    assert_no_issue(
        "functio w(lista<b8> b) → tensor<f32, [2]> ⇥ textus { redde b ↦ tensor<f32, [2]> via Le }",
        "conversio_via_target_takes_no_hint",
    );
}

#[test]
fn other_non_hint_targets_still_reject_a_via_hint() {
    // The byte list is `lista<b8>`, so a numeric list target is the bits
    // row's: an unsized numeric element has no width to match. Text is still
    // refused by the lowering hint gate before typecheck sees the pair.
    assert_rejects_with_issue_la(
        "functio w(lista<b8> b) → lista<numerus> { redde b ↦ lista<numerus> via Le }",
        "bits_conversion_width_mismatch",
    );
    assert_rejects_with_issue_la(
        "functio w(lista<b8> b) → textus { redde b ↦ textus via Le }",
        "conversio_via_target_takes_no_hint",
    );
}

// --- inverse: tensor ↦ octeti via Be|Le -----------------------------------

const TENSOR_UNSPECIFIED: &str = "tensor_to_list_b8_unspecified";
const TENSOR_ELEMENT: &str = "tensor_to_list_b8_element_unsupported";

#[test]
fn tensor_to_octeti_is_accepted_for_every_element_and_order_and_is_infallible() {
    for element in [
        "u8", "i8", "i16", "u16", "i32", "u32", "i64", "u64", "f32", "f64",
    ] {
        if matches!(element, "u8" | "i8") {
            // An 8-bit element has no byte order and takes no clause (B6).
            assert_accepts_la(&format!(
                "functio w(tensor<{element}, [2, 2]> t) → lista<b8> {{ redde t ↦ lista<b8> }}"
            ));
            continue;
        }
        for hint in ["Le", "Be"] {
            // No `⇥` and no handler: the row cannot fail.
            assert_accepts_la(&format!(
                "functio w(tensor<{element}, [2, 2]> t) → lista<b8> {{ redde t ↦ lista<b8> via {hint} }}"
            ));
        }
    }
}

#[test]
fn tensor_to_octeti_refuses_a_clause_on_one_byte_elements() {
    for element in ["u8", "i8"] {
        for hint in ["Le", "Be"] {
            assert_rejects_with_issue_la(
                &format!(
                    "functio w(tensor<{element}, [2]> t) → lista<b8> {{ redde t ↦ lista<b8> via {hint} }}"
                ),
                "bits_via_clause_not_allowed",
            );
        }
    }
}

#[test]
fn tensor_to_octeti_without_a_hint_is_unspecified_even_for_one_byte_elements() {
    // B6: an 8-bit element takes no clause, so it is no longer unspecified
    // without one; every wider element still is.
    assert_accepts_la("functio w(tensor<u8, [2]> t) → lista<b8> { redde t ↦ lista<b8> }");
    for element in ["i16", "u32", "f32", "f64"] {
        let err = error_with_issue(
            &format!("functio w(tensor<{element}, [2]> t) → lista<b8> {{ redde t ↦ lista<b8> }}"),
            TENSOR_UNSPECIFIED,
        );
        assert_eq!(err.kind, SemanticErrorKind::ConversionInvalid);
    }
}

#[test]
fn tensor_to_octeti_with_an_unsupported_element_is_declined_by_name() {
    let err = error_with_issue(
        "functio w(tensor<f16, [2]> t) → lista<b8> { redde t ↦ lista<b8> via Le }",
        TENSOR_ELEMENT,
    );
    assert_eq!(err.kind, SemanticErrorKind::ConversionInvalid);
}

// --- record (fixed-layout genus) elements -------------------

/// A neutral fixed-layout record: one `f16` and a 32-slot `i8` array. The
/// checker never reads the layout; the run-time conversion resolves it.
const RECORD: &str = "genus Tabula {\n    varia f16 a\n    varia lista<i8, magnitudo 32> b\n}\n";

fn record_read(shape: &str, hint: &str) -> String {
    format!(
        "{RECORD}functio w(lista<b8> b) → numerus ⇥ textus {{ sit _ t ← b ↦ tensor<Tabula, {shape}> via {hint}\n redde 1 }}"
    )
}

#[test]
fn record_element_is_accepted_with_literal_extents() {
    // A run-time extent rides the same row; the runner tests cover its shape
    // operand.
    for shape in ["[4]", "[2, 3]"] {
        assert_accepts_la(&record_read(shape, "Le"));
    }
}

#[test]
fn record_element_gets_no_static_length_check() {
    // The record width lives in the layout law, not in the checker, so a
    // static byte window of any length reaches the run-time check.
    assert_accepts_la(&format!(
        "{RECORD}incipit {{ sit _ t ← |00 01 02 03|[0‥4] ↦ tensor<Tabula, [4]> via Le }}"
    ));
}

#[test]
fn record_element_requires_the_little_endian_clause() {
    assert_rejects_with_issue_la(
        &format!("{RECORD}incipit {{ sit _ t ← |00 01 02 03| ↦ tensor<Tabula, [1]> }}"),
        UNSPECIFIED,
    );
    assert_rejects_with_issue_la(&record_read("[4]", "Be"), RECORD_BIG_ENDIAN);
}
