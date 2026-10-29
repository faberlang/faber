//! Structure inventory of a parsed source document.
//!
//! WHY: the canonical Faber re-emitter prints from HIR, and HIR keeps no
//! representation for some surface constructs (test-suite hooks, top-level
//! statements beside `main`). A construct the emitter cannot carry vanishes
//! from the output and the meaning fingerprint never sees it. Comparing this
//! inventory of the source against the inventory of the emitted text turns a
//! silent drop into a refusal.
//!
//! The inventory counts top-level statement kinds and the members of every
//! test suite. Statements inside function, method, closure, and test bodies are
//! not counted: the HIR meaning fingerprint owns them.

use super::{ParsedSourceBody, Session, parse_frontend_source};
use crate::diagnostics::{Diagnostic, Span};
use crate::lexer::Interner;
use crate::syntax::{ProbandumDecl, Program, StmtKind};
use std::collections::BTreeMap;

/// One counted construct: how many times it appears and where it first does.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InventoryEntry {
    pub count: usize,
    /// Span of the first occurrence, relative to the peeled body.
    pub first_span: Span,
}

/// Counted constructs keyed by `(suite path, construct)`.
///
/// The suite path is empty for a top-level construct and `/outer/inner` for a
/// member of a nested suite.
pub(crate) type Inventory = BTreeMap<(String, String), InventoryEntry>;

/// A construct whose count dropped between source and output.
#[derive(Debug, Clone)]
pub(crate) struct LostConstruct {
    pub suite: String,
    pub construct: String,
    pub source_count: usize,
    pub output_count: usize,
    pub first_span: Span,
}

/// Parse `source` with the session's peeling and locale rules and count it.
///
/// # Errors
///
/// Returns the front-end diagnostics when the document does not lex and parse
/// cleanly; the caller fails closed rather than skipping the comparison.
pub(crate) fn parse_inventory(session: &Session, name: &str, source: &str) -> Result<Inventory, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let parsed = parse_frontend_source(session, name, source, &mut diagnostics);
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(diagnostics);
    }
    match parsed {
        Ok(ParsedSourceBody { program, interner, .. }) => Ok(inventory(&program, &interner)),
        Err(()) => Err(diagnostics),
    }
}

/// Count the top-level statements and test-suite members of `program`.
pub(crate) fn inventory(program: &Program, interner: &Interner) -> Inventory {
    let mut inv = Inventory::new();
    for stmt in &program.statements {
        bump(&mut inv, "", format!("top:{}", kind_name(&stmt.kind)), stmt.span);
        if let StmtKind::Probandum(decl) = &stmt.kind {
            count_suite(&mut inv, "", decl, interner);
        }
    }
    inv
}

/// Report every construct the output carries fewer times than the source.
///
/// One-sided on purpose: a construct the emitter adds is a meaning question
/// the HIR fingerprint already owns.
pub(crate) fn lost(source: &Inventory, output: &Inventory) -> Vec<LostConstruct> {
    source
        .iter()
        .filter_map(|((suite, construct), entry)| {
            let output_count = output
                .get(&(suite.clone(), construct.clone()))
                .map_or(0, |found| found.count);
            (output_count < entry.count).then(|| LostConstruct {
                suite: suite.clone(),
                construct: construct.clone(),
                source_count: entry.count,
                output_count,
                first_span: entry.first_span,
            })
        })
        .collect()
}

fn count_suite(inv: &mut Inventory, parent: &str, decl: &ProbandumDecl, interner: &Interner) {
    let here = format!("{parent}/{}", interner.resolve(decl.name));
    for hook in &decl.body.setup {
        bump(inv, &here, format!("hook:{:?}:all={}", hook.kind, hook.all), hook.span);
    }
    for case in &decl.body.tests {
        bump(inv, &here, format!("test:{}", interner.resolve(case.name)), case.span);
    }
    for nested in &decl.body.nested {
        bump(
            inv,
            &here,
            format!("nested-suite:{}", interner.resolve(nested.name)),
            nested.span,
        );
        count_suite(inv, &here, nested, interner);
    }
}

fn bump(inv: &mut Inventory, suite: &str, construct: String, span: Span) {
    inv.entry((suite.to_owned(), construct))
        .and_modify(|entry| entry.count += 1)
        .or_insert(InventoryEntry { count: 1, first_span: span });
}

/// Exhaustive on purpose: a new statement kind forces a decision here.
fn kind_name(kind: &StmtKind) -> &'static str {
    match kind {
        StmtKind::Var(_) => "var",
        StmtKind::SizeDecl(_) => "size-decl",
        StmtKind::Func(_) => "func",
        StmtKind::Class(_) => "class",
        StmtKind::Interface(_) => "interface",
        StmtKind::TypeAlias(_) => "type-alias",
        StmtKind::Schema(_) => "schema",
        StmtKind::Enum(_) => "enum",
        StmtKind::Union(_) => "union",
        StmtKind::Import(_) => "import",
        StmtKind::Probandum(_) => "probandum",
        StmtKind::Proba(_) => "proba",
        StmtKind::Ex(_) => "ex",
        StmtKind::Block(_) => "block",
        StmtKind::Expr(_) => "expr",
        StmtKind::IncDec(_) => "incdec",
        StmtKind::Si(_) => "si",
        StmtKind::Dum(_) => "dum",
        StmtKind::Itera(_) => "itera",
        StmtKind::Elige(_) => "elige",
        StmtKind::Discerne(_) => "discerne",
        StmtKind::Custodi(_) => "custodi",
        StmtKind::Fac(_) => "fac",
        StmtKind::Redde(_) => "redde",
        StmtKind::ReddeFilum(_) => "redde_filum",
        StmtKind::Rumpe(_) => "rumpe",
        StmtKind::Perge(_) => "perge",
        StmtKind::Iace(_) => "iace",
        StmtKind::Mori(_) => "mori",
        StmtKind::Tacet(_) => "tacet",
        StmtKind::Adfirma(_) => "adfirma",
        StmtKind::Require(_) => "require",
        StmtKind::Reject(_) => "reject",
        StmtKind::Scribe(_) => "scribe",
        StmtKind::Incipit(_) => "incipit",
        StmtKind::Reddet(_) => "reddet",
        StmtKind::Tacebit(_) => "tacebit",
    }
}

#[cfg(test)]
#[path = "source_inventory_test.rs"]
mod tests;
