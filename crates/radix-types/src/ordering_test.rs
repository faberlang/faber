use super::*;
use crate::numeric_width::NumericWidth;

#[test]
fn builtins_conform_and_decimals_do_not() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let u8_ty = types.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8));
    let float = types.primitive(Primitive::Fractus);
    let f32_ty = types.intern(Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32));
    let text = types.primitive(Primitive::Textus);
    let instans = types.primitive(Primitive::Instans);
    let d64 = types.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::D64));
    let boolean = types.primitive(Primitive::Bivalens);

    assert_eq!(
        builtin_ordering_kind(&types, int),
        Some(OrderingKind::Integer)
    );
    assert_eq!(
        builtin_ordering_kind(&types, u8_ty),
        Some(OrderingKind::Integer)
    );
    assert_eq!(
        builtin_ordering_kind(&types, float),
        Some(OrderingKind::Float)
    );
    assert_eq!(
        builtin_ordering_kind(&types, f32_ty),
        Some(OrderingKind::Float)
    );
    assert_eq!(
        builtin_ordering_kind(&types, text),
        Some(OrderingKind::Text)
    );
    assert_eq!(
        builtin_ordering_kind(&types, instans),
        Some(OrderingKind::Instans)
    );
    assert_eq!(builtin_ordering_kind(&types, d64), None);
    assert_eq!(builtin_ordering_kind(&types, boolean), None);
}

#[test]
fn tuples_conform_only_when_every_element_does() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let text = types.primitive(Primitive::Textus);
    let boolean = types.primitive(Primitive::Bivalens);
    let pair = types.intern(Type::Tuple(vec![int, text]));
    let bad = types.intern(Type::Tuple(vec![int, boolean]));

    assert_eq!(
        builtin_ordering_kind(&types, pair),
        Some(OrderingKind::Tuple(vec![
            OrderingKind::Integer,
            OrderingKind::Text
        ]))
    );
    assert_eq!(builtin_ordering_kind(&types, bad), None);
}

#[test]
fn genus_and_param_conform_through_the_caller() {
    let mut types = TypeTable::new();
    let genus = types.intern(Type::Struct(DefId(5000)));
    let other = types.intern(Type::Struct(DefId(5001)));
    let kind = |ty| ordering_kind(&types, ty, &|def| def == DefId(5000), &|_| false);
    assert_eq!(kind(genus), Some(OrderingKind::Genus(DefId(5000))));
    assert_eq!(kind(other), None);
}
