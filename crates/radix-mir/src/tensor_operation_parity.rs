//! Code-owned parity rows for tensor MIR operation families.
//!
//! TARGET: this is the checked source for operation-family support claims in
//! `docs/design/target-capability-matrix.md`. Docs may summarize these rows,
//! but target support/reject status changes should land here with tests first.

use super::MirCollectionOp;

/// Stable target/status row for one tensor operation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorOperationParityRow {
    pub family: TensorOperationFamily,
    pub target: TensorParityTarget,
    pub status: TensorParityStatus,
    pub evidence: &'static str,
}

/// Tensor operation family tracked by Stage 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorOperationFamily {
    IndexedAccess,
}

impl TensorOperationFamily {
    #[must_use]
    pub fn mir_ops(self) -> &'static [MirCollectionOp] {
        match self {
            Self::IndexedAccess => &[MirCollectionOp::TensorGet, MirCollectionOp::TensorSet],
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::IndexedAccess => "TensorGet/TensorSet indexed access",
        }
    }
}

/// Target or target family covered by a parity row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorParityTarget {
    RustRuntime,
    MirLowering,
    MirStepper,
    LlvmText,
    WasmText,
    MetalText,
    WgslText,
    SexpDump,
}

impl TensorParityTarget {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::RustRuntime => "rust-runtime",
            Self::MirLowering => "mir-lowering",
            Self::MirStepper => "mir-stepper",
            Self::LlvmText => "llvm-text",
            Self::WasmText => "wasm-text",
            Self::MetalText => "metal-text",
            Self::WgslText => "wgsl-text",
            Self::SexpDump => "sexp-dump",
        }
    }
}

/// Capability status for a tensor operation family on one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorParityStatus {
    Support,
    Rank1DeviceViewSupport,
    ImportProbeOnly,
    StructuralDump,
}

impl TensorParityStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Support => "support",
            Self::Rank1DeviceViewSupport => "rank-1 device-view support",
            Self::ImportProbeOnly => "import/probe-only",
            Self::StructuralDump => "structural dump",
        }
    }
}

pub const INDEXED_ACCESS_PARITY: &[TensorOperationParityRow] = &[
    TensorOperationParityRow {
        family: TensorOperationFamily::IndexedAccess,
        target: TensorParityTarget::RustRuntime,
        status: TensorParityStatus::Support,
        evidence: "crates/faber/src/tensor_test.rs::ponde_reports_out_of_bounds_and_negative_index",
    },
    TensorOperationParityRow {
        family: TensorOperationFamily::IndexedAccess,
        target: TensorParityTarget::MirLowering,
        status: TensorParityStatus::Support,
        evidence: "crates/radix/src/mir/lower_test.rs::lowers_tensor_bracket_access_to_tensor_runtime_ops",
    },
    TensorOperationParityRow {
        family: TensorOperationFamily::IndexedAccess,
        target: TensorParityTarget::MirStepper,
        status: TensorParityStatus::Support,
        evidence: "crates/radix/src/mir/stepper_test.rs::stepper_runs_tensor_indexed_access",
    },
    TensorOperationParityRow {
        family: TensorOperationFamily::IndexedAccess,
        target: TensorParityTarget::LlvmText,
        status: TensorParityStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol",
    },
    TensorOperationParityRow {
        family: TensorOperationFamily::IndexedAccess,
        target: TensorParityTarget::WasmText,
        status: TensorParityStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name",
    },
    TensorOperationParityRow {
        family: TensorOperationFamily::IndexedAccess,
        target: TensorParityTarget::MetalText,
        status: TensorParityStatus::Rank1DeviceViewSupport,
        evidence:
            "crates/radix/src/mir/metal_text_test.rs::indexed_tensor_view_metal_text_target_emits_output_view_copy",
    },
    TensorOperationParityRow {
        family: TensorOperationFamily::IndexedAccess,
        target: TensorParityTarget::WgslText,
        status: TensorParityStatus::Rank1DeviceViewSupport,
        evidence: "crates/radix/src/mir/wgsl_text_test.rs::indexed_tensor_view_wgsl_text_target_emits_output_view_copy",
    },
    TensorOperationParityRow {
        family: TensorOperationFamily::IndexedAccess,
        target: TensorParityTarget::SexpDump,
        status: TensorParityStatus::StructuralDump,
        evidence: "crates/radix/src/mir/dump.rs::collection_op",
    },
];

#[must_use]
pub fn indexed_access_parity() -> &'static [TensorOperationParityRow] {
    INDEXED_ACCESS_PARITY
}
