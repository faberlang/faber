//! NR-6 emitter proofs for the Cartesian `itera ab` range-product series.
//!
//! HIR→Faber emission must preserve the declared source/binder series with
//! contextual locale spellings (`ab`/`range`, `fixum`/`const`, `varia`/`var`), and the
//! emitted head must re-parse through the same series shape — never the
//! `ex` lockstep mode and never a collapsed single source.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirIteraMode, HirLiteral, HirRangeKind,
};
use radix_lexer::{Interner, KeywordPack, Span, Symbol, TokenKind};
use radix_syntax::{BindingPattern, IteraMode, StmtKind};
use radix_types::{DefId, TypeTable, USER_DEF_ID_BASE};

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, kind: TokenKind) -> &'static str {
        match kind {
            TokenKind::Itera => "itera",
            TokenKind::Ex => "ex",
            TokenKind::Apud => "apud",
            _ => "kw",
        }
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        key
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

/// English reader surface mirroring the `en` pack iteration rows
/// (`itera = "for"`, `ex = "from"`, `apud = "at"`, `ab = "range"`,
/// `fixum = "const"`).
struct EnSurface;

impl KeywordSurface for EnSurface {
    fn keyword(&self, kind: TokenKind) -> &'static str {
        match kind {
            TokenKind::Itera => "for",
            TokenKind::Ex => "from",
            TokenKind::Apud => "at",
            _ => "kw",
        }
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        match key {
            "ab" => "range",
            "fixum" => "const",
            "sit" => "let",
            "varia" => "mut",
            // The `en` pack's filum row (`filum = "thread"`), as emitted by
            // the locale-backed surface bridge.
            "filum" => "thread",
            key => key,
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

/// Minimal `en` pack for re-parsing emitted English heads: only the
/// spellings the iteration head itself carries need rows.
struct EnPack;

impl KeywordPack for EnPack {
    fn pack_id(&self) -> &'static str {
        "en-itera"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "for" => Some("itera"),
            "from" => Some("ex"),
            "at" => Some("apud"),
            "range" => Some("ab"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "mut" => Some("varia"),
            "thread" => Some("filum"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
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

fn int_range(start: u64, end: u64) -> HirExpression {
    expr(HirExpressionKind::Intervallum {
        start: Box::new(expr(HirExpressionKind::Literal(HirLiteral::Int(start)))),
        end: Box::new(expr(HirExpressionKind::Literal(HirLiteral::Int(end)))),
        step: None,
        kind: HirRangeKind::Exclusive,
    })
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

    fn binder(&mut self, name: &str) -> (DefId, Symbol) {
        let next: u32 = self
            .names
            .len()
            .try_into()
            .expect("test def ids fit in u32");
        let def_id = DefId(USER_DEF_ID_BASE + next);
        let sym = self.interner.intern(name);
        self.names.insert(def_id, sym);
        (def_id, sym)
    }

    fn range_product(&mut self, ranges: Vec<HirExpression>, binders: &[&str]) -> HirExpression {
        let bindings = binders.iter().map(|name| self.binder(name)).collect();
        expr(HirExpressionKind::IteraRange {
            ranges,
            bindings,
            body: HirBlock::simple(Vec::new(), None, Span::new(0, 0)),
            mutable: false,
        })
    }

    fn render(&self, product: &HirExpression, surface: &impl KeywordSurface) -> String {
        let types = TypeTable::new();
        let codegen = crate::FaberCodegen::new(&types, &self.interner);
        let mut writer = CodeWriter::new();
        codegen.write_expr(product, &self.names, surface, &mut writer);
        writer.finish()
    }
}

/// Emit → re-parse: the head must come back as mode `ab` with the exact
/// source and binder counts, proving no collapse to `ex` or one source.
fn assert_round_trips(code: &str, sources: usize, binders: usize, english: bool) {
    let lexed = if english {
        radix_lexer::lex_with_locale_pack(code, &EnPack)
    } else {
        radix_lexer::lex(code)
    };
    let parsed = radix_parser::parse(lexed);
    assert!(
        parsed.success(),
        "emitted range product must re-parse ({sources} sources), got:\n{code}\nerrors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let StmtKind::Itera(iter) = &program.statements[0].kind else {
        panic!("expected itera statement, got:\n{code}");
    };
    assert_eq!(
        std::mem::discriminant(&iter.mode),
        std::mem::discriminant(&IteraMode::Ab),
        "emitted head must keep the ab mode, got:\n{code}"
    );
    assert_eq!(iter.sources.len(), sources, "source count, got:\n{code}");
    assert_eq!(iter.bindings.len(), binders, "binder count, got:\n{code}");
    assert!(
        iter.bindings
            .iter()
            .all(|b| matches!(b, BindingPattern::Ident(_))),
        "series binders must stay name binders, got:\n{code}"
    );
}

#[test]
fn latin_two_series_round_trips() {
    let mut ctx = Ctx::new();
    let product = ctx.range_product(vec![int_range(0, 3), int_range(5, 7)], &["i", "j"]);
    let code = ctx.render(&product, &LatinSurface);
    assert!(
        code.starts_with("itera ab 0 ‥ 3, 5 ‥ 7 sit i, j {"),
        "N=2 Latin head must preserve the series, got:\n{code}"
    );
    assert_round_trips(&code, 2, 2, false);
}

#[test]
fn latin_three_series_round_trips() {
    let mut ctx = Ctx::new();
    let product = ctx.range_product(
        vec![int_range(0, 2), int_range(4, 6), int_range(8, 10)],
        &["i", "j", "p"],
    );
    let code = ctx.render(&product, &LatinSurface);
    assert!(
        code.starts_with("itera ab 0 ‥ 2, 4 ‥ 6, 8 ‥ 10 sit i, j, p {"),
        "N=3 Latin head must preserve the series, got:\n{code}"
    );
    assert_round_trips(&code, 3, 3, false);
}

#[test]
fn english_two_series_round_trips() {
    let mut ctx = Ctx::new();
    let product = ctx.range_product(vec![int_range(0, 3), int_range(5, 7)], &["i", "j"]);
    let code = ctx.render(&product, &EnSurface);
    assert!(
        code.starts_with("for range 0 ‥ 3, 5 ‥ 7 let i, j {"),
        "N=2 English head must keep the documented `for range` form, got:\n{code}"
    );
    assert!(
        !code.contains(" from "),
        "English head must not collapse to the ex lockstep mode, got:\n{code}"
    );
    assert_round_trips(&code, 2, 2, true);
}

#[test]
fn english_three_series_round_trips() {
    let mut ctx = Ctx::new();
    let product = ctx.range_product(
        vec![int_range(0, 2), int_range(4, 6), int_range(8, 10)],
        &["i", "j", "p"],
    );
    let code = ctx.render(&product, &EnSurface);
    assert!(
        code.starts_with("for range 0 ‥ 2, 4 ‥ 6, 8 ‥ 10 let i, j, p {"),
        "N=3 English head must keep the documented `for range` form, got:\n{code}"
    );
    assert_round_trips(&code, 3, 3, true);
}

/// A mutable authored binder (`varia`) must survive the emit; it is the same
/// fact the lowering carries on the node, not a presentation choice.
fn mutable_range_product(ctx: &mut Ctx) -> HirExpression {
    let mut product = ctx.range_product(vec![int_range(0, 3), int_range(5, 7)], &["i", "j"]);
    let HirExpressionKind::IteraRange { mutable, .. } = &mut product.kind else {
        panic!("range product must be IteraRange");
    };
    *mutable = true;
    product
}

#[test]
fn latin_mutable_binder_emits_varia() {
    let mut ctx = Ctx::new();
    let product = mutable_range_product(&mut ctx);
    let code = ctx.render(&product, &LatinSurface);
    assert!(
        code.starts_with("itera ab 0 ‥ 3, 5 ‥ 7 varia i, j {"),
        "mutable Latin head must keep varia, got:\n{code}"
    );
    assert_round_trips(&code, 2, 2, false);
}

#[test]
fn english_mutable_binder_emits_mut() {
    let mut ctx = Ctx::new();
    let product = mutable_range_product(&mut ctx);
    let code = ctx.render(&product, &EnSurface);
    assert!(
        code.starts_with("for range 0 ‥ 3, 5 ‥ 7 mut i, j {"),
        "mutable English head must keep mut, got:\n{code}"
    );
    assert_round_trips(&code, 2, 2, true);
}

#[test]
fn filum_lane_binder_round_trips_on_single_source_itera() {
    // BUG-4D4D0AE9 (K2): the declared lane binder re-emits in the shared
    // clause position (after `apud`, before the mutability marker) in both
    // surfaces, and the emitted head re-parses with the binder preserved.
    let build = |ctx: &mut Ctx| {
        let (binding, binding_name) = ctx.binder("v");
        let (filum_def, filum_name) = ctx.binder("f");
        let coordinates = vec![ctx.binder("i")];
        expr(HirExpressionKind::Itera {
            mode: HirIteraMode::Ex,
            coordinates,
            filum: Some((filum_def, filum_name)),
            binding,
            binding_name,
            iter: Box::new(int_range(0, 3)),
            body: HirBlock::simple(Vec::new(), None, Span::new(0, 0)),
            mutable: false,
        })
    };

    let mut ctx = Ctx::new();
    let lane_itera = build(&mut ctx);
    let code = ctx.render(&lane_itera, &LatinSurface);
    assert!(
        code.starts_with("itera ex 0 ‥ 3 apud [i] filum f sit v {"),
        "Latin head must keep the lane-binder clause, got:\n{code}"
    );
    let lexed = radix_lexer::lex(&code);
    let parsed = radix_parser::parse(lexed);
    assert!(
        parsed.success(),
        "emitted Latin head must re-parse, got:\n{code}\nerrors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let StmtKind::Itera(iter) = &program.statements[0].kind else {
        panic!("expected itera statement, got:\n{code}");
    };
    let filum = iter.filum.as_ref().expect("filum clause survives re-parse");
    assert_eq!(parsed.interner.resolve(filum.name), "f");

    let mut ctx = Ctx::new();
    let lane_itera = build(&mut ctx);
    let code = ctx.render(&lane_itera, &EnSurface);
    assert!(
        code.starts_with("for from 0 ‥ 3 at [i] thread f let v {"),
        "English head must project the pack's filum row, got:\n{code}"
    );
    let lexed = radix_lexer::lex_with_locale_pack(&code, &EnPack);
    let parsed = radix_parser::parse(lexed);
    assert!(
        parsed.success(),
        "emitted English head must re-parse, got:\n{code}\nerrors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let StmtKind::Itera(iter) = &program.statements[0].kind else {
        panic!("expected itera statement, got:\n{code}");
    };
    let filum = iter.filum.as_ref().expect("filum clause survives re-parse");
    assert_eq!(parsed.interner.resolve(filum.name), "f");
}
