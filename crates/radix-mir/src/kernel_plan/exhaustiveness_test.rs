//! Owning-crate exhaustiveness guard for [`CollectionKernelPlan`].
//!
//! Audit F1 (device-executor M3): `BatchedAttention` escaped python lower and
//! distributed validate because exhaustive matches over this enum live across
//! crates with no compile-time lock in the owner. Every later vocabulary
//! addition re-runs that risk.
//!
//! This test is the deny-unknown lock:
//! - [`variant_coverage`] is exhaustive — a new variant fails compilation
//!   until it is named here against every known match family.
//! - [`KNOWN_VARIANTS`] is the closed name set — a new name that is not
//!   listed, or a listed name with no representative, is a red test.
//! - every family flag must be `true`. `false` is the F1 silent-escape
//!   posture and must not land.
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
];

/// Known match families that must name every [`CollectionKernelPlan`] variant.
/// Adding a family is a deliberate deny-unknown list change.
const MATCH_FAMILIES: &[&str] = &[
    "python_lower",
    "dump",
    "wasm_import_names",
    "nvvm",
    "distributed_validate",
    "pipeline_shape_name",
];

/// Per-variant coverage against the known match families.
struct FamilyCoverage {
    variant: &'static str,
    python_lower: bool,
    dump: bool,
    wasm_import_names: bool,
    nvvm: bool,
    distributed_validate: bool,
    pipeline_shape_name: bool,
}

impl FamilyCoverage {
    fn all_families_named(&self) -> bool {
        self.python_lower
            && self.dump
            && self.wasm_import_names
            && self.nvvm
            && self.distributed_validate
            && self.pipeline_shape_name
    }

    fn missing_families(&self) -> Vec<&'static str> {
        MATCH_FAMILIES
            .iter()
            .copied()
            .zip([
                self.python_lower,
                self.dump,
                self.wasm_import_names,
                self.nvvm,
                self.distributed_validate,
                self.pipeline_shape_name,
            ])
            .filter_map(|(family, covered)| (!covered).then_some(family))
            .collect()
    }
}

/// Exhaustive coverage row. A new [`CollectionKernelPlan`] variant without
/// an arm here fails compilation — that is the owning-crate F1 lock.
fn variant_coverage(plan: &CollectionKernelPlan) -> FamilyCoverage {
    match plan {
        CollectionKernelPlan::Elementwise => covered("Elementwise"),
        CollectionKernelPlan::TiledMatMul(_) => covered("TiledMatMul"),
        CollectionKernelPlan::TreeReduction(_) => covered("TreeReduction"),
        CollectionKernelPlan::Transpose(_) => covered("Transpose"),
        CollectionKernelPlan::AxisReduction(_) => covered("AxisReduction"),
        CollectionKernelPlan::RowSoftmax(_) => covered("RowSoftmax"),
        CollectionKernelPlan::LayerNormalization(_) => covered("LayerNormalization"),
        CollectionKernelPlan::Gather(_) => covered("Gather"),
        CollectionKernelPlan::RmsNormalization(_) => covered("RmsNormalization"),
        CollectionKernelPlan::Rope(_) => covered("Rope"),
        CollectionKernelPlan::CausalMaskedSoftmax(_) => covered("CausalMaskedSoftmax"),
        CollectionKernelPlan::QuantizedMatMul(_) => covered("QuantizedMatMul"),
        CollectionKernelPlan::GroupedMatMul(_) => covered("GroupedMatMul"),
        CollectionKernelPlan::BatchedAttention(_) => covered("BatchedAttention"),
        CollectionKernelPlan::SsmConv1d(_) => covered("SsmConv1d"),
        CollectionKernelPlan::SsmScan(_) => covered("SsmScan"),
        CollectionKernelPlan::DistributedSum(_) => covered("DistributedSum"),
    }
}

fn covered(variant: &'static str) -> FamilyCoverage {
    FamilyCoverage {
        variant,
        python_lower: true,
        dump: true,
        wasm_import_names: true,
        nvvm: true,
        distributed_validate: true,
        pipeline_shape_name: true,
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
            axis: 0,
            batched: None,
        }),
        CollectionKernelPlan::LayerNormalization(LayerNormalizationPlan { axis: 0 }),
        CollectionKernelPlan::Gather(dummy_gather()),
        CollectionKernelPlan::RmsNormalization(dummy_rms()),
        CollectionKernelPlan::Rope(dummy_rope()),
        CollectionKernelPlan::CausalMaskedSoftmax(CausalMaskedSoftmaxPlan { rows: 1, cols: 1 }),
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
    ]
}

fn dummy_distributed_sum() -> DistributedSumPlan {
    DistributedSumPlan {
        filum: crate::MirFilumSumOp {
            filum_binder: None,
            term_binder: crate::MirLocalId(0),
            term_binder_name: None,
            coordinates: Vec::new(),
            lane_width: 32,
            term_body: Vec::new(),
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
fn collection_kernel_plan_match_families_deny_unknown_variants() {
    let plans = representative_plans();
    assert_eq!(
        plans.len(),
        KNOWN_VARIANTS.len(),
        "representative set drifted from KNOWN_VARIANTS; update both in lockstep"
    );

    let mut seen = vec![false; KNOWN_VARIANTS.len()];
    for plan in &plans {
        let coverage = variant_coverage(plan);
        let Some(index) = KNOWN_VARIANTS
            .iter()
            .position(|name| *name == coverage.variant)
        else {
            panic!(
                "unknown CollectionKernelPlan variant `{}`; add it to KNOWN_VARIANTS \
                 and cover MATCH_FAMILIES {MATCH_FAMILIES:?}",
                coverage.variant
            );
        };
        assert!(
            coverage.all_families_named(),
            "CollectionKernelPlan::{} is uncovered in match families {:?}; \
             visit {MATCH_FAMILIES:?}",
            coverage.variant,
            coverage.missing_families()
        );
        assert!(
            !seen[index],
            "duplicate representative for CollectionKernelPlan::{}",
            coverage.variant
        );
        seen[index] = true;
        assert_eq!(
            coverage.variant, KNOWN_VARIANTS[index],
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
