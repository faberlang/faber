//! Instance-literal brace-spacing law: non-empty single-line instance
//! literals get one space inside each brace (`Session { position = 0 }`);
//! empty instances stay tight (`Session {}`).

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{HirCallArg, HirExpression, HirExpressionKind, HirLiteral};
use radix_lexer::{Interner, Span, Symbol, TokenKind};
use radix_types::{DefId, TypeTable, USER_DEF_ID_BASE};

struct TestSurface;

impl KeywordSurface for TestSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "keyword"
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
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
    expr(HirExpressionKind::Literal(HirLiteral::Int(n)))
}

struct Ctx {
    types: TypeTable,
    interner: Interner,
    names: FxHashMap<DefId, Symbol>,
}

impl Ctx {
    fn new() -> Self {
        Self {
            types: TypeTable::new(),
            interner: Interner::new(),
            names: FxHashMap::default(),
        }
    }

    /// A code generator borrowing this context's tables.
    fn codegen(&self) -> crate::FaberCodegen<'_> {
        crate::FaberCodegen::new(&self.types, &self.interner)
    }

    fn def(&mut self, name: &str) -> DefId {
        let next: u32 = self
            .names
            .len()
            .try_into()
            .expect("test def ids fit in u32");
        let def_id = DefId(USER_DEF_ID_BASE + next);
        let sym = self.interner.intern(name);
        self.names.insert(def_id, sym);
        def_id
    }

    fn field(&mut self, name: &str, value: HirExpression) -> (Symbol, HirExpression) {
        (self.interner.intern(name), value)
    }

    fn render(&self, expr: &HirExpression) -> String {
        let codegen = self.codegen();
        let mut writer = CodeWriter::new();
        codegen.write_expr(expr, &self.names, &TestSurface, &mut writer);
        writer.finish()
    }
}

#[test]
fn struct_instance_nonempty_gets_inner_brace_spaces() {
    let mut ctx = Ctx::new();
    let def_id = ctx.def("Session");
    let fields = vec![ctx.field("position", lit(0)), ctx.field("context", lit(0))];
    assert_eq!(
        ctx.render(&expr(HirExpressionKind::Struct(def_id, fields))),
        "Session { position = 0, context = 0 }"
    );
}

#[test]
fn struct_instance_empty_stays_tight() {
    let mut ctx = Ctx::new();
    let def_id = ctx.def("Session");
    assert_eq!(
        ctx.render(&expr(HirExpressionKind::Struct(def_id, Vec::new()))),
        "Session {}"
    );
}

#[test]
fn finge_variant_named_fields_get_inner_brace_spaces() {
    let mut ctx = Ctx::new();
    let def_id = ctx.def("DtypeMismatch");
    let key = ctx.interner.intern("message");
    let value = ctx.interner.intern("boom");
    let arg = HirCallArg {
        name: Some(key),
        spread: false,
        marker: None,
        expr: expr(HirExpressionKind::Literal(HirLiteral::String(value))),
        span: Span::new(0, 0),
    };
    let call = expr(HirExpressionKind::Call(
        Box::new(expr(HirExpressionKind::Path(def_id))),
        Vec::new(),
        vec![arg],
        None,
    ));
    let mut codegen = ctx.codegen();
    codegen.variant_def_ids.insert(def_id);
    let mut writer = CodeWriter::new();
    codegen.write_expr(&call, &ctx.names, &TestSurface, &mut writer);
    assert_eq!(
        writer.finish(),
        "keyword DtypeMismatch { message = \"boom\" }"
    );
}
