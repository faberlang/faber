//! Structured control-flow shape recognition (`control`): each recognizer accepts exactly
//! its one CFG shape and rejects every near miss.
//!
//! Every test builds a minimal valid function for one shape, asserts what the recognizer
//! reports, then breaks the shape one property at a time and asserts the rejection.

use crate::control::{
    StructuredLoopControl, StructuredNestedLoopControl, StructuredNestedScalarLoop,
    StructuredScalarLoop, StructuredScalarSwitch, is_structured_indexed_value_join_branch,
    is_structured_indexed_write_branch, is_structured_indexed_write_goto,
    is_structured_indexed_write_loop, is_structured_indexed_write_loop_break,
    is_structured_indexed_write_loop_continue, is_structured_indexed_write_switch,
    is_structured_nested_indexed_write_loop_branch, is_structured_nested_indexed_write_loop_break,
    is_structured_nested_indexed_write_loop_continue, is_structured_nested_scalar_loop_branch,
    is_structured_return_branch, is_structured_scalar_loop, is_structured_scalar_loop_break,
    is_structured_scalar_loop_continue, is_structured_scalar_return_goto,
    is_structured_scalar_switch, is_structured_value_join_branch,
    structured_indexed_value_join_branch, structured_indexed_write_branch,
    structured_indexed_write_goto, structured_indexed_write_loop,
    structured_indexed_write_loop_break, structured_indexed_write_loop_continue,
    structured_indexed_write_switch, structured_nested_indexed_write_loop_branch,
    structured_nested_indexed_write_loop_break, structured_nested_indexed_write_loop_continue,
    structured_nested_scalar_loop_branch, structured_return_branch, structured_scalar_loop,
    structured_scalar_loop_break, structured_scalar_loop_continue, structured_scalar_return_goto,
    structured_scalar_switch, structured_value_join_branch,
};
use crate::{
    MirBlock, MirBlockId, MirCallee, MirConstant, MirFunction, MirFunctionId, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProjection, MirStatement,
    MirStatementKind, MirSwitchCase, MirTempId, MirTerminator, MirTerminatorKind, MirType,
    MirValue, MirValueId, MirValueKind,
};
use radix_lexer::Span;
use radix_types::TypeId;

// ---- builders -------------------------------------------------------------------------

fn bb(id: u32) -> MirBlockId {
    MirBlockId(id)
}

fn lid(id: u32) -> MirLocalId {
    MirLocalId(id)
}

fn konst() -> MirOperand {
    MirOperand::Constant(MirConstant::Int(0))
}

fn value(kind: MirValueKind) -> MirValue {
    MirValue {
        id: MirValueId(0),
        kind,
        ty: MirType::semantic(TypeId(0)),
        span: Span::default(),
    }
}

fn assign(place: MirPlace, kind: MirValueKind) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place,
            value: value(kind),
        },
        span: Span::default(),
    }
}

fn place_assign(place: MirPlace) -> MirStatement {
    assign(place, MirValueKind::Operand(konst()))
}

/// A statement that is not an assignment (a void call).
fn call_stmt() -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call {
            destination: None,
            callee: MirCallee::Function(MirFunctionId(1)),
            args: Vec::new(),
        },
        span: Span::default(),
    }
}

fn temp_place(id: u32) -> MirPlace {
    MirPlace {
        base: MirPlaceBase::Temp(MirTempId(id)),
        projections: Vec::new(),
    }
}

/// `temp[0] = 0`: a temp base with a projection is not a plain temp assignment.
fn projected_temp_assign(id: u32) -> MirStatement {
    place_assign(MirPlace {
        base: MirPlaceBase::Temp(MirTempId(id)),
        projections: vec![MirProjection::Index(konst())],
    })
}

fn temp_assign(id: u32) -> MirStatement {
    place_assign(temp_place(id))
}

fn local_assign(local: u32) -> MirStatement {
    place_assign(MirPlace::local(lid(local)))
}

fn output_place(output: u32, index: u32, proven: bool) -> MirPlace {
    let index = MirOperand::Place(MirPlace::local(lid(index)));
    MirPlace {
        base: MirPlaceBase::Local(lid(output)),
        projections: vec![if proven {
            MirProjection::ProvenIndex(index)
        } else {
            MirProjection::Index(index)
        }],
    }
}

/// `output[index] = 0` for the indexed kernel (output local 1, index local 2).
fn write() -> MirStatement {
    place_assign(output_place(1, 2, false))
}

fn write_proven() -> MirStatement {
    place_assign(output_place(1, 2, true))
}

/// `output[index] = local`.
fn write_from(local: u32) -> MirStatement {
    assign(
        output_place(1, 2, false),
        MirValueKind::Operand(MirOperand::Place(MirPlace::local(lid(local)))),
    )
}

fn block(id: u32, statements: Vec<MirStatement>, kind: MirTerminatorKind) -> MirBlock {
    MirBlock {
        id: bb(id),
        statements,
        terminator: MirTerminator {
            kind,
            span: Span::default(),
        },
        span: Span::default(),
    }
}

fn goto(target: u32) -> MirTerminatorKind {
    MirTerminatorKind::Goto(bb(target))
}

fn branch(then_block: u32, else_block: u32) -> MirTerminatorKind {
    MirTerminatorKind::Branch {
        condition: konst(),
        then_block: bb(then_block),
        else_block: bb(else_block),
    }
}

fn switch(cases: &[(MirConstant, u32)], default: u32) -> MirTerminatorKind {
    MirTerminatorKind::Switch {
        value: konst(),
        cases: cases
            .iter()
            .map(|(value, target)| MirSwitchCase {
                value: value.clone(),
                target: bb(*target),
            })
            .collect(),
        default: bb(default),
    }
}

fn ret_none() -> MirTerminatorKind {
    MirTerminatorKind::Return(None)
}

fn ret_const() -> MirTerminatorKind {
    MirTerminatorKind::Return(Some(konst()))
}

fn ret_local(local: u32) -> MirTerminatorKind {
    MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(lid(local)))))
}

fn function(params: &[u32], locals: &[(u32, bool)], blocks: Vec<MirBlock>) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: params
            .iter()
            .map(|local| MirParam {
                local: lid(*local),
                name: None,
                ty: MirType::semantic(TypeId(0)),
                mode: MirParamMode::Owned,
                span: Span::default(),
            })
            .collect(),
        locals: locals
            .iter()
            .map(|(id, mutable)| MirLocal {
                id: lid(*id),
                name: None,
                ty: MirType::semantic(TypeId(0)),
                mutable: *mutable,
                span: Span::default(),
            })
            .collect(),
        temps: Vec::new(),
        blocks,
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

/// One scalar parameter (local 0) and one mutable loop local (local 1).
fn scalar_fn(blocks: Vec<MirBlock>) -> MirFunction {
    function(&[0], &[(0, false), (1, true)], blocks)
}

/// Indexed kernel `(input 0, output 1, index 2)` and one mutable loop local (local 3).
fn indexed_fn(blocks: Vec<MirBlock>) -> MirFunction {
    function(
        &[0, 1, 2],
        &[(0, false), (1, false), (2, false), (3, true)],
        blocks,
    )
}

fn expect_none<T: std::fmt::Debug>(name: &str, found: Option<T>) {
    assert!(found.is_none(), "{name}: expected rejection, got {found:?}");
}

// ---- loops ----------------------------------------------------------------------------

fn scalar_loop() -> MirFunction {
    scalar_fn(vec![
        block(0, vec![local_assign(1), temp_assign(0)], goto(1)),
        block(1, vec![temp_assign(1)], branch(2, 3)),
        block(2, vec![temp_assign(2), local_assign(1)], goto(1)),
        block(3, vec![], ret_local(1)),
    ])
}

#[test]
fn scalar_loop_reports_its_blocks_and_loop_local() {
    let f = scalar_loop();
    assert_eq!(
        structured_scalar_loop(&f),
        Some(StructuredScalarLoop {
            local: lid(1),
            condition_block: bb(1),
            body_block: bb(2),
            exit_block: bb(3),
        })
    );
    assert!(is_structured_scalar_loop(&f));
    // It is a plain loop, not any of the control or indexed shapes.
    assert!(!is_structured_scalar_loop_break(&f));
    assert!(!is_structured_scalar_loop_continue(&f));
    assert!(!is_structured_indexed_write_loop(&f));
    assert!(!is_structured_nested_scalar_loop_branch(&f));
}

#[test]
fn scalar_loop_looks_through_empty_goto_blocks_to_the_condition() {
    // entry -> (empty) 4 -> (empty) 5 -> condition 1
    let f = scalar_fn(vec![
        block(0, vec![local_assign(1)], goto(4)),
        block(1, vec![temp_assign(1)], branch(2, 3)),
        block(2, vec![local_assign(1)], goto(1)),
        block(3, vec![], ret_local(1)),
        block(4, vec![], goto(5)),
        block(5, vec![], goto(1)),
    ]);
    let found = structured_scalar_loop(&f).expect("goto chain to the condition is transparent");
    assert_eq!(found.condition_block, bb(1));
}

#[test]
fn scalar_loop_tolerates_empty_unreachable_blocks_only() {
    let mut f = scalar_loop();
    f.blocks
        .push(block(4, vec![], MirTerminatorKind::Unreachable));
    assert!(
        is_structured_scalar_loop(&f),
        "empty unreachable block is inert"
    );

    f.blocks[4].statements.push(temp_assign(9));
    assert!(
        !is_structured_scalar_loop(&f),
        "an unreachable block with a statement is not inert"
    );

    let mut f = scalar_loop();
    f.blocks.push(block(4, vec![], ret_none()));
    assert!(
        !is_structured_scalar_loop(&f),
        "a stray returning block is not part of the loop"
    );
}

#[test]
fn scalar_loop_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("loop local is immutable", |f| f.locals[1].mutable = false),
        ("loop local is not declared", |f| f.locals.truncate(1)),
        ("a second parameter", |f| {
            f.params.push(f.params[0].clone());
        }),
        ("no parameter", |f| f.params.clear()),
        ("entry does not goto", |f| {
            f.blocks[0].terminator.kind = ret_none()
        }),
        ("entry assigns the parameter", |f| {
            f.blocks[0].statements = vec![local_assign(0)];
        }),
        ("entry assigns no local", |f| {
            f.blocks[0].statements = vec![temp_assign(0)];
        }),
        ("entry assigns two locals", |f| {
            f.blocks[0].statements = vec![local_assign(1), local_assign(5)];
        }),
        ("entry assigns through a projected temp", |f| {
            f.blocks[0].statements = vec![projected_temp_assign(0)];
        }),
        ("entry assigns a projected local", |f| {
            f.blocks[0].statements = vec![place_assign(output_place(1, 0, false))];
        }),
        ("entry holds a non-assignment", |f| {
            f.blocks[0].statements.push(call_stmt());
        }),
        ("condition assigns a local", |f| {
            f.blocks[1].statements = vec![local_assign(1)];
        }),
        ("condition is not a branch", |f| {
            f.blocks[1].terminator.kind = goto(2)
        }),
        ("body does not update the loop local", |f| {
            f.blocks[2].statements = vec![temp_assign(2)];
        }),
        ("body updates the loop local twice", |f| {
            f.blocks[2].statements = vec![local_assign(1), local_assign(1)];
        }),
        ("body does not return to the condition", |f| {
            f.blocks[2].terminator.kind = goto(3);
        }),
        ("exit has a statement", |f| {
            f.blocks[3].statements = vec![temp_assign(3)]
        }),
        ("exit returns nothing", |f| {
            f.blocks[3].terminator.kind = ret_none()
        }),
        ("exit returns a constant", |f| {
            f.blocks[3].terminator.kind = ret_const()
        }),
        ("exit returns another local", |f| {
            f.blocks[3].terminator.kind = ret_local(0)
        }),
        ("condition targets a missing block", |f| {
            f.blocks[1].terminator.kind = branch(2, 77);
        }),
        ("entry goto chain is cyclic", |f| {
            f.blocks[0].terminator.kind = goto(0);
        }),
        ("entry goto targets a missing block", |f| {
            f.blocks[0].terminator.kind = goto(77);
        }),
    ];
    for (name, break_it) in cases {
        let mut f = scalar_loop();
        break_it(&mut f);
        expect_none(name, structured_scalar_loop(&f));
    }
}

#[test]
fn entry_goto_cycle_through_empty_blocks_is_rejected() {
    // 0 -> 4 -> 5 -> 4: never reaches a condition.
    let f = scalar_fn(vec![
        block(0, vec![local_assign(1)], goto(4)),
        block(1, vec![temp_assign(1)], branch(2, 3)),
        block(2, vec![local_assign(1)], goto(1)),
        block(3, vec![], ret_local(1)),
        block(4, vec![], goto(5)),
        block(5, vec![], goto(4)),
    ]);
    assert!(!is_structured_scalar_loop(&f));
}

fn scalar_control(control_target: u32) -> MirFunction {
    scalar_fn(vec![
        block(0, vec![local_assign(1)], goto(1)),
        block(1, vec![temp_assign(0)], branch(2, 5)),
        block(2, vec![temp_assign(1)], branch(3, 4)),
        block(3, vec![], goto(control_target)),
        block(4, vec![temp_assign(2), local_assign(1)], goto(1)),
        block(5, vec![], ret_local(1)),
    ])
}

#[test]
fn scalar_loop_break_and_continue_differ_by_the_control_target() {
    let brk = scalar_control(5);
    assert_eq!(
        structured_scalar_loop_break(&brk),
        Some(StructuredLoopControl {
            local: lid(1),
            condition_block: bb(1),
            body_block: bb(2),
            exit_block: bb(5),
            control_block: bb(3),
            continuation_block: bb(4),
        })
    );
    assert!(is_structured_scalar_loop_break(&brk));
    assert!(!is_structured_scalar_loop_continue(&brk));
    assert!(
        !is_structured_scalar_loop(&brk),
        "the body branches, so it is not a plain loop"
    );

    let cont = scalar_control(1);
    assert_eq!(
        structured_scalar_loop_continue(&cont).map(|c| (c.control_block, c.exit_block)),
        Some((bb(3), bb(5)))
    );
    assert!(is_structured_scalar_loop_continue(&cont));
    assert!(!is_structured_scalar_loop_break(&cont));
}

#[test]
fn scalar_loop_control_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("loop local is immutable", |f| f.locals[1].mutable = false),
        ("two parameters", |f| {
            f.params.push(f.params[0].clone());
        }),
        ("entry does not assign a loop local", |f| {
            f.blocks[0].statements.clear();
        }),
        ("condition assigns a local", |f| {
            f.blocks[1].statements = vec![local_assign(1)];
        }),
        ("condition is not a branch", |f| {
            f.blocks[1].terminator.kind = goto(2)
        }),
        ("body is not a branch", |f| {
            f.blocks[2].terminator.kind = goto(4)
        }),
        ("body branches with a non-temp statement", |f| {
            f.blocks[2].statements = vec![local_assign(1)];
        }),
        ("control block has a statement", |f| {
            f.blocks[3].statements = vec![temp_assign(3)];
        }),
        ("continuation does not update the loop local", |f| {
            f.blocks[4].statements = vec![temp_assign(2)];
        }),
        ("continuation does not return to the condition", |f| {
            f.blocks[4].terminator.kind = goto(5);
        }),
        ("exit has a statement", |f| {
            f.blocks[5].statements = vec![temp_assign(3)]
        }),
        ("exit returns a different local", |f| {
            f.blocks[5].terminator.kind = ret_local(0)
        }),
        ("body targets a missing control block", |f| {
            f.blocks[2].terminator.kind = branch(77, 4);
        }),
        ("a stray non-empty block", |f| {
            f.blocks.push(block(6, vec![], ret_none()));
        }),
    ];
    for (name, break_it) in cases {
        for control_target in [5, 1] {
            let mut f = scalar_control(control_target);
            break_it(&mut f);
            expect_none(name, structured_scalar_loop_break(&f));
            expect_none(name, structured_scalar_loop_continue(&f));
        }
    }
}

fn indexed_loop() -> MirFunction {
    indexed_fn(vec![
        block(0, vec![local_assign(3)], goto(1)),
        block(1, vec![temp_assign(0)], branch(2, 3)),
        block(2, vec![write(), local_assign(3)], goto(1)),
        block(3, vec![], ret_none()),
    ])
}

#[test]
fn indexed_write_loop_reports_its_blocks_and_accepts_either_statement_order() {
    let f = indexed_loop();
    assert_eq!(
        structured_indexed_write_loop(&f),
        Some(StructuredScalarLoop {
            local: lid(3),
            condition_block: bb(1),
            body_block: bb(2),
            exit_block: bb(3),
        })
    );
    assert!(is_structured_indexed_write_loop(&f));
    assert!(!is_structured_indexed_write_loop_break(&f));
    assert!(!is_structured_indexed_write_loop_continue(&f));

    // The loop update may precede the write (the plain loop body allows both orders).
    let mut f = indexed_loop();
    f.blocks[2].statements = vec![local_assign(3), write()];
    assert!(is_structured_indexed_write_loop(&f));

    // A proven index is an indexed write like any other.
    let mut f = indexed_loop();
    f.blocks[2].statements = vec![write_proven(), local_assign(3)];
    assert!(is_structured_indexed_write_loop(&f));
}

#[test]
fn indexed_write_loop_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("two parameters is not an indexed kernel", |f| {
            f.params.truncate(2);
        }),
        ("loop local is immutable", |f| f.locals[3].mutable = false),
        ("loop local is the index parameter", |f| {
            f.blocks[0].statements = vec![local_assign(2)];
        }),
        ("body never writes the output", |f| {
            f.blocks[2].statements = vec![local_assign(3)];
        }),
        ("body never updates the loop local", |f| {
            f.blocks[2].statements = vec![write()];
        }),
        ("body writes the output twice", |f| {
            f.blocks[2].statements = vec![write(), write(), local_assign(3)];
        }),
        ("body updates the loop local twice", |f| {
            f.blocks[2].statements = vec![write(), local_assign(3), local_assign(3)];
        }),
        ("body writes another local", |f| {
            f.blocks[2].statements = vec![write(), local_assign(3), local_assign(0)];
        }),
        ("body writes a different output index", |f| {
            f.blocks[2].statements = vec![place_assign(output_place(1, 0, false)), local_assign(3)];
        }),
        ("body writes a different output", |f| {
            f.blocks[2].statements = vec![place_assign(output_place(0, 2, false)), local_assign(3)];
        }),
        ("body does not return to the condition", |f| {
            f.blocks[2].terminator.kind = goto(3);
        }),
        ("exit returns a value", |f| {
            f.blocks[3].terminator.kind = ret_const()
        }),
        ("exit has a statement", |f| {
            f.blocks[3].statements = vec![temp_assign(3)]
        }),
        ("a stray returning block", |f| {
            f.blocks.push(block(4, vec![], ret_none()));
        }),
        ("condition is not a branch", |f| {
            f.blocks[1].terminator.kind = ret_none()
        }),
        ("condition assigns a local", |f| {
            f.blocks[1].statements = vec![local_assign(3)];
        }),
    ];
    for (name, break_it) in cases {
        let mut f = indexed_loop();
        break_it(&mut f);
        expect_none(name, structured_indexed_write_loop(&f));
    }
}

fn indexed_control(control_target: u32) -> MirFunction {
    indexed_fn(vec![
        block(0, vec![local_assign(3)], goto(1)),
        block(1, vec![temp_assign(0)], branch(2, 5)),
        block(2, vec![temp_assign(1)], branch(3, 4)),
        block(3, vec![], goto(control_target)),
        block(4, vec![write(), local_assign(3)], goto(1)),
        block(5, vec![], ret_none()),
    ])
}

#[test]
fn indexed_write_loop_break_and_continue_differ_by_the_control_target() {
    let brk = indexed_control(5);
    assert_eq!(
        structured_indexed_write_loop_break(&brk),
        Some(StructuredLoopControl {
            local: lid(3),
            condition_block: bb(1),
            body_block: bb(2),
            exit_block: bb(5),
            control_block: bb(3),
            continuation_block: bb(4),
        })
    );
    assert!(is_structured_indexed_write_loop_break(&brk));
    assert!(!is_structured_indexed_write_loop_continue(&brk));
    assert!(!is_structured_indexed_write_loop(&brk));

    let cont = indexed_control(1);
    assert!(structured_indexed_write_loop_continue(&cont).is_some());
    assert!(is_structured_indexed_write_loop_continue(&cont));
    assert!(!is_structured_indexed_write_loop_break(&cont));
}

#[test]
fn indexed_write_loop_control_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("two parameters", |f| f.params.truncate(2)),
        ("loop local is immutable", |f| f.locals[3].mutable = false),
        ("condition assigns a local", |f| {
            f.blocks[1].statements = vec![local_assign(3)];
        }),
        ("condition is not a branch", |f| {
            f.blocks[1].terminator.kind = goto(2)
        }),
        ("body is not a branch", |f| {
            f.blocks[2].terminator.kind = goto(4)
        }),
        ("body branches with a local assignment", |f| {
            f.blocks[2].statements = vec![local_assign(3)];
        }),
        ("control block has a statement", |f| {
            f.blocks[3].statements = vec![temp_assign(3)];
        }),
        ("continuation never writes the output", |f| {
            f.blocks[4].statements = vec![local_assign(3)];
        }),
        ("continuation never updates the loop local", |f| {
            f.blocks[4].statements = vec![write()];
        }),
        ("continuation does not return to the condition", |f| {
            f.blocks[4].terminator.kind = goto(5);
        }),
        ("exit returns a value", |f| {
            f.blocks[5].terminator.kind = ret_const()
        }),
        ("exit has a statement", |f| {
            f.blocks[5].statements = vec![temp_assign(3)]
        }),
        ("a stray returning block", |f| {
            f.blocks.push(block(6, vec![], ret_none()));
        }),
    ];
    for (name, break_it) in cases {
        for control_target in [5, 1] {
            let mut f = indexed_control(control_target);
            break_it(&mut f);
            expect_none(name, structured_indexed_write_loop_break(&f));
            expect_none(name, structured_indexed_write_loop_continue(&f));
        }
    }
}

fn nested_scalar_loop() -> MirFunction {
    scalar_fn(vec![
        block(0, vec![local_assign(1)], goto(1)),
        block(1, vec![temp_assign(0)], branch(2, 3)),
        block(2, vec![temp_assign(1)], branch(4, 5)),
        block(3, vec![], ret_local(1)),
        block(4, vec![local_assign(1)], goto(5)),
        block(5, vec![local_assign(1)], goto(1)),
    ])
}

#[test]
fn nested_scalar_loop_branch_reports_then_and_continuation() {
    let f = nested_scalar_loop();
    assert_eq!(
        structured_nested_scalar_loop_branch(&f),
        Some(StructuredNestedScalarLoop {
            local: lid(1),
            condition_block: bb(1),
            body_block: bb(2),
            exit_block: bb(3),
            then_block: bb(4),
            continuation_block: bb(5),
        })
    );
    assert!(is_structured_nested_scalar_loop_branch(&f));
    assert!(!is_structured_scalar_loop(&f));
    assert!(!is_structured_nested_indexed_write_loop_branch(&f));
}

#[test]
fn nested_scalar_loop_branch_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("five blocks", |f| {
            f.blocks.pop();
        }),
        ("seven blocks", |f| {
            f.blocks.push(block(6, vec![], ret_none()))
        }),
        ("two parameters", |f| {
            f.params.push(f.params[0].clone());
        }),
        ("loop local is immutable", |f| f.locals[1].mutable = false),
        ("entry is not a goto", |f| {
            f.blocks[0].terminator.kind = ret_none()
        }),
        ("entry does not goto the condition", |f| {
            f.blocks[0].terminator.kind = goto(2);
        }),
        ("condition assigns a local", |f| {
            f.blocks[1].statements = vec![local_assign(1)];
        }),
        ("condition is not a branch", |f| {
            f.blocks[1].terminator.kind = goto(2)
        }),
        ("condition body is the wrong block", |f| {
            f.blocks[1].terminator.kind = branch(4, 3);
        }),
        ("condition exit is the wrong block", |f| {
            f.blocks[1].terminator.kind = branch(2, 4);
        }),
        ("body branches the wrong way round", |f| {
            f.blocks[2].terminator.kind = branch(5, 4);
        }),
        ("body holds a local assignment", |f| {
            f.blocks[2].statements = vec![local_assign(1)];
        }),
        ("then block never updates the loop local", |f| {
            f.blocks[4].statements.clear();
        }),
        ("then block skips the continuation", |f| {
            f.blocks[4].terminator.kind = goto(1);
        }),
        ("continuation never updates the loop local", |f| {
            f.blocks[5].statements.clear();
        }),
        ("continuation does not return to the condition", |f| {
            f.blocks[5].terminator.kind = goto(3);
        }),
        ("exit has a statement", |f| {
            f.blocks[3].statements = vec![temp_assign(3)]
        }),
        ("exit returns nothing", |f| {
            f.blocks[3].terminator.kind = ret_none()
        }),
        ("exit returns a different local", |f| {
            f.blocks[3].terminator.kind = ret_local(0)
        }),
    ];
    for (name, break_it) in cases {
        let mut f = nested_scalar_loop();
        break_it(&mut f);
        expect_none(name, structured_nested_scalar_loop_branch(&f));
    }
}

fn nested_indexed_loop() -> MirFunction {
    indexed_fn(vec![
        block(0, vec![local_assign(3)], goto(1)),
        block(1, vec![temp_assign(0)], branch(2, 3)),
        block(2, vec![temp_assign(1)], branch(4, 5)),
        block(3, vec![], ret_none()),
        block(4, vec![local_assign(3)], goto(5)),
        block(5, vec![write(), local_assign(3)], goto(1)),
    ])
}

#[test]
fn nested_indexed_write_loop_branch_requires_write_before_update() {
    let f = nested_indexed_loop();
    assert_eq!(
        structured_nested_indexed_write_loop_branch(&f),
        Some(StructuredNestedScalarLoop {
            local: lid(3),
            condition_block: bb(1),
            body_block: bb(2),
            exit_block: bb(3),
            then_block: bb(4),
            continuation_block: bb(5),
        })
    );
    assert!(is_structured_nested_indexed_write_loop_branch(&f));
    assert!(!is_structured_nested_scalar_loop_branch(&f));

    // Unlike the plain indexed loop body, the nested continuation is ordered: the write
    // must come first, then the loop update.
    let mut reordered = nested_indexed_loop();
    reordered.blocks[5].statements = vec![local_assign(3), write()];
    assert!(!is_structured_nested_indexed_write_loop_branch(&reordered));

    // A temp assignment may sit anywhere in the continuation.
    let mut with_temps = nested_indexed_loop();
    with_temps.blocks[5].statements = vec![
        temp_assign(5),
        write_proven(),
        temp_assign(6),
        local_assign(3),
    ];
    assert!(is_structured_nested_indexed_write_loop_branch(&with_temps));
}

#[test]
fn nested_indexed_write_loop_branch_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("five blocks", |f| {
            f.blocks.pop();
        }),
        ("two parameters", |f| f.params.truncate(2)),
        ("loop local is immutable", |f| f.locals[3].mutable = false),
        ("entry is not a goto", |f| {
            f.blocks[0].terminator.kind = ret_none()
        }),
        ("entry does not goto the condition", |f| {
            f.blocks[0].terminator.kind = goto(2);
        }),
        ("condition assigns a local", |f| {
            f.blocks[1].statements = vec![local_assign(3)];
        }),
        ("condition is not a branch", |f| {
            f.blocks[1].terminator.kind = goto(2)
        }),
        ("condition body is the wrong block", |f| {
            f.blocks[1].terminator.kind = branch(4, 3);
        }),
        ("condition exit is the wrong block", |f| {
            f.blocks[1].terminator.kind = branch(2, 4);
        }),
        ("body branches the wrong way round", |f| {
            f.blocks[2].terminator.kind = branch(5, 4);
        }),
        ("then block never updates the loop local", |f| {
            f.blocks[4].statements.clear();
        }),
        ("then block skips the continuation", |f| {
            f.blocks[4].terminator.kind = goto(1);
        }),
        ("continuation never writes", |f| {
            f.blocks[5].statements = vec![local_assign(3)];
        }),
        ("continuation never updates", |f| {
            f.blocks[5].statements = vec![write()]
        }),
        ("continuation does not return to the condition", |f| {
            f.blocks[5].terminator.kind = goto(3);
        }),
        ("exit returns a value", |f| {
            f.blocks[3].terminator.kind = ret_const()
        }),
        ("exit has a statement", |f| {
            f.blocks[3].statements = vec![temp_assign(3)]
        }),
    ];
    for (name, break_it) in cases {
        let mut f = nested_indexed_loop();
        break_it(&mut f);
        expect_none(name, structured_nested_indexed_write_loop_branch(&f));
    }
}

fn nested_control(control_target: u32) -> MirFunction {
    indexed_fn(vec![
        block(0, vec![local_assign(3)], goto(1)),
        block(1, vec![temp_assign(0)], branch(2, 3)),
        block(2, vec![temp_assign(1)], branch(4, 5)),
        block(3, vec![], ret_none()),
        block(4, vec![temp_assign(2)], branch(6, 7)),
        block(5, vec![write(), local_assign(3)], goto(1)),
        block(6, vec![], goto(control_target)),
        block(7, vec![local_assign(3)], goto(5)),
    ])
}

#[test]
fn nested_indexed_write_loop_break_and_continue_differ_by_the_control_target() {
    let brk = nested_control(3);
    assert_eq!(
        structured_nested_indexed_write_loop_break(&brk),
        Some(StructuredNestedLoopControl {
            local: lid(3),
            condition_block: bb(1),
            body_block: bb(2),
            exit_block: bb(3),
            nested_branch_block: bb(4),
            control_block: bb(6),
            nested_update_block: bb(7),
            continuation_block: bb(5),
        })
    );
    assert!(is_structured_nested_indexed_write_loop_break(&brk));
    assert!(!is_structured_nested_indexed_write_loop_continue(&brk));
    assert!(!is_structured_nested_indexed_write_loop_branch(&brk));

    let cont = nested_control(1);
    assert_eq!(
        structured_nested_indexed_write_loop_continue(&cont).map(|c| c.control_block),
        Some(bb(6))
    );
    assert!(is_structured_nested_indexed_write_loop_continue(&cont));
    assert!(!is_structured_nested_indexed_write_loop_break(&cont));
}

#[test]
fn nested_indexed_write_loop_control_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("seven blocks", |f| {
            f.blocks.pop();
        }),
        ("two parameters", |f| f.params.truncate(2)),
        ("loop local is immutable", |f| f.locals[3].mutable = false),
        ("entry is not a goto", |f| {
            f.blocks[0].terminator.kind = ret_none()
        }),
        ("entry does not goto the condition", |f| {
            f.blocks[0].terminator.kind = goto(2);
        }),
        ("condition assigns a local", |f| {
            f.blocks[1].statements = vec![local_assign(3)];
        }),
        ("condition is not a branch", |f| {
            f.blocks[1].terminator.kind = goto(2)
        }),
        ("condition body is the wrong block", |f| {
            f.blocks[1].terminator.kind = branch(4, 3);
        }),
        ("body does not branch to the nested branch", |f| {
            f.blocks[2].terminator.kind = branch(5, 4);
        }),
        ("nested branch does not reach the control block", |f| {
            f.blocks[4].terminator.kind = branch(7, 6);
        }),
        ("nested branch has a local assignment", |f| {
            f.blocks[4].statements = vec![local_assign(3)];
        }),
        ("control block has a statement", |f| {
            f.blocks[6].statements = vec![temp_assign(3)];
        }),
        ("nested update never updates the loop local", |f| {
            f.blocks[7].statements.clear();
        }),
        ("nested update skips the continuation", |f| {
            f.blocks[7].terminator.kind = goto(1);
        }),
        ("continuation writes after updating", |f| {
            f.blocks[5].statements = vec![local_assign(3), write()];
        }),
        ("continuation does not return to the condition", |f| {
            f.blocks[5].terminator.kind = goto(3);
        }),
        ("exit returns a value", |f| {
            f.blocks[3].terminator.kind = ret_const()
        }),
        ("exit has a statement", |f| {
            f.blocks[3].statements = vec![temp_assign(3)]
        }),
    ];
    for (name, break_it) in cases {
        for control_target in [3, 1] {
            let mut f = nested_control(control_target);
            break_it(&mut f);
            expect_none(name, structured_nested_indexed_write_loop_break(&f));
            expect_none(name, structured_nested_indexed_write_loop_continue(&f));
        }
    }
}

// ---- branches, gotos and joins --------------------------------------------------------

#[test]
fn return_branch_follows_goto_chains_to_each_returning_arm() {
    let f = scalar_fn(vec![
        block(0, vec![], branch(1, 2)),
        block(1, vec![temp_assign(0)], ret_const()),
        block(2, vec![], goto(3)),
        block(3, vec![], ret_local(1)),
    ]);
    let found = structured_return_branch(&f).expect("both arms return");
    assert_eq!((found.then_block, found.else_block), (bb(1), bb(3)));
    assert!(is_structured_return_branch(&f));
}

#[test]
fn return_branch_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let base = || {
        scalar_fn(vec![
            block(0, vec![], branch(1, 2)),
            block(1, vec![], ret_const()),
            block(2, vec![], ret_const()),
        ])
    };
    let cases: &[(&str, Break)] = &[
        ("entry is not a branch", |f| {
            f.blocks[0].terminator.kind = goto(1)
        }),
        ("arm returns nothing", |f| {
            f.blocks[1].terminator.kind = ret_none()
        }),
        ("arm has a non-temp statement", |f| {
            f.blocks[1].statements = vec![local_assign(1)]
        }),
        ("goto arm has a statement", |f| {
            f.blocks[1].terminator.kind = goto(2);
            f.blocks[1].statements = vec![temp_assign(0)];
        }),
        ("arm goto loops", |f| f.blocks[1].terminator.kind = goto(1)),
        ("arm is a missing block", |f| {
            f.blocks[0].terminator.kind = branch(1, 77)
        }),
        ("arm goto target is missing", |f| {
            f.blocks[1].terminator.kind = goto(77)
        }),
        ("stray block", |f| {
            f.blocks.push(block(3, vec![], ret_const()))
        }),
    ];
    for (name, break_it) in cases {
        let mut f = base();
        break_it(&mut f);
        expect_none(name, structured_return_branch(&f));
    }
}

#[test]
fn scalar_return_goto_needs_a_pure_goto_chain_covering_every_block() {
    let f = scalar_fn(vec![
        block(0, vec![], goto(2)),
        block(1, vec![], ret_const()),
        block(2, vec![], goto(1)),
    ]);
    assert_eq!(structured_scalar_return_goto(&f), Some(bb(1)));
    assert!(is_structured_scalar_return_goto(&f));

    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("entry is not a goto", |f| {
            f.blocks[0].terminator.kind = ret_const()
        }),
        ("intermediate block has a statement", |f| {
            f.blocks[2].statements = vec![temp_assign(0)]
        }),
        ("target has a statement", |f| {
            f.blocks[1].statements = vec![temp_assign(0)]
        }),
        ("target returns nothing", |f| {
            f.blocks[1].terminator.kind = ret_none()
        }),
        ("target is not a return", |f| {
            f.blocks[1].terminator.kind = MirTerminatorKind::Unreachable
        }),
        ("chain is cyclic", |f| f.blocks[1].terminator.kind = goto(0)),
        ("chain skips a block", |f| {
            f.blocks[0].terminator.kind = goto(1)
        }),
        ("goto target is missing", |f| {
            f.blocks[2].terminator.kind = goto(77)
        }),
    ];
    for (name, break_it) in cases {
        let mut f = scalar_fn(vec![
            block(0, vec![], goto(2)),
            block(1, vec![], ret_const()),
            block(2, vec![], goto(1)),
        ]);
        break_it(&mut f);
        expect_none(name, structured_scalar_return_goto(&f));
    }
}

#[test]
fn indexed_write_goto_needs_a_write_block_returning_nothing() {
    let f = indexed_fn(vec![
        block(0, vec![], goto(1)),
        block(1, vec![temp_assign(0), write()], ret_none()),
    ]);
    assert_eq!(structured_indexed_write_goto(&f), Some(bb(1)));
    assert!(is_structured_indexed_write_goto(&f));

    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("two parameters", |f| f.params.truncate(2)),
        ("target returns a value", |f| {
            f.blocks[1].terminator.kind = ret_const()
        }),
        ("target never writes", |f| {
            f.blocks[1].statements = vec![temp_assign(0)]
        }),
        ("target has an extra statement", |f| {
            f.blocks[1].statements.push(local_assign(3))
        }),
        ("target writes twice", |f| {
            f.blocks[1].statements.push(write())
        }),
        ("temp assignment after the write", |f| {
            f.blocks[1].statements.push(temp_assign(1));
        }),
    ];
    for (name, break_it) in cases {
        let mut f = indexed_fn(vec![
            block(0, vec![], goto(1)),
            block(1, vec![temp_assign(0), write()], ret_none()),
        ]);
        break_it(&mut f);
        expect_none(name, structured_indexed_write_goto(&f));
    }
}

fn indexed_branch() -> MirFunction {
    indexed_fn(vec![
        block(0, vec![], branch(1, 4)),
        block(1, vec![write()], goto(3)),
        block(2, vec![], MirTerminatorKind::Unreachable),
        block(3, vec![], ret_none()),
        block(4, vec![], goto(5)),
        block(5, vec![temp_assign(0), write_proven()], goto(3)),
    ])
}

#[test]
fn indexed_write_branch_joins_two_write_arms() {
    let f = indexed_branch();
    let found = structured_indexed_write_branch(&f).expect("two write arms share one join");
    assert_eq!((found.then_block, found.else_block), (bb(1), bb(5)));
    assert!(is_structured_indexed_write_branch(&f));
    // It is a plain write branch, not a value join.
    assert!(!is_structured_indexed_value_join_branch(&f));
}

#[test]
fn indexed_write_branch_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("two parameters", |f| f.params.truncate(2)),
        ("entry is not a branch", |f| {
            f.blocks[0].terminator.kind = goto(1)
        }),
        ("then arm never writes", |f| f.blocks[1].statements.clear()),
        ("then arm writes twice", |f| {
            f.blocks[1].statements = vec![write(), write()]
        }),
        ("arms join in different blocks", |f| {
            f.blocks[1].terminator.kind = goto(2);
            f.blocks[2].terminator.kind = ret_none();
        }),
        ("arm is not a goto", |f| {
            f.blocks[1].terminator.kind = ret_none()
        }),
        ("join returns a value", |f| {
            f.blocks[3].terminator.kind = ret_const()
        }),
        ("arm goto cycles", |f| {
            f.blocks[4].terminator.kind = goto(4);
        }),
        ("arm is a missing block", |f| {
            f.blocks[0].terminator.kind = branch(1, 77)
        }),
        ("stray non-unreachable block", |f| {
            f.blocks[2].terminator.kind = ret_none();
        }),
        ("stray unreachable block with a statement", |f| {
            f.blocks[2].statements = vec![temp_assign(0)];
        }),
        ("join target is missing", |f| {
            f.blocks[1].terminator.kind = goto(77);
            f.blocks[5].terminator.kind = goto(77);
        }),
    ];
    for (name, break_it) in cases {
        let mut f = indexed_branch();
        break_it(&mut f);
        expect_none(name, structured_indexed_write_branch(&f));
    }
}

fn indexed_switch() -> MirFunction {
    indexed_fn(vec![
        block(
            0,
            vec![],
            switch(&[(MirConstant::Int(1), 1), (MirConstant::Bool(true), 4)], 2),
        ),
        block(1, vec![write()], goto(3)),
        block(2, vec![write_proven()], goto(3)),
        block(3, vec![], ret_none()),
        block(4, vec![], goto(5)),
        block(5, vec![write()], goto(3)),
        block(6, vec![], MirTerminatorKind::Unreachable),
    ])
}

#[test]
fn indexed_write_switch_resolves_cases_through_goto_chains() {
    let f = indexed_switch();
    let found = structured_indexed_write_switch(&f).expect("every arm writes and joins");
    assert_eq!(found.default, bb(2));
    assert_eq!(
        found.cases,
        vec![
            MirSwitchCase {
                value: MirConstant::Int(1),
                target: bb(1)
            },
            MirSwitchCase {
                value: MirConstant::Bool(true),
                target: bb(5)
            },
        ]
    );
    assert!(is_structured_indexed_write_switch(&f));
    assert!(!is_structured_indexed_write_branch(&f));
}

#[test]
fn indexed_write_switch_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("two parameters", |f| f.params.truncate(2)),
        ("entry is not a switch", |f| {
            f.blocks[0].terminator.kind = branch(1, 2)
        }),
        ("no cases", |f| f.blocks[0].terminator.kind = switch(&[], 2)),
        ("float case", |f| {
            f.blocks[0].terminator.kind = switch(&[(MirConstant::Float(1.0), 1)], 2);
        }),
        ("magnus case (host-only inf)", |f| {
            f.blocks[0].terminator.kind = switch(
                &[(MirConstant::Magnus("18446744073709551616".into()), 1)],
                2,
            );
        }),
        ("a case arm never writes", |f| {
            f.blocks[1].statements.clear()
        }),
        ("the default arm never writes", |f| {
            f.blocks[2].statements.clear()
        }),
        ("the default arm is missing", |f| {
            f.blocks[0].terminator.kind = switch(&[(MirConstant::Int(1), 1)], 77);
        }),
        ("arms join in different blocks", |f| {
            f.blocks[2].terminator.kind = goto(6);
        }),
        ("join returns a value", |f| {
            f.blocks[3].terminator.kind = ret_const()
        }),
        ("stray non-unreachable block", |f| {
            f.blocks[6].terminator.kind = ret_none();
        }),
    ];
    for (name, break_it) in cases {
        let mut f = indexed_switch();
        break_it(&mut f);
        expect_none(name, structured_indexed_write_switch(&f));
    }
}

fn indexed_value_join() -> MirFunction {
    indexed_fn(vec![
        block(0, vec![], branch(1, 3)),
        block(1, vec![temp_assign(0), local_assign(3)], goto(2)),
        block(2, vec![write_from(3)], ret_none()),
        block(3, vec![], goto(4)),
        block(4, vec![local_assign(3)], goto(2)),
    ])
}

#[test]
fn indexed_value_join_branch_reports_the_join_local() {
    let f = indexed_value_join();
    let found = structured_indexed_value_join_branch(&f).expect("both arms assign the join local");
    assert_eq!(
        (
            found.then_block,
            found.else_block,
            found.join_block,
            found.join_local
        ),
        (bb(1), bb(4), bb(2), lid(3))
    );
    assert!(is_structured_indexed_value_join_branch(&f));
    assert!(!is_structured_indexed_write_branch(&f));
}

#[test]
fn indexed_value_join_branch_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("two parameters", |f| f.params.truncate(2)),
        ("entry is not a branch", |f| {
            f.blocks[0].terminator.kind = goto(1)
        }),
        ("then arm assigns nothing", |f| {
            f.blocks[1].statements.clear()
        }),
        ("then arm assigns another local", |f| {
            f.blocks[1].statements = vec![local_assign(0)]
        }),
        ("then arm assigns the join local twice", |f| {
            f.blocks[1].statements = vec![local_assign(3), local_assign(3)];
        }),
        ("then arm assigns a projected place", |f| {
            f.blocks[1].statements = vec![local_assign(3), place_assign(output_place(1, 2, false))];
        }),
        ("then arm has a non-assignment", |f| {
            f.blocks[1].statements.push(call_stmt());
        }),
        ("then arm does not goto the join", |f| {
            f.blocks[1].terminator.kind = goto(3)
        }),
        ("join holds a call, not an assignment", |f| {
            f.blocks[2].statements = vec![call_stmt()]
        }),
        ("join writes a constant", |f| {
            f.blocks[2].statements = vec![write()]
        }),
        ("join writes from a projected place", |f| {
            f.blocks[2].statements = vec![assign(
                output_place(1, 2, false),
                MirValueKind::Operand(MirOperand::Place(output_place(0, 2, false))),
            )];
        }),
        ("join writes elsewhere", |f| {
            f.blocks[2].statements = vec![assign(
                MirPlace::local(lid(3)),
                MirValueKind::Operand(MirOperand::Place(MirPlace::local(lid(3)))),
            )];
        }),
        ("join has two statements", |f| {
            f.blocks[2].statements.push(temp_assign(0))
        }),
        ("join returns a value", |f| {
            f.blocks[2].terminator.kind = ret_const()
        }),
        ("arms join in different blocks", |f| {
            f.blocks[4].terminator.kind = goto(1)
        }),
        ("arm goto cycles", |f| f.blocks[3].terminator.kind = goto(3)),
        ("stray block", |f| {
            f.blocks.push(block(5, vec![], ret_none()))
        }),
    ];
    for (name, break_it) in cases {
        let mut f = indexed_value_join();
        break_it(&mut f);
        expect_none(name, structured_indexed_value_join_branch(&f));
    }
}

fn value_join() -> MirFunction {
    scalar_fn(vec![
        block(0, vec![], branch(1, 3)),
        block(1, vec![local_assign(1)], goto(2)),
        block(2, vec![], ret_local(1)),
        block(3, vec![], goto(4)),
        block(4, vec![temp_assign(0), local_assign(1)], goto(2)),
    ])
}

#[test]
fn value_join_branch_reports_the_returned_join_local() {
    let f = value_join();
    let found = structured_value_join_branch(&f).expect("both arms assign the returned local");
    assert_eq!(
        (
            found.then_block,
            found.else_block,
            found.join_block,
            found.join_local
        ),
        (bb(1), bb(4), bb(2), lid(1))
    );
    assert!(is_structured_value_join_branch(&f));
    assert!(!is_structured_return_branch(&f));
}

#[test]
fn value_join_branch_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("two parameters", |f| f.params.push(f.params[0].clone())),
        ("no parameter", |f| f.params.clear()),
        ("entry is not a branch", |f| {
            f.blocks[0].terminator.kind = goto(1)
        }),
        ("join returns nothing", |f| {
            f.blocks[2].terminator.kind = ret_none()
        }),
        ("join returns a constant", |f| {
            f.blocks[2].terminator.kind = ret_const()
        }),
        ("join returns a projected place", |f| {
            f.blocks[2].terminator.kind =
                MirTerminatorKind::Return(Some(MirOperand::Place(output_place(1, 0, false))));
        }),
        ("then arm assigns a different local", |f| {
            f.blocks[1].statements = vec![local_assign(0)];
        }),
        ("else arm never assigns the join local", |f| {
            f.blocks[4].statements = vec![temp_assign(0)];
        }),
        ("arms join in different blocks", |f| {
            f.blocks[4].terminator.kind = goto(1)
        }),
        ("stray block", |f| {
            f.blocks.push(block(5, vec![], ret_none()))
        }),
    ];
    for (name, break_it) in cases {
        let mut f = value_join();
        break_it(&mut f);
        expect_none(name, structured_value_join_branch(&f));
    }
}

fn scalar_switch() -> MirFunction {
    scalar_fn(vec![
        block(
            0,
            vec![],
            switch(&[(MirConstant::Int(1), 1), (MirConstant::UInt(2), 2)], 3),
        ),
        block(1, vec![], ret_const()),
        block(2, vec![], goto(4)),
        block(3, vec![], ret_const()),
        block(4, vec![], ret_const()),
        block(5, vec![], MirTerminatorKind::Unreachable),
    ])
}

#[test]
fn scalar_switch_resolves_each_case_to_its_returning_block() {
    let f = scalar_switch();
    let found = structured_scalar_switch(&f).expect("every arm returns a value");
    assert_eq!(
        found,
        StructuredScalarSwitch {
            cases: vec![
                MirSwitchCase {
                    value: MirConstant::Int(1),
                    target: bb(1)
                },
                MirSwitchCase {
                    value: MirConstant::UInt(2),
                    target: bb(4)
                },
            ],
            default: bb(3),
        }
    );
    assert!(is_structured_scalar_switch(&f));
}

#[test]
fn scalar_switch_rejects_each_broken_property() {
    type Break = fn(&mut MirFunction);
    let cases: &[(&str, Break)] = &[
        ("two parameters", |f| f.params.push(f.params[0].clone())),
        ("entry is not a switch", |f| {
            f.blocks[0].terminator.kind = branch(1, 3)
        }),
        ("no cases", |f| f.blocks[0].terminator.kind = switch(&[], 3)),
        ("float case", |f| {
            f.blocks[0].terminator.kind = switch(&[(MirConstant::Float(0.5), 1)], 3);
        }),
        ("magnus case (host-only inf)", |f| {
            f.blocks[0].terminator.kind = switch(
                &[(MirConstant::Magnus("18446744073709551616".into()), 1)],
                3,
            );
        }),
        ("a goto arm has a statement", |f| {
            f.blocks[2].statements = vec![temp_assign(0)]
        }),
        ("a case arm returns nothing", |f| {
            f.blocks[1].terminator.kind = ret_none()
        }),
        ("the default arm returns nothing", |f| {
            f.blocks[3].terminator.kind = ret_none()
        }),
        ("an arm has a non-temp statement", |f| {
            f.blocks[1].statements = vec![local_assign(1)]
        }),
        ("the default arm is missing", |f| {
            f.blocks[0].terminator.kind = switch(&[(MirConstant::Int(1), 1)], 77);
        }),
        ("stray non-unreachable block", |f| {
            f.blocks[5].terminator.kind = ret_const()
        }),
    ];
    for (name, break_it) in cases {
        let mut f = scalar_switch();
        break_it(&mut f);
        expect_none(name, structured_scalar_switch(&f));
    }
}

#[test]
fn recognizers_return_none_for_a_function_without_blocks() {
    let f = function(&[0, 1, 2], &[], Vec::new());
    expect_none("return branch", structured_return_branch(&f));
    expect_none("scalar loop", structured_scalar_loop(&f));
    expect_none("indexed loop", structured_indexed_write_loop(&f));
    expect_none("scalar switch", structured_scalar_switch(&f));
    expect_none("value join", structured_value_join_branch(&f));
    expect_none("goto", structured_scalar_return_goto(&f));
    expect_none("indexed switch", structured_indexed_write_switch(&f));
    expect_none("indexed branch", structured_indexed_write_branch(&f));
    expect_none(
        "indexed value join",
        structured_indexed_value_join_branch(&f),
    );
}
