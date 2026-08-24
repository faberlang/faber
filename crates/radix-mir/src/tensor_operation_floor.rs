//! Code-owned tensor operation coverage floor for workload-shaped MIR work.
//!
//! TARGET: Stage 8 of the tensor systems timeline. This table records the
//! minimum operation families a workload can ask about before claiming MIR
//! tensor readiness. Rows are capability facts, not native target execution.

use super::MirCollectionOp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorOperationFloorRow {
    pub family: TensorOperationFloorFamily,
    pub target: TensorOperationFloorTarget,
    pub status: TensorOperationFloorStatus,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorOperationFloorFamily {
    Construction,
    FlattenOutput,
    Shape,
    IndexedAccess,
    ElementwiseArithmetic,
    Reductions,
    Rank2MatMul,
    /// Activation and domain-constrained unary ops (`ReLU`, Sqrt, Exp, Log, …)
    /// plus `MirCollectionOp` activation ops (e.g. `TensorGelu`).
    ///
    /// **Architecture note — `MirUnOp` vs `MirCollectionOp`:** Most activation
    /// ops are `MirUnOp` variants dispatched through `MirValueKind::Unary`.
    /// `TensorGelu` is a `MirCollectionOp` activation dispatched through
    /// `MirIntrinsic::Collection`. The floor family unifies both dispatch
    /// surfaces under one semantic category.
    Activations,
    /// Normalization ops dispatched through `MirCollectionOp`.
    ///
    /// Unlike `Activations` (whose ops are `MirUnOp` variants with empty
    /// `mir_ops()`), `Normalizations` contains real `MirCollectionOp`
    /// variants such as `TensorLayerNorm`.
    Normalizations,
    /// Gradient handle infrastructure for host-side gradient accumulation.
    ///
    /// Gradient handles provide create/accumulate/read/zero operations for
    /// accumulating parameter gradients on a host. Unlike other floor families
    /// (which track tensor operations), this family tracks gradient handle
    /// delivery across backends. The MIR runner implements gradient handles
    /// in-process (S5-U1); the LLVM and WGSL backends both implement gradient
    /// handles; Wasm is deferred.
    ///
    /// `mir_ops()` returns an empty slice — gradient handles are not `MirCollectionOp`
    /// variants. This family serves a documentation and tracking role, matching the
    /// pattern established by `Activations` (whose ops are `MirUnOp` variants).
    GradientHandles,
}

impl TensorOperationFloorFamily {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Construction => "construction/from-flat",
            Self::FlattenOutput => "flatten/output",
            Self::Shape => "shape",
            Self::IndexedAccess => "indexed access",
            Self::ElementwiseArithmetic => "elementwise arithmetic",
            Self::Reductions => "reductions",
            Self::Rank2MatMul => "rank-2 matmul",
            Self::Activations => "activations",
            Self::Normalizations => "normalizations",
            Self::GradientHandles => "gradient-handles",
        }
    }

    #[must_use]
    pub fn mir_ops(self) -> &'static [MirCollectionOp] {
        match self {
            Self::Construction => &[
                MirCollectionOp::TensorCreate,
                MirCollectionOp::TensorFromFlat,
            ],
            Self::FlattenOutput => &[MirCollectionOp::TensorFlatten],
            Self::Shape => &[MirCollectionOp::TensorShape],
            Self::IndexedAccess => &[MirCollectionOp::TensorGet, MirCollectionOp::TensorSet],
            Self::ElementwiseArithmetic => &[
                MirCollectionOp::TensorAdd,
                MirCollectionOp::TensorSub,
                MirCollectionOp::TensorMul,
                MirCollectionOp::TensorDiv,
            ],
            Self::Reductions => &[
                MirCollectionOp::Sum,
                MirCollectionOp::TensorMean,
                MirCollectionOp::TensorMax,
                MirCollectionOp::TensorMin,
            ],
            Self::Rank2MatMul => &[MirCollectionOp::TensorMatMul],
            // Most activation ops are MirUnOp variants (Relu, Sqrt, Exp, Log, …).
            // TensorGelu and TensorRelu are MirCollectionOp activations, tracked
            // here so the floor taxonomy classifies them correctly as activation
            // functions.
            Self::Activations => &[MirCollectionOp::TensorGelu, MirCollectionOp::TensorRelu],
            Self::Normalizations => &[
                MirCollectionOp::TensorLayerNorm,
                MirCollectionOp::TensorSoftmax,
            ],
            // Gradient handles are not MirCollectionOp variants. This family
            // exists for floor taxonomy and target tracking. The mir_ops()
            // slice is empty because the operations live on the host ABI
            // surface (createGradientBuffer, accumulateGradient, etc.).
            Self::GradientHandles => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorOperationFloorTarget {
    MirStepper,
    LlvmText,
    WasmText,
    SexpDump,
    NativeSystemsTargets,
}

impl TensorOperationFloorTarget {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::MirStepper => "mir-stepper",
            Self::LlvmText => "llvm-text",
            Self::WasmText => "wasm-text",
            Self::SexpDump => "sexp-dump",
            Self::NativeSystemsTargets => "native-systems-targets",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorOperationFloorStatus {
    MirExecutable,
    ImportProbeOnly,
    StructuralDump,
    CheckedDebt,
}

impl TensorOperationFloorStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MirExecutable => "mir-executable",
            Self::ImportProbeOnly => "import/probe-only",
            Self::StructuralDump => "structural dump",
            Self::CheckedDebt => "checked debt",
        }
    }
}

pub const REQUIRED_TENSOR_OPERATION_FLOOR_FAMILIES: &[TensorOperationFloorFamily] = &[
    TensorOperationFloorFamily::Construction,
    TensorOperationFloorFamily::FlattenOutput,
    TensorOperationFloorFamily::Shape,
    TensorOperationFloorFamily::IndexedAccess,
    TensorOperationFloorFamily::ElementwiseArithmetic,
    TensorOperationFloorFamily::Reductions,
    TensorOperationFloorFamily::Rank2MatMul,
    TensorOperationFloorFamily::Activations,
    TensorOperationFloorFamily::Normalizations,
    TensorOperationFloorFamily::GradientHandles,
];

pub const TENSOR_OPERATION_FLOOR_ROWS: &[TensorOperationFloorRow] = &[
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Construction,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_runs_tensor_shape_fixture",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Construction,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Construction,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Construction,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::StructuralDump,
        evidence: "crates/radix/src/mir/dump.rs::collection_op",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Construction,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-timeline/CAMPAIGN.md::Stage 9 - Kernel And Systems Target Lowering",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::FlattenOutput,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_runs_tensor_shape_fixture",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::FlattenOutput,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::FlattenOutput,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::FlattenOutput,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::StructuralDump,
        evidence: "crates/radix/src/mir/dump.rs::collection_op",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::FlattenOutput,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-timeline/CAMPAIGN.md::Stage 9 - Kernel And Systems Target Lowering",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Shape,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_runs_tensor_shape_fixture",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Shape,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Shape,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Shape,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::StructuralDump,
        evidence: "crates/radix/src/mir/dump.rs::collection_op",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Shape,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-timeline/CAMPAIGN.md::Stage 9 - Kernel And Systems Target Lowering",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::IndexedAccess,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix/src/mir/tensor_operation_parity.rs::INDEXED_ACCESS_PARITY",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::IndexedAccess,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/tensor_operation_parity.rs::INDEXED_ACCESS_PARITY",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::IndexedAccess,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/tensor_operation_parity.rs::INDEXED_ACCESS_PARITY",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::IndexedAccess,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::StructuralDump,
        evidence: "crates/radix/src/mir/tensor_operation_parity.rs::INDEXED_ACCESS_PARITY",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::IndexedAccess,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-timeline/CAMPAIGN.md::Stage 9 - Kernel And Systems Target Lowering",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::ElementwiseArithmetic,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_runs_tensor_arithmetic_elementwise_fixture",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::ElementwiseArithmetic,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::ElementwiseArithmetic,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::ElementwiseArithmetic,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::StructuralDump,
        evidence: "crates/radix/src/mir/dump.rs::collection_op",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::ElementwiseArithmetic,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-timeline/CAMPAIGN.md::Stage 9 - Kernel And Systems Target Lowering",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Reductions,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_runs_tensor_arithmetic_reduction_fixture",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Reductions,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Reductions,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Reductions,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::StructuralDump,
        evidence: "crates/radix/src/mir/dump.rs::collection_op",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Reductions,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-timeline/CAMPAIGN.md::Stage 9 - Kernel And Systems Target Lowering",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix/src/mir/runner_test.rs::runner_runs_tensor_arithmetic_matmul_fixture",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::StructuralDump,
        evidence: "crates/radix/src/mir/dump.rs::collection_op",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-timeline/CAMPAIGN.md::Stage 9 - Kernel And Systems Target Lowering",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Activations,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix-mir-runner — MirUnOp dispatch (ReLU, Sqrt, Exp, Log, …) + TensorGelu (MirCollectionOp) dispatch via runtime.rs tensor_gelu",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Activations,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/llvm_text/symbols.rs::runtime_symbol — activation runtime symbols are imported but not yet lowered",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Activations,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::ImportProbeOnly,
        evidence: "crates/radix/src/mir/wasm_text/import_names.rs::runtime_import_name — activation host imports are declared but not yet lowered",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Activations,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::StructuralDump,
        evidence: "crates/radix/src/mir/dump.rs — MirUnOp variants are dumped as unary op labels; activation ops follow the same pattern",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Activations,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-timeline/CAMPAIGN.md::Stage 9 - Kernel And Systems Target Lowering",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Normalizations,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix-mir-runner/src/runtime.rs — TensorLayerNorm dispatched via manual shape-loops (d-p-10-layernorm U2)",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Normalizations,
        target: TensorOperationFloorTarget::LlvmText,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/mir-swarm/delivery/d-p-10-layernorm.md::Non-goals — No device-native layernorm kernels; LLVM text lowering deferred",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Normalizations,
        target: TensorOperationFloorTarget::WasmText,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/mir-swarm/delivery/d-p-10-layernorm.md::Non-goals — No device-native layernorm kernels; Wasm text lowering deferred",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Normalizations,
        target: TensorOperationFloorTarget::SexpDump,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/mir-swarm/delivery/d-p-10-layernorm.md::Non-goals — LayerNorm is non-dumpable composite until s-expression policy exists",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::Normalizations,
        target: TensorOperationFloorTarget::NativeSystemsTargets,
        status: TensorOperationFloorStatus::CheckedDebt,
        evidence: "docs/factory/mir-swarm/delivery/d-p-10-layernorm.md::Non-goals — No device-native layernorm kernels; native systems targets deferred",
    },
    TensorOperationFloorRow {
        family: TensorOperationFloorFamily::GradientHandles,
        target: TensorOperationFloorTarget::MirStepper,
        status: TensorOperationFloorStatus::MirExecutable,
        evidence: "crates/radix-mir-runner/src/runtime.rs::eval_gradient — in-process gradient-handle dispatch (Create/Accumulate/Read/Zero over opaque handle storage); proof: crates/radix-mir-runner/src/lib_test.rs::runner_gradient_handle_create_accumulate_read_zero_round_trip",
    },
];

#[must_use]
pub fn required_tensor_operation_floor_families() -> &'static [TensorOperationFloorFamily] {
    REQUIRED_TENSOR_OPERATION_FLOOR_FAMILIES
}

#[must_use]
pub fn tensor_operation_floor_rows() -> &'static [TensorOperationFloorRow] {
    TENSOR_OPERATION_FLOOR_ROWS
}
