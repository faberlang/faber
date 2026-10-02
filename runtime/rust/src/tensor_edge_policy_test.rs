use super::{
    ClampEdgePolicy, ReflectEdgePolicy, Tensor, TensorEdgePolicy, TensorEdgePolicyProvider,
    WrapEdgePolicy,
};

fn forwarded_policy<T, const R: usize, P>() -> TensorEdgePolicy<T>
where
    P: TensorEdgePolicyProvider<T, R>,
{
    P::edge_policy()
}

fn generic_limes<T, const R: usize, P>(
    tensor: &Tensor<T>,
    offsets: &[i64],
) -> Result<Tensor<T>, &'static str>
where
    T: Clone + Default,
    P: TensorEdgePolicyProvider<T, R>,
{
    tensor.shift(offsets)?.limes(P::edge_policy())
}

#[test]
fn builtin_tags_forward_through_element_and_rank_generics() {
    assert!(matches!(
        forwarded_policy::<i32, 1, ClampEdgePolicy>(),
        TensorEdgePolicy::Clamp
    ));
    assert!(matches!(
        forwarded_policy::<f32, 4, ReflectEdgePolicy>(),
        TensorEdgePolicy::Reflect
    ));
    assert!(matches!(
        forwarded_policy::<u8, 3, WrapEdgePolicy>(),
        TensorEdgePolicy::Wrap
    ));
}

#[test]
fn generic_provider_forwards_into_limes() {
    let tensor = Tensor::structa(vec![10_i32, 20, 30], &[3]).unwrap();

    let wrapped = generic_limes::<i32, 1, WrapEdgePolicy>(&tensor, &[1]).unwrap();
    let clamped = generic_limes::<i32, 1, ClampEdgePolicy>(&tensor, &[1]).unwrap();
    let reflected = generic_limes::<i32, 1, ReflectEdgePolicy>(&tensor, &[1]).unwrap();

    assert_eq!(wrapped.planata().unwrap(), vec![20, 30, 10]);
    assert_eq!(clamped.planata().unwrap(), vec![20, 30, 30]);
    assert_eq!(reflected.planata().unwrap(), vec![20, 30, 20]);
}

#[test]
fn builtin_policy_markers_are_zero_sized() {
    assert_eq!(std::mem::size_of::<ClampEdgePolicy>(), 0);
    assert_eq!(std::mem::size_of::<ReflectEdgePolicy>(), 0);
    assert_eq!(std::mem::size_of::<WrapEdgePolicy>(), 0);
}
