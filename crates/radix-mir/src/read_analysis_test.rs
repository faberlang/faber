use super::*;
use crate::{MirRuntimeCall, MirType};
use radix_lexer::Span;
use radix_types::TypeId;

#[test]
fn tensor_fill_shape_receiver_is_not_a_value_read() {
    let statement = MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(2))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFill),
                args: vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Place(MirPlace::local(MirLocalId(1))),
                ],
                return_ty: MirType::semantic(TypeId(0)),
            },
        },
        span: Span::default(),
    };

    assert_eq!(statement_read_locals(&statement), vec![MirLocalId(1)]);
}
