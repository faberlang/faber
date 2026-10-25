//! Emitter proofs for arm- and loop-attached `cape` handlers.
//!
//! Grammar order is fixed: `fac block cape_clause? ('dum' expression)?` puts
//! the handler before the loop condition, and `arm`/`else_arm` carry their own
//! `cape_clause?`. Each emitted shape must re-parse into the same attachment:
//! a `fac … dum` handler stays on the `fac` (never a stray statement after
//! `dum cond`), and a handled `secus`/`sin` arm keeps its own handler instead of
//! compacting the synthetic `Handled` tail into `secus ergo redde fac …`.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    HirBlock, HirCape, HirExpression, HirExpressionKind, HirId, HirLiteral, HirStatement,
    HirStatementKind, HirUnOp,
};
use radix_lexer::{Interner, Span, Symbol, TokenKind};
use radix_syntax::{SecusClause, StmtKind};
use radix_types::{DefId, TypeTable, USER_DEF_ID_BASE};

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, kind: TokenKind) -> &'static str {
        match kind {
            TokenKind::Cape => "cape",
            TokenKind::Rumpe => "rumpe",
            TokenKind::Perge => "perge",
            TokenKind::Verum => "verum",
            TokenKind::Falsum => "falsum",
            TokenKind::Non => "non",
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

fn block(statements: Vec<HirStatement>) -> HirBlock {
    HirBlock::simple(statements, None, Span::new(0, 0))
}

fn tail_block(tail: HirExpression) -> HirBlock {
    HirBlock::simple(Vec::new(), Some(Box::new(tail)), Span::new(0, 0))
}

fn bool_lit(value: bool) -> HirExpression {
    expr(HirExpressionKind::Literal(HirLiteral::Bool(value)))
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
            body: block(vec![int_stmt(value)]),
            span: Span::new(0, 0),
        }
    }

    fn render(&self, expr: &HirExpression) -> String {
        let types = TypeTable::new();
        let codegen = crate::FaberCodegen::new(&types, &self.interner);
        let mut writer = CodeWriter::new();
        codegen.write_expr(expr, &self.names, &LatinSurface, &mut writer);
        writer.finish()
    }
}

fn parse_single(code: &str) -> radix_syntax::Program {
    let parsed = radix_parser::parse(radix_lexer::lex(code));
    assert!(
        parsed.success(),
        "emitted cape form must re-parse, got:\n{code}\nerrors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    assert_eq!(
        program.statements.len(),
        1,
        "emitted cape form must re-parse as one statement (no stray handler), got:\n{code}"
    );
    program
}

/// `fac { 1 } cape err { 2 } dum verum` lowers to `Handled { body: Loop }`
/// whose loop ends in the synthetic `si non cond { rumpe }` exit test.
fn fac_dum_handled(ctx: &mut Ctx) -> HirExpression {
    let exit_test = HirStatement {
        id: HirId(3),
        kind: HirStatementKind::Expr(expr(HirExpressionKind::Si {
            cond: Box::new(expr(HirExpressionKind::Unary(
                HirUnOp::Not,
                Box::new(bool_lit(true)),
            ))),
            then_block: block(vec![HirStatement {
                id: HirId(4),
                kind: HirStatementKind::Rumpe,
                span: Span::new(0, 0),
            }]),
            then_catch: None,
            else_block: None,
        })),
        span: Span::new(0, 0),
    };
    let loop_expr = expr(HirExpressionKind::Loop(block(vec![int_stmt(1), exit_test])));
    let catch = ctx.cape("err", 2);
    expr(HirExpressionKind::Handled {
        body: tail_block(loop_expr),
        catch: Box::new(catch),
    })
}

#[test]
fn fac_dum_cape_emits_handler_before_dum() {
    let mut ctx = Ctx::new();
    let handled = fac_dum_handled(&mut ctx);
    let code = ctx.render(&handled);
    let cape_at = code.find("cape err").expect("handler must be emitted");
    let dum_at = code
        .find("dum verum")
        .expect("loop condition must be emitted");
    assert!(
        cape_at < dum_at,
        "grammar order is `fac {{ }} cape err {{ }} dum cond`, got:\n{code}"
    );

    let program = parse_single(&code);
    let StmtKind::Fac(fac) = &program.statements[0].kind else {
        panic!("expected a fac statement, got:\n{code}");
    };
    assert!(
        fac.catch.is_some(),
        "handler must attach to fac, got:\n{code}"
    );
    assert!(
        fac.while_.is_some(),
        "dum condition must attach to fac, got:\n{code}"
    );
}

#[test]
fn si_secus_arm_cape_keeps_per_arm_handlers() {
    // `si verum { 1 } cape a { 2 } secus { 3 } cape b { 4 }`: the handled
    // else arm lowers to a block whose tail is `Handled` over the arm block.
    let mut ctx = Ctx::new();
    let then_catch = ctx.cape("a", 2);
    let else_catch = ctx.cape("b", 4);
    let si = expr(HirExpressionKind::Si {
        cond: Box::new(bool_lit(true)),
        then_block: block(vec![int_stmt(1)]),
        then_catch: Some(Box::new(then_catch)),
        else_block: Some(tail_block(expr(HirExpressionKind::Handled {
            body: block(vec![int_stmt(3)]),
            catch: Box::new(else_catch),
        }))),
    });
    let code = ctx.render(&si);
    assert!(
        !code.contains("ergo"),
        "a handled secus arm must stay braced, not `secus ergo redde fac …`, got:\n{code}"
    );

    let program = parse_single(&code);
    let StmtKind::Si(stmt) = &program.statements[0].kind else {
        panic!("expected a si statement, got:\n{code}");
    };
    assert!(
        stmt.catch.is_some(),
        "then arm handler must survive, got:\n{code}"
    );
    let Some(SecusClause::Block { catch, .. }) = &stmt.else_ else {
        panic!("expected a braced secus arm, got:\n{code}");
    };
    assert!(
        catch.is_some(),
        "secus arm handler must survive, got:\n{code}"
    );
}

#[test]
fn si_sin_arm_cape_keeps_handler_on_sin() {
    // `si verum { 1 } sin falsum { 2 } cape e { 3 }`: the handled `sin` arm is
    // a nested conditional with `then_catch`, still flattened to `sin`.
    let mut ctx = Ctx::new();
    let sin_catch = ctx.cape("e", 3);
    let si = expr(HirExpressionKind::Si {
        cond: Box::new(bool_lit(true)),
        then_block: block(vec![int_stmt(1)]),
        then_catch: None,
        else_block: Some(tail_block(expr(HirExpressionKind::Si {
            cond: Box::new(bool_lit(false)),
            then_block: block(vec![int_stmt(2)]),
            then_catch: Some(Box::new(sin_catch)),
            else_block: None,
        }))),
    });
    let code = ctx.render(&si);
    assert!(
        code.contains("sin falsum"),
        "handled sin arm must flatten to `sin`, got:\n{code}"
    );

    let program = parse_single(&code);
    let StmtKind::Si(stmt) = &program.statements[0].kind else {
        panic!("expected a si statement, got:\n{code}");
    };
    let Some(SecusClause::Sin { stmt: sin, .. }) = &stmt.else_ else {
        panic!("expected a sin arm, got:\n{code}");
    };
    assert!(
        sin.catch.is_some(),
        "sin arm handler must survive, got:\n{code}"
    );
}
