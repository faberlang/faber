//! Range-spacing law for `Intervallum` emission (need, operator
//! ruling 2026-10-17 option A): range glyphs are always space-padded, like
//! every other infix glyph. The retired 91dd7cb9 literal exception is gone, so
//! bare integer endpoints are padded too.

use crate::surface::{KeywordSurface, TypeBindingKind};
use radix_codegen_shared::CodeWriter;
use radix_hir::{HirExpression, HirExpressionKind, HirRangeKind, HirUnOp};
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

fn expr(kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(1),
        kind,
        ty: None,
        span: Span::new(0, 0),
    }
}

fn lit(n: u64) -> HirExpression {
    expr(HirExpressionKind::Literal(radix_hir::HirLiteral::Int(n)))
}

fn neg(inner: HirExpression) -> HirExpression {
    expr(HirExpressionKind::Unary(HirUnOp::Neg, Box::new(inner)))
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

    fn method_call(&mut self, name: &str) -> HirExpression {
        let receiver = self.path("o");
        let sym = self.interner.intern(name);
        expr(HirExpressionKind::MethodCall(
            Box::new(receiver),
            sym,
            Vec::new(),
            Vec::new(),
            None,
        ))
    }

    fn render(&self, expr: &HirExpression) -> String {
        let types = TypeTable::new();
        let codegen = crate::FaberCodegen::new(&types, &self.interner);
        let mut writer = CodeWriter::new();
        codegen.write_expr(expr, &self.names, &LatinSurface, &mut writer);
        writer.finish()
    }
}

fn range(start: HirExpression, end: HirExpression, kind: HirRangeKind) -> HirExpression {
    expr(HirExpressionKind::Intervallum {
        start: Box::new(start),
        end: Box::new(end),
        step: None,
        kind,
    })
}

#[test]
fn literal_pairs_emit_spaced() {
    let ctx = Ctx::new();
    assert_eq!(
        ctx.render(&range(lit(0), lit(5), HirRangeKind::Exclusive)),
        "0 ‥ 5"
    );
    assert_eq!(
        ctx.render(&range(lit(1), lit(10), HirRangeKind::Inclusive)),
        "1 … 10"
    );
    // Unary minus over a literal is one operand in the emitter
    // (`Unary` writes `unop` + operand with no inner spacing, ops.rs Neg => "-"),
    // but the glyph padding does not depend on the endpoint shape.
    assert_eq!(
        ctx.render(&range(neg(lit(1)), lit(5), HirRangeKind::Exclusive)),
        "-1 ‥ 5"
    );
    assert_eq!(
        ctx.render(&range(lit(0), neg(lit(5)), HirRangeKind::Exclusive)),
        "0 ‥ -5"
    );
}

#[test]
fn non_literal_endpoints_emit_spaced() {
    let mut ctx = Ctx::new();
    let n = ctx.path("n");
    let m = ctx.path("m");
    let len = ctx.method_call("length");
    assert_eq!(
        ctx.render(&range(lit(0), n.clone(), HirRangeKind::Exclusive)),
        "0 ‥ n"
    );
    assert_eq!(ctx.render(&range(n, m, HirRangeKind::Inclusive)), "n … m");
    assert_eq!(
        ctx.render(&range(lit(0), len, HirRangeKind::Exclusive)),
        "0 ‥ o.length()"
    );
    // Arithmetic endpoint is not a bare literal.
    let sum = expr(HirExpressionKind::Binary(
        radix_hir::HirBinOp::Add,
        Box::new(lit(1)),
        Box::new(lit(2)),
    ));
    assert_eq!(
        ctx.render(&range(lit(0), sum, HirRangeKind::Exclusive)),
        "0 ‥ 1 + 2"
    );
}

#[test]
fn slices_follow_the_same_spacing_law() {
    let mut ctx = Ctx::new();
    let xs = ctx.path("xs");
    let n = ctx.path("n");
    let spaced = expr(HirExpressionKind::Index(
        Box::new(xs.clone()),
        Box::new(range(lit(0), lit(5), HirRangeKind::Exclusive)),
    ));
    assert_eq!(ctx.render(&spaced), "xs[0 ‥ 5]");
    let variable = expr(HirExpressionKind::Index(
        Box::new(xs.clone()),
        Box::new(range(lit(0), n.clone(), HirRangeKind::Exclusive)),
    ));
    assert_eq!(ctx.render(&variable), "xs[0 ‥ n]");
}

#[test]
fn conversion_targets_follow_the_same_spacing_law() {
    let mut ctx = Ctx::new();
    let n = ctx.path("n");
    let spaced = expr(HirExpressionKind::Conversio {
        route: None,
        source: Box::new(lit(1)),
        target: radix_hir::HirConversioTarget::Intervallum(Box::new(range(
            lit(0),
            lit(10),
            HirRangeKind::Exclusive,
        ))),
        params: Vec::new(),
        recovery: None,
        implicit: None,
    });
    assert_eq!(ctx.render(&spaced), "1 ↦ 0 ‥ 10");
    let variable = expr(HirExpressionKind::Conversio {
        route: None,
        source: Box::new(lit(1)),
        target: radix_hir::HirConversioTarget::Intervallum(Box::new(range(
            lit(0),
            n.clone(),
            HirRangeKind::Exclusive,
        ))),
        params: Vec::new(),
        recovery: None,
        implicit: None,
    });
    assert_eq!(ctx.render(&variable), "1 ↦ 0 ‥ n");
}
