//! Local compile-time constants: `fixum T x = e` (DECISIONS D5.10).
//!
//! A local constant is an ordinary immutable local whose initializer is a
//! compile-time fact. Its initializer must be evaluable by the same evaluator
//! that folds `generis` statics ([`super::statica::fold`]), so this pass hands
//! that evaluator a scratch module made of the already-folded statics plus
//! every local constant, and reports what it rejects under local-constant
//! issue names. The scratch module is discarded: the local keeps its authored
//! initializer, so every backend and Faber re-emission see an ordinary
//! immutable local.
//!
//! The evaluable surface is therefore exactly the statics' surface: literals,
//! arithmetic on scalars, other constants (statics and other local
//! constants), and collections of those. The value must fit the declared
//! type whatever its overflow policy (`=` states a compile-time fact).

use super::statica;
use crate::{SemanticError, SemanticErrorKind, TypeTable};
use radix_hir::visit::{HirVisitor, walk_local, walk_program};
use radix_hir::{
    DefId, HirConst, HirId, HirItem, HirItemKind, HirLocal, HirModule, HirPresentation, HirStatic,
};
use radix_lexer::Interner;
use rustc_hash::FxHashSet;

const ISSUE_NOT_CONSTANT: &str = "local_constant_not_constant";
const ISSUE_OVERFLOW: &str = "local_constant_overflow";
const ISSUE_DIVISION_BY_ZERO: &str = "local_constant_division_by_zero";

/// Evaluate every local compile-time constant and report the ones that are
/// not constant or do not fit their type.
///
/// Runs after [`statica::fold`] succeeded, so the statics copied into the
/// scratch module are already valid and folded; every error the scratch fold
/// reports belongs to a local constant.
pub fn check(
    hir: &HirModule,
    presentation: &HirPresentation,
    types: &TypeTable,
    interner: &mut Interner,
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

    statica::fold(&mut scratch, types, interner)
        .into_iter()
        .map(|error| relabel(&error))
        .collect()
}

/// Recast a static's fault as the matching local-constant fault.
fn relabel(error: &SemanticError) -> SemanticError {
    let issue = match error.issue() {
        Some("static_arithmetic_overflow") => ISSUE_OVERFLOW,
        Some("static_division_by_zero") => ISSUE_DIVISION_BY_ZERO,
        _ => ISSUE_NOT_CONSTANT,
    };
    let mut relabeled =
        SemanticError::structured(SemanticErrorKind::StaticInitializerInvalid, error.span)
            .with_arg("issue", issue);
    if let Some(name) = error.args.iter().find(|arg| arg.name == "name") {
        relabeled = relabeled.with_arg("name", name.value.clone());
    }
    relabeled
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
