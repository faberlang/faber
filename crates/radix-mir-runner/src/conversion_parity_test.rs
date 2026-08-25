//! E3 runner parity for synthetic conversion-arm calls.

use super::*;
use radix_mir::{MirFunction, MirLocal, MirParam, MirStatement, MirTemp, MirTerminator};
use radix_types::Type;

fn span() -> Span {
    Span::default()
}

fn function(
    id: u32,
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    temps: Vec<MirTemp>,
    blocks: Vec<MirBlock>,
    return_ty: radix_mir::MirType,
    error_ty: Option<radix_mir::MirType>,
) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params,
        locals,
        temps,
        blocks,
        return_ty,
        error_ty,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

fn block(id: u32, statements: Vec<MirStatement>, terminator: MirTerminatorKind) -> MirBlock {
    MirBlock {
        id: MirBlockId(id),
        statements,
        terminator: MirTerminator {
            kind: terminator,
            span: span(),
        },
        span: span(),
    }
}

#[test]
fn conversion_arm_call_runs_once_only_on_propagate_error_path() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirCallee, MirConstant, MirDiagnosticKind, MirIntrinsic, MirLocalId, MirOperand, MirPlace,
        MirRuntimeCall, MirStatementKind, MirTempId, MirTerminatorKind,
    };
    use radix_types::{Primitive, TypeTable};

    let mut types = TypeTable::new();
    let vacuum = radix_mir::MirType::semantic(types.primitive(Primitive::Vacuum));
    let source_def = radix_hir::DefId(10);
    let destination_def = radix_hir::DefId(20);
    let source_ty = types.intern(Type::Enum(source_def));
    let destination_ty = types.intern(Type::Enum(destination_def));
    let source_mir_ty = radix_mir::MirType::semantic(source_ty);
    let destination_mir_ty = radix_mir::MirType::semantic(destination_ty);
    let mut interner = Interner::new();
    let raw = interner.intern("raw-source-error");
    let converted = interner.intern("conversion-arm-ran");

    let source_failure = function(
        0,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![block(
            0,
            Vec::new(),
            MirTerminatorKind::ReturnError(MirOperand::Constant(MirConstant::String(raw))),
        )],
        vacuum,
        Some(source_mir_ty),
    );
    let source_success = function(
        1,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![block(0, Vec::new(), MirTerminatorKind::Return(None))],
        vacuum,
        Some(source_mir_ty),
    );
    let conversion_arm = function(
        2,
        vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: source_mir_ty,
            mode: MirParamMode::Owned,
            span: span(),
        }],
        vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: source_mir_ty,
            mutable: false,
            span: span(),
        }],
        Vec::new(),
        vec![block(
            0,
            vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
                        args: vec![MirOperand::Constant(MirConstant::String(converted))],
                        return_ty: vacuum,
                    },
                },
                span: span(),
            }],
            MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        )],
        destination_mir_ty,
        None,
    );

    let propagate = |id: u32, callee: MirFunctionId| {
        function(
            id,
            Vec::new(),
            Vec::new(),
            vec![
                MirTemp {
                    id: MirTempId(0),
                    ty: source_mir_ty,
                    span: span(),
                },
                MirTemp {
                    id: MirTempId(1),
                    ty: destination_mir_ty,
                    span: span(),
                },
            ],
            vec![
                block(
                    0,
                    Vec::new(),
                    MirTerminatorKind::TryCall {
                        destination: None,
                        callee: MirCallee::Function(callee),
                        args: Vec::new(),
                        ok_block: MirBlockId(1),
                        error_place: MirPlace::temp(MirTempId(0)),
                        error_block: MirBlockId(2),
                    },
                ),
                block(1, Vec::new(), MirTerminatorKind::Return(None)),
                block(
                    2,
                    vec![MirStatement {
                        kind: MirStatementKind::Call {
                            destination: Some(MirPlace::temp(MirTempId(1))),
                            callee: MirCallee::Function(MirFunctionId(2)),
                            args: vec![MirOperand::Temp(MirTempId(0))],
                        },
                        span: span(),
                    }],
                    MirTerminatorKind::ReturnError(MirOperand::Temp(MirTempId(1))),
                ),
            ],
            vacuum,
            Some(destination_mir_ty),
        )
    };

    let program = MirProgram {
        functions: vec![
            source_failure,
            source_success,
            conversion_arm,
            propagate(3, MirFunctionId(0)),
            propagate(4, MirFunctionId(1)),
        ],
    };
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);

    runner.push_frame(MirFunctionId(3), MirBlockId(0));
    runner.run_until_depth(0);
    assert!(
        runner.errors.is_empty(),
        "failure propagation should run: {:?}",
        runner.errors
    );
    assert_eq!(
        runner.return_error,
        Some(Value::Str("raw-source-error".to_owned()))
    );

    runner.push_frame(MirFunctionId(4), MirBlockId(0));
    runner.run_until_depth(0);
    assert!(
        runner.errors.is_empty(),
        "successful call should run: {:?}",
        runner.errors
    );
    assert_eq!(runner.return_value, Some(Value::Unit));
    assert_eq!(host.stdout_lines, ["conversion-arm-ran"]);
}
