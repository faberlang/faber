//! `TensorFill` postures on the runner: the statement form (dropped result)
//! is the in-place mutation, while the bound form returns a fresh tensor and
//! leaves its source unchanged. The posture is read off the destination
//! temp's liveness, so the scan itself is pinned here too.

use super::*;

use radix_mir::{
    MirCollectionOp, MirIntrinsic, MirOptionOp, MirRuntimeCall, MirStatement, MirStatementKind,
    MirTerminator, MirType,
};
use radix_types::TypeId;

use crate::value::TensorValue;

fn function() -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![radix_mir::MirTemp {
            id: MirTempId(0),
            ty: MirType::semantic(TypeId(0)),
            span: Span::default(),
        }],
        blocks: vec![],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn floats(values: &[f64]) -> Vec<Value> {
    values.iter().copied().map(Value::Float).collect()
}

fn contiguous(values: &[f64], shape: &[i64]) -> Rc<RefCell<TensorValue>> {
    Rc::new(RefCell::new(TensorValue::contiguous(
        floats(values),
        shape.to_vec(),
    )))
}

fn logical(tensor: &Rc<RefCell<TensorValue>>) -> Vec<f64> {
    tensor
        .borrow()
        .logical_values()
        .expect("valid tensor layout")
        .into_iter()
        .map(|value| match value {
            Value::Float(value) => value,
            other => panic!("expected a float element, got {other:?}"),
        })
        .collect()
}

fn fill_call() -> MirRuntimeCall {
    MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFill),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    }
}

/// Run the fill statement with `receiver` in frame slot 0, the scalar fill
/// operand in slot 1, and `destination` as the call destination.
fn statement_fill_with_destination(
    receiver: &Rc<RefCell<TensorValue>>,
    fill: f64,
    destination: Option<MirPlace>,
) -> Result<StatementOutcome, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let types = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    frame
        .values
        .insert(MirValueId(0), Value::Tensor(Rc::clone(receiver)));
    frame.values.insert(MirValueId(1), Value::Float(fill));
    runner.execute_statement(&MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination,
            call: fill_call(),
        },
        span: Span::default(),
    })
}

/// Run the bound fill with `receiver` in frame slot 0 and the scalar fill
/// operand in slot 1.
fn bound_fill(receiver: &Rc<RefCell<TensorValue>>, fill: f64) -> Result<Value, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let types = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    frame
        .values
        .insert(MirValueId(0), Value::Tensor(Rc::clone(receiver)));
    frame.values.insert(MirValueId(1), Value::Float(fill));
    runner.eval_runtime_call(&fill_call(), Span::default())
}

#[test]
fn statement_form_fill_mutates_receiver() {
    let receiver = contiguous(&[0.0; 4], &[2, 2]);
    let outcome =
        statement_fill_with_destination(&receiver, 7.0, None).expect("fill statement runs");
    assert!(matches!(outcome, StatementOutcome::Continue));
    assert_eq!(logical(&receiver), [7.0; 4]);
}

/// The lowering shape of `t.fill(v);`: the call still writes a temp, but the
/// block never reads it — the discarded fill is the mutation statement.
#[test]
fn dead_temp_destination_fill_mutates_receiver() {
    let receiver = contiguous(&[0.0; 4], &[2, 2]);
    let outcome =
        statement_fill_with_destination(&receiver, 7.0, Some(MirPlace::temp(MirTempId(0))))
            .expect("fill statement runs");
    assert!(matches!(outcome, StatementOutcome::Continue));
    assert_eq!(logical(&receiver), [7.0; 4]);
}

#[test]
fn bound_form_fill_returns_fresh_and_leaves_source_unchanged() {
    let receiver = contiguous(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let result = bound_fill(&receiver, 0.0).expect("bound fill runs");
    let Value::Tensor(filled) = result else {
        panic!("expected a tensor result, got {result:?}");
    };
    assert!(!Rc::ptr_eq(&filled, &receiver), "bound fill is fresh");
    assert_eq!(logical(&filled), [0.0; 4]);
    assert_eq!(logical(&receiver), [1.0, 2.0, 3.0, 4.0]);
}

// -- Posture scan: a fill result is "dropped" only when nothing reads it. --

fn fill_into_temp() -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::temp(MirTempId(0))),
            call: fill_call(),
        },
        span: Span::default(),
    }
}

fn assign_value(place: MirPlace, kind: MirValueKind) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place,
            value: MirValue {
                id: MirValueId(7),
                kind,
                ty: MirType::semantic(TypeId(0)),
                span: Span::default(),
            },
        },
        span: Span::default(),
    }
}

fn block_of(statements: Vec<MirStatement>, terminator: MirTerminatorKind) -> MirBlock {
    MirBlock {
        id: MirBlockId(0),
        statements,
        terminator: MirTerminator {
            kind: terminator,
            span: Span::default(),
        },
        span: Span::default(),
    }
}

fn dropped_after(later: Vec<MirStatement>, terminator: MirTerminatorKind) -> bool {
    let mut statements = vec![fill_into_temp()];
    statements.extend(later);
    let block = block_of(statements, terminator);
    fill_result_is_dropped(&block.statements[0], &block, 0)
}

#[test]
fn unread_temp_destination_is_a_dropped_fill() {
    assert!(dropped_after(vec![], MirTerminatorKind::Return(None)));
}

#[test]
fn forwarded_temp_keeps_the_bound_posture() {
    let forward = assign_value(
        MirPlace::local(MirLocalId(0)),
        MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
    );
    assert!(!dropped_after(
        vec![forward],
        MirTerminatorKind::Return(None)
    ));
}

/// `let T ∪ nihil maybe ← t.fill(v)` lowers the bound result through
/// `Some(temp)`. The scan once skipped option values, mistook that binding for
/// a discarded fill, and mutated the source while binding a unit.
#[test]
fn option_wrapped_temp_keeps_the_bound_posture() {
    let wrapped = assign_value(
        MirPlace::local(MirLocalId(0)),
        MirValueKind::Option(MirOptionOp::Some(MirOperand::Temp(MirTempId(0)))),
    );
    assert!(!dropped_after(
        vec![wrapped],
        MirTerminatorKind::Return(None)
    ));
}

#[test]
fn coalesced_temp_keeps_the_bound_posture() {
    let coalesced = assign_value(
        MirPlace::local(MirLocalId(0)),
        MirValueKind::Option(MirOptionOp::Coalesce {
            value: MirOperand::Constant(MirConstant::Nil),
            fallback: MirOperand::Temp(MirTempId(0)),
        }),
    );
    assert!(!dropped_after(
        vec![coalesced],
        MirTerminatorKind::Return(None)
    ));
}

#[test]
fn returned_temp_keeps_the_bound_posture() {
    assert!(!dropped_after(
        vec![],
        MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(0))))
    ));
}

#[test]
fn temp_read_through_a_place_operand_keeps_the_bound_posture() {
    let read = assign_value(
        MirPlace::local(MirLocalId(0)),
        MirValueKind::Operand(MirOperand::Place(MirPlace::temp(MirTempId(0)))),
    );
    assert!(!dropped_after(vec![read], MirTerminatorKind::Return(None)));
}

#[test]
fn overwriting_the_temp_is_not_a_read() {
    let rewrite = assign_value(
        MirPlace::temp(MirTempId(0)),
        MirValueKind::Operand(MirOperand::Constant(MirConstant::Unit)),
    );
    assert!(dropped_after(
        vec![rewrite],
        MirTerminatorKind::Return(None)
    ));
}

#[test]
fn a_different_temp_is_not_a_read() {
    let other = assign_value(
        MirPlace::local(MirLocalId(0)),
        MirValueKind::Option(MirOptionOp::Some(MirOperand::Temp(MirTempId(1)))),
    );
    assert!(dropped_after(vec![other], MirTerminatorKind::Return(None)));
}
