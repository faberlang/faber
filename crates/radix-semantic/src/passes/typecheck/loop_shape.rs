//! Loop shape rule (need, ruling O1).
//!
//! Inside a loop whose iterated collection is the local or parameter `X`, a
//! **method** call on that same `X` that changes its shape or order is a
//! compile error. Methods are the checked surface; a bracket store
//! (`xs[i] ← v`, `m[k] ← v`) is the "developer knows best" surface and stays
//! accepted. Only the visible case is checked: the same name, no aliases,
//! object fields or `mut` arguments (deferred by need).
//!
//! [`SHAPE_CHANGING_METHODS`] is the one place the rule's method rows live.
//! The ledger rows are added there when the ledger type exists.

use super::TypeChecker;
use crate::SemanticErrorKind;
use radix_hir::{DefId, HirBlock, HirExpression, HirExpressionKind};
use radix_intrinsics::{IntrinsicReceiver, MethodIntrinsic};
use radix_lexer::Span;

pub(super) const ISSUE_LOOP_COLLECTION_SHAPE_CHANGE: &str = "loop_collection_shape_change";

/// Registry rows (receiver, canonical method name) that change the shape or
/// order of their receiver. Append the ledger rows here.
const SHAPE_CHANGING_METHODS: &[(IntrinsicReceiver, &str)] = &[
    // lista: append (appende and its sink twin immerge), extend, remove first/last,
    // in-place reverse, in-place sort.
    (IntrinsicReceiver::Lista, "appende"),
    (IntrinsicReceiver::Lista, "immerge"),
    (IntrinsicReceiver::Lista, "extende"),
    (IntrinsicReceiver::Lista, "decapita"),
    (IntrinsicReceiver::Lista, "detrahe"),
    (IntrinsicReceiver::Lista, "inverte"),
    (IntrinsicReceiver::Lista, "ordina"),
    // copia: add (adde and its sink twin immerge), remove.
    (IntrinsicReceiver::Copia, "adde"),
    (IntrinsicReceiver::Copia, "immerge"),
    (IntrinsicReceiver::Copia, "dele"),
    // tabula: put (even when it would only update), delete.
    (IntrinsicReceiver::Tabula, "pone"),
    (IntrinsicReceiver::Tabula, "dele"),
    // queue: append, remove first.
    (IntrinsicReceiver::Queue, "appende"),
    (IntrinsicReceiver::Queue, "decapita"),
    // stack: append, remove last.
    (IntrinsicReceiver::Stack, "appende"),
    (IntrinsicReceiver::Stack, "detrahe"),
];

fn changes_shape(entry: &MethodIntrinsic) -> bool {
    SHAPE_CHANGING_METHODS
        .iter()
        .any(|(receiver, name)| *receiver == entry.receiver && *name == entry.faber_name)
}

/// The local or parameter a loop iterates, when the source is that bare name.
fn iterated_def(source: &HirExpression) -> Option<DefId> {
    match &TypeChecker::method_receiver_place(source).kind {
        HirExpressionKind::Path(def_id) => Some(*def_id),
        _ => None,
    }
}

impl TypeChecker<'_> {
    /// [`Self::check_loop_block`] with the iterated sources of this loop
    /// recorded for the shape rule while its body is checked.
    pub(super) fn check_loop_block_over<'s>(
        &mut self,
        sources: impl IntoIterator<Item = &'s HirExpression>,
        block: &mut HirBlock,
    ) {
        let depth = self.loop_iterated.len();
        self.loop_iterated
            .extend(sources.into_iter().filter_map(iterated_def));
        self.check_loop_block(block);
        self.loop_iterated.truncate(depth);
    }

    /// Reject `place.method(..)` when `place` is a collection an enclosing
    /// loop iterates and the method changes its shape or order.
    pub(super) fn check_loop_shape_method(
        &mut self,
        place: &HirExpression,
        entry: &MethodIntrinsic,
        method: &str,
        span: Span,
    ) {
        let HirExpressionKind::Path(def_id) = &place.kind else {
            return;
        };
        if !self.loop_iterated.contains(def_id) || !changes_shape(entry) {
            return;
        }
        let binding = self
            .binding_names
            .get(def_id)
            .or_else(|| self.resolver.get_symbol(*def_id).map(|symbol| &symbol.name))
            .and_then(|symbol| self.interner.map(|interner| interner.resolve(*symbol)))
            .unwrap_or("value")
            .to_owned();
        self.error_issue_with_args(
            SemanticErrorKind::OperandTypesInvalid,
            ISSUE_LOOP_COLLECTION_SHAPE_CHANGE,
            span,
            &[("binding", binding), ("method", method.to_owned())],
        );
    }
}
