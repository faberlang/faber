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

fn parse_hex_bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "odd hex length");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
        .collect()
}

fn parse_hex_f32(hex: &str) -> f32 {
    let bits = u32::from_str_radix(hex, 16).expect("hex f32 bits");
    f32::from_bits(bits)
}

#[test]
fn q6_k_gi2_goldens_match_cpu_reference() {
    let raw = include_str!(
        "../../../../docs/factory/gpu-inference-gguf/evidence/gi2-dequant-goldens.json"
    );
    let json: serde_json::Value = serde_json::from_str(raw).expect("gi2 goldens json");
    let fixtures = json["block_fixtures"]
        .as_array()
        .expect("block_fixtures array");
    let mut seen = 0usize;
    for fixture in fixtures {
        if fixture["type"].as_str() != Some("Q6_K") {
            continue;
        }
        seen += 1;
        let bytes = parse_hex_bytes(fixture["bytes_hex"].as_str().expect("bytes_hex"));
        let got = dequant_q6_k(&bytes).expect("Q6_K width");
        let want = fixture["output_hex"].as_array().expect("output_hex");
        assert_eq!(want.len(), 256, "{}", fixture["name"]);
        for (i, hex) in want.iter().enumerate() {
            let expected = parse_hex_f32(hex.as_str().expect("output hex"));
            assert_eq!(
                got[i].to_bits(),
                expected.to_bits(),
                "{}[{i}]",
                fixture["name"]
            );
        }
    }
    assert!(seen >= 3, "expected GI2-1 Q6_K block fixtures, got {seen}");
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

#[test]
fn q6_k_packed_gemm_probe_matches_cpu_reference() {
    let raw = include_str!(
        "../../../../docs/factory/gpu-inference-gguf/evidence/gi2-dequant-goldens.json"
    );
    let json: serde_json::Value = serde_json::from_str(raw).expect("gi2 goldens json");
    let fixture = json["block_fixtures"]
        .as_array()
        .expect("block_fixtures")
        .iter()
        .find(|row| row["type"].as_str() == Some("Q6_K"))
        .expect("at least one Q6_K fixture");
    let bytes = parse_hex_bytes(fixture["bytes_hex"].as_str().expect("bytes_hex"));
    let weights = dequant_q6_k(&bytes).expect("Q6_K block");
    let k = 256usize;
    let left = vec![1.0f32; k];
    let out = packed_gemm_f32(&left, &weights, 1, k, 1);
    let expected: f32 = weights.iter().copied().sum();
    assert_eq!(out[0].to_bits(), expected.to_bits());
}
