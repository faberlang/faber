//! Index proofs: the first consumer of the interval facts (need, slice 4).
//!
//! When the checker proves every index component of an access `0 <= i < bound`
//! against its own container, it records the access once, by its `Index`
//! expression id, in `ShapeObligationTable::proven_indexes`. MIR lowering carries
//! a recorded access as a proven projection; the host runner and the shared
//! device index analysis (`radix_mir::abi::lane_index`) omit their bounds check
//! for it. No consumer derives a proof of its own.
//!
//! WHAT IS PROVEN
//! ==============
//! - A list or text element read or write, `xs[i]`: `0 <= i` and `i <` the live
//!   length of the named list or text binding at this point.
//! - A tensor element access, `t[i]` (rank 1) or `t[[i, j, ...]]`: every
//!   component `k` satisfies `0 <= c_k < extent(t, k)`, each against its OWN
//!   axis. A single unproven component leaves the whole access checked: a
//!   flat or partial proof is never inferred for the rest.
//!
//! Anything else (a map key, a window `xs[lo ‥ hi]`, a vector lane, a matrix
//! cell, a range slot, an index the interval facts do not bound) records
//! nothing and keeps its check.
//!
//! WHERE THE PROOF IS TAKEN
//! ========================
//! The query reads the flow facts as they stand when the access has just been
//! checked, after the operands' own calls killed what they could kill. A read
//! is evaluated where the checker meets it, in evaluation order. A store
//! target is evaluated at the store, after its right-hand side, so the target
//! is proved by [`TypeChecker::record_store_index_proof`] once that right-hand
//! side has been checked, and an index read inside a target proves nothing (it
//! is evaluated at the store too, not where the target is checked).

use radix_hir::{HirArrayElement, HirExpression, HirExpressionKind, HirId};

use super::flow_range::{BoundKey, IntRange};
use super::{IndexId, Type, TypeChecker};

impl TypeChecker<'_> {
    /// Record the proof for an index read, when every component is proven.
    pub(super) fn record_index_proof(
        &mut self,
        access: HirId,
        object: &HirExpression,
        index: &HirExpression,
    ) {
        if self.index_proof_suppressed == 0 && self.index_access_proven(object, index) {
            self.shape_obligations.proven_indexes.insert(access);
        }
    }

    /// Record the proof for an assignment target, when it is an index access.
    /// Called after the right-hand side was checked.
    pub(super) fn record_store_index_proof(&mut self, target: &HirExpression) {
        if let HirExpressionKind::Index(object, index) = &target.kind
            && self.index_access_proven(object, index)
        {
            self.shape_obligations.proven_indexes.insert(target.id);
        }
    }

    /// Run `check` with proof recording off: an index read inside an
    /// assignment target is evaluated at the store.
    pub(super) fn without_index_proofs<R>(&mut self, check: impl FnOnce(&mut Self) -> R) -> R {
        self.index_proof_suppressed += 1;
        let result = check(self);
        self.index_proof_suppressed -= 1;
        result
    }

    /// `0 <= index < bound` for one component, against the bound it must meet.
    fn component_proven(&self, component: &HirExpression, bound: &BoundKey) -> bool {
        if !self.is_integer_expr(component) {
            return false;
        }
        let range: IntRange = self.range_of_expr(component);
        range.proves_at_least(&BoundKey::Const(0), false) && range.proves_at_most(bound, true)
    }

    fn index_access_proven(&mut self, object: &HirExpression, index: &HirExpression) -> bool {
        let Some(object_ty) = self.value_type(object) else {
            return false;
        };
        let mut ty = self.resolve_inferred_type(object_ty);
        ty = self.types.peel_access_marker(ty).1;
        while let Type::Ref(_, inner) = self.types.get(ty).clone() {
            ty = self.resolve_inferred_type(inner);
            ty = self.types.peel_access_marker(ty).1;
        }
        match self.types.get(ty).clone() {
            Type::Tensor(_, shape) => self.tensor_access_proven(shape, index),
            _ if self.has_live_length(ty) && !self.is_range_valued(index) => {
                let Some(source) = self.length_source(object) else {
                    return false;
                };
                let bound = self.range_flow.live_length(source);
                super::integer_range::const_int_value(index).is_none()
                    && self.component_proven(index, &bound)
            }
            _ => false,
        }
    }

    /// Whether the index is a window or other non-scalar value.
    fn is_range_valued(&self, index: &HirExpression) -> bool {
        matches!(index.kind, HirExpressionKind::Intervallum { .. }) || !self.is_integer_expr(index)
    }

    fn tensor_access_proven(&mut self, shape: IndexId, index: &HirExpression) -> bool {
        let Some(axes) = self.axis_bounds(shape) else {
            return false;
        };
        let components: Vec<&HirExpression> = match &index.kind {
            HirExpressionKind::Array(elements) => {
                let mut components = Vec::with_capacity(elements.len());
                for element in elements {
                    let HirArrayElement::Expr(component) = element else {
                        return false;
                    };
                    components.push(component);
                }
                components
            }
            _ => vec![index],
        };
        if components.is_empty() || components.len() != axes.len() {
            return false;
        }
        // An all-literal access is the constant-index case the checker's
        // static range proof and the device literal-coordinate analysis
        // already own; it records nothing here.
        if components
            .iter()
            .all(|component| super::integer_range::const_int_value(component).is_some())
        {
            return false;
        }
        components.iter().zip(&axes).all(|(component, axis)| {
            !matches!(component.kind, HirExpressionKind::Intervallum { .. })
                && axis
                    .as_ref()
                    .is_some_and(|bound| self.component_proven(component, bound))
        })
    }
}
