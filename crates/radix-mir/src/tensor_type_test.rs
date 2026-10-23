use super::*;
use radix_types::{NumericWidth, Primitive, RuntimeExtentId, TypeTable};

fn tensor_ty(types: &mut TypeTable, dims: Vec<IndexExpr>) -> MirType {
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let ids = dims
        .into_iter()
        .map(|dim| types.intern_index(dim))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(ids));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

fn runtime_dim() -> IndexExpr {
    IndexExpr::Runtime(RuntimeExtentId::ScalarParameter { parameter: 0 })
}

#[test]
fn tensor_dims_literal_shape_gives_literal_dims() {
    let mut types = TypeTable::new();
    let ty = tensor_ty(
        &mut types,
        vec![IndexExpr::Literal(3), IndexExpr::Literal(5)],
    );
    let validation = MirValidationContext::new(&types);
    assert_eq!(
        tensor_dims(ty, &validation),
        Ok(vec![Dim::Literal(3), Dim::Literal(5)])
    );
    assert_eq!(tensor_literal_dims(ty, &validation), Ok(vec![3, 5]));
}

#[test]
fn tensor_dims_runtime_extent_gives_runtime_dim() {
    let mut types = TypeTable::new();
    let ty = tensor_ty(&mut types, vec![IndexExpr::Literal(3), runtime_dim()]);
    let validation = MirValidationContext::new(&types);
    assert_eq!(
        tensor_dims(ty, &validation),
        Ok(vec![Dim::Literal(3), Dim::Runtime])
    );
}

#[test]
fn tensor_literal_dims_errors_on_runtime_with_unchanged_message() {
    let mut types = TypeTable::new();
    let ty = tensor_ty(&mut types, vec![runtime_dim(), IndexExpr::Literal(4)]);
    let validation = MirValidationContext::new(&types);
    assert_eq!(
        tensor_literal_dims(ty, &validation),
        Err("kernel collection op requires literal dimensions")
    );
}

#[test]
fn tensor_dims_rejects_non_tensor_type() {
    let mut types = TypeTable::new();
    let f32 = MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));
    let validation = MirValidationContext::new(&types);
    assert_eq!(
        tensor_dims(f32, &validation),
        Err("kernel collection op requires tensor types")
    );
}

#[test]
fn dims_product_checked_overflow_fails_closed() {
    assert_eq!(
        dims_product_checked(&[Dim::Literal(4_294_967_296), Dim::Literal(4_294_967_296)]),
        Err("kernel tensor shape element count overflows")
    );
}

#[test]
fn dims_product_checked_zero_dim_fails_closed() {
    assert_eq!(
        dims_product_checked(&[Dim::Literal(0), Dim::Literal(4)]),
        Err("kernel tensor shape requires positive literal dimensions")
    );
}

#[test]
fn dims_product_checked_zero_after_runtime_dim_fails_closed() {
    assert_eq!(
        dims_product_checked(&[Dim::Runtime, Dim::Literal(0)]),
        Err(ZERO_LITERAL_DIM_MESSAGE)
    );
}

#[test]
fn dims_product_checked_overflow_after_runtime_dim_fails_closed() {
    assert_eq!(
        dims_product_checked(&[
            Dim::Runtime,
            Dim::Literal(4_294_967_296),
            Dim::Literal(4_294_967_296),
        ]),
        Err(DIM_PRODUCT_OVERFLOW_MESSAGE)
    );
}

#[test]
fn dims_product_checked_runtime_dim_keeps_the_literal_message() {
    assert_eq!(
        dims_product_checked(&[Dim::Literal(3), Dim::Runtime]),
        Err("kernel collection op requires literal dimensions")
    );
}
