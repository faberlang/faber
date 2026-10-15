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

// -- where the result is finally returned, and the write-once rule --

fn local_place(id: u32) -> MirPlace {
    MirPlace::local(MirLocalId(id))
}

#[test]
fn a_directly_returned_result_is_returned_as_itself() {
    let f = function(vec![], returned(1));
    assert_eq!(
        result_returned_as(&f, &local_place(1)),
        Some(local_place(1))
    );
}

#[test]
fn a_forwarded_result_is_returned_as_the_end_of_the_chain() {
    // `temp0 ← softmax(..)`, `local1 ← temp0`, `local2 ← local1`, `return local2`.
    let f = function(
        vec![
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
            ),
            assign(
                local_place(2),
                MirValueKind::Operand(MirOperand::Place(local_place(1))),
            ),
        ],
        returned(2),
    );
    assert_eq!(
        result_returned_as(&f, &MirPlace::temp(MirTempId(0))),
        Some(local_place(2))
    );
}

/// `mut y ← x.softmax(); y ← other; return y` returns `other`, not the recipe's
/// result: a backend that treated `y` as the recipe's destination would skip
/// storing `other` and return the wrong tensor.
#[test]
fn a_forward_target_reassigned_before_the_return_is_refused() {
    let f = function(
        vec![
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
            ),
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Place(local_place(9))),
            ),
        ],
        returned(1),
    );
    assert_eq!(result_returned_as(&f, &MirPlace::temp(MirTempId(0))), None);
}

#[test]
fn a_destination_written_twice_is_refused() {
    let f = function(
        vec![
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Place(local_place(8))),
            ),
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Place(local_place(9))),
            ),
        ],
        returned(1),
    );
    assert_eq!(result_returned_as(&f, &local_place(1)), None);
}

#[test]
fn a_write_through_a_projection_is_refused() {
    let mut element = local_place(1);
    element
        .projections
        .push(crate::MirProjection::Index(MirOperand::Constant(
            crate::MirConstant::Int(0),
        )));
    let f = function(
        vec![assign(
            element,
            MirValueKind::Operand(MirOperand::Constant(crate::MirConstant::Int(1))),
        )],
        returned(1),
    );
    assert_eq!(result_returned_as(&f, &local_place(1)), None);
}

/// A recipe that writes the kernel's output parameter in a `void` kernel has no
/// `Return` of its result. Nothing reads it, so it is "only returned" for the
/// row mapping, but it is not a result the return can be taught to skip.
#[test]
fn a_result_nobody_returns_is_unread_but_not_returned_as_anything() {
    let f = function(vec![], MirTerminatorKind::Return(None));
    assert!(result_only_returned(&f, &local_place(1)));
    assert_eq!(result_returned_as(&f, &local_place(1)), None);
}

/// The write-once rule applies to the shared predicate too: a forward target
/// reassigned before the return is not the recipe's result.
#[test]
fn a_reassigned_forward_target_is_refused_by_the_shared_predicate_too() {
    let f = function(
        vec![
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
            ),
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Place(local_place(9))),
            ),
        ],
        returned(1),
    );
    assert!(!result_only_returned(&f, &MirPlace::temp(MirTempId(0))));
}

// -- a void kernel that writes its output parameter --

/// `output ⇇ x.recipe()`: `temp0 ← recipe(..)`, `output(local1) ← temp0`, and the
/// kernel returns nothing.
#[test]
fn a_void_kernel_forwarding_its_result_to_the_output_parameter_qualifies() {
    let f = function(
        vec![assign(
            local_place(1),
            MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
        )],
        MirTerminatorKind::Return(None),
    );
    assert!(result_unread_in_a_void_kernel(
        &f,
        &MirPlace::temp(MirTempId(0))
    ));
    assert_eq!(result_returned_as(&f, &MirPlace::temp(MirTempId(0))), None);
}

/// A discarded result in a kernel that returns something else: the return
/// writes `output[index]` from every thread, so a row owner's writes would race.
#[test]
fn a_kernel_returning_another_value_does_not_qualify() {
    let f = function(vec![], returned(7));
    assert!(!result_unread_in_a_void_kernel(
        &f,
        &MirPlace::temp(MirTempId(0))
    ));
}

#[test]
fn a_void_kernel_whose_result_is_read_does_not_qualify() {
    let f = function(
        vec![
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
            ),
            assign(
                local_place(2),
                MirValueKind::Binary {
                    op: crate::MirBinOp::Add,
                    lhs: MirOperand::Place(local_place(1)),
                    rhs: MirOperand::Constant(crate::MirConstant::Int(1)),
                },
            ),
        ],
        MirTerminatorKind::Return(None),
    );
    assert!(!result_unread_in_a_void_kernel(
        &f,
        &MirPlace::temp(MirTempId(0))
    ));
}

/// The output parameter written twice is not one recipe result.
#[test]
fn a_void_kernel_writing_the_output_twice_does_not_qualify() {
    let f = function(
        vec![
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
            ),
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Place(local_place(9))),
            ),
        ],
        MirTerminatorKind::Return(None),
    );
    assert!(!result_unread_in_a_void_kernel(
        &f,
        &MirPlace::temp(MirTempId(0))
    ));
}

// -- a place written at most once --

#[test]
fn a_forward_target_written_once_is_written_at_most_once() {
    let f = function(
        vec![assign(
            local_place(1),
            MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
        )],
        MirTerminatorKind::Return(None),
    );
    assert!(written_at_most_once(&f, &local_place(1)));
}

/// `s ← temp; s ← s / n`: the second write gives `s` another value.
#[test]
fn a_forward_target_written_again_is_not() {
    let f = function(
        vec![
            assign(
                local_place(1),
                MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
            ),
            assign(
                local_place(1),
                MirValueKind::Binary {
                    op: crate::MirBinOp::Add,
                    lhs: MirOperand::Place(local_place(1)),
                    rhs: MirOperand::Constant(crate::MirConstant::Int(1)),
                },
            ),
        ],
        MirTerminatorKind::Return(None),
    );
    assert!(!written_at_most_once(&f, &local_place(1)));
}

#[test]
fn a_write_through_a_projection_is_not_written_at_most_once() {
    let mut element = local_place(1);
    element
        .projections
        .push(crate::MirProjection::Index(MirOperand::Constant(
            crate::MirConstant::Int(0),
        )));
    let f = function(
        vec![assign(
            element.clone(),
            MirValueKind::Operand(MirOperand::Constant(crate::MirConstant::Int(1))),
        )],
        MirTerminatorKind::Return(None),
    );
    assert!(!written_at_most_once(&f, &local_place(1)));
    assert!(!written_at_most_once(&f, &element));
}
