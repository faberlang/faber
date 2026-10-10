//! The list window gate (ruling B10 of need): `xs[lo‥hi]` on a list is
//! refused with `list_window_unsupported_on_target` on every emitter except the
//! Faber re-emitter and the MIR runner; scalar indexing and the octeti window
//! are untouched.

use super::*;
use radix_hir::{HirBlock, HirId, HirLiteral, HirStatement, HirStatementKind};
use radix_lexer::Span;
use radix_types::Primitive;

fn expr(kind: HirExpressionKind, ty: Option<TypeId>) -> HirExpression {
    HirExpression {
        id: HirId(1),
        kind,
        ty,
        span: Span::new(0, 0),
    }
}

fn module_of(expression: HirExpression) -> HirModule {
    let statement = HirStatement {
        id: HirId(2),
        kind: HirStatementKind::Expr(expression),
        span: Span::new(0, 0),
    };
    HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(vec![statement], None, Span::new(0, 0))),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

/// `object[index]` with `object` of `object_ty` and `index` of `index_ty`.
fn index_module(object_ty: TypeId, index_ty: TypeId) -> HirModule {
    let object = expr(
        HirExpressionKind::Literal(HirLiteral::Int(0)),
        Some(object_ty),
    );
    let index = expr(
        HirExpressionKind::Literal(HirLiteral::Int(0)),
        Some(index_ty),
    );
    module_of(expr(
        HirExpressionKind::Index(Box::new(object), Box::new(index)),
        None,
    ))
}

fn issue_of(error: &CodegenError) -> Option<&str> {
    error
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str())
}

#[test]
fn list_window_is_rejected_by_name_on_a_source_target() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let list = types.array(int);
    let range = types.intervallum(int);
    let module = index_module(list, range);
    for target in [
        Target::HirRust,
        Target::HirTypeScript,
        Target::HirGo,
        Target::MirWasm,
    ] {
        let error = reject_list_window(&module, &types, target).expect_err("list window refused");
        assert_eq!(issue_of(&error), Some("list_window_unsupported_on_target"));
        assert!(
            error
                .args
                .iter()
                .any(|arg| arg.name == "target" && arg.value == format!("{target:?}")),
            "{target:?}: {error:?}"
        );
    }
}

#[test]
fn list_window_is_admitted_by_the_faber_emitter_and_the_runner() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let list = types.array(int);
    let range = types.intervallum(int);
    let module = index_module(list, range);
    reject_list_window(&module, &types, Target::HirFaber).expect("faber re-emits the window");
    reject_list_window(&module, &types, Target::MirStepper)
        .expect("the runner executes the window");
}

#[test]
fn scalar_list_index_and_octeti_window_are_untouched() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let list = types.array(int);
    let range = types.intervallum(int);
    let octeti = types.primitive(Primitive::Octeti);
    reject_list_window(&index_module(list, int), &types, Target::HirRust)
        .expect("a scalar list index is not a window");
    reject_list_window(&index_module(octeti, range), &types, Target::HirRust)
        .expect("the octeti window is not a list window");
}
