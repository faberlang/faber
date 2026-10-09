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
use radix_hir::{DefId, HirModule};
use radix_lexer::{Interner, Symbol};
pub use radix_runtime_contract::display_tokens::DisplayTokens;
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
    display_tokens: DisplayTokens,
}

impl<'a> NameCatalog<'a> {
    /// Build a catalog from the definition-bearing nodes in one HIR program.
    #[must_use]
    pub fn new(hir: &HirModule, interner: &'a Interner) -> Self {
        Self {
            names: collect_names(hir),
            interner,
            display_tokens: DisplayTokens::latin(),
        }
    }

    /// Output tokens (`true`/`false`/`none`/`tuple` words) of the module this
    /// catalog serves. One module is one locale, so one value per catalog.
    #[must_use]
    pub fn display_tokens(&self) -> &DisplayTokens {
        &self.display_tokens
    }

    /// Install the module's output tokens; the driver calls this once per
    /// codegen invocation with `display_tokens_for_module`.
    pub fn set_display_tokens(&mut self, tokens: DisplayTokens) {
        self.display_tokens = tokens;
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
    pub fn catalog_contains_def(&self, def_id: DefId) -> bool {
        self.names.contains_key(&def_id)
    }

    /// Definition identities present in the catalog (the local emit def table).
    pub fn registered_defs(&self) -> impl Iterator<Item = DefId> + '_ {
        self.names.keys().copied()
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
pub fn collect_names(hir: &HirModule) -> FxHashMap<DefId, Symbol> {
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
#[path = "names_test.rs"]
mod tests;
