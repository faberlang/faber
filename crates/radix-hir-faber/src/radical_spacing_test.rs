//! Regression coverage for radical glyph restoration, spacing, and operand grouping.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{HirBinOp, HirExpression, HirExpressionKind};
use radix_lexer::{Interner, Span, Symbol, TokenKind};
use radix_types::{DefId, TypeTable, USER_DEF_ID_BASE};

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "keyword"
    }

    fn contextual_keyword(&self, key: &'static str) -> &str {
        key
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

fn expr_with_id(id: radix_hir::HirId, kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id,
        kind,
        ty: None,
        span: Span::new(0, 0),
    }
}

fn and(left: HirExpression, right: HirExpression) -> HirExpression {
    expr_with_id(
        radix_hir::HirId(4),
        HirExpressionKind::Binary(HirBinOp::BitAnd, Box::new(left), Box::new(right)),
    )
}

struct Ctx {
    interner: Interner,
    names: FxHashMap<DefId, Symbol>,
}

impl Ctx {
    fn new() -> Self {
        Self {
            interner: Interner::new(),
            names: FxHashMap::default(),
        }
    }

    fn path(&mut self, name: &str) -> HirExpression {
        let next: u32 = self
            .names
            .len()
            .try_into()
            .expect("test def ids fit in u32");
        let def_id = DefId(USER_DEF_ID_BASE + next);
        let sym = self.interner.intern(name);
        self.names.insert(def_id, sym);
        expr_with_id(radix_hir::HirId(0), HirExpressionKind::Path(def_id))
    }

    fn method_call(
        &mut self,
        receiver: HirExpression,
        name: &str,
        id: radix_hir::HirId,
    ) -> HirExpression {
        expr_with_id(
            id,
            HirExpressionKind::MethodCall(
                Box::new(receiver),
                self.interner.intern(name),
                Vec::new(),
                Vec::new(),
                None,
            ),
        )
    }

    fn render(
        &self,
        expr: &HirExpression,
        glyph_ids: &[radix_hir::HirId],
        quarta_ids: &[radix_hir::HirId],
    ) -> String {
        let types = TypeTable::new();
        let mut codegen = crate::FaberCodegen::new(&types, &self.interner);
        for id in glyph_ids {
            codegen.glyph_ids.insert(*id);
        }
        for id in quarta_ids {
            codegen.radical_quarta_ids.insert(*id);
        }
        let mut writer = CodeWriter::new();
        codegen.write_expr(expr, &self.names, &LatinSurface, &mut writer);
        writer.finish()
    }
}

#[test]
fn radical_cbrt_prints_spaced() {
    let mut ctx = Ctx::new();
    let x = ctx.path("x");
    let call = ctx.method_call(x, "radix_cubica", radix_hir::HirId(1));

    assert_eq!(ctx.render(&call, &[radix_hir::HirId(1)], &[]), "∛ x");
}

#[test]
fn radical_parenthesizes_compound_operand() {
    let mut ctx = Ctx::new();
    let mut a = ctx.path("a");
    a.id = radix_hir::HirId(2);
    let mut b = ctx.path("b");
    b.id = radix_hir::HirId(3);
    let receiver = and(a, b);
    let call = ctx.method_call(receiver, "radix", radix_hir::HirId(1));

    assert_eq!(ctx.render(&call, &[radix_hir::HirId(1)], &[]), "√ (a ∧ b)");
}

#[test]
fn radical_sqrt_prints_spaced() {
    let mut ctx = Ctx::new();
    let x = ctx.path("x");
    let call = ctx.method_call(x, "radix", radix_hir::HirId(1));

    assert_eq!(ctx.render(&call, &[radix_hir::HirId(1)], &[]), "√ x");
}

#[test]
fn radical_fourth_root_restores_outer_glyph() {
    let mut ctx = Ctx::new();
    let x = ctx.path("x");
    let inner = ctx.method_call(x, "radix", radix_hir::HirId(5));
    let outer = ctx.method_call(inner, "radix", radix_hir::HirId(1));

    assert_eq!(ctx.render(&outer, &[], &[radix_hir::HirId(1)]), "∜ x");
}

#[test]
fn radical_fourth_root_parenthesizes_compound_operand() {
    let mut ctx = Ctx::new();
    let mut a = ctx.path("a");
    a.id = radix_hir::HirId(2);
    let mut b = ctx.path("b");
    b.id = radix_hir::HirId(3);
    let inner = ctx.method_call(and(a, b), "radix", radix_hir::HirId(5));
    let outer = ctx.method_call(inner, "radix", radix_hir::HirId(1));

    assert_eq!(ctx.render(&outer, &[], &[radix_hir::HirId(1)]), "∜ (a ∧ b)");
}
