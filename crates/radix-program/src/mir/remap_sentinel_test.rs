//! ARCH2-2 sentinel fixture: every symbol-bearing field the remap walker
//! visits carries a distinct sentinel text, the program is merged into a
//! second (entry) interner, and every field must resolve to its own
//! sentinel afterwards.
//!
//! The expected program is built from the same fixture with the sentinel
//! symbols of the TARGET interner, so a field the walker misses keeps its
//! source interner value and the programs compare unequal.

use super::DefId;
use super::remap::remap_program_text_symbols;
use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirCallee, MirClosureCallee, MirClosureEnvironmentId, MirClosureValue, MirConstant,
    MirConversion, MirConversionFlavor, MirFilumCoordinate, MirFilumReduceOp, MirFunction,
    MirFunctionId, MirIntrinsic, MirKeyValueOperand, MirLocal, MirLocalId, MirNamedOperand,
    MirOperand, MirOptionChainLink, MirOptionOp, MirOptionUnwrapMode, MirParam, MirParamMode,
    MirPlace, MirPlaceBase, MirProgram, MirProjection, MirProvider, MirProviderKind,
    MirRuntimeCall, MirStatement, MirStatementKind, MirStructFieldDefaults, MirSwitchCase,
    MirTerminator, MirTerminatorKind, MirType, MirUnOp, MirValue, MirValueId, MirValueKind,
};
use radix_module::lexer::{Interner, Span, Symbol};
use radix_types::{Primitive, TypeTable};

/// The distinct sentinel texts, indexed by fixture slot.
const SENTINELS: [&str; 52] = [
    "sentinel.function.name",
    "sentinel.param.name",
    "sentinel.local.name",
    "sentinel.place.field",
    "sentinel.place.variant-field",
    "sentinel.place.index",
    "sentinel.value.string",
    "sentinel.value.closure-environment",
    "sentinel.value.unary.operand",
    "sentinel.value.binary.lhs",
    "sentinel.value.binary.rhs",
    "sentinel.option.some",
    "sentinel.option.is-nil",
    "sentinel.option.is-not-nil",
    "sentinel.option.unwrap.value",
    "sentinel.option.coalesce.value",
    "sentinel.option.coalesce.fallback",
    "sentinel.option.chain.base",
    "sentinel.option.chain.link.field",
    "sentinel.option.chain.link.variant-field",
    "sentinel.option.chain.link.index",
    "sentinel.option.chain.link.call.callee-environment",
    "sentinel.option.chain.link.call.arg",
    "sentinel.call.callee-environment",
    "sentinel.call.arg",
    "sentinel.call.value-callee",
    "sentinel.runtime.provider.module.0",
    "sentinel.runtime.provider.module.1",
    "sentinel.runtime.provider.name",
    "sentinel.runtime.arg",
    "sentinel.runtime.format.template",
    "sentinel.runtime.convert.param",
    "sentinel.runtime.convert.recovery",
    "sentinel.runtime.convert.default-field.name",
    "sentinel.runtime.convert.default-field.value",
    "sentinel.filum.binder",
    "sentinel.filum.term-binder-name",
    "sentinel.filum.coordinate.name",
    "sentinel.filum.term-body.string",
    "sentinel.construct.named.name",
    "sentinel.construct.named.value",
    "sentinel.construct.keyed.key",
    "sentinel.construct.keyed.value",
    "sentinel.constant.regex.pattern",
    "sentinel.constant.regex.flags",
    "sentinel.terminator.return-error",
    "sentinel.terminator.try-call.callee-value",
    "sentinel.terminator.try-call.arg",
    "sentinel.terminator.try-call.error-place.variant-field",
    "sentinel.terminator.branch.condition",
    "sentinel.terminator.switch.value",
    "sentinel.terminator.switch.case",
];

/// Build the fixture program; slot `i` of every symbol-bearing field carries
/// `sym(i)`.
fn sentinel_program(sym: &dyn Fn(usize) -> Symbol) -> MirProgram {
    let types = TypeTable::new();
    let ty = MirType::semantic(types.primitive(Primitive::Numerus));
    let span = Span::default();
    let string = |slot: usize| MirConstant::String(sym(slot));

    let full_projections = || MirPlace {
        base: MirPlaceBase::Local(MirLocalId(0)),
        projections: vec![
            MirProjection::Field(sym(3)),
            MirProjection::VariantField {
                variant: DefId(9),
                field: sym(4),
            },
            MirProjection::Index(MirOperand::Constant(string(5))),
            MirProjection::ClosureCapture {
                environment: MirClosureEnvironmentId(0),
                source: DefId(10),
            },
            MirProjection::VectorLane(0),
            MirProjection::MatrixCell { row: 0, column: 1 },
        ],
    };
    let simple_place = || MirPlace {
        base: MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![],
    };
    let assign = |value: MirValueKind, id: u32| MirStatement {
        kind: MirStatementKind::Assign {
            place: simple_place(),
            value: MirValue {
                id: MirValueId(id),
                kind: value,
                ty,
                span,
            },
        },
        span,
    };
    let operand = |constant: MirConstant| MirOperand::Constant(constant);

    let mut statements = vec![
        MirStatement {
            kind: MirStatementKind::Assign {
                place: full_projections(),
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(operand(string(6))),
                    ty,
                    span,
                },
            },
            span,
        },
        assign(
            MirValueKind::Closure(MirClosureValue {
                function: MirFunctionId(1),
                environment: operand(string(7)),
                environment_id: MirClosureEnvironmentId(0),
            }),
            1,
        ),
        assign(
            MirValueKind::Unary {
                op: MirUnOp::Neg,
                operand: operand(MirConstant::Ascii(sym(8))),
            },
            2,
        ),
        assign(
            MirValueKind::Binary {
                op: MirBinOp::Add,
                lhs: operand(string(9)),
                rhs: operand(string(10)),
            },
            3,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::Some(operand(string(11)))),
            4,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::IsNil(operand(string(12)))),
            5,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::IsNotNil(operand(string(13)))),
            6,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::Unwrap {
                value: operand(string(14)),
                mode: MirOptionUnwrapMode::Assert,
            }),
            7,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::Coalesce {
                value: operand(string(15)),
                fallback: operand(string(16)),
            }),
            8,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::Chain {
                base: operand(string(17)),
                link: MirOptionChainLink::Field(sym(18)),
            }),
            9,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::Chain {
                base: operand(string(17)),
                link: MirOptionChainLink::VariantField {
                    variant: DefId(20),
                    field: sym(19),
                },
            }),
            10,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::Chain {
                base: operand(string(17)),
                link: MirOptionChainLink::Index(operand(string(20))),
            }),
            11,
        ),
        assign(
            MirValueKind::Option(MirOptionOp::Chain {
                base: operand(string(17)),
                link: MirOptionChainLink::Call {
                    callee: MirCallee::Closure(MirClosureCallee {
                        function: MirFunctionId(2),
                        environment: operand(string(21)),
                        environment_id: MirClosureEnvironmentId(1),
                    }),
                    args: vec![operand(string(22))],
                },
            }),
            12,
        ),
        MirStatement {
            kind: MirStatementKind::Call {
                destination: Some(simple_place()),
                callee: MirCallee::Closure(MirClosureCallee {
                    function: MirFunctionId(3),
                    environment: operand(string(23)),
                    environment_id: MirClosureEnvironmentId(2),
                }),
                args: vec![operand(string(24))],
            },
            span,
        },
        MirStatement {
            kind: MirStatementKind::Call {
                destination: None,
                callee: MirCallee::Value(operand(string(25))),
                args: vec![],
            },
            span,
        },
        MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(simple_place()),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Provider(MirProvider {
                        kind: MirProviderKind::Package,
                        module: vec![sym(26), sym(27)],
                        name: sym(28),
                    }),
                    args: vec![operand(string(29))],
                    return_ty: ty,
                },
            },
            span,
        },
        MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::FormatString { template: sym(30) },
                    args: vec![],
                    return_ty: ty,
                },
            },
            span,
        },
        MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(simple_place()),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Convert(MirConversion {
                        store: None,
                        flavor: MirConversionFlavor::Cast,
                        source_ty: ty,
                        target_ty: ty,
                        params: vec![sym(31)],
                        recovery: Some(operand(string(32))),
                        struct_defaults: vec![MirStructFieldDefaults {
                            def_id: DefId(30),
                            fields: vec![MirNamedOperand {
                                name: sym(33),
                                value: operand(string(34)),
                            }],
                        }],
                    }),
                    args: vec![],
                    return_ty: ty,
                },
            },
            span,
        },
        MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(simple_place()),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::FilumReduce(MirFilumReduceOp {
                        op: radix_mir::MirReduceOp::Sum,
                        form: radix_mir::MirFilumForm::Declared,
                        term_ty: radix_mir::MirType::semantic(radix_types::TypeId(0)),
                        filum_binder: Some(sym(35)),
                        term_binder: MirLocalId(5),
                        term_binder_name: Some(sym(36)),
                        coordinates: vec![MirFilumCoordinate {
                            local: MirLocalId(6),
                            name: Some(sym(37)),
                        }],
                        lane_width: 32,
                        term_body: vec![{
                            let mut statement =
                                assign(MirValueKind::Operand(operand(string(38))), 20);
                            let MirStatementKind::Assign { place, .. } = &mut statement.kind else {
                                unreachable!("built an assign")
                            };
                            *place = simple_place();
                            statement
                        }],
                        term_captures: None,
                        term_result: MirLocalId(7),
                    }),
                    args: vec![],
                    return_ty: ty,
                },
            },
            span,
        },
        MirStatement {
            kind: MirStatementKind::Construct {
                destination: simple_place(),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Record,
                    ty,
                    fields: MirAggregateFields::Named(vec![MirNamedOperand {
                        name: sym(39),
                        value: operand(string(40)),
                    }]),
                },
            },
            span,
        },
        MirStatement {
            kind: MirStatementKind::Construct {
                destination: simple_place(),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Map,
                    ty,
                    fields: MirAggregateFields::Keyed(vec![MirKeyValueOperand {
                        key: operand(string(41)),
                        value: operand(string(42)),
                    }]),
                },
            },
            span,
        },
    ];

    // Exercise the ordered-items aggregate arm too (both item shapes).
    statements.push(MirStatement {
        kind: MirStatementKind::Construct {
            destination: simple_place(),
            aggregate: MirAggregate {
                kind: MirAggregateKind::Array,
                ty,
                fields: MirAggregateFields::Ordered(vec![
                    MirAggregateItem::Operand(operand(string(6))),
                    MirAggregateItem::Spread(operand(string(6))),
                ]),
            },
        },
        span,
    });

    let block = |id: u32, statements: Vec<MirStatement>, terminator: MirTerminatorKind| MirBlock {
        id: MirBlockId(id),
        statements,
        terminator: MirTerminator {
            kind: terminator,
            span,
        },
        span,
    };

    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(0)),
            name: Some(sym(0)),
            params: vec![MirParam {
                local: MirLocalId(0),
                name: Some(sym(1)),
                ty,
                mode: MirParamMode::Owned,
                span,
            }],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: Some(sym(2)),
                ty,
                mutable: false,
                span,
            }],
            temps: vec![],
            blocks: vec![
                block(
                    0,
                    statements,
                    MirTerminatorKind::Return(Some(operand(MirConstant::Regex {
                        pattern: sym(43),
                        flags: Some(sym(44)),
                    }))),
                ),
                block(
                    1,
                    vec![],
                    MirTerminatorKind::ReturnError(operand(string(45))),
                ),
                block(
                    2,
                    vec![],
                    MirTerminatorKind::TryCall {
                        destination: Some(simple_place()),
                        callee: MirCallee::Value(operand(string(46))),
                        args: vec![operand(string(47))],
                        ok_block: MirBlockId(3),
                        error_place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![MirProjection::VariantField {
                                variant: DefId(50),
                                field: sym(48),
                            }],
                        },
                        error_block: MirBlockId(4),
                    },
                ),
                block(
                    3,
                    vec![],
                    MirTerminatorKind::Branch {
                        condition: operand(string(49)),
                        then_block: MirBlockId(0),
                        else_block: MirBlockId(1),
                    },
                ),
                block(
                    4,
                    vec![],
                    MirTerminatorKind::Switch {
                        value: operand(string(50)),
                        cases: vec![MirSwitchCase {
                            value: MirConstant::String(sym(51)),
                            target: MirBlockId(0),
                        }],
                        default: MirBlockId(1),
                    },
                ),
            ],
            return_ty: ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }],
    }
}

#[test]
fn every_symbol_bearing_field_survives_the_merge_as_its_own_sentinel() {
    let mut source = Interner::new();
    let source_symbols: Vec<Symbol> = SENTINELS.iter().map(|s| source.intern(s)).collect();

    // The merged interner pre-populates unrelated text first, so the same
    // sentinel text lands on a DIFFERENT symbol value there: a field the
    // walker misses keeps its source value and cannot pass by coincidence.
    let mut target = Interner::new();
    for filler in 0..SENTINELS.len() {
        target.intern(&format!("entry-interner.filler.{filler:02}"));
    }
    let target_symbols: Vec<Symbol> = SENTINELS.iter().map(|s| target.intern(s)).collect();

    let mut program = sentinel_program(&|slot| source_symbols[slot]);
    let expected = sentinel_program(&|slot| target_symbols[slot]);

    remap_program_text_symbols(&mut program, &source, &mut target);

    assert_eq!(
        program, expected,
        "a symbol-bearing field did not remap to its own sentinel text"
    );

    // Every sentinel resolves to itself in the merged interner.
    for (slot, text) in SENTINELS.iter().enumerate() {
        assert_eq!(target.resolve(target_symbols[slot]), *text);
    }
}
