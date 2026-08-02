//! Shared HIR name recovery for target backends.
//!
//! HIR nodes refer to declarations through [`DefId`]s, while generated source
//! needs stable textual names. This module builds a small catalog by walking the
//! HIR once and recording every definition symbol the visitor exposes. Backends
//! use it to recover source spellings without each target reimplementing HIR
//! traversal or reaching back into resolver internals.
//!
//! INVARIANTS
//! ==========
//! - The catalog is best-effort recovery from HIR, not a name-resolution pass.
//! - Missing entries produce a deterministic placeholder so codegen failures are
//!   reproducible and obvious instead of panicking in a backend.
//! - Symbols remain interned; text is resolved lazily through the shared
//!   [`Interner`] owned by the compile session.

use radix_hir::visit::HirVisitor;
use radix_hir::{DefId, HirProgram};
use radix_lexer::{Interner, Symbol};
use rustc_hash::FxHashMap;

/// Error returned when a `DefId` is not in the name catalog.
#[derive(Debug, Clone, Copy)]
pub struct MissingDefError {
    /// The [`DefId`] that was not found.
    pub def_id: DefId,
}

impl MissingDefError {
    /// Visible marker emitted when a missing name reaches codegen output.
    ///
    /// Contains comment delimiters so it is not a valid identifier in any
    /// target language, making the gap immediately obvious in generated code.
    pub const MARKER: &'static str = "/* unresolved_def */";
}

/// [`DefId`]-to-symbol lookup shared by target emitters.
///
/// This type exists because HIR is intentionally identifier-light after
/// semantic resolution: references carry compiler IDs, and only declarations
/// retain the source symbol needed for target names. The catalog keeps that
/// recovery local to codegen while preserving the invariant that the interner
/// owns all string storage.
pub struct NameCatalog<'a> {
    names: FxHashMap<DefId, Symbol>,
    interner: &'a Interner,
}

impl<'a> NameCatalog<'a> {
    /// Build a catalog from the definition-bearing nodes in one HIR program.
    #[must_use]
    pub fn new(hir: &HirProgram, interner: &'a Interner) -> Self {
        Self {
            names: collect_names(hir),
            interner,
        }
    }

    /// Shared compile-session interner for source spellings.
    #[must_use]
    pub fn interner(&self) -> &Interner {
        self.interner
    }

    /// Resolve an already-known symbol through the compile-session interner.
    #[must_use]
    pub fn resolve_symbol(&self, sym: Symbol) -> &str {
        self.interner.resolve(sym)
    }

    /// Resolve a definition ID to its source spelling.
    ///
    /// Returns `Err(MissingDefError)` when the [`DefId`] is not in the catalog.
    /// Callers must decide policy: propagate the error, panic, or substitute
    /// [`MissingDefError::MARKER`] for output. The catalog never silently
    /// emits a plausible-looking identifier for a missing definition.
    ///
    /// # Errors
    ///
    /// Returns [`MissingDefError`] when `def_id` was not collected from the HIR.
    pub fn resolve_def(&self, def_id: DefId) -> Result<&str, MissingDefError> {
        self.names
            .get(&def_id)
            .map(|sym| self.resolve_symbol(*sym))
            .ok_or(MissingDefError { def_id })
    }

    /// Whether the HIR program contained a source symbol for this definition.
    #[must_use]
    pub fn contains_def(&self, def_id: DefId) -> bool {
        self.names.contains_key(&def_id)
    }

    /// Register a DefId-to-name mapping not present in the local HIR.
    ///
    /// Used to resolve cross-file `DefIds` (types/functions imported from sibling
    /// modules) that the collector did not visit. Existing entries are preserved
    /// — local HIR definitions always win.
    pub fn register_def(&mut self, def_id: DefId, name: Symbol) {
        self.names.entry(def_id).or_insert(name);
    }
}

/// Collect names from top-level HIR items.
#[must_use]
pub fn collect_names(hir: &HirProgram) -> FxHashMap<DefId, Symbol> {
    let mut collector = NameCollector {
        names: FxHashMap::default(),
    };
    collector.visit_program(hir);
    collector.names
}

struct NameCollector {
    names: FxHashMap<DefId, Symbol>,
}

impl HirVisitor for NameCollector {
    fn visit_def(&mut self, def_id: DefId, name: Symbol) {
        self.names.insert(def_id, name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radix_hir::HirProgram;

    // PRP-025: resolve_def must return Err for unknown DefId, not a sentinel.
    #[test]
    fn resolve_def_fails_for_missing() {
        let interner = Interner::new();
        let hir = HirProgram {
            items: Vec::new(),
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };
        let catalog = NameCatalog::new(&hir, &interner);
        let result = catalog.resolve_def(DefId(999));
        assert!(result.is_err(), "unknown DefId must return Err");
        let err = result.unwrap_err();
        assert_eq!(err.def_id, DefId(999));
        assert!(
            MissingDefError::MARKER.contains("unresolved"),
            "marker must be visibly non-identifier-like"
        );
    }

    /// Empty HIR program produces empty catalog and `resolve_def` fails.
    #[test]
    fn empty_catalog_fails_resolve() {
        let interner = Interner::new();
        let hir = HirProgram {
            items: Vec::new(),
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };
        let catalog = NameCatalog::new(&hir, &interner);
        assert!(!catalog.contains_def(DefId(0)));
        assert!(catalog.resolve_def(DefId(0)).is_err());
    }

    /// Symbol resolution through the interner round-trips correctly.
    #[test]
    fn resolve_symbol_roundtrip() {
        let mut interner = Interner::new();
        let hir = HirProgram {
            items: Vec::new(),
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };
        let sym = interner.intern("hello");
        let catalog = NameCatalog::new(&hir, &interner);
        assert_eq!(catalog.resolve_symbol(sym), "hello");
    }
}
