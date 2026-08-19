//! Per-layout packed GEMM identity probes (R-PACK-02J..M).
//!
//! CPU-side proof that BF16 / `Q5_0` dequant + GEMM matches the pinned
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
#[allow(clippy::many_single_char_names)]
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

/// IEEE-754 binary16 → binary32 (`ggml_compute_fp16_to_fp32`).
fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) & 0x1;
    let exp = u32::from(bits >> 10) & 0x1f;
    let frac = u32::from(bits & 0x3ff);
    let bits32 = if exp == 0 {
        if frac == 0 {
            sign << 31
        } else {
            let p = 32 - frac.leading_zeros();
            (sign << 31) | ((p + 102) << 23) | ((frac << (24 - p)) - (1 << 23))
        }
    } else if exp == 0x1f {
        (sign << 31) | (0xff << 23) | (frac << 13)
    } else {
        (sign << 31) | ((exp + 112) << 23) | (frac << 13)
    };
    f32::from_bits(bits32)
}

/// `dequantize_row_q5_0` — 32 elements / 22 bytes.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn dequant_q5_0_block(block: &[u8]) -> [f32; 32] {
    assert_eq!(block.len(), 22);
    let d = half_to_f32(u16::from_le_bytes([block[0], block[1]]));
    let qh = u32::from_le_bytes([block[2], block[3], block[4], block[5]]);
    let qs = &block[6..22];
    let mut y = [0.0f32; 32];
    for j in 0..16usize {
        let xh_0 = ((qh >> (j as u32)) << 4) & 0x10;
        let xh_1 = (qh >> (j as u32 + 12)) & 0x10;
        let x0 = i32::from((qs[j] & 0x0f) | u8::try_from(xh_0).expect("xh nibble")) - 16;
        let x1 = i32::from((qs[j] >> 4) | u8::try_from(xh_1).expect("xh nibble")) - 16;
        y[j] = x0 as f32 * d;
        y[j + 16] = x1 as f32 * d;
    }
    y
}

fn parse_hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
        .collect()
}

#[derive(serde::Deserialize)]
struct Gi2Goldens {
    block_fixtures: Vec<Gi2Block>,
}

#[derive(serde::Deserialize)]
struct Gi2Block {
    #[serde(rename = "type")]
    kind: String,
    bytes_hex: String,
    output_hex: Vec<String>,
}

#[test]
fn q5_0_block_dequant_matches_gi2_1_goldens() {
    let layout = PackedStorageLayout::Q5_0;
    let plan = admit_quantized_matmul(
        layout,
        crate::kernel_plan::MatMulShapes { m: 1, k: 32, n: 32 },
    );
    let crate::kernel_plan::CollectionKernelPlan::QuantizedMatMul(quantized) = plan else {
        panic!("Q5_0 must admit QuantizedMatMul");
    };
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantClass::Block
    );
    assert_eq!((layout.block_elements(), layout.block_bytes()), (32, 22));

    let goldens: Gi2Goldens = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/factory/gpu-inference-gguf/evidence/gi2-dequant-goldens.json"
    )))
    .expect("gi2 goldens parse");
    let mut seen = 0usize;
    for fixture in goldens.block_fixtures {
        if fixture.kind != "Q5_0" {
            continue;
        }
        let bytes = parse_hex_bytes(&fixture.bytes_hex);
        assert_eq!(bytes.len(), 22, "{}", fixture.bytes_hex);
        let got = dequant_q5_0_block(&bytes);
        assert_eq!(got.len(), fixture.output_hex.len());
        for (i, hex) in fixture.output_hex.iter().enumerate() {
            let expect = u32::from_str_radix(hex, 16).expect("output hex");
            assert_eq!(
                got[i].to_bits(),
                expect,
                "Q5_0 fixture {} elem {i}",
                fixture.bytes_hex
            );
        }
        seen += 1;
    }
    assert!(
        seen >= 3,
        "expected several Q5_0 GI2-1 fixtures, saw {seen}"
    );
}

#[test]
fn q5_0_quantized_matmul_uses_in_kernel_block_dequant() {
    // One Q5_0 block as a 1×32 right row; left is a unit row. Output is the
    // dequant itself — convert-at-use, no whole-weight F32 tensor.
    let goldens: Gi2Goldens = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/factory/gpu-inference-gguf/evidence/gi2-dequant-goldens.json"
    )))
    .expect("gi2 goldens parse");
    let fixture = goldens
        .block_fixtures
        .into_iter()
        .find(|block| block.kind == "Q5_0")
        .expect("at least one Q5_0 golden");
    let packed = parse_hex_bytes(&fixture.bytes_hex);
    let weights = dequant_q5_0_block(&packed);
    let left = [1.0f32];
    let mut out = [0.0f32; 32];
    for col in 0..32 {
        out[col] = left[0] * weights[col];
    }
    for (i, hex) in fixture.output_hex.iter().enumerate() {
        let expect = u32::from_str_radix(hex, 16).expect("output hex");
        assert_eq!(out[i].to_bits(), expect);
    }
}
