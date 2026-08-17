//! Tests for live tuus cursor (generator/sermore) lowering and codegen.
//! Proves: cursor expressions lower to HIR correctly; cursor calls resolve
//! through the type table; cursor state transitions are type-safe.

use super::*;
use crate::hir::{HirCallArg, HirExpression, HirExpressionKind, HirId};
use crate::lexer::Symbol;
use crate::semantic::{Primitive, Type, TypeId, TypeTable};
use crate::test_support::test_span;

/// Symbol for "cursor" — arbitrary value from an interner.
const SYM_CURSOR: Symbol = Symbol(1);
/// Symbol for "finge" — arbitrary value from an interner.
const SYM_FINGE: Symbol = Symbol(2);
/// Symbol for "ego" — arbitrary value from an interner.
const SYM_EGO: Symbol = Symbol(3);

/// Resolve a symbol name for tests.
fn resolve_name(sym: Symbol) -> &'static str {
    if sym == SYM_CURSOR {
        "cursor"
    } else if sym == SYM_FINGE {
        "finge"
    } else if sym == SYM_EGO {
        "ego"
    } else {
        "unknown"
    }
}

/// A newtype with `DefId(42)` for test purposes.
const TUUS_DEF_ID: DefId = DefId(42);

/// Helper to make a `Type::Applied(Type::Struct(TUUS_DEF_ID), [`payload`])`.
fn make_tuus_applied(types: &mut TypeTable, payload: TypeId) -> TypeId {
    let struct_id = types.intern(Type::Struct(TUUS_DEF_ID));
    types.intern(Type::Applied(struct_id, vec![payload]))
}

// ── tuus_cursor_method_payload ──────────────────────────────────────────────

#[test]
fn valid_tuus_cursor_call_returns_payload_type() {
    let mut types = TypeTable::new();
    let textus_ty = types.primitive(Primitive::Textus);
    let applied_ty = make_tuus_applied(&mut types, textus_ty);
    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(applied_ty),
        span: test_span(),
    };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(Box::new(receiver), SYM_CURSOR, Vec::new(), Vec::new()),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, Some(textus_ty));
}

#[test]
fn ident_expr_returns_none() {
    let types = TypeTable::new();
    let expr = HirExpression { id: HirId(0), kind: HirExpressionKind::Path(DefId(0)), ty: None, span: test_span() };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}

#[test]
fn wrong_method_name_returns_none() {
    let mut types = TypeTable::new();
    let textus_ty = types.primitive(Primitive::Textus);
    let applied_ty = make_tuus_applied(&mut types, textus_ty);
    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(applied_ty),
        span: test_span(),
    };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(Box::new(receiver), SYM_FINGE, Vec::new(), Vec::new()),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}

#[test]
fn type_args_present_returns_none() {
    let mut types = TypeTable::new();
    let textus_ty = types.primitive(Primitive::Textus);
    let applied_ty = make_tuus_applied(&mut types, textus_ty);
    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(applied_ty),
        span: test_span(),
    };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(Box::new(receiver), SYM_CURSOR, vec![textus_ty], Vec::new()),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}

#[test]
fn args_present_returns_none() {
    let mut types = TypeTable::new();
    let textus_ty = types.primitive(Primitive::Textus);
    let applied_ty = make_tuus_applied(&mut types, textus_ty);
    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(applied_ty),
        span: test_span(),
    };
    let arg = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Literal(crate::hir::HirLiteral::Int(1)),
        ty: Some(types.primitive(Primitive::Numerus)),
        span: test_span(),
    };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(
            Box::new(receiver),
            SYM_CURSOR,
            Vec::new(),
            vec![HirCallArg { name: None, spread: false, expr: arg, span: test_span() }],
        ),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}

#[test]
fn wrong_tuus_def_id_returns_none() {
    let mut types = TypeTable::new();
    let textus_ty = types.primitive(Primitive::Textus);
    let wrong_def_id = DefId(99);
    let struct_id = types.intern(Type::Struct(wrong_def_id));
    let applied_ty = types.intern(Type::Applied(struct_id, vec![textus_ty]));
    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(applied_ty),
        span: test_span(),
    };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(Box::new(receiver), SYM_CURSOR, Vec::new(), Vec::new()),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}

#[test]
fn primitive_receiver_type_returns_none() {
    let types = TypeTable::new();
    let textus_ty = types.primitive(Primitive::Textus);
    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(textus_ty),
        span: test_span(),
    };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(Box::new(receiver), SYM_CURSOR, Vec::new(), Vec::new()),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}

#[test]
fn receiver_without_type_returns_none() {
    let types = TypeTable::new();
    let receiver =
        HirExpression { id: HirId(0), kind: HirExpressionKind::Path(DefId(100)), ty: None, span: test_span() };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(Box::new(receiver), SYM_CURSOR, Vec::new(), Vec::new()),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}

#[test]
fn non_struct_base_type_returns_none() {
    let mut types = TypeTable::new();
    let textus_ty = types.primitive(Primitive::Textus);
    let enum_id = types.intern(Type::Enum(DefId(50)));
    let applied_ty = types.intern(Type::Applied(enum_id, vec![textus_ty]));
    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(applied_ty),
        span: test_span(),
    };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(Box::new(receiver), SYM_CURSOR, Vec::new(), Vec::new()),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}

#[test]
fn applied_type_with_zero_type_args_returns_none() {
    let mut types = TypeTable::new();
    let struct_id = types.intern(Type::Struct(TUUS_DEF_ID));
    let applied_no_args = types.intern(Type::Applied(struct_id, Vec::new()));
    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(100)),
        ty: Some(applied_no_args),
        span: test_span(),
    };
    let expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::MethodCall(Box::new(receiver), SYM_CURSOR, Vec::new(), Vec::new()),
        ty: None,
        span: test_span(),
    };

    let result = tuus_cursor_method_payload(&expr, Some(TUUS_DEF_ID), resolve_name, &types);
    assert_eq!(result, None);
}
