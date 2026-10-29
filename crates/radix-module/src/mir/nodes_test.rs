//! Tests for MIR node semantics and program dump rendering.
//!
//! These tests prove that MirType wraps semantic types with optional
//! layout slots and that dump_program renders MIR structures (blocks,
//! runtime calls, aggregate construction, option operations, intrinsic
//! dispatch) deterministically. Dump output is a diagnostic contract.

use super::*;
use crate::lexer::Symbol;
use crate::mir::dump_program;
use crate::semantic::{Primitive, TypeTable};
use crate::test_support::test_span;
use radix_mir::device::MirDeviceFunctionMetadata;
use std::collections::HashMap;

#[test]
fn mir_type_wraps_semantic_type_and_reserves_layout_slot() {
    let types = TypeTable::new();
    let text = types.primitive(Primitive::Textus);
    let plain = MirType::semantic(text);
    let laid_out = MirType::with_layout(text, MirLayoutId(7));

    assert_eq!(plain.semantic_id(), text);
    assert_eq!(plain.layout_id(), None);
    assert_eq!(laid_out.semantic_id(), text);
    assert_eq!(laid_out.layout_id(), Some(MirLayoutId(7)));
}

#[test]
fn synthetic_device_metadata_attaches_to_source_less_functions() {
    let types = TypeTable::new();
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let function = MirFunction {
        id: MirFunctionId(7),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: test_span(),
    };
    let program = MirProgram { functions: vec![function.clone()] };
    let metadata = [(
        function.id,
        MirDeviceFunctionMetadata {
            identity: MirSyntheticFunctionIdentity { parent: MirFunctionId(2), ordinal: 0 },
            role: MirDeviceRole::Kernel,
            shader_stage: MirKernelShaderStage::Compute,
            visibility: MirFunctionVisibility::Private,
            origin: MirFunctionOrigin::SyntheticKernelClosure,
            lane_domain_local: None,
            refused_backends: Default::default(),
            tier: radix_mir::device::KernelTier::Default,
        },
    )]
    .into_iter()
    .collect();
    let mut device = MirDeviceContext::from_hir_roles_with_metadata(HashMap::default(), &metadata);
    device.attach_program(&program);

    assert_eq!(device.role_for_function(&function), Some(MirDeviceRole::Kernel));
    assert_eq!(device.metadata_for_function(&function), metadata.get(&function.id).copied());
}

#[test]
fn dump_program_renders_basic_blocks_deterministically() {
    let types = TypeTable::new();
    let number = MirType::semantic(types.primitive(Primitive::Numerus));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: number, mutable: false, span: test_span() }],
            temps: vec![MirTemp { id: MirTempId(0), ty: number, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::local(MirLocalId(0)),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Binary {
                                op: MirBinOp::Add,
                                lhs: MirOperand::Constant(MirConstant::Int(1)),
                                rhs: MirOperand::Constant(MirConstant::Int(2)),
                            },
                            ty: number,
                            span: test_span(),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: number,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    assert_eq!(
        dump_program(&program),
        "\
function f0 -> ty#1 {
  locals:
    let _0: ty#1
  temps:
    %0: ty#1
  bb0:
    _0 = const int 1 + const int 2: ty#1
    return _0
}
"
    );
}

#[test]
fn dump_program_renders_runtime_calls_and_branches() {
    let types = TypeTable::new();
    let boolean = MirType::semantic(types.primitive(Primitive::Bivalens));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(3),
            source: None,
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: boolean,
                mode: MirParamMode::Owned,
                span: test_span(),
            }],
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Branch {
                            condition: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            then_block: MirBlockId(1),
                            else_block: MirBlockId(2),
                        },
                        span: test_span(),
                    },
                    span: test_span(),
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: None,
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Scribe),
                                args: vec![MirOperand::Constant(MirConstant::String(Symbol(9)))],
                                return_ty: vacuum,
                            },
                        },
                        span: test_span(),
                    }],
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
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    assert_eq!(
        dump_program(&program),
        "\
function f3 -> ty#5 {
  params:
    _0: ty#3
  bb0:
    branch _0 bb1 bb2
  bb1:
    runtime diagnostic scribe(const string sym#9) -> ty#5
    goto bb2
  bb2:
    return
}
"
    );
}

#[test]
fn dump_program_renders_aggregate_payloads_and_operand_index_projection() {
    let types = TypeTable::new();
    let text = MirType::semantic(types.primitive(Primitive::Textus));
    let number = MirType::semantic(types.primitive(Primitive::Numerus));

    let indexed_place = MirPlace {
        base: MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![MirProjection::Index(MirOperand::Constant(MirConstant::String(Symbol(
            14,
        ))))],
    };

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![
                MirLocal { id: MirLocalId(0), name: None, ty: text, mutable: false, span: test_span() },
                MirLocal { id: MirLocalId(1), name: None, ty: number, mutable: false, span: test_span() },
            ],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    MirStatement {
                        kind: MirStatementKind::Construct {
                            destination: MirPlace::local(MirLocalId(0)),
                            aggregate: MirAggregate {
                                kind: MirAggregateKind::Struct(crate::hir::DefId(7)),
                                ty: text,
                                fields: MirAggregateFields::Named(vec![
                                    MirNamedOperand {
                                        name: Symbol(11),
                                        value: MirOperand::Constant(MirConstant::String(Symbol(12))),
                                    },
                                    MirNamedOperand {
                                        name: Symbol(13),
                                        value: MirOperand::Constant(MirConstant::Int(36)),
                                    },
                                ]),
                            },
                        },
                        span: test_span(),
                    },
                    MirStatement {
                        kind: MirStatementKind::Construct {
                            destination: MirPlace::local(MirLocalId(1)),
                            aggregate: MirAggregate {
                                kind: MirAggregateKind::Map,
                                ty: number,
                                fields: MirAggregateFields::Keyed(vec![MirKeyValueOperand {
                                    key: MirOperand::Constant(MirConstant::String(Symbol(14))),
                                    value: MirOperand::Constant(MirConstant::Int(1)),
                                }]),
                            },
                        },
                        span: test_span(),
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(indexed_place))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: number,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    assert_eq!(
        dump_program(&program),
        "\
function f0 -> ty#1 {
  locals:
    let _0: ty#0
    let _1: ty#1
  bb0:
    _0 = construct struct def#7: ty#0 {sym#11: const string sym#12, sym#13: const int 36}
    _1 = construct map: ty#1 {const string sym#14 => const int 1}
    return _1[const string sym#14]
}
"
    );
}

#[test]
fn dump_program_renders_option_operations() {
    let types = TypeTable::new();
    let text = MirType::semantic(types.primitive(Primitive::Textus));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![
                MirLocal { id: MirLocalId(0), name: None, ty: text, mutable: false, span: test_span() },
                MirLocal { id: MirLocalId(1), name: None, ty: text, mutable: false, span: test_span() },
                MirLocal { id: MirLocalId(2), name: None, ty: text, mutable: false, span: test_span() },
            ],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(0)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Option(MirOptionOp::Some(MirOperand::Constant(
                                    MirConstant::String(Symbol(9)),
                                ))),
                                ty: text,
                                span: test_span(),
                            },
                        },
                        span: test_span(),
                    },
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(1)),
                            value: MirValue {
                                id: MirValueId(1),
                                kind: MirValueKind::Option(MirOptionOp::Chain {
                                    base: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                    link: MirOptionChainLink::Field(Symbol(10)),
                                }),
                                ty: text,
                                span: test_span(),
                            },
                        },
                        span: test_span(),
                    },
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(2)),
                            value: MirValue {
                                id: MirValueId(2),
                                kind: MirValueKind::Option(MirOptionOp::Coalesce {
                                    value: MirOperand::Place(MirPlace::local(MirLocalId(1))),
                                    fallback: MirOperand::Constant(MirConstant::String(Symbol(11))),
                                }),
                                ty: text,
                                span: test_span(),
                            },
                        },
                        span: test_span(),
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(2))))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: text,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    assert_eq!(
        dump_program(&program),
        "\
function f0 -> ty#0 {
  locals:
    let _0: ty#0
    let _1: ty#0
    let _2: ty#0
  bb0:
    _0 = option some(const string sym#9): ty#0
    _1 = option chain(_0, .sym#10): ty#0
    _2 = option coalesce(_1, const string sym#11): ty#0
    return _2
}
"
    );
}

#[test]
fn dump_program_renders_structured_runtime_intrinsics() {
    let types = TypeTable::new();
    let text = MirType::semantic(types.primitive(Primitive::Textus));
    let number = MirType::semantic(types.primitive(Primitive::Numerus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![
                MirLocal { id: MirLocalId(0), name: None, ty: text, mutable: false, span: test_span() },
                MirLocal { id: MirLocalId(1), name: None, ty: number, mutable: false, span: test_span() },
            ],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: None,
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Mone),
                                args: vec![MirOperand::Constant(MirConstant::String(Symbol(20)))],
                                return_ty: vacuum,
                            },
                        },
                        span: test_span(),
                    },
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::local(MirLocalId(0))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::FormatString { template: Symbol(21) },
                                args: vec![MirOperand::Constant(MirConstant::String(Symbol(22)))],
                                return_ty: text,
                            },
                        },
                        span: test_span(),
                    },
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::local(MirLocalId(1))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Convert(MirConversion {
                                    store: None,
                                    flavor: MirConversionFlavor::Runtime,
                                    source_ty: number,
                                    target_ty: number,
                                    params: Vec::new(),
                                    recovery: Some(MirOperand::Constant(MirConstant::Int(0))),
                                    struct_defaults: Vec::new(),
                                }),
                                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                                return_ty: number,
                            },
                        },
                        span: test_span(),
                    },
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::local(MirLocalId(1))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Collection(MirCollectionOp::Length),
                                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                                return_ty: number,
                            },
                        },
                        span: test_span(),
                    },
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::local(MirLocalId(0))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Provider(MirProvider {
                                    kind: MirProviderKind::Package,
                                    module: vec![Symbol(30), Symbol(31)],
                                    name: Symbol(32),
                                }),
                                args: Vec::new(),
                                return_ty: text,
                            },
                        },
                        span: test_span(),
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: text,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    assert_eq!(
        dump_program(&program),
        "\
function f0 -> ty#0 {
  locals:
    let _0: ty#0
    let _1: ty#1
  bb0:
    runtime diagnostic mone(const string sym#20) -> ty#5
    _0 = runtime format_string template sym#21(const string sym#22) -> ty#0
    _1 = runtime convert runtime ty#1 -> ty#1 recovery const int 0(_0) -> ty#1
    _1 = runtime collection length(_0) -> ty#1
    _0 = runtime provider sym#30/sym#31::sym#32() -> ty#0
    return _0
}
"
    );
}
