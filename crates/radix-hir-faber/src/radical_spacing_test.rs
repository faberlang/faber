//! Spacing law for glyph-marked square-root method calls.

use crate::surface::{KeywordSurface, TypeBindingKind};
use radix_codegen_shared::CodeWriter;
use radix_hir::{HirExpression, HirExpressionKind};
use radix_lexer::{Interner, Span, Symbol, TokenKind};
use radix_types::{DefId, TypeTable, USER_DEF_ID_BASE};
use rustc_hash::FxHashMap;

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
fn radical_sqrt_prints_spaced() {
    let mut ctx = Ctx::new();
    let x = ctx.path("x");
    let call = ctx.method_call(x, "radix", radix_hir::HirId(1));

    assert_eq!(ctx.render(&call, &[radix_hir::HirId(1)], &[]), "√ x");
}
