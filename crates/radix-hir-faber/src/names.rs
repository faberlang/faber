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

use handroll::FxHashMap;
use radix_hir::visit::HirVisitor;
use radix_hir::{DefId, HirItemKind, HirModule};
use radix_lexer::Symbol;

impl super::FaberCodegen<'_> {
    /// Conversation-family identity of a compiler-owned frame struct.
    ///
    /// WHY: `sermo`/`scrinium`/`meus`/`tuus` are builtin structs, not user
    /// nominals, so their spelling projects through the target pack
    /// (DECISIONS D6.16). A user definition never matches: only builtin
    /// `DefId`s qualify.
    pub(super) fn builtin_frame_family(
        &self,
        def_id: DefId,
        names: &FxHashMap<DefId, Symbol>,
    ) -> Option<crate::surface::FrameFamily> {
        if !def_id.is_builtin() {
            return None;
        }
        let sym = names.get(&def_id)?;
        crate::surface::FrameFamily::from_latin(self.interner.resolve(*sym))
    }

    /// Carrier identity of the compiler-owned `erratum` genus.
    ///
    /// WHY: `erratum` is a compiler-owned builtin genus, so its type head
    /// projects through the target pack (`error` in en, `erratum` in la).
    /// Re-emitting the Latin definition name would produce a spelling the
    /// target pack rejects (`PARSE040 wrong_locale_type`). The `is_builtin()`
    /// guard keeps a user genus that happens to be named `erratum` on its own
    /// spelling.
    pub(super) fn builtin_erratum_carrier(
        &self,
        def_id: DefId,
        names: &FxHashMap<DefId, Symbol>,
    ) -> Option<crate::surface::TypeBindingKind> {
        if !def_id.is_builtin() {
            return None;
        }
        let sym = names.get(&def_id)?;
        (self.interner.resolve(*sym) == "erratum")
            .then_some(crate::surface::TypeBindingKind::Erratum)
    }

    /// Return the printable spelling for an already-resolved definition.
    ///
    /// The fallback is for robustness at the output boundary, not a resolver.
    /// A missing entry means the HIR did not expose a source name to this pass.
    pub(super) fn name_for_def(&self, def_id: DefId, names: &FxHashMap<DefId, Symbol>) -> String {
        names.get(&def_id).map_or_else(
            || missing_def_spelling(def_id),
            |sym| self.symbol_to_string(*sym),
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
    ) -> String {
        self.imported_def_spellings
            .get(&def_id)
            .cloned()
            .unwrap_or_else(|| self.name_for_def(def_id, names))
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
    pub(super) fn symbol_to_string(&self, sym: Symbol) -> String {
        self.interner.resolve(sym).to_owned()
    }

    /// Collect `DefId` -> Symbol mappings for all definitions in the program.
    ///
    /// The collection pass keeps emission deterministic and cheap: expression,
    /// type, pattern, and declaration writers can print resolved references
    /// without walking surrounding HIR to rediscover names.
    pub(super) fn collect_names(&self, hir: &HirModule) -> FxHashMap<DefId, Symbol> {
        let mut collector = FaberNameCollector {
            names: FxHashMap::default(),
        };
        collector.visit_program(hir);
        collector.names
    }

    /// Union-qualified spellings (`Union.VARIANT`) for local variants whose
    /// bare name cannot re-resolve: the same variant name declared in two
    /// local unions. (A local/imported collision cannot reach here — the
    /// authored bare spelling would already fail check.)
    pub(super) fn ambiguous_local_variant_spellings(
        &self,
        hir: &HirModule,
    ) -> FxHashMap<DefId, String> {
        let mut counts: handroll::FxHashMap<Symbol, usize> = handroll::FxHashMap::default();
        for item in &hir.items {
            if let HirItemKind::Enum(enm) = &item.kind {
                for variant in &enm.variants {
                    *counts.entry(variant.name).or_default() += 1;
                }
            }
        }

        let mut qualified = FxHashMap::default();
        for item in &hir.items {
            let HirItemKind::Enum(enm) = &item.kind else {
                continue;
            };
            let union_name = self.symbol_to_string(enm.name);
            for variant in &enm.variants {
                if counts.get(&variant.name).is_some_and(|n| *n > 1) {
                    let variant_name = self.symbol_to_string(variant.name);
                    qualified.insert(variant.def_id, format!("{union_name}.{variant_name}"));
                }
            }
        }
        qualified
    }

    /// Imported variant def ids that must take the three-part pattern spelling
    /// (`ns.Union.VARIANT`): the bare variant name is declared by two imported
    /// namespaces, or collides with a local union variant. Every other
    /// imported variant keeps the authored two-part `ns.VARIANT` form.
    pub(super) fn imported_variants_needing_union_path(
        &self,
        hir: &HirModule,
    ) -> handroll::FxHashSet<DefId> {
        // spelling "ns.Union.VARIANT" → (namespace, variant name) pairs.
        let mut namespaces_by_variant: handroll::FxHashMap<&str, handroll::FxHashSet<&str>> =
            handroll::FxHashMap::default();
        let mut defs_by_spelling: Vec<(DefId, &str)> = Vec::new();
        for (def_id, spelling) in &self.imported_pattern_spellings {
            let parts: Vec<&str> = spelling.split('.').collect();
            if parts.len() < 3 {
                continue;
            }
            let namespace = parts[0];
            let variant = parts[parts.len() - 1];
            namespaces_by_variant
                .entry(variant)
                .or_default()
                .insert(namespace);
            defs_by_spelling.push((*def_id, variant));
        }
        let local_variant_names: handroll::FxHashSet<&str> = hir
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                HirItemKind::Enum(enm) => Some(&enm.variants),
                _ => None,
            })
            .flatten()
            .map(|variant| self.interner.resolve(variant.name))
            .collect();

        defs_by_spelling
            .into_iter()
            .filter(|(_, variant)| {
                namespaces_by_variant
                    .get(variant)
                    .is_some_and(|namespaces| namespaces.len() > 1)
                    || local_variant_names.contains(variant)
            })
            .map(|(def_id, _)| def_id)
            .collect()
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

    // A folded static re-emits its authored initializer, whose locals (a
    // `praefixum` body's bindings, D8.5) live only there.
    fn visit_item(&mut self, item: &radix_hir::HirItem) {
        if let HirItemKind::Constant(konst) = &item.kind
            && let Some(authored) = konst
                .statica
                .as_ref()
                .and_then(|statica| statica.authored.as_deref())
        {
            self.visit_expr(authored);
        }
        radix_hir::visit::walk_item(self, item);
    }
}
