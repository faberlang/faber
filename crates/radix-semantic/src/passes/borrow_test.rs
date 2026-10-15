//! Coverage for borrow-check diagnostics.
//!
//! These tests prove that the borrow pass reports ownership violations for
//! declared access facts and call arguments (Rust-output ownership semantics).

use super::borrow::{analyze, analyze_modes, analyze_rust_lifetime};
use super::{DefId, Resolver, SemanticErrorKind, TypeTable, WarningKind};
use crate::SemanticParamMode;
use crate::test_support::args_have_no_legacy_message;
use crate::test_support::test_span;
use crate::{AccessMarker, FuncSig, ParamType, Primitive, SemanticError};
use radix_hir::{
    HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction, HirItem, HirItemKind,
    HirLiteral, HirModule, HirParam, HirParamMode, HirStatement, HirStatementKind,
};
use radix_lexer::Interner;

fn expect_structured_error<'a>(
    errors: &'a [SemanticError],
    kind: SemanticErrorKind,
    issue: &str,
) -> &'a SemanticError {
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
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Move,
            optional: false,
            sponte: false,
        }],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    });

    let call = HirExpression {
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: Some(func_ty),
                span: test_span(),
            }),
            Vec::new(),
            vec![radix_hir::HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(4),
                    kind: HirExpressionKind::Path(DefId(1)),
                    ty: None,
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: None,
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                            id: radix_hir::HirId(1),
                            kind: HirStatementKind::Expr(call),
                            span: test_span(),
                        },
                        HirStatement {
                            id: radix_hir::HirId(5),
                            kind: HirStatementKind::Expr(HirExpression {
                                id: radix_hir::HirId(6),
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

                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(&errors, SemanticErrorKind::UseAfterMove, "use_after_move");
}

#[test]
fn reports_mutable_borrow_conflict() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let shared = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Ref(
            radix_hir::HirRefKind::Shared,
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: None,
                span: test_span(),
            }),
        ),
        ty: None,
        span: test_span(),
    };
    let mutable = HirExpression {
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::Ref(
            radix_hir::HirRefKind::Mutable,
            Box::new(HirExpression {
                id: radix_hir::HirId(4),
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
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                            id: radix_hir::HirId(5),
                            kind: HirStatementKind::Expr(shared),
                            span: test_span(),
                        },
                        HirStatement {
                            id: radix_hir::HirId(6),
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

                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::MutableBorrowConflict,
        "mutable_borrow_conflict",
    );
}

#[test]
fn allows_repoint_of_de_parameter() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let assign = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Assign(
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            Box::new(HirExpression {
                id: radix_hir::HirId(3),
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
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                        id: radix_hir::HirId(4),
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

                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    // Re-pointing the parameter name is legal; only a write through it is
    // refused.
    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(result.is_ok(), "re-point of a ref parameter: {result:?}");
}

#[test]
fn reports_use_after_move_in_assignment_index() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let move_callee_ty = types.function(FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Move,
            optional: false,
            sponte: false,
        }],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    });
    let move_index = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: Some(move_callee_ty),
                span: test_span(),
            }),
            Vec::new(),
            vec![radix_hir::HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(3),
                    kind: HirExpressionKind::Path(DefId(2)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: Some(numerus),
        span: test_span(),
    };
    let assign = HirExpression {
        id: radix_hir::HirId(4),
        kind: HirExpressionKind::Assign(
            Box::new(HirExpression {
                id: radix_hir::HirId(5),
                kind: HirExpressionKind::Index(
                    Box::new(HirExpression {
                        id: radix_hir::HirId(6),
                        kind: HirExpressionKind::Path(DefId(1)),
                        ty: None,
                        span: test_span(),
                    }),
                    Box::new(HirExpression {
                        id: radix_hir::HirId(7),
                        kind: HirExpressionKind::Path(DefId(2)),
                        ty: Some(numerus),
                        span: test_span(),
                    }),
                ),
                ty: Some(numerus),
                span: test_span(),
            }),
            Box::new(HirExpression {
                id: radix_hir::HirId(8),
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
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![
                    HirParam {
                        def_id: DefId(1),
                        name: radix_lexer::Symbol(2),
                        ty: numerus,
                        mode: HirParamMode::Owned,
                        optional: false,
                        sponte: false,
                        default: None,
                        span: test_span(),
                    },
                    HirParam {
                        def_id: DefId(2),
                        name: radix_lexer::Symbol(3),
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
                            id: radix_hir::HirId(9),
                            kind: HirStatementKind::Expr(move_index),
                            span: test_span(),
                        },
                        HirStatement {
                            id: radix_hir::HirId(10),
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

                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
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
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::MutRef,
            optional: false,
            sponte: false,
        }],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    });
    let call = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: Some(callee_ty),
                span: test_span(),
            }),
            Vec::new(),
            vec![radix_hir::HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(3),
                    kind: HirExpressionKind::Path(DefId(1)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                        id: radix_hir::HirId(4),
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

                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::ModeMismatch,
        "de_to_in_or_ex_position",
    );
}

#[test]
fn warns_when_in_parameter_is_never_mutated() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let read = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Path(DefId(1)),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
                    ty: types.access_marker(AccessMarker::Mut, numerus),
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
                        id: radix_hir::HirId(2),
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

                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::UnusedMutRefParam),
        "unused_mut_parameter",
    );
}

#[test]
fn warns_when_ex_parameter_is_never_consumed() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let read = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Path(DefId(1)),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
                    ty: types.access_marker(AccessMarker::Own, numerus),
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
                        id: radix_hir::HirId(2),
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

                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::UnusedMoveParam),
        "unused_own_parameter",
    );
}

#[test]
fn analyze_modes_rejects_use_after_move_without_rust_lifetime() {
    use super::borrow::analyze_modes;
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let own_numerus = types.access_marker(AccessMarker::Own, numerus);
    let func_ty = types.function(FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: own_numerus,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    });

    let call = HirExpression {
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: Some(func_ty),
                span: test_span(),
            }),
            Vec::new(),
            vec![radix_hir::HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(4),
                    kind: HirExpressionKind::Path(DefId(1)),
                    ty: Some(own_numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: None,
        span: test_span(),
    };

    let read = HirExpression {
        id: radix_hir::HirId(5),
        kind: HirExpressionKind::Path(DefId(1)),
        ty: Some(own_numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
                    ty: own_numerus,
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
                            id: radix_hir::HirId(6),
                            kind: HirStatementKind::Expr(call),
                            span: test_span(),
                        },
                        HirStatement {
                            id: radix_hir::HirId(7),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze_modes(&program, &Resolver::new(), &types, &Interner::new());
    assert!(
        result.is_err(),
        "mode analysis must reject use-after-move of own"
    );
    expect_structured_error(
        &result.unwrap_err(),
        SemanticErrorKind::UseAfterMove,
        "use_after_move",
    );
}

#[test]
fn reports_write_while_borrowed_via_mutating_method_call() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);

    // Shared borrow of local
    let shared = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Ref(
            radix_hir::HirRefKind::Shared,
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
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
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: radix_hir::HirId(4),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            radix_lexer::Symbol(100),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(5),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(5)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: Some(vacuum),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                            id: radix_hir::HirId(6),
                            kind: HirStatementKind::Expr(shared),
                            span: test_span(),
                        },
                        HirStatement {
                            id: radix_hir::HirId(7),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(
        result.is_err(),
        "mutating method call on shared-borrowed value must report conflict"
    );
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::MutableBorrowConflict,
        "write_while_borrowed",
    );
}

#[test]
fn allows_consecutive_mutating_method_calls_on_same_receiver() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);

    let method_call = |expr_id, receiver_id, arg_id, value| HirExpression {
        id: radix_hir::HirId(expr_id),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: radix_hir::HirId(receiver_id),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            radix_lexer::Symbol(100),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(arg_id),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: Some(vacuum),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                            id: radix_hir::HirId(1),
                            kind: HirStatementKind::Expr(method_call(3, 4, 5, 5)),
                            span: test_span(),
                        },
                        HirStatement {
                            id: radix_hir::HirId(2),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
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
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Ref(
            radix_hir::HirRefKind::Shared,
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
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
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: radix_hir::HirId(4),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            radix_lexer::Symbol(101),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(5),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(5)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: Some(numerus),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                            id: radix_hir::HirId(6),
                            kind: HirStatementKind::Expr(shared),
                            span: test_span(),
                        },
                        HirStatement {
                            id: radix_hir::HirId(7),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
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
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(numerus),
                span: test_span(),
            }),
            radix_lexer::Symbol(100),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(3),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(5)),
                    ty: Some(numerus),
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: Some(vacuum),
        span: test_span(),
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                        id: radix_hir::HirId(4),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    // With a mut access parameter mutated by a method call, the function
    // should not warn about an unused mut parameter.
    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    // The method call mutates the parameter, so no unused-mut warning appears.
    assert!(
        result.is_ok(),
        "mutating method call should suppress the unused mut parameter warning"
    );
}

#[test]
fn bodyless_function_with_in_param_has_no_warning() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(
        result.is_ok(),
        "bodyless function with a mut access parameter should not produce a warning"
    );
}

#[test]
fn bodyless_function_with_ex_param_has_no_warning() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(
        result.is_ok(),
        "bodyless function with an own access parameter should not produce a warning"
    );
}

#[test]
fn bodyless_function_with_owned_param_is_ok() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(
        result.is_ok(),
        "bodyless function with owned param should pass"
    );
}

#[test]
fn empty_function_body_has_no_borrow_errors() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = analyze(&program, &Resolver::new(), &types, &Interner::new());
    assert!(
        result.is_ok(),
        "function with no body should pass borrow check"
    );
}

// ---- CDA-U5: `↤` routes through borrow/ownership rules like `←` ----

#[test]
fn reports_conversio_assign_through_ref_parameter() {
    // NO-BYPASS: `↤` stores through the target place exactly like `←`, so the
    // ownership-mode pass must flag an assignment through a `de` parameter
    // regardless of which operator was written.
    let source = r#"
genus Punctum { varia numerus x }
functio mutate(de Punctum p) → vacuum {
  p.x ↤ "2"
}
incipit { }
"#;
    let lex = radix_lexer::lex(source);
    let mut parsed = radix_parser::parse(lex);
    assert!(parsed.success(), "parse: {:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    let config = crate::PassConfig::base();
    let result = crate::analyze(&program, &config, &mut parsed.interner);
    assert!(
        !result.is_ok(),
        "conversio-assign through de should be rejected: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|err| {
            err.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "assign_through_ref")
        }),
        "missing assign_through_ref: {:?}",
        result.diagnostics
    );
}

fn dual_explicit_ref_program(param_ty: radix_types::TypeId) -> HirModule {
    let shared = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Ref(
            radix_hir::HirRefKind::Shared,
            Box::new(HirExpression {
                id: radix_hir::HirId(2),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(param_ty),
                span: test_span(),
            }),
        ),
        ty: None,
        span: test_span(),
    };
    let mutable = HirExpression {
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::Ref(
            radix_hir::HirRefKind::Mutable,
            Box::new(HirExpression {
                id: radix_hir::HirId(4),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(param_ty),
                span: test_span(),
            }),
        ),
        ty: None,
        span: test_span(),
    };
    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
                    ty: param_ty,
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
                            id: radix_hir::HirId(5),
                            kind: HirStatementKind::Expr(shared),
                            span: test_span(),
                        },
                        HirStatement {
                            id: radix_hir::HirId(6),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn analyze_modes_does_not_reject_unmarked_explicit_ref_conflict() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let result = analyze_modes(
        &dual_explicit_ref_program(numerus),
        &Resolver::new(),
        &types,
        &Interner::new(),
    );
    assert!(
        result.is_ok(),
        "unmarked explicit refs stay Rust-lifetime-only, got: {:?}",
        result.err()
    );
}

#[test]
fn analyze_modes_rejects_mut_root_explicit_ref_conflict() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let mut_ty = types.access_marker(AccessMarker::Mut, numerus);
    let result = analyze_modes(
        &dual_explicit_ref_program(mut_ty),
        &Resolver::new(),
        &types,
        &Interner::new(),
    );
    assert!(
        result.is_err(),
        "mut-root explicit refs are a language conflict"
    );
    expect_structured_error(
        &result.unwrap_err(),
        SemanticErrorKind::MutableBorrowConflict,
        "mutable_borrow_conflict",
    );
}

/// MU-1-baseline shape (D-1): a `const`/unmarked source passed into a single
/// `mut` param, then used sequentially after the call. The call-scoped borrow
/// must release at call end, so rust-lifetime qualification accepts the
/// program.
fn const_into_mut_sequential_use_program(types: &mut TypeTable) -> HirModule {
    let numerus = types.primitive(Primitive::Numerus);
    // `poke(a)` with a named `poke` Path (callee.ty unset).
    let poke_call = HirExpression {
        id: radix_hir::HirId(4),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(3),
                kind: HirExpressionKind::Path(DefId(0)),
                ty: None,
                span: test_span(),
            }),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(5),
                    kind: HirExpressionKind::Path(DefId(2)),
                    ty: None,
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: None,
        span: test_span(),
    };
    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: DefId(0),
            kind: HirItemKind::Function(Box::new(HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![HirParam {
                    def_id: DefId(1),
                    name: radix_lexer::Symbol(2),
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
                test: None,
            })),
            span: test_span(),
        }],
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: radix_hir::HirId(1),
                    kind: HirStatementKind::Local(radix_hir::HirLocal {
                        def_id: DefId(2),
                        name: radix_lexer::Symbol(3),
                        ty: Some(numerus),
                        init: Some(HirExpression {
                            id: radix_hir::HirId(2),
                            kind: HirExpressionKind::Literal(HirLiteral::Int(3)),
                            ty: Some(numerus),
                            span: test_span(),
                        }),
                        mutable: false,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span: test_span(),
                },
                HirStatement {
                    id: radix_hir::HirId(6),
                    kind: HirStatementKind::Expr(poke_call),
                    span: test_span(),
                },
                HirStatement {
                    id: radix_hir::HirId(7),
                    kind: HirStatementKind::Expr(HirExpression {
                        id: radix_hir::HirId(8),
                        kind: HirExpressionKind::Path(DefId(2)),
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
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn rust_lifetime_releases_mut_param_call_borrow_after_call() {
    let mut types = TypeTable::new();
    let result = analyze_rust_lifetime(
        &const_into_mut_sequential_use_program(&mut types),
        &Resolver::new(),
        &types,
    );
    assert!(
        result.is_ok(),
        "mut-param call borrow must release at call end (D-1 unmarked const source), got: {:?}",
        result.err()
    );
}

#[test]
fn borrow_conflict_names_the_borrowed_place() {
    // `conflict` resolves the place spelling through the shared lookup; the
    // diagnostic must still carry the binding name.
    let source = r"
fn f(ref tf32[2, 2] a) → void {
    mut mut tf32[2, 2] x ← a
    mut mut tf32[2, 2] y ← a
    print x.flatten()
    print y.flatten()
}
main {
}";
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    let fixture = crate::test_support::analyze_source_with_config(source, config);
    let conflict = fixture
        .result
        .diagnostics
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "mutable_borrow_conflict")
        })
        .expect("mutable_borrow_conflict diagnostic");
    assert!(
        conflict
            .args
            .iter()
            .any(|arg| arg.name == "name" && arg.value == "a"),
        "conflict should name the borrowed place: {conflict:?}"
    );
}
