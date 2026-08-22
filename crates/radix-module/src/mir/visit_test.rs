// Tests for MIR structural visitors and CFG edge extraction.
//
// These tests prove that MirVisitor and FallibleMirVisitor walk MIR
// storage order without following control-flow edges, and that
// terminator_successors extracts correct block targets from every
// terminator kind.

use super::*;
use crate::semantic::TypeId;
use crate::test_support::test_span;
use radix_mir::visit::*;

fn ty() -> MirType {
    MirType::semantic(TypeId(0))
}

fn sample_program() -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(7),
            source: None,
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: ty(),
                mode: MirParamMode::Owned,
                span: test_span(),
            }],
            locals: vec![MirLocal { id: MirLocalId(1), name: None, ty: ty(), mutable: false, span: test_span() }],
            temps: vec![MirTemp { id: MirTempId(0), ty: ty(), span: test_span() }],
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(1)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(MirLocalId(0)))),
                                ty: ty(),
                                span: test_span(),
                            },
                        },
                        span: test_span(),
                    }],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Branch {
                            condition: MirOperand::Value(MirValueId(0)),
                            then_block: MirBlockId(1),
                            else_block: MirBlockId(2),
                        },
                        span: test_span(),
                    },
                    span: test_span(),
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(2)), span: test_span() },
                    span: test_span(),
                },
                MirBlock {
                    id: MirBlockId(2),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                },
            ],
            return_ty: ty(),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

#[derive(Default)]
struct CountingVisitor {
    functions: usize,
    params: usize,
    locals: usize,
    temps: usize,
    blocks: usize,
    statements: usize,
    terminators: usize,
    values: usize,
    operands: usize,
    places: usize,
}

impl MirVisitor for CountingVisitor {
    fn visit_function(&mut self, function: &MirFunction) {
        self.functions += 1;
        walk_function(self, function);
    }

    fn visit_param(&mut self, _param: &MirParam) {
        self.params += 1;
    }

    fn visit_local(&mut self, _local: &MirLocal) {
        self.locals += 1;
    }

    fn visit_temp(&mut self, _temp: &MirTemp) {
        self.temps += 1;
    }

    fn visit_block(&mut self, block: &MirBlock) {
        self.blocks += 1;
        walk_block(self, block);
    }

    fn visit_stmt(&mut self, stmt: &MirStatement) {
        self.statements += 1;
        walk_stmt(self, stmt);
    }

    fn visit_terminator(&mut self, terminator: &MirTerminator) {
        self.terminators += 1;
        walk_terminator(self, terminator);
    }

    fn visit_value(&mut self, value: &MirValue) {
        self.values += 1;
        walk_value(self, value);
    }

    fn visit_operand(&mut self, operand: &MirOperand) {
        self.operands += 1;
        walk_operand(self, operand);
    }

    fn visit_place(&mut self, place: &MirPlace) {
        self.places += 1;
        walk_place(self, place);
    }
}

#[test]
fn structural_visitor_walks_storage_order_without_following_cfg_edges() {
    let mut visitor = CountingVisitor::default();
    visitor.visit_program(&sample_program());

    assert_eq!(visitor.functions, 1);
    assert_eq!(visitor.params, 1);
    assert_eq!(visitor.locals, 1);
    assert_eq!(visitor.temps, 1);
    assert_eq!(visitor.blocks, 3);
    assert_eq!(visitor.statements, 1);
    assert_eq!(visitor.terminators, 3);
    assert_eq!(visitor.values, 1);
    assert_eq!(visitor.operands, 2);
    assert_eq!(visitor.places, 2);
}

#[test]
fn closure_callee_walks_environment_operand() {
    let callee = MirCallee::Closure(MirClosureCallee {
        function: MirFunctionId(1),
        environment: MirOperand::Place(MirPlace::local(MirLocalId(7))),
        environment_id: MirClosureEnvironmentId(2),
    });
    let mut visitor = CountingVisitor::default();

    visitor.visit_callee(&callee);

    assert_eq!(visitor.operands, 1);
    assert_eq!(visitor.places, 1);
}

#[test]
fn terminator_successors_goto_returns_target() {
    assert_eq!(
        terminator_successors(&MirTerminatorKind::Goto(MirBlockId(2))),
        vec![MirBlockId(2)]
    );
}

#[test]
fn terminator_successors_branch_returns_then_and_else() {
    let branch = MirTerminatorKind::Branch {
        condition: MirOperand::Constant(MirConstant::Bool(true)),
        then_block: MirBlockId(3),
        else_block: MirBlockId(4),
    };
    assert_eq!(terminator_successors(&branch), vec![MirBlockId(3), MirBlockId(4)]);
}

#[test]
fn terminator_successors_try_call_returns_ok_and_error() {
    let try_call = MirTerminatorKind::TryCall {
        destination: None,
        callee: MirCallee::Function(MirFunctionId(1)),
        args: Vec::new(),
        ok_block: MirBlockId(5),
        error_place: MirPlace::local(MirLocalId(0)),
        error_block: MirBlockId(6),
    };
    assert_eq!(terminator_successors(&try_call), vec![MirBlockId(5), MirBlockId(6)]);
}

#[test]
fn terminator_successors_switch_returns_cases_and_default() {
    let switch = MirTerminatorKind::Switch {
        value: MirOperand::Constant(MirConstant::Int(1)),
        cases: vec![
            MirSwitchCase { value: MirConstant::Int(1), target: MirBlockId(8) },
            MirSwitchCase { value: MirConstant::Int(2), target: MirBlockId(9) },
        ],
        default: MirBlockId(10),
    };
    assert_eq!(
        terminator_successors(&switch),
        vec![MirBlockId(8), MirBlockId(9), MirBlockId(10)]
    );
}

#[test]
fn terminator_successors_return_none_is_empty() {
    assert_eq!(terminator_successors(&MirTerminatorKind::Return(None)), Vec::new());
}

#[test]
fn terminator_successors_switch_no_cases_returns_default_only() {
    let switch = MirTerminatorKind::Switch {
        value: MirOperand::Constant(MirConstant::Int(0)),
        cases: vec![],
        default: MirBlockId(99),
    };
    assert_eq!(terminator_successors(&switch), vec![MirBlockId(99)]);
}

#[test]
fn structural_visitor_walks_empty_program() {
    let program = MirProgram { functions: vec![] };
    let mut visitor = CountingVisitor::default();
    visitor.visit_program(&program);

    assert_eq!(visitor.functions, 0);
    assert_eq!(visitor.blocks, 0);
}

#[derive(Default)]
struct StopAfterFirstBlock {
    blocks: usize,
}

impl FallibleMirVisitor for StopAfterFirstBlock {
    type Error = &'static str;

    fn visit_block(&mut self, _block: &MirBlock) -> Result<(), Self::Error> {
        self.blocks += 1;
        Err("stop")
    }
}

#[test]
fn fallible_visitor_short_circuits_on_error() {
    let mut visitor = StopAfterFirstBlock::default();
    let result = visitor.visit_program(&sample_program());

    assert_eq!(result, Err("stop"));
    assert_eq!(visitor.blocks, 1);
}

#[test]
fn fallible_visitor_visits_empty_program_ok() {
    let program = MirProgram { functions: vec![] };
    let mut visitor = StopAfterFirstBlock::default();
    let result = visitor.visit_program(&program);

    assert_eq!(result, Ok(()));
    assert_eq!(visitor.blocks, 0);
}
