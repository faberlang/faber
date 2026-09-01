use super::{IndexExpr, Primitive, Type, TypeTable, width_pattern_applies, width_pattern_covers};

fn cap(types: &mut TypeTable, value: u64) -> super::IndexId {
    types.intern_index(IndexExpr::Literal(value))
}

#[test]
fn bounded_family_relation_is_directional() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let list = types.array(numerus);
    let cap4 = cap(&mut types, 4);
    let cap8 = cap(&mut types, 8);
    let list4 = types.bounded_array(numerus, cap4);
    let list8 = types.bounded_array(numerus, cap8);

    assert!(width_pattern_applies(&types, list, list4));
    assert!(width_pattern_applies(&types, list8, list4));
    assert!(width_pattern_covers(&types, list, list4));
    assert!(width_pattern_covers(&types, list4, list4));
    assert!(!width_pattern_covers(&types, list4, list8));
}

#[test]
fn numerus_width_rows_do_not_cross_without_equal_width() {
    let mut types = TypeTable::new();
    let i32 = types.intern(Type::SizedNumeric(
        Primitive::Numerus,
        super::NumericWidth::I32,
    ));
    let i64 = types.intern(Type::SizedNumeric(
        Primitive::Numerus,
        super::NumericWidth::I64,
    ));
    assert!(width_pattern_applies(&types, i64, i32));
    assert!(width_pattern_covers(&types, i32, i32));
    assert!(!width_pattern_covers(&types, i32, i64));
}
