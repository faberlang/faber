//! Host-side packed dequant probes (R-PACK-02 layout bodies).
//!
//! These kernels are the CPU reference the per-layout emit units compare
//! against the GI2-1 goldens. They are not a device execution path and they
//! are not a whole-model F32 expansion: each call dequants one stored
//! block.

use super::packed::PackedDequantError;
use crate::abi::PackedStorageLayout;

/// IEEE-754 binary16 → binary32 (`ggml_compute_fp16_to_fp32`).
#[must_use]
pub fn half_to_f32(bits: u16) -> f32 {
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

/// `get_scale_min_k4` — 6-bit scale + 6-bit min from `scales[12]`.
#[must_use]
pub fn get_scale_min_k4(index: usize, scales: &[u8]) -> (u8, u8) {
    if index < 4 {
        (scales[index] & 63, scales[index + 4] & 63)
    } else {
        (
            (scales[index + 4] & 0x0f) | ((scales[index - 4] >> 6) << 4),
            (scales[index + 4] >> 4) | ((scales[index] >> 6) << 4),
        )
    }
}

/// `block_q5_K` (`d`, `dmin`, `scales[12]`, `qh[32]`, `qs[128]`) → 256 f32.
///
/// Mirrors `dequantize_row_q5_K` at the GI2-1 ggml pin (`a957b7747`).
///
/// # Errors
/// [`PackedDequantError::BlockBytesMismatch`] when `block` is not 176 bytes.
pub fn dequant_q5_k(block: &[u8]) -> Result<[f32; 256], PackedDequantError> {
    let want = PackedStorageLayout::Q5_K.block_bytes() as usize;
    if block.len() != want {
        return Err(PackedDequantError::BlockBytesMismatch {
            expected: PackedStorageLayout::Q5_K.block_bytes(),
            actual: block.len(),
        });
    }
    let d = half_to_f32(u16::from_le_bytes([block[0], block[1]]));
    let min = half_to_f32(u16::from_le_bytes([block[2], block[3]]));
    let scales = &block[4..16];
    let qh = &block[16..48];
    let qs = &block[48..176];
    let mut y = [0.0f32; 256];
    let mut u1: u8 = 1;
    let mut u2: u8 = 2;
    let mut ql_off = 0usize;
    let mut is = 0usize;
    let mut out = 0usize;
    for _pair in 0..4 {
        let (sc, m) = get_scale_min_k4(is, scales);
        let d1 = d * f32::from(sc);
        let m1 = min * f32::from(m);
        let (sc, m) = get_scale_min_k4(is + 1, scales);
        let d2 = d * f32::from(sc);
        let m2 = min * f32::from(m);
        for l in 0..32usize {
            let q = qs[ql_off + l];
            let lo = (q & 0x0f) + u8::from(qh[l] & u1 != 0) * 16;
            y[out + l] = d1 * f32::from(lo) - m1;
        }
        for l in 0..32usize {
            let q = qs[ql_off + l];
            let hi = (q >> 4) + u8::from(qh[l] & u2 != 0) * 16;
            y[out + 32 + l] = d2 * f32::from(hi) - m2;
        }
        ql_off += 32;
        is += 2;
        out += 64;
        u1 <<= 2;
        u2 <<= 2;
    }
    Ok(y)
}

/// `block_q6_K` (`ql[128]`, `qh[64]`, `scales[16]`, `d`) → 256 f32.
///
/// Mirrors `dequantize_row_q6_K` at the GI2-1 ggml pin (`a957b7747`).
///
/// # Errors
/// [`PackedDequantError::BlockBytesMismatch`] when `block` is not 210 bytes.
pub fn dequant_q6_k(block: &[u8]) -> Result<[f32; 256], PackedDequantError> {
    let want = PackedStorageLayout::Q6_K.block_bytes() as usize;
    if block.len() != want {
        return Err(PackedDequantError::BlockBytesMismatch {
            expected: PackedStorageLayout::Q6_K.block_bytes(),
            actual: block.len(),
        });
    }
    let d = half_to_f32(u16::from_le_bytes([block[208], block[209]]));
    let ql = &block[0..128];
    let qh = &block[128..192];
    let sc = &block[192..208];
    let mut y = [0.0f32; 256];
    for n in [0usize, 128] {
        let sc_base = n / 16;
        let ql_off = n / 2;
        let qh_off = n / 4;
        for l in 0..32usize {
            let is = l / 16;
            let q1 = i32::from((ql[ql_off + l] & 0x0f) | (((qh[qh_off + l] >> 0) & 3) << 4)) - 32;
            let q2 =
                i32::from((ql[ql_off + l + 32] & 0x0f) | (((qh[qh_off + l] >> 2) & 3) << 4)) - 32;
            let q3 = i32::from((ql[ql_off + l] >> 4) | (((qh[qh_off + l] >> 4) & 3) << 4)) - 32;
            let q4 =
                i32::from((ql[ql_off + l + 32] >> 4) | (((qh[qh_off + l] >> 6) & 3) << 4)) - 32;
            let sc0 = f32::from(sc[sc_base + is] as i8);
            let sc2 = f32::from(sc[sc_base + is + 2] as i8);
            let sc4 = f32::from(sc[sc_base + is + 4] as i8);
            let sc6 = f32::from(sc[sc_base + is + 6] as i8);
            y[n + l] = d * sc0 * f32::from(q1 as i8);
            y[n + l + 32] = d * sc2 * f32::from(q2 as i8);
            y[n + l + 64] = d * sc4 * f32::from(q3 as i8);
            y[n + l + 96] = d * sc6 * f32::from(q4 as i8);
        }
    }
    Ok(y)
}

/// Packed GEMM over already-dequantized right columns: `C[m,n] = A[m,k] · W[k,n]`.
///
/// The weight operand is the dequantized superblock (never a whole-model
/// F32 table). Used by the R-PACK-02 per-layout probes.
#[must_use]
pub fn packed_gemm_f32(left: &[f32], right: &[f32], m: usize, k: usize, n: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; m.saturating_mul(n)];
    for row in 0..m {
        for col in 0..n {
            let mut acc = 0.0f32;
            for kk in 0..k {
                acc += left[row * k + kk] * right[kk * n + col];
            }
            out[row * n + col] = acc;
        }
    }
    out
}

/// Crafted `Q5_K` superblock: `d = 1`, `dmin = 0`, every scale `1`, every
/// quant nibble `1`, high bits off → every logical element is `1.0`.
#[must_use]
pub fn synthetic_q5_k_ones_block() -> [u8; 176] {
    let mut block = [0u8; 176];
    // f16 1.0 = 0x3C00
    block[0] = 0x00;
    block[1] = 0x3c;
    // dmin = +0
    block[2] = 0x00;
    block[3] = 0x00;
    // scales[0..4] = 1, scales[4..8] = 0 (mins), scales[8..12] = 1
    for i in 0..4 {
        block[4 + i] = 1;
        block[12 + i] = 1;
    }
    // qs: low and high nibble both 1
    for byte in &mut block[48..176] {
        *byte = 0x11;
    }
    block
}

#[cfg(test)]
mod tests {
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
}
