//! Canonical emission of type tests whose operand is a nullable list element.
//!
//! A `TypeCheckTarget` form belongs to the outer type-test expression, not its
//! operand. Preserve that site when an element of `list<int ∪ none>` is tested.

use crate::FaberCodegen;
use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirId, HirOwnerRef, HirTypeForm, HirTypeFormHead,
    HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_types::{Primitive, Type, TypeTable};

struct EnSurface;

impl KeywordSurface for EnSurface {
    fn keyword(&self, _kind: TokenKind) -> &str {
        "keyword"
    }

    fn contextual_keyword(&self, key: &'static str) -> &str {
        match key {
            "est" => "is",
            "non" => "not",
            key => key,
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Numerus) => "int",
            TypeBindingKind::Primitive(Primitive::Nihil) => "none",
            _ => "type",
        }
    }
}

fn render_element_type_test(use_alias_form: bool) -> String {
    let mut types = TypeTable::new();
    let int_ty = types.primitive(Primitive::Numerus);
    let none_ty = types.primitive(Primitive::Nihil);
    let element_ty = types.intern(Type::Union(vec![int_ty, none_ty]));
    let list_ty = types.array(element_ty);
    let bool_ty = types.primitive(Primitive::Bivalens);

    let mut interner = Interner::new();
    let xs_name = interner.intern("xs");
    let alias_name = interner.intern("Count");
    let xs_def = DefId(7);
    let alias_def = DefId(8);
    let target_ty = if use_alias_form {
        types.intern(Type::Alias(alias_def, int_ty))
    } else {
        int_ty
    };

    // This is the element expression `xs[0]`, with the nullable element type
    // that triggered the canonical-emission regression.
    let element = HirExpression {
        id: HirId(4),
        kind: HirExpressionKind::Index(
            Box::new(HirExpression {
                id: HirId(2),
                kind: HirExpressionKind::Path(xs_def),
                ty: Some(list_ty),
                span: Span::default(),
            }),
            Box::new(HirExpression {
                id: HirId(3),
                kind: HirExpressionKind::Literal(radix_hir::HirLiteral::Int(0)),
                ty: Some(int_ty),
                span: Span::default(),
            }),
        ),
        ty: Some(element_ty),
        span: Span::default(),
    };
    let test = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::TypeCheck {
            positive: true,
            expr: Box::new(element),
            ty: target_ty,
        },
        ty: Some(bool_ty),
        span: Span::default(),
    };

    let mut names = FxHashMap::default();
    names.insert(xs_def, xs_name);
    let mut codegen = FaberCodegen::new(&types, &interner);
    if use_alias_form {
        codegen.authored_type_forms.insert(
            HirTypeSite {
                owner: HirOwnerRef::Hir(HirId(1)),
                slot: HirTypeSlot::TypeCheckTarget,
            },
            HirTypeForm {
                ty: target_ty,
                head: HirTypeFormHead::Named {
                    def_id: alias_def,
                    path: vec![alias_name],
                },
                args: Vec::new(),
                label: None,
            },
        );
    }

    let mut writer = CodeWriter::new();
    codegen.write_expr(&test, &names, &EnSurface, &mut writer);
    writer.finish()
}

#[test]
fn type_test_on_nullable_list_element_emits_valid_target_and_uses_its_own_site() {
    assert_eq!(render_element_type_test(false), "xs[0] is int");

    // The distinct authored target spelling proves the site key is the
    // TypeCheck node (id 1), not the list-element operand (id 4).
    assert_eq!(render_element_type_test(true), "xs[0] is Count");
}
