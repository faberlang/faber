use super::{
    ERR_BROADCAST_SHAPE, ERR_CRUX_ENTROPIA_EMPTY_TENSOR, ERR_CRUX_ENTROPIA_NON_FINITE_INPUT,
    ERR_CRUX_ENTROPIA_SHAPE_MISMATCH, ERR_CRUX_ENTROPIA_TARGET_NON_FINITE,
    ERR_CRUX_ENTROPIA_TARGET_RANGE, ERR_DIVIDE_NON_FINITE_INPUT, ERR_DIVIDE_NON_FINITE_RESULT,
    ERR_DIVIDE_ZERO_DENOMINATOR, ERR_ELEMENT_COUNT_OVERFLOW, ERR_LAYERNORM_AXIS_OUT_OF_RANGE,
    ERR_LAYERNORM_BETA_NON_FINITE, ERR_LAYERNORM_BETA_SHAPE_MISMATCH, ERR_LAYERNORM_EMPTY_TENSOR,
    ERR_LAYERNORM_EPSILON_INVALID, ERR_LAYERNORM_GAMMA_NON_FINITE,
    ERR_LAYERNORM_GAMMA_SHAPE_MISMATCH, ERR_LAYERNORM_NON_FINITE_INPUT,
    ERR_LAYERNORM_RANK_TOO_HIGH, ERR_MATMUL_ARGUMENT_RANK, ERR_MATMUL_INNER_DIMENSION,
    ERR_MATMUL_RECEIVER_RANK, ERR_MEDIA_EMPTY, ERR_PERMUTE_AXIS_OUT_OF_RANGE,
    ERR_PERMUTE_DUPLICATE_AXIS, ERR_PERMUTE_NEGATIVE_AXIS, ERR_PERMUTE_RANK,
    ERR_SOFTMAX_EMPTY_TENSOR, ERR_SOFTMAX_NON_FINITE_INPUT, ERR_TRANSPOSE_RANK, Tensor,
    tensor_flat_offset, tensor_shape_element_count, tensor_shape_has_element_count,
};

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
    assert_eq!(tensor.planata(), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn structa_rejects_negative_shape_dimension() {
    let err = Tensor::structa(vec![1.0f32], &[-1]).unwrap_err();
    assert_eq!(err, "tensor shape dimension must be non-negative");
}

#[test]
fn convert_elements_preserves_shape_and_maps_values() {
    let tensor = Tensor::structa(vec![1i64, 2, 3, 4], &[2, 2]).expect("shape matches data");
    let converted = tensor.convert_elements(|value| {
        // SAFETY: test values are small integers.
        #[allow(clippy::cast_precision_loss)]
        let value = value as f64;
        value
    });
    assert_eq!(converted.magnitudines(), vec![2, 2]);
    assert_eq!(converted.planata(), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn convert_elements_empty_tensor() {
    let tensor: Tensor<i64> = Tensor::structa(Vec::new(), &[0, 5]).expect("zero-extent shape");
    let converted: Tensor<f64> = tensor.convert_elements(|v| v as f64);
    assert_eq!(converted.magnitudines(), vec![0, 5]);
    assert_eq!(converted.planata(), Vec::<f64>::new());
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
fn materialize_breaks_sectio_alias() {
    let mut tensor =
        Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).expect("shape matches data");
    let mut materialized = tensor.sectio(0, 1).expect("valid slice").materialize();

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
    assert_eq!(c.planata(), vec![11.0, 22.0, 33.0]);
}

#[test]
fn addita_broadcasts_size_one_dimension() {
    let a = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let b = Tensor::structa(vec![10.0f32, 20.0], &[2, 1]).unwrap();
    let c = a.addita(&b).expect("broadcast-compatible shape");
    assert_eq!(c.magnitudines(), vec![2, 2]);
    // a = [[1,2],[3,4]]; b = [[10],[20]] broadcasts to [[10,10],[20,20]].
    assert_eq!(c.planata(), vec![11.0, 12.0, 23.0, 24.0]);
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
    assert_eq!(result.planata(), Vec::<f32>::new());
}

#[test]
fn subtrahe_broadcasts_zero_extent_with_size_one_axis_to_empty_result() {
    let row = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[1, 3]).unwrap();
    let empty = Tensor::<f32>::structa(Vec::new(), &[0, 3]).unwrap();

    let result = row
        .subtrahe(&empty)
        .expect("one/zero broadcast is compatible");

    assert_eq!(result.magnitudines(), vec![0, 3]);
    assert_eq!(result.planata(), Vec::<f32>::new());
}

#[test]
fn multiplica_broadcasts_zero_extent_with_size_one_axis_to_empty_result() {
    let empty = Tensor::<f32>::structa(Vec::new(), &[2, 0, 3]).unwrap();
    let lane = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[1, 1, 3]).unwrap();

    let result = empty
        .multiplica(&lane)
        .expect("zero/one broadcast is compatible");

    assert_eq!(result.magnitudines(), vec![2, 0, 3]);
    assert_eq!(result.planata(), Vec::<f32>::new());
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
            .planata(),
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
            .planata(),
        vec![10.0, 40.0, 90.0]
    );
}

#[test]
fn addita_integer_tensors_sum_without_widening() {
    let a = Tensor::structa(vec![1i64, 2, 3], &[3]).unwrap();
    let b = Tensor::structa(vec![4i64, 5, 6], &[3]).unwrap();
    assert_eq!(
        a.addita(&b).expect("broadcast-compatible shape").planata(),
        vec![5, 7, 9]
    );
}

#[test]
fn summa_folds_all_elements_to_element_type() {
    let grid = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    assert_eq!(grid.summa(), 10.0);
    let ints = Tensor::structa(vec![1i64, 2, 3, 4], &[4]).unwrap();
    assert_eq!(ints.summa(), 10);
}

#[test]
fn summa_empty_tensor_returns_default() {
    let empty_f32 = Tensor::<f32>::structa(Vec::new(), &[0]).unwrap();
    assert_eq!(empty_f32.summa(), 0.0);

    let empty_i64 = Tensor::<i64>::structa(Vec::new(), &[0, 0]).unwrap();
    assert_eq!(empty_i64.summa(), 0);
}

#[test]
fn neg_negates_f32_elements_and_preserves_shape() {
    let tensor = Tensor::structa(vec![1.0f32, -2.0, 0.0, 4.5], &[2, 2]).unwrap();

    let negated = tensor.neg();

    assert_eq!(negated.magnitudines(), vec![2, 2]);
    assert_eq!(negated.planata(), vec![-1.0, 2.0, -0.0, -4.5]);
}

#[test]
fn neg_empty_tensor_preserves_shape() {
    let tensor = Tensor::<f32>::structa(Vec::new(), &[0, 3]).unwrap();

    let negated = tensor.neg();

    assert_eq!(negated.magnitudines(), vec![0, 3]);
    assert_eq!(negated.planata(), Vec::<f32>::new());
}

#[test]
fn scala_scales_f32_elements_and_preserves_shape() {
    let tensor = Tensor::structa(vec![1.0f32, -2.0, 3.5, 4.0], &[2, 2]).unwrap();

    let scaled = tensor.scala(0.5);

    assert_eq!(scaled.magnitudines(), vec![2, 2]);
    assert_eq!(scaled.planata(), vec![0.5, -1.0, 1.75, 2.0]);
}

#[test]
fn scala_empty_tensor_preserves_shape() {
    let tensor = Tensor::<f32>::structa(Vec::new(), &[0, 2]).unwrap();

    let scaled = tensor.scala(2.0);

    assert_eq!(scaled.magnitudines(), vec![0, 2]);
    assert_eq!(scaled.planata(), Vec::<f32>::new());
}

#[test]
fn divide_broadcasts_finite_f32_tensors() {
    let lhs = Tensor::structa(vec![8.0f32, 18.0, -24.0, 40.0], &[2, 2]).unwrap();
    let rhs = Tensor::structa(vec![2.0f32, -4.0], &[2, 1]).unwrap();

    let divided = lhs.divide(&rhs).expect("finite broadcast division");

    assert_eq!(divided.magnitudines(), vec![2, 2]);
    assert_eq!(divided.planata(), vec![4.0, 9.0, 6.0, -10.0]);
}

#[test]
fn reciproca_preserves_shape_and_values() {
    let tensor = Tensor::structa(vec![2.0f32, -4.0, 0.25, 8.0], &[2, 2]).unwrap();

    let reciprocal = tensor.reciproca().expect("finite reciprocal");

    assert_eq!(reciprocal.magnitudines(), vec![2, 2]);
    assert_eq!(reciprocal.planata(), vec![0.5, -0.25, 4.0, 0.125]);
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
    assert_eq!(transposed.planata(), vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
}

#[test]
fn transpose_rank2_materializes_views_without_aliasing() {
    let mut tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[3, 2]).unwrap();
    let view = tensor.sectio(1, 3).expect("axis-0 view");
    let transposed = view.transpose_rank2().expect("rank-2 view transpose");

    tensor.ponde(&[1, 0], 99.0).unwrap();

    assert_eq!(transposed.magnitudines(), vec![2, 2]);
    assert_eq!(transposed.planata(), vec![3.0, 5.0, 4.0, 6.0]);
}

#[test]
fn transpose_rank2_rejects_non_rank2_tensor() {
    let tensor = Tensor::structa(vec![1.0f32, 2.0, 3.0], &[3]).unwrap();

    assert_eq!(tensor.transpose_rank2().unwrap_err(), ERR_TRANSPOSE_RANK);
}

#[test]
fn permute_materializes_general_axis_order() {
    let tensor = Tensor::structa((0..24).collect::<Vec<i32>>(), &[2, 3, 4]).unwrap();

    let permuted = tensor.permute(&[2, 0, 1]).expect("valid axis order");

    assert_eq!(permuted.magnitudines(), vec![4, 2, 3]);
    assert_eq!(
        permuted.planata(),
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
    assert_eq!(permuted.planata(), vec![3.0, 5.0, 4.0, 6.0]);
}

#[test]
fn permute_accepts_rank_zero_empty_axis_list() {
    let tensor = Tensor::structa(vec![42_i32], &[]).unwrap();

    let permuted = tensor.permute(&[]).expect("rank-0 identity permute");

    assert_eq!(permuted.magnitudines(), Vec::<i64>::new());
    assert_eq!(permuted.planata(), vec![42]);
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
        result.planata(),
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
        result.planata(),
        vec![38.0, 44.0, 50.0, 56.0, 83.0, 98.0, 113.0, 128.0]
    );
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
    let b = Tensor::structa(vec![1.0, 2.0, 3.0], &[3]).unwrap();
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
    let result_data = result.planata();
    let expected: Vec<f32> = vec![-1.2247449, 0.0, 1.2247449, -1.2247449, 0.0, 1.2247449];
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

    let result_data = result.planata();
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
    let expected = [-1.2247449f32, 0.0, 1.2247449];
    for (a, e) in result.planata().iter().zip(expected.iter()) {
        assert!(
            (a - e).abs() < 1e-4,
            "rank-1 layernorm output {a} differs from expected {e}"
        );
    }
}

#[test]
fn layernorm_rank2_axis0_no_affine() {
    // 2×3 input, axis=0 (normalize each column independently)
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]).unwrap();
    let result = input.layernorm(0, 1e-5, None, None).unwrap();

    assert_eq!(result.magnitudines(), vec![2, 3]);
    let result_data = result.planata();

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
    let result_data = result.planata();

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
    let data = output.planata();
    let sum: f32 = data.iter().sum();
    assert!((sum - 1.0).abs() < 1e-6, "sum must be 1.0, got {sum}");
}

#[test]
fn softmax_rank2_sums_to_one_per_row() {
    let input = Tensor::structa(vec![1.0f32, 2.0, 3.0, 1.0, 2.0, 3.0], &[2, 3]).unwrap();
    let output = input.softmax().unwrap();
    let data = output.planata();
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
    let data = output.planata();
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
    let data = output.planata();
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
    let data = output.planata();
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
