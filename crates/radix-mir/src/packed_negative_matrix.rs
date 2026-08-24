//! Packed-kernel negative matrix (R-PACK-02P).
//!
//! Wrong layout, truncated block, and misaligned range must fail closed
//! with a typed diagnostic. Never silently repaired, never FNV-fallback'd.

#![cfg(test)]

use crate::abi::PackedStorageLayout;
use crate::kernel_plan::{
    admit_quantized_gather_from_ggml_type_id, admit_quantized_matmul,
    admit_quantized_matmul_from_ggml_type_id, dequant_packed_block, dequant_q4_k_block,
    dequant_q8_0_block, CollectionKernelPlan, MatMulShapes, PackedAdmissionError,
    PackedDequantError, QuantizedMatMulPlan, MATMUL_TILE,
};

fn shapes(m: u64, k: u64, n: u64) -> MatMulShapes {
    MatMulShapes {
        m,
        k,
        n,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    }
}

#[test]
fn wrong_layout_ggml_id_fails_closed() {
    for id in [2u32, 3, 7, 9, 15, 16, 40, 99] {
        let error = admit_quantized_matmul_from_ggml_type_id(id, shapes(8, 32, 32))
            .expect_err("unrepresentable layout must fail closed");
        assert_eq!(
            error,
            PackedAdmissionError::UnrepresentableLayout { ggml_type_id: id }
        );
        assert!(
            error.to_string().contains("unrepresentable"),
            "typed diagnostic, got {error}"
        );
        let gather = admit_quantized_gather_from_ggml_type_id(id, 4, 32, 2)
            .expect_err("unrepresentable gather layout must fail closed");
        assert_eq!(
            gather,
            PackedAdmissionError::UnrepresentableLayout { ggml_type_id: id }
        );
    }
}

#[test]
fn f32_quantized_matmul_plan_fails_closed() {
    let plan = CollectionKernelPlan::QuantizedMatMul(QuantizedMatMulPlan {
        m: 8,
        k: 32,
        n: 32,
        layout: PackedStorageLayout::F32,
        tile: MATMUL_TILE,
        workgroup_x: MATMUL_TILE,
        workgroup_y: MATMUL_TILE,
    });
    let error = plan
        .validate()
        .expect_err("F32 QuantizedMatMul must not be silently repaired to TiledMatMul");
    assert!(
        format!("{error:?}").contains("F32") || error.to_string().contains("F32"),
        "typed F32 fallback diagnostic, got {error:?}"
    );
}

#[test]
fn truncated_q8_0_block_fails_closed() {
    for actual in [0usize, 1, 16, 33, 33] {
        let error = dequant_q8_0_block(&vec![0u8; actual]).expect_err("truncated Q8_0");
        assert_eq!(
            error,
            PackedDequantError::BlockBytesMismatch {
                expected: 34,
                actual
            }
        );
        assert!(
            error.to_string().contains("34"),
            "typed length diagnostic, got {error}"
        );
    }
}

#[test]
fn truncated_q4_k_block_fails_closed() {
    for actual in [0usize, 34, 143, 145] {
        let error = dequant_q4_k_block(&vec![0u8; actual]).expect_err("truncated Q4_K");
        assert_eq!(
            error,
            PackedDequantError::BlockBytesMismatch {
                expected: 144,
                actual
            }
        );
    }
}

#[test]
fn truncated_superblock_via_packed_dequant_fails_closed() {
    let error = dequant_packed_block(PackedStorageLayout::Q5_K, &[0u8; 144])
        .expect_err("Q5_K fixture with Q4_K length must fail closed");
    assert_eq!(
        error,
        PackedDequantError::BlockBytesMismatch {
            expected: 176,
            actual: 144
        }
    );
    let error = dequant_packed_block(PackedStorageLayout::Q6_K, &[0u8; 176])
        .expect_err("Q6_K fixture with Q5_K length must fail closed");
    assert_eq!(
        error,
        PackedDequantError::BlockBytesMismatch {
            expected: 210,
            actual: 176
        }
    );
}

#[test]
fn wrong_layout_on_packed_block_fails_closed() {
    let q8 = [0u8; 34];
    let error = dequant_packed_block(PackedStorageLayout::Q4_K, &q8)
        .expect_err("Q8_0 bytes as Q4_K must fail closed");
    assert_eq!(
        error,
        PackedDequantError::BlockBytesMismatch {
            expected: 144,
            actual: 34
        }
    );
    let error =
        dequant_packed_block(PackedStorageLayout::F16, &q8).expect_err("no silent F16 reinterpret");
    assert_eq!(
        error,
        PackedDequantError::UnrepresentableLayout {
            layout: PackedStorageLayout::F16
        }
    );
    let error = dequant_packed_block(PackedStorageLayout::Bf16, &q8)
        .expect_err("no silent BF16 reinterpret");
    assert_eq!(
        error,
        PackedDequantError::UnrepresentableLayout {
            layout: PackedStorageLayout::Bf16
        }
    );
    let error = dequant_packed_block(PackedStorageLayout::F32, &q8)
        .expect_err("no silent F32 expansion path");
    assert_eq!(
        error,
        PackedDequantError::UnrepresentableLayout {
            layout: PackedStorageLayout::F32
        }
    );
    let error = dequant_packed_block(PackedStorageLayout::MXFP4, &q8)
        .expect_err("no silent MXFP4 reinterpret");
    assert_eq!(
        error,
        PackedDequantError::UnrepresentableLayout {
            layout: PackedStorageLayout::MXFP4
        }
    );
}

#[test]
fn misaligned_range_last_block_truncated_fails_closed() {
    // Claim 33 Q8_0 elements (not a 32-element multiple). One full block
    // (34 B) is present; the extra element needs a second block that is
    // missing / truncated.
    let packed = [0u8; 34];
    let logical = 33usize;
    let block_elems = PackedStorageLayout::Q8_0.block_elements() as usize;
    let block_bytes = PackedStorageLayout::Q8_0.block_bytes() as usize;
    assert!(
        !logical.is_multiple_of(block_elems),
        "fixture must be misaligned"
    );
    let last = logical - 1;
    let start = (last / block_elems) * block_bytes;
    let tail = packed.get(start..start + block_bytes);
    assert!(
        tail.is_none(),
        "misaligned tail must not silently yield a full block"
    );
    let error = dequant_q8_0_block(packed.get(start..).unwrap_or(&[]))
        .expect_err("truncated tail block must fail closed");
    assert_eq!(
        error,
        PackedDequantError::BlockBytesMismatch {
            expected: 34,
            actual: 0
        }
    );
}

#[test]
fn misaligned_q4_k_range_fails_closed() {
    // 257 logical elements need a second 144-byte superblock; only one
    // superblock is stored.
    let packed = [0u8; 144];
    let last = 256usize;
    let start = (last / 256) * 144;
    let error = dequant_q4_k_block(packed.get(start..).unwrap_or(&[]))
        .expect_err("misaligned Q4_K tail must fail closed");
    assert_eq!(
        error,
        PackedDequantError::BlockBytesMismatch {
            expected: 144,
            actual: 0
        }
    );
}

#[test]
fn packed_admission_does_not_fnv_fallback_unrepresentable_ids() {
    let error = admit_quantized_matmul_from_ggml_type_id(0x811c_9dc5, shapes(1, 32, 32))
        .expect_err("FNV-looking id must not fallback");
    assert_eq!(
        error,
        PackedAdmissionError::UnrepresentableLayout {
            ggml_type_id: 0x811c_9dc5
        }
    );
}

#[test]
fn aligned_packed_fixture_still_admits() {
    let plan = admit_quantized_matmul(PackedStorageLayout::Q8_0, shapes(2, 32, 32));
    assert!(
        matches!(plan, CollectionKernelPlan::QuantizedMatMul(_)),
        "positive control: aligned Q8_0 admits"
    );
    dequant_q8_0_block(&[0u8; 34]).expect("full Q8_0 block dequants");
}
