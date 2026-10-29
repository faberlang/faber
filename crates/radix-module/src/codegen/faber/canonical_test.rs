//! Canonical Faber re-emission tests via `codegen::faber`.
//!
//! These tests build HIR programs by hand and assert that `FaberCodegen`
//! produces grammar-valid canonical Faber source. They lock the round-trip
//! contract: every construct that the compiler can lower to HIR must re-emit
//! to source that the parser would accept.

use super::FaberCodegen;
use super::LocaleKeywordBridge;
use crate::hir::DefId;
use crate::hir::{
    HirAnnotation, HirArrayElement, HirBinOp, HirBlock, HirCallArg, HirCasuArm, HirEnum, HirExpression,
    HirExpressionKind, HirField, HirFunction, HirItem, HirItemKind, HirLiteral, HirMethod, HirModule, HirNonNullKind,
    HirObjectField, HirObjectKey, HirOptionalChainKind, HirParam, HirParamMode, HirPattern, HirReceiver, HirStatement,
    HirStatementKind, HirStruct, HirTestMetadata, HirTestModifier, HirUnOp, HirVariant, HirVariantField,
};
use crate::lexer::Interner;
use crate::lexer::Span;
use crate::semantic::InferVar;
use crate::semantic::{Primitive, Type, TypeTable};
use radix_types::USER_DEF_ID_BASE;

fn span() -> Span {
    Span::default()
}

fn generate_faber(program: &HirModule, types: &TypeTable, interner: &Interner) -> String {
    let latin = crate::locale::latin_locale_pack();
    let surface = LocaleKeywordBridge(crate::locale::KeywordSurface::new(&latin));
    FaberCodegen::new(types, interner)
        .generate(program, &super::empty_presentation(), &surface)
        .expect("codegen")
        .code
}

#[test]
fn emits_basic_function_and_entry() {
    let mut interner = Interner::new();
    let name_greet = interner.intern("greet");
    let name_param = interner.intern("name");
    let name_text = interner.intern("salve");

    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);

    let function = HirFunction {
        cli_args: None,
        name: name_greet,
        type_params: Vec::new(),
        params: vec![HirParam {
            def_id: DefId(1),
            name: name_param,
            ty: textus,
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: span(),
        }],
        ret_ty: Some(textus),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(2),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: crate::hir::HirId(3),
                    kind: HirExpressionKind::Literal(HirLiteral::String(name_text)),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,

        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: crate::hir::HirId(0),
            def_id: DefId(USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: Some(HirBlock { statements: Vec::new(), expr: None, span: span(), ..Default::default() }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("functio greet"));
    assert!(code.contains("incipit"));
    assert!(code.contains("redde \"salve\""));
}

#[test]
fn emits_alternate_exit_signature() {
    let mut interner = Interner::new();
    let name = interner.intern("divide");

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: crate::hir::HirId(0),
            def_id: DefId(USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name,
                type_params: Vec::new(),
                params: Vec::new(),
                ret_ty: Some(numerus),
                err_ty: Some(textus),
                body: None,
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,

                test: None,
            })),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("functio divide() → numerus ⇥ textus"));
}

#[test]
fn emits_post_name_declaration_markers_for_params_and_fields() {
    let mut interner = Interner::new();
    let configure = interner.intern("configure");
    let profile = interner.intern("Profile");
    let name = interner.intern("name");
    let default_name = interner.intern("Marcus");

    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);

    let optional_param = HirParam {
        def_id: DefId(1),
        name,
        ty: textus,
        mode: HirParamMode::Owned,
        optional: true,
        sponte: true,
        default: Some(HirExpression {
            id: crate::hir::HirId(1),
            kind: HirExpressionKind::Literal(HirLiteral::String(default_name)),
            ty: Some(textus),
            span: span(),
        }),
        span: span(),
    };

    let program = HirModule {
        items: vec![
            HirItem {
                annotations: Vec::new(),
                id: crate::hir::HirId(2),
                def_id: DefId(USER_DEF_ID_BASE + 2),
                kind: HirItemKind::Function(Box::new(HirFunction {
                    cli_args: None,
                    name: configure,
                    type_params: Vec::new(),
                    params: vec![optional_param],
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
                })),
                span: span(),
            },
            HirItem {
                annotations: Vec::new(),
                id: crate::hir::HirId(3),
                def_id: DefId(3),
                kind: HirItemKind::Struct(HirStruct {
                    name: profile,
                    type_params: Vec::new(),
                    fields: vec![HirField {
                        def_id: DefId(4),
                        name,
                        ty: textus,
                        modifier: None,
                        sponte: true,
                        annotations: Vec::new(),
                        init: None,
                        span: span(),
                    }],
                    methods: Vec::new(),
                    implements: Vec::new(),
                }),
                span: span(),
            },
        ],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("functio configure(textus name sponte vel \"Marcus\")"));
    assert!(code.contains("textus name sponte"));
    assert!(!code.contains("si textus name"));
}

#[test]
fn renders_unresolved_infer_as_marker() {
    let mut interner = Interner::new();
    let name = interner.intern("x");
    let mut types = TypeTable::new();
    let unresolved = types.intern(Type::Infer(InferVar(99)));

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(1),
                kind: HirStatementKind::Local(crate::hir::HirLocal {
                    def_id: DefId(1),
                    name,
                    ty: Some(unresolved),
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("sit _ x"));
}

#[test]
fn renders_option_types_as_nullable_union() {
    let mut interner = Interner::new();
    let name = interner.intern("maybe");
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let option_textus = types.option(textus);
    let nested_option = types.option(option_textus);

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(1),
                kind: HirStatementKind::Local(crate::hir::HirLocal {
                    def_id: DefId(1),
                    name,
                    ty: Some(nested_option),
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("sit textus ∪ nihil maybe"));
    assert!(!code.contains("si textus"));
}

#[test]
fn rejects_hir_error_nodes_in_direct_faber_codegen() {
    let interner = Interner::new();
    let types = TypeTable::new();
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(1),
                kind: HirStatementKind::Expr(HirExpression {
                    id: crate::hir::HirId(2),
                    kind: HirExpressionKind::Error,
                    ty: None,
                    span: span(),
                }),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let mut r#gen = FaberCodegen::new(&types, &interner);
    let latin = crate::locale::latin_locale_pack();
    let surface = LocaleKeywordBridge(crate::locale::KeywordSurface::new(&latin));
    let Err(error) = r#gen.generate(&program, &super::empty_presentation(), &surface) else {
        panic!("expected faber codegen error");
    };
    assert!(
        error
            .args
            .iter()
            .any(|arg| { arg.name == "issue" && arg.value == "hir_error_expression_reached_codegen" })
    );
}

#[test]
fn emits_parameter_references_with_original_names() {
    let mut interner = Interner::new();
    let name_identity = interner.intern("identity");
    let name_x = interner.intern("x");

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let function = HirFunction {
        cli_args: None,
        name: name_identity,
        type_params: Vec::new(),
        params: vec![HirParam {
            def_id: DefId(100),
            name: name_x,
            ty: numerus,
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: span(),
        }],
        ret_ty: Some(numerus),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(2),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: crate::hir::HirId(3),
                    kind: HirExpressionKind::Path(DefId(100)),
                    ty: Some(numerus),
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,

        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: crate::hir::HirId(0),
            def_id: DefId(USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("redde x"));
    assert!(!code.contains("def_100"));
}

#[test]
fn emits_si_sin_secus_chain_with_ergo_redde() {
    let mut interner = Interner::new();
    let name_grade = interner.intern("grade");
    let text_a = interner.intern("A");
    let text_b = interner.intern("B");
    let text_f = interner.intern("F");

    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let bivalens = types.primitive(Primitive::Bivalens);

    let expr = HirExpression {
        id: crate::hir::HirId(10),
        kind: HirExpressionKind::Si {
            cond: Box::new(HirExpression {
                id: crate::hir::HirId(11),
                kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                ty: Some(bivalens),
                span: span(),
            }),
            then_block: HirBlock {
                statements: vec![HirStatement {
                    id: crate::hir::HirId(12),
                    kind: HirStatementKind::Redde(Some(HirExpression {
                        id: crate::hir::HirId(13),
                        kind: HirExpressionKind::Literal(HirLiteral::String(text_a)),
                        ty: Some(textus),
                        span: span(),
                    })),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            },
            then_catch: None,
            else_block: Some(HirBlock {
                statements: Vec::new(),
                expr: Some(Box::new(HirExpression {
                    id: crate::hir::HirId(14),
                    kind: HirExpressionKind::Si {
                        cond: Box::new(HirExpression {
                            id: crate::hir::HirId(15),
                            kind: HirExpressionKind::Literal(HirLiteral::Bool(false)),
                            ty: Some(bivalens),
                            span: span(),
                        }),
                        then_block: HirBlock {
                            statements: vec![HirStatement {
                                id: crate::hir::HirId(16),
                                kind: HirStatementKind::Redde(Some(HirExpression {
                                    id: crate::hir::HirId(17),
                                    kind: HirExpressionKind::Literal(HirLiteral::String(text_b)),
                                    ty: Some(textus),
                                    span: span(),
                                })),
                                span: span(),
                            }],
                            expr: None,
                            span: span(),
                            ..Default::default()
                        },
                        then_catch: None,
                        else_block: Some(HirBlock {
                            statements: vec![HirStatement {
                                id: crate::hir::HirId(18),
                                kind: HirStatementKind::Redde(Some(HirExpression {
                                    id: crate::hir::HirId(19),
                                    kind: HirExpressionKind::Literal(HirLiteral::String(text_f)),
                                    ty: Some(textus),
                                    span: span(),
                                })),
                                span: span(),
                            }],
                            expr: None,
                            span: span(),
                            ..Default::default()
                        }),
                    },
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
                ..Default::default()
            }),
        },
        ty: Some(textus),
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: name_grade,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(textus),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(20),
                kind: HirStatementKind::Expr(expr),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,

        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: crate::hir::HirId(0),
            def_id: DefId(USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    // `redde` branches have no statement slot in the compact `sic … secus …`
    // expression form, so the chain stays in the statement `ergo redde` form
    // (locked by the hir-faber roundtrip contract; SEM033 on re-check if a
    // bare-expression emit dropped the returns).
    assert!(code.contains("si verum ergo redde \"A\""));
    assert!(code.contains("sin falsum ergo redde \"B\""));
    assert!(code.contains("secus ergo redde \"F\""));
    assert!(!code.contains("aliter"));
    assert!(!code.contains("sic"), "redde branches must keep the ergo chain:\n{code}");
}

#[test]
fn canonical_si_continuations_start_on_new_lines() {
    let mut interner = Interner::new();
    let name_check = interner.intern("check");

    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let bivalens = types.primitive(Primitive::Bivalens);

    let bool_expr = |id, value| HirExpression {
        id: crate::hir::HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Bool(value)),
        ty: Some(bivalens),
        span: span(),
    };
    let tacet_block = |id| HirBlock {
        statements: vec![HirStatement { id: crate::hir::HirId(id), kind: HirStatementKind::Tacet, span: span() }],
        expr: None,
        span: span(),
        ..Default::default()
    };

    let expr = HirExpression {
        id: crate::hir::HirId(10),
        kind: HirExpressionKind::Si {
            cond: Box::new(bool_expr(11, true)),
            then_block: tacet_block(12),
            then_catch: None,
            else_block: Some(HirBlock {
                statements: Vec::new(),
                expr: Some(Box::new(HirExpression {
                    id: crate::hir::HirId(13),
                    kind: HirExpressionKind::Si {
                        cond: Box::new(bool_expr(14, false)),
                        then_block: tacet_block(15),
                        then_catch: None,
                        else_block: Some(tacet_block(16)),
                    },
                    ty: Some(vacuum),
                    span: span(),
                })),
                span: span(),
                ..Default::default()
            }),
        },
        ty: Some(vacuum),
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: name_check,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(vacuum),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(17),
                kind: HirStatementKind::Expr(expr),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,

        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: crate::hir::HirId(0),
            def_id: DefId(USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(
        code.contains("    }\n    sin falsum {\n"),
        "sin must start its own line:\n{code}"
    );
    assert!(code.contains("    }\n    secus {\n"), "secus must start its own line:\n{code}");
    assert!(!code.contains("} sin "), "sin must not attach to }}:\n{code}");
    assert!(!code.contains("} secus "), "secus must not attach to }}:\n{code}");
}

#[test]
fn emits_canonical_operator_spellings_and_precedence() {
    let mut interner = Interner::new();
    let total = interner.intern("total");

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let bivalens = types.primitive(Primitive::Bivalens);

    let int_expr = |id, value| HirExpression {
        id: crate::hir::HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: Some(numerus),
        span: span(),
    };

    let comparison = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Binary(
            HirBinOp::And,
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Binary(HirBinOp::Eq, Box::new(int_expr(3, 1)), Box::new(int_expr(4, 1))),
                ty: Some(bivalens),
                span: span(),
            }),
            Box::new(HirExpression {
                id: crate::hir::HirId(5),
                kind: HirExpressionKind::Binary(
                    HirBinOp::LtEq,
                    Box::new(HirExpression {
                        id: crate::hir::HirId(6),
                        kind: HirExpressionKind::Binary(
                            HirBinOp::Add,
                            Box::new(int_expr(7, 2)),
                            Box::new(int_expr(8, 3)),
                        ),
                        ty: Some(numerus),
                        span: span(),
                    }),
                    Box::new(HirExpression {
                        id: crate::hir::HirId(9),
                        kind: HirExpressionKind::Binary(
                            HirBinOp::Mul,
                            Box::new(int_expr(10, 4)),
                            Box::new(int_expr(11, 5)),
                        ),
                        ty: Some(numerus),
                        span: span(),
                    }),
                ),
                ty: Some(bivalens),
                span: span(),
            }),
        ),
        ty: Some(bivalens),
        span: span(),
    };

    let not_comparison = HirExpression {
        id: crate::hir::HirId(12),
        kind: HirExpressionKind::Unary(HirUnOp::Not, Box::new(comparison)),
        ty: Some(bivalens),
        span: span(),
    };

    let inc_stmt = HirStatement {
        id: crate::hir::HirId(13),
        kind: HirStatementKind::IncDec(crate::hir::HirIncDec {
            target: HirExpression {
                id: crate::hir::HirId(14),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: span(),
            },
            op: crate::hir::HirIncDecOp::Inc,
        }),
        span: span(),
    };

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: crate::hir::HirId(16),
                    kind: HirStatementKind::Local(crate::hir::HirLocal {
                        def_id: DefId(1),
                        name: total,
                        ty: Some(numerus),
                        init: Some(int_expr(17, 0)),
                        mutable: true,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span: span(),
                },
                HirStatement { id: crate::hir::HirId(18), kind: HirStatementKind::Expr(not_comparison), span: span() },
                inc_stmt,
            ],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("non (1 ≡ 1 et 2 + 3 ≤ 4 * 5)"));
    assert!(code.contains("total ↑"));
}

#[test]
fn optional_chain_receiver_keeps_required_variant_qualifier() {
    let mut interner = Interner::new();
    let first_name = interner.intern("First");
    let second_name = interner.intern("Second");
    let variant_name = interner.intern("Shared");

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let make_enum = |item_id, enum_def, enum_name, variant_def| HirItem {
        annotations: Vec::new(),
        id: crate::hir::HirId(item_id),
        def_id: DefId(enum_def),
        kind: HirItemKind::Enum(HirEnum {
            name: enum_name,
            type_params: Vec::new(),
            commune_fields: Vec::new(),
            variants: vec![HirVariant {
                def_id: DefId(variant_def),
                name: variant_name,
                fields: Vec::new(),
                span: span(),
            }],
        }),
        span: span(),
    };
    let optional_call = HirExpression {
        id: crate::hir::HirId(10),
        kind: HirExpressionKind::OptionalChain(
            Box::new(HirExpression {
                id: crate::hir::HirId(11),
                kind: HirExpressionKind::Path(DefId(2)),
                ty: Some(numerus),
                span: span(),
            }),
            HirOptionalChainKind::Call(vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: crate::hir::HirId(12),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                    ty: Some(numerus),
                    span: span(),
                },
                span: span(),
            }]),
        ),
        ty: Some(numerus),
        span: span(),
    };
    let program = HirModule {
        items: vec![make_enum(0, 100, first_name, 2), make_enum(1, 101, second_name, 3)],
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(13),
                kind: HirStatementKind::Expr(optional_call),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(
        code.contains("First.Shared?(1)"),
        "optional chain must qualify an ambiguous local variant path:\n{code}"
    );
}

#[test]
fn emits_shared_argument_spelling_for_optional_and_non_null_calls() {
    let mut interner = Interner::new();
    let callable = interner.intern("callable");

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let arg = |id, value, spread| HirCallArg {
        name: None,
        spread,
        marker: None,
        expr: HirExpression {
            id: crate::hir::HirId(id),
            kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
            ty: Some(numerus),
            span: span(),
        },
        span: span(),
    };

    let optional_call = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::OptionalChain(
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: span(),
            }),
            HirOptionalChainKind::Call(vec![arg(3, 1, false), arg(4, 2, true)]),
        ),
        ty: Some(numerus),
        span: span(),
    };

    let non_null_call = HirExpression {
        id: crate::hir::HirId(5),
        kind: HirExpressionKind::NonNull(
            Box::new(HirExpression {
                id: crate::hir::HirId(6),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: span(),
            }),
            HirNonNullKind::Call(vec![arg(7, 3, false), arg(8, 4, true)]),
        ),
        ty: Some(numerus),
        span: span(),
    };

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: crate::hir::HirId(9),
                    kind: HirStatementKind::Local(crate::hir::HirLocal {
                        def_id: DefId(1),
                        name: callable,
                        ty: Some(numerus),
                        init: None,
                        mutable: false,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span: span(),
                },
                HirStatement { id: crate::hir::HirId(10), kind: HirStatementKind::Expr(optional_call), span: span() },
                HirStatement { id: crate::hir::HirId(11), kind: HirStatementKind::Expr(non_null_call), span: span() },
            ],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("callable?(1, sparge 2)"));
    assert!(code.contains("callable!(3, sparge 4)"));
}

#[test]
fn emits_local_and_control_statement_forms() {
    let mut interner = Interner::new();
    let counter = interner.intern("counter");

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: crate::hir::HirId(1),
                    kind: HirStatementKind::Local(crate::hir::HirLocal {
                        def_id: DefId(1),
                        name: counter,
                        ty: Some(numerus),
                        init: Some(HirExpression {
                            id: crate::hir::HirId(2),
                            kind: HirExpressionKind::Literal(HirLiteral::Int(0)),
                            ty: Some(numerus),
                            span: span(),
                        }),
                        mutable: true,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span: span(),
                },
                HirStatement { id: crate::hir::HirId(3), kind: HirStatementKind::Perge, span: span() },
                HirStatement { id: crate::hir::HirId(4), kind: HirStatementKind::Rumpe, span: span() },
                HirStatement { id: crate::hir::HirId(5), kind: HirStatementKind::Tacet, span: span() },
                HirStatement { id: crate::hir::HirId(6), kind: HirStatementKind::Redde(None), span: span() },
            ],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("varia numerus counter ← 0"));
    assert!(code.contains("perge"));
    assert!(code.contains("rumpe"));
    assert!(code.contains("tacet"));
    assert!(code.contains("redde"));
}

#[test]
fn emits_synthetic_proba_functions_as_proba_cases() {
    let mut interner = Interner::new();
    let name = interner.intern("one plus one equals two");
    let blocked = interner.intern("blocked by maintenance");
    let suite = interner.intern("arithmetic suite");

    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);

    let function = HirFunction {
        cli_args: None,
        name,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(vacuum),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(1),
                kind: HirStatementKind::Expr(HirExpression {
                    id: crate::hir::HirId(2),
                    kind: HirExpressionKind::Adfirma(
                        Box::new(HirExpression {
                            id: crate::hir::HirId(3),
                            kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                            ty: Some(types.primitive(Primitive::Bivalens)),
                            span: span(),
                        }),
                        None,
                    ),
                    ty: Some(vacuum),
                    span: span(),
                }),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,

        test: Some(HirTestMetadata {
            name,
            suite_path: vec![suite],
            modifiers: vec![HirTestModifier::Omitte(blocked)],
            span: span(),
        }),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: crate::hir::HirId(0),
            def_id: DefId(USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("proba \"one plus one equals two\" omitte \"blocked by maintenance\""));
    assert!(!code.contains("functio one plus one equals two"));
}

#[test]
fn discerne_cases_do_not_emit_nested_blocks() {
    let mut interner = Interner::new();
    let ok = interner.intern("OK");

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);

    let arm_body = HirExpression {
        id: crate::hir::HirId(3),
        kind: HirExpressionKind::Block(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(4),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: crate::hir::HirId(5),
                    kind: HirExpressionKind::Literal(HirLiteral::String(ok)),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        ty: Some(textus),
        span: span(),
    };

    let discerne = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Literal(HirLiteral::Int(200)),
                ty: Some(numerus),
                span: span(),
            }],
            arms: vec![HirCasuArm {
                patterns: vec![HirPattern::Literal(HirLiteral::Int(200))],
                body: arm_body,
                span: span(),
            }],
            exhaustive: false,
        },
        ty: Some(textus),
        span: span(),
    };

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(6),
                kind: HirStatementKind::Expr(discerne),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("casu 200 {\n            redde \"OK\""));
    assert!(!code.contains("casu 200 {\n            {"));
}

#[test]
fn discerne_patterns_preserve_bindings_aliases_and_variants() {
    let mut interner = Interner::new();
    let event = interner.intern("Event");
    let click = interner.intern("Click");
    let payload = interner.intern("payload");
    let alias = interner.intern("matched");
    let ok = interner.intern("ok");

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);

    let enum_item = HirItem {
        annotations: Vec::new(),
        id: crate::hir::HirId(0),
        def_id: DefId(USER_DEF_ID_BASE),
        kind: HirItemKind::Enum(HirEnum {
            name: event,
            type_params: Vec::new(),
            commune_fields: Vec::new(),
            variants: vec![HirVariant {
                def_id: DefId(2),
                name: click,
                fields: vec![HirVariantField { name: payload, ty: numerus, span: span() }],
                span: span(),
            }],
        }),
        span: span(),
    };

    let discerne = HirExpression {
        id: crate::hir::HirId(3),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![HirExpression {
                id: crate::hir::HirId(4),
                kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                ty: Some(numerus),
                span: span(),
            }],
            arms: vec![HirCasuArm {
                patterns: vec![HirPattern::Alias(
                    DefId(4),
                    alias,
                    Box::new(HirPattern::Variant(DefId(2), vec![HirPattern::Binding(DefId(3), payload)])),
                )],
                body: HirExpression {
                    id: crate::hir::HirId(6),
                    kind: HirExpressionKind::Literal(HirLiteral::String(ok)),
                    ty: Some(textus),
                    span: span(),
                },
                span: span(),
            }],
            exhaustive: false,
        },
        ty: Some(textus),
        span: span(),
    };

    let program = HirModule {
        items: vec![enum_item],
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(7),
                kind: HirStatementKind::Expr(discerne),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("casu Click sit payload ut matched {"));
    assert!(code.contains("\"ok\""));
}

#[test]
fn emits_object_literal_fields_from_verte_entries() {
    let mut interner = Interner::new();
    let nomen = interner.intern("nomen");
    let activus = interner.intern("activus");
    let marcus = interner.intern("Marcus");

    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let bivalens = types.primitive(Primitive::Bivalens);
    let union_ty = types.intern(Type::Union(vec![textus, bivalens]));
    let map_ty = types.map(textus, union_ty);

    let expr = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Verte {
            source: Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Tuple(Vec::new(), None),
                ty: None,
                span: span(),
            }),
            target: map_ty,
            entries: Some(vec![
                HirObjectField {
                    key: HirObjectKey::Ident(nomen),
                    value: Some(HirExpression {
                        id: crate::hir::HirId(3),
                        kind: HirExpressionKind::Literal(HirLiteral::String(marcus)),
                        ty: Some(textus),
                        span: span(),
                    }),
                },
                HirObjectField {
                    key: HirObjectKey::Ident(activus),
                    value: Some(HirExpression {
                        id: crate::hir::HirId(4),
                        kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                        ty: Some(bivalens),
                        span: span(),
                    }),
                },
            ]),
        },
        ty: Some(map_ty),
        span: span(),
    };

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(5),
                kind: HirStatementKind::Expr(expr),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("\"nomen\": \"Marcus\""));
    assert!(code.contains("\"activus\": verum"));
    assert!(code.contains("∷ tabula<textus, textus ∪ bivalens>"));
}

#[test]
fn forma_capture_with_spread_params_falls_back_to_struct_literal() {
    let mut interner = Interner::new();
    let forma = interner.intern("forma");
    let template = interner.intern("template");
    let params = interner.intern("params");
    let message = interner.intern("Hello §");
    let items = interner.intern("items");

    let mut types = TypeTable::new();
    let forma_ty = types.intern(Type::Struct(DefId(42)));

    let expr = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Verte {
            source: Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Tuple(Vec::new(), None),
                ty: None,
                span: span(),
            }),
            target: forma_ty,
            entries: Some(vec![
                HirObjectField {
                    key: HirObjectKey::Ident(template),
                    value: Some(HirExpression {
                        id: crate::hir::HirId(3),
                        kind: HirExpressionKind::Literal(HirLiteral::String(message)),
                        ty: None,
                        span: span(),
                    }),
                },
                HirObjectField {
                    key: HirObjectKey::Ident(params),
                    value: Some(HirExpression {
                        id: crate::hir::HirId(4),
                        kind: HirExpressionKind::Array(vec![HirArrayElement::Spread(HirExpression {
                            id: crate::hir::HirId(5),
                            kind: HirExpressionKind::Literal(HirLiteral::String(items)),
                            ty: None,
                            span: span(),
                        })]),
                        ty: None,
                        span: span(),
                    }),
                },
            ]),
        },
        ty: Some(forma_ty),
        span: span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: crate::hir::HirId(0),
            def_id: DefId(USER_DEF_ID_BASE + 42),
            kind: HirItemKind::Struct(HirStruct {
                name: forma,
                type_params: Vec::new(),
                fields: Vec::new(),
                methods: Vec::new(),
                implements: Vec::new(),
            }),
            span: span(),
        }],
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(6),
                kind: HirStatementKind::Expr(expr),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(!code.contains('`'));
    assert!(code.contains("forma {"));
    assert!(code.contains("params = [sparge \"items\"]"));
}

#[test]
fn emits_regex_literals_with_flags() {
    let mut interner = Interner::new();
    let pattern = interner.intern("\\d+");
    let flags = interner.intern("g");

    let types = TypeTable::new();
    let regex = types.primitive(Primitive::Regex);

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(1),
                kind: HirStatementKind::Expr(HirExpression {
                    id: crate::hir::HirId(2),
                    kind: HirExpressionKind::Literal(HirLiteral::Regex(pattern, Some(flags))),
                    ty: Some(regex),
                    span: span(),
                }),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("regex /\\d+/"));
    assert!(!code.contains("↦ regex"));
}

#[test]
fn emits_braced_unit_marker_from_hir_annotation() {
    let mut interner = Interner::new();
    let futura = interner.intern("futura");
    let work = interner.intern("work");

    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);

    let function = HirFunction {
        cli_args: None,
        name: work,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(vacuum),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: crate::hir::HirId(1),
                kind: HirStatementKind::Redde(None),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: vec![HirAnnotation {
                family: futura,
                fields: Vec::new(),
                raw_args: Vec::new(),
                contract_id: None,
                brace_authored: true,
                span: span(),
            }],
            id: crate::hir::HirId(0),
            def_id: DefId(USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(
        code.contains("@ futura { }"),
        "canonical emit must use braced unit marker, got:\n{code}"
    );
    assert!(
        !code.contains("@ futura\nfunctio"),
        "must not emit sugar-only @ futura line:\n{code}"
    );
}

#[test]
fn emits_entry_annotations_before_incipit() {
    let mut interner = Interner::new();
    let cli = interner.intern("cli");
    let nomen = interner.intern("nomen");
    let smoke = interner.intern("smoke");

    let types = TypeTable::new();

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock { statements: Vec::new(), expr: None, span: span(), ..Default::default() }),
        entry_annotations: vec![HirAnnotation {
            family: cli,
            fields: vec![crate::hir::HirAnnotationField {
                name: nomen,
                value: crate::hir::HirAnnotationValue::String(smoke),
                span: span(),
            }],
            raw_args: Vec::new(),
            contract_id: None,
            brace_authored: true,
            span: span(),
        }],
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(
        code.contains("@ cli { nomen = \"smoke\" }"),
        "expected braced entry cli annotation, got:\n{code}"
    );
    let cli_pos = code.find("@ cli {").expect("entry cli annotation");
    let incipit_pos = code.find("incipit {").expect("incipit block");
    assert!(cli_pos < incipit_pos, "entry annotations must precede incipit:\n{code}");
}

#[test]
fn localized_adfirma_message_uses_each_locale_surface() {
    let cases = [
        ("en", "fn check(int n) { assert n ≻ 0 panic \"bad\" }\n", "assert n ≻ 0 panic"),
        (
            "la",
            "functio check(numerus n) { adfirma n ≻ 0 mori \"bad\" }\n",
            "adfirma n ≻ 0 mori",
        ),
    ];

    for (locale, source, expected) in cases {
        let pack_path = crate::locale::workspace_locale_pack_path(locale);
        let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed locale pack should load");
        let session = crate::driver::Session::new(crate::driver::Config::default().with_locale_pack(pack));
        let result = crate::driver::reemit_faber(
            &session,
            session
                .config
                .locale_pack
                .as_ref()
                .expect("emission locale pack"),
            &format!("mori-{locale}.fab"),
            source,
        );
        assert!(result.success(), "{locale} canonical emit failed: {:?}", result.diagnostics);
        let output = result.output.expect("canonical output").code;
        assert!(
            output.contains(expected),
            "adfirma message must use the {locale} Mori surface, got:\n{output}"
        );
    }
}

#[test]
fn localized_emit_consults_locale_pack_surface() {
    // Phase 1 threading: the canonical emitter consults the reader pack surface,
    // so localized source re-emits in the localized surface (round-trip preserves
    // the spelling). This is the inverse of the old Latin-only emit behavior.
    let pack_path = crate::locale::workspace_locale_pack_path("th-TH");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed th-TH pack should load");

    let config = crate::driver::Config::default().with_locale_pack(pack);
    let session = crate::driver::Session::new(config);
    let result = crate::driver::reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "salve.th.fab",
        "ฟังก์ชัน salve() { }\n",
    );

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("emit should produce output");
    assert!(
        output.code.contains("ฟังก์ชัน salve()"),
        "localized emit should produce the Thai surface: {}",
        output.code
    );
    assert!(
        !output.code.contains("functio salve"),
        "localized emit should not emit the Latin keyword: {}",
        output.code
    );
}

#[test]
fn canonical_emit_preserves_legacy_posture_annotation_without_duplicate_word() {
    let pack = crate::locale::latin_locale_pack();
    let session = crate::driver::Session::new(crate::driver::Config::default().with_locale_pack(pack.clone()));
    let result =
        crate::driver::reemit_faber(&session, &pack, "legacy-posture.fab", "@ futura\nfunctio f() → vacuum { }\n");

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("emit should produce output").code;
    assert!(output.contains("@ futura"), "legacy posture annotation must remain:\n{output}");
    assert!(
        !output.contains("fiet →"),
        "legacy annotation-only source must not gain duplicate posture word:\n{output}"
    );
}

#[test]
fn localized_emit_uses_llm_contextual_morphology_surface() {
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");

    let config = crate::driver::Config::default().with_locale_pack(pack);
    let session = crate::driver::Session::new(config);
    let source = r"
fn fetch() async → int { return 1 }
fn forward() async → int { return_await fetch() }
fn stream() async_generator → int {
  await_const _ value ← fetch()
  await fetch()
  yield value
}
";
    let result = crate::driver::reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "morphology.llm.fab",
        source,
    );

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("emit should produce output").code;
    assert!(
        output.contains("fn fetch() async → int"),
        "async posture must localize:\n{output}"
    );
    assert!(
        output.contains("fn stream() async_generator → int"),
        "async stream posture must localize:\n{output}"
    );
    assert!(
        output.contains("return_await fetch()"),
        "reddet must localize without an extra return:\n{output}"
    );
    assert!(
        output.contains("await_const _ value ← fetch()"),
        "figendum must localize and preserve the authored hole:\n{output}"
    );
    assert!(output.contains("await fetch()"), "tacebit must localize:\n{output}");
    assert!(output.contains("yield value"), "cede yield must localize:\n{output}");
}

#[test]
fn localized_emit_round_trips_under_same_pack() {
    // Phase 3 (req. #10 round-trip): localized emit -> re-parse -> re-emit under
    // the same pack is lossless. The emitted Thai surface re-lexes to the same
    // token kinds, so a second canonical pass is byte-identical (idempotent).
    let pack_path = crate::locale::workspace_locale_pack_path("th-TH");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed th-TH pack should load");
    let config = crate::driver::Config::default().with_locale_pack(pack);
    let session = crate::driver::Session::new(config);

    // Localized (Thai) source: exercises functio/fixum/textus/numerus/incipit/
    // nota/redde keywords + the textus/numerus primitives + the scriptum builtin.
    // Spellings must match locale/th-TH/pack.toml (EBNF.th-TH glossary).
    let source = "\
ฟังก์ชัน salve(ข้อความ name) → ข้อความ {
    กำหนด ข้อความ greeting ← จารึก(\"salve, §!\", name)
    คืน greeting
}

เริ่ม {
    กำหนด จำนวน score ← 82
    บันทึก score
}
";

    // First canonical pass over localized source.
    let first = crate::driver::reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "salve.th.fab",
        source,
    );
    assert!(first.success(), "first pass should compile: {:?}", first.diagnostics);
    let first_code = first
        .output
        .as_ref()
        .expect("first pass should emit")
        .code
        .clone();
    assert!(first_code.contains("ฟังก์ชัน"), "first emit should localize: {first_code}");

    // Second pass: re-emit the localized output. Lossless round-trip requires the
    // localized spellings re-lex to the same token kinds, yielding identical output.
    let second = crate::driver::reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "salve.th.fab",
        &first_code,
    );
    assert!(second.success(), "round-trip re-parse should compile: {:?}", second.diagnostics);
    let second_code = second
        .output
        .as_ref()
        .expect("second pass should emit")
        .code
        .clone();

    assert_eq!(
        first_code, second_code,
        "localized emit must round-trip losslessly (idempotent re-emit):\n--- first ---\n{first_code}\n--- second ---\n{second_code}"
    );
}

#[test]
fn emits_empty_program_with_only_incipit() {
    let interner = Interner::new();
    let types = TypeTable::new();

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock { statements: Vec::new(), expr: None, span: span(), ..Default::default() }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(code.contains("incipit {"), "empty program must emit incipit block:\n{code}");
}

#[test]
fn emits_empty_program_with_no_entry() {
    let interner = Interner::new();
    let types = TypeTable::new();

    let program = HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &types, &interner);
    assert!(
        !code.contains("incipit"),
        "program with no entry must not emit incipit:\n{code}"
    );
}

#[test]
fn re_emits_ego_as_self_keyword_not_type_name() {
    // Regression: `ego` (self) lowers to a `Path` of the enclosing struct
    // def, and the emitter printed the type name (`Ray.intersecat_capsam()`),
    // which is not valid Faber source. A `Path` to a struct/enum def in value
    // position can only come from `ego`, so it must re-emit as the self
    // keyword instead of the type name.
    let mut interner = Interner::new();
    let name_ray = interner.intern("Ray");
    let name_method = interner.intern("intersecat_capsam");
    let types = TypeTable::new();
    let ray_def = DefId(USER_DEF_ID_BASE + 50);
    let method_def = DefId(USER_DEF_ID_BASE + 51);

    let method = HirMethod {
        annotations: Vec::new(),
        def_id: method_def,
        func: HirFunction {
            cli_args: None,
            name: name_method,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: None,
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: crate::hir::HirId(1),
                    kind: HirStatementKind::Redde(Some(HirExpression {
                        id: crate::hir::HirId(2),
                        kind: HirExpressionKind::Path(ray_def),
                        ty: None,
                        span: span(),
                    })),
                    span: span(),
                }],
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        },
        receiver: HirReceiver::None,
        span: span(),
    };
    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: crate::hir::HirId(0),
            def_id: ray_def,
            kind: HirItemKind::Struct(HirStruct {
                name: name_ray,
                type_params: Vec::new(),
                fields: Vec::new(),
                methods: vec![method],
                implements: Vec::new(),
            }),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let code = generate_faber(&program, &types, &interner);
    assert!(
        code.contains("ego"),
        "ego must re-emit as the self keyword, not the type name:\n{code}"
    );
    assert!(
        !code.contains("Ray.intersecat"),
        "type-qualified self call must not be emitted:\n{code}"
    );
}

// ── Conversion-directed assignment (↤, U+21A4) round-trip ───────────────────

fn canonical_lex_parse(code: &str) {
    let lex = crate::lexer::lex(code);
    assert!(lex.success(), "emitted code should lex: {:?}", lex.errors);
    let parse = crate::parser::parse(lex);
    assert!(parse.success(), "emitted code should parse: {:?}", parse.errors);
}

fn canonical_round_trip(source: &str) -> (String, String) {
    let pack = crate::locale::latin_locale_pack();
    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_resolved_locale_pack(pack.clone())
            .with_dev_locale_root(),
    );
    let result = crate::driver::reemit_faber(&session, &pack, "conversio-assign.fab", source);
    assert!(result.success(), "canonical emit failed: {:?}", result.diagnostics);
    let first = result.output.expect("output").code;
    canonical_lex_parse(&first);
    let second = crate::driver::reemit_faber(&session, &pack, "conversio-assign.fab", &first);
    assert!(second.success(), "re-parse should compile: {:?}", second.diagnostics);
    let second_code = second.output.expect("output").code;
    assert_eq!(first, second_code, "canonical(canonical(x)) must equal canonical(x)");
    (first, second_code)
}

#[test]
fn canonical_round_trips_conversio_assign() {
    // Expression-position `↤` through the HIR lane. The typed-initializer form
    // (`sit numerus n ↤ "42"`) round-trips at the author level (syntax);
    // its HIR-lane end-to-end proof lands with the U5 semantic scope
    // resolution (the binding must be visible while the initializer checks).
    let source = "incipit {\n  sit numerus n\n  n ↤ \"42\"\n}\n";
    let (code, _) = canonical_round_trip(source);
    assert!(code.contains(" ↤ "), "↤ must survive canonical emit:\n{code}");
    assert!(!code.contains(" ← "), "must never rewrite ↤ to ←:\n{code}");
    assert!(!code.contains(" ↦ "), "must never rewrite ↤ to ↦:\n{code}");
}

#[test]
fn canonical_round_trips_conversio_assign_recovery() {
    let source = "incipit {\n  sit numerus n\n  n ↤ \"x\" ⊥ 0\n}\n";
    let (code, _) = canonical_round_trip(source);
    assert!(code.contains(" ↤ "), "↤ must survive canonical emit:\n{code}");
    assert!(code.contains(" ⊥ 0"), "⊥ recovery must survive canonical emit:\n{code}");
    assert!(!code.contains(" ← "), "must never rewrite ↤ to ←:\n{code}");
    assert!(!code.contains(" ↦ "), "must never rewrite ↤ to ↦:\n{code}");
}

#[test]
fn canonical_conversio_assign_matches_ordinary_assign_for_regular_stores() {
    let source = "incipit {\n  sit numerus n\n  n ← 5\n}\n";
    let (code, _) = canonical_round_trip(source);
    assert!(code.contains(" ← "), "ordinary ← must stay ←:\n{code}");
    assert!(!code.contains(" ↤ "), "ordinary ← must not become ↤:\n{code}");
}
