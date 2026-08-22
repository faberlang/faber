//! Packed `QuantizedGather` identity probe (R-PACK-02N).
//!
//! CPU-side proof that row gather + in-kernel dequant matches the GI2-1
//! goldens without a whole-table F32 materialization.

#![cfg(test)]

use crate::abi::PackedStorageLayout;
use crate::kernel_plan::{
    admit_quantized_gather, dequant_q8_0_block, AdmittedGather, PackedDequantKind,
};

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

/// Gather rows from a packed table by dequantizing only the addressed
/// blocks. Never allocates an f32 copy of the whole table first.
fn quantized_gather_rows(
    layout: PackedStorageLayout,
    packed: &[u8],
    table_cols: usize,
    ids: &[u32],
) -> Vec<f32> {
    let block_elems = layout.block_elements() as usize;
    let block_bytes = layout.block_bytes() as usize;
    let mut out = Vec::with_capacity(ids.len() * table_cols);
    for &id in ids {
        for col in 0..table_cols {
            let idx = id as usize * table_cols + col;
            let blk = idx / block_elems;
            let elem = idx % block_elems;
            let start = blk * block_bytes;
            let block = &packed[start..start + block_bytes];
            let values = match layout {
                PackedStorageLayout::Q8_0 => {
                    dequant_q8_0_block(block).expect("Q8_0 block").to_vec()
                }
                other => panic!("probe covers Q8_0; got {other:?}"),
            };
            out.push(values[elem]);
        }
    }
    out
}

#[test]
fn q8_0_quantized_gather_matches_gi2_1_goldens_without_table_expansion() {
    let layout = PackedStorageLayout::Q8_0;
    let admitted = admit_quantized_gather(layout, 2, 32, 2);
    let AdmittedGather::Quantized(plan) = admitted else {
        panic!("Q8_0 embedding table must admit QuantizedGather");
    };
    assert_eq!(plan.dequant.class, PackedDequantKind::Block);
    assert_eq!((layout.block_elements(), layout.block_bytes()), (32, 34));

    let goldens: Gi2Goldens = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/factory/gpu-inference-gguf/evidence/gi2-dequant-goldens.json"
    )))
    .expect("gi2 goldens parse");
    let fixture = goldens
        .block_fixtures
        .into_iter()
        .find(|block| block.kind == "Q8_0")
        .expect("at least one Q8_0 golden");
    let packed = parse_hex_bytes(&fixture.bytes_hex);
    assert_eq!(packed.len(), 34);
    // One packed row (32 logical elements). Gather id 0 twice — convert-at-use.
    let got = quantized_gather_rows(layout, &packed, 32, &[0, 0]);
    assert_eq!(got.len(), 64);
    for row in 0..2 {
        for (i, hex) in fixture.output_hex.iter().enumerate() {
            let expect = u32::from_str_radix(hex, 16).expect("output hex");
            assert_eq!(
                got[row * 32 + i].to_bits(),
                expect,
                "Q8_0 gathered row {row} elem {i}"
            );
        }
    }
}
