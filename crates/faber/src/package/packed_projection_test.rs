use super::*;

#[test]
fn dense_rung_projections_bind_q5_0_quantized_matmul() {
    for op in DENSE_PROJECTIONS {
        let plan = admit_packed_projection(*op, PackedStorageLayout::Q5_0);
        let quantized = packed_projection_plan(&plan).unwrap_or_else(|| {
            panic!("{} must bind QuantizedMatMul, not F32 TiledMatMul", op.name)
        });
        assert!(is_packed_quantized_matmul(&plan), "{}", op.name);
        assert_eq!(quantized.layout, PackedStorageLayout::Q5_0, "{}", op.name);
        assert_eq!((quantized.m, quantized.k, quantized.n), (op.m, op.k, op.n));
        assert!(
            (op.k * op.n).is_multiple_of(u64::from(PackedStorageLayout::Q5_0.block_elements())),
            "{} K·N must be a Q5_0 block multiple",
            op.name
        );
    }
}

#[test]
fn f32_projection_stays_tiled_matmul() {
    let plan = admit_packed_projection(DENSE_PROJECTIONS[0], PackedStorageLayout::F32);
    assert!(
        matches!(plan, CollectionKernelPlan::TiledMatMul(_)),
        "F32 is the dense bring-up fallback, not QuantizedMatMul"
    );
    assert!(!is_packed_quantized_matmul(&plan));
}
