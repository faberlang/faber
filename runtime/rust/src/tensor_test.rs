use super::{
    ERR_BROADCAST_SHAPE, ERR_CRUX_ENTROPIA_EMPTY_TENSOR, ERR_CRUX_ENTROPIA_NON_FINITE_INPUT,
    ERR_CRUX_ENTROPIA_SHAPE_MISMATCH, ERR_CRUX_ENTROPIA_TARGET_NON_FINITE,
    ERR_CRUX_ENTROPIA_TARGET_RANGE, ERR_DIVIDE_NON_FINITE_INPUT, ERR_DIVIDE_NON_FINITE_RESULT,
    ERR_DIVIDE_ZERO_DENOMINATOR, ERR_ELEMENT_COUNT_OVERFLOW, ERR_FORMA_LAYOUT_NOT_VIEWABLE,
    ERR_INDEX_OUT_OF_BOUNDS, ERR_LAYERNORM_AXIS_OUT_OF_RANGE, ERR_LAYERNORM_BETA_NON_FINITE,
    ERR_LAYERNORM_BETA_SHAPE_MISMATCH, ERR_LAYERNORM_EMPTY_TENSOR, ERR_LAYERNORM_EPSILON_INVALID,
    ERR_LAYERNORM_GAMMA_NON_FINITE, ERR_LAYERNORM_GAMMA_SHAPE_MISMATCH,
    ERR_LAYERNORM_NON_FINITE_INPUT, ERR_LAYERNORM_RANK_TOO_HIGH, ERR_MATMUL_ARGUMENT_RANK,
    ERR_MATMUL_BATCH_DIMENSION, ERR_MATMUL_INNER_DIMENSION, ERR_MATMUL_RECEIVER_RANK,
    ERR_MEDIA_EMPTY, ERR_NEGATIVE_INDEX, ERR_PERMUTE_AXIS_OUT_OF_RANGE, ERR_PERMUTE_DUPLICATE_AXIS,
    ERR_PERMUTE_NEGATIVE_AXIS, ERR_PERMUTE_RANK, ERR_SECTIO_INVALID_SLICE_BOUNDS,
    ERR_SOFTMAX_EMPTY_TENSOR, ERR_SOFTMAX_NON_FINITE_INPUT, ERR_SOFTMAX_RANK,
    ERR_TENSOR_COALESCE_REQUIRES_OPTIONAL, ERR_TENSOR_COPY_INTO_SHAPE_MISMATCH,
    ERR_TENSOR_EDGE_NOT_SHIFTED, ERR_TENSOR_EDGE_POLICY_INVALID, ERR_TENSOR_EDGE_RANK_MISMATCH,
    ERR_TENSOR_EDGE_READ_ONLY, ERR_TENSOR_EDGE_UNRESOLVED_READ, ERR_TENSOR_MATERIALIZE_UNRESOLVED,
    ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED, ERR_TRANSPOSE_RANK, Tensor, TensorEdgePolicy,
    tensor_flat_offset, tensor_shape_element_count, tensor_shape_has_element_count,
};
use std::sync::Arc;

#[test]
fn vacua_has_rank_zero() {
    let tensor: Tensor<f32> = Tensor::vacua();
    assert_eq!(tensor.longitudo(), 0);
    assert_eq!(tensor.element_count(), 1);
}

#[test]
fn crea_rejects_negative_shape_dimension() {
    let err = Tensor::<f32>::crea(&[-1, 0], 0.0).unwrap_err();
    assert_eq!(err, "tensor shape dimension must be non-negative");
}

#[test]
fn crea_rejects_overflowing_shape_product() {
    let err = Tensor::<f32>::crea(&[i64::MAX, 2], 0.0).unwrap_err();
    assert_eq!(err, ERR_ELEMENT_COUNT_OVERFLOW);
}

#[test]
fn tensor_shape_element_count_returns_some_for_valid_shape() {
    assert_eq!(tensor_shape_element_count(&[2, 3, 4]), Some(24));
}

#[test]
fn tensor_shape_element_count_rejects_negative_dimension() {
    assert_eq!(tensor_shape_element_count(&[-1, 4]), None);
}

#[test]
fn tensor_shape_element_count_rejects_overflow() {
    assert_eq!(tensor_shape_element_count(&[i64::MAX, i64::MAX]), None);
}

#[test]
fn tensor_shape_has_element_count_matches() {
    assert!(tensor_shape_has_element_count(&[2, 3], 6));
    assert!(!tensor_shape_has_element_count(&[2, 3], 5));
}

#[test]
fn error_element_count_overflow_string() {
    assert_eq!(ERR_ELEMENT_COUNT_OVERFLOW, "tensor element count overflow");
}

#[test]
fn tensor_flat_offset_checks_rank_bounds_and_overflow() {
    assert_eq!(tensor_flat_offset(&[2, 3], &[1, 2]), Some(5));
    assert_eq!(tensor_flat_offset(&[2, 3], &[2, 0]), None);
    assert_eq!(tensor_flat_offset(&[2, 3], &[0]), None);
    assert_eq!(tensor_flat_offset(&[2, 3], &[-1, 0]), None);
}

#[test]
fn ponde_writes_value_at_valid_index() {
    let mut tensor = Tensor::crea(&[2, 2], 0.0f32).expect("valid shape");
    assert!(tensor.ponde(&[0, 0], 1.0).is_ok());
    assert_eq!(tensor.accipe(&[0, 0]).expect("valid index"), Some(1.0));
}

#[test]
fn ponde_rejects_out_of_bounds_index() {
    let mut tensor = Tensor::crea(&[2, 2], 0.0f32).expect("valid shape");
    assert_eq!(
        tensor.ponde(&[9, 9], 9.0),
        Err("tensor index out of bounds")
    );
}

#[test]
fn ponde_rejects_negative_index() {
    let mut tensor = Tensor::crea(&[2, 2], 0.0f32).expect("valid shape");
    assert_eq!(
        tensor.ponde(&[-1, 0], 9.0),
        Err("tensor index must be non-negative")
    );
}

#[test]
fn accipe_rejects_negative_index() {
    let tensor = Tensor::crea(&[2, 2], 0.0f32).expect("valid shape");
    assert_eq!(
        tensor.accipe(&[-1, 0]),
        Err("tensor index must be non-negative")
    );
    assert_eq!(tensor.accipe(&[9, 9]).expect("in-range type"), None);
}

#[test]
fn structa_and_planata_round_trip() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).expect("shape matches data");
    assert_eq!(tensor.magnitudines(), vec![2, 2]);
    assert_eq!(tensor.planata().unwrap(), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn planata_flattens_a_standard_layout_and_strided_views_identically() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3])
        .expect("shape matches data");
    assert_eq!(
        tensor.planata().unwrap(),
        vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    );

    // [[1, 2, 3], [4, 5, 6]] transposed is [[1, 4], [2, 5], [3, 6]].
    let transposed = tensor.transpose_rank2().expect("rank-2 transpose");
    assert_eq!(
        transposed.planata().unwrap(),
        vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]
    );

    // A strides-only view with a nonzero base offset.
    let tail = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[4])
        .expect("shape matches data")
        .sectio(1, 4)
        .expect("valid slice");
    assert_eq!(tail.planata().unwrap(), vec![2.0, 3.0, 4.0]);
}

#[test]
fn reple_fills_a_standard_layout_in_place_and_through_view_strides() {
    let mut dense = Tensor::crea(&[2, 2], 1.0f32).expect("valid shape");
    let alias = dense.clone();
    dense.reple(7.5).expect("dense fill");
    assert_eq!(dense.planata().unwrap(), vec![7.5, 7.5, 7.5, 7.5]);
    // The fill stays in the shared storage: every handle observes it.
    assert_eq!(alias.planata().unwrap(), vec![7.5, 7.5, 7.5, 7.5]);

    // The view covers ordinals 0, 2, 4 of the parent; odd slots stay put.
    let mut parent =
        Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[6]).expect("shape matches data");
    let mut strided = parent.sectio_strided(0, 6, 2).expect("valid slice");
    strided.reple(0.0).expect("strided fill");
    assert_eq!(
        parent.planata().unwrap(),
        vec![0.0, 2.0, 0.0, 4.0, 0.0, 6.0]
    );
}

#[test]
fn reple_on_a_truncated_slice_view_writes_only_the_view() {
    let mut parent =
        Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[4]).expect("shape matches data");
    // Row-major from zero, yet only the first half of the shared buffer.
    let mut slice = parent.sectio(0, 2).expect("valid slice");
    slice.reple(9.0).expect("slice fill");
    assert_eq!(parent.planata().unwrap(), vec![9.0, 9.0, 3.0, 4.0]);
}

#[test]
fn structa_rejects_negative_shape_dimension() {
    let err = Tensor::structa(vec![1.0f32], &[-1]).unwrap_err();
    assert_eq!(err, "tensor shape dimension must be non-negative");
}

#[test]
fn structa_and_forma_infer_one_exact_shape_hole() {
    let tensor = Tensor::structa_with_holes(vec![0, 1, 2, 3, 4, 5], &[Some(2), None])
        .expect("element count determines the missing extent");
    assert_eq!(tensor.magnitudines(), vec![2, 3]);
    assert_eq!(tensor.planata().unwrap(), vec![0, 1, 2, 3, 4, 5]);

    let reshaped = tensor
        .forma_with_holes(&[None, Some(2)])
        .expect("reshape count determines the missing extent");
    assert_eq!(reshaped.magnitudines(), vec![3, 2]);
    assert_eq!(reshaped.planata().unwrap(), vec![0, 1, 2, 3, 4, 5]);
    assert!(Arc::ptr_eq(&tensor.data, &reshaped.data));
}

#[test]
fn shape_holes_reject_ambiguous_inexact_and_authored_negative_dimensions() {
    assert_eq!(
        Tensor::<i32>::structa_with_holes(vec![1, 2, 3, 4], &[None, None]).unwrap_err(),
        ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED
    );
    assert_eq!(
        Tensor::<i32>::structa_with_holes(Vec::new(), &[Some(0), None]).unwrap_err(),
        ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED
    );
    assert_eq!(
        Tensor::<i32>::structa_with_holes(vec![1, 2, 3, 4, 5, 6], &[Some(4), None]).unwrap_err(),
        "tensor element count does not match shape"
    );
    assert_eq!(
        Tensor::<i32>::structa_with_holes(vec![1, 2], &[Some(-1), None]).unwrap_err(),
        "tensor shape dimension must be non-negative"
    );
    let tensor = Tensor::structa(vec![1, 2, 3, 4], &[2, 2]).unwrap();
    assert_eq!(
        tensor.forma_with_holes(&[Some(-1), None]).unwrap_err(),
        "tensor shape dimension must be non-negative"
    );
}

#[test]
fn convert_elements_preserves_shape_and_maps_values() {
    let tensor = Tensor::structa(vec![1i64, 2, 3, 4], &[2, 2]).expect("shape matches data");
    let converted = tensor
        .convert_elements(|value| {
            // SAFETY: test values are small integers.
            #[allow(clippy::cast_precision_loss)]
            let value = value as f64;
            value
        })
        .expect("valid source values");
    assert_eq!(converted.magnitudines(), vec![2, 2]);
    assert_eq!(converted.planata().unwrap(), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn convert_elements_empty_tensor() {
    let tensor: Tensor<i64> = Tensor::structa(Vec::new(), &[0, 5]).expect("zero-extent shape");
    let converted: Tensor<f64> = tensor
        .convert_elements(|_| -> f64 {
            unreachable!("conversion closure is not called for an empty tensor")
        })
        .expect("empty source converts");
    assert_eq!(converted.magnitudines(), vec![0, 5]);
    assert_eq!(converted.planata().unwrap(), Vec::<f64>::new());
}

#[test]
fn sectio_slices_axis_zero() {
    let tensor = Tensor::crea(&[3, 2], 1.0f32).expect("valid shape");
    let slice = tensor.sectio(1, 3).expect("valid slice");
    assert_eq!(slice.longitudo(), 2);
    assert_eq!(slice.magnitudines(), vec![2, 2]);
}

#[test]
fn sectio_returns_axis_zero_view() {
    let mut tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[3, 2])
        .expect("shape matches data");
    let mut slice = tensor.sectio(1, 3).expect("valid slice");

    tensor.ponde(&[1, 0], 30.0).expect("parent write succeeds");
    assert_eq!(
        slice.accipe(&[0, 0]).expect("slice read succeeds"),
        Some(30.0)
    );

    slice.ponde(&[1, 1], 60.0).expect("slice write succeeds");
    assert_eq!(
        tensor.accipe(&[2, 1]).expect("parent read succeeds"),
        Some(60.0)
    );
}

#[test]
fn sectio_strided_returns_axis_zero_view() {
    let mut tensor = Tensor::structa(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &[5, 2]).unwrap();
    let mut slice = tensor.sectio_strided(1, 5, 2).unwrap();

    assert_eq!(slice.magnitudines(), vec![2, 2]);
    assert_eq!(slice.planata().unwrap(), vec![3, 4, 7, 8]);
    assert!(Arc::ptr_eq(&tensor.data, &slice.data));

    tensor.ponde(&[3, 1], 80).unwrap();
    assert_eq!(slice.accipe(&[1, 1]).unwrap(), Some(80));
    slice.ponde(&[0, 0], 30).unwrap();
    assert_eq!(tensor.accipe(&[1, 0]).unwrap(), Some(30));
}

#[test]
fn sectio_strided_rejects_nonpositive_steps_and_invalid_bounds() {
    let tensor = Tensor::structa(vec![1, 2, 3], &[3]).unwrap();

    assert_eq!(
        tensor.sectio_strided(0, 3, 0).unwrap_err(),
        ERR_SECTIO_INVALID_SLICE_BOUNDS
    );
    assert_eq!(
        tensor.sectio_strided(0, 3, -1).unwrap_err(),
        ERR_SECTIO_INVALID_SLICE_BOUNDS
    );
    assert_eq!(
        tensor.sectio_strided(0, 4, 1).unwrap_err(),
        "tensor index out of bounds"
    );
}

#[test]
fn expanded_inserts_an_axis_and_uses_a_zero_stride_view() {
    let mut tensor = Tensor::structa(vec![7, 9], &[2]).unwrap();
    let expanded = tensor.expanded(&[2, 2]).unwrap();

    assert_eq!(expanded.magnitudines(), vec![2, 2]);
    assert_eq!(expanded.planata().unwrap(), vec![7, 9, 7, 9]);
    assert!(Arc::ptr_eq(&tensor.data, &expanded.data));
    tensor.ponde(&[1], 90).unwrap();
    assert_eq!(expanded.accipe(&[1, 1]).unwrap(), Some(90));

    let copy = expanded
        .materialize()
        .expect("ordinary expanded view copies");
    assert!(!Arc::ptr_eq(&expanded.data, &copy.data));
    tensor.ponde(&[0], 70).unwrap();
    assert_eq!(copy.accipe(&[0, 0]).unwrap(), Some(7));
}

#[test]
fn expanded_stretches_a_singleton_axis_without_copying() {
    let tensor = Tensor::structa(vec![10, 20, 30], &[3, 1]).unwrap();

    let expanded = tensor.expanded(&[3, 4]).unwrap();

    assert_eq!(
        expanded.planata().unwrap(),
        vec![10, 10, 10, 10, 20, 20, 20, 20, 30, 30, 30, 30]
    );
    assert!(Arc::ptr_eq(&tensor.data, &expanded.data));
}

#[test]
fn expanded_maps_a_one_dimensional_row_to_the_rightmost_axis() {
    let tensor = Tensor::structa(vec![2, 4, 6], &[3]).unwrap();

    let expanded = tensor.expanded(&[2, 3]).unwrap();

    assert_eq!(expanded.planata().unwrap(), vec![2, 4, 6, 2, 4, 6]);
    assert!(Arc::ptr_eq(&tensor.data, &expanded.data));
}

#[test]
fn expanded_rejects_incompatible_or_negative_target_shapes() {
    let tensor = Tensor::structa(vec![1, 2, 3], &[3]).unwrap();

    assert_eq!(tensor.expanded(&[2, 2]).unwrap_err(), ERR_BROADCAST_SHAPE);
    assert_eq!(
        tensor.expanded(&[-1, 3]).unwrap_err(),
        "tensor shape dimension must be non-negative"
    );
}

#[test]
fn expanded_fills_holes_only_from_the_exact_shape_witness() {
    let tensor = Tensor::structa((0..64).collect::<Vec<i32>>(), &[64]).unwrap();
    let expanded = tensor
        .expanded_with_witness(&[None, Some(64)], &[3, 64])
        .expect("compiler-resolved batch extent fills the hole");

    assert_eq!(expanded.magnitudines(), vec![3, 64]);
    assert!(Arc::ptr_eq(&tensor.data, &expanded.data));
    let values = expanded.planata().unwrap();
    assert_eq!(&values[..64], &(0..64).collect::<Vec<i32>>());
    assert_eq!(&values[64..128], &(0..64).collect::<Vec<i32>>());
    assert_eq!(&values[128..], &(0..64).collect::<Vec<i32>>());

    let singleton_row = Tensor::structa((0..64).collect::<Vec<i32>>(), &[1, 64]).unwrap();
    let stretched = singleton_row
        .expanded_with_witness(&[None, Some(64)], &[3, 64])
        .expect("singleton source batch axis can stretch");
    assert!(Arc::ptr_eq(&singleton_row.data, &stretched.data));
    assert_eq!(stretched.planata().unwrap(), values);
    assert_eq!(
        singleton_row
            .expanded_with_witness(&[None, Some(64)], &[3, 63])
            .unwrap_err(),
        ERR_BROADCAST_SHAPE
    );
    assert_eq!(
        singleton_row
            .expanded_with_witness(&[Some(-1), Some(64)], &[3, 64])
            .unwrap_err(),
        "tensor shape dimension must be non-negative"
    );
}

#[test]
fn shift_is_a_shared_optional_view_and_unresolved_reads_stay_fallible() {
    let tensor = Tensor::structa(vec![10, 20, 30], &[3]).unwrap();
    let shifted = tensor.shift(&[1]).unwrap();

    assert!(Arc::ptr_eq(&tensor.data, &shifted.data));
    assert_eq!(shifted.accipe(&[0]).unwrap(), Some(20));
    assert_eq!(shifted.accipe(&[2]).unwrap(), None);
    assert_eq!(shifted.planata(), Err(ERR_TENSOR_EDGE_UNRESOLVED_READ));
    assert_eq!(
        shifted.materialize().unwrap_err(),
        ERR_TENSOR_MATERIALIZE_UNRESOLVED
    );
    let coalesced = shifted.coalesce(-1).unwrap();
    assert!(!Arc::ptr_eq(&tensor.data, &coalesced.data));
    assert_eq!(coalesced.planata().unwrap(), vec![20, 30, -1]);
    assert_eq!(
        tensor.coalesce(-1).unwrap_err(),
        ERR_TENSOR_COALESCE_REQUIRES_OPTIONAL
    );
    assert_eq!(
        tensor.shift(&[]).unwrap_err(),
        ERR_TENSOR_EDGE_RANK_MISMATCH
    );
    assert_eq!(
        tensor.limes(TensorEdgePolicy::Wrap).unwrap_err(),
        ERR_TENSOR_EDGE_NOT_SHIFTED
    );
}

#[test]
fn limes_builtin_policies_keep_views_and_resolve_boundaries() {
    let tensor = Tensor::structa(vec![10, 20, 30], &[3]).unwrap();
    let wrapped = tensor
        .shift(&[1])
        .unwrap()
        .limes(TensorEdgePolicy::Wrap)
        .unwrap();
    let clamped = tensor
        .shift(&[1])
        .unwrap()
        .limes(TensorEdgePolicy::Clamp)
        .unwrap();
    let reflected = tensor
        .shift(&[1])
        .unwrap()
        .limes(TensorEdgePolicy::Reflect)
        .unwrap();

    assert!(Arc::ptr_eq(&tensor.data, &wrapped.data));
    assert!(Arc::ptr_eq(&tensor.data, &clamped.data));
    assert!(Arc::ptr_eq(&tensor.data, &reflected.data));
    assert_eq!(wrapped.planata().unwrap(), vec![20, 30, 10]);
    assert_eq!(clamped.planata().unwrap(), vec![20, 30, 30]);
    assert_eq!(reflected.planata().unwrap(), vec![20, 30, 20]);

    let copy = wrapped.materialize().unwrap();
    assert!(!Arc::ptr_eq(&wrapped.data, &copy.data));
    assert_eq!(copy.planata().unwrap(), vec![20, 30, 10]);
}

#[test]
fn custom_limes_maps_and_transforms_only_displaced_boundary_values() {
    let tensor = Tensor::structa(vec![10, 20, 30], &[3]).unwrap();
    let custom = tensor
        .shift(&[1])
        .unwrap()
        .limes(TensorEdgePolicy::Custom {
            remap: |_| vec![0],
            transform: Some(|coordinate, value| {
                value + i32::try_from(coordinate[0]).expect("small shifted coordinate")
            }),
        })
        .unwrap();

    assert_eq!(custom.planata().unwrap(), vec![20, 30, 13]);
    assert!(Arc::ptr_eq(&tensor.data, &custom.data));

    let invalid = tensor
        .shift(&[1])
        .unwrap()
        .limes(TensorEdgePolicy::Custom {
            remap: |_| vec![99],
            transform: None,
        })
        .unwrap();
    assert_eq!(invalid.accipe(&[2]), Err(ERR_TENSOR_EDGE_POLICY_INVALID));
    assert_eq!(invalid.planata(), Err(ERR_TENSOR_EDGE_POLICY_INVALID));
    assert_eq!(
        invalid.materialize().unwrap_err(),
        ERR_TENSOR_EDGE_POLICY_INVALID
    );
    assert_eq!(invalid.summa(), Err(ERR_TENSOR_EDGE_POLICY_INVALID));
    assert_eq!(
        invalid.convert_elements(|value| value).unwrap_err(),
        ERR_TENSOR_EDGE_POLICY_INVALID
    );
    assert_eq!(
        invalid
            .addita(&Tensor::structa(vec![0, 0, 0], &[3]).unwrap())
            .unwrap_err(),
        ERR_TENSOR_EDGE_POLICY_INVALID
    );
    let mut invalid_write = invalid.clone();
    assert_eq!(invalid_write.ponde(&[0], 5), Err(ERR_TENSOR_EDGE_READ_ONLY));
    assert_eq!(invalid_write.reple(5), Err(ERR_TENSOR_EDGE_READ_ONLY));
}

#[test]
fn shifted_edge_metadata_composes_through_reshape_transpose_slice_and_expand() {
    let tensor = Tensor::structa(vec![1, 2, 3, 4], &[2, 2]).unwrap();
    let wrapped = tensor
        .shift(&[1, 0])
        .unwrap()
        .limes(TensorEdgePolicy::Wrap)
        .unwrap();
    let reshaped = wrapped.forma(&[4]).unwrap();
    let expanded = reshaped.expanded(&[2, 4]).unwrap();

    assert!(Arc::ptr_eq(&tensor.data, &expanded.data));
    assert_eq!(expanded.planata().unwrap(), vec![3, 4, 1, 2, 3, 4, 1, 2]);

    let matrix = Tensor::structa(vec![1, 2, 3, 4, 5, 6], &[2, 3]).unwrap();
    let sliced = matrix
        .shift(&[1, 0])
        .unwrap()
        .limes(TensorEdgePolicy::Wrap)
        .unwrap()
        .transpose_rank2()
        .unwrap()
        .sectio(1, 3)
        .unwrap();
    assert!(Arc::ptr_eq(&matrix.data, &sliced.data));
    assert_eq!(sliced.planata().unwrap(), vec![5, 2, 6, 3]);
}

#[test]
fn forma_reshapes_as_a_shared_view_and_rejects_unrepresentable_order() {
    let mut tensor = Tensor::structa(vec![1, 2, 3, 4, 5, 6], &[2, 3]).unwrap();
    let mut reshaped = tensor.forma(&[3, 2]).unwrap();

    assert_eq!(reshaped.planata().unwrap(), vec![1, 2, 3, 4, 5, 6]);
    assert!(Arc::ptr_eq(&tensor.data, &reshaped.data));
    tensor.ponde(&[1, 0], 40).unwrap();
    assert_eq!(reshaped.accipe(&[1, 1]).unwrap(), Some(40));
    reshaped.ponde(&[2, 1], 60).unwrap();
    assert_eq!(tensor.accipe(&[1, 2]).unwrap(), Some(60));

    let transposed = tensor.transpose_rank2().unwrap();
    assert_eq!(
        transposed.forma(&[6]).unwrap_err(),
        ERR_FORMA_LAYOUT_NOT_VIEWABLE
    );
}

#[test]
fn materialize_breaks_sectio_alias() {
    let mut tensor =
        Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).expect("shape matches data");
    let mut materialized = tensor
        .sectio(0, 1)
        .expect("valid slice")
        .materialize()
        .expect("ordinary slice copies");
    assert!(!Arc::ptr_eq(&tensor.data, &materialized.data));

    tensor.ponde(&[0, 0], 10.0).expect("parent write succeeds");
    assert_eq!(
        materialized
            .accipe(&[0, 0])
            .expect("materialized read succeeds"),
        Some(1.0)
    );

    materialized
        .ponde(&[0, 1], 20.0)
        .expect("materialized write succeeds");
    assert_eq!(
        tensor.accipe(&[0, 1]).expect("parent read succeeds"),
        Some(2.0)
    );
}

#[test]
fn tensor_is_send_sync_when_elements_are() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Tensor<f32>>();
}

#[test]
fn sectio_rejects_negative_bounds() {
    let tensor = Tensor::crea(&[3, 2], 1.0f32).expect("valid shape");
    assert_eq!(
        tensor.sectio(-1, 2).unwrap_err(),
        "tensor slice bounds must be non-negative"
    );
    assert_eq!(
        tensor.sectio(2, 1).unwrap_err(),
        "tensor slice end must be at least start"
    );
}

#[test]
fn sectio_rejects_out_of_bounds_start() {
    let tensor = Tensor::crea(&[3, 2], 1.0f32).expect("valid shape");
    assert_eq!(
        tensor.sectio(3, 4).unwrap_err(),
        "tensor index out of bounds"
    );
    assert_eq!(
        tensor.sectio(10, 15).unwrap_err(),
        "tensor index out of bounds"
    );
}

#[test]
fn addita_sums_elementwise() {
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();
    let b = Tensor::structa(vec![10.0f32, 20.0, 30.0], &[3]).unwrap();
    let c = a.addita(&b).expect("broadcast-compatible shape");
    assert_eq!(c.magnitudines(), vec![3]);
    assert_eq!(c.planata().unwrap(), vec![11.0, 22.0, 33.0]);
}

#[test]
fn addita_broadcasts_size_one_dimension() {
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let b = Tensor::structa(vec![10.0f32, 20.0], &[2, 1]).unwrap();
    let c = a.addita(&b).expect("broadcast-compatible shape");
    assert_eq!(c.magnitudines(), vec![2, 2]);
    // a = [[1,2],[3,4]]; b = [[10],[20]] broadcasts to [[10,10],[20,20]].
    assert_eq!(c.planata().unwrap(), vec![11.0, 12.0, 23.0, 24.0]);
}

#[test]
fn addita_rejects_broadcast_shape_mismatch() {
    let a = Tensor::structa(vec![1.0f32, 2.0], &[2]).unwrap();
    let b = Tensor::structa(vec![10.0f32, 20.0, 30.0], &[3]).unwrap();
    assert_eq!(a.addita(&b).unwrap_err(), ERR_BROADCAST_SHAPE);
}

#[test]
fn addita_broadcasts_zero_extent_with_size_one_axis_to_empty_result() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[0, 3]).unwrap();
    let row = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[1, 3]).unwrap();

    let result = empty
        .addita(&row)
        .expect("zero/one broadcast is compatible");

    assert_eq!(result.magnitudines(), vec![0, 3]);
    assert_eq!(result.planata().unwrap(), Vec::<f32>::new());
}

#[test]
fn subtrahe_broadcasts_zero_extent_with_size_one_axis_to_empty_result() {
    let row = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[1, 3]).unwrap();
    let empty = Tensor::<f32>::structa(Vec::new(), &[0, 3]).unwrap();

    let result = row
        .subtrahe(&empty)
        .expect("one/zero broadcast is compatible");

    assert_eq!(result.magnitudines(), vec![0, 3]);
    assert_eq!(result.planata().unwrap(), Vec::<f32>::new());
}

#[test]
fn multiplica_broadcasts_zero_extent_with_size_one_axis_to_empty_result() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[2, 0, 3]).unwrap();
    let lane = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[1, 1, 3]).unwrap();

    let result = empty
        .multiplica(&lane)
        .expect("zero/one broadcast is compatible");

    assert_eq!(result.magnitudines(), vec![2, 0, 3]);
    assert_eq!(result.planata().unwrap(), Vec::<f32>::new());
}

#[test]
fn zero_extent_addita_rejects_non_one_mismatch() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[0, 3]).unwrap();
    let rows = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();

    assert_eq!(empty.addita(&rows).unwrap_err(), ERR_BROADCAST_SHAPE);
}

#[test]
fn zero_extent_subtrahe_rejects_non_one_mismatch() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[0, 3]).unwrap();
    let rows = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();

    assert_eq!(empty.subtrahe(&rows).unwrap_err(), ERR_BROADCAST_SHAPE);
}

#[test]
fn zero_extent_multiplica_rejects_non_one_mismatch() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[0, 3]).unwrap();
    let rows = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();

    assert_eq!(empty.multiplica(&rows).unwrap_err(), ERR_BROADCAST_SHAPE);
}

#[test]
fn subtrahe_is_elementwise() {
    let a = Tensor::structa(vec![10.0f32, 20.0, 30.0], &[3]).unwrap();
    let b = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();
    assert_eq!(
        a.subtrahe(&b)
            .expect("broadcast-compatible shape")
            .planata()
            .unwrap(),
        vec![9.0, 18.0, 27.0]
    );
}

#[test]
fn multiplica_is_elementwise() {
    let a = Tensor::structa(vec![10.0f32, 20.0, 30.0], &[3]).unwrap();
    let b = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();
    assert_eq!(
        a.multiplica(&b)
            .expect("broadcast-compatible shape")
            .planata()
            .unwrap(),
        vec![10.0, 40.0, 90.0]
    );
}

#[test]
fn addita_integer_tensors_sum_without_widening() {
    let a = Tensor::structa(vec![1i64, 2, 3], &[3]).unwrap();
    let b = Tensor::structa(vec![4i64, 5, 6], &[3]).unwrap();
    assert_eq!(
        a.addita(&b)
            .expect("broadcast-compatible shape")
            .planata()
            .unwrap(),
        vec![5, 7, 9]
    );
}

#[test]
#[allow(clippy::float_cmp)]
fn summa_folds_all_elements_to_element_type() {
    let grid = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    assert_eq!(grid.summa().unwrap(), 10.0);
    let ints = Tensor::structa(vec![1i64, 2, 3, 4], &[4]).unwrap();
    assert_eq!(ints.summa().unwrap(), 10);
}

#[test]
#[allow(clippy::float_cmp)]
fn summa_empty_tensor_returns_default() {
    let empty_f32 = Tensor::<f32>::structa(Vec::new(), &[0]).unwrap();
    assert_eq!(empty_f32.summa().unwrap(), 0.0);

    let empty_ints = Tensor::<i64>::structa(Vec::new(), &[0, 0]).unwrap();
    assert_eq!(empty_ints.summa().unwrap(), 0);
}

#[test]
fn neg_negates_f32_elements_and_preserves_shape() {
    let tensor = Tensor::structa(vec![1.0f32, -2.0, 0.0, 4.5], &[2, 2]).unwrap();

    let negated = tensor.neg().unwrap();

    assert_eq!(negated.magnitudines(), vec![2, 2]);
    assert_eq!(negated.planata().unwrap(), vec![-1.0, 2.0, -0.0, -4.5]);
}

#[test]
fn neg_empty_tensor_preserves_shape() {
    let tensor = Tensor::<f32>::structa(Vec::new(), &[0, 3]).unwrap();

    let negated = tensor.neg().unwrap();

    assert_eq!(negated.magnitudines(), vec![0, 3]);
    assert_eq!(negated.planata().unwrap(), Vec::<f32>::new());
}

#[test]
fn scala_scales_f32_elements_and_preserves_shape() {
    let tensor = Tensor::structa(vec![1.0f32, -2.0, 3.5, 4.0], &[2, 2]).unwrap();

    let scaled = tensor.scala(0.5).unwrap();

    assert_eq!(scaled.magnitudines(), vec![2, 2]);
    assert_eq!(scaled.planata().unwrap(), vec![0.5, -1.0, 1.75, 2.0]);
}

#[test]
fn scala_empty_tensor_preserves_shape() {
    let tensor = Tensor::<f32>::structa(Vec::new(), &[0, 2]).unwrap();

    let scaled = tensor.scala(2.0).unwrap();

    assert_eq!(scaled.magnitudines(), vec![0, 2]);
    assert_eq!(scaled.planata().unwrap(), Vec::<f32>::new());
}

#[test]
fn divide_broadcasts_finite_f32_tensors() {
    let lhs = Tensor::structa(vec![8.0f32, 18.0, -24.0, 40.0], &[2, 2]).unwrap();
    let rhs = Tensor::structa(vec![2.0f32, -4.0], &[2, 1]).unwrap();

    let divided = lhs.divide(&rhs).expect("finite broadcast division");

    assert_eq!(divided.magnitudines(), vec![2, 2]);
    assert_eq!(divided.planata().unwrap(), vec![4.0, 9.0, 6.0, -10.0]);
}

#[test]
fn reciproca_preserves_shape_and_values() {
    let tensor = Tensor::structa(vec![2.0f32, -4.0, 0.25, 8.0], &[2, 2]).unwrap();

    let reciprocal = tensor.reciproca().expect("finite reciprocal");

    assert_eq!(reciprocal.magnitudines(), vec![2, 2]);
    assert_eq!(reciprocal.planata().unwrap(), vec![0.5, -0.25, 4.0, 0.125]);
}

#[test]
fn reciproca_rejects_zero_denominator() {
    let zero = Tensor::structa(vec![1.0f32, 0.0], &[2]).unwrap();
    assert_eq!(zero.reciproca().unwrap_err(), ERR_DIVIDE_ZERO_DENOMINATOR);
}

#[test]
fn divide_rejects_zero_denominator_without_materializing_infinity() {
    let lhs = Tensor::structa(vec![1.0f32, -2.0], &[2]).unwrap();
    let rhs = Tensor::structa(vec![1.0f32, -0.0], &[2]).unwrap();

    assert_eq!(lhs.divide(&rhs).unwrap_err(), ERR_DIVIDE_ZERO_DENOMINATOR);
}

#[test]
fn divide_rejects_non_finite_inputs_before_dividing() {
    let lhs = Tensor::structa(vec![1.0f32, f32::INFINITY], &[2]).unwrap();
    let rhs = Tensor::structa(vec![1.0f32, 2.0], &[2]).unwrap();
    assert_eq!(lhs.divide(&rhs).unwrap_err(), ERR_DIVIDE_NON_FINITE_INPUT);

    let lhs = Tensor::structa(vec![1.0f32], &[]).unwrap();
    let rhs = Tensor::structa(vec![f32::NAN], &[]).unwrap();
    assert_eq!(lhs.divide(&rhs).unwrap_err(), ERR_DIVIDE_NON_FINITE_INPUT);
}

#[test]
fn divide_rejects_non_finite_results() {
    let lhs = Tensor::structa(vec![f32::MAX], &[]).unwrap();
    let rhs = Tensor::structa(vec![f32::MIN_POSITIVE], &[]).unwrap();

    assert_eq!(lhs.divide(&rhs).unwrap_err(), ERR_DIVIDE_NON_FINITE_RESULT);
}

#[test]
fn divide_rejects_broadcast_shape_mismatch() {
    let lhs = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let rhs = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();

    assert_eq!(lhs.divide(&rhs).unwrap_err(), ERR_BROADCAST_SHAPE);
}

#[test]
fn addita_zips_same_shape_standard_layouts() {
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).expect("shape matches data");
    let b = Tensor::structa(vec![10.0f32, 20.0, 30.0, 40.0], &[2, 2]).expect("shape matches data");
    let c = a.addita(&b).expect("same shape");
    assert_eq!(c.planata().unwrap(), vec![11.0, 22.0, 33.0, 44.0]);
}

#[test]
fn addita_of_a_tensor_with_itself_reads_the_shared_buffer_under_one_guard() {
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).expect("shape matches data");
    let c = a.addita(&a).expect("same tensor");
    assert_eq!(c.planata().unwrap(), vec![2.0, 4.0, 6.0, 8.0]);
}

#[test]
fn addita_walks_a_strided_view_against_a_broadcast_operand() {
    // lhs = [[1, 4], [2, 5], [3, 6]]; [100, 10] broadcasts over the columns.
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3])
        .expect("shape matches data");
    let lhs = a.transpose_rank2().expect("rank-2 transpose");
    let b = Tensor::structa(vec![100.0f32, 10.0], &[2]).expect("shape matches data");
    let c = lhs.addita(&b).expect("broadcast-compatible shape");
    assert_eq!(c.magnitudines(), vec![3, 2]);
    assert_eq!(
        c.planata().unwrap(),
        vec![101.0, 14.0, 102.0, 15.0, 103.0, 16.0]
    );
}

#[test]
fn divide_divides_same_shape_standard_layouts() {
    let a = Tensor::structa(vec![8.0f32, 6.0, 4.0, 2.0], &[2, 2]).expect("shape matches data");
    let b = Tensor::structa(vec![2.0f32, 3.0, 4.0, 4.0], &[2, 2]).expect("shape matches data");
    let c = a.divide(&b).expect("same shape");
    assert_eq!(c.planata().unwrap(), vec![4.0, 2.0, 1.0, 0.5]);
}

#[test]
fn divide_walks_a_strided_view_and_keeps_the_first_row_major_fault() {
    // lhs = [[1, 3], [2, 4]]; [2, 4] broadcasts over the columns.
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).expect("shape matches data");
    let lhs = a.transpose_rank2().expect("rank-2 transpose");
    let b = Tensor::structa(vec![2.0f32, 4.0], &[2]).expect("shape matches data");
    let c = lhs.divide(&b).expect("broadcast-compatible shape");
    assert_eq!(c.planata().unwrap(), vec![0.5, 0.75, 1.0, 1.0]);

    // [[1, 0], [2, 0]] transposed is [[1, 2], [0, 0]]: the first zero
    // denominator sits at row-major ordinal 2 and wins by name.
    let faults = Tensor::structa(vec![1.0f32, 0.0, 2.0, 0.0], &[2, 2]).expect("shape matches data");
    let denominators = faults.transpose_rank2().expect("rank-2 transpose");
    let numerator = Tensor::crea(&[2, 2], 4.0f32).expect("valid shape");
    assert_eq!(
        numerator.divide(&denominators).unwrap_err(),
        ERR_DIVIDE_ZERO_DENOMINATOR
    );
}

#[test]
#[allow(clippy::float_cmp)]
fn media_averages_f32_elements() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();

    assert_eq!(tensor.media().unwrap(), 2.5);
}

#[test]
fn media_rejects_empty_tensor() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[0]).unwrap();

    assert_eq!(empty.media().unwrap_err(), ERR_MEDIA_EMPTY);
}

#[test]
fn transpose_rank2_materializes_rows_as_columns() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();

    let transposed = tensor.transpose_rank2().expect("rank-2 transpose");

    assert_eq!(transposed.magnitudines(), vec![3, 2]);
    assert_eq!(
        transposed.planata().unwrap(),
        vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]
    );
}

#[test]
fn transpose_rank2_returns_a_shared_view() {
    let mut tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[3, 2]).unwrap();
    let view = tensor.sectio(1, 3).expect("axis-0 view");
    let transposed = view.transpose_rank2().expect("rank-2 view transpose");

    assert!(Arc::ptr_eq(&tensor.data, &transposed.data));
    tensor.ponde(&[1, 0], 99.0).unwrap();

    assert_eq!(transposed.magnitudines(), vec![2, 2]);
    assert_eq!(transposed.planata().unwrap(), vec![99.0, 5.0, 4.0, 6.0]);
}

#[test]
fn transpose_rank2_swaps_only_the_trailing_axes_of_batched_tensors() {
    let tensor = Tensor::structa((0..12).collect::<Vec<i32>>(), &[2, 2, 3]).unwrap();

    let transposed = tensor.transpose_rank2().unwrap();

    assert_eq!(transposed.magnitudines(), vec![2, 3, 2]);
    assert_eq!(
        transposed.planata().unwrap(),
        vec![0, 3, 1, 4, 2, 5, 6, 9, 7, 10, 8, 11]
    );
    assert!(Arc::ptr_eq(&tensor.data, &transposed.data));
}

#[test]
fn transpose_rank2_preserves_the_rank_at_least_two_static_contract() {
    let tensor = Tensor::structa((0..12).collect::<Vec<i32>>(), &[1, 1, 2, 2, 3]).unwrap();

    let transposed = tensor.transpose_rank2().unwrap();

    assert_eq!(transposed.magnitudines(), vec![1, 1, 2, 3, 2]);
    assert_eq!(
        transposed.planata().unwrap(),
        vec![0, 3, 1, 4, 2, 5, 6, 9, 7, 10, 8, 11]
    );
    assert!(Arc::ptr_eq(&tensor.data, &transposed.data));
}

#[test]
fn transpose_rank2_rejects_rank_below_two() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();

    assert_eq!(tensor.transpose_rank2().unwrap_err(), ERR_TRANSPOSE_RANK);
}

#[test]
fn permute_materializes_general_axis_order() {
    let tensor = Tensor::structa((0..24).collect::<Vec<i32>>(), &[2, 3, 4]).unwrap();

    let permuted = tensor.permute(&[2, 0, 1]).expect("valid axis order");

    assert_eq!(permuted.magnitudines(), vec![4, 2, 3]);
    assert_eq!(
        permuted.planata().unwrap(),
        vec![
            0, 4, 8, 12, 16, 20, 1, 5, 9, 13, 17, 21, 2, 6, 10, 14, 18, 22, 3, 7, 11, 15, 19, 23
        ]
    );
}

#[test]
fn permute_materializes_views_without_aliasing() {
    let mut tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[3, 2]).unwrap();
    let view = tensor.sectio(1, 3).expect("axis-0 view");
    let permuted = view.permute(&[1, 0]).expect("rank-2 view permute");

    tensor.ponde(&[1, 0], 99.0).unwrap();

    assert_eq!(permuted.magnitudines(), vec![2, 2]);
    assert_eq!(permuted.planata().unwrap(), vec![3.0, 5.0, 4.0, 6.0]);
}

#[test]
fn permute_accepts_rank_zero_empty_axis_list() {
    let tensor = Tensor::structa(vec![42_i32], &[]).unwrap();

    let permuted = tensor.permute(&[]).expect("rank-0 identity permute");

    assert_eq!(permuted.magnitudines(), Vec::<i64>::new());
    assert_eq!(permuted.planata().unwrap(), vec![42]);
}

#[test]
fn permute_rejects_rank_mismatch_and_missing_axis() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();

    assert_eq!(tensor.permute(&[0]).unwrap_err(), ERR_PERMUTE_RANK);
}

#[test]
fn permute_rejects_negative_axis() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();

    assert_eq!(
        tensor.permute(&[0, -1]).unwrap_err(),
        ERR_PERMUTE_NEGATIVE_AXIS
    );
}

#[test]
fn permute_rejects_axis_out_of_range() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();

    assert_eq!(
        tensor.permute(&[0, 2]).unwrap_err(),
        ERR_PERMUTE_AXIS_OUT_OF_RANGE
    );
}

#[test]
fn permute_rejects_duplicate_axis() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();

    assert_eq!(
        tensor.permute(&[0, 0]).unwrap_err(),
        ERR_PERMUTE_DUPLICATE_AXIS
    );
}

#[test]
fn permute_rejects_rank_zero_non_empty_axis_list() {
    let tensor = Tensor::structa(vec![42_i32], &[]).unwrap();

    assert_eq!(tensor.permute(&[0]).unwrap_err(), ERR_PERMUTE_RANK);
}

#[test]
fn matmul_square_identity() {
    // I₃ × A = A
    let eye = Tensor::structa(vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], &[3, 3]).unwrap();
    let a = Tensor::structa(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0], &[3, 3]).unwrap();
    let result = eye.matmul(&a).expect("valid matmul");
    assert_eq!(result.magnitudines(), vec![3, 3]);
    assert_eq!(
        result.planata().unwrap(),
        vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]
    );
}

#[test]
fn matmul_rectangular() {
    // [2,3] × [3,4] → [2,4]
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let b = Tensor::structa(
        vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0,
        ],
        &[3, 4],
    )
    .unwrap();
    let result = a.matmul(&b).expect("valid matmul");
    assert_eq!(result.magnitudines(), vec![2, 4]);
    // Row 0: [1*1+2*5+3*9, 1*2+2*6+3*10, 1*3+2*7+3*11, 1*4+2*8+3*12]
    //       = [38, 44, 50, 56]
    // Row 1: [4*1+5*5+6*9, 4*2+5*6+6*10, 4*3+5*7+6*11, 4*4+5*8+6*12]
    //       = [83, 98, 113, 128]
    assert_eq!(
        result.planata().unwrap(),
        vec![38.0, 44.0, 50.0, 56.0, 83.0, 98.0, 113.0, 128.0]
    );
}

#[test]
fn matmul_rank_two_accepts_a_vector_argument() {
    let matrix = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let vector = Tensor::structa(vec![2.0f32, 3.0, 4.0], &[3]).unwrap();

    let result = matrix.matmul(&vector).unwrap();

    assert_eq!(result.magnitudines(), vec![2]);
    assert_eq!(result.planata().unwrap(), vec![20.0, 47.0]);
}

#[test]
fn matmul_rank_three_batches_a_rank_two_rhs_across_all_batches() {
    let lhs = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], &[2, 2, 2]).unwrap();
    let rhs = Tensor::structa(vec![10.0f32, 20.0], &[2, 1]).unwrap();

    let result = lhs.matmul(&rhs).unwrap();

    assert_eq!(result.magnitudines(), vec![2, 2, 1]);
    assert_eq!(result.planata().unwrap(), vec![50.0, 110.0, 170.0, 230.0]);
}

#[test]
fn matmul_rank_three_accepts_matching_rank_three_rhs_batches() {
    let lhs = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], &[2, 2, 2]).unwrap();
    let rhs = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2, 1]).unwrap();

    let result = lhs.matmul(&rhs).unwrap();

    assert_eq!(result.magnitudines(), vec![2, 2, 1]);
    assert_eq!(result.planata().unwrap(), vec![5.0, 11.0, 39.0, 53.0]);
}

#[test]
fn matmul_rank_four_accepts_rhs_batch_prefix() {
    let lhs = Tensor::structa(vec![1.0f32; 12], &[2, 3, 1, 2]).unwrap();
    let rhs = Tensor::structa(vec![1.0f32, 2.0, 10.0, 20.0], &[2, 2, 1]).unwrap();

    let result = lhs.matmul(&rhs).unwrap();

    assert_eq!(result.magnitudines(), vec![2, 3, 1, 1]);
    assert_eq!(
        result.planata().unwrap(),
        vec![3.0, 3.0, 3.0, 30.0, 30.0, 30.0]
    );
}

/// Values spread over twelve decades, so any change in accumulation order
/// changes the f32 bits.
fn order_sensitive(count: usize, salt: u32) -> Vec<f32> {
    (0..count)
        .map(|index| {
            let hash = (index as u32)
                .wrapping_mul(2_654_435_761)
                .wrapping_add(salt)
                >> 8;
            let magnitude = [1.0e6_f32, 1.0, 1.0e-3, 1.0e-6][(hash % 4) as usize];
            ((hash % 20_011) as f32 - 10_005.0) * magnitude
        })
        .collect()
}

/// The contraction as the language defines it: for every output element, a
/// left fold of products in ascending `k` starting from zero.
fn sequential_matmul(lhs: &[f32], rhs: &[f32], rows: usize, inner: usize, cols: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(rows * cols);
    for row in 0..rows {
        for col in 0..cols {
            let mut acc = 0.0_f32;
            for k in 0..inner {
                acc += lhs[row * inner + k] * rhs[k * cols + col];
            }
            out.push(acc);
        }
    }
    out
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|value| value.to_bits()).collect()
}

#[test]
fn matmul_dense_is_bit_identical_to_the_sequential_contraction() {
    let (rows, inner, cols) = (7, 33, 5);
    let lhs = order_sensitive(rows * inner, 1);
    let rhs = order_sensitive(inner * cols, 2);
    let a = Tensor::structa(lhs.clone(), &[rows as i64, inner as i64]).unwrap();
    let b = Tensor::structa(rhs.clone(), &[inner as i64, cols as i64]).unwrap();

    let result = a.matmul(&b).unwrap();

    assert_eq!(
        bits(&result.planata().unwrap()),
        bits(&sequential_matmul(&lhs, &rhs, rows, inner, cols))
    );
}

#[test]
fn matmul_dense_matrix_vector_is_bit_identical_to_the_sequential_contraction() {
    let (rows, inner) = (9, 40);
    let lhs = order_sensitive(rows * inner, 3);
    let rhs = order_sensitive(inner, 4);
    let a = Tensor::structa(lhs.clone(), &[rows as i64, inner as i64]).unwrap();
    let v = Tensor::structa(rhs.clone(), &[inner as i64]).unwrap();

    let result = a.matmul(&v).unwrap();

    assert_eq!(result.magnitudines(), vec![rows as i64]);
    assert_eq!(
        bits(&result.planata().unwrap()),
        bits(&sequential_matmul(&lhs, &rhs, rows, inner, 1))
    );
}

/// `std::sync::Mutex` is not reentrant: an operand sharing its buffer with the
/// receiver (`a·a`, or a clone) must lock it once.
#[test]
fn matmul_of_a_tensor_with_itself_locks_the_shared_buffer_once() {
    let n = 12;
    let data = order_sensitive(n * n, 5);
    let a = Tensor::structa(data.clone(), &[n as i64, n as i64]).unwrap();
    let alias = a.clone();
    let expected = sequential_matmul(&data, &data, n, n, n);

    assert_eq!(
        bits(&a.matmul(&a).unwrap().planata().unwrap()),
        bits(&expected)
    );
    assert_eq!(
        bits(&a.matmul(&alias).unwrap().planata().unwrap()),
        bits(&expected)
    );
}

#[test]
fn matmul_broadcasts_trailing_batches_over_one_rhs_matrix() {
    // Receiver batches [2, 3]; the argument has one batch axis [2], so each
    // argument matrix serves three receiver batches.
    let (outer, trailing, rows, inner, cols) = (2, 3, 4, 6, 5);
    let lhs = order_sensitive(outer * trailing * rows * inner, 6);
    let rhs = order_sensitive(outer * inner * cols, 7);
    let a = Tensor::structa(
        lhs.clone(),
        &[outer as i64, trailing as i64, rows as i64, inner as i64],
    )
    .unwrap();
    let b = Tensor::structa(rhs.clone(), &[outer as i64, inner as i64, cols as i64]).unwrap();

    let result = a.matmul(&b).unwrap();

    let mut expected = Vec::new();
    for batch in 0..outer * trailing {
        let lhs_matrix = &lhs[batch * rows * inner..][..rows * inner];
        let rhs_matrix = &rhs[(batch / trailing) * inner * cols..][..inner * cols];
        expected.extend(sequential_matmul(lhs_matrix, rhs_matrix, rows, inner, cols));
    }
    assert_eq!(
        result.magnitudines(),
        vec![outer as i64, trailing as i64, rows as i64, cols as i64]
    );
    assert_eq!(bits(&result.planata().unwrap()), bits(&expected));
}

/// A transposed operand is a strides-only view, not a standard layout: it
/// takes the strided path and must still match the sequential contraction.
#[test]
fn matmul_with_a_transposed_receiver_matches_the_sequential_contraction() {
    let (rows, inner, cols) = (6, 11, 4);
    let stored = order_sensitive(inner * rows, 8); // stored as [inner, rows]
    let rhs = order_sensitive(inner * cols, 9);
    let transposed = Tensor::structa(stored.clone(), &[inner as i64, rows as i64])
        .unwrap()
        .transpose_rank2()
        .unwrap();
    let b = Tensor::structa(rhs.clone(), &[inner as i64, cols as i64]).unwrap();

    let result = transposed.matmul(&b).unwrap();

    let mut lhs = vec![0.0_f32; rows * inner];
    for row in 0..rows {
        for k in 0..inner {
            lhs[row * inner + k] = stored[k * rows + row];
        }
    }
    assert_eq!(
        bits(&result.planata().unwrap()),
        bits(&sequential_matmul(&lhs, &rhs, rows, inner, cols))
    );
}

#[test]
fn matmul_with_a_transposed_argument_matches_the_sequential_contraction() {
    let (rows, inner, cols) = (5, 13, 7);
    let lhs = order_sensitive(rows * inner, 10);
    let stored = order_sensitive(cols * inner, 11); // stored as [cols, inner]
    let a = Tensor::structa(lhs.clone(), &[rows as i64, inner as i64]).unwrap();
    let transposed = Tensor::structa(stored.clone(), &[cols as i64, inner as i64])
        .unwrap()
        .transpose_rank2()
        .unwrap();

    let result = a.matmul(&transposed).unwrap();

    let mut rhs = vec![0.0_f32; inner * cols];
    for k in 0..inner {
        for col in 0..cols {
            rhs[k * cols + col] = stored[col * inner + k];
        }
    }
    assert_eq!(
        bits(&result.planata().unwrap()),
        bits(&sequential_matmul(&lhs, &rhs, rows, inner, cols))
    );
}

#[test]
fn matmul_with_a_strided_vector_argument_matches_the_sequential_contraction() {
    let (rows, inner) = (4, 5);
    let lhs = order_sensitive(rows * inner, 12);
    let wide = order_sensitive(inner * 2, 13);
    let a = Tensor::structa(lhs.clone(), &[rows as i64, inner as i64]).unwrap();
    // Every other element of a ten-element buffer: a rank-1 view of length 5.
    let strided = Tensor::structa(wide.clone(), &[(inner * 2) as i64])
        .unwrap()
        .sectio_strided(0, (inner * 2) as i64, 2)
        .unwrap();

    let result = a.matmul(&strided).unwrap();

    let vector: Vec<f32> = wide.iter().step_by(2).copied().collect();
    assert_eq!(
        bits(&result.planata().unwrap()),
        bits(&sequential_matmul(&lhs, &vector, rows, inner, 1))
    );
}

#[test]
fn matmul_over_an_empty_inner_dimension_yields_zeros() {
    let a: Tensor<f32> = Tensor::structa(Vec::new(), &[2, 0]).unwrap();
    let b: Tensor<f32> = Tensor::structa(Vec::new(), &[0, 3]).unwrap();

    let result = a.matmul(&b).unwrap();

    assert_eq!(result.magnitudines(), vec![2, 3]);
    assert_eq!(result.planata().unwrap(), vec![0.0; 6]);
}

#[test]
fn matmul_dense_works_for_non_copy_free_integer_elements() {
    // The bound is `Clone + Default + Add + Mul`, not `Copy` or float: the
    // fast path must hold for any such element type.
    let a = Tensor::structa(vec![1_i64, 2, 3, 4, 5, 6], &[2, 3]).unwrap();
    let b = Tensor::structa(vec![7_i64, 8, 9, 10, 11, 12], &[3, 2]).unwrap();

    assert_eq!(
        a.matmul(&b).unwrap().planata().unwrap(),
        vec![58, 64, 139, 154]
    );
}

#[test]
fn matmul_rejects_nonmatching_batch_prefix() {
    let lhs = Tensor::structa(vec![1.0f32; 8], &[2, 2, 2]).unwrap();
    let rhs = Tensor::structa(vec![1.0f32; 6], &[3, 2, 1]).unwrap();

    assert_eq!(lhs.matmul(&rhs).unwrap_err(), ERR_MATMUL_BATCH_DIMENSION);
}

#[test]
fn matmul_receiver_rank_rejects_with_error() {
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();
    let b = Tensor::structa(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    assert_eq!(a.matmul(&b).unwrap_err(), ERR_MATMUL_RECEIVER_RANK);
}

#[test]
fn matmul_argument_rank_rejects_with_error() {
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let b = Tensor::<f32>::vacua();
    assert_eq!(a.matmul(&b).unwrap_err(), ERR_MATMUL_ARGUMENT_RANK);
}

#[test]
fn matmul_inner_mismatch_rejects_with_error() {
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let b = Tensor::structa(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], &[4, 2]).unwrap();
    assert_eq!(a.matmul(&b).unwrap_err(), ERR_MATMUL_INNER_DIMENSION);
}

#[test]
fn matmul_rejects_overflowing_result_shape_before_allocation() {
    let a = Tensor::<f32>::crea(&[i64::MAX, 0], 0.0).expect("zero-element huge receiver");
    let b = Tensor::<f32>::crea(&[0, 2], 0.0).expect("zero-element argument");

    assert_eq!(a.matmul(&b).unwrap_err(), ERR_ELEMENT_COUNT_OVERFLOW);
}

#[test]
fn layernorm_matches_reference_rank2_axis1_no_affine() {
    // 2×3 input, axis=1 (normalize each row)
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let result = input.layernorm(1, 1e-5, None, None).unwrap();

    assert_eq!(result.magnitudines(), vec![2, 3]);

    // Row 0: mean=2.0, var=(1+0+1)/3=0.6667, inv_std≈1.22474
    // Expected row 0: [-1.2247, 0.0, 1.2247]
    // Row 1: mean=5.0, var=0.6667, same inv_std
    // Expected row 1: [-1.2247, 0.0, 1.2247]
    let result_data = result.planata().unwrap();
    let expected: Vec<f32> = vec![
        -1.224_744_9,
        0.0,
        1.224_744_9,
        -1.224_744_9,
        0.0,
        1.224_744_9,
    ];
    for (a, e) in result_data.iter().zip(expected.iter()) {
        assert!(
            (a - e).abs() < 1e-4,
            "layernorm output {a} differs from expected {e}"
        );
    }
}

#[test]
fn layernorm_matches_reference_rank2_axis1_with_affine() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let gamma = Tensor::structa(vec![1.0f32, 2.0, 0.5], &[3]).unwrap();
    let beta = Tensor::structa(vec![0.0f32, 0.1, -0.2], &[3]).unwrap();

    let result = input.layernorm(1, 1e-5, Some(&gamma), Some(&beta)).unwrap();
    assert_eq!(result.magnitudines(), vec![2, 3]);

    let result_data = result.planata().unwrap();
    let row0: Vec<f32> = vec![
        (-1.224_744_9 * 1.0) + 0.0,
        0.0 * 2.0 + 0.1,
        1.224_744_9 * 0.5 + (-0.2),
    ];
    let row1: Vec<f32> = vec![
        (-1.224_744_9 * 1.0) + 0.0,
        0.0 * 2.0 + 0.1,
        1.224_744_9 * 0.5 + (-0.2),
    ];

    for (a, e) in result_data.iter().zip(row0.iter().chain(row1.iter())) {
        assert!(
            (a - e).abs() < 1e-4,
            "affine layernorm output {a} differs from expected {e}"
        );
    }
}

#[test]
fn layernorm_rank1_no_affine() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();
    let result = input.layernorm(0, 1e-5, None, None).unwrap();

    assert_eq!(result.magnitudines(), vec![3]);
    // mean=2.0, var=(1+0+1)/3=0.6667, inv_std≈1.2247
    let expected = [-1.224_744_9_f32, 0.0, 1.224_744_9];
    for (a, e) in result.planata().unwrap().iter().zip(expected.iter()) {
        assert!(
            (a - e).abs() < 1e-4,
            "rank-1 layernorm output {a} differs from expected {e}"
        );
    }
}

#[test]
fn layernorm_equal_max_finite_values_remain_finite() {
    let input = Tensor::structa(vec![f32::MAX, f32::MAX], &[2]).unwrap();

    let result = input.layernorm(0, 1e-5, None, None).unwrap();

    assert_eq!(result.planata().unwrap(), vec![0.0, 0.0]);
}

#[test]
fn layernorm_rank2_axis0_no_affine() {
    // 2×3 input, axis=0 (normalize each column independently)
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let result = input.layernorm(0, 1e-5, None, None).unwrap();

    assert_eq!(result.magnitudines(), vec![2, 3]);
    let result_data = result.planata().unwrap();

    // Col 0: [1, 4], mean=2.5, centered=[-1.5, 1.5], var=(2.25+2.25)/2=2.25, inv_std≈0.66667
    // Expected col 0: [-1.0/√(0.444...), 1.0/√(0.444...)] = [-1.0, 1.0]
    // Col 1: [2, 5], mean=3.5, centered=[-1.5, 1.5], var=2.25, inv_std≈0.66667
    // Expected col 1: [-1.0, 1.0]
    // Col 2: [3, 6], mean=4.5, same pattern
    // Expected: [-1, -1, -1, 1, 1, 1]
    let expected = [-1.0f32, -1.0, -1.0, 1.0, 1.0, 1.0];
    for (a, e) in result_data.iter().zip(expected.iter()) {
        assert!(
            (a - e).abs() < 1e-4,
            "axis-0 layernorm output {a} differs from expected {e}"
        );
    }
}

#[test]
fn layernorm_rank2_axis0_with_affine() {
    // Regression test: (Some(g), Some(b)) arm indexed gamma/beta at
    // flattened (r*cols+c) instead of row (r), causing panic for cols>1.
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let gamma = Tensor::structa(vec![2.0f32, 0.5], &[2]).unwrap();
    let beta = Tensor::structa(vec![0.1f32, -0.1], &[2]).unwrap();
    let result = input.layernorm(0, 1e-5, Some(&gamma), Some(&beta)).unwrap();

    assert_eq!(result.magnitudines(), vec![2, 3]);
    let result_data = result.planata().unwrap();

    // axis=0 normalizes each column independently.
    // Pre-affine normalized y = [-1,-1,-1, 1,1,1] (same as no-affine test).
    // gamma = [2.0, 0.5], beta = [0.1, -0.1]
    // Row 0 (r=0): -1 * 2.0 + 0.1 = -1.9
    // Row 1 (r=1):  1 * 0.5 + (-0.1) = 0.4
    let expected = [-1.9f32, -1.9, -1.9, 0.4, 0.4, 0.4];
    for (a, e) in result_data.iter().zip(expected.iter()) {
        assert!(
            (a - e).abs() < 1e-4,
            "axis-0 affine layernorm output {a} differs from expected {e}"
        );
    }
}

#[test]
fn layernorm_rejects_non_finite_input() {
    let input = Tensor::structa(vec![1.0f32, f32::NAN, 3.0], &[3]).unwrap();
    assert_eq!(
        input.layernorm(0, 1e-5, None, None).unwrap_err(),
        ERR_LAYERNORM_NON_FINITE_INPUT
    );
}

#[test]
fn layernorm_rejects_empty_tensor() {
    let input = Tensor::structa(vec![], &[0]).unwrap();
    assert_eq!(
        input.layernorm(0, 1e-5, None, None).unwrap_err(),
        ERR_LAYERNORM_EMPTY_TENSOR
    );
}

#[test]
fn layernorm_rejects_rank_too_high() {
    let input = Tensor::structa(vec![1.0f32; 8], &[2, 2, 2]).unwrap();
    assert_eq!(
        input.layernorm(0, 1e-5, None, None).unwrap_err(),
        ERR_LAYERNORM_RANK_TOO_HIGH
    );
}

#[test]
fn layernorm_rejects_axis_out_of_range() {
    let input = Tensor::structa(vec![1.0f32, 2.0], &[2]).unwrap();
    assert_eq!(
        input.layernorm(1, 1e-5, None, None).unwrap_err(),
        ERR_LAYERNORM_AXIS_OUT_OF_RANGE
    );
}

#[test]
fn layernorm_rejects_gamma_shape_mismatch() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let gamma = Tensor::structa(vec![1.0f32], &[1]).unwrap();
    assert_eq!(
        input.layernorm(1, 1e-5, Some(&gamma), None).unwrap_err(),
        ERR_LAYERNORM_GAMMA_SHAPE_MISMATCH
    );
}

#[test]
fn layernorm_rejects_beta_shape_mismatch() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let beta = Tensor::structa(vec![1.0f32], &[1]).unwrap();
    assert_eq!(
        input.layernorm(1, 1e-5, None, Some(&beta)).unwrap_err(),
        ERR_LAYERNORM_BETA_SHAPE_MISMATCH
    );
}

#[test]
fn layernorm_rejects_non_finite_gamma() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let gamma = Tensor::structa(vec![f32::NAN, 1.0], &[2]).unwrap();
    assert_eq!(
        input.layernorm(1, 1e-5, Some(&gamma), None).unwrap_err(),
        ERR_LAYERNORM_GAMMA_NON_FINITE
    );
}

#[test]
fn layernorm_rejects_non_finite_beta() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let beta = Tensor::structa(vec![f32::INFINITY, 0.0], &[2]).unwrap();
    assert_eq!(
        input.layernorm(1, 1e-5, None, Some(&beta)).unwrap_err(),
        ERR_LAYERNORM_BETA_NON_FINITE
    );
}

#[test]
fn layernorm_rejects_zero_epsilon() {
    let input = Tensor::structa(vec![1.0f32, 2.0], &[2]).unwrap();
    assert_eq!(
        input.layernorm(0, 0.0, None, None).unwrap_err(),
        ERR_LAYERNORM_EPSILON_INVALID
    );
}

#[test]
fn layernorm_rejects_negative_epsilon() {
    let input = Tensor::structa(vec![1.0f32, 2.0], &[2]).unwrap();
    assert_eq!(
        input.layernorm(0, -1.0, None, None).unwrap_err(),
        ERR_LAYERNORM_EPSILON_INVALID
    );
}

#[test]
fn layernorm_rejects_nan_epsilon() {
    let input = Tensor::structa(vec![1.0f32, 2.0], &[2]).unwrap();
    assert_eq!(
        input.layernorm(0, f32::NAN, None, None).unwrap_err(),
        ERR_LAYERNORM_EPSILON_INVALID
    );
}

// ---------------------------------------------------------------------------
// Softmax forward tests
// ---------------------------------------------------------------------------

#[test]
fn softmax_rejects_empty_tensor() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[0]).unwrap();
    assert_eq!(empty.softmax().unwrap_err(), ERR_SOFTMAX_EMPTY_TENSOR);
}

#[test]
fn softmax_rejects_rank0_tensor() {
    let scalar = Tensor::structa(vec![2.0f32], &[]).unwrap();
    assert_eq!(scalar.softmax().unwrap_err(), ERR_SOFTMAX_RANK);
}

#[test]
fn softmax_rejects_non_finite_input() {
    let input = Tensor::structa(vec![1.0f32, f32::NAN, 3.0], &[3]).unwrap();
    assert_eq!(input.softmax().unwrap_err(), ERR_SOFTMAX_NON_FINITE_INPUT);

    let input = Tensor::structa(vec![1.0f32, f32::INFINITY, 3.0], &[3]).unwrap();
    assert_eq!(input.softmax().unwrap_err(), ERR_SOFTMAX_NON_FINITE_INPUT);
}

#[test]
fn softmax_rank1_sums_to_one() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();
    let output = input.softmax().unwrap();
    let data = output.planata().unwrap();
    let sum: f32 = data.iter().sum();
    assert!((sum - 1.0).abs() < 1e-6, "sum must be 1.0, got {sum}");
}

#[test]
fn softmax_rank2_sums_to_one_per_row() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 1.0, 2.0, 3.0], &[2, 3]).unwrap();
    let output = input.softmax().unwrap();
    let data = output.planata().unwrap();
    let row0_sum: f32 = data[0..3].iter().sum();
    let row1_sum: f32 = data[3..6].iter().sum();
    assert!(
        (row0_sum - 1.0).abs() < 1e-6,
        "row 0 sum must be 1.0, got {row0_sum}"
    );
    assert!(
        (row1_sum - 1.0).abs() < 1e-6,
        "row 1 sum must be 1.0, got {row1_sum}"
    );
}

#[test]
fn softmax_rank1_correct_values() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();
    let output = input.softmax().unwrap();
    let data = output.planata().unwrap();
    // softmax([1, 2, 3]) ≈ [0.09003057, 0.24472847, 0.66524096]
    let expected = [0.090_030_57, 0.244_728_47, 0.665_240_96];
    for i in 0..3 {
        assert!(
            (data[i] - expected[i]).abs() < 1e-6,
            "item {i}: got {}, expected {}",
            data[i],
            expected[i]
        );
    }
}

#[test]
fn softmax_rank2_identical_rows() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 1.0, 2.0, 3.0], &[2, 3]).unwrap();
    let output = input.softmax().unwrap();
    let data = output.planata().unwrap();
    // Both rows are identical since inputs are identical.
    for i in 0..3 {
        assert!(
            (data[i] - data[i + 3]).abs() < 1e-10,
            "rows must be identical; item {i}"
        );
    }
}

#[test]
fn softmax_rank2_correct_values() {
    // Different inputs per row: [1,2,3] and [7,8,9] as 2×3
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 7.0, 8.0, 9.0], &[2, 3]).unwrap();
    let output = input.softmax().unwrap();
    let data = output.planata().unwrap();
    // softmax([1,2,3]) ≈ [0.09003057, 0.24472847, 0.66524096]
    // softmax([7,8,9]) ≈ [0.09003057, 0.24472847, 0.66524096] (same because shift-invariant)
    let expected_row = [0.090_030_57, 0.244_728_47, 0.665_240_96];
    for i in 0..3 {
        assert!(
            (data[i] - expected_row[i]).abs() < 1e-6,
            "row 0 item {i}: got {}, expected {}",
            data[i],
            expected_row[i]
        );
    }
    for i in 0..3 {
        assert!(
            (data[i + 3] - expected_row[i]).abs() < 1e-6,
            "row 1 item {i}: got {}, expected {}",
            data[i + 3],
            expected_row[i]
        );
    }
}

#[test]
fn softmax_preserves_shape() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let output = input.softmax().unwrap();
    assert_eq!(output.longitudo(), 2);
    assert_eq!(output.element_count(), 6);
}

// ---------------------------------------------------------------------------
// Cross-entropy forward tests
// ---------------------------------------------------------------------------

#[test]
fn crux_entropia_rank2_correct_loss() {
    let logits = Tensor::structa(vec![2.0_f32, 1.0, 0.0, 1.0, 2.0, 1.0], &[2, 3]).unwrap();
    let targets = Tensor::structa(vec![1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0], &[2, 3]).unwrap();
    let loss = logits.crux_entropia(&targets).unwrap();
    // softmax row0 = [0.6652, 0.2447, 0.0900], row1 = [0.2119, 0.5761, 0.2119]
    // CE: row0 = -log(0.6652+ε) ≈ 0.4076, row1 = -log(0.5761+ε) ≈ 0.5514
    // Loss = (0.4076 + 0.5514) / 3 ≈ 0.3197
    let expected = 0.3197_f32;
    assert!(
        (loss - expected).abs() < 1e-4,
        "expected ~{expected}, got {loss}"
    );
}

#[test]
fn crux_entropia_rejects_empty_tensor() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[0]).unwrap();
    let targets = Tensor::<f32>::structa(Vec::new(), &[0]).unwrap();
    assert_eq!(
        empty.crux_entropia(&targets).unwrap_err(),
        ERR_CRUX_ENTROPIA_EMPTY_TENSOR
    );
}

#[test]
fn crux_entropia_rejects_non_finite_logits() {
    let logits = Tensor::structa(vec![1.0_f32, f32::NAN, 3.0], &[3]).unwrap();
    let targets = Tensor::structa(vec![1.0_f32, 0.0, 0.0], &[3]).unwrap();
    assert_eq!(
        logits.crux_entropia(&targets).unwrap_err(),
        ERR_CRUX_ENTROPIA_NON_FINITE_INPUT
    );
}

#[test]
fn crux_entropia_rejects_non_finite_targets() {
    let logits = Tensor::structa(vec![1.0_f32, 2.0, 3.0], &[3]).unwrap();
    let targets = Tensor::structa(vec![1.0_f32, f32::INFINITY, 0.0], &[3]).unwrap();
    assert_eq!(
        logits.crux_entropia(&targets).unwrap_err(),
        ERR_CRUX_ENTROPIA_TARGET_NON_FINITE
    );
}

#[test]
fn crux_entropia_rejects_target_out_of_range() {
    let logits = Tensor::structa(vec![1.0_f32, 2.0, 3.0], &[3]).unwrap();
    let targets = Tensor::structa(vec![1.5_f32, 0.0, 0.0], &[3]).unwrap();
    assert_eq!(
        logits.crux_entropia(&targets).unwrap_err(),
        ERR_CRUX_ENTROPIA_TARGET_RANGE
    );
}

#[test]
fn crux_entropia_rejects_shape_mismatch() {
    let logits = Tensor::structa(vec![1.0_f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let targets = Tensor::structa(vec![1.0_f32, 0.0, 0.0], &[3]).unwrap();
    assert_eq!(
        logits.crux_entropia(&targets).unwrap_err(),
        ERR_CRUX_ENTROPIA_SHAPE_MISMATCH
    );
}

// ── permute / scalar access: allocation-free paths ─────────────────────────

/// Reference permutation by explicit coordinates.
fn permuted_by_coordinates(values: &[f32], shape: &[usize], axes: &[usize]) -> Vec<f32> {
    let out_shape: Vec<usize> = axes.iter().map(|&axis| shape[axis]).collect();
    let count: usize = out_shape.iter().product();
    let in_strides: Vec<usize> = (0..shape.len())
        .map(|axis| shape[axis + 1..].iter().product())
        .collect();
    (0..count)
        .map(|ordinal| {
            let mut rest = ordinal;
            let mut offset = 0;
            for (position, &axis) in axes.iter().enumerate().rev() {
                let extent = out_shape[position];
                offset += (rest % extent) * in_strides[axis];
                rest /= extent;
            }
            values[offset]
        })
        .collect()
}

#[test]
fn permute_reads_a_standard_layout_through_permuted_strides() {
    let shape = [2_usize, 3, 4];
    let values: Vec<f32> = (0..24).map(|index| index as f32).collect();
    let tensor = Tensor::structa(values.clone(), &[2, 3, 4]).unwrap();
    for axes in [[0, 1, 2], [2, 0, 1], [1, 2, 0], [2, 1, 0], [0, 2, 1]] {
        let result = tensor.permute(&axes.map(|axis| axis as i64)).unwrap();
        assert_eq!(
            result.planata().unwrap(),
            permuted_by_coordinates(&values, &shape, &axes),
            "axes {axes:?}"
        );
    }
}

#[test]
fn permute_of_a_strides_only_view_composes_with_its_strides() {
    // A transposed view carries swapped strides over the same buffer.
    let values: Vec<f32> = (0..12).map(|index| index as f32).collect();
    let tensor = Tensor::structa(values.clone(), &[3, 4]).unwrap();
    let transposed = tensor.transpose_rank2().unwrap(); // logical [4, 3]
    let result = transposed.permute(&[1, 0]).unwrap(); // back to [3, 4]
    assert_eq!(result.magnitudines(), vec![3, 4]);
    assert_eq!(result.planata().unwrap(), values);
}

#[test]
fn permute_of_a_sliced_view_honors_its_offset() {
    let values: Vec<f32> = (0..12).map(|index| index as f32).collect();
    let rows = Tensor::structa(values, &[6, 2])
        .unwrap()
        .sectio(2, 5)
        .unwrap();
    let result = rows.permute(&[1, 0]).unwrap();
    assert_eq!(result.magnitudines(), vec![2, 3]);
    assert_eq!(
        result.planata().unwrap(),
        vec![4.0, 6.0, 8.0, 5.0, 7.0, 9.0]
    );
}

#[test]
fn accipe_and_ponde_agree_with_each_other_at_every_rank_including_high_ranks() {
    // Ranks up to eight index without allocating; nine takes the `Vec` path.
    for rank in [1_usize, 2, 4, 8, 9] {
        let shape = vec![2_i64; rank];
        let count = 1_usize << rank;
        let mut tensor = Tensor::structa(vec![0.0_f32; count], &shape).unwrap();
        let last = vec![1_i64; rank];
        tensor.ponde(&last, 7.0).unwrap();
        assert_eq!(tensor.accipe(&last).unwrap(), Some(7.0), "rank {rank}");
        assert_eq!(tensor.accipe(&vec![0_i64; rank]).unwrap(), Some(0.0));
        assert_eq!(
            tensor
                .planata()
                .unwrap()
                .iter()
                .filter(|v| **v == 7.0)
                .count(),
            1
        );
    }
}

#[test]
fn accipe_and_ponde_reject_negative_indices_before_looking_at_bounds() {
    let mut tensor = Tensor::structa(vec![1.0_f32, 2.0], &[2]).unwrap();
    assert_eq!(tensor.accipe(&[-1]).unwrap_err(), ERR_NEGATIVE_INDEX);
    assert_eq!(tensor.ponde(&[-1], 0.0).unwrap_err(), ERR_NEGATIVE_INDEX);
    // A parseable but out-of-bounds index is a different error.
    assert_eq!(
        tensor.ponde(&[2], 0.0).unwrap_err(),
        ERR_INDEX_OUT_OF_BOUNDS
    );
    assert_eq!(tensor.accipe(&[2]).unwrap(), None);
    // A negative index anywhere in the list wins over an out-of-bounds one.
    let mut matrix = Tensor::structa(vec![0.0_f32; 4], &[2, 2]).unwrap();
    assert_eq!(matrix.ponde(&[9, -1], 0.0).unwrap_err(), ERR_NEGATIVE_INDEX);
}

// ── write_values (`target ⇇ source`) ───────────────────────────────────────

#[test]
fn write_values_copies_every_value_into_the_destination() {
    let mut dest = Tensor::structa(vec![0.0_f32; 4], &[2, 2]).unwrap();
    let source = Tensor::structa(vec![1.0_f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    dest.write_values(&source).unwrap();
    assert_eq!(dest.planata().unwrap(), vec![1.0, 2.0, 3.0, 4.0]);
    assert_eq!(source.planata().unwrap(), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn write_values_preserves_storage_identity_for_every_handle() {
    let mut dest = Tensor::structa(vec![0.0_f32; 3], &[3]).unwrap();
    let other_handle = dest.clone();
    let source = Tensor::structa(vec![7.0_f32, 8.0, 9.0], &[3]).unwrap();
    dest.write_values(&source).unwrap();
    assert_eq!(other_handle.planata().unwrap(), vec![7.0, 8.0, 9.0]);
}

#[test]
fn write_values_through_a_mut_borrow_is_visible_to_the_caller() {
    // The shape the Rust emitter produces for a `mut` tensor parameter.
    fn store(source: &Tensor<f32>, out: &mut Tensor<f32>) {
        (*out).write_values(source).unwrap();
    }
    let mut out = Tensor::structa(vec![0.0_f32; 2], &[2]).unwrap();
    let source = Tensor::structa(vec![5.0_f32, 6.0], &[2]).unwrap();
    store(&source, &mut out);
    assert_eq!(out.planata().unwrap(), vec![5.0, 6.0]);
}

#[test]
fn write_values_writes_through_the_destination_strides() {
    let backing = Tensor::structa(vec![0.0_f32; 6], &[3, 2]).unwrap();
    let mut transposed = backing.transpose_rank2().unwrap();
    let source = Tensor::structa(vec![1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    transposed.write_values(&source).unwrap();
    // The view reads back what was written; the backing storage holds the
    // transposed layout.
    assert_eq!(
        transposed.planata().unwrap(),
        vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    );
    assert_eq!(
        backing.planata().unwrap(),
        vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]
    );

    let flat = Tensor::structa(vec![0.0_f32; 6], &[6]).unwrap();
    let mut every_other = flat.sectio_strided(0, 6, 2).unwrap();
    let three = Tensor::structa(vec![1.0_f32, 2.0, 3.0], &[3]).unwrap();
    every_other.write_values(&three).unwrap();
    assert_eq!(flat.planata().unwrap(), vec![1.0, 0.0, 2.0, 0.0, 3.0, 0.0]);
}

#[test]
fn write_values_into_a_dense_prefix_view_leaves_the_rest_of_the_buffer_alone() {
    // `sectio(0, 3)` is a standard-layout view of a longer buffer: the slice
    // move must touch exactly its three elements.
    let flat = Tensor::structa(vec![9.0_f32; 6], &[6]).unwrap();
    let mut prefix = flat.sectio(0, 3).unwrap();
    let source = Tensor::structa(vec![1.0_f32, 2.0, 3.0], &[3]).unwrap();
    prefix.write_values(&source).unwrap();
    assert_eq!(flat.planata().unwrap(), vec![1.0, 2.0, 3.0, 9.0, 9.0, 9.0]);

    // An offset slice is strided, not standard layout, and writes in place.
    let mut middle = flat.sectio(2, 5).unwrap();
    middle.write_values(&source).unwrap();
    assert_eq!(flat.planata().unwrap(), vec![1.0, 2.0, 1.0, 2.0, 3.0, 9.0]);
}

#[test]
fn write_values_moves_a_large_dense_tensor() {
    let count = 100_000;
    let values: Vec<f32> = (0..count).map(|index| index as f32 * 0.5).collect();
    let source = Tensor::structa(values.clone(), &[count as i64]).unwrap();
    let mut dest = Tensor::structa(vec![0.0_f32; count], &[count as i64]).unwrap();
    dest.write_values(&source).unwrap();
    assert_eq!(dest.planata().unwrap(), values);
}

#[test]
fn write_values_reads_an_overlapping_source_in_full_first() {
    let mut dest = Tensor::structa(vec![1.0_f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let transposed_self = dest.transpose_rank2().unwrap();
    dest.write_values(&transposed_self).unwrap();
    // A write-as-you-read copy would give [1, 3, 3, 4].
    assert_eq!(dest.planata().unwrap(), vec![1.0, 3.0, 2.0, 4.0]);
}

#[test]
fn write_values_rejects_a_shape_mismatch_without_writing() {
    let mut dest = Tensor::structa(vec![0.0_f32; 4], &[2, 2]).unwrap();
    let wrong = Tensor::structa(vec![1.0_f32; 4], &[4]).unwrap();
    assert_eq!(
        dest.write_values(&wrong).unwrap_err(),
        ERR_TENSOR_COPY_INTO_SHAPE_MISMATCH
    );
    assert_eq!(dest.planata().unwrap(), vec![0.0; 4]);
    let wrong_extent = Tensor::structa(vec![1.0_f32; 6], &[2, 3]).unwrap();
    assert_eq!(
        dest.write_values(&wrong_extent).unwrap_err(),
        ERR_TENSOR_COPY_INTO_SHAPE_MISMATCH
    );
}

#[test]
fn write_values_rejects_an_edge_view_destination() {
    let base = Tensor::structa(vec![1.0_f32, 2.0, 3.0], &[3]).unwrap();
    let mut shifted = base.shift(&[1]).unwrap();
    let source = Tensor::structa(vec![0.0_f32; 3], &[3]).unwrap();
    assert_eq!(
        shifted.write_values(&source).unwrap_err(),
        ERR_TENSOR_EDGE_READ_ONLY
    );
    assert_eq!(base.planata().unwrap(), vec![1.0, 2.0, 3.0]);
}

#[test]
fn write_values_rejects_an_unresolved_optional_source() {
    let base = Tensor::structa(vec![1.0_f32, 2.0, 3.0], &[3]).unwrap();
    let shifted = base.shift(&[1]).unwrap();
    let mut dest = Tensor::structa(vec![0.0_f32; 3], &[3]).unwrap();
    assert_eq!(
        dest.write_values(&shifted).unwrap_err(),
        ERR_TENSOR_EDGE_UNRESOLVED_READ
    );
    assert_eq!(dest.planata().unwrap(), vec![0.0; 3]);
}

#[test]
fn write_values_accepts_a_resolved_edge_view_source() {
    let base = Tensor::structa(vec![1.0_f32, 2.0, 3.0], &[3]).unwrap();
    let wrapped = base
        .shift(&[1])
        .unwrap()
        .limes(TensorEdgePolicy::Wrap)
        .unwrap();
    let mut dest = Tensor::structa(vec![0.0_f32; 3], &[3]).unwrap();
    dest.write_values(&wrapped).unwrap();
    assert_eq!(dest.planata().unwrap(), wrapped.planata().unwrap());
}
