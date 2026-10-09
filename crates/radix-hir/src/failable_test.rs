use super::derive_failable_defs;
use crate::{
    DefId, HirBlock, HirCape, HirExpression, HirExpressionKind, HirFunction, HirId, HirItem,
    HirItemKind, HirLiteral, HirMethod, HirModule, HirReceiver, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span};
use radix_syntax::CallablePosture;
use radix_types::{Type, TypeTable};

fn throw_expr(
    id: u32,
    text: &str,
    interner: &mut Interner,
    span: radix_lexer::Span,
) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Throw(Box::new(HirExpression {
            id: HirId(id + 1),
            kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern(text))),
            ty: None,
            span,
        })),
        ty: None,
        span,
    }
}

fn func(
    name: &str,
    _def_id: DefId,
    body: Option<HirBlock>,
    interner: &mut Interner,
    _span: radix_lexer::Span,
) -> HirFunction {
    HirFunction {
        name: interner.intern(name),
        type_params: Vec::new(),
        params: Vec::new(),
        cli_args: None,
        ret_ty: None,
        err_ty: None,
        body,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        posture: CallablePosture::SyncFinite,
        is_async: false,
        test: None,
    }
}

fn statement(id: u32, expr: HirExpression, span: radix_lexer::Span) -> HirStatement {
    HirStatement {
        id: HirId(id),
        kind: HirStatementKind::Expr(expr),
        span,
    }
}

#[test]
fn arm_attached_cape_contains_then_throw() {
    let mut interner = Interner::new();
    let span = Span::new(0, 0);
    let si = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Si {
            cond: Box::new(HirExpression {
                id: HirId(1),
                kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                ty: None,
                span,
            }),
            then_block: HirBlock::simple(
                vec![statement(
                    5,
                    throw_expr(3, "error", &mut interner, span),
                    span,
                )],
                None,
                span,
            ),
            then_catch: Some(Box::new(HirCape {
                binding_def_id: DefId(3),
                binding_name: interner.intern("err"),
                binding_ty: None,
                body: HirBlock::simple(Vec::new(), None, span),
                span,
            })),
            else_block: None,
        },
        ty: None,
        span,
    };
    let hir = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(1),
            kind: HirItemKind::Function(Box::new(func(
                "classify",
                DefId(1),
                Some(HirBlock::simple(vec![statement(6, si, span)], None, span)),
                &mut interner,
                span,
            ))),
            span,
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let failable = derive_failable_defs(&hir, None);
    assert!(
        !failable.contains(&DefId(1)),
        "then-arm throw is contained by its attached cape"
    );
}

#[test]
fn trap_suppresses_failable_call_edge() {
    use crate::HirLocal;
    let mut interner = Interner::new();
    let span = Span::new(0, 0);
    let call = HirExpression {
        id: HirId(9),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: HirId(8),
                kind: HirExpressionKind::Path(DefId(42)),
                ty: None,
                span,
            }),
            Vec::new(),
            Vec::new(),
            None,
        ),
        ty: None,
        span,
    };
    let local = HirStatement {
        id: HirId(7),
        kind: HirStatementKind::Local(HirLocal {
            def_id: DefId(6),
            name: interner.intern("r"),
            ty: None,
            init: Some(HirExpression {
                id: HirId(5),
                kind: HirExpressionKind::Trap(Box::new(HirBlock::simple(
                    vec![statement(6, call, span)],
                    None,
                    span,
                ))),
                ty: None,
                span,
            }),
            mutable: false,
            runtime_provided: false,
            await_store: None,
            size_decl: false,
            size_static: false,
            is_await: false,
        }),
        span,
    };
    let hir = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(1),
            kind: HirItemKind::Function(Box::new(func(
                "tutum",
                DefId(1),
                Some(HirBlock::simple(vec![local], None, span)),
                &mut interner,
                span,
            ))),
            span,
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let failable = derive_failable_defs(&hir, None);
    assert!(
        !failable.contains(&DefId(1)),
        "a trapped call must not make the enclosing function failable"
    );
}

#[test]
fn method_call_to_failable_method_propagates() {
    let mut interner = Interner::new();
    let span = Span::new(0, 0);
    let mut types = TypeTable::new();

    let struct_def = DefId(10);
    let method_def = DefId(11);
    let caller_def = DefId(12);
    let struct_ty = types.intern(Type::Struct(struct_def));

    let method = HirMethod {
        def_id: method_def,
        func: func(
            "salte",
            method_def,
            Some(HirBlock::simple(
                vec![statement(4, throw_expr(2, "no", &mut interner, span), span)],
                None,
                span,
            )),
            &mut interner,
            span,
        ),
        annotations: Vec::new(),
        receiver: HirReceiver::None,
        span,
    };
    let receiver = HirExpression {
        id: HirId(20),
        kind: HirExpressionKind::Path(DefId(99)),
        ty: Some(struct_ty),
        span,
    };
    let call = HirExpression {
        id: HirId(21),
        kind: HirExpressionKind::MethodCall(
            Box::new(receiver),
            interner.intern("salte"),
            Vec::new(),
            Vec::new(),
            None,
        ),
        ty: None,
        span,
    };
    let hir = HirModule {
        items: vec![
            HirItem {
                annotations: Vec::new(),
                id: HirId(30),
                def_id: struct_def,
                kind: HirItemKind::Struct(crate::HirStruct {
                    name: interner.intern("Ponto"),
                    type_params: Vec::new(),
                    fields: Vec::new(),
                    methods: vec![method],
                    implements: Vec::new(),
                }),
                span,
            },
            HirItem {
                annotations: Vec::new(),
                id: HirId(31),
                def_id: caller_def,
                kind: HirItemKind::Function(Box::new(func(
                    "caller",
                    caller_def,
                    Some(HirBlock::simple(
                        vec![statement(22, call, span)],
                        None,
                        span,
                    )),
                    &mut interner,
                    span,
                ))),
                span,
            },
        ],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let failable = derive_failable_defs(&hir, Some(&types));
    assert!(
        failable.contains(&method_def),
        "the throwing method is failable"
    );
    assert!(
        failable.contains(&caller_def),
        "F-034: a method call to a failable method marks the caller failable"
    );

    // Without a type table there is no receiver resolution, so the edge
    // cannot form; the method itself stays failable by direct evidence.
    let failable = derive_failable_defs(&hir, None);
    assert!(failable.contains(&method_def));
    assert!(!failable.contains(&caller_def));
}
