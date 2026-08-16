//! Source spelling recovery for canonical Faber emission.
//!
//! HIR references definitions by [`DefId`], while emitted Faber source must use
//! identifiers. This module builds the small DefId-to-Symbol table that lets the
//! pretty-printer recover source spellings from the shared interner.
//!
//! This is not name resolution. Semantic analysis has already decided what each
//! reference means; the backend only reverses that stable identity into a
//! printable spelling. When a definition is absent from the collected table, the
//! `def_N` fallback is deliberately synthetic so degraded backend output is
//! visible instead of silently borrowing an unrelated spelling.

use radix_hir::visit::HirVisitor;
use radix_hir::{DefId, HirProgram};
use radix_lexer::{Interner, Symbol};
use rustc_hash::FxHashMap;

impl super::FaberCodegen {
    /// Return the printable spelling for an already-resolved definition.
    ///
    /// The fallback is for robustness at the output boundary, not a resolver.
    /// A missing entry means the HIR did not expose a source name to this pass.
    pub(super) fn name_for_def(
        &self,
        def_id: DefId,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
    ) -> String {
        names.get(&def_id).map_or_else(
            || missing_def_spelling(def_id),
            |sym| self.symbol_to_string(*sym, interner),
        )
    }

    /// Spelling for a resolved reference that may be imported: imported
    /// nominals and union variants carry their qualified source spelling
    /// (`base.Vector3`, `manifestum.Cognita`) from the import contract, since
    /// their `DefId` has no local name-table entry.
    pub(super) fn name_for_def_imported(
        &self,
        def_id: DefId,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
    ) -> String {
        self.imported_def_spellings
            .get(&def_id)
            .cloned()
            .unwrap_or_else(|| self.name_for_def(def_id, names, interner))
    }

    /// Prefer the collected binding name, falling back to the HIR placeholder.
    pub(super) fn resolved_binding_symbol(
        def_id: DefId,
        fallback: Symbol,
        names: &FxHashMap<DefId, Symbol>,
    ) -> Symbol {
        names.get(&def_id).copied().unwrap_or(fallback)
    }

    /// Convert an interned symbol back into its source spelling.
    pub(super) fn symbol_to_string(&self, sym: Symbol, interner: &Interner) -> String {
        interner.resolve(sym).to_owned()
    }

    /// Collect `DefId` -> Symbol mappings for all definitions in the program.
    ///
    /// The collection pass keeps emission deterministic and cheap: expression,
    /// type, pattern, and declaration writers can print resolved references
    /// without walking surrounding HIR to rediscover names.
    pub(super) fn collect_names(&self, hir: &HirProgram) -> FxHashMap<DefId, Symbol> {
        let mut collector = FaberNameCollector {
            names: FxHashMap::default(),
        };
        collector.visit_program(hir);
        collector.names
    }
}

fn missing_def_spelling(def_id: DefId) -> String {
    format!("def_{}", def_id.0)
}

struct FaberNameCollector {
    names: FxHashMap<DefId, Symbol>,
}

impl HirVisitor for FaberNameCollector {
    fn visit_def(&mut self, def_id: DefId, name: Symbol) {
        self.names.insert(def_id, name);
    }
}
