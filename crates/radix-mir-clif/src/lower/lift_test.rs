//! Which list operations are elementwise lifts, and which operator each is.

use super::{Lift, list_lift, mismatch_message};
use radix_mir::{MirBinOp, MirCollectionOp};

#[test]
fn arithmetic_and_comparison_lifts_map_to_their_scalar_operator() {
    let table = [
        (MirCollectionOp::ArrayAdd, Lift::Binary(MirBinOp::Add)),
        (MirCollectionOp::ArraySub, Lift::Binary(MirBinOp::Sub)),
        (MirCollectionOp::ArrayMul, Lift::Binary(MirBinOp::Mul)),
        (MirCollectionOp::ArrayDiv, Lift::Binary(MirBinOp::Div)),
        (MirCollectionOp::ArrayMod, Lift::Binary(MirBinOp::Mod)),
        (MirCollectionOp::ArrayTrueDiv, Lift::TrueDiv),
        (MirCollectionOp::ArrayNeg, Lift::Negate),
        (MirCollectionOp::ArrayNot, Lift::Not),
        (MirCollectionOp::ArrayAnd, Lift::Binary(MirBinOp::And)),
        (MirCollectionOp::ArrayOr, Lift::Binary(MirBinOp::Or)),
        (MirCollectionOp::ArrayLt, Lift::Binary(MirBinOp::Lt)),
        (MirCollectionOp::ArrayGt, Lift::Binary(MirBinOp::Gt)),
        (MirCollectionOp::ArrayLtEq, Lift::Binary(MirBinOp::LtEq)),
        (MirCollectionOp::ArrayGtEq, Lift::Binary(MirBinOp::GtEq)),
        (MirCollectionOp::ArrayValueEq, Lift::Binary(MirBinOp::Eq)),
        (
            MirCollectionOp::ArrayValueNotEq,
            Lift::Binary(MirBinOp::NotEq),
        ),
    ];
    for (op, lift) in table {
        assert_eq!(list_lift(op), Some(lift), "{op:?}");
        assert!(mismatch_message(lift).ends_with("length mismatch"));
    }
}

#[test]
fn other_collection_operations_are_not_lifts() {
    assert_eq!(list_lift(MirCollectionOp::Length), None);
    assert_eq!(list_lift(MirCollectionOp::Append), None);
    assert_eq!(list_lift(MirCollectionOp::Sum), None);
}
