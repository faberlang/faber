//! Tensor runtime ABI row contracts: runner execution and backend probe-only status.
//!
//! Tests: each RuntimeAbiFacet has expected status per target with evidence citations.

use super::{TensorRuntimeAbiFacet, TensorRuntimeAbiStatus, TensorRuntimeAbiTarget, tensor_runtime_abi_rows};

#[test]
fn tensor_runtime_abi_rows_record_runner_execution_contract() {
    let rows = tensor_runtime_abi_rows();
    let expected = [
        TensorRuntimeAbiFacet::ValueCarrier,
        TensorRuntimeAbiFacet::OperationExecution,
        TensorRuntimeAbiFacet::RecoverableFailure,
        TensorRuntimeAbiFacet::ObservableOutput,
    ];

    for facet in expected {
        let row = rows
            .iter()
            .find(|row| row.target == TensorRuntimeAbiTarget::MirStepper && row.facet == facet)
            .unwrap_or_else(|| panic!("missing runner ABI row for {facet:?}"));
        assert_eq!(row.status, TensorRuntimeAbiStatus::Executable);
        assert!(
            row.evidence.contains("crates/radix/src/mir/runner"),
            "runner ABI row should cite runner evidence: {row:?}"
        );
    }
}

#[test]
fn tensor_runtime_abi_rows_keep_wasm_and_llvm_probe_only() {
    let rows = tensor_runtime_abi_rows();
    for target in [TensorRuntimeAbiTarget::WasmText, TensorRuntimeAbiTarget::LlvmText] {
        let row = rows
            .iter()
            .find(|row| row.target == target && row.facet == TensorRuntimeAbiFacet::RuntimeImport)
            .unwrap_or_else(|| panic!("missing import row for {target:?}"));
        assert_eq!(row.status, TensorRuntimeAbiStatus::ImportProbeOnly);
        assert!(
            row.evidence.contains("import_names.rs") || row.evidence.contains("symbols.rs"),
            "probe-only row should cite import naming evidence: {row:?}"
        );
    }
}
