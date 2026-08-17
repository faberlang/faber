//! Line-sensitive grammar guard (FORMAT-PRETTY S3 unit 4).
//!
//! Faber has explicit line-sensitive forms (compact `ergo` bodies, annotation
//! sugar). The pretty engine must never flatten source merely because tokens
//! could fit. When a construct cannot be preserved confidently, the whole file
//! is left unchanged, the path/location is reported, and `--check` mode fails
//! (a partial rewrite is worse than an unformatted file).
//!
//! The guard is a conservative pre-pass over the parsed AST. Everything the
//! pretty emitter handles explicitly is passed; the two unconfident classes
//! flagged here are:
//!
//! 1. **Compact `ergo` arms whose statement owns a brace block** — laying out
//!    nested control flow compactly is exactly where a printer can restructure
//!    something the author wrote deliberately, so the file is left untouched.
//! 2. **Annotation statement sugar with a raw token that cannot be re-emitted
//!    verbatim** — the emitter must never turn an unknown token into a `?`.

use crate::lexer::{keyword_spelling_for_token, Span, TokenKind};
use crate::syntax::{
    Annotation, AnnotationKind, BlockStmt, ClassMemberKind, IfBody, ProbandumBody, Program, SecusClause, Stmt, StmtKind,
};

/// Why a file was flagged as not-confidently-formattable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardKind {
    /// A compact `ergo` arm whose statement owns a brace block.
    ErgoBlockNesting,
    /// An annotation whose raw token payload cannot be re-emitted verbatim.
    AnnotationTokenLossy,
}

/// One unconfident construct found by the pre-pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardReport {
    pub kind: GuardKind,
    pub span: Span,
}

/// Statement kinds that own a brace block. An `ergo` arm wrapping one of these
/// cannot be laid out compactly with confidence.
#[rustfmt::skip]
fn is_block_owning_stmt(kind: &StmtKind) -> bool {
    matches!(
        kind,
        StmtKind::Func(_)
            | StmtKind::Class(_)
            | StmtKind::Interface(_)
            | StmtKind::Probandum(_)
            | StmtKind::Proba(_)
            | StmtKind::Block(_)
            | StmtKind::Si(_)
            | StmtKind::Dum(_)
            | StmtKind::Itera(_)
            | StmtKind::Elige(_)
            | StmtKind::Discerne(_)
            | StmtKind::Custodi(_)
            | StmtKind::Fac(_)
            | StmtKind::Incipit(_)
            | StmtKind::Cura(_)
    )
}

/// Run the guard pre-pass over a program.
#[must_use]
pub fn find_guard_hits(program: &Program) -> Vec<GuardReport> {
    let mut reports = Vec::new();
    for stmt in &program.statements {
        visit_stmt(stmt, &mut reports);
    }
    reports
}

fn push_ergo_guard(stmt: &Stmt, reports: &mut Vec<GuardReport>) {
    reports.push(GuardReport { kind: GuardKind::ErgoBlockNesting, span: stmt.span });
}

/// Check one `ergo` body: flag it when the body statement owns a brace block,
/// and recurse into it either way.
fn visit_ergo(stmt: &Stmt, reports: &mut Vec<GuardReport>) {
    if is_block_owning_stmt(&stmt.kind) {
        push_ergo_guard(stmt, reports);
    }
    visit_stmt(stmt, reports);
}

fn visit_if_body(body: &IfBody, reports: &mut Vec<GuardReport>) {
    match body {
        IfBody::Block(block) => visit_block(block, reports),
        IfBody::Ergo(stmt) => visit_ergo(stmt, reports),
    }
}

fn visit_block(block: &BlockStmt, reports: &mut Vec<GuardReport>) {
    for stmt in &block.statements {
        visit_stmt(stmt, reports);
    }
}

fn visit_stmt(stmt: &Stmt, reports: &mut Vec<GuardReport>) {
    for annotation in &stmt.annotations {
        visit_annotation(annotation, reports);
    }
    match &stmt.kind {
        StmtKind::Var(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Union(_)
        | StmtKind::Schema(_)
        | StmtKind::Import(_)
        | StmtKind::Ex(_)
        | StmtKind::Expr(_)
        | StmtKind::IncDec(_)
        | StmtKind::Redde(_)
        | StmtKind::Rumpe(_)
        | StmtKind::Perge(_)
        | StmtKind::Iace(_)
        | StmtKind::Mori(_)
        | StmtKind::Tacet(_)
        | StmtKind::Reddet(_)
        | StmtKind::Tacebit(_)
        | StmtKind::Adfirma(_)
        | StmtKind::Require(_)
        | StmtKind::Scribe(_) => {}
        StmtKind::Func(func) => {
            if let Some(body) = &func.body {
                visit_block(body, reports);
            }
        }
        StmtKind::Class(class) => {
            for member in &class.members {
                if let ClassMemberKind::Method(method) = &member.kind {
                    if let Some(body) = &method.body {
                        visit_block(body, reports);
                    }
                }
            }
        }
        StmtKind::Interface(_) => {
            // Interface methods are contract declarations without bodies.
        }
        StmtKind::Probandum(suite) => {
            for setup in &suite.body.setup {
                visit_block(&setup.body, reports);
            }
            for test in &suite.body.tests {
                visit_block(&test.body, reports);
            }
            for nested in &suite.body.nested {
                // Nested suites are declared through the same visitor path
                // when reached; visit their bodies here.
                visit_probandum_body(&nested.body, reports);
            }
        }
        StmtKind::Proba(case) => visit_block(&case.body, reports),
        StmtKind::Block(block) => visit_block(block, reports),
        StmtKind::Si(si) => {
            visit_if_body(&si.then, reports);
            if let Some(else_) = &si.else_ {
                visit_secus_clause(else_, reports);
            }
            if let Some(catch) = &si.catch {
                visit_block(&catch.body, reports);
            }
        }
        StmtKind::Dum(dum) => {
            visit_if_body(&dum.body, reports);
            if let Some(catch) = &dum.catch {
                visit_block(&catch.body, reports);
            }
        }
        StmtKind::Itera(itera) => {
            visit_if_body(&itera.body, reports);
            if let Some(catch) = &itera.catch {
                visit_block(&catch.body, reports);
            }
        }
        StmtKind::Elige(elige) => {
            for case in &elige.cases {
                visit_if_body(&case.body, reports);
            }
            if let Some(default) = &elige.default {
                visit_if_body(&default.body, reports);
            }
            if let Some(catch) = &elige.catch {
                visit_block(&catch.body, reports);
            }
        }
        StmtKind::Discerne(discerne) => {
            for arm in &discerne.arms {
                visit_if_body(&arm.body, reports);
            }
            if let Some(default) = &discerne.default {
                visit_if_body(&default.body, reports);
            }
        }
        StmtKind::Custodi(custodi) => {
            for clause in &custodi.clauses {
                visit_if_body(&clause.body, reports);
            }
        }
        StmtKind::Fac(fac) => {
            visit_block(&fac.body, reports);
            if let Some(catch) = &fac.catch {
                visit_block(&catch.body, reports);
            }
        }
        StmtKind::Incipit(incipit) => {
            visit_if_body(&incipit.body, reports);
        }
        StmtKind::Cura(cura) => {
            visit_block(&cura.body, reports);
            if let Some(catch) = &cura.catch {
                visit_block(&catch.body, reports);
            }
        }
    }
}

fn visit_secus_clause(secus: &SecusClause, reports: &mut Vec<GuardReport>) {
    match secus {
        SecusClause::Sin { stmt: nested, .. } => {
            visit_if_body(&nested.then, reports);
            if let Some(catch) = &nested.catch {
                visit_block(&catch.body, reports);
            }
            if let Some(else_) = &nested.else_ {
                visit_secus_clause(else_, reports);
            }
        }
        SecusClause::Block { body, catch, .. } => {
            visit_block(body, reports);
            if let Some(catch) = catch {
                visit_block(&catch.body, reports);
            }
        }
        SecusClause::Stmt { stmt, catch, .. } => {
            visit_stmt(stmt, reports);
            if let Some(catch) = catch {
                visit_block(&catch.body, reports);
            }
        }
    }
}

fn visit_probandum_body(body: &ProbandumBody, reports: &mut Vec<GuardReport>) {
    for setup in &body.setup {
        visit_block(&setup.body, reports);
    }
    for test in &body.tests {
        visit_block(&test.body, reports);
    }
    for nested in &body.nested {
        visit_probandum_body(&nested.body, reports);
    }
}

fn visit_annotation(annotation: &Annotation, reports: &mut Vec<GuardReport>) {
    if let AnnotationKind::Statement(statement) = &annotation.kind {
        for token in &statement.args {
            if !token_is_preservable(&token.kind) {
                reports.push(GuardReport { kind: GuardKind::AnnotationTokenLossy, span: token.span });
            }
        }
    }
}

/// Whether a raw annotation token can be re-emitted verbatim.
///
/// Mirrors the coverage of the emitter's token writer: every keyword (via the
/// keyword table), identifier, literal, and punctuation token has a faithful
/// spelling. Reserved-placeholder, trivia, and error kinds do not.
#[must_use]
pub fn token_is_preservable(kind: &TokenKind) -> bool {
    if keyword_spelling_for_token(kind).is_some() {
        return true;
    }
    matches!(
        kind,
        TokenKind::Ident(_)
            | TokenKind::Underscore(_)
            | TokenKind::Integer(_)
            | TokenKind::Float(_)
            | TokenKind::String(_)
            | TokenKind::AsciiString(_)
            | TokenKind::BacktickString(_)
            | TokenKind::OctetiString(_)
            | TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::LBracket
            | TokenKind::RBracket
            | TokenKind::Comma
            | TokenKind::Colon
            | TokenKind::Semicolon
            | TokenKind::Dot
            | TokenKind::Arrow
            | TokenKind::ExitArrow
            | TokenKind::Cup
            | TokenKind::At
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::BitwiseAnd
            | TokenKind::BitwiseOr
            | TokenKind::BitwiseXor
            | TokenKind::BitwiseNot
            | TokenKind::BitwiseShl
            | TokenKind::BitwiseShr
            | TokenKind::Bang
            | TokenKind::Question
            | TokenKind::Eq
            | TokenKind::Assign
            | TokenKind::ConversioAssign
            | TokenKind::EqEq
            | TokenKind::ApproxEq
            | TokenKind::ApproxNotEq
            | TokenKind::BangEq
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::LtAscii
            | TokenKind::GtAscii
            | TokenKind::LtEq
            | TokenKind::GtEq
            | TokenKind::PostInc
            | TokenKind::PostDec
            | TokenKind::QuestionDot
            | TokenKind::QuestionBracket
            | TokenKind::QuestionParen
            | TokenKind::BangDot
            | TokenKind::BangBracket
            | TokenKind::BangParen
            | TokenKind::DotDot
            | TokenKind::Ellipsis
            | TokenKind::Therefore
            | TokenKind::LineComment(_)
    )
}

#[cfg(test)]
#[path = "guard_test.rs"]
mod tests;
