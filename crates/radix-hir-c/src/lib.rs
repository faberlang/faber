//! C HIR codegen backend (leaf crate) — a novelty target.
//!
//! Registration spike (no goal doc yet; modeled on `hir-odin` unit OD1). Emits one C11 translation unit
//! from checked HIR for a scalar-and-print program: integer and text literals,
//! folded integer constants, immutable-use locals of those types, and `print`.
//! Everything else fails closed with a named "unsupported on target c"
//! diagnostic; the README lists what is supported and rejected.
//!
//! C is the permissive compiler, so this emitter is the strict one: nothing it
//! emits may reach undefined behavior. Integer arithmetic is folded at emit
//! time in exact integers and never emitted as C arithmetic until the exact
//! model lands.
//!
//! The output needs no runtime library. Headers and helper functions are
//! emitted inline, only when the program uses them. The leaf never invokes a C
//! compiler; building and running the output is the harness's job.
//!
//! `radix-module` re-exports this leaf through its codegen surface and owns
//! target dispatch.

mod error;
mod expr;
mod helpers;
mod names;
mod stmt;
mod ty;

#[cfg(test)]
mod emit_test;
#[cfg(test)]
mod names_test;
#[cfg(test)]
mod reject_test;
#[cfg(test)]
mod test_support;

use crate::error::unsupported;
use crate::helpers::Includes;
use crate::names::Namer;
use crate::ty::Ty;
use radix_codegen_shared::names::NameCatalog;
use radix_codegen_shared::{Codegen, CodegenError};
use radix_hir::{DefId, HirItemKind, HirModule};
use radix_lexer::Interner;
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

/// C backend output.
#[derive(Debug, Clone)]
pub struct COutput {
    /// Complete generated C translation unit.
    pub code: String,
}

/// Shared context for C emission.
pub struct CCodegen<'a> {
    names: NameCatalog<'a>,
}

impl<'a> CCodegen<'a> {
    /// Build a codegen context from one analyzed unit.
    #[must_use]
    pub fn new(hir: &'a HirModule, interner: &'a Interner) -> Self {
        Self {
            names: NameCatalog::new(hir, interner),
        }
    }
}

impl Codegen for CCodegen<'_> {
    type Output = COutput;

    fn generate(
        &self,
        hir: &HirModule,
        types: &TypeTable,
        _interner: &Interner,
    ) -> Result<COutput, CodegenError> {
        radix_codegen_shared::reject_hir_errors(hir)?;
        let code = Emitter {
            names: &self.names,
            types,
            scope: Namer::program_scope(),
            locals: FxHashMap::default(),
            includes: Includes::default(),
        }
        .program(hir)?;
        Ok(COutput { code })
    }
}

pub(crate) struct Emitter<'a> {
    names: &'a NameCatalog<'a>,
    types: &'a TypeTable,
    /// Names claimed in `main` (the only scope in this subset).
    scope: Namer,
    /// Locals of `main`, by definition.
    locals: FxHashMap<DefId, Local>,
    includes: Includes,
}

/// One local of `main`.
pub(crate) struct Local {
    pub(crate) name: String,
    pub(crate) ty: Ty,
    /// The value of an immutable integer local with a folded initializer, so a
    /// read of it folds too.
    pub(crate) constant: Option<i128>,
}

impl Emitter<'_> {
    fn program(mut self, hir: &HirModule) -> Result<String, CodegenError> {
        if hir.entry_args_name.is_some() || hir.entry_is_async || !hir.entry_annotations.is_empty()
        {
            return Err(unsupported("entry with arguments, async, or annotations"));
        }
        Self::check_items(hir)?;
        let mut body = String::new();
        if let Some(entry) = &hir.entry {
            self.block(&mut body, entry)?;
        }
        let mut out = self.includes.text();
        out.push_str("int main(void) {\n");
        out.push_str(&body);
        out.push_str("    return 0;\n}\n");
        Ok(out)
    }

    /// The subset has no file-scope items: the first one is rejected by name.
    fn check_items(hir: &HirModule) -> Result<(), CodegenError> {
        let Some(item) = hir.items.first() else {
            return Ok(());
        };
        if !item.annotations.is_empty() {
            return Err(unsupported("annotation"));
        }
        Err(unsupported(match &item.kind {
            HirItemKind::Function(_) => "function declaration",
            HirItemKind::Constant(_) => "module constant",
            HirItemKind::Struct(_) => "genus declaration",
            HirItemKind::Enum(_) => "enum declaration",
            HirItemKind::Interface(_) => "interface declaration",
            HirItemKind::TypeAlias(_) => "type alias",
            HirItemKind::Schema(_) => "schema declaration",
            HirItemKind::Import(_) => "import (library modules)",
        }))
    }
}
