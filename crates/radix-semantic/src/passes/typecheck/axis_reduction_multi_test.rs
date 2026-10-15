//! Multi-dimension rest form for axis reductions (operator ruling 2026-10-13,
//! need item 6): `t.summa(axis 1, axis 2)` removes every named axis in
//! one grouped reduction, with each axis numbered in the source tensor.

use crate::test_support::{analyze_ok, assert_rejects_with_issue, entry_local_tys};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type};

/// Shape of the last `incipit` local, which must be a literal-extent `tf32`.
fn result_shape(source: &str) -> Vec<i64> {
    let unit = analyze_ok(source);
    let ty = *entry_local_tys(&unit).last().expect("result local");
    let Type::Tensor(element, shape) = unit.types.get(ty) else {
        panic!("expected a tensor result, got {:?}", unit.types.get(ty));
    };
    assert_eq!(
        unit.types.get(*element),
        &Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32),
        "expected a tf32 element"
    );
    let IndexExpr::Tuple(dims) = unit.types.get_index(*shape) else {
        panic!(
            "expected a resolved shape, got {:?}",
            unit.types.get_index(*shape)
        );
    };
    dims.iter()
        .map(|dim| match unit.types.get_index(*dim) {
            IndexExpr::Literal(extent) => i64::try_from(*extent).expect("extent fits i64"),
            other => panic!("expected a literal extent, got {other:?}"),
        })
        .collect()
}

#[test]
fn sum_over_two_axes_removes_both() {
    let shape = result_shape(
        r"incipit {
    sit tensor<f32, [2, 3, 4, 5]> g ← vacua
    sit tensor<f32, [2, 5]> kept ← g.summa(axis 1, axis 2)
    nota kept
}",
    );
    assert_eq!(shape, vec![2, 5]);
}

#[test]
fn repeated_axis_is_rejected() {
    assert_rejects_with_issue(
        r"incipit {
    sit tensor<f32, [2, 3, 4, 5]> g ← vacua
    sit tensor<f32, [2, 5]> kept ← g.summa(axis 1, axis 1)
    nota kept
}",
        "axis_reduction_repeated",
    );
}

#[test]
fn mean_over_two_axes_divides_by_product() {
    let shape = result_shape(
        r"incipit {
    sit tensor<f32, [2, 3, 4, 5]> g ← vacua
    sit tensor<f32, [2, 5]> kept ← g.media(axis 1, axis 2)
    nota kept
}",
    );
    assert_eq!(shape, vec![2, 5]);
}
