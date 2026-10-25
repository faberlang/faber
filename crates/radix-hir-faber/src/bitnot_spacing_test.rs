//! Prefix `¬` spacing law: the glyph operator is spaced
//! like its word sibling `non` — one space before the operand.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{HirBinOp, HirExpression, HirExpressionKind, HirUnOp};
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

fn expr(kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(1),
        kind,
        ty: None,
        span: Span::new(0, 0),
    }
}

fn un(op: HirUnOp, inner: HirExpression) -> HirExpression {
    expr(HirExpressionKind::Unary(op, Box::new(inner)))
}

fn and(l: HirExpression, r: HirExpression) -> HirExpression {
    expr(HirExpressionKind::Binary(
        HirBinOp::BitAnd,
        Box::new(l),
        Box::new(r),
    ))
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
        expr(HirExpressionKind::Path(def_id))
    }

    fn render(&self, expr: &HirExpression) -> String {
        let types = TypeTable::new();
        let codegen = crate::FaberCodegen::new(&types, &self.interner);
        let mut writer = CodeWriter::new();
        codegen.write_expr(expr, &self.names, &LatinSurface, &mut writer);
        writer.finish()
    }
}

#[test]
fn bitnot_spacing_law() {
    let mut ctx = Ctx::new();
    let x = ctx.path("x");
    assert_eq!(ctx.render(&un(HirUnOp::BitNot, x)), "¬ x");
    let a = ctx.path("a");
    let b = ctx.path("b");
    assert_eq!(ctx.render(&un(HirUnOp::BitNot, and(a, b))), "¬ (a ∧ b)");
    let a = ctx.path("a");
    let b = ctx.path("b");
    assert_eq!(ctx.render(&and(a, un(HirUnOp::BitNot, b))), "a ∧ ¬ b");
    // A nested `¬` ties with the operand precedence, so it parenthesizes.
    let x = ctx.path("x");
    assert_eq!(
        ctx.render(&un(HirUnOp::BitNot, un(HirUnOp::BitNot, x))),
        "¬ (¬ x)"
    );
}
