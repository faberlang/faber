//! S0-R1: interface snapshots serialize kinded applied size arguments.

use crate::file_interface::{
    import_interface_type, snapshot_interface_type, InterfaceAppliedArg, InterfaceIndexSnapshot,
    InterfaceTypeSnapshot,
};
use crate::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};
use radix_lexer::Interner;
use radix_types::AppliedArg;

#[test]
fn file_interface_preserves_generic_size_literal_round_trip() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = types.intern_index(IndexExpr::Literal(3));
    let base = types.vector_with_width(elem, width);
    let applied = types.intern(Type::Applied(base, vec![AppliedArg::Size(width)]));

    let snapshot =
        snapshot_interface_type(applied, &types, &interner).expect("snapshot applied size");

    let InterfaceTypeSnapshot::Applied(_, args) = &snapshot else {
        panic!("expected applied snapshot, got {snapshot:?}");
    };
    assert_eq!(
        args,
        &vec![InterfaceAppliedArg::Size(InterfaceIndexSnapshot::Literal(
            3
        ))]
    );

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
        .expect("import applied size");

    let Type::Applied(_, args) = target_types.get(imported) else {
        panic!("expected imported applied type");
    };
    assert_eq!(args.len(), 1);
    match &args[0] {
        AppliedArg::Size(index) => {
            assert!(matches!(
                target_types.get_index(*index),
                IndexExpr::Literal(3)
            ));
        }
        other => panic!("expected size argument, got {other:?}"),
    }
}

#[test]
fn file_interface_preserves_generic_size_param_round_trip() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let m = interner.intern("M");
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = types.intern_index(IndexExpr::Param(m));
    let base = types.vector_with_width(elem, width);
    let applied = types.intern(Type::Applied(base, vec![AppliedArg::Size(width)]));

    let snapshot =
        snapshot_interface_type(applied, &types, &interner).expect("snapshot applied size param");

    let InterfaceTypeSnapshot::Applied(_, args) = &snapshot else {
        panic!("expected applied snapshot, got {snapshot:?}");
    };
    assert_eq!(
        args,
        &vec![InterfaceAppliedArg::Size(InterfaceIndexSnapshot::Param(
            "M".to_owned()
        ))]
    );

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
        .expect("import applied size param");

    let Type::Applied(_, args) = target_types.get(imported) else {
        panic!("expected imported applied type");
    };
    match &args[0] {
        AppliedArg::Size(index) => {
            assert!(
                matches!(target_types.get_index(*index), IndexExpr::Param(sym) if target_interner.resolve(*sym) == "M")
            );
        }
        other => panic!("expected size argument, got {other:?}"),
    }
}

#[test]
fn file_interface_preserves_generic_kinded_args_in_order() {
    // A mixed application `[Type(textus), Size(4)]` keeps argument kind and
    // order across the snapshot boundary; sizes never become fake types.
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let textus = types.primitive(Primitive::Textus);
    let width = types.intern_index(IndexExpr::Literal(4));
    let base = types.array(textus);
    let applied = types.intern(Type::Applied(
        base,
        vec![AppliedArg::Type(textus), AppliedArg::Size(width)],
    ));

    let snapshot =
        snapshot_interface_type(applied, &types, &interner).expect("snapshot mixed args");

    let InterfaceTypeSnapshot::Applied(_, args) = &snapshot else {
        panic!("expected applied snapshot, got {snapshot:?}");
    };
    assert_eq!(
        args,
        &vec![
            InterfaceAppliedArg::Type(InterfaceTypeSnapshot::Primitive(Primitive::Textus)),
            InterfaceAppliedArg::Size(InterfaceIndexSnapshot::Literal(4)),
        ]
    );
}
