#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
use super::PackedDequantError;
use super::*;

#[test]
fn q5_k_synthetic_ones_block_dequants_to_ones() {
    // First-failing-oracle: the f32-only substrate had no Q5_K dequant.
    let y = dequant_q5_k(&synthetic_q5_k_ones_block()).expect("Q5_K block width");
    assert_eq!(y.len(), 256);
    for (i, value) in y.iter().enumerate() {
        assert_eq!(*value, 1.0, "element {i}");
    }
}

#[test]
fn q5_k_refuses_wrong_block_width() {
    let err = dequant_q5_k(&[0u8; 16]).expect_err("width must fail closed");
    assert!(matches!(
        err,
        PackedDequantError::BlockBytesMismatch {
            expected: 176,
            actual: 16
        }
    ));
}

#[test]
fn q5_k_packed_gemm_probe_matches_cpu_reference() {
    let weights = dequant_q5_k(&synthetic_q5_k_ones_block()).expect("Q5_K block");
    let k = 256usize;
    let left = vec![1.0f32; k];
    let out = packed_gemm_f32(&left, &weights, 1, k, 1);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0], 256.0);
}

#[test]
fn q6_k_refuses_wrong_block_width() {
    let err = dequant_q6_k(&[0u8; 16]).expect_err("width must fail closed");
    assert!(matches!(
        err,
        PackedDequantError::BlockBytesMismatch {
            expected: 210,
            actual: 16
        }
    ));
}
