//! Comparisons, logic and masks over aggregates (advisor rulings 2026-10-04).
//!
//! One rule for an author: `≡`/`≢` compare WHOLE values on every type, and the
//! numeric comparison glyphs `< > ≤ ≥ ≅ ≇` lift PER ELEMENT on numeric
//! aggregates like `+`.
//!
//! - `a ≡ b` over two tensors is one `bivalens`: the same type, the same shape
//!   and every element pair `≡`. A shape that is part of the type and differs is
//!   a check-time error naming both types; a shape known only at run time makes
//!   the comparison `falsum`, never a run-time error. A bool-or-numeric scalar
//!   against an aggregate is rejected and the diagnostic points at `≅`.
//! - A register `vector` or `matrix` takes `< > ≤ ≥ ≅ ≇` against a scalar (either
//!   side) or a same-shape peer and gives a bool register of the same shape, the
//!   same lifting rule as a tensor (rule 15.1). `et`/`aut`/`non` and the `✓ ✗`
//!   select lift over a bool register the same way, both sides evaluated.
//! - A bool aggregate is never a truth value: `if mask` names `reduce via all`
//!   and `reduce via any`.

use super::tensor_lift::{LiftSide, RegisterKind, RegisterOperand};
use super::{
    BOOLEAN_OPERAND_REQUIRED, BOOLEAN_OPERANDS_REQUIRED, HirBinOp, HirExpression,
    NUMERIC_OPERANDS_REQUIRED, Type, TypeChecker, TypeId, integer_range, is_numeric_literal,
};
use crate::{IndexConstraint, IndexId};
use radix_types::IndexExpr;

/// `a ≡ 2.0`: a whole-value comparison never takes a scalar.
pub(in crate::passes::typecheck) const ISSUE_AGGREGATE_EQUALITY_SCALAR_OPERAND: &str =
    "aggregate_equality_scalar_operand";
/// `a ≡ b` over aggregates whose shapes (part of the type) differ.
pub(in crate::passes::typecheck) const ISSUE_AGGREGATE_EQUALITY_SHAPE_MISMATCH: &str =
    "aggregate_equality_shape_mismatch";
/// `if mask { … }`: a bool aggregate is not a truth value.
pub(in crate::passes::typecheck) const ISSUE_AGGREGATE_MASK_NOT_TRUTH_VALUE: &str =
    "aggregate_mask_not_truth_value";

/// Which aggregate a comparison operand is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AggregateKind {
    Tensor,
    Vector,
    Matrix,
    /// Compares as its mathematical tensor (a differing shape is `falsum`) and
    /// has no lifted operators yet, so only the scalar and `≈` rejections apply.
    Sparsa,
    /// A `lista`, bounded or not: it compares as itself (`≡` is the scalar
    /// rule), so only the scalar-operand, mask and `≈` rejections apply.
    List,
}

impl AggregateKind {
    fn name(self) -> &'static str {
        match self {
            Self::Tensor => "tensor",
            Self::Vector => "vector",
            Self::Matrix => "matrix",
            Self::Sparsa => "sparsa",
            Self::List => "list",
        }
    }
}

/// A tensor, vector or matrix operand: its kind, element and shape (the width
/// of a vector).
struct AggregateOperand {
    kind: AggregateKind,
    elem: TypeId,
    extent: IndexId,
}

impl TypeChecker<'_> {
    fn aggregate_operand(&mut self, ty: TypeId) -> Option<AggregateOperand> {
        if let LiftSide::Tensor { elem, shape } = self.lift_side(ty) {
            return Some(AggregateOperand {
                kind: AggregateKind::Tensor,
                elem,
                extent: shape,
            });
        }
        if let Some((elem, extent)) = self.sparsa_operand(ty) {
            return Some(AggregateOperand {
                kind: AggregateKind::Sparsa,
                elem,
                extent,
            });
        }
        let Some(register) = self.register_operand(ty) else {
            return self.list_operand(ty);
        };
        Some(AggregateOperand {
            kind: match register.kind {
                RegisterKind::Vector => AggregateKind::Vector,
                RegisterKind::Matrix => AggregateKind::Matrix,
            },
            elem: register.elem,
            extent: register.extent,
        })
    }

    /// A `lista<T>` / `lista<T, magnitudo N>` operand: its element and capacity (an
    /// unbounded list has the unspecified shape).
    fn list_operand(&mut self, ty: TypeId) -> Option<AggregateOperand> {
        let elem = self.lista_element_type(ty)?;
        let extent = self.lista_capacity(ty);
        let extent = match extent {
            Some(capacity) => capacity,
            None => self.types.unspecified_shape(),
        };
        Some(AggregateOperand {
            kind: AggregateKind::List,
            elem,
            extent,
        })
    }

    /// The name of the aggregate kind (`tensor`, `vector`, `matrix`, `list`)
    /// of `ty`; `None` for a scalar or a `sparsa`.
    pub(super) fn glyph_aggregate_kind(&mut self, ty: TypeId) -> Option<&'static str> {
        self.aggregate_operand(ty)
            .filter(|aggregate| aggregate.kind != AggregateKind::Sparsa)
            .map(|aggregate| aggregate.kind.name())
    }

    /// The kind name for a diagnostic that names the aggregate of an operand
    /// pair (the first operand that is one).
    pub(super) fn aggregate_kind_of_pair(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> &'static str {
        [lhs_ty, rhs_ty]
            .into_iter()
            .find_map(|ty| self.aggregate_operand(ty))
            .map_or("tensor", |aggregate| aggregate.kind.name())
    }

    /// Whether `ty` is a `lista` of bool: a mask that is not a truth value.
    pub(in crate::passes::typecheck) fn is_bool_list_mask(&mut self, ty: TypeId) -> bool {
        self.list_operand(ty)
            .is_some_and(|aggregate| self.is_bool(aggregate.elem))
    }

    fn sparsa_operand(&mut self, ty: TypeId) -> Option<(TypeId, IndexId)> {
        let resolved = self.value_carrier_type(ty);
        let (_, payload) = self.types.peel_access_marker(resolved);
        match self.types.get(payload).clone() {
            Type::Sparsa(elem, shape) => Some((self.resolve_inferred_type(elem), shape)),
            _ => None,
        }
    }

    /// Whether `ty` is a `sparsa`: `≈ ≉` over one waits for L7 like a tensor.
    pub(super) fn is_sparsa_operand(&mut self, ty: TypeId) -> bool {
        self.sparsa_operand(ty).is_some()
    }

    /// `≡ ≢` where an operand is a tensor, vector, matrix or sparsa. `None` means the
    /// pair keeps the scalar equality rule: no aggregate, an aggregate of
    /// another kind, a vector or matrix pair that agrees, or a `nihil` test.
    pub(super) fn check_aggregate_equality(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let span = lhs.span;
        let (left, right) = (
            self.aggregate_operand(lhs_ty),
            self.aggregate_operand(rhs_ty),
        );
        match (left, right) {
            (None, None) => None,
            (Some(left), Some(right)) => {
                if left.kind != right.kind {
                    return None;
                }
                self.check_aggregate_equality_pair(&left, &right, (lhs_ty, rhs_ty), span)
            }
            (Some(aggregate), None) => self.reject_scalar_equality(&aggregate, rhs, rhs_ty, span),
            (None, Some(aggregate)) => self.reject_scalar_equality(&aggregate, lhs, lhs_ty, span),
        }
    }

    /// Two aggregates of one kind. A tensor pair is checked here (shape, then
    /// exact element types) and gives `bivalens`; a vector or matrix pair keeps
    /// the exact-type rule and is only intercepted to name a differing shape.
    fn check_aggregate_equality_pair(
        &mut self,
        left: &AggregateOperand,
        right: &AggregateOperand,
        (lhs_ty, rhs_ty): (TypeId, TypeId),
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if self.is_error(left.elem) || self.is_error(right.elem) {
            return Some(self.error_type);
        }
        // A shape that is part of the type and differs is a type question; a
        // shape known only at run time is a value question (`falsum`). A
        // sparsa compares as its mathematical tensor, so its shapes never
        // conflict statically.
        if !matches!(left.kind, AggregateKind::Sparsa | AggregateKind::List)
            && self.static_shape_conflict(left.extent, right.extent)
        {
            let args = [
                ("kind", left.kind.name().to_owned()),
                ("left", self.format_type_for_diagnostic(lhs_ty)),
                ("right", self.format_type_for_diagnostic(rhs_ty)),
            ];
            self.invalid_operand_types_with_args(
                ISSUE_AGGREGATE_EQUALITY_SHAPE_MISMATCH,
                span,
                &args,
            );
            return Some(self.bool_type());
        }
        // Only a tensor pair continues here; a vector or matrix pair keeps the
        // exact-type rule, and a sparsa or list pair compares as itself.
        if left.kind != AggregateKind::Tensor {
            return None;
        }
        // Bind inference holes the two shapes agree on; a pair that cannot
        // unify (distinct run-time extents) is compared at run time.
        self.solve_index_constraints(&[IndexConstraint::Equal(left.extent, right.extent)]);
        // A comparison is a `bivalens` even when its elements are rejected, so
        // the error does not cascade into the surrounding expression.
        self.exact_equality_types(left.elem, right.elem, span);
        Some(self.bool_type())
    }

    /// Whether two shapes differ in the type itself: a different rank, or two
    /// literal extents that differ on one axis. Run-time extents, size
    /// parameters and inference holes never conflict statically.
    fn static_shape_conflict(&mut self, left: IndexId, right: IndexId) -> bool {
        let (Some(left), Some(right)) = (self.shape_dims(left), self.shape_dims(right)) else {
            return false;
        };
        if left.len() != right.len() {
            return true;
        }
        left.iter()
            .zip(&right)
            .any(|(l, r)| matches!((l, r), (Some(l), Some(r)) if l != r))
    }

    /// The axes of a shape, each `Some(extent)` when a literal. `None` when
    /// the rank itself is not known yet (a hole standing for a whole shape).
    fn shape_dims(&mut self, shape: IndexId) -> Option<Vec<Option<u64>>> {
        let shape = self.resolve_index(shape);
        let resolved_axes = match self.types.get_index(shape).clone() {
            IndexExpr::Tuple(resolved_axes) => resolved_axes,
            IndexExpr::Infer(_) | IndexExpr::Unspecified | IndexExpr::Param(_) => return None,
            _ => vec![shape],
        };
        let mut dims = Vec::with_capacity(resolved_axes.len());
        for axis in resolved_axes {
            let axis = self.resolve_index(axis);
            dims.push(match self.types.get_index(axis) {
                IndexExpr::Literal(value) => Some(*value),
                _ => None,
            });
        }
        Some(dims)
    }

    /// An aggregate against a numeric or bool scalar under `≡ ≢`: never the
    /// same value; the help points at `≅`. Any other peer (`nihil`, an
    /// optional) keeps its own rule.
    fn reject_scalar_equality(
        &mut self,
        aggregate: &AggregateOperand,
        scalar: &HirExpression,
        scalar_ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let resolved = self.resolve_inferred_type(scalar_ty);
        if resolved == self.error_type {
            return None;
        }
        let literal = is_numeric_literal(scalar) || integer_range::is_const_int(scalar);
        let scalar_like =
            literal || self.numeric_shape(resolved).is_some() || self.is_bool(resolved);
        if !scalar_like {
            return None;
        }
        self.invalid_operand_types_with_args(
            ISSUE_AGGREGATE_EQUALITY_SCALAR_OPERAND,
            span,
            &[("kind", aggregate.kind.name().to_owned())],
        );
        Some(self.bool_type())
    }

    /// The register operand of an operator and the other side, when exactly
    /// one side is a register and the other a scalar, or both are registers of
    /// one kind. `None` leaves the pair to its own rule.
    fn register_lift_pairing(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<(RegisterOperand, Option<RegisterOperand>, TypeId)> {
        let (left, right) = (self.register_operand(lhs_ty), self.register_operand(rhs_ty));
        match (left, right) {
            (Some(left), Some(right)) if left.kind == right.kind => {
                Some((left, Some(right), lhs_ty))
            }
            (Some(register), None) if matches!(self.lift_side(rhs_ty), LiftSide::Scalar(_)) => {
                Some((register, None, rhs_ty))
            }
            (None, Some(register)) if matches!(self.lift_side(lhs_ty), LiftSide::Scalar(_)) => {
                Some((register, None, lhs_ty))
            }
            _ => None,
        }
    }

    /// `< > ≤ ≥ ≅ ≇` over a register: lifted per element against a scalar
    /// (either side) or a same-shape register, giving a bool register of the
    /// same kind and shape. The element rule is the scalar rule for the glyph.
    pub(super) fn check_register_lift_compare(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let span = lhs.span;
        let (register, peer, other_ty) = self.register_lift_pairing(lhs_ty, rhs_ty)?;
        let register_first = self.register_operand(lhs_ty).is_some();
        if !self.register_element_admitted(&register, span) {
            return Some(self.error_type);
        }
        let other_elem = if let Some(peer) = &peer {
            if !self.register_element_admitted(peer, span) {
                return Some(self.error_type);
            }
            let before = self.errors.len();
            self.constrain_index_equal(register.extent, peer.extent, span);
            if self.errors.len() != before {
                return Some(self.error_type);
            }
            peer.elem
        } else {
            if self.is_error(other_ty) {
                return Some(self.error_type);
            }
            let resolved = self.resolve_inferred_type(other_ty);
            if !self.is_infer(resolved) && self.numeric_shape(resolved).is_none() {
                self.invalid_operand_types(NUMERIC_OPERANDS_REQUIRED, span);
                return Some(self.error_type);
            }
            other_ty
        };
        let (l_elem, r_elem) = if register_first {
            (register.elem, other_elem)
        } else {
            (other_elem, register.elem)
        };
        let before = self.errors.len();
        match op {
            HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq => {
                self.numeric_ordering_bin(l_elem, r_elem, span);
            }
            HirBinOp::ApproxEq | HirBinOp::ApproxNotEq => {
                self.numeric_value_equality_types(l_elem, r_elem, span);
            }
            _ => return None,
        }
        if self.errors.len() != before {
            return Some(self.error_type);
        }
        let bivalens = self.bool_type();
        Some(self.register_result(&register, bivalens))
    }

    /// `non` over a bool register.
    pub(super) fn check_register_lift_not(
        &mut self,
        operand: &HirExpression,
        operand_ty: TypeId,
    ) -> Option<TypeId> {
        let register = self.register_operand(operand_ty)?;
        if !self.is_bool(register.elem) {
            self.invalid_operand_types(BOOLEAN_OPERAND_REQUIRED, operand.span);
            return Some(self.error_type);
        }
        let bivalens = self.bool_type();
        Some(self.register_result(&register, bivalens))
    }

    /// `et` and `aut` over bool registers and scalars; both operands are
    /// always evaluated.
    pub(super) fn check_register_lift_logic(
        &mut self,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let span = lhs.span;
        let (register, peer, other_ty) = self.register_lift_pairing(lhs_ty, rhs_ty)?;
        let bivalens = self.bool_type();
        if !self.is_bool(register.elem) {
            self.invalid_operand_types(BOOLEAN_OPERANDS_REQUIRED, span);
            return Some(self.error_type);
        }
        if let Some(peer) = &peer {
            if !self.is_bool(peer.elem) {
                self.invalid_operand_types(BOOLEAN_OPERANDS_REQUIRED, span);
                return Some(self.error_type);
            }
            let before = self.errors.len();
            self.constrain_index_equal(register.extent, peer.extent, span);
            if self.errors.len() != before {
                return Some(self.error_type);
            }
        } else {
            let resolved = self.resolve_inferred_type(other_ty);
            if self.is_infer(resolved) {
                self.unify_assign_issue(other_ty, bivalens, span, BOOLEAN_OPERANDS_REQUIRED);
            } else if !self.is_bool(other_ty) {
                self.invalid_operand_types(BOOLEAN_OPERANDS_REQUIRED, span);
                return Some(self.error_type);
            }
        }
        Some(self.register_result(&register, bivalens))
    }

    /// Whether a condition type is an aggregate that may be a select mask.
    pub(in crate::passes::typecheck) fn is_aggregate_mask_type(&mut self, ty: TypeId) -> bool {
        self.aggregate_operand(ty).is_some_and(|aggregate| {
            !matches!(aggregate.kind, AggregateKind::Sparsa | AggregateKind::List)
        })
    }

    /// Report a bool aggregate used where a truth value is required, naming the
    /// reductions; any other type is the plain `condition_must_be_bivalens`.
    pub(in crate::passes::typecheck) fn report_mask_not_truth_value(
        &mut self,
        ty: TypeId,
        span: radix_lexer::Span,
    ) {
        let Some(aggregate) = self.aggregate_operand(ty) else {
            self.invalid_operand_types("condition_must_be_bivalens", span);
            return;
        };
        if !self.is_bool(aggregate.elem) {
            self.invalid_operand_types("condition_must_be_bivalens", span);
            return;
        }
        self.invalid_operand_types_with_args(
            ISSUE_AGGREGATE_MASK_NOT_TRUTH_VALUE,
            span,
            &[("kind", aggregate.kind.name().to_owned())],
        );
    }
}
