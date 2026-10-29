//! Emitter proofs for the `fac omnia` (en `do all`) surface marker.
//!
//! The marker is surface-only: `HirBreakableKind::FacOmnia` keeps the plain
//! `fac` breakable semantics and exists so HIR→Faber emission round-trips the
//! authored form. Each emitted shape must re-parse with the marker set — a
//! dropped `omnia` would silently rewrite the author's source.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    HirBlock, HirBreakableKind, HirCape, HirExpression, HirExpressionKind, HirId, HirLiteral,
    HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, KeywordPack, Span, Symbol, TokenKind};
use radix_syntax::StmtKind;
use radix_types::{DefId, TypeTable, USER_DEF_ID_BASE};

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, kind: TokenKind) -> &'static str {
        match kind {
            TokenKind::Cape => "cape",
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

/// English reader surface mirroring the `en` pack rows (`fac = "do"`,
/// `omnia = "all"`, `cape = "catch"`).
struct EnSurface;

impl KeywordSurface for EnSurface {
    fn keyword(&self, kind: TokenKind) -> &'static str {
        match kind {
            TokenKind::Cape => "catch",
            _ => "kw",
        }
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        match key {
            "fac" => "do",
            "omnia" => "all",
            "machina" => "device",
            key => key,
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

/// Minimal `en` pack for re-parsing the emitted English form.
struct EnPack;

impl KeywordPack for EnPack {
    fn pack_id(&self) -> &'static str {
        "en-fac-omnia"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "do" => Some("fac"),
            "all" => Some("omnia"),
            "device" => Some("machina"),
            "catch" => Some("cape"),
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

fn int_stmt(value: u64) -> HirStatement {
    HirStatement {
        id: HirId(2),
        kind: HirStatementKind::Expr(expr(HirExpressionKind::Literal(HirLiteral::Int(value)))),
        span: Span::new(0, 0),
    }
}

fn fac_block(kind: HirBreakableKind, value: u64) -> HirBlock {
    let mut block = HirBlock::simple(vec![int_stmt(value)], None, Span::new(0, 0));
    block.breakable = Some(kind);
    block
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

    fn cape(&mut self, name: &str, value: u64) -> HirCape {
        HirCape {
            binding_def_id: DefId(USER_DEF_ID_BASE),
            binding_name: self.interner.intern(name),
            binding_ty: None,
            body: HirBlock::simple(vec![int_stmt(value)], None, Span::new(0, 0)),
            span: Span::new(0, 0),
        }
    }

    fn render(&self, expr: &HirExpression, surface: &impl KeywordSurface) -> String {
        let types = TypeTable::new();
        let codegen = crate::FaberCodegen::new(&types, &self.interner);
        let mut writer = CodeWriter::new();
        codegen.write_expr(expr, &self.names, surface, &mut writer);
        writer.finish()
    }
}

fn assert_fac_omnia(code: &str, english: bool, expect_catch: bool) {
    assert_fac_modifier(
        code,
        english,
        expect_catch,
        radix_syntax::FacModifier::Omnia,
    );
}

fn assert_fac_modifier(
    code: &str,
    english: bool,
    expect_catch: bool,
    modifier: radix_syntax::FacModifier,
) {
    let lexed = if english {
        radix_lexer::lex_with_locale_pack(code, &EnPack)
    } else {
        radix_lexer::lex(code)
    };
    let parsed = radix_parser::parse(lexed);
    assert!(
        parsed.success(),
        "emitted fac omnia form must re-parse, got:\n{code}\nerrors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    assert_eq!(
        program.statements.len(),
        1,
        "emitted fac omnia form must re-parse as one statement, got:\n{code}"
    );
    let StmtKind::Fac(fac) = &program.statements[0].kind else {
        panic!("expected a fac statement, got:\n{code}");
    };
    assert_eq!(
        fac.modifier, modifier,
        "marker must survive re-parse, got:\n{code}"
    );
    assert_eq!(
        fac.catch.is_some(),
        expect_catch,
        "handler attachment must survive re-parse, got:\n{code}"
    );
}

#[test]
fn fac_omnia_block_emits_marker() {
    let ctx = Ctx::new();
    let code = ctx.render(
        &expr(HirExpressionKind::Block(fac_block(
            HirBreakableKind::FacOmnia,
            1,
        ))),
        &LatinSurface,
    );
    assert!(
        code.starts_with("fac omnia {"),
        "breakable FacOmnia block must emit its own keyword, got:\n{code}"
    );
    assert_fac_omnia(&code, false, false);
}

#[test]
fn plain_fac_block_emits_no_marker() {
    let ctx = Ctx::new();
    let code = ctx.render(
        &expr(HirExpressionKind::Block(fac_block(
            HirBreakableKind::Fac,
            1,
        ))),
        &LatinSurface,
    );
    assert!(
        code.starts_with("fac {") && !code.contains("omnia"),
        "plain fac must not gain the marker, got:\n{code}"
    );
}

#[test]
fn fac_omnia_cape_emits_marker_before_handler() {
    // `fac omnia { 1 } cape err { 2 }` lowers to `Handled` over the
    // breakable block tail; the marker must precede the body brace.
    let mut ctx = Ctx::new();
    let catch = ctx.cape("err", 2);
    let handled = expr(HirExpressionKind::Handled {
        body: HirBlock::simple(
            Vec::new(),
            Some(Box::new(expr(HirExpressionKind::Block(fac_block(
                HirBreakableKind::FacOmnia,
                1,
            ))))),
            Span::new(0, 0),
        ),
        catch: Box::new(catch),
    });
    let code = ctx.render(&handled, &LatinSurface);
    let omnia_at = code.find("fac omnia").expect("marker must be emitted");
    let cape_at = code.find("cape err").expect("handler must be emitted");
    assert!(
        omnia_at < cape_at,
        "marker belongs on the fac body, got:\n{code}"
    );
    assert_fac_omnia(&code, false, true);
}

#[test]
fn english_do_all_round_trips() {
    let mut ctx = Ctx::new();
    let catch = ctx.cape("err", 2);
    let handled = expr(HirExpressionKind::Handled {
        body: HirBlock::simple(
            Vec::new(),
            Some(Box::new(expr(HirExpressionKind::Block(fac_block(
                HirBreakableKind::FacOmnia,
                1,
            ))))),
            Span::new(0, 0),
        ),
        catch: Box::new(catch),
    });
    let code = ctx.render(&handled, &EnSurface);
    assert!(
        code.starts_with("do all {"),
        "en surface renders `do all`, got:\n{code}"
    );
    assert!(
        code.contains("catch err"),
        "en surface renders the handler, got:\n{code}"
    );
    assert_fac_omnia(&code, true, true);
}

#[test]
fn fac_machina_block_emits_marker() {
    let ctx = Ctx::new();
    let code = ctx.render(
        &expr(HirExpressionKind::Block(fac_block(
            HirBreakableKind::FacMachina,
            1,
        ))),
        &LatinSurface,
    );
    assert!(
        code.starts_with("fac machina {"),
        "breakable FacMachina block must emit its own keyword, got:\n{code}"
    );
    assert_fac_modifier(&code, false, false, radix_syntax::FacModifier::Machina);
}

#[test]
fn english_do_device_round_trips() {
    let mut ctx = Ctx::new();
    let catch = ctx.cape("err", 2);
    let handled = expr(HirExpressionKind::Handled {
        body: HirBlock::simple(
            Vec::new(),
            Some(Box::new(expr(HirExpressionKind::Block(fac_block(
                HirBreakableKind::FacMachina,
                1,
            ))))),
            Span::new(0, 0),
        ),
        catch: Box::new(catch),
    });
    let code = ctx.render(&handled, &EnSurface);
    assert!(
        code.starts_with("do device {"),
        "en surface renders `do device`, got:\n{code}"
    );
    assert_fac_modifier(&code, true, true, radix_syntax::FacModifier::Machina);
}
