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
    );
    assert!(errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria)));
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
    );
    assert!(errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnusedVariable)));
    assert!(!errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria)));
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
    );
    assert!(!errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria)));
}

#[test]
fn does_not_warn_on_varia_local_mutated_by_intrinsic_method() {
    let mut interner = Interner::new();
    let method = interner.intern("appende");
    let def_id = radix_hir::DefId(1);
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let lista_numerus = types.array(numerus);
    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![
                HirStatement {
                    id: radix_hir::HirId(1),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id,
                        name: radix_lexer::Symbol(1),
                        ty: Some(lista_numerus),
                        init: Some(HirExpression {
                            id: radix_hir::HirId(2),
                            kind: HirExpressionKind::Vacua,
                            ty: Some(lista_numerus),
                            span: test_span(),
                        }),
                        mutable: true,
                        is_await: false,
                        runtime_provided: false,
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
                                ty: Some(lista_numerus),
                                span: test_span(),
                            }),
                            method,
                            Vec::new(),
                            vec![HirCallArg {
                                name: None,
                                spread: false,
                                expr: lit_expr(),
                                span: test_span(),
                            }],
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

    let errors = lint(&program, &Resolver::new(), &types, &interner);
    assert!(!errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria)));
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
    );
    assert!(errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnusedImport)));
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

    let errors = lint(&program, &Resolver::new(), &types, &Interner::new());
    assert!(errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryCast)));
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

    let errors = lint(&program, &Resolver::new(), &types, &Interner::new());
    assert!(errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnnecessaryCast)));
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

    let errors = lint(&program, &Resolver::new(), &types, &Interner::new());
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
    );
    assert!(errors
        .iter()
        .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::UnreachableCode)));
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

    let errors = lint(&program, &Resolver::new(), &types, &Interner::new());
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
    fixum textus x ← "seed"
    fixum textus m ← "a" + x + "b"
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
    fixum textus x ← "seed"
    fixum textus m ← "a" + x
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
    fixum textus x ← "seed"
    fixum textus y ← "more"
    fixum textus z ← "again"
    fixum textus m ← x + y + z
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
    let errors = lint(&chain_program(chain), &Resolver::new(), &types, &interner);
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
