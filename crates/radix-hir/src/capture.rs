//! Closure capture facts for by-reference capture (operator ruling
//! 2026-09-24).
//!
//! An unmarked closure shares its enclosing scope: a captured name IS the
//! enclosing binding. Backends only need real shared storage for a binding
//! when sharing is observable, i.e. when the binding is rebound after its
//! declaration — anywhere, inside or outside a closure. A binding that is
//! never rebound can be copied at capture with identical behavior, so it
//! keeps the cheap value capture.
//!
//! Scope of the facts: locals declared in the walked body (including locals
//! declared inside nested closures). Parameters and loop bindings are not
//! tracked; `for … const i` bindings are fresh per iteration and never
//! rebound, so a closure created in a loop sees its own `i`.
//!
//! Backends whose aggregates are reference carriers (the MIR runner) only
//! need [`CaptureFacts::shared`]. Backends with value-semantics carriers (Rust
//! `Vec`/`String`) also consult [`CaptureFacts::content_mutated`]: a captured
//! aggregate whose content is mutated in place must share storage too.

use crate::nodes::{
    HirBlock, HirExpression, HirExpressionKind, HirLocal, HirModule, HirParam, HirStatement,
    HirStatementKind,
};
use crate::visit::{HirVisitor, walk_expr, walk_local, walk_stmt};
use radix_types::DefId;
use rustc_hash::{FxHashMap, FxHashSet};

/// Capture facts for one callable body (or a whole module).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CaptureFacts {
    /// Locals (`const` or `var`) referenced from inside a closure nested
    /// deeper than the declaration.
    pub captured: FxHashSet<DefId>,
    /// Captured `var` locals that are also rebound (`←`, `↤`, `↑`/`↓` on the
    /// bare name) somewhere in the body. These need shared-cell storage.
    pub shared: FxHashSet<DefId>,
    /// Captured locals that are the root of an in-place content mutation: a
    /// method-call receiver (`xs.append(…)`), a field/index assignment target
    /// (`xs[0] ← …`), or a `⇇` copy-into target (`xs ⇇ ys`). Conservative:
    /// any method call counts.
    pub content_mutated: FxHashSet<DefId>,
}

impl CaptureFacts {
    /// Facts for a callable body block.
    #[must_use]
    pub fn for_block(block: &HirBlock) -> Self {
        let mut collector = CaptureCollector::default();
        collector.visit_block(block);
        collector.finish()
    }

    /// Facts for an arbitrary expression body (closure expression bodies).
    #[must_use]
    pub fn for_expr(expr: &HirExpression) -> Self {
        let mut collector = CaptureCollector::default();
        collector.visit_expr(expr);
        collector.finish()
    }

    /// Facts for every callable body and the entry block of a module.
    #[must_use]
    pub fn for_module(module: &HirModule) -> Self {
        let mut collector = CaptureCollector::default();
        collector.visit_program(module);
        collector.finish()
    }

    #[must_use]
    pub fn is_shared(&self, def_id: DefId) -> bool {
        self.shared.contains(&def_id)
    }
}

#[derive(Default)]
struct CaptureCollector {
    depth: u32,
    /// Closure depth at which each tracked local is declared.
    declared: FxHashMap<DefId, u32>,
    /// Tracked locals declared `var`.
    mutable: FxHashSet<DefId>,
    /// Deepest closure depth at which each def is referenced.
    deepest_use: FxHashMap<DefId, u32>,
    rebound: FxHashSet<DefId>,
    content_mutated: FxHashSet<DefId>,
}

impl CaptureCollector {
    fn finish(self) -> CaptureFacts {
        let captured: FxHashSet<DefId> = self
            .declared
            .iter()
            .filter(|(def_id, depth)| {
                self.deepest_use
                    .get(def_id)
                    .is_some_and(|used| used > depth)
            })
            .map(|(def_id, _)| *def_id)
            .collect();
        let shared = captured
            .iter()
            .copied()
            .filter(|def_id| self.mutable.contains(def_id) && self.rebound.contains(def_id))
            .collect();
        let content_mutated = captured
            .iter()
            .copied()
            .filter(|def_id| self.content_mutated.contains(def_id))
            .collect();
        CaptureFacts {
            captured,
            shared,
            content_mutated,
        }
    }

    fn note_rebind(&mut self, target: &HirExpression) {
        match &target.kind {
            HirExpressionKind::Path(def_id) => {
                self.rebound.insert(*def_id);
            }
            HirExpressionKind::Field(base, _) | HirExpressionKind::Index(base, _) => {
                self.note_content_mutation(base);
            }
            _ => {}
        }
    }

    fn note_content_mutation(&mut self, place: &HirExpression) {
        match &place.kind {
            HirExpressionKind::Path(def_id) => {
                self.content_mutated.insert(*def_id);
            }
            HirExpressionKind::Field(base, _) | HirExpressionKind::Index(base, _) => {
                self.note_content_mutation(base);
            }
            _ => {}
        }
    }

    fn visit_closure(&mut self, params: &[HirParam], body: &HirExpression) {
        self.depth += 1;
        for param in params {
            self.visit_param(param);
        }
        self.visit_expr(body);
        self.depth -= 1;
    }
}

impl HirVisitor for CaptureCollector {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::IncDec(inc_dec) = &stmt.kind {
            self.note_rebind(&inc_dec.target);
        }
        walk_stmt(self, stmt);
    }

    fn visit_local(&mut self, local: &HirLocal) {
        self.declared.insert(local.def_id, self.depth);
        if local.mutable {
            self.mutable.insert(local.def_id);
        }
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                let deepest = self.deepest_use.entry(*def_id).or_insert(self.depth);
                *deepest = (*deepest).max(self.depth);
            }
            HirExpressionKind::Assign(target, _)
            | HirExpressionKind::ConversioAssign { target, .. } => {
                self.note_rebind(target);
            }
            HirExpressionKind::MethodCall(receiver, ..) => self.note_content_mutation(receiver),
            HirExpressionKind::CopyInto { target, .. } => self.note_content_mutation(target),
            HirExpressionKind::SinkInto { sink, .. } => self.note_content_mutation(sink),
            HirExpressionKind::Clausura(params, _, _, body) => {
                self.visit_closure(params, body);
                return;
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

#[cfg(test)]
#[path = "capture_test.rs"]
mod capture_tests;
