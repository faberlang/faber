//! ST2-2.6: the program layer answers import-effect questions.
//!
//! `radix-module` can prove a comptime body pure only inside its own module;
//! `radix_semantic::passes::praefixum::function_effect` asks an
//! [`ImportedEffects`] about every place a body reaches into an import.
//! [`PackageImportEffects`] answers from the analyzed nodes of a package: it
//! resolves an import namespace to a sibling module through the package's
//! [`ModuleGraph`], the member to a top-level function or constant of that
//! sibling, and judges a function by running `function_effect` over the
//! sibling's own HIR with a resolver for the sibling's own imports.
//!
//! The answer for anything not resolved is [`ImportedCallee::Unavailable`]: a
//! library binding, an unknown or unexported member, a method reference, a
//! selective import. A wrong `Pure` would be an unsound build, so `Pure` is
//! only ever the answer for a proven pure function or a sibling constant.

use crate::analyze::{AnalyzedProgramNode, ModuleGraph, ModuleId};
use radix_module::hir::{DefId, HirItemKind};
use radix_module::semantic::passes::praefixum::{
    ImportedCallee, ImportedEffects, ImportedReference, ImportedReferenceKind, function_effect,
};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Per-(module, definition) answers plus the cycle guard for one program
/// walk.
#[derive(Default)]
struct ImportMemo {
    done: HashMap<(ModuleId, DefId), ImportedCallee>,
    in_progress: HashSet<(ModuleId, DefId)>,
    /// Times the guard answered for a function still being judged.
    cycle_hits: u32,
    depth: u32,
}

/// Import-effect answers over the analyzed nodes of a package, for the
/// references of one module (`from`).
pub struct PackageImportEffects<'a> {
    nodes: &'a BTreeMap<ModuleId, AnalyzedProgramNode>,
    imports: &'a ModuleGraph,
    from: ModuleId,
    memo: ImportMemo,
}

impl<'a> PackageImportEffects<'a> {
    #[must_use]
    pub fn new(
        nodes: &'a BTreeMap<ModuleId, AnalyzedProgramNode>,
        imports: &'a ModuleGraph,
        from: ModuleId,
    ) -> Self {
        Self {
            nodes,
            imports,
            from,
            memo: ImportMemo::default(),
        }
    }
}

impl ImportedEffects for PackageImportEffects<'_> {
    fn callee(&mut self, reference: &ImportedReference) -> ImportedCallee {
        Scope {
            nodes: self.nodes,
            imports: self.imports,
            from: &self.from,
            memo: &mut self.memo,
        }
        .callee(reference)
    }
}

/// The resolver for one module's references, sharing the program-wide memo.
struct Scope<'s> {
    nodes: &'s BTreeMap<ModuleId, AnalyzedProgramNode>,
    imports: &'s ModuleGraph,
    from: &'s ModuleId,
    memo: &'s mut ImportMemo,
}

impl<'s> Scope<'s> {
    /// The sibling module and member name a namespace reference names.
    fn resolve(
        &self,
        reference: &ImportedReference,
    ) -> Option<(&'s ModuleId, &'s AnalyzedProgramNode, String)> {
        let node = self.nodes.get(self.from)?;
        let binding = node.analysis.hir.items.iter().find_map(|item| {
            let HirItemKind::Import(import) = &item.kind else {
                return None;
            };
            if import.selective {
                return None;
            }
            import
                .items
                .iter()
                .find(|entry| entry.def_id == reference.namespace)
                .map(|entry| {
                    node.analysis
                        .interner
                        .resolve(entry.alias.unwrap_or(entry.name))
                        .to_owned()
                })
        })?;
        let (_, target_id) = self
            .imports
            .local_edges(self.from)
            .iter()
            .find(|(edge, _)| *edge == binding)?;
        let target = self.nodes.get(target_id)?;
        let member = node.analysis.interner.resolve(reference.member).to_owned();
        node.namespace_exports.get(&binding)?.get(&member)?;
        Some((target_id, target, member))
    }

    /// The effect of one sibling function, memoized with a cycle guard.
    fn function(
        &mut self,
        target_id: &ModuleId,
        target: &AnalyzedProgramNode,
        def_id: DefId,
    ) -> ImportedCallee {
        let key = (target_id.clone(), def_id);
        if let Some(known) = self.memo.done.get(&key) {
            return known.clone();
        }
        if self.memo.in_progress.contains(&key) {
            // Recursion across modules: the cycle's own members report their
            // effects.
            self.memo.cycle_hits += 1;
            return ImportedCallee::Pure;
        }
        self.memo.in_progress.insert(key.clone());
        self.memo.depth += 1;
        let hits_before = self.memo.cycle_hits;
        let found = {
            let mut inner = Scope {
                nodes: self.nodes,
                imports: self.imports,
                from: target_id,
                memo: &mut *self.memo,
            };
            function_effect(
                &target.analysis.hir,
                &target.analysis.types,
                &target.analysis.interner,
                def_id,
                &mut inner,
            )
        };
        self.memo.depth -= 1;
        self.memo.in_progress.remove(&key);
        // A `Pure` computed while a cycle member was still being judged is
        // provisional: it is final only at the outermost frame.
        let provisional = found == ImportedCallee::Pure
            && self.memo.cycle_hits != hits_before
            && self.memo.depth > 0;
        if !provisional {
            self.memo.done.insert(key, found.clone());
        }
        found
    }
}

impl ImportedEffects for Scope<'_> {
    fn callee(&mut self, reference: &ImportedReference) -> ImportedCallee {
        if reference.kind == ImportedReferenceKind::Method {
            return ImportedCallee::Unavailable;
        }
        let Some((target_id, target, member)) = self.resolve(reference) else {
            return ImportedCallee::Unavailable;
        };
        match reference.kind {
            ImportedReferenceKind::Value => {
                if has_top_level_constant(target, &member) {
                    ImportedCallee::Pure
                } else {
                    ImportedCallee::Unavailable
                }
            }
            ImportedReferenceKind::Call => match top_level_function(target, &member) {
                Some(def_id) => self.function(target_id, target, def_id),
                None => ImportedCallee::Unavailable,
            },
            ImportedReferenceKind::Method => ImportedCallee::Unavailable,
        }
    }
}

fn top_level_function(node: &AnalyzedProgramNode, name: &str) -> Option<DefId> {
    node.analysis.hir.items.iter().find_map(|item| {
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        (node.analysis.interner.resolve(function.name) == name).then_some(item.def_id)
    })
}

fn has_top_level_constant(node: &AnalyzedProgramNode, name: &str) -> bool {
    node.analysis.hir.items.iter().any(|item| {
        matches!(&item.kind, HirItemKind::Constant(konst)
            if node.analysis.interner.resolve(konst.name) == name)
    })
}

#[cfg(all(test, feature = "package-mir"))]
#[path = "praefixum_imports_test.rs"]
mod praefixum_imports_test;
