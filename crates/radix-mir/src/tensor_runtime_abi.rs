//! Code-owned rows for the first MIR tensor runtime ABI slice.
//!
//! TARGET: Stage 6 of the tensor systems timeline. This table records
//! executable runner support separately from probe-only import names so docs
//! and future package/runtime work cannot over-claim target support.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorRuntimeAbiRow {
    pub facet: TensorRuntimeAbiFacet,
    pub target: TensorRuntimeAbiTarget,
    pub status: TensorRuntimeAbiStatus,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorRuntimeAbiFacet {
    ValueCarrier,
    OperationExecution,
    RecoverableFailure,
    ObservableOutput,
    RuntimeImport,
}

impl TensorRuntimeAbiFacet {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ValueCarrier => "tensor value carrier",
            Self::OperationExecution => "tensor operation execution",
            Self::RecoverableFailure => "recoverable tensor failure",
            Self::ObservableOutput => "observable tensor output",
            Self::RuntimeImport => "tensor runtime import",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorRuntimeAbiTarget {
    MirStepper,
    WasmText,
    LlvmText,
}

impl TensorRuntimeAbiTarget {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MirStepper => "mir-stepper",
            Self::WasmText => "wasm-text",
            Self::LlvmText => "llvm-text",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorRuntimeAbiStatus {
    Executable,
    ImportProbeOnly,
}

impl TensorRuntimeAbiStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::ImportProbeOnly => "import/probe-only",
        }
    }
}

pub const TENSOR_RUNTIME_ABI_ROWS: &[TensorRuntimeAbiRow] = &[
    TensorRuntimeAbiRow {
        facet: TensorRuntimeAbiFacet::ValueCarrier,
        target: TensorRuntimeAbiTarget::MirStepper,
        status: TensorRuntimeAbiStatus::Executable,
        evidence: "crates/radix/src/mir/runner/value.rs::Value::Tensor",
    },
    TensorRuntimeAbiRow {
        facet: TensorRuntimeAbiFacet::OperationExecution,
        target: TensorRuntimeAbiTarget::MirStepper,
        status: TensorRuntimeAbiStatus::Executable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_runs_tensor_runtime_abi_success_path",
    },
    TensorRuntimeAbiRow {
        facet: TensorRuntimeAbiFacet::RecoverableFailure,
        target: TensorRuntimeAbiTarget::MirStepper,
        status: TensorRuntimeAbiStatus::Executable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_reports_tensor_runtime_abi_negative_index_failure",
    },
    TensorRuntimeAbiRow {
        facet: TensorRuntimeAbiFacet::ObservableOutput,
        target: TensorRuntimeAbiTarget::MirStepper,
        status: TensorRuntimeAbiStatus::Executable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_runs_tensor_runtime_abi_success_path",
    },
    TensorRuntimeAbiRow {
        facet: TensorRuntimeAbiFacet::RuntimeImport,
        target: TensorRuntimeAbiTarget::WasmText,
        status: TensorRuntimeAbiStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name",
    },
    TensorRuntimeAbiRow {
        facet: TensorRuntimeAbiFacet::RuntimeImport,
        target: TensorRuntimeAbiTarget::LlvmText,
        status: TensorRuntimeAbiStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol",
    },
];

#[must_use]
pub fn tensor_runtime_abi_rows() -> &'static [TensorRuntimeAbiRow] {
    TENSOR_RUNTIME_ABI_ROWS
}
