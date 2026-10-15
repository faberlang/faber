//! Local compile-time constants: `fixum T x = e` (DECISIONS D5.10).
//!
//! A local constant is an ordinary immutable local whose initializer is a
//! compile-time fact. Its initializer must be evaluable by the same evaluator
//! that folds module constants ([`super::statica::fold_with_imports`]), so
//! this pass hands that evaluator a scratch module made of the already-folded
//! module constants plus every local constant, and reports what it rejects
//! under the same issue names (one condition, one identity). `module.NAME`
//! references to imported constants resolve through the resolver, exactly as
//! they do for module constants. The scratch module is
//! discarded: the local keeps its authored initializer, so every backend and
//! Faber re-emission see an ordinary immutable local.
//!
//! The evaluable surface is therefore exactly the module constants' surface:
//! literals, arithmetic on scalars, other constants (module and local), and
//! collections of those. The value must fit the declared type whatever its
//! overflow policy (`=` states a compile-time fact).

use super::statica;
use crate::{Resolver, SemanticError, TypeTable};
use radix_hir::visit::{HirVisitor, walk_local, walk_program};
use radix_hir::{
    DefId, HirConst, HirId, HirItem, HirItemKind, HirLocal, HirModule, HirPresentation, HirStatic,
};
use radix_lexer::Interner;
use rustc_hash::FxHashSet;

/// Evaluate every local compile-time constant and report the ones that are
/// not constant or do not fit their type.
///
/// Runs after [`statica::fold`] succeeded, so the module constants copied into the
/// scratch module are already valid and folded; every error the scratch fold
/// reports belongs to a local constant.
pub fn check(
    hir: &HirModule,
    presentation: &HirPresentation,
    types: &TypeTable,
    interner: &mut Interner,
    resolver: &Resolver,
) -> Vec<SemanticError> {
    if presentation.compile_time_locals.is_empty() {
        return Vec::new();
    }
    let marked: FxHashSet<DefId> = presentation.compile_time_locals.iter().copied().collect();
    let mut scratch = HirModule {
        items: hir
            .items
            .iter()
            .filter(|item| {
                matches!(&item.kind, HirItemKind::Constant(konst) if konst.statica.is_some())
            })
            .cloned()
            .collect(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let mut collector = Collector {
        marked: &marked,
        found: Vec::new(),
    };
    walk_program(&mut collector, hir);
    if collector.found.is_empty() {
        return Vec::new();
    }
    let mut next_id = statica::max_hir_id(hir) + 1;
    for local in collector.found {
        let (Some(init), Some(ty)) = (local.init, local.ty) else {
            continue;
        };
        scratch.items.push(HirItem {
            id: HirId(next_id),
            def_id: local.def_id,
            annotations: Vec::new(),
            span: init.span,
            kind: HirItemKind::Constant(HirConst {
                name: local.name,
                ty: Some(ty),
                value: init,
                mutable: false,
                is_await: false,
                statica: Some(HirStatic { authored: None }),
            }),
        });
        next_id += 1;
    }

    statica::fold_with_imports(&mut scratch, types, interner, resolver)
}

struct Collector<'a> {
    marked: &'a FxHashSet<DefId>,
    found: Vec<HirLocal>,
}

impl HirVisitor for Collector<'_> {
    fn visit_local(&mut self, local: &HirLocal) {
        if self.marked.contains(&local.def_id) {
            self.found.push(local.clone());
        }
        walk_local(self, local);
    }
}
