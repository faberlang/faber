//! Regression coverage for the tensor type-directed construction and reshape clean break.
//!
//! These tests lock the typecheck behaviour for `tensor.creata()`, `tensor.structa()`,
//! `tensor.formata()`, and receiver-based alternatives (`crea`, `forma`, `formata`).
//! They also verify that stale API shapes produce structured diagnostics.

use radix_diagnostics::DiagnosticArg;

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

fn accepts(source: &str) {
    let result = compile(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = compile(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.args == vec![DiagnosticArg::new("issue", issue)]),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

const IMPORT: &str = r#"importa ex "norma:tensor" tensor
"#;

#[test]
fn static_tensor_creata_and_structa_accept_complete_type_args() {
    accepts(
        &(IMPORT.to_owned()
            + r"incipit {
  sit tf32[2, 2] grid ← tensor.creata<tf32[2, 2]>(0.0)
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
  sit tf32[2, 3] data ← tensor.structa<tf32[2, 3]>(flat)
  nota grid.longitudo()
  nota data.longitudo()
}"),
    );
}

#[test]
fn static_tensor_formata_accepts_complete_type_arg() {
    accepts(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
  sit tf32[2, 3] data ← flat ↦ tf32[2, 3]
  sit tf32[3, 2] view ← data.formata<tf32[3, 2]>()
  nota view.longitudo()
}",
    );
}

#[test]
fn dynamic_tensor_conversions_accept_payload_first() {
    accepts(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit lista<numerus> shape ← [2, 2]
  sit tf32[2, 2] grid ← tf32[2, 2] {}
  sit tf32[2, 2] data ← flat ↦ tf32[shape]
  sit tf32[4] view ← data.forma([4])
  nota view.longitudo()
}",
    );
}

#[test]
fn rejects_old_crea_argument_order() {
    // The argument-order heuristic retired with the crea registration; the
    // swapped-args costume now hits the retirement suggestion like every other
    // strue/crea spelling.
    rejects_with_issue(
        r"incipit {
  sit tf32[] seed ← vacua
  sit _ grid ← seed.crea([2, 2], 0.0)
}",
        "tensor_crea_retired",
    );
}

#[test]
fn rejects_old_structa_receiver_name() {
    rejects_with_issue(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit tf32[] seed ← vacua
  sit _ grid ← seed.structa(flat, [2, 2])
}",
        "tensor_receiver_structa_renamed",
    );
}

#[test]
fn rejects_old_tensor_creata_with_runtime_shape() {
    rejects_with_issue(
        &(IMPORT.to_owned()
            + r"incipit {
  sit _ grid ← tensor.creata(0.0, [2, 2])
}"),
        "tensor_creata_complete_type_arg_required",
    );
}

#[test]
fn rejects_old_tensor_creata_element_type_arg_with_runtime_shape() {
    rejects_with_issue(
        &(IMPORT.to_owned()
            + r"incipit {
  sit _ grid ← tensor.creata<numerus>(0, [2, 2])
}"),
        "tensor_type_arg_incomplete",
    );
}

#[test]
fn rejects_element_only_tensor_creata_type_arg() {
    rejects_with_issue(
        &(IMPORT.to_owned()
            + r"incipit {
  sit _ grid ← tensor.creata<f32>(0.0)
}"),
        "tensor_type_arg_incomplete",
    );
}

#[test]
fn rejects_tensor_structa_forma() {
    rejects_with_issue(
        &(IMPORT.to_owned()
            + r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
  sit _ grid ← tensor.structa_forma(flat, [2, 3])
}"),
        "tensor_structa_forma_removed",
    );
}

#[test]
fn rejects_receiver_creata_with_type_args() {
    rejects_with_issue(
        r"incipit {
  sit tf32[] seed ← vacua
  sit _ grid ← seed.creata<tf32[2, 2]>(0.0)
}",
        "tensor_receiver_creata_type_args",
    );
}

#[test]
fn rejects_tensor_creata_without_complete_type_arg() {
    rejects_with_issue(
        &(IMPORT.to_owned()
            + r"incipit {
  sit _ grid ← tensor.creata(0.0)
}"),
        "tensor_creata_complete_type_arg_required",
    );
}

#[test]
fn rejects_formata_without_complete_type_arg() {
    rejects_with_issue(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit tf32[2, 2] data ← flat ↦ tf32[2, 2]
  sit _ view ← data.formata()
}",
        "tensor_formata_complete_type_arg_required",
    );
}

#[test]
fn rejects_formata_value_args() {
    rejects_with_issue(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit tf32[2, 2] data ← flat ↦ tf32[2, 2]
  sit _ view ← data.formata<tf32[4]>([4])
}",
        "tensor_formata_forbids_value_args",
    );
}

#[test]
fn rejects_tensor_creata_element_type_mismatch_at_assignment() {
    rejects_with_issue(
        &(IMPORT.to_owned()
            + r#"incipit {
  sit _ grid ← tensor.creata<tf32[2, 2]>("bad")
}"#),
        "argument_type_mismatch",
    );
}

#[test]
fn rejects_formata_element_type_mismatch_with_structured_issue_arg() {
    rejects_with_issue(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit tf32[2, 2] data ← flat ↦ tf32[2, 2]
  sit _ view ← data.formata<tensor<numerus, [4]>>()
}",
        "tensor_formata_element_type_mismatch",
    );
}

#[test]
fn rejects_formata_element_count_mismatch_with_structured_issue_arg() {
    rejects_with_issue(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit tf32[2, 3] data ← flat ↦ tf32[2, 3]
  sit _ view ← data.formata<tf32[4, 2]>()
}",
        "tensor_formata_element_count_mismatch",
    );
}

#[test]
fn static_formata_accepts_equal_element_count_reshape() {
    accepts(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
  sit tf32[2, 3] data ← flat ↦ tf32[2, 3]
  sit tf32[3, 2] view ← data.formata<tf32[3, 2]>()
  nota view.longitudo()
}",
    );
}

#[test]
fn preserves_lista_to_tensor_conversion() {
    accepts(
        r"incipit {
  sit lista<numerus> xs ← [1, 2, 3, 4]
  sit tensor<numerus, [4]> grid ← xs ↦ tensor<numerus, [4]>
  nota grid.longitudo()
}",
    );
}
