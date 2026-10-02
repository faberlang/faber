//! Lazy coordinate resolution for shifted tensor views and named edge policies.
//!
//! The tensor carrier owns storage and strides. This module only describes
//! how a requested logical coordinate is displaced and resolved at an edge.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinEdgePolicy {
    Clamp,
    Reflect,
    Wrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomEdgePolicyError {
    CoordinateRankMismatch,
    CoordinateArithmeticOverflow,
    CoordinateOutsidePolicyRange,
    MappedCoordinateRankMismatch,
    MappedCoordinateOutOfBounds,
}

/// The logical-coordinate state carried by a shifted tensor view. A resolved
/// policy remains a view; only its coordinate lookup changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TensorEdgeResolution<P> {
    /// A `shift` view whose out-of-range reads are optional.
    Optional,
    /// A built-in `limes` mapping.
    Builtin(BuiltinEdgePolicy),
    /// A typechecked custom policy's dispatch identity.
    Custom(P),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TensorEdgeDescriptor<P> {
    pub(crate) shift: Vec<i128>,
    pub(crate) resolution: TensorEdgeResolution<P>,
}

impl<P> TensorEdgeDescriptor<P> {
    /// Construct the unresolved lazy view returned by `shift`.
    #[must_use]
    pub(crate) fn shifted(shift: Vec<i128>) -> Self {
        Self {
            shift,
            resolution: TensorEdgeResolution::Optional,
        }
    }

    /// Resolve an optional edge view without copying its source data. A view
    /// cannot be resolved twice or changed back to the optional state.
    #[must_use]
    pub(crate) fn limes(self, resolution: TensorEdgeResolution<P>) -> Option<Self> {
        if matches!(self.resolution, TensorEdgeResolution::Optional)
            && !matches!(resolution, TensorEdgeResolution::Optional)
        {
            Some(Self { resolution, ..self })
        } else {
            None
        }
    }
}

/// Add a shift in logical coordinate space without narrowing at integer
/// boundaries. `None` means the coordinate and shift ranks disagree or their
/// widened sum cannot be represented.
#[must_use]
pub(crate) fn shifted_coordinate(coordinate: &[i128], shift: &[i128]) -> Option<Vec<i128>> {
    if coordinate.len() != shift.len() {
        return None;
    }
    coordinate
        .iter()
        .zip(shift)
        .map(|(coordinate, shift)| coordinate.checked_add(*shift))
        .collect()
}

/// Resolve an optional shifted read. Any coordinate outside either side of
/// any axis produces no element.
#[must_use]
pub(crate) fn resolve_optional(
    coordinate: &[i128],
    shift: &[i128],
    extents: &[usize],
) -> Option<Vec<usize>> {
    let shifted = shifted_coordinate(coordinate, shift)?;
    in_bounds_coordinate(&shifted, extents)
}

/// Resolve a shifted coordinate through one of the explicit built-in edge
/// policies. Empty axes have no element; singleton axes always map to zero.
#[must_use]
pub(crate) fn resolve_builtin(
    policy: BuiltinEdgePolicy,
    coordinate: &[i128],
    shift: &[i128],
    extents: &[usize],
) -> Option<Vec<usize>> {
    let shifted = shifted_coordinate(coordinate, shift)?;
    if shifted.len() != extents.len() {
        return None;
    }
    shifted
        .iter()
        .zip(extents)
        .map(|(coordinate, extent)| map_axis(policy, *coordinate, *extent))
        .collect()
}

/// Apply a user coordinate remap, read the selected element, then apply its
/// optional value transform to the displaced coordinate and value.
pub(crate) fn resolve_custom_value<T, E, M, R, V>(
    coordinate: &[i128],
    shift: &[i128],
    extents: &[usize],
    remap: M,
    read: R,
    transform: V,
) -> Result<T, CustomEdgeValueError<E>>
where
    M: FnOnce(&[i64]) -> Vec<i64>,
    R: FnOnce(&[usize]) -> Result<T, E>,
    V: FnOnce(&[i64], T) -> T,
{
    let shifted = checked_shifted_coordinate(coordinate, shift, extents)
        .map_err(CustomEdgeValueError::Policy)?;
    if let Some(in_bounds) = in_bounds_coordinate(&shifted, extents) {
        return read(&in_bounds).map_err(CustomEdgeValueError::Read);
    }
    let policy_coordinate =
        checked_policy_coordinate(&shifted).map_err(CustomEdgeValueError::Policy)?;
    let mapped: Vec<i128> = remap(&policy_coordinate)
        .into_iter()
        .map(i128::from)
        .collect();
    let mapped =
        checked_mapped_coordinate(&mapped, extents).map_err(CustomEdgeValueError::Policy)?;
    read(&mapped)
        .map(|value| transform(&policy_coordinate, value))
        .map_err(CustomEdgeValueError::Read)
}

/// Failures while resolving a custom edge or reading its validated target.
/// Once `limes` resolves a custom policy, neither kind is an optional hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomEdgeValueError<E> {
    Policy(CustomEdgePolicyError),
    Read(E),
}

fn checked_mapped_coordinate(
    coordinate: &[i128],
    extents: &[usize],
) -> Result<Vec<usize>, CustomEdgePolicyError> {
    if coordinate.len() != extents.len() {
        return Err(CustomEdgePolicyError::MappedCoordinateRankMismatch);
    }
    in_bounds_coordinate(coordinate, extents)
        .ok_or(CustomEdgePolicyError::MappedCoordinateOutOfBounds)
}

fn checked_policy_coordinate(coordinate: &[i128]) -> Result<Vec<i64>, CustomEdgePolicyError> {
    coordinate
        .iter()
        .map(|coordinate| {
            i64::try_from(*coordinate)
                .map_err(|_| CustomEdgePolicyError::CoordinateOutsidePolicyRange)
        })
        .collect()
}

fn checked_shifted_coordinate(
    coordinate: &[i128],
    shift: &[i128],
    extents: &[usize],
) -> Result<Vec<i128>, CustomEdgePolicyError> {
    if coordinate.len() != shift.len() || coordinate.len() != extents.len() {
        return Err(CustomEdgePolicyError::CoordinateRankMismatch);
    }
    coordinate
        .iter()
        .zip(shift)
        .map(|(coordinate, shift)| {
            coordinate
                .checked_add(*shift)
                .ok_or(CustomEdgePolicyError::CoordinateArithmeticOverflow)
        })
        .collect()
}

fn in_bounds_coordinate(coordinate: &[i128], extents: &[usize]) -> Option<Vec<usize>> {
    if coordinate.len() != extents.len() {
        return None;
    }
    coordinate
        .iter()
        .zip(extents)
        .map(|(coordinate, extent)| {
            let extent = i128::try_from(*extent).ok()?;
            if *coordinate < 0 || *coordinate >= extent {
                return None;
            }
            usize::try_from(*coordinate).ok()
        })
        .collect()
}

fn map_axis(policy: BuiltinEdgePolicy, coordinate: i128, extent: usize) -> Option<usize> {
    if extent == 0 {
        return None;
    }
    if extent == 1 {
        return Some(0);
    }
    let extent = i128::try_from(extent).ok()?;
    let mapped = match policy {
        BuiltinEdgePolicy::Clamp => coordinate.clamp(0, extent - 1),
        BuiltinEdgePolicy::Reflect => {
            // Reflect around the endpoint without duplicating it. For n > 1,
            // the period is 2*(n-1): -1 maps to 1 and n maps to n-2.
            let period = (extent - 1).checked_mul(2)?;
            let folded = coordinate.rem_euclid(period);
            if folded < extent {
                folded
            } else {
                period - folded
            }
        }
        BuiltinEdgePolicy::Wrap => coordinate.rem_euclid(extent),
    };
    usize::try_from(mapped).ok()
}

#[cfg(test)]
mod tests {
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
            |coordinate| vec![-coordinate[0]],
            |index| {
                values
                    .get(index[0])
                    .copied()
                    .ok_or("invalid backing offset")
            },
            |coordinate, value| if coordinate[0] < 0 { -value } else { value },
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
                &[values.len()],
                |_| vec![9],
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
                &[4],
                &[0],
                &[values.len()],
                |_| vec![0, 1],
                |index| {
                    values
                        .get(index[0])
                        .copied()
                        .ok_or("invalid backing offset")
                },
                |_, _| panic!("an invalid custom rank cannot reach value mapping"),
            ),
            Err(CustomEdgeValueError::Policy(
                CustomEdgePolicyError::MappedCoordinateRankMismatch
            ))
        );
        assert_eq!(
            resolve_custom_value(
                &[i128::from(i64::MAX)],
                &[1],
                &[4],
                |_| panic!("a coordinate outside i64 cannot reach Policy"),
                |_| Err("read failure"),
                |_, value: i128| value,
            ),
            Err(CustomEdgeValueError::Policy(
                CustomEdgePolicyError::CoordinateOutsidePolicyRange
            ))
        );
        assert_eq!(
            resolve_custom_value(
                &[1],
                &[0],
                &[values.len()],
                |_| panic!("in-range coordinate bypasses remap"),
                |_| Err("read failure"),
                |_: &[i64], value: i128| value,
            ),
            Err(CustomEdgeValueError::Read("read failure"))
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
}
