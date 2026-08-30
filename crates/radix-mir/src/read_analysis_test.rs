use super::*;
use crate::{MirProjection, MirRuntimeCall, MirType, MirValueId};
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
    // The slot-level mirror applies the same fill-receiver exclusion.
    assert_eq!(
        statement_read_slots(&statement),
        vec![MirPlaceBase::Local(MirLocalId(1))]
    );
}

#[test]
fn statement_read_slots_cover_bare_and_place_temps() {
    let statement = MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::temp(MirTempId(3))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                args: vec![
                    MirOperand::Temp(MirTempId(0)),
                    MirOperand::Place(MirPlace {
                        base: MirPlaceBase::Temp(MirTempId(1)),
                        projections: Vec::new(),
                    }),
                ],
                return_ty: MirType::semantic(TypeId(0)),
            },
        },
        span: Span::default(),
    };

    assert_eq!(
        statement_read_slots(&statement),
        vec![
            MirPlaceBase::Temp(MirTempId(0)),
            MirPlaceBase::Temp(MirTempId(1))
        ]
    );
}

#[test]
fn terminator_read_slots_cover_temp_returns() {
    let terminator = MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(2))));
    assert_eq!(
        terminator_read_slots(&terminator),
        vec![MirPlaceBase::Temp(MirTempId(2))]
    );
}

#[test]
fn destination_place_and_slot_base_enumerate_temps_and_reject_projections() {
    let assign = MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::temp(MirTempId(4)),
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
                ty: MirType::semantic(TypeId(0)),
                span: Span::default(),
            },
        },
        span: Span::default(),
    };
    let place = statement_destination_place(&assign).expect("assign writes a destination");
    assert_eq!(
        place_slot_base(place),
        Some(MirPlaceBase::Temp(MirTempId(4)))
    );
    // The pre-existing local-only destination view stays local-only.
    assert_eq!(statement_destination(&assign), None);

    let projected = MirPlace {
        base: MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![MirProjection::Index(MirOperand::Temp(MirTempId(0)))],
    };
    assert_eq!(place_slot_base(&projected), None);
}
