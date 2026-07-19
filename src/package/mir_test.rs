use super::*;
use radix::lexer::Span;
use radix::mir::MirTempId;

#[test]
fn fmir_runtime_cli_binding_skips_superset_decoy_record() {
    let mut interner = Interner::default();
    let run_entry = interner.intern("run_entry");
    let name = interner.intern("name");
    let extra = interner.intern("extra");
    let build_time = interner.intern("build-time");
    let decoy_extra = interner.intern("decoy-extra");
    let runtime = interner.intern("runtime");
    let ty = MirType::semantic(TypeId(0));
    let span = Span::default();
    let mut program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: Some(run_entry),
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    record_construct(
                        MirTempId(0),
                        ty,
                        span,
                        vec![
                            MirNamedOperand {
                                name,
                                value: MirOperand::Constant(MirConstant::String(build_time)),
                            },
                            MirNamedOperand {
                                name: extra,
                                value: MirOperand::Constant(MirConstant::String(decoy_extra)),
                            },
                        ],
                    ),
                    record_construct(
                        MirTempId(1),
                        ty,
                        span,
                        vec![MirNamedOperand {
                            name,
                            value: MirOperand::Constant(MirConstant::String(build_time)),
                        }],
                    ),
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }],
            return_ty: ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            span,
        }],
    };
    let cli = FmirTextCliSection {
        root: FmirTextCliRootSection {
            record: "args".to_owned(),
            operand: vec![FmirTextCliOperand {
                field: "name".to_owned(),
                ty: FmirTextCliValueType::Textus,
            }],
        },
    };

    let patched = patch_fmir_text_cli_record(
        &mut program,
        &cli,
        "run_entry",
        &interner,
        &[MirNamedOperand {
            name,
            value: MirOperand::Constant(MirConstant::String(runtime)),
        }],
    );

    assert!(patched);
    assert_eq!(record_field_string(&program, 0, name), Some(build_time));
    assert_eq!(record_field_string(&program, 1, name), Some(runtime));
}

fn record_construct(
    destination: MirTempId,
    ty: MirType,
    span: Span,
    fields: Vec<MirNamedOperand>,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace::temp(destination),
            aggregate: MirAggregate {
                kind: MirAggregateKind::Record,
                ty,
                fields: MirAggregateFields::Named(fields),
            },
        },
        span,
    }
}

fn record_field_string(
    program: &MirProgram,
    statement_index: usize,
    field_name: Symbol,
) -> Option<Symbol> {
    let statement = program
        .functions
        .first()?
        .blocks
        .first()?
        .statements
        .get(statement_index)?;
    let MirStatementKind::Construct { aggregate, .. } = &statement.kind else {
        return None;
    };
    let MirAggregateFields::Named(fields) = &aggregate.fields else {
        return None;
    };
    fields.iter().find_map(|field| {
        if field.name != field_name {
            return None;
        }
        match field.value {
            MirOperand::Constant(MirConstant::String(symbol)) => Some(symbol),
            _ => None,
        }
    })
}

// R0 red artifact contract: adding `MirConstant::UInt(u64)` to the serialized
// MIR schema is an approved clean break, so the package MIR artifact version
// moves 2 → 3 (no dual-format reader). Fails until R2 lands the bump.
#[test]
fn package_mir_artifact_version_is_3_for_unsigned_constant_schema() {
    assert_eq!(
        PACKAGE_MIR_ARTIFACT_VERSION, 3,
        "MirConstant::UInt requires the FMIR artifact version 3 clean break"
    );
}
