//! Emitter proofs for the bracket forms of the inferred constructor.
//!
//! Typecheck rewrites `_ [ … ]` and a bare `[ … ]` at a tuple cell into a
//! plain `Array` or `Tuple` node and records the node id in
//! `inferred_constructor_ids`. The hole's authored type form stays under the
//! same id (`AscriptionTarget`). Re-emission must print exactly what was
//! authored, so the authored type-form count of the output matches the input.

use crate::surface::{KeywordSurface, TypeBindingKind};
use crate::{FaberCodegen, empty_presentation};
use radix_hir::{
    HirArrayElement, HirAuthoredTypeUse, HirBlock, HirExpression, HirExpressionKind, HirId,
    HirLiteral, HirModule, HirOwnerRef, HirStatement, HirStatementKind, HirTypeForm,
    HirTypeFormHead, HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_types::{InferVar, Primitive, Type, TypeTable};

struct TestSurface;

impl KeywordSurface for TestSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        key
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

fn int(id: u32, value: u64) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: None,
        span: Span::default(),
    }
}

/// Emit one expression statement as the entry block.
fn emit(
    types: &TypeTable,
    interner: &Interner,
    expr: HirExpression,
    inferred: bool,
    hole: bool,
    infer_ty: radix_types::TypeId,
) -> String {
    let id = expr.id;
    let hir = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(
            vec![HirStatement {
                id: HirId(90),
                kind: HirStatementKind::Expr(expr),
                span: Span::default(),
            }],
            None,
            Span::default(),
        )),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let mut presentation = empty_presentation();
    if inferred {
        presentation.inferred_constructor_ids.push(id);
    }
    if hole {
        presentation.authored_types.push(HirAuthoredTypeUse {
            site: HirTypeSite {
                owner: HirOwnerRef::Hir(id),
                slot: HirTypeSlot::AscriptionTarget,
            },
            form: HirTypeForm {
                ty: infer_ty,
                head: HirTypeFormHead::Infer,
                args: Vec::new(),
                label: None,
            },
        });
    }
    FaberCodegen::new(types, interner)
        .generate(&hir, &presentation, &TestSurface)
        .expect("bracket literal must emit")
        .code
}

struct Fx {
    types: TypeTable,
    interner: Interner,
    infer_ty: radix_types::TypeId,
    tuple_ty: radix_types::TypeId,
}

fn fixture() -> Fx {
    let mut types = TypeTable::new();
    let int_ty = types.primitive(Primitive::Numerus);
    let infer_ty = types.intern(Type::Infer(InferVar(0)));
    let tuple_ty = types.intern(Type::Tuple(vec![int_ty, int_ty]));
    Fx {
        types,
        interner: Interner::new(),
        infer_ty,
        tuple_ty,
    }
}

fn tuple_expr(fx: &Fx) -> HirExpression {
    HirExpression {
        id: HirId(5),
        kind: HirExpressionKind::Tuple(vec![int(6, 1), int(7, 2)], None),
        ty: Some(fx.tuple_ty),
        span: Span::default(),
    }
}

#[test]
fn hole_bracket_at_tuple_cell_reemits_as_hole_bracket() {
    let fx = fixture();
    let code = emit(
        &fx.types,
        &fx.interner,
        tuple_expr(&fx),
        true,
        true,
        fx.infer_ty,
    );
    assert!(
        code.contains("_ [1, 2]"),
        "hole tuple must print `_ [..]`: {code}"
    );
    assert!(
        !code.contains("kw<") && !code.contains("iuncta"),
        "no type prefix: {code}"
    );
}

#[test]
fn bare_bracket_at_tuple_cell_reemits_bare() {
    let fx = fixture();
    let code = emit(
        &fx.types,
        &fx.interner,
        tuple_expr(&fx),
        true,
        false,
        fx.infer_ty,
    );
    assert!(
        code.contains("[1, 2]"),
        "bare tuple literal must print bare: {code}"
    );
    assert!(!code.contains("_ ["), "no hole was authored: {code}");
    assert!(!code.contains("iuncta"), "no type prefix: {code}");
}

#[test]
fn hole_bracket_at_list_cell_reemits_as_hole_bracket() {
    let fx = fixture();
    let array = HirExpression {
        id: HirId(5),
        kind: HirExpressionKind::Array(vec![
            HirArrayElement::Expr(int(6, 1)),
            HirArrayElement::Expr(int(7, 2)),
        ]),
        ty: None,
        span: Span::default(),
    };
    let code = emit(&fx.types, &fx.interner, array, true, true, fx.infer_ty);
    assert!(
        code.contains("_ [1, 2]"),
        "hole list must print `_ [..]`: {code}"
    );
}

#[test]
fn plain_array_literal_is_untouched() {
    let fx = fixture();
    let array = HirExpression {
        id: HirId(5),
        kind: HirExpressionKind::Array(vec![HirArrayElement::Expr(int(6, 1))]),
        ty: None,
        span: Span::default(),
    };
    let code = emit(&fx.types, &fx.interner, array, false, false, fx.infer_ty);
    assert!(
        code.contains("[1]") && !code.contains("_ ["),
        "plain array: {code}"
    );
}

#[test]
fn unmarked_tuple_keeps_its_type_prefix() {
    let fx = fixture();
    let code = emit(
        &fx.types,
        &fx.interner,
        tuple_expr(&fx),
        false,
        false,
        fx.infer_ty,
    );
    assert!(
        code.contains("iuncta<"),
        "authored tuple keeps the prefix: {code}"
    );
}
