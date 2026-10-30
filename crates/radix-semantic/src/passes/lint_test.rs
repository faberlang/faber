//! Coverage for quality-warning diagnostics.
//!
//! These tests prove that the lint pass fires on unused variables, unnecessary
//! `varia` declarations, shadowed names, unreachable code, unnecessary casts,
//! unused imports, and explicit `ignotum` annotations.

use super::lint::lint;
use super::{Resolver, SemanticErrorKind, TypeTable, WarningKind};
use crate::test_support::args_have_no_legacy_message;
use crate::test_support::test_span;
use crate::{Primitive, SemanticError, TypeId};
use radix_hir::{
    HirBinOp, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirImport, HirImportItem,
    HirItem, HirItemKind, HirLiteral, HirLocal, HirModule, HirObjectField, HirObjectKey,
    HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span};
use radix_syntax::Visibility;

fn lit_expr() -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(0),
        kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
        ty: None,
        span: test_span(),
    }
}

fn path_expr(def_id: radix_hir::DefId) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(0),
        kind: HirExpressionKind::Path(def_id),
        ty: None,
        span: test_span(),
    }
}

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
        .expect("structured lint diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

#[test]
fn warns_on_unused_local() {
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Local(HirLocal {
                    def_id: radix_hir::DefId(1),
                    name: radix_lexer::Symbol(1),
                    ty: None,
                    init: Some(lit_expr()),
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::UnusedVariable),
        "unused_variable",
    );
}

#[test]
fn does_not_warn_on_used_local() {
    let def_id = radix_hir::DefId(1);
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: radix_hir::HirId(1),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id,
                        name: radix_lexer::Symbol(1),
                        ty: None,
                        init: Some(lit_expr()),
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
                    id: radix_hir::HirId(2),
                    kind: HirStatementKind::Expr(path_expr(def_id)),
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
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnusedVariable)),
        "used local should not trigger unused_variable warning"
    );
}

#[test]
fn warns_on_varia_local_never_mutated() {
    let def_id = radix_hir::DefId(1);
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: radix_hir::HirId(1),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id,
                        name: radix_lexer::Symbol(1),
                        ty: None,
                        init: Some(lit_expr()),
                        mutable: true,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span: test_span(),
                },
                HirStatement {
                    id: radix_hir::HirId(2),
                    kind: HirStatementKind::Expr(path_expr(def_id)),
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
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    assert!(
        errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria))
    );
}

#[test]
fn does_not_warn_on_unused_varia_local() {
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Local(HirLocal {
                    def_id: radix_hir::DefId(1),
                    name: radix_lexer::Symbol(1),
                    ty: None,
                    init: Some(lit_expr()),
                    mutable: true,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    assert!(
        errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnusedVariable))
    );
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria))
    );
}

#[test]
fn does_not_warn_on_varia_local_reassigned() {
    let def_id = radix_hir::DefId(1);
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: radix_hir::HirId(1),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id,
                        name: radix_lexer::Symbol(1),
                        ty: None,
                        init: Some(lit_expr()),
                        mutable: true,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span: test_span(),
                },
                HirStatement {
                    id: radix_hir::HirId(2),
                    kind: HirStatementKind::Expr(HirExpression {
                        id: radix_hir::HirId(3),
                        kind: HirExpressionKind::Assign(
                            Box::new(path_expr(def_id)),
                            Box::new(lit_expr()),
                        ),
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
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria))
    );
}

/// Lint a program that declares `varia <receiver_ty> x` and then only calls
/// `x.<method>(1)`; returns whether WARN013 (`UnnecessaryVaria`) fired.
fn varia_local_warns_after_method_call(
    method_name: &str,
    receiver: fn(&mut TypeTable) -> TypeId,
) -> bool {
    let mut interner = Interner::new();
    let method = interner.intern(method_name);
    let def_id = radix_hir::DefId(1);
    let mut types = TypeTable::new();
    let receiver_ty = receiver(&mut types);
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: radix_hir::HirId(1),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id,
                        name: radix_lexer::Symbol(1),
                        ty: Some(receiver_ty),
                        init: Some(HirExpression {
                            id: radix_hir::HirId(2),
                            kind: HirExpressionKind::Vacua,
                            ty: Some(receiver_ty),
                            span: test_span(),
                        }),
                        mutable: true,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span: test_span(),
                },
                HirStatement {
                    id: radix_hir::HirId(3),
                    kind: HirStatementKind::Expr(HirExpression {
                        id: radix_hir::HirId(4),
                        kind: HirExpressionKind::MethodCall(
                            Box::new(HirExpression {
                                id: radix_hir::HirId(5),
                                kind: HirExpressionKind::Path(def_id),
                                ty: Some(receiver_ty),
                                span: test_span(),
                            }),
                            method,
                            Vec::new(),
                            vec![HirCallArg {
                                name: None,
                                spread: false,
                                marker: None,
                                expr: lit_expr(),
                                span: test_span(),
                            }],
                            None,
                        ),
                        ty: Some(types.primitive(Primitive::Vacuum)),
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
    };

    let errors = lint(&program, &Resolver::new(), &types, &interner, None);
    errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria))
}

fn lista_numerus(types: &mut TypeTable) -> TypeId {
    let numerus = types.primitive(Primitive::Numerus);
    types.array(numerus)
}

fn numerus(types: &mut TypeTable) -> TypeId {
    types.primitive(Primitive::Numerus)
}

// A mutating intrinsic (`appende`) rewrites the list in place and typecheck
// rejects it on an immutable binding receiver, so the `varia` stays necessary.
#[test]
fn does_not_warn_on_varia_list_only_appended() {
    assert!(!varia_local_warns_after_method_call(
        "appende",
        lista_numerus
    ));
}

// A non-intrinsic (user) method on the receiver is not a reassignment either.
#[test]
fn warns_on_varia_local_only_receiving_unknown_method() {
    assert!(varia_local_warns_after_method_call("auge", lista_numerus));
}

// Numeric imperativus methods (`n.adde(1)`) rewrite the scalar value itself and
// require a `varia` receiver, so they count as reassignment.
#[test]
fn does_not_warn_on_varia_numerus_updated_by_imperativus_method() {
    assert!(!varia_local_warns_after_method_call("adde", numerus));
}

#[test]
fn unused_local_warning_uses_declaration_span() {
    let declaration_span = Span::new(10, 20);
    let initializer_span = Span::new(30, 40);
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Local(HirLocal {
                    def_id: radix_hir::DefId(1),
                    name: radix_lexer::Symbol(1),
                    ty: None,
                    init: Some(HirExpression {
                        id: radix_hir::HirId(2),
                        kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                        ty: None,
                        span: initializer_span,
                    }),
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: declaration_span,
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    assert!(errors.iter().any(|err| {
        err.kind == SemanticErrorKind::Warning(WarningKind::UnusedVariable)
            && err.span == declaration_span
    }));
    assert!(!errors.iter().any(|err| {
        err.kind == SemanticErrorKind::Warning(WarningKind::UnusedVariable)
            && err.span == initializer_span
    }));
}

#[test]
fn warns_on_unreachable_code() {
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: radix_hir::HirId(1),
                    kind: HirStatementKind::Redde(Some(lit_expr())),
                    span: test_span(),
                },
                HirStatement {
                    id: radix_hir::HirId(2),
                    kind: HirStatementKind::Expr(lit_expr()),
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
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::UnreachableCode),
        "unreachable_code",
    );
}

#[test]
fn warns_on_unused_import() {
    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: radix_hir::DefId(10),
            kind: HirItemKind::Import(HirImport {
                path: radix_lexer::Symbol(1),
                visibility: Visibility::Privata,
                items: vec![HirImportItem {
                    def_id: radix_hir::DefId(11),
                    name: radix_lexer::Symbol(2),
                    alias: None,
                }],
                selective: false,
                target: None,
            }),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    assert!(
        errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnusedImport))
    );
}

#[test]
fn unused_import_warning_points_at_the_import_statement() {
    let import_span = Span::new(40, 62);
    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: radix_hir::DefId(10),
            kind: HirItemKind::Import(HirImport {
                path: radix_lexer::Symbol(1),
                visibility: Visibility::Privata,
                items: vec![HirImportItem {
                    def_id: radix_hir::DefId(11),
                    name: radix_lexer::Symbol(2),
                    alias: None,
                }],
                selective: false,
                target: None,
            }),
            span: import_span,
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    let warning = errors
        .iter()
        .find(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnusedImport))
        .expect("unused import warning");
    assert_eq!(
        warning.span, import_span,
        "the warning must anchor at the import, not at the file start"
    );
}

#[test]
fn warns_on_shadowed_variable() {
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: radix_hir::HirId(1),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id: radix_hir::DefId(1),
                        name: radix_lexer::Symbol(1),
                        ty: None,
                        init: Some(lit_expr()),
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
                    id: radix_hir::HirId(2),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id: radix_hir::DefId(2),
                        name: radix_lexer::Symbol(1),
                        ty: None,
                        init: Some(lit_expr()),
                        mutable: false,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
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
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    expect_structured_error(
        &errors,
        SemanticErrorKind::ShadowedVariable,
        "shadowed_variable",
    );
}

#[test]
fn warns_on_unnecessary_cast() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Expr(HirExpression {
                    id: radix_hir::HirId(2),
                    kind: HirExpressionKind::Verte {
                        source: Box::new(HirExpression {
                            id: radix_hir::HirId(3),
                            kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                            ty: Some(numerus),
                            span: test_span(),
                        }),
                        target: numerus,
                        entries: None,
                    },
                    ty: Some(numerus),
                    span: test_span(),
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(&program, &Resolver::new(), &types, &Interner::new(), None);
    assert!(
        errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryCast))
    );
}

#[test]
fn warns_on_unnecessary_cast_through_alias() {
    use crate::Type;

    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    // Create an alias that resolves to Numerus — the cast should still be flagged
    let alias = types.intern(Type::Alias(radix_hir::DefId(999), numerus));
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Expr(HirExpression {
                    id: radix_hir::HirId(2),
                    kind: HirExpressionKind::Verte {
                        source: Box::new(HirExpression {
                            id: radix_hir::HirId(3),
                            kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                            ty: Some(numerus),
                            span: test_span(),
                        }),
                        target: alias,
                        entries: None,
                    },
                    ty: Some(alias),
                    span: test_span(),
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(&program, &Resolver::new(), &types, &Interner::new(), None);
    assert!(
        errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryCast))
    );
}

#[test]
fn no_warning_on_verte_with_entries() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let struct_ty = types.intern(crate::Type::Struct(radix_hir::DefId(100)));
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Expr(HirExpression {
                    id: radix_hir::HirId(2),
                    kind: HirExpressionKind::Verte {
                        source: Box::new(HirExpression {
                            id: radix_hir::HirId(3),
                            kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                            ty: Some(struct_ty),
                            span: test_span(),
                        }),
                        target: struct_ty,
                        // Entries present = struct construction, not a redundant cast
                        entries: Some(vec![HirObjectField {
                            key: HirObjectKey::Ident(radix_lexer::Symbol(0)),
                            value: Some(HirExpression {
                                id: radix_hir::HirId(4),
                                kind: HirExpressionKind::Literal(HirLiteral::Int(42)),
                                ty: Some(numerus),
                                span: test_span(),
                            }),
                        }]),
                    },
                    ty: Some(struct_ty),
                    span: test_span(),
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(&program, &Resolver::new(), &types, &Interner::new(), None);
    // Should NOT contain an UnnecessaryCast warning
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryCast)),
        "struct construction through Verte should not trigger unnecessary cast warning"
    );
}

#[test]
fn warns_on_unreachable_after_break() {
    let loop_block = HirBlock {
        statements: vec![
            HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Rumpe,
                span: test_span(),
            },
            HirStatement {
                id: radix_hir::HirId(2),
                kind: HirStatementKind::Expr(lit_expr()),
                span: test_span(),
            },
        ],
        expr: None,
        span: test_span(),
        ..Default::default()
    };
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(3),
                kind: HirStatementKind::Expr(HirExpression {
                    id: radix_hir::HirId(4),
                    kind: HirExpressionKind::Loop(loop_block),
                    ty: None,
                    span: test_span(),
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    assert!(
        errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnreachableCode))
    );
}

#[test]
fn warns_on_explicit_ignotum_annotation() {
    let types = TypeTable::new();
    let ignotum = types.primitive(Primitive::Ignotum);
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Local(HirLocal {
                    def_id: radix_hir::DefId(1),
                    name: radix_lexer::Symbol(1),
                    ty: Some(ignotum),
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = lint(&program, &Resolver::new(), &types, &Interner::new(), None);
    expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::ExplicitIgnotumAnnotation),
        "explicit_ignotum_annotation",
    );
}

#[test]
fn warnings_only_findings_are_non_fatal() {
    // Regression guard for the lint boundary: a program that produces only
    // warnings must return its findings without making them fatal. Callers
    // (SemanticResult / the driver) decide fatality via is_error().
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Local(HirLocal {
                    def_id: radix_hir::DefId(1),
                    name: radix_lexer::Symbol(1),
                    ty: None,
                    init: Some(lit_expr()),
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let findings = lint(
        &program,
        &Resolver::new(),
        &TypeTable::new(),
        &Interner::new(),
        None,
    );
    assert!(
        findings
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnusedVariable)),
        "unused local should produce a warning finding"
    );
    assert!(
        findings.iter().all(|err| !err.is_error()),
        "warnings-only findings must not be fatal"
    );
}

// ---- CDA-U5: `↤` mutates its target place for lint mutation tracking ----

#[test]
fn conversio_assign_counts_as_mutation_for_unnecessary_varia() {
    // NO-BYPASS: lint's mutation tracking must see a `↤` store as a mutation.
    // If the form silently skipped `mark_place_mutated`, a `varia` mutated only
    // through `↤` would receive a spurious `unnecessary_varia` warning.
    let result = crate::test_support::analyze_source(
        r#"
incipit {
    varia numerus count ← 0
    count ↤ "1"
    nota count
}
"#,
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        !diagnostics
            .iter()
            .any(|diag| !diag.is_error() && diag.issue() == Some("unnecessary_varia")),
        "conversio-assign should mark the target place mutated: {diagnostics:?}"
    );
}

#[test]
fn mutating_method_call_counts_as_mutation_for_unnecessary_varia() {
    // A user method that changes `ego` needs a `varia` receiver binding (the
    // checker rejects it on `fixum`), so the lint must not advise `fixum`.
    let result = crate::test_support::analyze_source(
        r"
genus Contador {
    varia numerus n
    functio cresce() → vacuum { ego.n ← ego.n + 1 }
}
incipit {
    varia Contador k ← Contador { n = 1 }
    k.cresce()
}
",
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        !diagnostics
            .iter()
            .any(|diag| !diag.is_error() && diag.issue() == Some("unnecessary_varia")),
        "mutating method call should mark the receiver mutated: {diagnostics:?}"
    );
}

#[test]
fn publica_function_is_not_unused_function() {
    let result =
        crate::test_support::analyze_source("@ publica\nfunctio servata() → numerus { redde 1 }");
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("unused_function")),
        "a public function is an exported use: {:?}",
        result.diagnostics
    );
}

#[test]
fn plain_unused_function_still_warns() {
    let result = crate::test_support::analyze_source("functio servata() → numerus { redde 1 }");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("unused_function")),
        "a private unused function should still warn: {:?}",
        result.diagnostics
    );
}

const SORT_PERSONA: &str = "genus Persona {\n    varia textus nomen\n    varia numerus aetas\n}\n";

fn unnecessary_varia_count(source: &str) -> usize {
    crate::test_support::analyze_source(source)
        .diagnostics
        .iter()
        .filter(|diag| !diag.is_error() && diag.issue() == Some("unnecessary_varia"))
        .count()
}

#[test]
fn plain_in_place_sort_counts_as_mutation_for_unnecessary_varia() {
    let source = "incipit {\n    varia lista<numerus> xs ← [3, 1, 2]\n    xs.ordina()\n}";
    assert_eq!(unnecessary_varia_count(source), 0);
}

#[test]
fn plain_in_place_sort_of_copy_binding_counts_as_mutation_for_unnecessary_varia() {
    // `copy` wraps the binding type in an access marker; the receiver lookup
    // must see through it. Only the never-mutated source `xs` is flagged.
    let source = "incipit {\n    varia lista<numerus> xs ← [3, 1, 2]\n    varia lista<numerus> ys ← copy xs\n    ys.ordina()\n}";
    assert_eq!(unnecessary_varia_count(source), 1);
}

#[test]
fn keyed_in_place_sort_counts_as_mutation_for_unnecessary_varia() {
    let source = format!(
        "{SORT_PERSONA}incipit {{\n    varia lista<Persona> ps ← [Persona {{ nomen = \"b\", aetas = 2 }}]\n    ps.ordina(Persona p ∴ p.aetas)\n}}"
    );
    assert_eq!(unnecessary_varia_count(&source), 0);
}

#[test]
fn keyed_in_place_sort_of_copy_keeps_never_mutated_source_flagged() {
    // `ps` is only read (copied); `m` is the sorted receiver. Only the source
    // binding is unnecessary.
    let source = format!(
        "{SORT_PERSONA}incipit {{\n    varia lista<Persona> ps ← [Persona {{ nomen = \"b\", aetas = 2 }}]\n    varia lista<Persona> m ← copy ps\n    m.ordina(Persona p ∴ p.aetas)\n}}"
    );
    assert_eq!(unnecessary_varia_count(&source), 1);
}

#[test]
fn never_mutated_varia_stays_flagged_without_in_place_sort() {
    let source = "incipit {\n    varia lista<numerus> xs ← [3, 1, 2]\n    nota xs.longitudo()\n}";
    assert_eq!(unnecessary_varia_count(source), 1);
}

// The bare harness above compiles the latin surface; the corpus exemplar uses
// the English one. These twins pin the same registry lookup through the en
// lex pack for both method forms.
fn en_config() -> crate::PassConfig {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config
}

fn en_unnecessary_varia_count(source: &str) -> usize {
    let result = crate::test_support::analyze_source_with_config(source, en_config());
    assert!(
        !result
            .diagnostics
            .iter()
            .any(super::super::error::SemanticError::is_error),
        "en fixture must be error-free: {:?}",
        result.diagnostics
    );
    result
        .diagnostics
        .iter()
        .filter(|diag| !diag.is_error() && diag.issue() == Some("unnecessary_varia"))
        .count()
}

#[test]
fn english_plain_in_place_sort_counts_as_mutation_for_unnecessary_varia() {
    let source = "main {\n    mut list<int> xs ← [3, 1, 2]\n    xs.sort()\n}";
    assert_eq!(en_unnecessary_varia_count(source), 0);
}

#[test]
fn english_plain_in_place_sort_of_copy_binding_counts_as_mutation_for_unnecessary_varia() {
    // The `copy` access marker must peel for the en `.sort()` surface too.
    let source = "main {\n    mut list<int> xs ← [3, 1, 2]\n    mut list<int> ys ← copy xs\n    ys.sort()\n}";
    assert_eq!(en_unnecessary_varia_count(source), 1);
}

#[test]
fn english_keyed_in_place_sort_counts_as_mutation_for_unnecessary_varia() {
    // The exemplar shape: a `const` source and a `var` copy sorted in place.
    let source = "class Persona {\n    mut string nomen\n    mut int aetas\n}\nmain {\n    let list<Persona> ps ← [Persona { nomen = \"b\", aetas = 2 }]\n    mut list<Persona> m ← copy ps\n    m.sort(Persona p ∴ p.aetas)\n}";
    assert_eq!(en_unnecessary_varia_count(source), 0);
}

// A plain bind of a shareable value aliases its source (D-1), so a write
// through the new name is a write to what the source binding names, and the
// share itself needs the source's `varia`: following a WARN013 `let` advice
// there would turn the bind into SEM065 (`const_shared_into_var`).
const ALIAS_BOX: &str = "class Box {\n    mut int v = 0\n}\n";

#[test]
fn write_through_an_alias_counts_as_mutation_for_unnecessary_varia() {
    // The probe: `mut Box b ← a` shares `a`'s object, so `b.v ← 2` writes
    // what `a` names. WARN013 must not advise `let` on `a`.
    let source = format!(
        "{ALIAS_BOX}main {{\n    mut Box a ← Box {{ v = 1 }}\n    mut Box b ← a\n    b.v ← 2\n    print a.v\n}}"
    );
    assert_eq!(en_unnecessary_varia_count(&source), 0);
}

#[test]
fn varia_source_shared_into_a_varia_cell_stays_unflagged() {
    // No write anywhere, but the share itself needs `a`'s `varia` (`let a`
    // makes `mut Box b ← a` SEM065). The cell's own warning stands: `b` is
    // never written, so `let b` is genuinely legal.
    let source = format!(
        "{ALIAS_BOX}main {{\n    mut Box a ← Box {{ v = 1 }}\n    mut Box b ← a\n    print a.v\n    print b.v\n}}"
    );
    assert_eq!(en_unnecessary_varia_count(&source), 1);
}

#[test]
fn chained_alias_intermediate_source_stays_unflagged_and_advice_applies() {
    // Chain `b ← a`, then `c ← b`: SEM065 reads the initializer's immediate
    // place root, so the intermediate `b` needs `varia` exactly like the
    // ultimate source `a`. Only the never-shared tail `c` is advised, and
    // applying that advice must stay legal: the rewritten source compiles
    // without SEM065 (the helper asserts error-freeness).
    let source = format!(
        "{ALIAS_BOX}main {{\n    mut Box a ← Box {{ v = 1 }}\n    mut Box b ← a\n    mut Box c ← b\n    print a.v\n    print b.v\n    print c.v\n}}"
    );
    assert_eq!(en_unnecessary_varia_count(&source), 1);
    let rewritten = format!(
        "{ALIAS_BOX}main {{\n    mut Box a ← Box {{ v = 1 }}\n    mut Box b ← a\n    let Box c ← b\n    print a.v\n    print b.v\n    print c.v\n}}"
    );
    assert_eq!(en_unnecessary_varia_count(&rewritten), 1);
}

#[test]
fn write_through_a_chained_alias_counts_as_mutation_for_the_whole_family() {
    // `b ← a`, `c ← b`: one write through the tail mutates the shared
    // object, so no member of the family may be advised `let`.
    let source = format!(
        "{ALIAS_BOX}main {{\n    mut Box a ← Box {{ v = 1 }}\n    mut Box b ← a\n    mut Box c ← b\n    c.v ← 2\n    print a.v\n    print b.v\n    print c.v\n}}"
    );
    assert_eq!(en_unnecessary_varia_count(&source), 0);
}

#[test]
fn copy_source_of_a_varia_cell_stays_flagged() {
    // `copy` acquires an independent value: no alias, no share, so the
    // never-written source keeps its warning (negative twin).
    let source = format!(
        "{ALIAS_BOX}main {{\n    mut Box a ← Box {{ v = 1 }}\n    mut Box b ← copy a\n    b.v ← 2\n    print a.v\n}}"
    );
    assert_eq!(en_unnecessary_varia_count(&source), 1);
}

#[test]
fn scalar_source_of_a_varia_cell_stays_flagged() {
    // Scalars copy by value: `let a` cannot trip SEM065, so the warning
    // stands (negative twin).
    let source = "main {\n    mut int a ← 1\n    mut int b ← a\n    b ← 2\n    print a\n}";
    assert_eq!(en_unnecessary_varia_count(source), 1);
}

const ORDINA_CLAVIS_EXEMPLAR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/lista/ordina-clavis.fab"
));

fn strip_frontmatter(raw: &str) -> &str {
    if let Some(rest) = raw.strip_prefix("+++")
        && let Some(end) = rest.find("+++")
    {
        return &rest[end + 3..];
    }
    raw
}

#[test]
fn ordina_clavis_exemplar_has_no_unnecessary_varia() {
    // `personae` is only read (the in-place sort works on `per_aetatem`), so
    // the exemplar keeps it `const`; this pins the file against a `var`
    // regression.
    let source = strip_frontmatter(ORDINA_CLAVIS_EXEMPLAR);
    assert_eq!(
        en_unnecessary_varia_count(source),
        0,
        "corpus/lista/ordina-clavis.fab must keep a `const` source binding"
    );
}

// ---- PST-U1: PreferStringTemplate — naive string concatenation chains ----
//
// Threshold B (operator decision 856a3b7a): warn only on string-typed `+`
// chains of 3+ operands with at least one string-literal operand. Two-operand
// concat and pure-variable chains do NOT warn; numeric `+` is untouched.

fn int_lit(value: u64, ty: Option<TypeId>) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(0),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty,
        span: test_span(),
    }
}

fn string_literal(interner: &mut Interner, text: &str, ty: Option<TypeId>) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(0),
        kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern_raw(text))),
        ty,
        span: test_span(),
    }
}

fn ascii_literal(interner: &mut Interner, text: &str, ty: Option<TypeId>) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(0),
        kind: HirExpressionKind::Literal(HirLiteral::Ascii(interner.intern_raw(text))),
        ty,
        span: test_span(),
    }
}

fn path_typed(def_id: radix_hir::DefId, ty: Option<TypeId>) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(0),
        kind: HirExpressionKind::Path(def_id),
        ty,
        span: test_span(),
    }
}

fn add_expr(lhs: HirExpression, rhs: HirExpression, ty: Option<TypeId>) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(0),
        kind: HirExpressionKind::Binary(HirBinOp::Add, Box::new(lhs), Box::new(rhs)),
        ty,
        span: test_span(),
    }
}

fn chain_program(expr: HirExpression) -> HirModule {
    HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(1),
                kind: HirStatementKind::Expr(expr),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

fn has_prefer_string_template(errors: &[SemanticError]) -> bool {
    errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::PreferStringTemplate))
}

#[test]
fn warns_on_mixed_string_chain_with_literal() {
    let result = crate::test_support::analyze_source(
        r#"
incipit {
    sit textus x ← "seed"
    sit textus m ← "a" + x + "b"
    nota m
}
"#,
    );
    let diagnostics = result.diagnostics.clone();
    let warning = diagnostics
        .iter()
        .find(|diag| diag.issue() == Some("prefer_string_template"))
        .unwrap_or_else(|| panic!("expected PreferStringTemplate warning, got: {diagnostics:?}"));
    assert_eq!(crate::semantic_spec(warning.kind).code, "WARN018");
    let suggestion = warning
        .args
        .iter()
        .find(|arg| arg.name == "suggestion")
        .map(|arg| arg.value.as_str());
    assert_eq!(
        suggestion,
        Some("\"a§b\"(x)"),
        "suggestion should point at the template form: {diagnostics:?}"
    );
}

#[test]
fn does_not_warn_on_two_operand_string_concat() {
    let result = crate::test_support::analyze_source(
        r#"
incipit {
    sit textus x ← "seed"
    sit textus m ← "a" + x
    nota m
}
"#,
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("prefer_string_template")),
        "two-operand concat must not warn (threshold B): {diagnostics:?}"
    );
}

#[test]
fn does_not_warn_on_pure_variable_string_chain() {
    let result = crate::test_support::analyze_source(
        r#"
incipit {
    sit textus x ← "seed"
    sit textus y ← "more"
    sit textus z ← "again"
    sit textus m ← x + y + z
    nota m
}
"#,
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("prefer_string_template")),
        "pure-variable chains must not warn (threshold B): {diagnostics:?}"
    );
}

#[test]
fn does_not_warn_on_numeric_chain() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let chain = add_expr(
        add_expr(
            int_lit(1, Some(numerus)),
            int_lit(2, Some(numerus)),
            Some(numerus),
        ),
        int_lit(3, Some(numerus)),
        Some(numerus),
    );
    let errors = lint(
        &chain_program(chain),
        &Resolver::new(),
        &types,
        &Interner::new(),
        None,
    );
    assert!(
        !has_prefer_string_template(&errors),
        "numeric `+` must never trigger PreferStringTemplate: {errors:?}"
    );
}

#[test]
fn warns_on_chain_with_ascii_literal_and_folds_text_into_suggestion() {
    // Advisory: an ascii literal counts as a string-literal
    // operand, and its text folds into the Unicode template suggestion.
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let ascii = types.primitive(Primitive::Ascii);
    let chain = add_expr(
        add_expr(
            string_literal(&mut interner, "a", Some(textus)),
            ascii_literal(&mut interner, "b", Some(ascii)),
            Some(textus),
        ),
        path_typed(radix_hir::DefId(1), Some(textus)),
        Some(textus),
    );
    let errors = lint(
        &chain_program(chain),
        &Resolver::new(),
        &types,
        &interner,
        None,
    );
    let err = expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::PreferStringTemplate),
        "prefer_string_template",
    );
    let suggestion = err
        .args
        .iter()
        .find(|arg| arg.name == "suggestion")
        .map(|arg| arg.value.as_str());
    // Chain is `("a" + 'b') + x` in source order: both literal texts fold into
    // the template body ("ab"), the trailing path becomes the `§` hole.
    assert_eq!(suggestion, Some("\"ab§\"(_)"));
}

// ---- GT-U1: PreferGuillemetQuote — `"…"` literals that escape `"` ----
//
// Operator proposal 4 (2026-10-13): a short string that escapes a quote
// (`\"`) reads better as a guillemet block, which needs no escape for `"`
// or `'`. Advisory only.

#[test]
fn warns_prefer_guillemet_for_quote_on_escaped_quote() {
    let result = crate::test_support::analyze_source(
        r#"
incipit {
    nota "say \"hi\""
}
"#,
    );
    let diagnostics = result.diagnostics.clone();
    let warning = diagnostics
        .iter()
        .find(|diag| diag.issue() == Some("prefer_guillemet_for_quote"))
        .unwrap_or_else(|| panic!("expected PreferGuillemetQuote warning, got: {diagnostics:?}"));
    assert_eq!(crate::semantic_spec(warning.kind).code, "WARN034");
}

#[test]
fn no_prefer_guillemet_for_quote_on_guillemet_literal() {
    let result = crate::test_support::analyze_source(
        r#"
incipit {
    nota «say "hi"»
}
"#,
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("prefer_guillemet_for_quote")),
        "guillemet text needs no escape and must not warn: {diagnostics:?}"
    );
}

#[test]
fn no_prefer_guillemet_for_quote_on_plain_or_escaped_backslash() {
    let result = crate::test_support::analyze_source(
        r#"
incipit {
    nota "plain"
    nota "back\\slash"
}
"#,
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("prefer_guillemet_for_quote")),
        "plain text and escaped backslashes must not warn: {diagnostics:?}"
    );
}

// EOT-1: the lint surface must eventually see each selective item as an
// ordinary local. Until EOT-2M installs those bindings, the source fixture
// stops at the known parser/lowering issue `expected_expression`.
#[test]
fn eot_selective_import_lint_red_driver_reaches_bound_items() {
    let result = crate::test_support::analyze_source(
        r#"importa ex "norma:consolum" fixum dic ut output, funde ut output_bytes,
incipit { nota output }"#,
    );
    assert!(
        result.is_ok(),
        "EOT-2M lint red driver must analyze selective bindings; current diagnostics={:?}; expected later issues include `selective_import_missing_member`, and `selective_import_duplicate_alias`",
        result.diagnostics
    );
}

#[test]
fn eot_legacy_import_lint_surface_remains_unchanged() {
    let result = crate::test_support::analyze_source(
        r#"importa ex "norma:consolum" consolum
incipit {
  nota "legacy output"
}"#,
    );
    assert!(
        result.is_ok(),
        "legacy namespace import/output baseline must remain analyzable: {:?}",
        result.diagnostics
    );
}

// =============================================================================
// EOT-2M — per-binding lint and resolved-use receipts
// =============================================================================

#[test]
fn selective_import_lint_warns_unused_member_per_binding() {
    let result = crate::test_support::analyze_source(
        r#"importa ex "norma:consolum" fixum dic ut output, funde,
incipit { nota output }"#,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    let unused: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|err| crate::test_support::issue(err) == Some("unused_import"))
        .collect();
    assert_eq!(
        unused.len(),
        1,
        "exactly the unused member warns; got: {:?}",
        result.diagnostics
    );

    // The used alias's DefId is distinct from the unused member's DefId, so
    // the single warning above is attributable to one binding identity.
    let mut interner = result.interner.clone();
    let output_name = interner.intern("output");
    let funde_name = interner.intern("funde");
    let output_def = result
        .resolver
        .lookup(output_name)
        .expect("used alias DefId");
    let funde_def = result
        .resolver
        .lookup(funde_name)
        .expect("unused member DefId");
    assert_ne!(output_def, funde_def);
}

#[test]
fn selective_import_resolved_use_records_each_binding() {
    use crate::QualifiedIdentityTable;
    use crate::resolved_use::{ResolvedUseKind, analyze_resolved_uses};

    let mut result = crate::test_support::analyze_source(
        r#"importa ex "norma:consolum" fixum dic ut output, funde,
incipit { output("salve") }"#,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    let hir = result.hir.clone().expect("hir");
    let qualified = QualifiedIdentityTable::default();
    let index = analyze_resolved_uses(&hir, &result.types, &qualified);

    let output_name = result.interner.intern("output");
    let funde_name = result.interner.intern("funde");
    let output_def = result
        .resolver
        .lookup(output_name)
        .expect("used alias DefId");
    let funde_def = result
        .resolver
        .lookup(funde_name)
        .expect("unused member DefId");

    // One ImportBinding receipt per selective member, on its own DefId.
    // (ImportBinding receipts are keyed, not use-site indexed; scan `uses`.)
    for def in [output_def, funde_def] {
        assert!(
            index.uses().iter().any(|use_site| {
                use_site.key == crate::resolved_use::ResolvedUseKey::Local(def)
                    && use_site.kind == ResolvedUseKind::ImportBinding
            }),
            "DefId {def:?} must carry its own ImportBinding receipt"
        );
    }
    // The call through the alias is a use of the alias's local DefId.
    assert!(
        index
            .uses_of_local(output_def)
            .any(|use_site| use_site.kind == ResolvedUseKind::Call),
        "the aliased call must record a Call use on the binding DefId"
    );
}

// ── Ruling R3: `perge` in `fac … dum` reaches the condition test ───────────

fn unreachable_count(source: &str) -> usize {
    crate::test_support::analyze_source(source)
        .result
        .diagnostics
        .iter()
        .filter(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnreachableCode))
        .count()
}

#[test]
fn fac_dum_condition_reached_by_perge_is_not_unreachable() {
    let source = r"
incipit {
    varia numerus n ← 0
    fac {
        n ← n + 1
        si n ≺ 3 {
            perge
        }
        rumpe
    } dum n ≺ 10
}
";
    assert_eq!(unreachable_count(source), 0);
}

#[test]
fn fac_dum_condition_reached_by_perge_through_bare_fac_is_not_unreachable() {
    let source = r"
incipit {
    varia numerus n ← 0
    fac {
        n ← n + 1
        fac {
            si n ≺ 3 {
                perge
            }
        }
        rumpe
    } dum n ≺ 10
}
";
    assert_eq!(unreachable_count(source), 0);
}

#[test]
fn fac_dum_condition_after_rumpe_without_perge_is_unreachable() {
    let source = r"
incipit {
    varia numerus n ← 0
    fac {
        n ← n + 1
        rumpe
    } dum n ≺ 10
}
";
    assert_eq!(unreachable_count(source), 1);
}

#[test]
fn fac_dum_perge_in_nested_loop_does_not_reach_outer_condition() {
    let source = r"
incipit {
    varia numerus n ← 0
    fac {
        n ← n + 1
        dum n ≺ 3 {
            n ← n + 1
            perge
        }
        rumpe
    } dum n ≺ 10
}
";
    assert_eq!(unreachable_count(source), 1);
}
