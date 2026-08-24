//! M3 red driver: runner-facing max/min collection operations must have
//! distinct MIR identities before their runtime twins land.

use radix_mir::MirCollectionOp;

#[test]
fn tensor_extrema_collection_ops_have_distinct_identities() {
    assert_ne!(MirCollectionOp::TensorMax, MirCollectionOp::TensorMin);
    assert_eq!(format!("{:?}", MirCollectionOp::TensorMax), "TensorMax");
    assert_eq!(format!("{:?}", MirCollectionOp::TensorMin), "TensorMin");
}
