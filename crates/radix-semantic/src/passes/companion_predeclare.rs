//! Pre-resolve companion name registration for `@ radix backward` functions.
//!
//! This pass runs after [`collect`](super::collect::collect) (which registers
//! top-level declaration names) and before [`resolve`](super::resolve::resolve)
//! (which resolves identifier references). Its sole job is to register the
//! names of generated backward companion functions in the resolver so source
//! code that calls the companion by name does not receive `SEM001
//! unknown_identifier`.
//!
//! The companion bodies are generated much later (MIR reverse-AD transform,
//! Stages A–D), but name registration must happen before resolve so the call
//! site can bind. Post-resolve `radix_lane::collect` consumes the pre-allocated
//! `DefId`s idempotently — it only allocates a new identity when one was not
//! pre-registered.
//!
//! INVARIANTS
//! ==========
//! - Companion `DefId`s use the `0x8000_0000` high-bit scheme shared with
//!   [`RadixLaneMetadata::allocate_companion_def_id`].
//! - Only functions that carry **both** `@ radix lane "air"` and
//!   `@ radix backward "name"` are pre-registered.
//! - This pass does **not** emit diagnostics for malformed `@ radix`
//!   annotations — `radix_lane::collect` owns all error reporting for those.

use crate::radix_lane;
use crate::{Resolver, SemanticError, SemanticErrorKind, Symbol, SymbolKind};
use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol as LexSymbol};
use radix_syntax::{Annotation, Program, StmtKind};
use rustc_hash::FxHashMap;

/// Pre-allocated backward companion identities ready for consumption by
/// [`radix_lane::collect`].
#[derive(Debug, Clone, Default)]
pub struct CompanionPredeclarations {
    /// Companion name → pre-allocated `DefId`.
    pub by_name: FxHashMap<LexSymbol, DefId>,
    /// Highest companion index allocated so `radix_lane::collect` can seed its
    /// counter past ours.
    pub max_companion_index: u32,
}

/// Pre-register backward companion names in the resolver before the resolve
/// pass binds identifier references.
///
/// # Errors
///
/// Returns an error list when registering a companion name collides with an
/// already-defined symbol in the same scope. Malformed `@ radix` annotations
/// are ignored — `radix_lane::collect` owns all annotation-level diagnostics.
pub fn predeclare(
    program: &Program,
    resolver: &mut Resolver,
    interner: &Interner,
) -> Result<CompanionPredeclarations, Vec<SemanticError>> {
    let mut errors = Vec::new();
    let mut next_companion_id: u32 = 0;
    let mut by_name: FxHashMap<LexSymbol, DefId> = FxHashMap::default();

    for stmt in &program.statements {
        let StmtKind::Func(decl) = &stmt.kind else {
            continue;
        };

        // Collect annotations from both the statement and the function decl.
        let annotations: Vec<&Annotation> = stmt
            .annotations
            .iter()
            .chain(decl.annotations.iter())
            .collect();

        // Reuse the existing annotation parsers — throw away their errors
        // because `radix_lane::collect` owns full error reporting.
        let mut throwaway = Vec::new();

        let lane =
            radix_lane::collect_lane_from_annotations(&annotations, interner, &mut throwaway);
        let backward =
            radix_lane::collect_backward_from_annotations(&annotations, interner, &mut throwaway);

        // Only pre-register when both annotations are present and valid.
        let Some(backward) = backward else { continue };
        if lane != Some(radix_lane::RadixLane::Air) {
            continue;
        }

        // The primal function was registered during `passes::collect` — its
        // DefId must be in scope.
        let Some(_primal_def_id) = resolver.lookup(decl.name.name) else {
            // Should not happen: collect already registered this name.
            continue;
        };

        // Allocate companion DefId with the stable 0x8000_0000 high-bit scheme.
        let companion_def_id = DefId(next_companion_id | 0x8000_0000);
        next_companion_id = next_companion_id.wrapping_add(1);

        // Register the companion name so resolve can bind calls to it.
        let companion_symbol = Symbol {
            def_id: companion_def_id,
            name: backward.companion_name,
            kind: SymbolKind::Function,
            ty: None,
            mutable: false,
            span: Span::default(), // No single AST span for a synthetic declaration.
        };

        if let Err(dup) = resolver.define(companion_symbol) {
            errors.push(
                SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, Span::default())
                    .with_arg("issue", "companion_name_collision")
                    .with_arg("name", interner.resolve(dup).to_string()),
            );
            continue;
        }

        by_name.insert(backward.companion_name, companion_def_id);
    }

    let max_companion_index = next_companion_id.saturating_sub(1);

    if errors.is_empty() {
        Ok(CompanionPredeclarations {
            by_name,
            max_companion_index,
        })
    } else {
        Err(errors)
    }
}
