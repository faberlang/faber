//! Owning-crate exhaustiveness guard for [`CollectionKernelPlan`].
//!
//! Audit F1 (device-executor M3): `BatchedAttention` escaped python lower and
//! distributed validate because exhaustive matches over this enum live across
//! crates with no compile-time lock in the owner. Every later vocabulary
//! addition re-runs that risk.
//!
//! This test is the deny-unknown lock:
//! - [`variant_coverage`] is exhaustive — a new variant fails compilation
//!   until it is named here.
//! - [`KNOWN_VARIANTS`] is the closed name set — a new name that is not
//!   listed, or a listed name with no representative, is a red test.
//!
//! Match-family sites a new variant must visit in the same change:
//! python lower (`radix-mir-python/src/lower.rs`), dump (`dump.rs` /
//! collection-op dump arms), wasm `import_names` (`radix-mir-wasm`), nvvm
//! (`radix-mir-llvm/src/nvvm/`), distributed validate
//! (`distributed/validate_test.rs`), pipeline `shape_name` /
//! [`crate::device_program_plans::RecipeKind`].

use super::*;
use crate::abi::{PackedStorageLayout, ReducedProjection};

/// Closed variant-name set. Adding a [`CollectionKernelPlan`] variant
/// without listing it here is a red test once the exhaustive match compiles.
const KNOWN_VARIANTS: &[&str] = &[
    "Elementwise",
    "TiledMatMul",
    "TreeReduction",
    "Transpose",
    "AxisReduction",
    "RowSoftmax",
    "LayerNormalization",
    "Gather",
    "RmsNormalization",
    "Rope",
    "CausalMaskedSoftmax",
    "QuantizedMatMul",
    "GroupedMatMul",
    "BatchedAttention",
    "SsmConv1d",
    "SsmScan",
    "DistributedSum",
    "LaneGrid",
    "SingleLane",
    "GridReduce",
];

/// Exhaustive variant naming. A new [`CollectionKernelPlan`] variant
/// without an arm here fails compilation — that is the owning-crate F1 lock.
fn variant_coverage(plan: &CollectionKernelPlan) -> &'static str {
    match plan {
        CollectionKernelPlan::Elementwise => "Elementwise",
        CollectionKernelPlan::TiledMatMul(_) => "TiledMatMul",
        CollectionKernelPlan::TreeReduction(_) => "TreeReduction",
        CollectionKernelPlan::Transpose(_) => "Transpose",
        CollectionKernelPlan::AxisReduction(_) => "AxisReduction",
        CollectionKernelPlan::RowSoftmax(_) => "RowSoftmax",
        CollectionKernelPlan::LayerNormalization(_) => "LayerNormalization",
        CollectionKernelPlan::Gather(_) => "Gather",
        CollectionKernelPlan::RmsNormalization(_) => "RmsNormalization",
        CollectionKernelPlan::Rope(_) => "Rope",
        CollectionKernelPlan::CausalMaskedSoftmax(_) => "CausalMaskedSoftmax",
        CollectionKernelPlan::QuantizedMatMul(_) => "QuantizedMatMul",
        CollectionKernelPlan::GroupedMatMul(_) => "GroupedMatMul",
        CollectionKernelPlan::BatchedAttention(_) => "BatchedAttention",
        CollectionKernelPlan::SsmConv1d(_) => "SsmConv1d",
        CollectionKernelPlan::SsmScan(_) => "SsmScan",
        CollectionKernelPlan::DistributedSum(_) => "DistributedSum",
        CollectionKernelPlan::LaneGrid { .. } => "LaneGrid",
        CollectionKernelPlan::SingleLane => "SingleLane",
        CollectionKernelPlan::GridReduce { .. } => "GridReduce",
    }
}

fn dummy_shared() -> SharedMemoryLayout {
    SharedMemoryLayout {
        element_byte_width: 4,
        slot_count: 1,
        buffer_name: "wg".to_owned(),
    }
}

fn dummy_matmul() -> MatMulPlan {
    MatMulPlan {
        dims: crate::kernel_plan::MatMulDims::Literal { m: 1, k: 1, n: 1 },
        m: 1,
        k: 1,
        n: 1,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
        tile: 1,
        workgroup_x: 1,
        workgroup_y: 1,
        shared_memory: MatMulSharedMemory {
            shared_a: dummy_shared(),
            shared_b: dummy_shared(),
        },
        barriers: Vec::new(),
        oob_padding: OobPaddingPolicy::ZeroFill,
        batch: 1,
        right_batch: 1,
    }
}

fn dummy_reduction() -> ReductionPlan {
    ReductionPlan {
        op: ReduceOp::Sum,
        length: 1,
        workgroup_x: 1,
        partials: 1,
        shared_memory: dummy_shared(),
        barriers: Vec::new(),
        oob_padding: OobPaddingPolicy::ZeroFill,
    }
}

fn dummy_axis_reduction() -> AxisReductionPlan {
    AxisReductionPlan {
        op: ReduceOp::Sum,
        axis: 0,
        projection: ReducedProjection {
            axis_extent: 1,
            inner_stride: 1,
        },
    }
}

fn dummy_gather() -> GatherPlan {
    GatherPlan {
        table_rows: 1,
        table_cols: 1,
        id_count: 1,
    }
}

fn dummy_rms() -> RmsNormalizationPlan {
    RmsNormalizationPlan {
        dims: RowOpDims::Literal { rows: 1, width: 1 },
        axis: 0,
        epsilon_bits: 1,
        width: 1,
    }
}

fn dummy_rope() -> RopePlan {
    RopePlan {
        pos: 0,
        dim: 2,
        width: 2,
        per_row: false,
        rows: 0,
        rotate_half: false,
    }
}

fn dummy_quantized_matmul() -> QuantizedMatMulPlan {
    QuantizedMatMulPlan {
        m: 1,
        k: 1,
        n: 1,
        layout: PackedStorageLayout::Q8_0,
        tile: 1,
        workgroup_x: 1,
        workgroup_y: 1,
    }
}

fn dummy_grouped_matmul() -> GroupedMatMulPlan {
    GroupedMatMulPlan {
        m: 1,
        k: 1,
        n: 1,
        groups: 1,
        layout: PackedStorageLayout::Q8_0,
        tile: 1,
        workgroup_x: 1,
        workgroup_y: 1,
    }
}

fn dummy_batched_attention() -> BatchedAttentionPlan {
    BatchedAttentionPlan {
        kind: BatchedAttentionKind::Softmax,
        kv_heads: 1,
        q_per_kv: 1,
        seq: 1,
        head_dim: 1,
        hidden: 1,
        layout: None,
        rotate_half: false,
        output_strides: [1, 1, 1, 1],
        scale_bits: 0,
        decode_outputs_per_workgroup: None,
    }
}

/// One representative of every live variant, in [`KNOWN_VARIANTS`] order.
fn representative_plans() -> [CollectionKernelPlan; KNOWN_VARIANTS.len()] {
    [
        CollectionKernelPlan::Elementwise,
        CollectionKernelPlan::TiledMatMul(dummy_matmul()),
        CollectionKernelPlan::TreeReduction(dummy_reduction()),
        CollectionKernelPlan::Transpose(TransposePlan {
            m: 1,
            n: 1,
            workgroup_x: 1,
            dispatch_x: 1,
            batched: None,
        }),
        CollectionKernelPlan::AxisReduction(dummy_axis_reduction()),
        CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
            dims: RowOpDims::Literal { rows: 1, width: 1 },
            axis: 0,
            batched: None,
        }),
        CollectionKernelPlan::LayerNormalization(LayerNormalizationPlan {
            dims: RowOpDims::Literal { rows: 1, width: 1 },
            axis: 0,
            epsilon_bits: 0,
        }),
        CollectionKernelPlan::Gather(dummy_gather()),
        CollectionKernelPlan::RmsNormalization(dummy_rms()),
        CollectionKernelPlan::Rope(dummy_rope()),
        CollectionKernelPlan::CausalMaskedSoftmax(CausalMaskedSoftmaxPlan {
            dims: RowOpDims::Literal { rows: 1, width: 1 },
            rows: 1,
            cols: 1,
        }),
        CollectionKernelPlan::QuantizedMatMul(dummy_quantized_matmul()),
        CollectionKernelPlan::GroupedMatMul(dummy_grouped_matmul()),
        CollectionKernelPlan::BatchedAttention(dummy_batched_attention()),
        CollectionKernelPlan::SsmConv1d(SsmConv1dPlan {
            length: 1,
            channels: 1,
            kernel_width: 1,
            layout: PackedStorageLayout::F32,
        }),
        CollectionKernelPlan::SsmScan(SsmScanPlan {
            length: 1,
            state_dim: 1,
            layout: PackedStorageLayout::F32,
        }),
        CollectionKernelPlan::DistributedSum(dummy_distributed_sum()),
        CollectionKernelPlan::LaneGrid {
            source_resource: 0,
            rank: 2,
        },
        CollectionKernelPlan::SingleLane,
        CollectionKernelPlan::GridReduce {
            source_resource: 0,
            rank: 2,
            workgroup: super::GRID_REDUCE_WORKGROUP,
            partials_cap: super::GRID_REDUCE_PARTIALS_CAP,
            partial_layout: super::GridReducePartialLayout::for_op(crate::MirReduceOp::Max, 4, 2),
        },
    ]
}

fn dummy_distributed_sum() -> DistributedSumPlan {
    DistributedSumPlan {
        filum: crate::MirFilumReduceOp {
            op: crate::MirReduceOp::Sum,
            form: crate::MirFilumForm::Declared,
            term_ty: crate::MirType::semantic(radix_types::TypeId(0)),
            filum_binder: None,
            term_binder: crate::MirLocalId(0),
            term_binder_name: None,
            coordinates: Vec::new(),
            lane_width: 32,
            term_body: Vec::new(),
            term_captures: None,
            term_result: crate::MirLocalId(1),
        },
        length: 32,
        workgroup_x: 32,
        partials: 1,
        shared_memory: dummy_shared(),
        barriers: Vec::new(),
        oob_padding: super::OobPaddingPolicy::ZeroFill,
    }
}

#[test]
fn collection_kernel_plan_deny_unknown_variants() {
    let plans = representative_plans();
    assert_eq!(
        plans.len(),
        KNOWN_VARIANTS.len(),
        "representative set drifted from KNOWN_VARIANTS; update both in lockstep"
    );

    let mut seen = vec![false; KNOWN_VARIANTS.len()];
    for plan in &plans {
        let variant = variant_coverage(plan);
        let Some(index) = KNOWN_VARIANTS.iter().position(|name| *name == variant) else {
            panic!("unknown CollectionKernelPlan variant `{variant}`; add it to KNOWN_VARIANTS");
        };
        assert!(
            !seen[index],
            "duplicate representative for CollectionKernelPlan::{variant}",
        );
        seen[index] = true;
        assert_eq!(
            variant, KNOWN_VARIANTS[index],
            "representative order must match KNOWN_VARIANTS"
        );
    }

    let missing: Vec<&'static str> = KNOWN_VARIANTS
        .iter()
        .zip(seen)
        .filter_map(|(name, present)| (!present).then_some(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "KNOWN_VARIANTS entries with no representative: {missing:?}"
    );
}
