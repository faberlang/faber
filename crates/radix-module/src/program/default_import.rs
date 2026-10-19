//! Default import names from a target's `regio` declaration (D7.7).
//!
//! A path-inferred import (`importa ex "library:geo"` with no spelled name)
//! binds the imported file's declared `regio` name when it has one, and the
//! last path segment otherwise. The parser only sees the consumer, so it
//! records the path segment; every program-layer walk that has the target in
//! hand (the radix file-namespace contract in `compile.rs`, the package and
//! library paths in `radix-program`) resolves the declared name and rewrites
//! the consumer's inferred binding through [`apply_default_import_names`]
//! before any binding-keyed surface is built. The driver applies the same map
//! to its own parse through [`crate::driver::ModuleImportContract`], so the
//! analyzed unit and the contract agree on every binding name.

use std::collections::BTreeMap;

use crate::lexer::Interner;
use crate::syntax::{ImportDecl, ImportKind, Program, StmtKind};

/// The file's declared `regio` name, when it declares one.
#[must_use]
pub fn program_region_name(program: &Program, interner: &Interner) -> Option<String> {
    program
        .region
        .as_ref()
        .map(|region| interner.resolve(region.name.name).to_owned())
}

/// Whether an import's binding is a default name nobody spelled.
///
/// Only these imports take the target's declared name; an explicit name or
/// `ut` alias always wins.
#[must_use]
pub fn import_takes_default_name(decl: &ImportDecl) -> bool {
    matches!(decl.kind, ImportKind::Named { alias: None, inferred: true, .. })
}

/// Rewrite default-named imports to the declared names in `names`.
///
/// `names` maps an import path spelling to its target's `regio` name. Imports
/// absent from the map keep their path-segment default.
pub fn apply_default_import_names(program: &mut Program, interner: &mut Interner, names: &BTreeMap<String, String>) {
    if names.is_empty() {
        return;
    }
    for stmt in &mut program.statements {
        let StmtKind::Import(decl) = &mut stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        if !import_takes_default_name(decl) {
            continue;
        }
        let Some(declared) = names.get(interner.resolve(decl.path)) else {
            continue;
        };
        let declared = interner.intern(declared);
        if let ImportKind::Named { name, .. } = &mut decl.kind {
            name.name = declared;
        }
    }
}

#[cfg(test)]
#[path = "default_import_test.rs"]
mod tests;
