//! Post-parse completeness check for `LineComment` tokens.
//!
//! Every `#` line-comment token must be claimed by a legal syntax trivia
//! carrier (forward statement/member leading trivia, branch-link leading
//! trivia, block-end trivia, or program-end trivia) or it is reported with one
//! structured invalid-placement diagnostic. Comments are never silently
//! discarded.
//!
//! WHY: A cursor-embedded claim counter would need every legal position and
//! every skip path (`peek`/`advance` inside expressions, annotation parsing,
//! container close loops) to maintain shared state. One post-parse diff over
//! the final `Program` is complete by construction and extensible additively:
//! a future legal position adds a carrier and the check picks it up.

use crate::{ParseError, ParseErrorKind, Parser};
use radix_lexer::{Span, TokenKind};
use radix_syntax::{
    BlockStmt, CapeClause, ClassDecl, ClassMemberKind, FuncDecl, IfBody, ProbandumDecl, Program,
    SecusClause, Stmt, StmtKind, Trivia,
};
use radix_syntax::{Visitor, walk_stmt};
use std::collections::HashSet;

/// Stable `issue` sub-variant for the invalid-placement diagnostic identity.
///
/// The rendered identity is `PARSE060.invalid_comment_placement`; tests assert
/// this value rather than rendered prose.
pub(crate) const INVALID_COMMENT_PLACEMENT_ISSUE: &str = "invalid_comment_placement";

/// Run the completeness check for one parsed program, appending one structured
/// diagnostic per unclaimed `LineComment` token to the parser error list.
pub(super) fn check(parser: &mut Parser, program: &Program) {
    let claimed = claimed_comment_spans(program);
    let unclaimed: Vec<(Span, String)> = parser
        .tokens
        .iter()
        .filter_map(|token| match token.kind {
            TokenKind::LineComment(text) if !claimed.contains(&token.span.start) => {
                Some((token.span, parser.interner.resolve(text).to_owned()))
            }
            _ => None,
        })
        .collect();
    for (span, text) in unclaimed {
        parser.errors.push(
            ParseError::structured(ParseErrorKind::CommentPlacementInvalid, span)
                .with_arg("issue", INVALID_COMMENT_PLACEMENT_ISSUE)
                .with_arg("comment", text),
        );
    }
}

/// Compute the source-offset set claimed by every legal syntax trivia carrier.
///
/// Comments are identified by their token start offset: each lexer `LineComment`
/// token occupies distinct source bytes, so start offsets are unique. Exposed
/// for tests so the completeness matrix can diff carriers against the token
/// stream directly.
pub(crate) fn claimed_comment_spans(program: &Program) -> HashSet<u32> {
    let mut claimed = HashSet::new();
    claim_program(program, &mut claimed);
    claimed
}

fn claim_program(program: &Program, claimed: &mut HashSet<u32>) {
    claim_trivia(&program.end_trivia, claimed);
    if let Some(region) = &program.region {
        claim_trivia(&region.leading_trivia, claimed);
    }
    for stmt in &program.statements {
        claim_stmt(stmt, claimed);
    }

    // The hand-written carrier walk above covers statement/declaration trivia.
    // Reuse the syntax tree walker for blocks nested inside expressions too:
    // closure, comptime, and trap bodies are expression-owned, not statement
    // children of the surrounding block.
    let mut expression_blocks = ExpressionBlockTrivia { claimed };
    for stmt in &program.statements {
        expression_blocks.visit_stmt(stmt);
    }
}

struct ExpressionBlockTrivia<'a> {
    claimed: &'a mut HashSet<u32>,
}

impl Visitor for ExpressionBlockTrivia<'_> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        claim_trivia(&stmt.leading_trivia, self.claimed);
        walk_stmt(self, stmt);
    }

    fn visit_block(&mut self, block: &BlockStmt) {
        claim_trivia(&block.end_trivia, self.claimed);
        for stmt in &block.statements {
            self.visit_stmt(stmt);
        }
    }
}

fn claim_stmt(stmt: &Stmt, claimed: &mut HashSet<u32>) {
    claim_trivia(&stmt.leading_trivia, claimed);
    match &stmt.kind {
        StmtKind::Func(func) => claim_func_decl(func, claimed),
        StmtKind::Class(class) => claim_class_members(class, claimed),
        StmtKind::Interface(interface) => {
            for method in &interface.methods {
                claim_trivia(&method.leading_trivia, claimed);
            }
        }
        StmtKind::Enum(enum_decl) => {
            for member in &enum_decl.members {
                claim_trivia(&member.leading_trivia, claimed);
            }
        }
        StmtKind::Union(union_decl) => {
            for field in &union_decl.shared_fields {
                claim_trivia(&field.leading_trivia, claimed);
            }
            for variant in &union_decl.variants {
                claim_trivia(&variant.leading_trivia, claimed);
            }
        }
        StmtKind::Schema(schema) => {
            claim_trivia(&schema.end_trivia, claimed);
            for column in &schema.columns {
                claim_trivia(&column.leading_trivia, claimed);
            }
        }
        StmtKind::Var(_)
        | StmtKind::SizeDecl(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Import(_)
        | StmtKind::Ex(_)
        | StmtKind::Expr(_)
        | StmtKind::IncDec(_)
        | StmtKind::Redde(_)
        | StmtKind::ReddeFilum(_)
        | StmtKind::Rumpe(_)
        | StmtKind::Perge(_)
        | StmtKind::Iace(_)
        | StmtKind::Mori(_)
        | StmtKind::Tacet(_)
        | StmtKind::Adfirma(_)
        | StmtKind::Require(_)
        | StmtKind::Reject(_)
        | StmtKind::Scribe(_)
        | StmtKind::Reddet(_)
        | StmtKind::Tacebit(_) => {}
        StmtKind::Probandum(probandum) => claim_probandum(probandum, claimed),
        StmtKind::Proba(proba) => claim_block(&proba.body, claimed),
        StmtKind::Block(block) => claim_block(block, claimed),
        StmtKind::Si(si) => {
            claim_if_body(&si.then, claimed);
            claim_catch(si.catch.as_ref(), claimed);
            if let Some(else_) = &si.else_ {
                claim_secus(else_, claimed);
            }
        }
        StmtKind::Dum(dum) => {
            claim_if_body(&dum.body, claimed);
            claim_catch(dum.catch.as_ref(), claimed);
        }
        StmtKind::Itera(itera) => {
            claim_if_body(&itera.body, claimed);
            claim_catch(itera.catch.as_ref(), claimed);
        }
        StmtKind::Elige(elige) => {
            for case in &elige.cases {
                claim_if_body(&case.body, claimed);
            }
            if let Some(default) = &elige.default {
                claim_if_body(&default.body, claimed);
            }
            claim_catch(elige.catch.as_ref(), claimed);
        }
        StmtKind::Discerne(discerne) => {
            for arm in &discerne.arms {
                claim_if_body(&arm.body, claimed);
            }
            if let Some(default) = &discerne.default {
                claim_if_body(&default.body, claimed);
            }
        }
        StmtKind::Custodi(custodi) => {
            for clause in &custodi.clauses {
                claim_if_body(&clause.body, claimed);
            }
        }
        StmtKind::Fac(fac) => {
            claim_block(&fac.body, claimed);
            claim_catch(fac.catch.as_ref(), claimed);
        }
        StmtKind::Incipit(incipit) => claim_if_body(&incipit.body, claimed),
    }
}

fn claim_class_members(class: &ClassDecl, claimed: &mut HashSet<u32>) {
    claim_trivia(&class.end_trivia, claimed);
    for member in &class.members {
        claim_trivia(&member.leading_trivia, claimed);
        if let ClassMemberKind::Method(func) = &member.kind {
            claim_func_decl(func, claimed);
        }
    }
}

fn claim_func_decl(func: &FuncDecl, claimed: &mut HashSet<u32>) {
    claim_trivia(&func.leading_trivia, claimed);
    if let Some(body) = &func.body {
        claim_block(body, claimed);
    }
}

fn claim_block(block: &BlockStmt, claimed: &mut HashSet<u32>) {
    claim_trivia(&block.end_trivia, claimed);
    for stmt in &block.statements {
        claim_stmt(stmt, claimed);
    }
}

fn claim_if_body(body: &IfBody, claimed: &mut HashSet<u32>) {
    match body {
        IfBody::Block(block) => claim_block(block, claimed),
        IfBody::Ergo(stmt) => claim_stmt(stmt, claimed),
    }
}

fn claim_catch(catch: Option<&CapeClause>, claimed: &mut HashSet<u32>) {
    if let Some(catch) = catch {
        claim_block(&catch.body, claimed);
    }
}

fn claim_secus(secus: &SecusClause, claimed: &mut HashSet<u32>) {
    match secus {
        SecusClause::Sin {
            stmt: si,
            leading_trivia,
        } => {
            claim_trivia(leading_trivia, claimed);
            claim_if_body(&si.then, claimed);
            claim_catch(si.catch.as_ref(), claimed);
            if let Some(else_) = &si.else_ {
                claim_secus(else_, claimed);
            }
        }
        SecusClause::Block {
            body,
            catch,
            leading_trivia,
        } => {
            claim_trivia(leading_trivia, claimed);
            claim_block(body, claimed);
            claim_catch(catch.as_ref(), claimed);
        }
        SecusClause::Stmt {
            stmt,
            catch,
            leading_trivia,
        } => {
            claim_trivia(leading_trivia, claimed);
            claim_stmt(stmt, claimed);
            claim_catch(catch.as_ref(), claimed);
        }
    }
}

fn claim_probandum(probandum: &ProbandumDecl, claimed: &mut HashSet<u32>) {
    for setup in &probandum.body.setup {
        claim_block(&setup.body, claimed);
    }
    for test in &probandum.body.tests {
        claim_block(&test.body, claimed);
    }
    for nested in &probandum.body.nested {
        claim_probandum(nested, claimed);
    }
}

fn claim_trivia(trivia: &[Trivia], claimed: &mut HashSet<u32>) {
    for item in trivia {
        if let Trivia::CommentLine(comment) = item {
            claimed.insert(comment.span.start);
        }
    }
}
