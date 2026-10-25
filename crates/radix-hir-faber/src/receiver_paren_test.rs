//! Receiver parenthesization for postfix forms: a method
//! call, field access or index whose receiver binds looser than the postfix
//! chain prints the receiver in parentheses; atomic receivers stay bare. A
//! rounding call whose receiver would be parenthesized prints as its glyph
//! when it heads an output statement, where a leading `(` reads as a call.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    HirBinOp, HirCallArg, HirExpression, HirExpressionKind, HirId, HirLiteral, HirScribeKind,
    HirUnOp,
};
use radix_lexer::{Interner, Span, Symbol, TokenKind};
use radix_types::{DefId, TypeTable, USER_DEF_ID_BASE};

struct TestSurface;

impl KeywordSurface for TestSurface {
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
        id: HirId(1),
        kind,
        ty: None,
        span: Span::new(0, 0),
    }
}

fn float(value: f64) -> HirExpression {
    expr(HirExpressionKind::Literal(HirLiteral::Float(value)))
}

fn int(value: u64) -> HirExpression {
    expr(HirExpressionKind::Literal(HirLiteral::Int(value)))
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
        let next: u32 = self.names.len().try_into().expect("test def ids fit");
        let def_id = DefId(USER_DEF_ID_BASE + next);
        let sym = self.interner.intern(name);
        self.names.insert(def_id, sym);
        expr(HirExpressionKind::Path(def_id))
    }

    fn method(&mut self, receiver: HirExpression, name: &str) -> HirExpression {
        let sym = self.interner.intern(name);
        expr(HirExpressionKind::MethodCall(
            Box::new(receiver),
            sym,
            Vec::new(),
            Vec::<HirCallArg>::new(),
            None,
        ))
    }

    fn field(&mut self, object: HirExpression, name: &str) -> HirExpression {
        let sym = self.interner.intern(name);
        expr(HirExpressionKind::Field(Box::new(object), sym))
    }

    fn render(&self, expr: &HirExpression) -> String {
        let types = TypeTable::new();
        let codegen = crate::FaberCodegen::new(&types, &self.interner);
        let mut writer = CodeWriter::new();
        codegen.write_expr(expr, &self.names, &TestSurface, &mut writer);
        writer.finish()
    }
}

fn sum(lhs: HirExpression, rhs: HirExpression) -> HirExpression {
    expr(HirExpressionKind::Binary(
        HirBinOp::Add,
        Box::new(lhs),
        Box::new(rhs),
    ))
}

#[test]
fn receiver_negative_literal_is_parenthesized() {
    let mut ctx = Ctx::new();
    let call = ctx.method(neg(float(2.5)), "abs");
    assert_eq!(ctx.render(&call), "(-2.5).abs()");
}

#[test]
fn receiver_unary_minus_on_name_is_parenthesized() {
    let mut ctx = Ctx::new();
    let x = ctx.path("x");
    let call = ctx.method(neg(x), "abs");
    assert_eq!(ctx.render(&call), "(-x).abs()");
}

#[test]
fn receiver_binary_is_parenthesized() {
    let mut ctx = Ctx::new();
    let a = ctx.path("a");
    let b = ctx.path("b");
    let call = ctx.method(sum(a, b), "abs");
    assert_eq!(ctx.render(&call), "(a + b).abs()");
}

#[test]
fn receiver_of_field_access_is_parenthesized() {
    let mut ctx = Ctx::new();
    let x = ctx.path("x");
    let access = ctx.field(neg(x), "re");
    assert_eq!(ctx.render(&access), "(-x).re");
}

#[test]
fn receiver_of_index_is_parenthesized() {
    let mut ctx = Ctx::new();
    let a = ctx.path("a");
    let b = ctx.path("b");
    let indexed = expr(HirExpressionKind::Index(
        Box::new(sum(a, b)),
        Box::new(int(0)),
    ));
    assert_eq!(ctx.render(&indexed), "(a + b)[0]");
}

#[test]
fn receiver_atomic_forms_stay_bare() {
    let mut ctx = Ctx::new();
    let x = ctx.path("x");
    let name_call = ctx.method(x, "abs");
    assert_eq!(ctx.render(&name_call), "x.abs()");
    let literal_call = ctx.method(float(2.5), "abs");
    assert_eq!(ctx.render(&literal_call), "2.5.abs()");
    let inner = ctx.path("o");
    let first = ctx.method(inner, "first");
    let chained = ctx.method(first, "abs");
    assert_eq!(ctx.render(&chained), "o.first().abs()");
}

#[test]
fn receiver_rounding_call_heading_output_prints_glyph() {
    let mut ctx = Ctx::new();
    let nearest = ctx.method(neg(float(2.5)), "rotunda");
    let heading = expr(HirExpressionKind::Scribe(
        HirScribeKind::Nota,
        vec![nearest.clone()],
    ));
    assert_eq!(ctx.render(&heading), "nota ⌊-2.5⌉");
    // Outside the head position the method form stays.
    let trailing = expr(HirExpressionKind::Scribe(
        HirScribeKind::Nota,
        vec![int(1), nearest],
    ));
    assert_eq!(ctx.render(&trailing), "nota 1, (-2.5).rotunda()");
    // An atomic receiver keeps the method form even at the head.
    let x = ctx.path("x");
    let floor = ctx.method(x, "pavimentum");
    let bare = expr(HirExpressionKind::Scribe(HirScribeKind::Nota, vec![floor]));
    assert_eq!(ctx.render(&bare), "nota x.pavimentum()");
}
