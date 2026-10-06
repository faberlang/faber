use super::*;
use crate::{
    MirBlock, MirBlockId, MirFunctionId, MirLocalId, MirTempId, MirTerminator, MirType, MirValue,
    MirValueId,
};
use radix_lexer::Span;
use radix_types::TypeId;

fn ty() -> MirType {
    MirType::semantic(TypeId(0))
}

fn assign(place: MirPlace, kind: MirValueKind) -> crate::MirStatement {
    crate::MirStatement {
        kind: MirStatementKind::Assign {
            place,
            value: MirValue {
                id: MirValueId(0),
                kind,
                ty: ty(),
                span: Span::default(),
            },
        },
        span: Span::default(),
    }
}

fn function(statements: Vec<crate::MirStatement>, terminator: MirTerminatorKind) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: terminator,
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: ty(),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn returned(local: u32) -> MirTerminatorKind {
    MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(local)))))
}

#[test]
fn a_result_returned_directly_has_no_other_reader() {
    let f = function(vec![], returned(1));
    assert!(result_only_returned(&f, &MirPlace::local(MirLocalId(1))));
}

#[test]
fn a_result_forwarded_through_a_temp_to_the_return_has_no_other_reader() {
    // `temp0 ← softmax(..)`, `local1 ← temp0`, `return local1`.
    let f = function(
        vec![assign(
            MirPlace::local(MirLocalId(1)),
            MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
        )],
        returned(1),
    );
    assert!(result_only_returned(&f, &MirPlace::temp(MirTempId(0))));
}

/// A same-kernel consumer reads `output[index]`, which the per-row mapping has
/// not written yet when a different thread owns that row.
#[test]
fn a_result_read_by_another_statement_is_refused() {
    let f = function(
        vec![assign(
            MirPlace::local(MirLocalId(2)),
            MirValueKind::Binary {
                op: crate::MirBinOp::Add,
                lhs: MirOperand::Place(MirPlace::local(MirLocalId(1))),
                rhs: MirOperand::Constant(crate::MirConstant::Int(1)),
            },
        )],
        returned(2),
    );
    assert!(!result_only_returned(&f, &MirPlace::local(MirLocalId(1))));
}

#[test]
fn a_result_read_after_a_forward_is_refused() {
    // `local1 ← temp0`, then `local2 ← local1 + 1`, `return local2`.
    let f = function(
        vec![
            assign(
                MirPlace::local(MirLocalId(1)),
                MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
            ),
            assign(
                MirPlace::local(MirLocalId(2)),
                MirValueKind::Binary {
                    op: crate::MirBinOp::Add,
                    lhs: MirOperand::Place(MirPlace::local(MirLocalId(1))),
                    rhs: MirOperand::Constant(crate::MirConstant::Int(1)),
                },
            ),
        ],
        returned(2),
    );
    assert!(!result_only_returned(&f, &MirPlace::temp(MirTempId(0))));
}

#[test]
fn a_result_that_is_both_forwarded_and_returned_is_refused() {
    let f = function(
        vec![assign(
            MirPlace::local(MirLocalId(2)),
            MirValueKind::Operand(MirOperand::Place(MirPlace::local(MirLocalId(1)))),
        )],
        returned(1),
    );
    assert!(!result_only_returned(&f, &MirPlace::local(MirLocalId(1))));
}

#[test]
fn a_projected_destination_is_refused() {
    let mut place = MirPlace::local(MirLocalId(1));
    place
        .projections
        .push(crate::MirProjection::Index(MirOperand::Constant(
            crate::MirConstant::Int(0),
        )));
    let f = function(vec![], returned(1));
    assert!(!result_only_returned(&f, &place));
}
