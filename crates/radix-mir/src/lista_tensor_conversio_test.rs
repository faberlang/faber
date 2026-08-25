use super::*;
use radix_types::{IndexExpr, NumericWidth};

fn bounded_lista(types: &mut TypeTable, elem: TypeId, n: u64) -> TypeId {
    let capacity = types.intern_index(IndexExpr::Literal(n));
    types.intern(Type::BoundedArray {
        element: elem,
        capacity,
    })
}

#[test]
fn bounded_numeric_source_admits_padded_snapshot() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lista = bounded_lista(&mut types, f32, 8);
    let shape = types.intern_index(IndexExpr::Literal(8));
    let tensor = types.intern(Type::Tensor(f32, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Ok(())
    );
    assert_eq!(
        resolve_bounded_lista_padded_shape(lista, tensor, &types),
        Some((f32, 8))
    );
}

#[test]
fn bounded_non_numeric_source_rejects() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let lista = bounded_lista(&mut types, textus, 8);
    let shape = types.intern_index(IndexExpr::Literal(8));
    let tensor = types.intern(Type::Tensor(textus, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Err(ListaToTensorError::NonNumericElement)
    );
}

#[test]
fn bounded_capacity_mismatch_rejects() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lista = bounded_lista(&mut types, f32, 8);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor = types.intern(Type::Tensor(f32, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Err(ListaToTensorError::CapacityMismatch)
    );
}

#[test]
fn unbounded_source_stays_on_flat_path() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lista = types.array(f32);
    let shape = types.intern_index(IndexExpr::Literal(8));
    let tensor = types.intern(Type::Tensor(f32, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Ok(())
    );
    assert_eq!(
        resolve_bounded_lista_padded_shape(lista, tensor, &types),
        None
    );
}
