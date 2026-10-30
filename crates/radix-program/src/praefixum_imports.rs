//! ST2-2.6 / ST2-2.11: the program layer answers import-effect questions.
//!
//! `radix-module` can prove a comptime body pure only inside its own module;
//! `radix_semantic::passes::praefixum::function_effect` asks an
//! [`ImportedEffects`] about every place a body reaches into an import.
//! [`PackageImportEffects`] answers from the analyzed nodes of a package: it
//! resolves an import namespace to a sibling module through the package's
//! [`ModuleGraph`] or to a library module through the importing node's
//! expanded library bindings, the member to a top-level function or constant
//! of that module, and judges a function by running `function_effect` over
//! the module's own HIR with a resolver for that module's own imports.
//!
//! Library bodies ride a caller-supplied [`LibraryBodies`] table rather than
//! the [`crate::library::LibraryInterfaceCache`]: the deferral hook runs the
//! MIR callback with the cache mutably borrowed, so the resolver owns the
//! analyzed library modules it may walk.
//!
//! The answer for anything not resolved is [`ImportedCallee::Unavailable`]: an
//! unknown or unexported member, a bodyless library function, a method whose
//! receiver no sibling import exports or that has no such method there, a
//! selective import, a library with no supplied body. A wrong `Pure` would be an unsound
//! build, so `Pure` is only ever the answer for a proven pure function or a
//! top-level constant.

use crate::LibraryImportBinding;
use crate::analyze::{AnalyzedProgramNode, ModuleGraph, ModuleId};
use radix_module::driver::AnalyzedModule;
use radix_module::hir::{DefId, HirItemKind, LibraryIdentity, LibraryProvider};
use radix_module::semantic::passes::praefixum::{
    ImportedCallee, ImportedEffects, ImportedReference, ImportedReferenceKind, function_effect,
};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The canonical `(provider package, module path)` identity of one library.
type LibraryKey = (String, Vec<String>);

/// Analyzed library modules the resolver may walk, keyed by
/// `(provider package, module path)`.
///
/// Owned by the resolver: the deferral hook runs the MIR callback with the
/// library cache mutably borrowed for the same call, so the answers cannot
/// borrow the cache.
pub type LibraryBodies = BTreeMap<(String, Vec<String>), AnalyzedModule>;

/// Per-module, per-definition answers plus the cycle guard for one program
/// walk. The module half is a sibling [`ModuleId`] or a library identity.
#[derive(Default)]
struct ImportMemo {
    done: HashMap<(Home, DefId), ImportedCallee>,
    in_progress: HashSet<(Home, DefId)>,
    /// Times the guard answered for a function still being judged.
    cycle_hits: u32,
    depth: u32,
}

/// The module a function is defined in: a package sibling or a library.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Home {
    Package(ModuleId),
    Library(LibraryKey),
}

/// Import-effect answers over the analyzed nodes of a package, for the
/// references of one module (`from`).
pub struct PackageImportEffects<'a> {
    nodes: &'a BTreeMap<ModuleId, AnalyzedProgramNode>,
    imports: &'a ModuleGraph,
    /// Library bodies this resolver may walk. Empty unless the caller
    /// supplies them through [`Self::with_library_bodies`]; a library
    /// reference with no body supplied stays `Unavailable`.
    libraries: LibraryBodies,
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
            libraries: LibraryBodies::new(),
            from,
            memo: ImportMemo::default(),
        }
    }

    /// Supply the analyzed library modules the resolver may walk for library
    /// references. A reference whose library or member is absent stays
    /// `Unavailable`.
    #[must_use]
    pub fn with_library_bodies(mut self, libraries: LibraryBodies) -> Self {
        self.libraries = libraries;
        self
    }
}

impl ImportedEffects for PackageImportEffects<'_> {
    fn callee(&mut self, reference: &ImportedReference) -> ImportedCallee {
        Scope {
            nodes: self.nodes,
            imports: self.imports,
            libraries: &self.libraries,
            from: Home::Package(self.from.clone()),
            memo: &mut self.memo,
        }
        .callee(reference)
    }
}

/// The resolver for one module's references, sharing the program-wide memo.
struct Scope<'s> {
    nodes: &'s BTreeMap<ModuleId, AnalyzedProgramNode>,
    imports: &'s ModuleGraph,
    libraries: &'s LibraryBodies,
    from: Home,
    memo: &'s mut ImportMemo,
}

impl<'s> Scope<'s> {
    /// The sibling module and member name a namespace reference names, when
    /// the module's own imports name it and it exports the member.
    fn sibling(
        &self,
        module_id: &ModuleId,
        reference: &ImportedReference,
    ) -> Option<(&'s ModuleId, &'s AnalyzedProgramNode, String)> {
        let nodes: &'s BTreeMap<ModuleId, AnalyzedProgramNode> = self.nodes;
        let node = nodes.get(module_id)?;
        let binding = namespace_binding(node, reference.namespace)?;
        let (_, target_id) = self
            .imports
            .local_edges(module_id)
            .iter()
            .find(|(edge, _)| *edge == binding)?;
        let target = nodes.get(target_id)?;
        let member = node.analysis.interner.resolve(reference.member).to_owned();
        node.namespace_exports.get(&binding)?.get(&member)?;
        Some((target_id, target, member))
    }

    /// Answer one reference made by a package module.
    fn package(&mut self, module_id: &ModuleId, reference: &ImportedReference) -> ImportedCallee {
        if reference.kind == ImportedReferenceKind::Method {
            return self.method(module_id, reference);
        }
        if let Some((target_id, target, member)) = self.sibling(module_id, reference) {
            return match reference.kind {
                ImportedReferenceKind::Value => {
                    if has_top_level_constant(&target.analysis, &member) {
                        ImportedCallee::Pure
                    } else {
                        ImportedCallee::Unavailable
                    }
                }
                ImportedReferenceKind::Call => {
                    match top_level_function(&target.analysis, &member) {
                        Some(def_id) => self.function(&Home::Package(target_id.clone()), def_id),
                        None => ImportedCallee::Unavailable,
                    }
                }
                // Handled before the namespace lookup: a method receiver
                // is a struct definition, never an import binding.
                ImportedReferenceKind::Method => ImportedCallee::Unavailable,
            };
        }
        let nodes: &'s BTreeMap<ModuleId, AnalyzedProgramNode> = self.nodes;
        let Some(node) = nodes.get(module_id) else {
            return ImportedCallee::Unavailable;
        };
        let Some(binding) = namespace_binding(node, reference.namespace) else {
            return ImportedCallee::Unavailable;
        };
        // A binding with no local edge is a library edge of this node.
        let Some(import) = node
            .expanded_library_imports
            .iter()
            .find(|import| import.binding == binding)
        else {
            return ImportedCallee::Unavailable;
        };
        let member = node.analysis.interner.resolve(reference.member).to_owned();
        self.library_member(edge_key(import), &member, reference.kind)
    }

    /// Answer a method reference: the receiver's genus is declared in a
    /// sibling module, and the import whose file interface exports the
    /// receiver's `DefId` names that module. The method is looked up by name
    /// on the genus there and judged like a function of that module.
    fn method(&mut self, module_id: &ModuleId, reference: &ImportedReference) -> ImportedCallee {
        let Some(node) = self.nodes.get(module_id) else {
            return ImportedCallee::Unavailable;
        };
        let Some(symbol) = node.analysis.resolver.get_symbol(reference.namespace) else {
            return ImportedCallee::Unavailable;
        };
        let genus = node.analysis.interner.resolve(symbol.name).to_owned();
        let Some((home_id, home)) = self.receiver_home(module_id, reference.namespace) else {
            return ImportedCallee::Unavailable;
        };
        let method = node.analysis.interner.resolve(reference.member).to_owned();
        let Some(def_id) = genus_method(&home.analysis, &genus, &method) else {
            return ImportedCallee::Unavailable;
        };
        self.function(&Home::Package(home_id.clone()), def_id)
    }

    /// The local edge of `module_id` whose installed file interface exports
    /// the nominal `receiver`. Ownership is by the receiver's identity, never
    /// by export spelling: a homonym genus in another sibling does not
    /// compete, and a receiver no edge exports has no home.
    fn receiver_home(
        &self,
        module_id: &ModuleId,
        receiver: DefId,
    ) -> Option<(&'s ModuleId, &'s AnalyzedProgramNode)> {
        let node = self.nodes.get(module_id)?;
        let resolver = &node.analysis.resolver;
        let (_, target_id) = self
            .imports
            .local_edges(module_id)
            .iter()
            .find(|(binding, _)| {
                resolver
                    .file_interface_namespace(binding, &node.analysis.interner)
                    .is_some_and(|namespace| {
                        resolver
                            .imported_file_type_exports(namespace)
                            .iter()
                            .any(|export| export.def_id == receiver)
                    })
            })?;
        Some((target_id, self.nodes.get(target_id)?))
    }

    /// Answer one reference made by a library module.
    fn library(&mut self, key: &LibraryKey, reference: &ImportedReference) -> ImportedCallee {
        let libraries: &'s LibraryBodies = self.libraries;
        let Some(analysis) = libraries.get(key) else {
            return ImportedCallee::Unavailable;
        };
        let Some(binding) = analysis.libraries.bindings.get(&reference.namespace) else {
            return ImportedCallee::Unavailable;
        };
        let member = analysis.interner.resolve(reference.member).to_owned();
        self.library_member(identity_key(&binding.identity), &member, reference.kind)
    }

    /// Resolve a library binding and member by name and judge it as the
    /// library module's own function or constant. `member` is spelled in the
    /// referring module's interner.
    fn library_member(
        &mut self,
        key: LibraryKey,
        member: &str,
        kind: ImportedReferenceKind,
    ) -> ImportedCallee {
        let libraries: &'s LibraryBodies = self.libraries;
        let Some(analysis) = libraries.get(&key) else {
            return ImportedCallee::Unavailable;
        };
        match kind {
            ImportedReferenceKind::Value => {
                if has_top_level_constant(analysis, member) {
                    ImportedCallee::Pure
                } else {
                    ImportedCallee::Unavailable
                }
            }
            ImportedReferenceKind::Call => {
                let Some(def_id) = top_level_function(analysis, member) else {
                    return ImportedCallee::Unavailable;
                };
                // A bodyless (interface/externa) declaration cannot run.
                if !function_has_body(analysis, def_id) {
                    return ImportedCallee::Unavailable;
                }
                self.function(&Home::Library(key), def_id)
            }
            ImportedReferenceKind::Method => ImportedCallee::Unavailable,
        }
    }

    /// The effect of one function, memoized with a cycle guard.
    fn function(&mut self, home: &Home, def_id: DefId) -> ImportedCallee {
        let key = ((*home).clone(), def_id);
        if let Some(known) = self.memo.done.get(&key) {
            return known.clone();
        }
        if self.memo.in_progress.contains(&key) {
            // Recursion across modules: the cycle's own members report their
            // effects.
            self.memo.cycle_hits += 1;
            return ImportedCallee::Pure;
        }
        let nodes: &'s BTreeMap<ModuleId, AnalyzedProgramNode> = self.nodes;
        let libraries: &'s LibraryBodies = self.libraries;
        let module = match home {
            Home::Package(module_id) => nodes.get(module_id).map(|node| &node.analysis),
            Home::Library(key) => libraries.get(key),
        };
        let Some(module) = module else {
            return ImportedCallee::Unavailable;
        };
        self.memo.in_progress.insert(key.clone());
        self.memo.depth += 1;
        let hits_before = self.memo.cycle_hits;
        let found = {
            let mut inner = Scope {
                nodes,
                imports: self.imports,
                libraries,
                from: (*home).clone(),
                memo: &mut *self.memo,
            };
            function_effect(
                &module.hir,
                &module.types,
                &module.interner,
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
        match self.from.clone() {
            Home::Package(module_id) => self.package(&module_id, reference),
            Home::Library(key) => self.library(&key, reference),
        }
    }
}

/// The local binding name of the import binding `namespace`, when the
/// module's HIR imports it.
fn namespace_binding(node: &AnalyzedProgramNode, namespace: DefId) -> Option<String> {
    node.analysis.hir.items.iter().find_map(|item| {
        let HirItemKind::Import(import) = &item.kind else {
            return None;
        };
        if import.selective {
            return None;
        }
        import
            .items
            .iter()
            .find(|entry| entry.def_id == namespace)
            .map(|entry| {
                node.analysis
                    .interner
                    .resolve(entry.alias.unwrap_or(entry.name))
                    .to_owned()
            })
    })
}

/// The canonical `(provider package, module path)` key of a library edge.
fn edge_key(import: &LibraryImportBinding) -> LibraryKey {
    (
        import.module.package.clone(),
        import.module.module_path.clone(),
    )
}

/// The canonical `(provider package, module path)` key of a library identity.
fn identity_key(identity: &LibraryIdentity) -> LibraryKey {
    let package = match &identity.provider {
        LibraryProvider::Builtin(package) | LibraryProvider::Package(package) => package.clone(),
    };
    (package, identity.module_path.clone())
}

/// The definition of the method `method` on the genus `genus` declared in
/// `analysis`'s HIR.
fn genus_method(analysis: &AnalyzedModule, genus: &str, method: &str) -> Option<DefId> {
    analysis.hir.items.iter().find_map(|item| {
        let HirItemKind::Struct(strukt) = &item.kind else {
            return None;
        };
        if analysis.interner.resolve(strukt.name) != genus {
            return None;
        }
        strukt
            .methods
            .iter()
            .find(|candidate| analysis.interner.resolve(candidate.func.name) == method)
            .map(|candidate| candidate.def_id)
    })
}

fn top_level_function(analysis: &AnalyzedModule, name: &str) -> Option<DefId> {
    analysis.hir.items.iter().find_map(|item| {
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        (analysis.interner.resolve(function.name) == name).then_some(item.def_id)
    })
}

fn function_has_body(analysis: &AnalyzedModule, def_id: DefId) -> bool {
    analysis.hir.items.iter().any(|item| {
        item.def_id == def_id
            && matches!(&item.kind, HirItemKind::Function(function) if function.body.is_some())
    })
}

fn has_top_level_constant(analysis: &AnalyzedModule, name: &str) -> bool {
    analysis.hir.items.iter().any(|item| {
        matches!(&item.kind, HirItemKind::Constant(konst)
            if analysis.interner.resolve(konst.name) == name)
    })
}

#[cfg(all(test, feature = "package-mir"))]
#[path = "praefixum_imports_test.rs"]
mod praefixum_imports_test;
