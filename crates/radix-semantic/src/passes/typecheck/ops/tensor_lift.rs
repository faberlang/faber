//! Tensor elementwise lifting of scalar operators (gpu-reset rule 15, K4).
//!
//! A lifted operator is the scalar operator applied per element, so tensors get
//! no second arithmetic model:
//!
//! - `+ - * / % ÷`, unary `-`, `⤒ ⤓`, comparisons, `et`/`aut`/`non`, `vel` and
//!   the `✓ ✗` select lift over a tensor against a scalar or a same-shape
//!   tensor. `* / %` lift only against a scalar (`tensor * tensor` keeps its
//!   K10 status quo: not handled here).
//! - The element type is the F9 join of the operand element types: identical
//!   types stay, integers join by the smallest type holding both ranges (an
//!   untyped constant takes part by value), floats join by the lossless
//!   lattice, and a family crossing needs `↦`. Each result element is stored
//!   under that element type at the operation (ruling 19(g)).
//! - `÷` takes its width from ruling 23 per element. Comparisons produce
//!   `tensor<bivalens, S>`.
//! - No implicit broadcasting is added: two tensors must have the same shape.
//!
//! The runner is the oracle for these forms (`MirLiftOp`); every other target
//! fails closed.

use super::*;
use crate::IndexId;
use crate::passes::typecheck::integer_range::const_int_value;
use radix_hir::HirCallArg;

/// `≈`/`≉` on tensors wait for the numeric contract's tolerance ruling (L7).
pub(in crate::passes::typecheck) const ISSUE_TENSOR_APPROX_COMPARISON_DEFERRED: &str =
    "tensor_approx_comparison_deferred";
/// `vel` lifts only over `T ∪ nihil` elements.
pub(in crate::passes::typecheck) const ISSUE_TENSOR_COALESCE_ELEMENT_NULLABLE_REQUIRED: &str =
    "tensor_coalesce_element_nullable_required";
/// Decimal elements have no lifted form yet.
pub(in crate::passes::typecheck) const ISSUE_TENSOR_LIFT_DECIMAL_ELEMENT: &str =
    "tensor_lift_decimal_element_unsupported";
const ISSUE_CONDITION_MUST_BE_BIVALENS: &str = "condition_must_be_bivalens";

/// Spelling of the approximate-equality glyph in a deferral diagnostic.
fn approx_glyph(op: HirBinOp) -> &'static str {
    if op == HirBinOp::FuzzyNotEq {
        "≉"
    } else {
        "≈"
    }
}

/// How one operand takes part in a lifted operator.
enum LiftSide {
    Tensor {
        elem: TypeId,
        shape: IndexId,
    },
    /// A vector or matrix: keeps its own rules, never lifted here.
    Register,
    Scalar(TypeId),
}

/// The operands of a lifted operator with their shared shape.
struct LiftPair {
    l_elem: TypeId,
    r_elem: TypeId,
    l_tensor: bool,
    r_tensor: bool,
    shape: IndexId,
}

enum LiftPairing {
    NotLifted,
    Failed,
    Lifted(LiftPair),
}

impl TypeChecker<'_> {
    fn lift_side(&mut self, ty: TypeId) -> LiftSide {
        let resolved = self.value_carrier_type(ty);
        let (_, payload) = self.types.peel_access_marker(resolved);
        match self.types.get(payload).clone() {
            Type::Tensor(elem, shape) => LiftSide::Tensor {
                elem: self.resolve_inferred_type(elem),
                shape,
            },
            Type::Vector(..) | Type::Matrix(..) => LiftSide::Register,
            _ => LiftSide::Scalar(ty),
        }
    }

    /// The type a literal beside a tensor takes as its peer: the tensor's
    /// numeric element, so `x * 0.5` on an `f32` tensor stays `f32`.
    pub(in crate::passes::typecheck) fn scalar_peer_type(&mut self, ty: TypeId) -> TypeId {
        match self.lift_side(ty) {
            LiftSide::Tensor { elem, .. }
                if crate::numeric_width::is_tensor_numeric_element(self.types, elem) =>
            {
                elem
            }
            _ => ty,
        }
    }

    fn lift_result(&mut self, elem: TypeId, shape: IndexId) -> TypeId {
        self.types.tensor_with_shape(elem, shape)
    }

    fn lift_bool_result(&mut self, shape: IndexId) -> TypeId {
        let bivalens = self.bool_type();
        self.lift_result(bivalens, shape)
    }

    /// Pair the operands of a lifted operator. `allow_tensor_pair` admits two
    /// tensors (same shape required); a vector or matrix operand is not lifted.
    fn lift_pairing(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
        allow_tensor_pair: bool,
    ) -> LiftPairing {
        let (left, right) = (self.lift_side(lhs_ty), self.lift_side(rhs_ty));
        let scalar_is_error = |this: &Self, ty: TypeId| this.is_error(ty);
        match (left, right) {
            (
                LiftSide::Tensor {
                    elem: l_elem,
                    shape: l_shape,
                },
                LiftSide::Tensor {
                    elem: r_elem,
                    shape: r_shape,
                },
            ) if allow_tensor_pair => {
                let before = self.errors.len();
                self.constrain_index_equal(l_shape, r_shape, span);
                if self.errors.len() != before {
                    return LiftPairing::Failed;
                }
                LiftPairing::Lifted(LiftPair {
                    l_elem,
                    r_elem,
                    l_tensor: true,
                    r_tensor: true,
                    shape: l_shape,
                })
            }
            (LiftSide::Tensor { elem, shape }, LiftSide::Scalar(scalar)) => {
                if scalar_is_error(self, scalar) {
                    return LiftPairing::Failed;
                }
                LiftPairing::Lifted(LiftPair {
                    l_elem: elem,
                    r_elem: scalar,
                    l_tensor: true,
                    r_tensor: false,
                    shape,
                })
            }
            (LiftSide::Scalar(scalar), LiftSide::Tensor { elem, shape }) => {
                if scalar_is_error(self, scalar) {
                    return LiftPairing::Failed;
                }
                LiftPairing::Lifted(LiftPair {
                    l_elem: scalar,
                    r_elem: elem,
                    l_tensor: false,
                    r_tensor: true,
                    shape,
                })
            }
            _ => LiftPairing::NotLifted,
        }
    }

    /// A tensor element must be a numeric, non-decimal scalar.
    fn lift_element_admitted(&mut self, elem: TypeId, span: radix_lexer::Span) -> bool {
        if self.is_error(elem) {
            return false;
        }
        if !crate::numeric_width::is_tensor_numeric_element(self.types, elem) {
            self.invalid_operand_types(TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED, span);
            return false;
        }
        if self
            .numeric_shape(elem)
            .is_some_and(|shape| shape.width.is_decimal())
        {
            self.invalid_operand_types(ISSUE_TENSOR_LIFT_DECIMAL_ELEMENT, span);
            return false;
        }
        true
    }

    fn lift_pair_elements_admitted(&mut self, pair: &LiftPair, span: radix_lexer::Span) -> bool {
        (!pair.l_tensor || self.lift_element_admitted(pair.l_elem, span))
            && (!pair.r_tensor || self.lift_element_admitted(pair.r_elem, span))
    }

    /// The scalar operand of a lifted numeric operator must be numeric (an
    /// untyped literal still being inferred counts).
    fn lift_scalar_numeric(
        &mut self,
        pair: &LiftPair,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> bool {
        let scalar = if pair.l_tensor {
            pair.r_elem
        } else {
            pair.l_elem
        };
        if pair.l_tensor && pair.r_tensor {
            return true;
        }
        let resolved = self.resolve_inferred_type(scalar);
        if self.is_infer(resolved) || self.numeric_shape(resolved).is_some() {
            return true;
        }
        self.invalid_operand_types(issue, span);
        false
    }

    /// Element type of a lifted `+ - * / % ÷`: the F9 join of the operand
    /// elements, `÷` by ruling 23.
    fn lift_arith_element(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        l_elem: TypeId,
        r_elem: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        if op == HirBinOp::TrueDiv {
            return self.true_division_type(lhs, rhs, l_elem, r_elem, span);
        }
        self.lift_join_element(
            (l_elem, const_int_value(lhs)),
            (r_elem, const_int_value(rhs)),
            span,
        )
    }

    /// The element type two operands of an elementwise operator join to.
    pub(in crate::passes::typecheck) fn lift_join_element(
        &mut self,
        l: (TypeId, Option<i128>),
        r: (TypeId, Option<i128>),
        span: radix_lexer::Span,
    ) -> TypeId {
        if self.types.equals(l.0, r.0) && l.1.is_none() && r.1.is_none() {
            return l.0;
        }
        if let Some(joined) = self.integer_operand_join(l, r, span) {
            return joined;
        }
        self.numeric_arithmetic_bin(l.0, r.0, span)
    }

    /// `+ - * / % ÷` between a tensor and a scalar (either side), and `÷` between
    /// two tensors. `None` means the pair is not lifted here: the caller keeps
    /// its own rule (`+ -` between tensors, `* / %` between tensors, scalars).
    pub(in crate::passes::typecheck) fn check_tensor_lift_arith(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let span = lhs.span;
        let pair = match self.lift_pairing(lhs_ty, rhs_ty, span, op == HirBinOp::TrueDiv) {
            LiftPairing::NotLifted => return None,
            LiftPairing::Failed => return Some(self.error_type),
            LiftPairing::Lifted(pair) => pair,
        };
        if !self.lift_pair_elements_admitted(&pair, span)
            || !self.lift_scalar_numeric(&pair, span, NUMERIC_OPERANDS_REQUIRED)
        {
            return Some(self.error_type);
        }
        let elem = self.lift_arith_element(op, lhs, rhs, pair.l_elem, pair.r_elem, span);
        if self.is_error(elem) {
            return Some(self.error_type);
        }
        Some(self.lift_result(elem, pair.shape))
    }

    /// Unary `-` over a tensor keeps the element type (each element is stored
    /// under it, so negating the minimum of a signed width traps).
    pub(in crate::passes::typecheck) fn check_tensor_lift_neg(
        &mut self,
        operand: &HirExpression,
        operand_ty: TypeId,
    ) -> Option<TypeId> {
        let LiftSide::Tensor { elem, shape } = self.lift_side(operand_ty) else {
            return None;
        };
        if !self.lift_element_admitted(elem, operand.span) {
            return Some(self.error_type);
        }
        Some(self.lift_result(elem, shape))
    }

    /// `non` over a `bivalens` tensor.
    pub(in crate::passes::typecheck) fn check_tensor_lift_not(
        &mut self,
        operand: &HirExpression,
        operand_ty: TypeId,
    ) -> Option<TypeId> {
        let LiftSide::Tensor { elem, shape } = self.lift_side(operand_ty) else {
            return None;
        };
        if !self.is_bool(elem) {
            self.invalid_operand_types(BOOLEAN_OPERAND_REQUIRED, operand.span);
            return Some(self.error_type);
        }
        Some(self.lift_bool_result(shape))
    }

    /// `et` and `aut` over `bivalens` tensors and scalars; both operands are
    /// always evaluated.
    pub(in crate::passes::typecheck) fn check_tensor_lift_logic(
        &mut self,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let span = lhs.span;
        let pair = match self.lift_pairing(lhs_ty, rhs_ty, span, true) {
            LiftPairing::NotLifted => return None,
            LiftPairing::Failed => return Some(self.error_type),
            LiftPairing::Lifted(pair) => pair,
        };
        let bivalens = self.bool_type();
        for elem in [pair.l_elem, pair.r_elem] {
            let resolved = self.resolve_inferred_type(elem);
            if self.is_infer(resolved) {
                self.unify_assign_issue(elem, bivalens, span, BOOLEAN_OPERANDS_REQUIRED);
            } else if !self.is_bool(elem) {
                self.invalid_operand_types(BOOLEAN_OPERANDS_REQUIRED, span);
                return Some(self.error_type);
            }
        }
        Some(self.lift_bool_result(pair.shape))
    }

    /// Comparisons over tensors produce `tensor<bivalens, S>`. The element rule
    /// is the scalar rule for the glyph: ordering and `≅ ≇` join by value,
    /// `≡ ≠` need identical elements. `≈ ≉` wait for the numeric contract.
    pub(in crate::passes::typecheck) fn check_tensor_lift_compare(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let span = lhs.span;
        let pair = match self.lift_pairing(lhs_ty, rhs_ty, span, true) {
            LiftPairing::NotLifted => return None,
            LiftPairing::Failed => return Some(self.error_type),
            LiftPairing::Lifted(pair) => pair,
        };
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq) {
            self.invalid_operand_types_with_args(
                ISSUE_TENSOR_APPROX_COMPARISON_DEFERRED,
                span,
                &[("op", approx_glyph(op).to_owned())],
            );
            return Some(self.error_type);
        }
        let exact = matches!(
            op,
            HirBinOp::Eq | HirBinOp::NotEq | HirBinOp::StrictEq | HirBinOp::StrictNotEq
        );
        let bool_equality = exact
            && [(pair.l_tensor, pair.l_elem), (pair.r_tensor, pair.r_elem)]
                .iter()
                .all(|(is_tensor, elem)| !is_tensor || self.is_bool(*elem));
        if !bool_equality && !self.lift_pair_elements_admitted(&pair, span) {
            return Some(self.error_type);
        }
        let before = self.errors.len();
        match op {
            HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq => {
                self.numeric_ordering_bin(pair.l_elem, pair.r_elem, span);
            }
            HirBinOp::ApproxEq | HirBinOp::ApproxNotEq => {
                self.numeric_value_equality_types(pair.l_elem, pair.r_elem, span);
            }
            _ => {
                if !self.equality_constant_exempt(lhs, pair.l_elem, rhs, pair.r_elem) {
                    self.exact_equality_types(pair.l_elem, pair.r_elem, span);
                }
            }
        }
        if self.errors.len() != before {
            return Some(self.error_type);
        }
        Some(self.lift_bool_result(pair.shape))
    }

    /// `a ⤒ b` / `a ⤓ b` with a tensor receiver: the elementwise extremum
    /// against a scalar of the element type or a same-shape tensor.
    pub(in crate::passes::typecheck) fn check_tensor_extremum(
        &mut self,
        receiver_ty: TypeId,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> TypeId {
        let LiftSide::Tensor { elem, shape } = self.lift_side(receiver_ty) else {
            return self.error_type;
        };
        let [arg] = args else {
            return self.error_type;
        };
        let hint = is_numeric_literal(&arg.expr).then_some(elem);
        let arg_ty = self.check_expr_with_expected(&mut arg.expr, hint);
        if self.is_error(arg_ty) || !self.lift_element_admitted(elem, span) {
            return self.error_type;
        }
        match self.lift_side(arg_ty) {
            LiftSide::Tensor {
                elem: other,
                shape: other_shape,
            } => {
                let before = self.errors.len();
                self.constrain_index_equal(shape, other_shape, span);
                if self.errors.len() != before || !self.lift_element_admitted(other, span) {
                    return self.error_type;
                }
                let joined = self.lift_join_element((elem, None), (other, None), span);
                if self.is_error(joined) {
                    return self.error_type;
                }
                self.lift_result(joined, shape)
            }
            LiftSide::Register => {
                self.invalid_operand_types(ARGUMENT_TYPE_MISMATCH, span);
                self.error_type
            }
            LiftSide::Scalar(_) => {
                self.unify_assign_issue(arg_ty, elem, arg.expr.span, ARGUMENT_TYPE_MISMATCH);
                receiver_ty
            }
        }
    }

    /// `vel` over a tensor with nullable elements: each `nihil` element takes
    /// the fallback element (a scalar or a same-shape tensor).
    pub(in crate::passes::typecheck) fn check_tensor_coalesce(
        &mut self,
        lhs_ty: TypeId,
        lhs_span: radix_lexer::Span,
        rhs: &mut HirExpression,
    ) -> Option<TypeId> {
        let LiftSide::Tensor { elem, shape } = self.lift_side(lhs_ty) else {
            return None;
        };
        let Type::Option(inner) = self.types.get(self.resolve_inferred_type(elem)).clone() else {
            self.check_expr(rhs);
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_TENSOR_COALESCE_ELEMENT_NULLABLE_REQUIRED,
                lhs_span,
            );
            return Some(self.error_type);
        };
        let rhs_ty = if is_numeric_literal(rhs) {
            self.check_expr_with_expected(rhs, Some(inner))
        } else {
            self.check_expr(rhs)
        };
        let fallback_elem = match self.lift_side(rhs_ty) {
            LiftSide::Tensor {
                elem: other,
                shape: other_shape,
            } => {
                let before = self.errors.len();
                self.constrain_index_equal(shape, other_shape, rhs.span);
                if self.errors.len() != before {
                    return Some(self.error_type);
                }
                other
            }
            LiftSide::Register => {
                self.invalid_operand_types(COALESCE_FALLBACK_TYPE_MISMATCH, rhs.span);
                return Some(self.error_type);
            }
            LiftSide::Scalar(scalar) => scalar,
        };
        let element = match self
            .types
            .get(self.resolve_inferred_type(fallback_elem))
            .clone()
        {
            Type::Option(rhs_inner) => {
                self.unify_assign_issue(
                    rhs_inner,
                    inner,
                    rhs.span,
                    COALESCE_FALLBACK_TYPE_MISMATCH,
                );
                self.types.option(inner)
            }
            Type::Primitive(Primitive::Nihil) => self.types.option(inner),
            _ => {
                self.unify_assign_issue(
                    fallback_elem,
                    inner,
                    rhs.span,
                    COALESCE_FALLBACK_TYPE_MISMATCH,
                );
                inner
            }
        };
        Some(self.lift_result(element, shape))
    }

    /// The `✓ ✗` value conditional over a `bivalens` mask: both branches are
    /// evaluated and each element takes the branch its mask element selects.
    /// `mask_ty` is the already-checked condition type.
    pub(in crate::passes::typecheck) fn check_tensor_select(
        &mut self,
        cond: &HirExpression,
        mask_ty: TypeId,
        then_block: &mut HirBlock,
        else_block: Option<&mut HirBlock>,
        expected: Option<TypeId>,
    ) -> TypeId {
        let LiftSide::Tensor {
            elem: mask_elem,
            shape,
        } = self.lift_side(mask_ty)
        else {
            return self.error_type;
        };
        let span = cond.span;
        let plain_branch = |block: &HirBlock| block.statements.is_empty() && block.expr.is_some();
        let Some(else_block) =
            else_block.filter(|block| plain_branch(block) && plain_branch(then_block))
        else {
            self.invalid_operand_types(ISSUE_CONDITION_MUST_BE_BIVALENS, span);
            return self.error_type;
        };
        if !self.is_bool(mask_elem) {
            self.invalid_operand_types(ISSUE_CONDITION_MUST_BE_BIVALENS, span);
            return self.error_type;
        }
        let hint = expected.and_then(|ty| match self.lift_side(ty) {
            LiftSide::Tensor { elem, .. } => Some(elem),
            _ => None,
        });
        let then_side = self.check_select_branch(then_block, hint, shape, span);
        let else_side = self.check_select_branch(else_block, hint, shape, span);
        let (Some((then_elem, then_const)), Some((else_elem, else_const))) = (then_side, else_side)
        else {
            return self.error_type;
        };
        let joined = if let Some(joined) =
            self.integer_operand_join((then_elem, then_const), (else_elem, else_const), span)
        {
            joined
        } else {
            self.common_type_with_consts(
                (then_elem, then_const.map(|value| (value, value))),
                (else_elem, else_const.map(|value| (value, value))),
                span,
            )
        };
        if self.is_error(joined) {
            return self.error_type;
        }
        self.lift_result(joined, shape)
    }

    /// Check one select branch (a lone tail expression) and return the element
    /// type it contributes with its integer-constant value, if any. A tensor
    /// branch must have the mask's shape.
    fn check_select_branch(
        &mut self,
        block: &mut HirBlock,
        hint: Option<TypeId>,
        shape: IndexId,
        span: radix_lexer::Span,
    ) -> Option<(TypeId, Option<i128>)> {
        let literal = block.expr.as_deref().is_some_and(is_numeric_literal);
        let ty = self.check_block(block, hint.filter(|_| literal));
        if self.is_error(ty) {
            return None;
        }
        let konst = block.expr.as_deref().and_then(const_int_value);
        match self.lift_side(ty) {
            LiftSide::Tensor {
                elem,
                shape: branch_shape,
            } => {
                let before = self.errors.len();
                self.constrain_index_equal(shape, branch_shape, span);
                (self.errors.len() == before).then_some((elem, None))
            }
            LiftSide::Register => {
                self.invalid_operand_types(COMMON_TYPE_MISMATCH, span);
                None
            }
            LiftSide::Scalar(scalar) => Some((scalar, konst)),
        }
    }
}
