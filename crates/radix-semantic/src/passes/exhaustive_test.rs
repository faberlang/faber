//! Coverage for exhaustive pattern-match analysis.
//!
//! These tests prove that the pass reports non-exhaustive matches, duplicate
//! and unreachable patterns, and malformed `omnia` arms. Nullable-scrutinee
//! absorption warnings are also locked.
//!
//! uvf-u2: union (`discretio`) scrutinees participate in exhaustiveness and
//! `omnia` alignment through the enum-shaped HIR they lower to — see the probe
//! note in the union section below.

use super::exhaustive::{check, collect_enum_variants, discerne_definitely_covered};
use crate::SemanticError;
use crate::test_support::args_have_no_legacy_message;
use crate::test_support::test_span;
use crate::{DefId, SemanticErrorKind, Type, TypeId, TypeTable, WarningKind};
use radix_hir::{
    HirBlock, HirCasuArm, HirExpression, HirExpressionKind, HirItem, HirItemKind, HirLiteral,
    HirModule, HirPattern, HirStatement, HirStatementKind, HirVariant, HirVariantField,
};
use rustc_hash::{FxHashMap, FxHashSet};

fn expect_structured_error<'a>(
    errors: &'a [SemanticError],
    kind: SemanticErrorKind,
    expected_args: &[(&str, &str)],
) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.kind == kind
                && expected_args.iter().all(|(name, value)| {
                    err.args
                        .iter()
                        .any(|arg| arg.name == *name && arg.value == *value)
                })
        })
        .expect("structured semantic diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

fn lit_expr(id: u32) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
        ty: Some(TypeId(0)),
        span: test_span(),
    }
}

fn match_expr(scrutinee_ty: TypeId, patterns: Vec<HirPattern>) -> HirExpression {
    match_expr_with_exhaustive(scrutinee_ty, patterns, false)
}

/// Construct a `discerne` match where each pattern becomes its own arm.
fn match_expr_with_exhaustive(
    scrutinee_ty: TypeId,
    patterns: Vec<HirPattern>,
    exhaustive: bool,
) -> HirExpression {
    let scrutinee = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(scrutinee_ty),
        span: test_span(),
    };
    let arms = patterns
        .into_iter()
        .map(|pattern| HirCasuArm {
            patterns: vec![pattern],
            body: lit_expr(2),
            span: test_span(),
        })
        .collect();
    HirExpression {
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![scrutinee],
            arms,
            exhaustive,
        },
        ty: None,
        span: test_span(),
    }
}

fn match_expr_with_arms(scrutinee_ty: TypeId, arm_patterns: Vec<Vec<HirPattern>>) -> HirExpression {
    let scrutinee = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(scrutinee_ty),
        span: test_span(),
    };
    let arms = arm_patterns
        .into_iter()
        .map(|patterns| HirCasuArm {
            patterns,
            body: lit_expr(2),
            span: test_span(),
        })
        .collect();
    HirExpression {
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![scrutinee],
            arms,
            exhaustive: false,
        },
        ty: None,
        span: test_span(),
    }
}

fn program_with_match(enum_def: DefId, match_expr: HirExpression) -> HirModule {
    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(10),
            def_id: enum_def,
            kind: HirItemKind::Enum(radix_hir::HirEnum {
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                commune_fields: Vec::new(),
                variants: vec![
                    HirVariant {
                        def_id: DefId(2),
                        name: radix_lexer::Symbol(2),
                        fields: Vec::<HirVariantField>::new(),
                        span: test_span(),
                    },
                    HirVariant {
                        def_id: DefId(3),
                        name: radix_lexer::Symbol(3),
                        fields: Vec::<HirVariantField>::new(),
                        span: test_span(),
                    },
                ],
            }),
            span: test_span(),
        }],
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(20),
                kind: HirStatementKind::Expr(match_expr),
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

#[test]
fn reports_non_exhaustive_match() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr(enum_ty, vec![HirPattern::Variant(DefId(2), Vec::new())]);
    let program = program_with_match(enum_def, match_expr);

    let result = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    );
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::NonExhaustiveMatch,
        &[("issue", "missing_variants")],
    );
}

#[test]
fn reports_duplicate_variant_pattern() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr(
        enum_ty,
        vec![
            HirPattern::Variant(DefId(2), Vec::new()),
            HirPattern::Variant(DefId(2), Vec::new()),
        ],
    );
    let program = program_with_match(enum_def, match_expr);

    let result = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    );
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::PatternDuplicate,
        &[("issue", "duplicate_variant")],
    );
}

#[test]
fn counts_all_alternative_patterns_in_single_scrutinee_arm() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr_with_arms(
        enum_ty,
        vec![vec![
            HirPattern::Variant(DefId(2), Vec::new()),
            HirPattern::Variant(DefId(3), Vec::new()),
        ]],
    );
    let program = program_with_match(enum_def, match_expr);

    let result = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn reports_unreachable_after_wildcard() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr(
        enum_ty,
        vec![
            HirPattern::Wildcard,
            HirPattern::Variant(DefId(2), Vec::new()),
        ],
    );
    let program = program_with_match(enum_def, match_expr);

    let result = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    );
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::UnreachablePattern,
        &[("issue", "after_catchall")],
    );
}

// DEFER-013: a `casu _` over a nullable scrutinee (`T ∪ nihil`) silently
// swallows the null case. The pass must warn unless an explicit `casu nihil`
// arm is present. These tests build a `discerne` whose scrutinee carries the
// nullable type directly, with no enum item, so `program_with_match`'s enum
// scaffold is bypassed.

fn program_with_bare_match(match_expr: HirExpression) -> HirModule {
    HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(20),
                kind: HirStatementKind::Expr(match_expr),
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

#[test]
fn warns_when_wildcard_absorbs_nullable_constituent() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(crate::Primitive::Numerus);
    let nihil = types.primitive(crate::Primitive::Nihil);
    let nullable = types.intern(Type::Union(vec![numerus, nihil]));
    let match_expr = match_expr(nullable, vec![HirPattern::Wildcard]);
    let program = program_with_bare_match(match_expr);

    let result = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    );
    // WHY: warnings ride the same error stream, so the pass returns Err but the
    // result stays a successful `check` (`is_error()` is false for warnings).
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(WarningKind::WildcardAbsorbsNull),
        // The null-arm word is resolved from the locale pack's `[types] nihil`
        // surface at render time (D23.2), so it is not a literal arg.
        &[("issue", "wildcard_absorbs_null"), ("casu", "casu")],
    );
}

#[test]
fn warns_when_binding_absorbs_option_constituent() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(crate::Primitive::Numerus);
    let optional = types.option(numerus);
    let match_expr = match_expr(
        optional,
        vec![HirPattern::Binding(DefId(9), radix_lexer::Symbol(9))],
    );
    let program = program_with_bare_match(match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::WildcardAbsorbsNull))
    );
}

#[test]
fn suppresses_warning_when_explicit_null_arm_present() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(crate::Primitive::Numerus);
    let nihil = types.primitive(crate::Primitive::Nihil);
    let nullable = types.intern(Type::Union(vec![numerus, nihil]));
    let match_expr = match_expr_with_arms(
        nullable,
        vec![
            vec![HirPattern::Literal(HirLiteral::Nil)],
            vec![HirPattern::Wildcard],
        ],
    );
    let program = program_with_bare_match(match_expr);

    // No hard errors, and no wildcard-absorb warning once null is handled.
    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .err()
    .unwrap_or_default();
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::Warning(WarningKind::WildcardAbsorbsNull))
    );
}

#[test]
fn no_warning_when_scrutinee_is_not_nullable() {
    let types = TypeTable::new();
    let numerus = types.primitive(crate::Primitive::Numerus);
    let match_expr = match_expr(numerus, vec![HirPattern::Wildcard]);
    let program = program_with_bare_match(match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .err()
    .unwrap_or_default();
    assert!(errors.is_empty());
}

// --- omnia (exhaustive flag) tests ---
//
// `discerne omnia` is valid only over a single enum scrutinee and forbids
// catchall arms (wildcard, binding, ceterum). Every variant must be named
// explicitly — the keyword is an author promise that new variants will not
// be silently absorbed by a default arm.

fn omnia_match_expr(scrutinee_ty: TypeId, patterns: Vec<HirPattern>) -> HirExpression {
    match_expr_with_exhaustive(scrutinee_ty, patterns, true)
}

fn omnia_multi_scrutinee_match_expr(scrutinee_ty: TypeId) -> HirExpression {
    let scrutinee = |id| HirExpression {
        id: radix_hir::HirId(id),
        kind: HirExpressionKind::Path(DefId(100 + id)),
        ty: Some(scrutinee_ty),
        span: test_span(),
    };

    HirExpression {
        id: radix_hir::HirId(30),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![scrutinee(1), scrutinee(2)],
            arms: Vec::new(),
            exhaustive: true,
        },
        ty: None,
        span: test_span(),
    }
}

#[test]
fn omnia_accepts_all_variants_named() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = omnia_match_expr(
        enum_ty,
        vec![
            HirPattern::Variant(DefId(2), Vec::new()),
            HirPattern::Variant(DefId(3), Vec::new()),
        ],
    );
    let program = program_with_match(enum_def, match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .err()
    .unwrap_or_default();
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn omnia_rejects_wildcard_catchall() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = omnia_match_expr(
        enum_ty,
        vec![
            HirPattern::Variant(DefId(2), Vec::new()),
            HirPattern::Wildcard,
        ],
    );
    let program = program_with_match(enum_def, match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::OmniaForbidsCatchall,
        &[("issue", "catchall_arm"), ("keyword", "omnia")],
    );
}

#[test]
fn omnia_rejects_binding_catchall() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = omnia_match_expr(
        enum_ty,
        vec![
            HirPattern::Variant(DefId(2), Vec::new()),
            HirPattern::Binding(DefId(9), radix_lexer::Symbol(9)),
        ],
    );
    let program = program_with_match(enum_def, match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::OmniaForbidsCatchall,
        &[("issue", "catchall_arm"), ("keyword", "omnia")],
    );
}

#[test]
fn omnia_rejects_non_enum_scrutinee() {
    let types = TypeTable::new();
    let numerus = types.primitive(crate::Primitive::Numerus);
    let match_expr = omnia_match_expr(numerus, vec![HirPattern::Literal(HirLiteral::Int(1))]);
    // Non-enum scrutinee — use a bare match (no enum definition needed).
    let program = program_with_bare_match(match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::OmniaRequiresEnum,
        &[("issue", "enum_scrutinee"), ("keyword", "omnia")],
    );
}

#[test]
fn omnia_rejects_multi_scrutinee() {
    let types = TypeTable::new();
    let numerus = types.primitive(crate::Primitive::Numerus);
    let match_expr = omnia_multi_scrutinee_match_expr(numerus);
    let program = program_with_bare_match(match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::OmniaRequiresEnum,
        &[("issue", "single_enum_scrutinee"), ("keyword", "omnia")],
    );
}

#[test]
fn omnia_still_reports_missing_variants() {
    // omnia without a catchall and with a missing variant should still fire
    // the existing NonExhaustiveMatch error alongside any omnia checks.
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = omnia_match_expr(enum_ty, vec![HirPattern::Variant(DefId(2), Vec::new())]);
    let program = program_with_match(enum_def, match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::NonExhaustiveMatch,
        &[("issue", "missing_variants")],
    );
}

/// A mixed arm `casu A, _` (named variant + wildcard in the same arm) must be
/// flagged under omnia. The wildcard absorbs every remaining variant, which is
/// exactly the catchall-in-disguise that omnia exists to forbid. Regression for
/// the `all` → `any` quantifier fix: `all` let this slip through.
#[test]
fn omnia_rejects_mixed_variant_wildcard_arm() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr_with_exhaustive(
        enum_ty,
        vec![
            HirPattern::Variant(DefId(2), Vec::new()),
            HirPattern::Wildcard,
        ],
        true,
    );
    let program = program_with_match(enum_def, match_expr);

    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::OmniaForbidsCatchall,
        &[("issue", "catchall_arm"), ("keyword", "omnia")],
    );
}

// ── Edge cases: empty and single-variant enums ──────────────────────────────

fn program_with_variants(
    enum_def: DefId,
    match_expr: HirExpression,
    variants: Vec<HirVariant>,
) -> HirModule {
    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(10),
            def_id: enum_def,
            kind: HirItemKind::Enum(radix_hir::HirEnum {
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                commune_fields: Vec::new(),
                variants,
            }),
            span: test_span(),
        }],
        entry: Some(HirBlock {
            statements: vec![HirStatement {
                id: radix_hir::HirId(20),
                kind: HirStatementKind::Expr(match_expr),
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

#[test]
fn empty_enum_omnia_accepts_zero_arms() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let scrutinee = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(enum_ty),
        span: test_span(),
    };
    let match_expr = HirExpression {
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![scrutinee],
            arms: Vec::new(),
            exhaustive: true,
        },
        ty: None,
        span: test_span(),
    };
    let program = program_with_variants(enum_def, match_expr, Vec::new());
    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .err()
    .unwrap_or_default();
    assert!(
        errors.is_empty(),
        "expected no errors for empty enum omnia, got: {errors:?}"
    );
}

#[test]
fn single_variant_enum_accepts_exact_match() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr(enum_ty, vec![HirPattern::Variant(DefId(2), Vec::new())]);
    let variants = vec![HirVariant {
        def_id: DefId(2),
        name: radix_lexer::Symbol(2),
        fields: Vec::<HirVariantField>::new(),
        span: test_span(),
    }];
    let program = program_with_variants(enum_def, match_expr, variants);
    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .err()
    .unwrap_or_default();
    assert!(
        errors.is_empty(),
        "expected no errors for single-variant enum match, got: {errors:?}"
    );
}

#[test]
fn single_variant_omnia_accepts_exact_match() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr_with_exhaustive(
        enum_ty,
        vec![HirPattern::Variant(DefId(2), Vec::new())],
        true,
    );
    let variants = vec![HirVariant {
        def_id: DefId(2),
        name: radix_lexer::Symbol(2),
        fields: Vec::<HirVariantField>::new(),
        span: test_span(),
    }];
    let program = program_with_variants(enum_def, match_expr, variants);
    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .err()
    .unwrap_or_default();
    assert!(
        errors.is_empty(),
        "expected no errors for single-variant omnia, got: {errors:?}"
    );
}

#[test]
fn definitely_covered_enum_without_variant_list_needs_catchall() {
    // D4.8 (supersedes F-077's arm-derived variant set): an enum whose
    // variant list is not visible is not a closed set, so variant arms alone
    // never prove coverage — the driver supplies imported and builtin enum
    // lists (`collect_all_enum_variants`), and a list that is still missing
    // leaves only a catchall as proof.
    let mut types = TypeTable::new();
    let imported_enum_def = DefId(99);
    let enum_ty = types.intern(Type::Enum(imported_enum_def));
    let arm_body = HirExpression {
        id: radix_hir::HirId(91),
        kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
        ty: Some(types.primitive(crate::Primitive::Numerus)),
        span: test_span(),
    };
    let scrutinees = vec![HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(enum_ty),
        span: test_span(),
    }];
    // No entry for imported enum — simulates collect_enum_variants only knowing local enums.
    let enum_variants: FxHashMap<DefId, Vec<DefId>> = FxHashMap::default();

    // All variant arms → not provably covered without the variant list.
    let all_variant_body = arm_body.clone();
    let arms = vec![
        HirCasuArm {
            patterns: vec![HirPattern::Variant(DefId(2), Vec::new())],
            body: all_variant_body.clone(),
            span: test_span(),
        },
        HirCasuArm {
            patterns: vec![HirPattern::Variant(DefId(3), Vec::new())],
            body: all_variant_body,
            span: test_span(),
        },
    ];
    assert!(
        !discerne_definitely_covered(&scrutinees, &arms, &types, &enum_variants),
        "enum without a variant list must not be covered by variant arms alone"
    );

    // With the variant list supplied, the same arms are covered.
    let mut listed = FxHashMap::default();
    listed.insert(imported_enum_def, vec![DefId(2), DefId(3)]);
    assert!(
        discerne_definitely_covered(&scrutinees, &arms, &types, &listed),
        "enum with a supplied variant list is covered by all its variant arms"
    );

    // A catchall arm closes coverage regardless.
    let catchall_arms = vec![
        HirCasuArm {
            patterns: vec![HirPattern::Variant(DefId(2), Vec::new())],
            body: arm_body.clone(),
            span: test_span(),
        },
        HirCasuArm {
            patterns: vec![HirPattern::Wildcard],
            body: arm_body,
            span: test_span(),
        },
    ];
    assert!(
        discerne_definitely_covered(&scrutinees, &catchall_arms, &types, &enum_variants),
        "enum without a variant list is covered by a catchall"
    );

    // Empty arms.
    assert!(
        !discerne_definitely_covered(&scrutinees, &[], &types, &enum_variants),
        "enum with no arms must not be definitely covered"
    );
}

// ---------------------------------------------------------------------------
// uvf-u2 probe + regression locks: union scrutinees in exhaustiveness +
// omnia alignment
//
// Probe finding (done_when a): union variants ALREADY flow through
// `collect_enum_variants`. `discretio` lowers enum-shaped
// (`hir/lower/decl.rs` `lower_discretio` → `HirItemKind::Enum`), and union
// declarations register as `SymbolKind::Enum` (`semantic/passes/collect.rs`),
// so a union-typed scrutinee resolves to `Type::Enum(union_def_id)` and
// `enum_def_from_type` resolves it. Same-module missing-variant reporting and
// the omnia path (gate + catchall + missing-variant checks) were already green
// on the post-uvf-u1 tree — the goal's mechanism claims (enum-only
// enumeration at :63, OmniaRequiresEnum at :108/:123) are masking artifacts,
// the same class uvf-u1 recorded for G2b. No exhaustive.rs code change was
// needed; these fixtures lock done_when (b) and (c).
//
// These fixtures run real source through the full pipeline so the scrutinee
// type is exactly what typecheck assigns to a union-typed value (not a
// hand-built HIR stand-in).
// ---------------------------------------------------------------------------

fn union_probe_diagnostics(source: &str) -> Vec<crate::SemanticError> {
    crate::test_support::analyze_source(source)
        .diagnostics
        .clone()
}

fn union_probe_error_issues(source: &str) -> Vec<String> {
    union_probe_diagnostics(source)
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.issue().unwrap_or_default().to_owned())
        .collect()
}

const EVENTUM_SCRUTINY: &str = r"discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}";

#[test]
fn union_match_reports_missing_variants() {
    // done_when (b): a same-module union `match` omitting a variant reports the
    // missing variant(s) (NonExhaustiveMatch / missing_variants).
    let source = format!(
        r"{EVENTUM_SCRUTINY}

functio describe(Eventum e) → textus {{
    discerne e {{
        casu Eventum.Ictus sit vis {{ redde vis ↦ textus }}
    }}
}}

incipit {{
    sit Eventum a ← finge Ictus {{ vis = 3 }} ∷ Eventum
    sit Eventum b ← Eventum.Sonitus
    nota describe(a) + describe(b)
}}"
    );
    let issues = union_probe_error_issues(&source);
    assert!(
        issues.iter().any(|i| i == "missing_variants"),
        "expected missing_variants for a union match omitting Sonitus, got issues: {issues:?}"
    );
}

#[test]
fn union_omnia_accepts_all_variants_named() {
    // done_when (c): `omnia` over a union scrutinee with all variants named
    // checks green (finite declared variant set).
    let source = format!(
        r#"{EVENTUM_SCRUTINY}

functio describe(Eventum e) → textus {{
    discerne omnia e {{
        casu Eventum.Ictus sit vis {{ redde vis ↦ textus }}
        casu Eventum.Sonitus {{ redde "sonitus" }}
    }}
}}

incipit {{
    sit Eventum a ← finge Ictus {{ vis = 3 }} ∷ Eventum
    sit Eventum b ← Eventum.Sonitus
    nota describe(a) + describe(b)
}}"#
    );
    let diagnostics = union_probe_diagnostics(&source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected omnia over a fully-named union to check green, got: {diagnostics:?}"
    );
}

#[test]
fn union_omnia_forbids_catchall() {
    // Malformed omnia arms keep their current errors: a catchall arm under
    // omnia is forbidden even when the scrutinee is a union.
    let source = format!(
        r#"{EVENTUM_SCRUTINY}

functio describe(Eventum e) → textus {{
    discerne omnia e {{
        casu Eventum.Ictus sit vis {{ redde vis ↦ textus }}
        ceterum {{ redde "alter" }}
    }}
}}

incipit {{
    sit Eventum a ← finge Ictus {{ vis = 3 }} ∷ Eventum
    sit Eventum b ← Eventum.Sonitus
    nota describe(a) + describe(b)
}}"#
    );
    let issues = union_probe_error_issues(&source);
    assert!(
        issues.iter().any(|i| i == "catchall_arm"),
        "expected catchall_arm under omnia over a union, got issues: {issues:?}"
    );
}

#[test]
fn union_omnia_reports_missing_variants() {
    // omnia over a union that omits a variant still reports the missing one.
    let source = format!(
        r"{EVENTUM_SCRUTINY}

functio describe(Eventum e) → textus {{
    discerne omnia e {{
        casu Eventum.Ictus sit vis {{ redde vis ↦ textus }}
    }}
}}

incipit {{
    sit Eventum a ← finge Ictus {{ vis = 3 }} ∷ Eventum
    sit Eventum b ← Eventum.Sonitus
    nota describe(a) + describe(b)
}}"
    );
    let issues = union_probe_error_issues(&source);
    assert!(
        issues.iter().any(|i| i == "missing_variants"),
        "expected missing_variants under omnia over a union, got issues: {issues:?}"
    );
}

// ── D4.8: uncomputable coverage warns, never silent ─────────────────────────

fn coverage_unchecked(errors: &[SemanticError]) -> bool {
    errors.iter().any(|e| {
        e.kind == SemanticErrorKind::Warning(WarningKind::CoverageUnchecked)
            && e.issue() == Some("coverage_unchecked")
    })
}

#[test]
fn enum_without_variant_list_warns_coverage_unchecked() {
    let mut types = TypeTable::new();
    let unknown_enum_ty = types.intern(Type::Enum(DefId(99)));
    let match_expr = match_expr(
        unknown_enum_ty,
        vec![
            HirPattern::Variant(DefId(2), Vec::new()),
            HirPattern::Variant(DefId(3), Vec::new()),
        ],
    );
    let program = program_with_match(DefId(1), match_expr);
    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .err()
    .unwrap_or_default();
    assert!(
        coverage_unchecked(&errors),
        "expected CoverageUnchecked, got {errors:?}"
    );
    assert!(
        !errors.iter().any(SemanticError::is_error),
        "unchecked coverage is a warning, not an error: {errors:?}"
    );
}

#[test]
fn non_binding_payload_subpattern_warns_coverage_unchecked() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr(
        enum_ty,
        vec![
            HirPattern::Variant(DefId(2), vec![HirPattern::Literal(HirLiteral::Int(1))]),
            HirPattern::Variant(DefId(3), Vec::new()),
        ],
    );
    let program = program_with_match(enum_def, match_expr);
    let errors = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    )
    .err()
    .unwrap_or_default();
    assert!(
        coverage_unchecked(&errors),
        "expected CoverageUnchecked, got {errors:?}"
    );
}

#[test]
fn or_pattern_union_covers_each_alternative() {
    let mut types = TypeTable::new();
    let enum_def = DefId(1);
    let enum_ty = types.intern(Type::Enum(enum_def));
    let match_expr = match_expr_with_arms(
        enum_ty,
        vec![vec![HirPattern::Or(vec![
            HirPattern::Variant(DefId(2), Vec::new()),
            HirPattern::Variant(DefId(3), Vec::new()),
        ])]],
    );
    let program = program_with_match(enum_def, match_expr);

    let result = check(
        &program,
        &types,
        &collect_enum_variants(&program),
        &FxHashSet::default(),
        None,
    );
    assert!(
        result.is_ok(),
        "an Or arm covers the union of its alternatives"
    );
}

fn duplicate_literal_errors(source: &str) -> Vec<SemanticError> {
    let result = crate::test_support::analyze_source(source);
    result
        .diagnostics
        .iter()
        .cloned()
        .filter(|err| {
            err.kind == SemanticErrorKind::PatternDuplicate
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "duplicate_literal")
        })
        .collect()
}

fn assert_duplicate_literal_value(source: &str, expected: &str) {
    let errors = duplicate_literal_errors(source);
    assert_eq!(
        errors.len(),
        1,
        "expected one duplicate_literal: {errors:?}"
    );
    assert!(
        errors[0]
            .args
            .iter()
            .any(|arg| arg.name == "value" && arg.value == expected),
        "expected value {expected:?}, got {:?}",
        errors[0].args
    );
}

#[test]
fn repeated_switch_case_value_is_a_check_error_naming_the_value() {
    assert_duplicate_literal_value(
        r"
functio pick(numerus x) → numerus {
    elige x {
        casu 1 { redde 10 }
        casu 1 { redde 11 }
        casu -2 { redde 20 }
        ceterum { redde 0 }
    }
}
",
        "1",
    );
}

#[test]
fn repeated_negated_switch_case_value_names_the_sign() {
    assert_duplicate_literal_value(
        r"
functio pick(numerus x) → numerus {
    elige x {
        casu -2 { redde 20 }
        casu -2 { redde 21 }
        ceterum { redde 0 }
    }
}
",
        "-2",
    );
}

#[test]
fn switch_case_values_differing_only_by_sign_are_distinct() {
    assert!(
        duplicate_literal_errors(
            r"
functio pick(numerus x) → numerus {
    elige x {
        casu 2 { redde 10 }
        casu -2 { redde 20 }
        ceterum { redde 0 }
    }
}
",
        )
        .is_empty()
    );
}
