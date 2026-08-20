//! Target-neutral derived access facts for binding initializers.
//!
//! This provider records only facts that can be established from the initializer
//! shape itself. Aggregate literals and constructors allocate fresh aggregate
//! storage; aliases, calls, and every other shape remain conservatively unknown.
//! The table is advisory data for downstream planning and never emits semantic
//! diagnostics or changes ownership behavior.

use radix_hir::visit::{walk_local, HirVisitor};
use radix_hir::{
    DefId, HirArrayElement, HirExpression, HirExpressionKind, HirId, HirLocal, HirModule,
};
use rustc_hash::FxHashMap;

/// Conservative storage-origin classification for one expression or binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessFact {
    /// The expression constructs aggregate storage at its evaluation site.
    Fresh,
    /// The provider cannot prove a fresh aggregate storage origin.
    Unknown,
}

/// Derived access facts indexed by both expression and binding identity.
///
/// A binding initializer is recorded under its expression [`HirId`] and its
/// local [`DefId`]. Queries default to [`AccessFact::Unknown`] when an identity
/// has no row, so downstream consumers do not need to re-walk HIR to stay
/// conservative.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessFactTable {
    by_expression: FxHashMap<HirId, AccessFact>,
    by_binding: FxHashMap<DefId, AccessFact>,
}

impl AccessFactTable {
    /// Return the fact for an expression, conservatively defaulting to unknown.
    #[must_use]
    pub fn expression(&self, id: HirId) -> AccessFact {
        self.by_expression
            .get(&id)
            .copied()
            .unwrap_or(AccessFact::Unknown)
    }

    /// Return the fact for a local binding, conservatively defaulting to unknown.
    #[must_use]
    pub fn binding(&self, def_id: DefId) -> AccessFact {
        self.by_binding
            .get(&def_id)
            .copied()
            .unwrap_or(AccessFact::Unknown)
    }

    /// Iterate over expression-site facts recorded from binding initializers.
    pub fn expressions(&self) -> impl Iterator<Item = (HirId, AccessFact)> + '_ {
        self.by_expression.iter().map(|(id, fact)| (*id, *fact))
    }

    /// Iterate over binding-site facts recorded from local declarations.
    pub fn bindings(&self) -> impl Iterator<Item = (DefId, AccessFact)> + '_ {
        self.by_binding
            .iter()
            .map(|(def_id, fact)| (*def_id, *fact))
    }

    /// Number of expression-site rows.
    #[must_use]
    pub fn expression_count(&self) -> usize {
        self.by_expression.len()
    }

    /// Number of binding-site rows.
    #[must_use]
    pub fn binding_count(&self) -> usize {
        self.by_binding.len()
    }

    fn record(&mut self, local: &HirLocal) {
        let Some(init) = &local.init else {
            return;
        };
        let fact = classify_initializer(init);
        self.by_expression.insert(init.id, fact);
        self.by_binding.insert(local.def_id, fact);
    }
}

/// Derive target-neutral access facts from a typed HIR module.
///
/// The analysis is intentionally shape-only. It does not consult the type
/// table, infer ownership, or reject any source program.
#[must_use]
pub fn analyze_access_facts(hir: &HirModule) -> AccessFactTable {
    let mut facts = AccessFactTable::default();
    let mut collector = AccessFactCollector { facts: &mut facts };
    collector.visit_program(hir);
    facts
}

struct AccessFactCollector<'a> {
    facts: &'a mut AccessFactTable,
}

impl HirVisitor for AccessFactCollector<'_> {
    fn visit_local(&mut self, local: &HirLocal) {
        self.facts.record(local);
        walk_local(self, local);
    }
}

fn classify_initializer(expr: &HirExpression) -> AccessFact {
    match &expr.kind {
        HirExpressionKind::Array(elements)
            if elements
                .iter()
                .all(|element| matches!(element, HirArrayElement::Expr(_))) =>
        {
            AccessFact::Fresh
        }
        HirExpressionKind::Struct(_, _) | HirExpressionKind::Tuple(_, _) => AccessFact::Fresh,
        HirExpressionKind::Verte {
            entries: Some(_), ..
        } => AccessFact::Fresh,
        _ => AccessFact::Unknown,
    }
}
