use super::*;
use crate::nodes::{
    HirAnnotation, HirAnnotationField, HirAnnotationValue, HirBlockEnd, HirExpression,
    HirExpressionKind, HirId, HirLiteral, HirSourceAnchorId,
};
use radix_lexer::{Span, Symbol};

fn annotation_expr() -> HirAnnotation {
    HirAnnotation {
        family: Symbol(0),
        fields: vec![HirAnnotationField {
            name: Symbol(1),
            value: HirAnnotationValue::Expr(Box::new(HirExpression {
                id: HirId(0),
                kind: HirExpressionKind::Literal(HirLiteral::Int(42)),
                ty: None,
                span: Span::default(),
            })),
            span: Span::default(),
        }],
        brace_authored: true,
        raw_args: Vec::new(),
        contract_id: None,
        span: Span::default(),
    }
}

// PRP-016: read-only visitor must walk into annotation Expr payloads.
struct ExprCollector {
    seen: Vec<u64>,
}
impl HirVisitor for ExprCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Literal(HirLiteral::Int(n)) = &expr.kind {
            self.seen.push(*n);
        }
        walk_expr(self, expr);
    }
}

#[test]
fn visitor_sees_annotation_expressions() {
    let ann = annotation_expr();
    let mut collector = ExprCollector { seen: Vec::new() };
    collector.visit_annotation(&ann);
    assert_eq!(
        collector.seen,
        vec![42],
        "visitor must see Expr in annotation"
    );
}

// PRP-016 F3: mutable visitor must also walk annotation Expr payloads.
struct ExprMarker;
impl HirVisitorMut for ExprMarker {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        if let HirExpressionKind::Literal(HirLiteral::Int(n)) = &mut expr.kind {
            *n = 99;
        }
        walk_expr_mut(self, expr);
    }
}

#[test]
fn mut_visitor_sees_annotation_expressions() {
    let mut ann = annotation_expr();
    let mut marker = ExprMarker;
    marker.visit_annotation_mut(&mut ann);
    match &ann.fields[0].value {
        HirAnnotationValue::Expr(expr) => match &expr.kind {
            HirExpressionKind::Literal(HirLiteral::Int(n)) => {
                assert_eq!(*n, 99, "mutable visitor must modify Expr in annotation");
            }
            _ => panic!("expected literal"),
        },
        _ => panic!("expected expr"),
    }
}

// PRP-016 integration: walk_item must drive envelope annotations through
// visit_program, not just the standalone helper.
fn program_with_annotated_function() -> HirModule {
    use crate::nodes::{HirItem, HirItemKind};
    let func = HirFunction {
        name: Symbol(0),
        type_params: Vec::new(),
        params: Vec::new(),
        cli_args: None,
        ret_ty: None,
        err_ty: None,
        body: None,
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };
    HirModule {
        items: vec![HirItem {
            annotations: vec![annotation_expr()],
            id: HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(func)),
            span: Span::default(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn visit_program_drives_function_annotations() {
    let program = program_with_annotated_function();
    let mut collector = ExprCollector { seen: Vec::new() };
    collector.visit_program(&program);
    assert_eq!(
        collector.seen,
        vec![42],
        "visit_program must reach function annotation Expr"
    );
}

#[test]
fn visit_program_mut_drives_function_annotations() {
    let mut program = program_with_annotated_function();
    let mut marker = ExprMarker;
    marker.visit_program_mut(&mut program);
    assert!(matches!(program.items[0].kind, HirItemKind::Function(_)));
    match &program.items[0].annotations[0].fields[0].value {
        HirAnnotationValue::Expr(expr) => match &expr.kind {
            HirExpressionKind::Literal(HirLiteral::Int(n)) => {
                assert_eq!(
                    *n, 99,
                    "visit_program_mut must reach function annotation Expr"
                );
            }
            _ => panic!("expected literal"),
        },
        _ => panic!("expected expr"),
    }
}

// PRP-075: walk_program/walk_program_mut must visit entry_annotations.
fn program_with_entry_annotations() -> HirModule {
    HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: vec![annotation_expr()],
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn visit_program_drives_entry_annotations() {
    let program = program_with_entry_annotations();
    let mut collector = ExprCollector { seen: Vec::new() };
    collector.visit_program(&program);
    assert_eq!(
        collector.seen,
        vec![42],
        "visit_program must reach entry_annotation Expr",
    );
}

#[test]
fn visit_program_mut_drives_entry_annotations() {
    let mut program = program_with_entry_annotations();
    let mut marker = ExprMarker;
    marker.visit_program_mut(&mut program);
    match &program.entry_annotations[0].fields[0].value {
        HirAnnotationValue::Expr(expr) => match &expr.kind {
            HirExpressionKind::Literal(HirLiteral::Int(n)) => {
                assert_eq!(*n, 99, "visit_program_mut must reach entry_annotation Expr");
            }
            _ => panic!("expected literal"),
        },
        _ => panic!("expected expr"),
    }
}

// U-2 Stage 2: `HirBlock.end` is a structural field, not a statement.
// A visitor counting statements must neither count nor recurse into it.
#[derive(Default)]
struct StatementCounter {
    statements: usize,
    blocks: usize,
}
impl HirVisitor for StatementCounter {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        self.statements += 1;
        walk_stmt(self, stmt);
    }
    fn visit_block(&mut self, block: &HirBlock) {
        self.blocks += 1;
        walk_block(self, block);
    }
}

#[test]
fn visitor_does_not_count_block_ends_as_statements() {
    let entry = HirBlock {
        statements: vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Tacet,
            span: Span::default(),
        }],
        expr: None,
        span: Span::default(),
        breakable: None,
        end: Some(HirBlockEnd {
            id: HirSourceAnchorId(7),
            span: Span::default(),
        }),
    };
    let program = HirModule {
        items: Vec::new(),
        entry: Some(entry),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let mut counter = StatementCounter::default();
    counter.visit_program(&program);
    assert_eq!(
        counter.statements, 1,
        "the block end must not be visited as a statement"
    );
    assert_eq!(counter.blocks, 1, "the entry block is visited once");
}

// Traversal symmetry contract: `Praefixum` wraps a real inner expression
// subtree, so both walkers must recurse into it. The artifact
// collector/validator mirrors this shape (artifact collector↔validator↔
// visitor symmetry).
fn praefixum_expr(inner_id: HirId) -> HirExpression {
    HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Praefixum(Box::new(HirExpression {
            id: inner_id,
            kind: HirExpressionKind::Literal(HirLiteral::Int(7)),
            ty: None,
            span: Span::default(),
        })),
        ty: None,
        span: Span::default(),
    }
}

#[test]
fn visitor_recurses_into_praefixum_operand() {
    let expr = praefixum_expr(HirId(5));
    let mut collector = ExprCollector { seen: Vec::new() };
    collector.visit_expr(&expr);
    assert_eq!(
        collector.seen,
        vec![7],
        "visit_expr must recurse into the Praefixum operand"
    );
}

#[test]
fn mut_visitor_recurses_into_praefixum_operand() {
    let mut expr = praefixum_expr(HirId(5));
    let mut marker = ExprMarker;
    marker.visit_expr_mut(&mut expr);
    let HirExpressionKind::Praefixum(inner) = &expr.kind else {
        panic!("expected praefixum");
    };
    let HirExpressionKind::Literal(HirLiteral::Int(n)) = &inner.kind else {
        panic!("expected literal");
    };
    assert_eq!(*n, 99, "visit_expr_mut must reach the Praefixum operand");
}

// Type-alias generic parameters introduce DefIds, same as struct/enum/interface.
fn program_with_generic_type_alias() -> HirModule {
    use crate::nodes::{HirItem, HirItemKind, HirTypeAlias, HirTypeParam, HirTypeParamConstraint};
    use radix_syntax::GenericParamKind;
    use radix_types::TypeId;
    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(1),
            kind: HirItemKind::TypeAlias(HirTypeAlias {
                name: Symbol(0),
                type_params: vec![HirTypeParam {
                    def_id: DefId(2),
                    kind: GenericParamKind::Typus,
                    name: Symbol(3),
                    span: Span::default(),
                    constraint: HirTypeParamConstraint::Any,
                    default: None,
                }],
                ty: TypeId(0),
            }),
            span: Span::default(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[derive(Default)]
struct DefCollector {
    defs: Vec<u32>,
}
impl HirVisitor for DefCollector {
    fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
        self.defs.push(def_id.0);
    }
}
impl HirVisitorMut for DefCollector {
    fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
        self.defs.push(def_id.0);
    }
}

#[test]
fn visit_program_reports_type_alias_type_params() {
    let program = program_with_generic_type_alias();
    let mut collector = DefCollector::default();
    collector.visit_program(&program);
    assert_eq!(
        collector.defs,
        vec![1, 2],
        "visit_program must report the alias, then its type parameter"
    );
}

#[test]
fn visit_program_mut_reports_type_alias_type_params() {
    let mut program = program_with_generic_type_alias();
    let mut collector = DefCollector::default();
    collector.visit_program_mut(&mut program);
    assert_eq!(
        collector.defs,
        vec![1, 2],
        "visit_program_mut must report the alias, then its type parameter"
    );
}

// Interface method signatures carry their own contract-method DefId; a
// visitor that builds a definition index (the AIR specializer's id scan)
// must see it, same as struct methods reached through `walk_method`.
fn program_with_interface_methods() -> HirModule {
    use crate::nodes::{HirInterface, HirInterfaceMethod, HirItem, HirItemKind};
    let method = |def_id: u32| HirInterfaceMethod {
        def_id: DefId(def_id),
        availability: Vec::new(),
        name: Symbol(def_id),
        params: Vec::new(),
        ret_ty: None,
        err_ty: None,
        posture: radix_syntax::CallablePosture::SyncFinite,
        span: Span::default(),
    };
    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(1),
            kind: HirItemKind::Interface(HirInterface {
                name: Symbol(0),
                type_params: Vec::new(),
                methods: vec![method(2), method(3)],
            }),
            span: Span::default(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn visit_program_reports_interface_method_defs() {
    let program = program_with_interface_methods();
    let mut collector = DefCollector::default();
    collector.visit_program(&program);
    assert_eq!(
        collector.defs,
        vec![1, 2, 3],
        "visit_program must report the interface, then each method's contract DefId"
    );
}

#[test]
fn visit_program_mut_reports_interface_method_defs() {
    let mut program = program_with_interface_methods();
    let mut collector = DefCollector::default();
    collector.visit_program_mut(&mut program);
    assert_eq!(
        collector.defs,
        vec![1, 2, 3],
        "visit_program_mut must report the interface, then each method's contract DefId"
    );
}

// Behaviour-level traversal tests for the less frequently exercised control,
// binding, and aggregate arms. Events retain their interleaving so order
// differences between expression children and definitions are observable.
#[derive(Debug, PartialEq, Eq)]
enum WalkEvent {
    Expr(u32),
    Def(u32),
}

#[derive(Default)]
struct EventCollector(Vec<WalkEvent>);
impl HirVisitor for EventCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        self.0.push(WalkEvent::Expr(expr.id.0));
        walk_expr(self, expr);
    }

    fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
        self.0.push(WalkEvent::Def(def_id.0));
    }
}

fn expr(id: u32, kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind,
        ty: None,
        span: Span::default(),
    }
}

fn tail(id: u32) -> HirBlock {
    HirBlock::simple(
        Vec::new(),
        Some(Box::new(expr(id, HirExpressionKind::Vacua))),
        Span::default(),
    )
}

#[test]
fn visitor_walks_iteration_reduction_match_and_handler_in_semantic_order() {
    use crate::nodes::{HirCasuArm, HirIteraMode, HirPattern, HirReductionOp};

    let tree = expr(
        0,
        HirExpressionKind::Tuple(
            vec![
                expr(
                    1,
                    HirExpressionKind::IteraRange {
                        ranges: vec![
                            expr(2, HirExpressionKind::Vacua),
                            expr(3, HirExpressionKind::Vacua),
                        ],
                        bindings: vec![(DefId(10), Symbol(10)), (DefId(11), Symbol(11))],
                        body: tail(4),
                        mutable: false,
                    },
                ),
                expr(
                    5,
                    HirExpressionKind::Reduction {
                        op: HirReductionOp::Sum,
                        source: Box::new(expr(6, HirExpressionKind::Vacua)),
                        coordinates: vec![(DefId(12), Symbol(12))],
                        group: Some(Box::new(expr(7, HirExpressionKind::Vacua))),
                        filum: Some((DefId(13), Symbol(13))),
                        binding: Some((DefId(14), Symbol(14))),
                        body: Some(tail(8)),
                        identity: Some(Box::new(expr(9, HirExpressionKind::Vacua))),
                        mutable: false,
                    },
                ),
                expr(
                    10,
                    HirExpressionKind::Discerne {
                        scrutinees: vec![expr(11, HirExpressionKind::Vacua)],
                        arms: vec![
                            HirCasuArm {
                                patterns: vec![HirPattern::Alias(
                                    DefId(15),
                                    Symbol(15),
                                    Box::new(HirPattern::Binding(DefId(16), Symbol(16))),
                                )],
                                body: expr(12, HirExpressionKind::Vacua),
                                span: Span::default(),
                            },
                            HirCasuArm {
                                patterns: vec![HirPattern::TypeTest {
                                    ty: radix_types::TypeId(0),
                                    candidate: None,
                                    binding: Some((DefId(17), Symbol(17))),
                                }],
                                body: expr(13, HirExpressionKind::Vacua),
                                span: Span::default(),
                            },
                        ],
                        exhaustive: true,
                    },
                ),
                expr(
                    14,
                    HirExpressionKind::Si {
                        cond: Box::new(expr(15, HirExpressionKind::Vacua)),
                        then_block: tail(16),
                        then_catch: Some(Box::new(HirCape {
                            binding_def_id: DefId(18),
                            binding_name: Symbol(18),
                            binding_ty: None,
                            body: tail(17),
                            span: Span::default(),
                        })),
                        else_block: Some(tail(18)),
                    },
                ),
                expr(
                    19,
                    HirExpressionKind::Handled {
                        body: tail(20),
                        catch: Box::new(HirCape {
                            binding_def_id: DefId(19),
                            binding_name: Symbol(19),
                            binding_ty: None,
                            body: tail(21),
                            span: Span::default(),
                        }),
                    },
                ),
                expr(
                    22,
                    HirExpressionKind::Itera {
                        mode: HirIteraMode::Ex,
                        coordinates: vec![(DefId(20), Symbol(20))],
                        filum: Some((DefId(21), Symbol(21))),
                        binding: DefId(22),
                        binding_name: Symbol(22),
                        iter: Box::new(expr(23, HirExpressionKind::Vacua)),
                        body: tail(24),
                        mutable: false,
                    },
                ),
                expr(
                    25,
                    HirExpressionKind::IteraLockstep {
                        sources: vec![
                            expr(26, HirExpressionKind::Vacua),
                            expr(27, HirExpressionKind::Vacua),
                        ],
                        coordinates: vec![(DefId(23), Symbol(23))],
                        bindings: vec![(DefId(24), Symbol(24)), (DefId(25), Symbol(25))],
                        body: tail(28),
                        mutable: false,
                    },
                ),
                expr(
                    29,
                    HirExpressionKind::Scan {
                        op: HirReductionOp::Count,
                        total: crate::nodes::HirScanTotal::Inclusive,
                        source: Box::new(expr(30, HirExpressionKind::Vacua)),
                        coordinates: vec![(DefId(26), Symbol(26))],
                        group: Some(Box::new(expr(31, HirExpressionKind::Vacua))),
                        group_domain: None,
                        order: crate::nodes::HirSortDirection::Ascending,
                        filum: (DefId(27), Symbol(27)),
                        binding: (DefId(28), Symbol(28)),
                        body: tail(32),
                    },
                ),
                expr(
                    33,
                    HirExpressionKind::Sort {
                        direction: crate::nodes::HirSortDirection::Descending,
                        source_mode: crate::nodes::HirSortSource::Values,
                        source: Box::new(expr(34, HirExpressionKind::Vacua)),
                        binding: Some((DefId(29), Symbol(29))),
                        body: Some(tail(35)),
                        mutable: false,
                    },
                ),
            ],
            None,
        ),
    );

    let mut events = EventCollector::default();
    events.visit_expr(&tree);
    assert_eq!(
        events.0,
        vec![
            WalkEvent::Expr(0),
            WalkEvent::Expr(1),
            WalkEvent::Expr(2),
            WalkEvent::Expr(3),
            WalkEvent::Def(10),
            WalkEvent::Def(11),
            WalkEvent::Expr(4),
            WalkEvent::Expr(5),
            WalkEvent::Def(12),
            WalkEvent::Def(13),
            WalkEvent::Def(14),
            WalkEvent::Expr(6),
            WalkEvent::Expr(7),
            WalkEvent::Expr(8),
            WalkEvent::Expr(9),
            WalkEvent::Expr(10),
            WalkEvent::Expr(11),
            WalkEvent::Def(15),
            WalkEvent::Def(16),
            WalkEvent::Expr(12),
            WalkEvent::Def(17),
            WalkEvent::Expr(13),
            WalkEvent::Expr(14),
            WalkEvent::Expr(15),
            WalkEvent::Expr(16),
            WalkEvent::Def(18),
            WalkEvent::Expr(17),
            WalkEvent::Expr(18),
            WalkEvent::Expr(19),
            WalkEvent::Expr(20),
            WalkEvent::Def(19),
            WalkEvent::Expr(21),
            WalkEvent::Expr(22),
            WalkEvent::Def(20),
            WalkEvent::Def(21),
            WalkEvent::Def(22),
            WalkEvent::Expr(23),
            WalkEvent::Expr(24),
            WalkEvent::Expr(25),
            WalkEvent::Def(23),
            WalkEvent::Def(24),
            WalkEvent::Def(25),
            WalkEvent::Expr(26),
            WalkEvent::Expr(27),
            WalkEvent::Expr(28),
            WalkEvent::Expr(29),
            WalkEvent::Def(26),
            WalkEvent::Def(27),
            WalkEvent::Def(28),
            WalkEvent::Expr(30),
            WalkEvent::Expr(31),
            WalkEvent::Expr(32),
            WalkEvent::Expr(33),
            WalkEvent::Expr(34),
            WalkEvent::Def(29),
            WalkEvent::Expr(35),
        ],
    );
}

#[test]
fn visitor_walks_statement_children_and_object_keys_in_order() {
    use crate::nodes::{
        HirCustodi, HirCustodiClause, HirIncDec, HirIncDecOp, HirLocal, HirObjectField,
        HirObjectKey,
    };

    let block = HirBlock::simple(
        vec![
            HirStatement {
                id: HirId(0),
                kind: HirStatementKind::Local(HirLocal {
                    def_id: DefId(1),
                    name: Symbol(1),
                    ty: None,
                    init: Some(expr(1, HirExpressionKind::Vacua)),
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: Span::default(),
            },
            HirStatement {
                id: HirId(2),
                kind: HirStatementKind::Redde(Some(expr(2, HirExpressionKind::Vacua))),
                span: Span::default(),
            },
            HirStatement {
                id: HirId(3),
                kind: HirStatementKind::Rumpe,
                span: Span::default(),
            },
            HirStatement {
                id: HirId(4),
                kind: HirStatementKind::Perge,
                span: Span::default(),
            },
            HirStatement {
                id: HirId(5),
                kind: HirStatementKind::Tacet,
                span: Span::default(),
            },
            HirStatement {
                id: HirId(6),
                kind: HirStatementKind::IncDec(HirIncDec {
                    target: expr(3, HirExpressionKind::Vacua),
                    op: HirIncDecOp::Inc,
                }),
                span: Span::default(),
            },
            HirStatement {
                id: HirId(7),
                kind: HirStatementKind::Custodi(HirCustodi {
                    clauses: vec![HirCustodiClause {
                        cond: expr(4, HirExpressionKind::Vacua),
                        body: tail(5),
                        span: Span::default(),
                    }],
                }),
                span: Span::default(),
            },
        ],
        Some(Box::new(expr(
            6,
            HirExpressionKind::Verte {
                source: Box::new(expr(7, HirExpressionKind::Vacua)),
                target: radix_types::TypeId(0),
                entries: Some(vec![
                    HirObjectField {
                        key: HirObjectKey::Computed(expr(8, HirExpressionKind::Vacua)),
                        value: Some(expr(9, HirExpressionKind::Vacua)),
                    },
                    HirObjectField {
                        key: HirObjectKey::Spread(expr(10, HirExpressionKind::Vacua)),
                        value: None,
                    },
                ]),
            },
        ))),
        Span::default(),
    );

    let mut events = EventCollector::default();
    events.visit_block(&block);
    assert_eq!(
        events.0,
        vec![
            WalkEvent::Def(1),
            WalkEvent::Expr(1),
            WalkEvent::Expr(2),
            WalkEvent::Expr(3),
            WalkEvent::Expr(4),
            WalkEvent::Expr(5),
            WalkEvent::Expr(6),
            WalkEvent::Expr(7),
            WalkEvent::Expr(8),
            WalkEvent::Expr(9),
            WalkEvent::Expr(10),
        ]
    );
}

#[test]
fn mutable_walker_edits_each_nested_aggregate_expression() {
    use crate::nodes::{HirArrayElement, HirObjectField, HirObjectKey};

    let mut tree = expr(
        0,
        HirExpressionKind::Tuple(
            vec![
                expr(
                    1,
                    HirExpressionKind::Array(vec![
                        HirArrayElement::Expr(expr(
                            2,
                            HirExpressionKind::Literal(HirLiteral::Int(2)),
                        )),
                        HirArrayElement::Spread(expr(
                            3,
                            HirExpressionKind::Literal(HirLiteral::Int(3)),
                        )),
                        HirArrayElement::Infer(Span::default()),
                    ]),
                ),
                expr(
                    4,
                    HirExpressionKind::Verte {
                        source: Box::new(expr(5, HirExpressionKind::Literal(HirLiteral::Int(5)))),
                        target: radix_types::TypeId(0),
                        entries: Some(vec![HirObjectField {
                            key: HirObjectKey::Computed(expr(
                                6,
                                HirExpressionKind::Literal(HirLiteral::Int(6)),
                            )),
                            value: Some(expr(7, HirExpressionKind::Literal(HirLiteral::Int(7)))),
                        }]),
                    },
                ),
            ],
            None,
        ),
    );

    let mut marker = ExprMarker;
    marker.visit_expr_mut(&mut tree);
    let mut collector = ExprCollector { seen: Vec::new() };
    collector.visit_expr(&tree);
    assert_eq!(collector.seen, vec![99, 99, 99, 99, 99]);
}

#[test]
fn optional_and_non_null_chain_arguments_are_walked_in_order() {
    use crate::nodes::{HirArgMarker, HirCallArg, HirNonNullKind, HirOptionalChainKind};
    let arg = |id| HirCallArg {
        name: None,
        spread: false,
        marker: Some(HirArgMarker::Copy),
        expr: expr(id, HirExpressionKind::Vacua),
        span: Span::default(),
    };
    let chain = expr(
        0,
        HirExpressionKind::Tuple(
            vec![
                expr(
                    1,
                    HirExpressionKind::OptionalChain(
                        Box::new(expr(2, HirExpressionKind::Vacua)),
                        HirOptionalChainKind::Call(vec![arg(3), arg(4)]),
                    ),
                ),
                expr(
                    5,
                    HirExpressionKind::NonNull(
                        Box::new(expr(6, HirExpressionKind::Vacua)),
                        HirNonNullKind::Call(vec![arg(7)]),
                    ),
                ),
            ],
            None,
        ),
    );
    let mut events = EventCollector::default();
    events.visit_expr(&chain);
    assert_eq!(
        events.0,
        vec![
            WalkEvent::Expr(0),
            WalkEvent::Expr(1),
            WalkEvent::Expr(2),
            WalkEvent::Expr(3),
            WalkEvent::Expr(4),
            WalkEvent::Expr(5),
            WalkEvent::Expr(6),
            WalkEvent::Expr(7),
        ]
    );
}

#[test]
fn mutable_statement_walk_rewrites_return_and_guard_children() {
    let mut block = HirBlock::simple(
        vec![
            HirStatement {
                id: HirId(0),
                kind: HirStatementKind::Redde(Some(expr(
                    1,
                    HirExpressionKind::Literal(HirLiteral::Int(1)),
                ))),
                span: Span::default(),
            },
            HirStatement {
                id: HirId(2),
                kind: HirStatementKind::Custodi(crate::nodes::HirCustodi {
                    clauses: vec![crate::nodes::HirCustodiClause {
                        cond: expr(3, HirExpressionKind::Literal(HirLiteral::Int(3))),
                        body: tail(4),
                        span: Span::default(),
                    }],
                }),
                span: Span::default(),
            },
        ],
        None,
        Span::default(),
    );
    let mut marker = ExprMarker;
    marker.visit_block_mut(&mut block);
    let mut collector = ExprCollector { seen: Vec::new() };
    collector.visit_block(&block);
    // The return value and the guard condition are rewritten; the guard body's
    // tail is `Vacua`, which the marker leaves alone and the collector ignores.
    assert_eq!(collector.seen, vec![99, 99]);
}
