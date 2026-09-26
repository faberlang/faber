//! The one export-visibility resolver (DECISIONS D5.1).
//!
//! Every importable-name surface — the radix file-namespace path
//! (`compile.rs` import targets) and the package / library path
//! (`radix-program` `analyze.rs`, `library.rs`) — is built here, so the tier
//! rule cannot drift between them again:
//!
//! - `@ publica` — exported; importable by any consumer.
//! - `@ interna` — importable by same-package consumers only (the import-seam
//!   policy decides; the surface only carries the tier).
//! - `@ privata` or unmarked — module-private; absent from the export surface,
//!   present in the declaration surface so consumers get "exists but is not
//!   importable" instead of "does not exist".
//!
//! A `publica` import re-exports its binding as `@ publica`. `@ radix backward`
//! companions are compiler-generated and share their source declaration's
//! tier.

use std::collections::BTreeMap;

use crate::lexer::{Interner, TokenKind};
use crate::semantic::passes::visibility_annotation::{VisibilityTier, statement_export_tier};
use crate::syntax::{AnnotationKind, BindingPattern, ImportKind, Program, Stmt, StmtKind, Visibility};

/// Every importable name of one file with its tier. Module-private
/// declarations (unmarked or `@ privata`) are absent.
#[must_use]
pub fn program_export_surface(program: &Program, interner: &Interner) -> BTreeMap<String, VisibilityTier> {
    let mut surface = program_declaration_surface(program, interner);
    surface.retain(|_, tier| *tier != VisibilityTier::Privata);
    surface
}

/// Every named top-level declaration with its effective tier, including
/// module-private names (tier [`VisibilityTier::Privata`]).
#[must_use]
pub fn program_declaration_surface(program: &Program, interner: &Interner) -> BTreeMap<String, VisibilityTier> {
    let mut surface = BTreeMap::new();
    for stmt in &program.statements {
        let Some(tier) = statement_tier(stmt, interner) else {
            continue;
        };
        if let Some(name) = declared_name(stmt, interner) {
            surface.insert(name, tier);
        }
        for companion in backward_companion_export_names(stmt, interner) {
            surface.insert(companion, tier);
        }
    }
    surface
}

/// Effective tier of a top-level statement: its explicit annotation, else
/// module-private. Non-`publica` imports declare nothing and yield `None`.
fn statement_tier(stmt: &Stmt, interner: &Interner) -> Option<VisibilityTier> {
    if let StmtKind::Import(decl) = &stmt.kind {
        return (decl.visibility == Visibility::Publica).then_some(VisibilityTier::Publica);
    }
    Some(statement_export_tier(stmt, interner).unwrap_or(VisibilityTier::Privata))
}

fn declared_name(stmt: &Stmt, interner: &Interner) -> Option<String> {
    let symbol = match &stmt.kind {
        StmtKind::Func(decl) => decl.name.name,
        StmtKind::Class(decl) => decl.name.name,
        StmtKind::Interface(decl) => decl.name.name,
        StmtKind::TypeAlias(decl) => decl.name.name,
        StmtKind::Enum(decl) => decl.name.name,
        StmtKind::Union(decl) => decl.name.name,
        StmtKind::Var(decl) => {
            let BindingPattern::Ident(ident) = &decl.binding else {
                return None;
            };
            ident.name
        }
        StmtKind::Import(decl) if decl.visibility == Visibility::Publica => {
            return import_binding(interner, &decl.kind);
        }
        _ => return None,
    };
    Some(interner.resolve(symbol).to_owned())
}

/// The local binding a re-export import introduces.
pub(crate) fn import_binding(interner: &Interner, kind: &ImportKind) -> Option<String> {
    match kind {
        ImportKind::Named { name, alias, .. } => Some(
            interner
                .resolve(alias.as_ref().unwrap_or(name).name)
                .to_owned(),
        ),
        ImportKind::Wildcard { alias } => Some(interner.resolve(alias.name).to_owned()),
        // EOT-2M: selective members are caller-owned value locals, never a
        // re-exported namespace surface.
        ImportKind::Selective { .. } => None,
    }
}

/// Companion names from `@ radix backward "name"` annotations on a function
/// statement. Companions have no source statement of their own, so their
/// names are derived from the annotation.
fn backward_companion_export_names(stmt: &Stmt, interner: &Interner) -> Vec<String> {
    let StmtKind::Func(decl) = &stmt.kind else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for annotation in stmt.annotations.iter().chain(decl.annotations.iter()) {
        let AnnotationKind::Statement(annotation_stmt) = &annotation.kind else {
            continue;
        };
        if interner.resolve(annotation_stmt.name.name) != "radix" {
            continue;
        }
        let mut args = annotation_stmt.args.iter();
        let Some(TokenKind::Ident(directive)) = args.next().map(|arg| &arg.kind) else {
            continue;
        };
        if interner.resolve(*directive) != "backward" {
            continue;
        }
        let Some(TokenKind::String(name)) = args.next().map(|arg| &arg.kind) else {
            continue;
        };
        names.push(interner.resolve(*name).to_owned());
    }
    names
}
