//! Goto-control-shape MirProgram builders shared by the device-safety tests.
//!
//! Each `*_goto_program` fn builds a `MirProgram` exercising one goto control
//! shape (scalar vs indexed x loop-entry/return/switch-arm/branch-arm/value-join
//! permutations). Extracted from device_test.rs so the builder scaffolding sits
//! apart from the `#[test]` corpus.

use super::*;

/// Type set shared by the indexed goto builders: rank-1 f32 tensor params, f32
/// element type, u32 index type, and vacuum return type.
fn indexed_goto_types(types: &mut TypeTable) -> (MirType, MirType, MirType, MirType) {
    let f32_type_id = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let len = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_type_id, len)));
    let f32_ty = MirType::semantic(f32_type_id);
    let u32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    (tensor_ty, f32_ty, u32_ty, vacuum_ty)
}

/// A linear goto chain: block 0 -> `intermediate_count` pass-through blocks
/// (the first carries `first_statements`) -> `target_block`.
fn goto_chain(intermediate_count: usize, first_statements: &[MirStatement], target_block: MirBlock) -> Vec<MirBlock> {
    let mut blocks = vec![MirBlock {
        id: MirBlockId(0),
        statements: Vec::new(),
        terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
        span: test_span(),
    }];
    for index in 1..=intermediate_count {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirBlockId is u32 by design; test goto-chain block count is bounded by u32::MAX"
        )]
        blocks.push(MirBlock {
            id: MirBlockId(index as u32),
            statements: if index == 1 {
                first_statements.to_vec()
            } else {
                Vec::new()
            },
            terminator: MirTerminator {
                kind: MirTerminatorKind::Goto(MirBlockId(index as u32 + 1)),
                span: test_span(),
            },
            span: test_span(),
        });
    }
    blocks.push(target_block);
    blocks
}

/// MirValue wrapping an operand with the shared test span.
fn operand_value(id: MirValueId, operand: MirOperand, ty: MirType) -> MirValue {
    MirValue { id, kind: MirValueKind::Operand(operand), ty, span: test_span() }
}

/// Assign statement with the shared test span.
fn assign_stmt(place: MirPlace, value: MirValue) -> MirStatement {
    MirStatement { kind: MirStatementKind::Assign { place, value }, span: test_span() }
}

/// Owned parameter with the shared test span.
fn mir_param(local: MirLocalId, ty: MirType) -> MirParam {
    MirParam { local, name: None, ty, mode: MirParamMode::Owned, span: test_span() }
}

/// Mutable local with the shared test span.
fn mir_local(local: MirLocalId, ty: MirType) -> MirLocal {
    MirLocal { id: local, name: None, ty, mutable: true, span: test_span() }
}

/// Single-function MirProgram shell for the goto builders.
fn goto_program(params: Vec<MirParam>, locals: Vec<MirLocal>, blocks: Vec<MirBlock>, return_ty: MirType) -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(1)),
            name: None,
            params,
            locals,
            temps: Vec::new(),
            blocks,
            return_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

pub(super) fn indexed_write_goto_program(
    types: &mut TypeTable,
    intermediate_count: usize,
    extra_intermediate_statement: bool,
    extra_target_statement: bool,
) -> MirProgram {
    let (tensor_ty, f32_ty, u32_ty, vacuum_ty) = indexed_goto_types(types);
    let mut target_statements = vec![assign_stmt(
        indexed_local_place(MirLocalId(1), MirLocalId(2)),
        operand_value(
            MirValueId(0),
            MirOperand::Place(indexed_local_place(MirLocalId(0), MirLocalId(2))),
            f32_ty,
        ),
    )];
    if extra_target_statement {
        target_statements.push(assign_stmt(
            MirPlace::local(MirLocalId(3)),
            operand_value(MirValueId(1), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        ));
    }
    let intermediate_statements = if extra_intermediate_statement {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(3)),
            operand_value(MirValueId(2), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "MirBlockId is u32 by design; test goto-chain block count is bounded by u32::MAX"
    )]
    let target_block = MirBlock {
        id: MirBlockId((intermediate_count + 1) as u32),
        statements: target_statements,
        terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
        span: test_span(),
    };
    let blocks = goto_chain(intermediate_count, &intermediate_statements, target_block);
    goto_program(
        vec![
            mir_param(MirLocalId(0), tensor_ty),
            mir_param(MirLocalId(1), tensor_ty),
            mir_param(MirLocalId(2), u32_ty),
        ],
        vec![mir_local(MirLocalId(3), f32_ty)],
        blocks,
        vacuum_ty,
    )
}

pub(super) fn scalar_loop_entry_goto_program(
    types: &mut TypeTable,
    side_effecting_entry_goto: bool,
    cyclic_entry_goto: bool,
) -> MirProgram {
    let f32_ty = f32_type(types);
    let side_statements = if side_effecting_entry_goto {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(2)),
            operand_value(MirValueId(2), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    goto_program(
        vec![mir_param(MirLocalId(0), f32_ty)],
        vec![mir_local(MirLocalId(1), f32_ty), mir_local(MirLocalId(2), f32_ty)],
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(1)),
                    operand_value(MirValueId(0), MirOperand::Place(MirPlace::local(MirLocalId(0))), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(4)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(false)),
                        then_block: MirBlockId(2),
                        else_block: MirBlockId(3),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(1)),
                    operand_value(MirValueId(1), MirOperand::Constant(MirConstant::Float(0.0)), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(4),
                statements: side_statements,
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Goto(if cyclic_entry_goto {
                        MirBlockId(4)
                    } else {
                        MirBlockId(1)
                    }),
                    span: test_span(),
                },
                span: test_span(),
            },
        ],
        f32_ty,
    )
}

pub(super) fn indexed_loop_entry_goto_program(
    types: &mut TypeTable,
    side_effecting_entry_goto: bool,
    cyclic_entry_goto: bool,
) -> MirProgram {
    let (tensor_ty, f32_ty, u32_ty, vacuum_ty) = indexed_goto_types(types);
    let side_statements = if side_effecting_entry_goto {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(4)),
            operand_value(MirValueId(3), MirOperand::Constant(MirConstant::Int(1)), u32_ty),
        )]
    } else {
        Vec::new()
    };
    goto_program(
        vec![
            mir_param(MirLocalId(0), tensor_ty),
            mir_param(MirLocalId(1), tensor_ty),
            mir_param(MirLocalId(2), u32_ty),
        ],
        vec![mir_local(MirLocalId(3), u32_ty), mir_local(MirLocalId(4), u32_ty)],
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(3)),
                    operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Int(0)), u32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(4)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(false)),
                        then_block: MirBlockId(2),
                        else_block: MirBlockId(3),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: vec![
                    assign_stmt(
                        indexed_local_place(MirLocalId(1), MirLocalId(2)),
                        operand_value(
                            MirValueId(1),
                            MirOperand::Place(indexed_local_place(MirLocalId(0), MirLocalId(2))),
                            f32_ty,
                        ),
                    ),
                    assign_stmt(
                        MirPlace::local(MirLocalId(3)),
                        operand_value(MirValueId(2), MirOperand::Constant(MirConstant::Int(1)), u32_ty),
                    ),
                ],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(4),
                statements: side_statements,
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Goto(if cyclic_entry_goto {
                        MirBlockId(4)
                    } else {
                        MirBlockId(1)
                    }),
                    span: test_span(),
                },
                span: test_span(),
            },
        ],
        vacuum_ty,
    )
}

pub(super) fn scalar_return_goto_program(
    types: &mut TypeTable,
    intermediate_count: usize,
    extra_intermediate_statement: bool,
    extra_target_statement: bool,
) -> MirProgram {
    let f32_ty = f32_type(types);
    let intermediate_statements = if extra_intermediate_statement {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(0)),
            operand_value(MirValueId(1), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    let target_statements = if extra_target_statement {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(0)),
            operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "MirBlockId is u32 by design; test goto-chain block count is bounded by u32::MAX"
    )]
    let return_block = MirBlock {
        id: MirBlockId((intermediate_count + 1) as u32),
        statements: target_statements,
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
            span: test_span(),
        },
        span: test_span(),
    };
    let blocks = goto_chain(intermediate_count, &intermediate_statements, return_block);
    goto_program(vec![mir_param(MirLocalId(0), f32_ty)], Vec::new(), blocks, f32_ty)
}

pub(super) fn scalar_return_goto_cycle_program(types: &mut TypeTable) -> MirProgram {
    let f32_ty = f32_type(types);
    goto_program(
        vec![mir_param(MirLocalId(0), f32_ty)],
        Vec::new(),
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(0)), span: test_span() },
                span: test_span(),
            },
        ],
        f32_ty,
    )
}

pub(super) fn scalar_loop_control_entry_goto_program(
    types: &mut TypeTable,
    control_is_break: bool,
    side_effecting_entry_goto: bool,
    cyclic_entry_goto: bool,
) -> MirProgram {
    let f32_ty = f32_type(types);
    let side_statements = if side_effecting_entry_goto {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(2)),
            operand_value(MirValueId(3), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    goto_program(
        vec![mir_param(MirLocalId(0), f32_ty)],
        vec![mir_local(MirLocalId(1), f32_ty), mir_local(MirLocalId(2), f32_ty)],
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(1)),
                    operand_value(MirValueId(0), MirOperand::Place(MirPlace::local(MirLocalId(0))), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(6)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(false)),
                        then_block: MirBlockId(2),
                        else_block: MirBlockId(3),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(true)),
                        then_block: MirBlockId(4),
                        else_block: MirBlockId(5),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(4),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Goto(if control_is_break { MirBlockId(3) } else { MirBlockId(1) }),
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(5),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(1)),
                    operand_value(MirValueId(1), MirOperand::Constant(MirConstant::Float(0.0)), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(6),
                statements: side_statements,
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Goto(if cyclic_entry_goto {
                        MirBlockId(6)
                    } else {
                        MirBlockId(1)
                    }),
                    span: test_span(),
                },
                span: test_span(),
            },
        ],
        f32_ty,
    )
}

pub(super) fn indexed_loop_control_entry_goto_program(
    types: &mut TypeTable,
    control_is_break: bool,
    side_effecting_entry_goto: bool,
    cyclic_entry_goto: bool,
) -> MirProgram {
    let (tensor_ty, f32_ty, u32_ty, vacuum_ty) = indexed_goto_types(types);
    let side_statements = if side_effecting_entry_goto {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(4)),
            operand_value(MirValueId(4), MirOperand::Constant(MirConstant::Int(1)), u32_ty),
        )]
    } else {
        Vec::new()
    };
    goto_program(
        vec![
            mir_param(MirLocalId(0), tensor_ty),
            mir_param(MirLocalId(1), tensor_ty),
            mir_param(MirLocalId(2), u32_ty),
        ],
        vec![mir_local(MirLocalId(3), u32_ty), mir_local(MirLocalId(4), u32_ty)],
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(3)),
                    operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Int(0)), u32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(6)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(false)),
                        then_block: MirBlockId(2),
                        else_block: MirBlockId(3),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(true)),
                        then_block: MirBlockId(4),
                        else_block: MirBlockId(5),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(4),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Goto(if control_is_break { MirBlockId(3) } else { MirBlockId(1) }),
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(5),
                statements: vec![
                    assign_stmt(
                        indexed_local_place(MirLocalId(1), MirLocalId(2)),
                        operand_value(
                            MirValueId(1),
                            MirOperand::Place(indexed_local_place(MirLocalId(0), MirLocalId(2))),
                            f32_ty,
                        ),
                    ),
                    assign_stmt(
                        MirPlace::local(MirLocalId(3)),
                        operand_value(MirValueId(2), MirOperand::Constant(MirConstant::Int(1)), u32_ty),
                    ),
                ],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(6),
                statements: side_statements,
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Goto(if cyclic_entry_goto {
                        MirBlockId(6)
                    } else {
                        MirBlockId(1)
                    }),
                    span: test_span(),
                },
                span: test_span(),
            },
        ],
        vacuum_ty,
    )
}

pub(super) fn scalar_return_branch_arm_goto_program(types: &mut TypeTable, side_effecting_arm: bool) -> MirProgram {
    let f32_ty = f32_type(types);
    let arm_statements = if side_effecting_arm {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(0)),
            operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    goto_program(
        vec![mir_param(MirLocalId(0), f32_ty)],
        Vec::new(),
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(true)),
                        then_block: MirBlockId(1),
                        else_block: MirBlockId(2),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: arm_statements,
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(3)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            },
        ],
        f32_ty,
    )
}

pub(super) fn scalar_value_join_branch_arm_goto_program(types: &mut TypeTable, side_effecting_arm: bool) -> MirProgram {
    let f32_ty = f32_type(types);
    let arm_statements = if side_effecting_arm {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(1)),
            operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    goto_program(
        vec![mir_param(MirLocalId(0), f32_ty)],
        vec![mir_local(MirLocalId(1), f32_ty)],
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(true)),
                        then_block: MirBlockId(1),
                        else_block: MirBlockId(2),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: arm_statements,
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(3)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(4)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(1)),
                    operand_value(MirValueId(1), MirOperand::Place(MirPlace::local(MirLocalId(0))), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(5)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(4),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(1)),
                    operand_value(MirValueId(2), MirOperand::Constant(MirConstant::Float(0.0)), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(5)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(5),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                    span: test_span(),
                },
                span: test_span(),
            },
        ],
        f32_ty,
    )
}

pub(super) fn scalar_return_switch_arm_goto_program(types: &mut TypeTable, side_effecting_arm: bool) -> MirProgram {
    let f32_ty = f32_type(types);
    let arm_statements = if side_effecting_arm {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(0)),
            operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    goto_program(
        vec![mir_param(MirLocalId(0), f32_ty)],
        Vec::new(),
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Switch {
                        value: MirOperand::Constant(MirConstant::Bool(true)),
                        cases: vec![MirSwitchCase { value: MirConstant::Bool(true), target: MirBlockId(1) }],
                        default: MirBlockId(2),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: arm_statements,
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(3)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            },
        ],
        f32_ty,
    )
}

pub(super) fn indexed_switch_arm_goto_program(types: &mut TypeTable, side_effecting_arm: bool) -> MirProgram {
    let (tensor_ty, f32_ty, u32_ty, vacuum_ty) = indexed_goto_types(types);
    let mut arm_statements = if side_effecting_arm {
        vec![assign_stmt(
            indexed_local_place(MirLocalId(1), MirLocalId(2)),
            operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    let output_write_id = u32::from(side_effecting_arm);
    arm_statements.push(assign_stmt(
        indexed_local_place(MirLocalId(1), MirLocalId(2)),
        operand_value(
            MirValueId(output_write_id),
            MirOperand::Place(indexed_local_place(MirLocalId(0), MirLocalId(2))),
            f32_ty,
        ),
    ));
    goto_program(
        vec![
            mir_param(MirLocalId(0), tensor_ty),
            mir_param(MirLocalId(1), tensor_ty),
            mir_param(MirLocalId(2), u32_ty),
        ],
        Vec::new(),
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Switch {
                        value: MirOperand::Constant(MirConstant::Bool(true)),
                        cases: vec![MirSwitchCase { value: MirConstant::Bool(true), target: MirBlockId(1) }],
                        default: MirBlockId(2),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(3)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: vec![assign_stmt(
                    indexed_local_place(MirLocalId(1), MirLocalId(2)),
                    operand_value(MirValueId(2), MirOperand::Constant(MirConstant::Float(0.0)), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(4)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: arm_statements,
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(4)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(4),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            },
        ],
        vacuum_ty,
    )
}

pub(super) fn indexed_branch_arm_goto_program(types: &mut TypeTable, side_effecting_arm: bool) -> MirProgram {
    let (tensor_ty, f32_ty, u32_ty, vacuum_ty) = indexed_goto_types(types);
    let arm_statements = if side_effecting_arm {
        vec![assign_stmt(
            indexed_local_place(MirLocalId(1), MirLocalId(2)),
            operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    let output_write_id = u32::from(side_effecting_arm);
    goto_program(
        vec![
            mir_param(MirLocalId(0), tensor_ty),
            mir_param(MirLocalId(1), tensor_ty),
            mir_param(MirLocalId(2), u32_ty),
        ],
        Vec::new(),
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(true)),
                        then_block: MirBlockId(1),
                        else_block: MirBlockId(2),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: arm_statements,
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(3)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: vec![assign_stmt(
                    indexed_local_place(MirLocalId(1), MirLocalId(2)),
                    operand_value(MirValueId(2), MirOperand::Constant(MirConstant::Float(0.0)), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(4)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: vec![assign_stmt(
                    indexed_local_place(MirLocalId(1), MirLocalId(2)),
                    operand_value(
                        MirValueId(output_write_id),
                        MirOperand::Place(indexed_local_place(MirLocalId(0), MirLocalId(2))),
                        f32_ty,
                    ),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(4)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(4),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            },
        ],
        vacuum_ty,
    )
}

pub(super) fn indexed_value_join_branch_arm_goto_program(
    types: &mut TypeTable,
    side_effecting_arm: bool,
) -> MirProgram {
    let (tensor_ty, f32_ty, u32_ty, vacuum_ty) = indexed_goto_types(types);
    let arm_statements = if side_effecting_arm {
        vec![assign_stmt(
            MirPlace::local(MirLocalId(3)),
            operand_value(MirValueId(0), MirOperand::Constant(MirConstant::Float(1.0)), f32_ty),
        )]
    } else {
        Vec::new()
    };
    let then_value_id = u32::from(side_effecting_arm);
    let else_value_id = then_value_id + 1;
    let output_value_id = else_value_id + 1;
    goto_program(
        vec![
            mir_param(MirLocalId(0), tensor_ty),
            mir_param(MirLocalId(1), tensor_ty),
            mir_param(MirLocalId(2), u32_ty),
        ],
        vec![mir_local(MirLocalId(3), f32_ty)],
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Branch {
                        condition: MirOperand::Constant(MirConstant::Bool(true)),
                        then_block: MirBlockId(1),
                        else_block: MirBlockId(2),
                    },
                    span: test_span(),
                },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: arm_statements,
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(3)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(2),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(4)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(3),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(3)),
                    operand_value(
                        MirValueId(then_value_id),
                        MirOperand::Place(indexed_local_place(MirLocalId(0), MirLocalId(2))),
                        f32_ty,
                    ),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(5)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(4),
                statements: vec![assign_stmt(
                    MirPlace::local(MirLocalId(3)),
                    operand_value(MirValueId(else_value_id), MirOperand::Constant(MirConstant::Float(0.0)), f32_ty),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(5)), span: test_span() },
                span: test_span(),
            },
            MirBlock {
                id: MirBlockId(5),
                statements: vec![assign_stmt(
                    indexed_local_place(MirLocalId(1), MirLocalId(2)),
                    operand_value(
                        MirValueId(output_value_id),
                        MirOperand::Place(MirPlace::local(MirLocalId(3))),
                        f32_ty,
                    ),
                )],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            },
        ],
        vacuum_ty,
    )
}
