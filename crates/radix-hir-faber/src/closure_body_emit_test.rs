//! Closure body emission must preserve lexical versus breakable scopes.
//!
//! The compact closure grammar requires `fac` for block bodies. A plain HIR
//! block must instead use the still-live keyword form, without gaining a
//! break target. Expression and explicit `fac` bodies remain compact.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    HirBlock, HirBreakableKind, HirExpression, HirExpressionKind, HirId, HirLiteral, HirParam,
    HirParamMode,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_syntax::{ClausuraBody, ExprKind, StmtKind};
use radix_types::{DefId, Primitive, TypeTable, USER_DEF_ID_BASE};

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Clausura => "clausura",
            _ => panic!("unexpected keyword: {kind:?}"),
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "numerus"
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

fn block_body(breakable: Option<HirBreakableKind>) -> HirExpression {
    let mut block = HirBlock::simple(Vec::new(), None, Span::new(0, 0));
    block.breakable = breakable;
    expr(HirExpressionKind::Block(block))
}

fn render(arity: u32, body: HirExpression) -> String {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let params = (0..arity)
        .map(|index| HirParam {
            def_id: DefId(USER_DEF_ID_BASE + index),
            name: interner.intern(&format!("p{index}")),
            ty: types.primitive(Primitive::Numerus),
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: Span::new(0, 0),
        })
        .collect();
    let closure = expr(HirExpressionKind::Clausura(
        params,
        None,
        None,
        Box::new(body),
    ));
    let codegen = crate::FaberCodegen::new(&types, &interner);
    let mut writer = CodeWriter::new();
    codegen.write_expr(&closure, &FxHashMap::default(), &LatinSurface, &mut writer);
    writer.finish()
}

fn parsed_body(code: &str, arity: usize) -> ClausuraBody {
    let parsed = radix_parser::parse(radix_lexer::lex(code));
    assert!(parsed.success(), "{code}\n{:?}", parsed.errors);
    let program = parsed.program.expect("program");
    assert_eq!(program.statements.len(), 1, "{code}");
    let StmtKind::Expr(expression) = program
        .statements
        .into_iter()
        .next()
        .expect("statement")
        .kind
    else {
        panic!("expected closure expression: {code}");
    };
    let ExprKind::Clausura(closure) = expression.expr.kind else {
        panic!("expected closure: {code}");
    };
    assert_eq!(closure.params.len(), arity, "{code}");
    closure.body
}

#[test]
fn plain_closure_blocks_keep_keyword_form_for_every_arity() {
    for arity in 0..=2 {
        let code = render(arity, block_body(None));
        assert!(code.starts_with("clausura "), "{code}");
        assert!(!code.contains('∴') && !code.contains("fac"), "{code}");
        assert!(
            matches!(parsed_body(&code, arity as usize), ClausuraBody::Block(_)),
            "{code}"
        );
    }
}

#[test]
fn fac_closure_blocks_keep_compact_form_and_marker() {
    for (kind, marker) in [
        (HirBreakableKind::Fac, "∴ fac {"),
        (HirBreakableKind::FacOmnia, "∴ fac omnia {"),
        (HirBreakableKind::FacMachina, "∴ fac machina {"),
    ] {
        let code = render(0, block_body(Some(kind)));
        assert!(code.starts_with("() ") && code.contains(marker), "{code}");
        let ClausuraBody::Fac(fac) = parsed_body(&code, 0) else {
            panic!("expected explicit fac body: {code}");
        };
        let expected = match kind {
            HirBreakableKind::FacOmnia => radix_syntax::FacModifier::Omnia,
            HirBreakableKind::FacMachina => radix_syntax::FacModifier::Machina,
            _ => radix_syntax::FacModifier::None,
        };
        assert_eq!(fac.modifier, expected, "{code}");
    }
}

#[test]
fn expression_closures_keep_compact_parameter_shape() {
    for arity in 0..=2 {
        let code = render(arity, expr(HirExpressionKind::Literal(HirLiteral::Int(1))));
        assert!(code.ends_with(" ∴ 1"), "{code}");
        assert_eq!(code.starts_with('('), arity != 1, "{code}");
        assert!(
            matches!(parsed_body(&code, arity as usize), ClausuraBody::Expr(_)),
            "{code}"
        );
    }
}
