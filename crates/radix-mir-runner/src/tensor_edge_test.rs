use super::{
    BuiltinEdgePolicy, CustomEdgePolicyError, CustomEdgeValueError, TensorEdgeDescriptor,
    TensorEdgeResolution, resolve_builtin, resolve_custom_value, resolve_optional,
};

#[test]
fn unresolved_shift_returns_none_on_both_sides() {
    assert_eq!(resolve_optional(&[0], &[-1], &[3]), None);
    assert_eq!(resolve_optional(&[2], &[1], &[3]), None);
    assert_eq!(resolve_optional(&[1], &[1], &[3]), Some(vec![2]));
}

#[test]
fn limes_keeps_the_shifted_descriptor_and_resolves_it_once() {
    let shifted = TensorEdgeDescriptor::<u32>::shifted(vec![1, -2]);
    let resolved = shifted
        .limes(TensorEdgeResolution::Builtin(BuiltinEdgePolicy::Reflect))
        .expect("an optional edge view resolves once");
    assert_eq!(resolved.shift, vec![1, -2]);
    assert_eq!(
        resolved.resolution,
        TensorEdgeResolution::Builtin(BuiltinEdgePolicy::Reflect)
    );
    assert!(
        resolved
            .limes(TensorEdgeResolution::Builtin(BuiltinEdgePolicy::Wrap))
            .is_none()
    );
}

#[test]
fn builtins_map_negative_and_positive_edges() {
    let coordinate = [-1_i128];
    let shift = [0_i128];
    let extents = [4];
    assert_eq!(
        resolve_builtin(BuiltinEdgePolicy::Clamp, &coordinate, &shift, &extents),
        Some(vec![0])
    );
    assert_eq!(
        resolve_builtin(BuiltinEdgePolicy::Reflect, &coordinate, &shift, &extents),
        Some(vec![1])
    );
    assert_eq!(
        resolve_builtin(BuiltinEdgePolicy::Wrap, &coordinate, &shift, &extents),
        Some(vec![3])
    );
    let coordinate = [4_i128];
    assert_eq!(
        resolve_builtin(BuiltinEdgePolicy::Reflect, &coordinate, &shift, &extents),
        Some(vec![2])
    );
    assert_eq!(
        resolve_builtin(BuiltinEdgePolicy::Wrap, &coordinate, &shift, &extents),
        Some(vec![0])
    );
}

#[test]
fn singleton_and_zero_axes_are_defined() {
    for policy in [
        BuiltinEdgePolicy::Clamp,
        BuiltinEdgePolicy::Reflect,
        BuiltinEdgePolicy::Wrap,
    ] {
        assert_eq!(resolve_builtin(policy, &[-19], &[2], &[1]), Some(vec![0]));
        assert_eq!(resolve_builtin(policy, &[-19], &[2], &[0]), None);
    }
}

#[test]
fn every_axis_participates_in_shift_resolution() {
    assert_eq!(resolve_optional(&[0, 0], &[1, -1], &[2, 3]), None);
    assert_eq!(
        resolve_builtin(BuiltinEdgePolicy::Wrap, &[0, 0], &[1, -1], &[2, 3]),
        Some(vec![1, 2])
    );
    assert_eq!(
        resolve_builtin(BuiltinEdgePolicy::Reflect, &[-1, 4], &[0, 0], &[1, 0]),
        None
    );
}

#[test]
fn custom_edge_remap_and_value_transform_apply_only_at_boundary() {
    let values = [10_i128, 20, 30, 40];
    let resolved = resolve_custom_value(
        &[0],
        &[-1],
        &[values.len()],
        |coordinate| Ok(vec![-coordinate[0]]),
        |index| {
            values
                .get(index[0])
                .copied()
                .ok_or("invalid backing offset")
        },
        |coordinate, value| Ok(if coordinate[0] < 0 { -value } else { value }),
    );
    assert_eq!(resolved, Ok(-20_i128));
    let interior = resolve_custom_value(
        &[0],
        &[0],
        &[values.len()],
        |_| panic!("in-range coordinates bypass the edge remapper"),
        |index| {
            values
                .get(index[0])
                .copied()
                .ok_or("invalid backing offset")
        },
        |_, _| panic!("in-range values bypass the edge transform"),
    );
    assert_eq!(interior, Ok(10_i128));
    assert_eq!(
        resolve_custom_value(
            &[4],
            &[0],
            &[4],
            |_| Ok(vec![0, 1]),
            |_| -> Result<i128, &'static str> { panic!("invalid rank cannot reach backing read") },
            |_, _| panic!("invalid rank cannot reach value transform"),
        ),
        Err(CustomEdgeValueError::Policy(
            CustomEdgePolicyError::MappedCoordinateRankMismatch
        ))
    );
    assert_eq!(
        resolve_custom_value(
            &[4],
            &[0],
            &[values.len()],
            |_| Ok(vec![9]),
            |index| values
                .get(index[0])
                .copied()
                .ok_or("invalid backing offset"),
            |_, _| panic!("an invalid custom coordinate cannot reach value mapping"),
        ),
        Err(CustomEdgeValueError::Policy(
            CustomEdgePolicyError::MappedCoordinateOutOfBounds
        ))
    );
    assert_eq!(
        resolve_custom_value(
            &[1],
            &[0],
            &[values.len()],
            |_| panic!("in-range coordinate bypasses remap"),
            |_| -> Result<i64, &'static str> { Err("read failure") },
            |_, value| Ok(value),
        ),
        Err(CustomEdgeValueError::Read("read failure"))
    );
    assert_eq!(
        resolve_custom_value(
            &[4],
            &[0],
            &[values.len()],
            |_| Err("remap failure"),
            |index| {
                values
                    .get(index[0])
                    .copied()
                    .ok_or("invalid backing offset")
            },
            |_, value| Ok(value),
        ),
        Err(CustomEdgeValueError::Remap("remap failure"))
    );
    assert_eq!(
        resolve_custom_value(
            &[4],
            &[0],
            &[values.len()],
            |_| Ok(vec![0]),
            |index| {
                values
                    .get(index[0])
                    .copied()
                    .ok_or("invalid backing offset")
            },
            |_, _| Err("transform failure"),
        ),
        Err(CustomEdgeValueError::Transform("transform failure"))
    );
    assert_eq!(
        resolve_custom_value(
            &[i128::from(i64::MAX)],
            &[1],
            &[4],
            |_| panic!("a vector<int> coordinate outside i64 cannot reach Policy"),
            |_| -> Result<i128, &'static str> {
                panic!("invalid coordinate cannot reach backing read")
            },
            |_, _| panic!("invalid coordinate cannot reach value transform"),
        ),
        Err(CustomEdgeValueError::Policy(
            CustomEdgePolicyError::CoordinateOutsidePolicyRange
        ))
    );
}

#[test]
fn widened_shift_and_euclidean_mapping_handle_i64_edges() {
    assert_eq!(
        resolve_builtin(
            BuiltinEdgePolicy::Wrap,
            &[i128::from(i64::MIN)],
            &[-1],
            &[7]
        ),
        Some(vec![5])
    );
    assert_eq!(
        resolve_builtin(
            BuiltinEdgePolicy::Reflect,
            &[i128::from(i64::MAX)],
            &[i128::from(i64::MAX)],
            &[4]
        ),
        Some(vec![2])
    );
}

#[test]
fn coordinate_rank_mismatch_fails_closed() {
    assert_eq!(resolve_optional(&[0, 0], &[1], &[2, 2]), None);
    assert_eq!(
        resolve_builtin(BuiltinEdgePolicy::Wrap, &[0, 0], &[0, 0], &[2]),
        None
    );
}
