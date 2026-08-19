//! Coverage for borrow-check diagnostics.
//!
//! These tests prove that the borrow pass reports use-after-move for
//! moved-by-default parameters and call arguments (Rust-output ownership
//! semantics).

use super::borrow::analyze;
use super::{DefId, Resolver, SemanticErrorKind, TypeTable, WarningKind};
use crate::hir::{
    HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction, HirItem, HirItemKind, HirLiteral, HirModule,
    HirParam, HirParamMode, HirStatement, HirStatementKind,
};
use crate::semantic::SemanticParamMode;
use crate::semantic::{FuncSig, ParamType, Primitive, SemanticError};
use crate::test_support::args_have_no_legacy_message;
use crate::test_support::test_span;

fn expect_structured_error<'a>(errors: &'a [SemanticError], kind: SemanticErrorKind, issue: &str) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.kind == kind
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .expect("structured borrow diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

#[test]
fn reports_use_after_move() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let func_ty = types.function(FuncSig {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        params: vec![ParamType { ty: numerus, mode: SemanticParamMode::Move, optional: false }],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    });

    let call = HirExpression {
        id: crate::hir::HirId(3),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: Some(func_ty),
                span: test_span(),
            }),
            Vec::new(),
            vec![crate::hir::HirCallArg {
                name: None,
                spread: false,
                expr: HirExpression {
                    id: crate::hir::HirId(4),
                    kind: HirExpressionKind::Path(DefId(1)),
                    ty: None,
                    span: test_span(),
                },
                span: test_span(),
            }],
        ),
        ty: None,
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![
                        HirStatement {
                            id: crate::hir::HirId(1),
                            kind: HirStatementKind::Expr(call),
                            span: test_span(),
                        },
                        HirStatement {
                            id: crate::hir::HirId(5),
                            kind: HirStatementKind::Expr(HirExpression {
                                id: crate::hir::HirId(6),
                                kind: HirExpressionKind::Path(DefId(1)),
                                ty: None,
                                span: test_span(),
                            }),
                            span: test_span(),
                        },
                    ],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,

                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(&errors, SemanticErrorKind::UseAfterMove, "use_after_move");
}

#[test]
fn reports_mutable_borrow_conflict() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let shared = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Ref(
            crate::hir::HirRefKind::Shared,
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: None,
                span: test_span(),
            }),
        ),
        ty: None,
        span: test_span(),
    };
    let mutable = HirExpression {
        id: crate::hir::HirId(3),
        kind: HirExpressionKind::Ref(
            crate::hir::HirRefKind::Mutable,
            Box::new(HirExpression {
                id: crate::hir::HirId(4),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: None,
                span: test_span(),
            }),
        ),
        ty: None,
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![
                        HirStatement {
                            id: crate::hir::HirId(5),
                            kind: HirStatementKind::Expr(shared),
                            span: test_span(),
                        },
                        HirStatement {
                            id: crate::hir::HirId(6),
                            kind: HirStatementKind::Expr(mutable),
                            span: test_span(),
                        },
                    ],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,

                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(&errors, SemanticErrorKind::MutableBorrowConflict, "mutable_borrow_conflict");
}

#[test]
fn reports_assign_to_de_parameter() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let assign = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Assign(
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            Box::new(HirExpression {
                id: crate::hir::HirId(3),
                kind: HirExpressionKind::Literal(HirLiteral::Int(42)),
                ty: Some(numerus),
                span: test_span(),
            }),
        ),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::De,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![HirStatement {
                        id: crate::hir::HirId(4),
                        kind: HirStatementKind::Expr(assign),
                        span: test_span(),
                    }],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,

                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(&errors, SemanticErrorKind::AssignToImmutableBorrow, "assign_to_de_parameter");
}

#[test]
fn reports_use_after_move_in_assignment_index() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let move_callee_ty = types.function(FuncSig {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        params: vec![ParamType { ty: numerus, mode: SemanticParamMode::Move, optional: false }],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    });
    let move_index = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: Some(move_callee_ty),
                span: test_span(),
            }),
            Vec::new(),
            vec![crate::hir::HirCallArg {
                name: None,
                spread: false,
                expr: HirExpression {
                    id: crate::hir::HirId(3),
                    kind: HirExpressionKind::Path(DefId(2)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
        ),
        ty: Some(numerus),
        span: test_span(),
    };
    let assign = HirExpression {
        id: crate::hir::HirId(4),
        kind: HirExpressionKind::Assign(
            Box::new(HirExpression {
                id: crate::hir::HirId(5),
                kind: HirExpressionKind::Index(
                    Box::new(HirExpression {
                        id: crate::hir::HirId(6),
                        kind: HirExpressionKind::Path(DefId(1)),
                        ty: None,
                        span: test_span(),
                    }),
                    Box::new(HirExpression {
                        id: crate::hir::HirId(7),
                        kind: HirExpressionKind::Path(DefId(2)),
                        ty: Some(numerus),
                        span: test_span(),
                    }),
                ),
                ty: Some(numerus),
                span: test_span(),
            }),
            Box::new(HirExpression {
                id: crate::hir::HirId(8),
                kind: HirExpressionKind::Literal(HirLiteral::Int(42)),
                ty: Some(numerus),
                span: test_span(),
            }),
        ),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![
                    HirParam {
                        def_id: DefId(1),
                        name: crate::lexer::Symbol(2),
                        ty: numerus,
                        mode: HirParamMode::Owned,
                        optional: false,
                        sponte: false,
                        default: None,
                        span: test_span(),
                    },
                    HirParam {
                        def_id: DefId(2),
                        name: crate::lexer::Symbol(3),
                        ty: numerus,
                        mode: HirParamMode::Owned,
                        optional: false,
                        sponte: false,
                        default: None,
                        span: test_span(),
                    },
                ],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![
                        HirStatement {
                            id: crate::hir::HirId(9),
                            kind: HirStatementKind::Expr(move_index),
                            span: test_span(),
                        },
                        HirStatement {
                            id: crate::hir::HirId(10),
                            kind: HirStatementKind::Expr(assign),
                            span: test_span(),
                        },
                    ],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,

                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(&errors, SemanticErrorKind::UseAfterMove, "use_after_move");
}

#[test]
fn reports_de_passed_to_in_position() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let callee_ty = types.function(FuncSig {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        params: vec![ParamType { ty: numerus, mode: SemanticParamMode::MutRef, optional: false }],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    });
    let call = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: Some(callee_ty),
                span: test_span(),
            }),
            Vec::new(),
            vec![crate::hir::HirCallArg {
                name: None,
                spread: false,
                expr: HirExpression {
                    id: crate::hir::HirId(3),
                    kind: HirExpressionKind::Path(DefId(1)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
        ),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::De,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![HirStatement {
                        id: crate::hir::HirId(4),
                        kind: HirStatementKind::Expr(call),
                        span: test_span(),
                    }],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,

                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(&errors, SemanticErrorKind::ModeMismatch, "de_to_in_or_ex_position");
}

#[test]
fn warns_when_in_parameter_is_never_mutated() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let read = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Path(DefId(1)),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::In,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![HirStatement {
                        id: crate::hir::HirId(2),
                        kind: HirStatementKind::Expr(read),
                        span: test_span(),
                    }],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,

                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::UnusedMutRefParam),
        "unused_in_parameter",
    );
}

#[test]
fn warns_when_ex_parameter_is_never_consumed() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let read = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Path(DefId(1)),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Ex,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![HirStatement {
                        id: crate::hir::HirId(2),
                        kind: HirStatementKind::Expr(read),
                        span: test_span(),
                    }],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,

                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::UnusedMoveParam),
        "unused_ex_parameter",
    );
}

#[test]
fn analyze_modes_rejects_use_after_move_without_rust_lifetime() {
    use super::borrow::analyze_modes;
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let func_ty = types.function(FuncSig {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        params: vec![ParamType { ty: numerus, mode: SemanticParamMode::Move, optional: false }],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    });

    let call = HirExpression {
        id: crate::hir::HirId(3),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: Some(func_ty),
                span: test_span(),
            }),
            Vec::new(),
            vec![crate::hir::HirCallArg {
                name: None,
                spread: false,
                expr: HirExpression {
                    id: crate::hir::HirId(4),
                    kind: HirExpressionKind::Path(DefId(1)),
                    ty: None,
                    span: test_span(),
                },
                span: test_span(),
            }],
        ),
        ty: None,
        span: test_span(),
    };

    let read = HirExpression {
        id: crate::hir::HirId(5),
        kind: HirExpressionKind::Path(DefId(1)),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![
                        HirStatement {
                            id: crate::hir::HirId(6),
                            kind: HirStatementKind::Expr(call),
                            span: test_span(),
                        },
                        HirStatement {
                            id: crate::hir::HirId(7),
                            kind: HirStatementKind::Expr(read),
                            span: test_span(),
                        },
                    ],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze_modes(&program, &Resolver::new(), &types);
    assert!(result.is_err(), "mode analysis must reject use-after-ex");
    expect_structured_error(&result.unwrap_err(), SemanticErrorKind::UseAfterMove, "use_after_move");
}

#[test]
fn reports_write_while_borrowed_via_mutating_method_call() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);

    // Shared borrow of local
    let shared = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Ref(
            crate::hir::HirRefKind::Shared,
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
        ),
        ty: None,
        span: test_span(),
    };

    // Method call on same local that returns Vacuum (mutating)
    let method_call = HirExpression {
        id: crate::hir::HirId(3),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: crate::hir::HirId(4),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            crate::lexer::Symbol(100),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                expr: HirExpression {
                    id: crate::hir::HirId(5),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(5)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
        ),
        ty: Some(vacuum),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![
                        HirStatement {
                            id: crate::hir::HirId(6),
                            kind: HirStatementKind::Expr(shared),
                            span: test_span(),
                        },
                        HirStatement {
                            id: crate::hir::HirId(7),
                            kind: HirStatementKind::Expr(method_call),
                            span: test_span(),
                        },
                    ],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(
        result.is_err(),
        "mutating method call on shared-borrowed value must report conflict"
    );
    let errors = result.unwrap_err();
    expect_structured_error(&errors, SemanticErrorKind::MutableBorrowConflict, "write_while_borrowed");
}

#[test]
fn allows_consecutive_mutating_method_calls_on_same_receiver() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);

    let method_call = |expr_id, receiver_id, arg_id, value| HirExpression {
        id: crate::hir::HirId(expr_id),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: crate::hir::HirId(receiver_id),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            crate::lexer::Symbol(100),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                expr: HirExpression {
                    id: crate::hir::HirId(arg_id),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
        ),
        ty: Some(vacuum),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![
                        HirStatement {
                            id: crate::hir::HirId(1),
                            kind: HirStatementKind::Expr(method_call(3, 4, 5, 5)),
                            span: test_span(),
                        },
                        HirStatement {
                            id: crate::hir::HirId(2),
                            kind: HirStatementKind::Expr(method_call(6, 7, 8, 8)),
                            span: test_span(),
                        },
                    ],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(
        result.is_ok(),
        "statement-position mutating method calls must not retain a borrow: {result:?}"
    );
}

#[test]
fn allows_non_mutating_method_call_while_borrowed() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    // Shared borrow of local
    let shared = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::Ref(
            crate::hir::HirRefKind::Shared,
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
        ),
        ty: None,
        span: test_span(),
    };

    // Method call that returns non-Vacuum (non-mutating)
    let method_call = HirExpression {
        id: crate::hir::HirId(3),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: crate::hir::HirId(4),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            crate::lexer::Symbol(101),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                expr: HirExpression {
                    id: crate::hir::HirId(5),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(5)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
        ),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![
                        HirStatement {
                            id: crate::hir::HirId(6),
                            kind: HirStatementKind::Expr(shared),
                            span: test_span(),
                        },
                        HirStatement {
                            id: crate::hir::HirId(7),
                            kind: HirStatementKind::Expr(method_call),
                            span: test_span(),
                        },
                    ],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(
        result.is_ok(),
        "non-mutating method call on shared-borrowed value must not report conflict"
    );
}

#[test]
fn mutating_method_call_marks_param_as_mutated() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);

    // Method call on 'in' param returning Vacuum (mutating)
    let method_call = HirExpression {
        id: crate::hir::HirId(1),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: crate::hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            crate::lexer::Symbol(100),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                expr: HirExpression {
                    id: crate::hir::HirId(3),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(5)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
        ),
        ty: Some(vacuum),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::In,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: None,
                err_ty: None,
                body: Some(HirBlock {
                    statements: vec![HirStatement {
                        id: crate::hir::HirId(4),
                        kind: HirStatementKind::Expr(method_call),
                        span: test_span(),
                    }],
                    expr: None,
                    span: test_span(),
                    ..Default::default()
                }),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    // With an 'in' param mutated by a method call, the function should NOT
    // warn about unused 'in' parameter.
    let result = analyze(&program, &Resolver::new(), &types);
    // The method call mutates the param, so no unused_in_parameter warning.
    assert!(
        result.is_ok(),
        "mutating method call on 'in' param should suppress unused_in_parameter warning"
    );
}

#[test]
fn bodyless_function_with_in_param_has_no_warning() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::In,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
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
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(
        result.is_ok(),
        "bodyless function with 'in' param should not produce unused_in_parameter warning"
    );
}

#[test]
fn bodyless_function_with_ex_param_has_no_warning() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Ex,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
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
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(
        result.is_ok(),
        "bodyless function with 'ex' param should not produce unused_ex_parameter warning"
    );
}

#[test]
fn bodyless_function_with_owned_param_is_ok() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
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
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_ok(), "bodyless function with owned param should pass");
}

#[test]
fn empty_function_body_has_no_borrow_errors() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: vec![HirItem {
            id: crate::hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: crate::lexer::Symbol(2),
                    ty: numerus,
                    mode: HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
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
                annotations: Vec::new(),
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types);
    assert!(result.is_ok(), "function with no body should pass borrow check");
}

// ---- CDA-U5: `↤` routes through borrow/ownership rules like `←` ----

#[test]
fn reports_conversio_assign_to_de_parameter() {
    // NO-BYPASS: `↤` stores through the target place exactly like `←`, so the
    // ownership-mode pass must flag an assignment through a `de` parameter
    // regardless of which operator was written.
    let source = r#"
functio mutate(de numerus x) → vacuum {
  x ↤ "2"
}
incipit { }
"#;
    let lex = crate::lexer::lex(source);
    let mut parsed = crate::parser::parse(lex);
    assert!(parsed.success(), "parse: {:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    let config = crate::semantic::PassConfig::base();
    let result = crate::semantic::analyze(&program, &config, &mut parsed.interner);
    assert!(
        !result.success(),
        "conversio-assign through de should be rejected: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|err| {
            err.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "assign_to_de_parameter")
        }),
        "missing assign_to_de_parameter: {:?}",
        result.diagnostics
    );
}
