use super::*;
use crate::{
    MirAggregateKind, MirBlock, MirBlockId, MirCallee, MirClosureCallee, MirClosureEnvironmentId,
    MirClosureValue, MirConstant, MirFunctionId, MirKeyValueOperand, MirLocal, MirNamedOperand,
    MirOptionChainLink, MirOptionUnwrapMode, MirProjection, MirRuntimeCall, MirTemp, MirTerminator,
    MirType, MirValueId,
};
use radix_lexer::{Span, Symbol};
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

// ---------------------------------------------------------------------------
// Temp-read coverage (A-34): every operand-carrying MIR shape.
// ---------------------------------------------------------------------------

fn test_value(kind: MirValueKind) -> MirValue {
    MirValue {
        id: MirValueId(0),
        kind,
        ty: MirType::semantic(TypeId(0)),
        span: Span::default(),
    }
}

fn test_call(callee: MirCallee, args: Vec<MirOperand>) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call {
            destination: None,
            callee,
            args,
        },
        span: Span::default(),
    }
}

fn test_construct(fields: MirAggregateFields) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace::temp(MirTempId(90)),
            aggregate: MirAggregate {
                kind: MirAggregateKind::Record,
                ty: MirType::semantic(TypeId(0)),
                fields,
            },
        },
        span: Span::default(),
    }
}

fn closure_callee(environment: MirOperand) -> MirClosureCallee {
    MirClosureCallee {
        function: MirFunctionId(0),
        environment,
        environment_id: MirClosureEnvironmentId(0),
    }
}

fn temp_operand(id: u32) -> MirOperand {
    MirOperand::Temp(MirTempId(id))
}

#[test]
fn statement_reads_temp_covers_call_arguments_callees_and_aggregate_fields() {
    let call_arg = test_call(MirCallee::Function(MirFunctionId(0)), vec![temp_operand(1)]);
    assert!(statement_reads_temp(&call_arg, MirTempId(1)));
    assert!(!statement_reads_temp(&call_arg, MirTempId(2)));

    let value_callee = test_call(MirCallee::Value(temp_operand(2)), Vec::new());
    assert!(statement_reads_temp(&value_callee, MirTempId(2)));

    let closure_target = closure_callee(temp_operand(3));
    let closure_call = test_call(MirCallee::Closure(closure_target), Vec::new());
    assert!(statement_reads_temp(&closure_call, MirTempId(3)));

    let spread_item = MirAggregateItem::Spread(temp_operand(4));
    let spread = test_construct(MirAggregateFields::Ordered(vec![spread_item]));
    assert!(statement_reads_temp(&spread, MirTempId(4)));

    let named_item = MirNamedOperand {
        name: Symbol(0),
        value: temp_operand(5),
    };
    let named = test_construct(MirAggregateFields::Named(vec![named_item]));
    assert!(statement_reads_temp(&named, MirTempId(5)));

    let keyed_item = MirKeyValueOperand {
        key: temp_operand(6),
        value: temp_operand(7),
    };
    let keyed = test_construct(MirAggregateFields::Keyed(vec![keyed_item]));
    assert!(statement_reads_temp(&keyed, MirTempId(6)));
    assert!(statement_reads_temp(&keyed, MirTempId(7)));

    assert!(!statement_reads_temp(&call_arg, MirTempId(99)));
}

#[test]
fn value_reads_temp_covers_extent_option_and_closure_operands() {
    let extent = test_value(MirValueKind::TensorExtent {
        tensor: temp_operand(0),
        axis: 0,
    });
    assert!(value_reads_temp(&extent, MirTempId(0)));
    assert!(!value_reads_temp(&extent, MirTempId(1)));

    let some = test_value(MirValueKind::Option(MirOptionOp::Some(temp_operand(2))));
    assert!(value_reads_temp(&some, MirTempId(2)));

    let unwrap = test_value(MirValueKind::Option(MirOptionOp::Unwrap {
        value: temp_operand(3),
        mode: MirOptionUnwrapMode::Assume,
    }));
    assert!(value_reads_temp(&unwrap, MirTempId(3)));

    let coalesce = test_value(MirValueKind::Option(MirOptionOp::Coalesce {
        value: MirOperand::Constant(MirConstant::Int(0)),
        fallback: temp_operand(4),
    }));
    assert!(value_reads_temp(&coalesce, MirTempId(4)));

    let chain_index = test_value(MirValueKind::Option(MirOptionOp::Chain {
        base: MirOperand::Constant(MirConstant::Int(0)),
        link: MirOptionChainLink::Index(temp_operand(5)),
    }));
    assert!(value_reads_temp(&chain_index, MirTempId(5)));

    let chain_call = test_value(MirValueKind::Option(MirOptionOp::Chain {
        base: MirOperand::Constant(MirConstant::Int(0)),
        link: MirOptionChainLink::Call {
            callee: MirCallee::Value(temp_operand(6)),
            args: vec![temp_operand(7)],
        },
    }));
    assert!(value_reads_temp(&chain_call, MirTempId(6)));
    assert!(value_reads_temp(&chain_call, MirTempId(7)));

    let closure_value = test_value(MirValueKind::Closure(MirClosureValue {
        function: MirFunctionId(0),
        environment: temp_operand(8),
        environment_id: MirClosureEnvironmentId(0),
    }));
    assert!(value_reads_temp(&closure_value, MirTempId(8)));

    let none = test_value(MirValueKind::Option(MirOptionOp::None));
    assert!(!value_reads_temp(&none, MirTempId(0)));
}

#[test]
fn terminator_reads_temp_covers_return_error_and_try_call_operands() {
    let return_error = MirTerminatorKind::ReturnError(temp_operand(0));
    assert!(terminator_reads_temp(&return_error, MirTempId(0)));

    let try_call_arg = MirTerminatorKind::TryCall {
        destination: None,
        callee: MirCallee::Function(MirFunctionId(0)),
        args: vec![temp_operand(1)],
        ok_block: MirBlockId(0),
        error_place: MirPlace::local(MirLocalId(9)),
        error_block: MirBlockId(1),
    };
    assert!(terminator_reads_temp(&try_call_arg, MirTempId(1)));

    let try_call_value_callee = MirTerminatorKind::TryCall {
        destination: None,
        callee: MirCallee::Value(temp_operand(2)),
        args: Vec::new(),
        ok_block: MirBlockId(0),
        error_place: MirPlace::local(MirLocalId(9)),
        error_block: MirBlockId(1),
    };
    assert!(terminator_reads_temp(&try_call_value_callee, MirTempId(2)));

    let closure_target = closure_callee(temp_operand(3));
    let try_call_closure_callee = MirTerminatorKind::TryCall {
        destination: None,
        callee: MirCallee::Closure(closure_target),
        args: Vec::new(),
        ok_block: MirBlockId(0),
        error_place: MirPlace::local(MirLocalId(9)),
        error_block: MirBlockId(1),
    };
    assert!(terminator_reads_temp(
        &try_call_closure_callee,
        MirTempId(3)
    ));

    let none = MirTerminatorKind::Return(None);
    assert!(!terminator_reads_temp(&none, MirTempId(0)));

    let goto = MirTerminatorKind::Goto(MirBlockId(0));
    assert!(!terminator_reads_temp(&goto, MirTempId(0)));

    let unreachable = MirTerminatorKind::Unreachable;
    assert!(!terminator_reads_temp(&unreachable, MirTempId(0)));
}

/// The callers (`dead_fold_artifact_slots`, `dead_non_kernel_scalar_const_slots`,
/// `multi_output_result_aggregation_slots`) gate slot removal on
/// [`temp_directly_read`]; a temp whose only reader is a value shape the
/// walker missed is removed even though a later statement still reads it.
#[test]
fn temp_directly_read_traces_extent_read_of_temp() {
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![MirLocal {
            id: MirLocalId(1),
            name: None,
            ty: MirType::semantic(TypeId(0)),
            mutable: false,
            span: Span::default(),
        }],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: MirType::semantic(TypeId(0)),
            span: Span::default(),
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(1)),
                    value: test_value(MirValueKind::TensorExtent {
                        tensor: MirOperand::Temp(MirTempId(0)),
                        axis: 0,
                    }),
                },
                span: Span::default(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    assert!(
        temp_directly_read(&function, MirTempId(0)),
        "a temp read only by a TensorExtent must count as directly read"
    );
}

// ---------------------------------------------------------------------------
// Index-operand reads: a `MirProjection::Index` carries a live operand in
// operand and destination positions alike.
// ---------------------------------------------------------------------------

/// Single-block function shape for the index-operand tests: local 1 is the
/// projected base and temp 0 is the read under test.
fn index_operand_function(statements: Vec<MirStatement>, terminator: MirTerminator) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![MirLocal {
            id: MirLocalId(1),
            name: None,
            ty: MirType::semantic(TypeId(0)),
            mutable: false,
            span: Span::default(),
        }],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: MirType::semantic(TypeId(0)),
            span: Span::default(),
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator,
            span: Span::default(),
        }],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

/// The `Local(1)[Index(Temp(0))]` place every index-operand test projects.
fn index_operand_place() -> MirPlace {
    MirPlace {
        base: MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![MirProjection::Index(MirOperand::Temp(MirTempId(0)))],
    }
}

fn return_none_terminator() -> MirTerminator {
    MirTerminator {
        kind: MirTerminatorKind::Return(None),
        span: Span::default(),
    }
}

#[test]
fn index_operand_temp_counts_in_returned_place() {
    let terminator = MirTerminator {
        kind: MirTerminatorKind::Return(Some(MirOperand::Place(index_operand_place()))),
        span: Span::default(),
    };
    let function = index_operand_function(Vec::new(), terminator);
    assert!(temp_directly_read(&function, MirTempId(0)));
}

#[test]
fn index_operand_temp_counts_in_assign_destination() {
    let statement = MirStatement {
        kind: MirStatementKind::Assign {
            place: index_operand_place(),
            value: test_value(MirValueKind::Operand(MirOperand::Constant(
                MirConstant::Int(0),
            ))),
        },
        span: Span::default(),
    };
    let function = index_operand_function(vec![statement], return_none_terminator());
    assert!(temp_directly_read(&function, MirTempId(0)));
}

#[test]
fn index_operand_temp_counts_in_call_destination() {
    let mut call = test_call(MirCallee::Function(MirFunctionId(0)), Vec::new());
    let MirStatementKind::Call { destination, .. } = &mut call.kind else {
        panic!("test_call builds a Call statement");
    };
    *destination = Some(index_operand_place());
    let function = index_operand_function(vec![call], return_none_terminator());
    assert!(temp_directly_read(&function, MirTempId(0)));
}

#[test]
fn index_operand_temp_bare_destination_stays_a_write() {
    let statement = MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::temp(MirTempId(0)),
            value: test_value(MirValueKind::Operand(MirOperand::Constant(
                MirConstant::Int(0),
            ))),
        },
        span: Span::default(),
    };
    let function = index_operand_function(vec![statement], return_none_terminator());
    assert!(!temp_directly_read(&function, MirTempId(0)));
}

// ---------------------------------------------------------------------------
// Local index-operand reads: the local walkers mirror the temp walker, so a
// `Local(2)` named by an `Index` projection is a read in operand and
// destination positions alike.
// ---------------------------------------------------------------------------

/// The `Local(1)[Index(Local(2))]` place every local index-operand test
/// projects: local 1 is the written/addressed base and local 2 is the read
/// under test.
fn index_operand_local_place() -> MirPlace {
    MirPlace {
        base: MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![MirProjection::Index(MirOperand::Place(MirPlace::local(
            MirLocalId(2),
        )))],
    }
}

fn int_zero_value() -> MirValue {
    test_value(MirValueKind::Operand(MirOperand::Constant(
        MirConstant::Int(0),
    )))
}

#[test]
fn index_operand_local_counts_in_returned_place() {
    let terminator =
        MirTerminatorKind::Return(Some(MirOperand::Place(index_operand_local_place())));
    let read = terminator_read_locals(&terminator);
    assert!(read.contains(&MirLocalId(2)));
    assert!(read.contains(&MirLocalId(1)));

    let function = index_operand_function(
        Vec::new(),
        MirTerminator {
            kind: terminator,
            span: Span::default(),
        },
    );
    let function_read = function_read_locals(&function);
    assert!(function_read.contains(&MirLocalId(2)));
    assert!(function_read.contains(&MirLocalId(1)));
}

#[test]
fn index_operand_local_counts_in_assign_destination() {
    let statement = MirStatement {
        kind: MirStatementKind::Assign {
            place: index_operand_local_place(),
            value: int_zero_value(),
        },
        span: Span::default(),
    };
    // Local 2 is read through the destination's index; local 1 is only the
    // written base.
    assert_eq!(statement_read_locals(&statement), vec![MirLocalId(2)]);
    assert!(!statement_reads_local(&statement, MirLocalId(1)));
}

#[test]
fn index_operand_local_counts_in_call_destination() {
    let mut call = test_call(MirCallee::Function(MirFunctionId(0)), Vec::new());
    let MirStatementKind::Call { destination, .. } = &mut call.kind else {
        panic!("test_call builds a Call statement");
    };
    *destination = Some(index_operand_local_place());
    assert_eq!(statement_read_locals(&call), vec![MirLocalId(2)]);
}

#[test]
fn index_operand_local_counts_in_try_call_error_place() {
    let terminator = MirTerminatorKind::TryCall {
        destination: None,
        callee: MirCallee::Function(MirFunctionId(0)),
        args: Vec::new(),
        ok_block: MirBlockId(0),
        error_place: index_operand_local_place(),
        error_block: MirBlockId(1),
    };
    assert_eq!(terminator_read_locals(&terminator), vec![MirLocalId(2)]);
}

#[test]
fn index_operand_local_bare_destination_stays_a_write() {
    let statement = MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(1)),
            value: int_zero_value(),
        },
        span: Span::default(),
    };
    assert!(statement_read_locals(&statement).is_empty());
}
