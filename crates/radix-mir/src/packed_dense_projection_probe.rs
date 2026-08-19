//! Dense-projection per-op identity through packed GEMM (R-PACK-02O).
//!
//! Attention QKV/O, FFN gate/up/down, and the logits head bind
//! `QuantizedMatMul` and match the GI2-1 dequant goldens + CPU reference
//! without a whole-weight F32 materialization.

#![cfg(test)]

use crate::abi::PackedStorageLayout;
use crate::kernel_plan::{
    admit_quantized_matmul, dequant_q8_0_block, gemm_f32, CollectionKernelPlan, MatMulShapes,
};

/// Named dense-rung projections (`SmolLM2` aspect, block-aligned probe dims).
struct DenseProjection {
    name: &'static str,
    m: u64,
    k: u64,
    n: u64,
}

const DENSE_PROJECTIONS: &[DenseProjection] = &[
    DenseProjection {
        name: "attn_q",
        m: 2,
        k: 32,
        n: 32,
    },
    DenseProjection {
        name: "attn_k",
        m: 2,
        k: 32,
        n: 32,
    },
    DenseProjection {
        name: "attn_v",
        m: 2,
        k: 32,
        n: 32,
    },
    DenseProjection {
        name: "attn_o",
        m: 2,
        k: 32,
        n: 32,
    },
    DenseProjection {
        name: "ffn_gate",
        m: 2,
        k: 32,
        n: 64,
    },
    DenseProjection {
        name: "ffn_up",
        m: 2,
        k: 32,
        n: 64,
    },
    DenseProjection {
        name: "ffn_down",
        m: 2,
        k: 64,
        n: 32,
    },
    DenseProjection {
        name: "logits",
        m: 2,
        k: 32,
        n: 64,
    },
];

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

/// Packed right operand GEMM: dequant one `Q8_0` block at the load that needs
/// it. Never allocates an f32 tensor of the whole right operand.
fn q8_0_projection_gemm(left: &[f32], packed: &[u8], m: usize, k: usize, n: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; m * n];
    for row in 0..m {
        for col in 0..n {
            let mut acc = 0.0f32;
            for kk in 0..k {
                let idx = kk * n + col;
                let blk = idx / 32;
                let elem = idx % 32;
                let start = blk * 34;
                let block = dequant_q8_0_block(&packed[start..start + 34]).expect("Q8_0 block");
                acc += left[row * k + kk] * block[elem];
            }
            out[row * n + col] = acc;
        }
    }
    out
}

fn q8_0_golden_block() -> (Vec<u8>, [f32; 32]) {
    let goldens: Gi2Goldens = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/factory/gpu-inference-gguf/evidence/gi2-dequant-goldens.json"
    )))
    .expect("gi2 goldens parse");
    let fixture = goldens
        .block_fixtures
        .into_iter()
        .find(|block| block.kind == "Q8_0")
        .expect("Q8_0 golden");
    let packed = parse_hex_bytes(&fixture.bytes_hex);
    let values = dequant_q8_0_block(&packed).expect("Q8_0 dequant");
    for (i, hex) in fixture.output_hex.iter().enumerate() {
        let expect = u32::from_str_radix(hex, 16).expect("output hex");
        assert_eq!(values[i].to_bits(), expect, "GI2-1 Q8_0 elem {i}");
    }
    (packed, values)
}

#[test]
fn dense_projections_admit_quantized_matmul_not_f32_tiled() {
    let layout = PackedStorageLayout::Q8_0;
    for projection in DENSE_PROJECTIONS {
        let plan = admit_quantized_matmul(
            layout,
            MatMulShapes {
                m: projection.m,
                k: projection.k,
                n: projection.n,
            },
        );
        let CollectionKernelPlan::QuantizedMatMul(quantized) = plan else {
            panic!(
                "{} must bind QuantizedMatMul, not a dense F32 fallback",
                projection.name
            );
        };
        assert_eq!(quantized.layout, layout, "{}", projection.name);
        assert_eq!(quantized.m, projection.m, "{}", projection.name);
        assert_eq!(quantized.k, projection.k, "{}", projection.name);
        assert_eq!(quantized.n, projection.n, "{}", projection.name);
    }
}

#[test]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn dense_projections_match_cpu_reference_without_weight_expansion() {
    let (block, values) = q8_0_golden_block();
    for projection in DENSE_PROJECTIONS {
        let m = projection.m as usize;
        let k = projection.k as usize;
        let n = projection.n as usize;
        let blocks = (k * n) / 32;
        let mut packed = Vec::with_capacity(blocks * 34);
        let mut right = Vec::with_capacity(k * n);
        for _ in 0..blocks {
            packed.extend_from_slice(&block);
            right.extend_from_slice(&values);
        }
        let left: Vec<f32> = (0..m * k)
            .map(|i| if i.is_multiple_of(k) { 1.0 } else { 0.0 })
            .collect();
        let got = q8_0_projection_gemm(&left, &packed, m, k, n);
        let expect = gemm_f32(&left, m, k, &right, n);
        assert_eq!(got.len(), expect.len(), "{}", projection.name);
        for (i, (g, e)) in got.iter().zip(expect.iter()).enumerate() {
            assert!(g.is_finite(), "{} out[{i}] not finite", projection.name);
            assert_eq!(
                g.to_bits(),
                e.to_bits(),
                "{} out[{i}] packed vs CPU",
                projection.name
            );
        }
    }
}
