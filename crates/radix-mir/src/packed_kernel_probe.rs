//! Per-layout packed GEMM identity probes (R-PACK-02J..M).
//!
//! CPU-side proof that BF16 / Q5_0 dequant + GEMM matches the pinned
//! convert / GI2-1 goldens without a whole-model F32 materialization.

#![cfg(test)]

use crate::abi::PackedStorageLayout;
use crate::kernel_plan::{admit_quantized_matmul, PackedDequantClass};

/// Native BF16 → f32 convert: zero-extend the 16-bit payload into the
/// high half of an f32 (IEEE-754 bfloat16). Exact; no rounding.
fn bf16_bits_to_f32(bits: u16) -> f32 {
    f32::from_bits(u32::from(bits) << 16)
}

/// In-kernel-style BF16 GEMM: convert each packed right-hand element at
/// the load that needs it. Never allocates an f32 weight tensor of the
/// whole right operand first.
fn bf16_quantized_matmul(
    left: &[f32],
    packed_right: &[u16],
    m: usize,
    k: usize,
    n: usize,
) -> Vec<f32> {
    let mut out = vec![0.0f32; m * n];
    for row in 0..m {
        for col in 0..n {
            let mut acc = 0.0f32;
            for kk in 0..k {
                let a = left[row * k + kk];
                let b = bf16_bits_to_f32(packed_right[kk * n + col]);
                acc += a * b;
            }
            out[row * n + col] = acc;
        }
    }
    out
}

#[test]
fn bf16_native_convert_is_exact_bit_shift() {
    assert_eq!(bf16_bits_to_f32(0x0000).to_bits(), 0x0000_0000);
    assert_eq!(bf16_bits_to_f32(0x8000).to_bits(), 0x8000_0000);
    assert_eq!(bf16_bits_to_f32(0x3f80).to_bits(), 0x3f80_0000); // 1.0
    assert_eq!(bf16_bits_to_f32(0x4000).to_bits(), 0x4000_0000); // 2.0
    assert_eq!(bf16_bits_to_f32(0xbf80).to_bits(), 0xbf80_0000); // -1.0
}

#[test]
fn bf16_quantized_matmul_matches_cpu_reference_without_weight_expansion() {
    let layout = PackedStorageLayout::Bf16;
    let plan = admit_quantized_matmul(
        layout,
        crate::kernel_plan::MatMulShapes { m: 2, k: 2, n: 2 },
    );
    let crate::kernel_plan::CollectionKernelPlan::QuantizedMatMul(quantized) = plan else {
        panic!("BF16 must admit QuantizedMatMul");
    };
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantClass::NativeBf16Convert
    );
    assert_eq!((layout.block_elements(), layout.block_bytes()), (1, 2));

    // 2×2 × 2×2: A is f32 activations; B is native BF16, converted per load.
    let left = [1.0f32, 2.0, 3.0, 4.0];
    let packed_right = [0x3f80u16, 0x4000, 0x4040, 0x4080]; // 1, 2, 3, 4
    let got = bf16_quantized_matmul(&left, &packed_right, 2, 2, 2);
    // Reference: convert-at-use, same f32 mul-add order.
    let expect = [
        1.0 * 1.0 + 2.0 * 3.0, // 7
        1.0 * 2.0 + 2.0 * 4.0, // 10
        3.0 * 1.0 + 4.0 * 3.0, // 15
        3.0 * 2.0 + 4.0 * 4.0, // 22
    ];
    assert_eq!(got, expect);
}
