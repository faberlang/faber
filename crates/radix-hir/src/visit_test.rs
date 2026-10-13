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
