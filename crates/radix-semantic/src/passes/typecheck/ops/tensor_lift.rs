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
//!
//! A register `vector` or `matrix` takes the same arithmetic rule (the K4 note
//! "the same lifting rule applies to vector and matrix", rule 15.1): `+ - * /
//! % ÷` against a scalar on either side, unary `-`, and `÷`, `⤒`, `⤓` against a
//! same-shape register. `vector * vector` and `matrix * matrix` are errors that
//! point at `⊙` and `·`; `/` between two aggregates points at `⊘` and `÷`, and
//! `%` between two aggregates says it lifts only against a scalar (tensors
//! share both). `≈ ≉` on a register wait for L7 like the tensor form. Registers
//! lift no comparison, logic or select yet.

use super::{
    ARGUMENT_TYPE_MISMATCH, BOOLEAN_OPERAND_REQUIRED, BOOLEAN_OPERANDS_REQUIRED,
    COALESCE_FALLBACK_TYPE_MISMATCH, COMMON_TYPE_MISMATCH, HirBinOp, HirBlock, HirExpression,
    MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED, NUMERIC_OPERANDS_REQUIRED, Primitive,
    SemanticErrorKind, TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED, TENSOR_MULTIPLY_REQUIRES_GLYPH,
    Type, TypeChecker, TypeId, is_numeric_literal,
};
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
/// `vector * vector` is not elementwise; the glyphs say which product is meant.
pub(in crate::passes::typecheck) const ISSUE_VECTOR_MULTIPLY_REQUIRES_GLYPH: &str =
    "vector_multiply_requires_glyph";
/// `matrix * matrix` is not elementwise either.
pub(in crate::passes::typecheck) const ISSUE_MATRIX_MULTIPLY_REQUIRES_GLYPH: &str =
    "matrix_multiply_requires_glyph";
/// `/` between two aggregates is not elementwise; `⊘` (floor) and `÷` (true)
/// are. The `kind` argument names the aggregate.
pub(in crate::passes::typecheck) const ISSUE_ELEMENTWISE_DIVISION_REQUIRES_GLYPH: &str =
    "elementwise_division_requires_glyph";
/// `%` lifts only against a scalar; there is no elementwise glyph for it.
pub(in crate::passes::typecheck) const ISSUE_ELEMENTWISE_REMAINDER_REQUIRES_SCALAR: &str =
    "elementwise_remainder_requires_scalar";

/// Spelling of the approximate-equality glyph in a deferral diagnostic.
pub(super) fn approx_glyph(op: HirBinOp) -> &'static str {
    if op == HirBinOp::FuzzyNotEq {
        "≉"
    } else {
        "≈"
    }
}

/// How one operand takes part in a lifted operator.
pub(super) enum LiftSide {
    Tensor {
        elem: TypeId,
        shape: IndexId,
    },
    /// A vector or matrix: keeps its own rules, never lifted here.
    Register,
    Scalar(TypeId),
}

/// Which register a lifted form operates on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RegisterKind {
    Vector,
    Matrix,
}

/// A `vector` or `matrix` operand of a lifted form: the `extent` is the width
/// index of a vector or the shape index of a matrix.
pub(super) struct RegisterOperand {
    pub(super) kind: RegisterKind,
    pub(super) elem: TypeId,
    pub(super) extent: IndexId,
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
    pub(super) fn lift_side(&mut self, ty: TypeId) -> LiftSide {
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

    /// A register operand: its kind, element and width (vector) or shape
    /// (matrix).
    pub(super) fn register_operand(&mut self, ty: TypeId) -> Option<RegisterOperand> {
        let resolved = self.value_carrier_type(ty);
        let (_, payload) = self.types.peel_access_marker(resolved);
        match self.types.get(payload).clone() {
            Type::Vector(elem, extent) => Some(RegisterOperand {
                kind: RegisterKind::Vector,
                elem: self.resolve_inferred_type(elem),
                extent,
            }),
            Type::Matrix(elem, extent) => Some(RegisterOperand {
                kind: RegisterKind::Matrix,
                elem: self.resolve_inferred_type(elem),
                extent,
            }),
            _ => None,
        }
    }

    /// The register type of `register`'s kind and extent holding `elem`.
    pub(super) fn register_result(&mut self, register: &RegisterOperand, elem: TypeId) -> TypeId {
        match register.kind {
            RegisterKind::Vector => self.types.vector_with_width(elem, register.extent),
            RegisterKind::Matrix => self.types.matrix_with_shape(elem, register.extent),
        }
    }

    /// The type a literal beside a tensor or register takes as its peer: the
    /// numeric element, so `x * 0.5` on an `f32` tensor stays `f32`.
    pub(in crate::passes::typecheck) fn scalar_peer_type(&mut self, ty: TypeId) -> TypeId {
        let elem = match self.lift_side(ty) {
            LiftSide::Tensor { elem, .. } => Some(elem),
            _ => self.register_operand(ty).map(|register| register.elem),
        };
        match elem {
            Some(elem) if crate::numeric_width::is_tensor_numeric_element(self.types, elem) => elem,
            _ => ty,
        }
    }

    /// `+ - * / % ÷` between a register and a scalar (either side), and `÷`
    /// between two registers of one kind and shape. The element is the F9
    /// join of the register element and the scalar, as for a tensor (`÷` by
    /// ruling 23), and the result is a new register of the same shape. Two
    /// registers under `* / %` are the glyph diagnostics; every other pair
    /// keeps its own rule.
    fn check_register_lift_arith(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        if !matches!(
            op,
            HirBinOp::Add
                | HirBinOp::Sub
                | HirBinOp::Mul
                | HirBinOp::Div
                | HirBinOp::Mod
                | HirBinOp::TrueDiv
        ) {
            return None;
        }
        let span = lhs.span;
        let (left, right) = (self.register_operand(lhs_ty), self.register_operand(rhs_ty));
        let (register, scalar_ty, register_first) = match (left, right) {
            (Some(left), Some(right)) => {
                return self.check_register_pair(op, (&left, &right), (lhs, rhs), span);
            }
            (Some(register), None) => (register, rhs_ty, true),
            (None, Some(register)) => (register, lhs_ty, false),
            (None, None) => return None,
        };
        // A tensor or another aggregate beside a register keeps its own rule.
        if !matches!(self.lift_side(scalar_ty), LiftSide::Scalar(_)) {
            return None;
        }
        if self.is_error(scalar_ty) || !self.register_element_admitted(&register, span) {
            return Some(self.error_type);
        }
        let resolved = self.resolve_inferred_type(scalar_ty);
        if !self.is_infer(resolved) && self.numeric_shape(resolved).is_none() {
            self.invalid_operand_types(NUMERIC_OPERANDS_REQUIRED, span);
            return Some(self.error_type);
        }
        let (l_elem, r_elem) = if register_first {
            (register.elem, scalar_ty)
        } else {
            (scalar_ty, register.elem)
        };
        // The element is the F9 join of the operands, as for a tensor. Each
        // lane is stored under it at the operation (ruling 19(g)): an integer
        // lane is exact and traps where it leaves the element's range.
        let elem = self.lift_arith_element(op, lhs, rhs, l_elem, r_elem, span);
        if self.is_error(elem) {
            return Some(self.error_type);
        }
        Some(self.register_result(&register, elem))
    }

    /// Two registers under an arithmetic operator: `÷` pairs them elementwise;
    /// `* / %` are the glyph diagnostics; `+ -` keep their `addita` rule. A
    /// vector beside a matrix keeps its own rule.
    fn check_register_pair(
        &mut self,
        op: HirBinOp,
        (left, right): (&RegisterOperand, &RegisterOperand),
        (lhs, rhs): (&HirExpression, &HirExpression),
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if left.kind != right.kind {
            return None;
        }
        let kind = match left.kind {
            RegisterKind::Vector => "vector",
            RegisterKind::Matrix => "matrix",
        };
        match op {
            HirBinOp::Mul => {
                let issue = match left.kind {
                    RegisterKind::Vector => ISSUE_VECTOR_MULTIPLY_REQUIRES_GLYPH,
                    RegisterKind::Matrix => ISSUE_MATRIX_MULTIPLY_REQUIRES_GLYPH,
                };
                self.invalid_operand_types(issue, span);
                Some(self.error_type)
            }
            HirBinOp::Div | HirBinOp::Mod => {
                let issue = if op == HirBinOp::Div {
                    ISSUE_ELEMENTWISE_DIVISION_REQUIRES_GLYPH
                } else {
                    ISSUE_ELEMENTWISE_REMAINDER_REQUIRES_SCALAR
                };
                self.invalid_operand_types_with_args(issue, span, &[("kind", kind.to_owned())]);
                Some(self.error_type)
            }
            HirBinOp::TrueDiv => {
                if self.is_error(left.elem) || self.is_error(right.elem) {
                    return Some(self.error_type);
                }
                if !self.register_element_admitted(left, span)
                    || !self.register_element_admitted(right, span)
                {
                    return Some(self.error_type);
                }
                let before = self.errors.len();
                self.constrain_index_equal(left.extent, right.extent, span);
                if self.errors.len() != before {
                    return Some(self.error_type);
                }
                let elem = self.lift_arith_element(op, lhs, rhs, left.elem, right.elem, span);
                if self.is_error(elem) {
                    return Some(self.error_type);
                }
                Some(self.register_result(left, elem))
            }
            _ => None,
        }
    }

    /// A register element must be a numeric, non-decimal scalar (the matrix
    /// register keeps its own diagnostic for it).
    pub(super) fn register_element_admitted(
        &mut self,
        register: &RegisterOperand,
        span: radix_lexer::Span,
    ) -> bool {
        let issue = match register.kind {
            RegisterKind::Vector => TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED,
            RegisterKind::Matrix => MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED,
        };
        self.lift_element_admitted_as(register.elem, span, issue)
    }

    /// Unary `-` over a register keeps the element type and shape. Each lane
    /// is stored under that element at the operation, so an integer lane
    /// traps where the negation leaves the range (`-i8::MIN`).
    fn check_register_lift_neg(
        &mut self,
        operand: &HirExpression,
        operand_ty: TypeId,
    ) -> Option<TypeId> {
        let register = self.register_operand(operand_ty)?;
        if !self.register_element_admitted(&register, operand.span) {
            return Some(self.error_type);
        }
        Some(self.value_carrier_type(operand_ty))
    }

    pub(super) fn lift_result(&mut self, elem: TypeId, shape: IndexId) -> TypeId {
        self.types.tensor_with_shape(elem, shape)
    }

    pub(super) fn lift_bool_result(&mut self, shape: IndexId) -> TypeId {
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
        self.lift_element_admitted_as(elem, span, TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED)
    }

    /// [`Self::lift_element_admitted`] reporting a non-numeric element as `issue`.
    fn lift_element_admitted_as(
        &mut self,
        elem: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> bool {
        if self.is_error(elem) {
            return false;
        }
        if !crate::numeric_width::is_tensor_numeric_element(self.types, elem) {
            self.invalid_operand_types(issue, span);
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
        if let Some(result) = self.check_register_lift_arith(op, lhs, rhs, lhs_ty, rhs_ty) {
            return Some(result);
        }
        let pair = match self.lift_pairing(lhs_ty, rhs_ty, span, op == HirBinOp::TrueDiv) {
            LiftPairing::NotLifted => {
                let tensor_pair = matches!(self.lift_side(lhs_ty), LiftSide::Tensor { .. })
                    && matches!(self.lift_side(rhs_ty), LiftSide::Tensor { .. });
                if tensor_pair {
                    match op {
                        HirBinOp::Mul => {
                            self.invalid_operand_types(TENSOR_MULTIPLY_REQUIRES_GLYPH, span);
                            return Some(self.error_type);
                        }
                        HirBinOp::Div | HirBinOp::Mod => {
                            let issue = if op == HirBinOp::Div {
                                ISSUE_ELEMENTWISE_DIVISION_REQUIRES_GLYPH
                            } else {
                                ISSUE_ELEMENTWISE_REMAINDER_REQUIRES_SCALAR
                            };
                            self.invalid_operand_types_with_args(
                                issue,
                                span,
                                &[("kind", "tensor".to_owned())],
                            );
                            return Some(self.error_type);
                        }
                        _ => {}
                    }
                }
                return None;
            }
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
            return self.check_register_lift_neg(operand, operand_ty);
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
            return self.check_register_lift_not(operand, operand_ty);
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
        if let Some(result) = self.check_register_lift_logic(lhs, lhs_ty, rhs_ty) {
            return Some(result);
        }
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

    /// Comparisons over aggregates (advisor rulings 2026-10-04). `≡ ≢` compare
    /// whole values on every type and give one `bivalens` (see
    /// `check_aggregate_equality`); `< > ≤ ≥ ≅ ≇` lift per element like `+`, over
    /// a tensor or a register `vector`/`matrix` against a scalar or a same-shape
    /// peer, giving a bool aggregate of the same kind and shape. The element rule
    /// is the scalar rule for the glyph (ordering and `≅ ≇` join by value).
    /// `≈ ≉` wait for the numeric contract (L7).
    pub(in crate::passes::typecheck) fn check_tensor_lift_compare(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let span = lhs.span;
        if matches!(
            op,
            HirBinOp::Eq | HirBinOp::NotEq | HirBinOp::StrictEq | HirBinOp::StrictNotEq
        ) {
            return self.check_aggregate_equality(lhs, rhs, lhs_ty, rhs_ty);
        }
        // `≈ ≉` over a register or a sparsa waits for the same L7 ruling as
        // over a tensor.
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq)
            && (self.register_operand(lhs_ty).is_some()
                || self.register_operand(rhs_ty).is_some()
                || self.is_sparsa_operand(lhs_ty)
                || self.is_sparsa_operand(rhs_ty))
        {
            let kind = self.aggregate_kind_of_pair(lhs_ty, rhs_ty);
            self.invalid_operand_types_with_args(
                ISSUE_TENSOR_APPROX_COMPARISON_DEFERRED,
                span,
                &[
                    ("op", approx_glyph(op).to_owned()),
                    ("kind", kind.to_owned()),
                ],
            );
            return Some(self.error_type);
        }
        if let Some(result) = self.check_register_lift_compare(op, lhs, lhs_ty, rhs_ty) {
            return Some(result);
        }
        let pair = match self.lift_pairing(lhs_ty, rhs_ty, span, true) {
            LiftPairing::NotLifted => return None,
            LiftPairing::Failed => return Some(self.error_type),
            LiftPairing::Lifted(pair) => pair,
        };
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq) {
            self.invalid_operand_types_with_args(
                ISSUE_TENSOR_APPROX_COMPARISON_DEFERRED,
                span,
                &[
                    ("op", approx_glyph(op).to_owned()),
                    ("kind", "tensor".to_owned()),
                ],
            );
            return Some(self.error_type);
        }
        if !self.lift_pair_elements_admitted(&pair, span) {
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
            _ => return None,
        }
        if self.errors.len() != before {
            return Some(self.error_type);
        }
        Some(self.lift_bool_result(pair.shape))
    }

    /// `a ⤒ b` / `a ⤓ b` with a register receiver: the lanewise extremum
    /// against a scalar of the element type or a register of the same kind
    /// and shape (a tensor peer is a mismatch).
    fn check_register_extremum(
        &mut self,
        register: &RegisterOperand,
        receiver_ty: TypeId,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> TypeId {
        let [arg] = args else {
            return self.error_type;
        };
        let hint = is_numeric_literal(&arg.expr).then_some(register.elem);
        let arg_ty = self.check_expr_with_expected(&mut arg.expr, hint);
        if self.is_error(arg_ty) || !self.register_element_admitted(register, span) {
            return self.error_type;
        }
        if let Some(other) = self.register_operand(arg_ty) {
            if other.kind != register.kind {
                self.invalid_operand_types(ARGUMENT_TYPE_MISMATCH, span);
                return self.error_type;
            }
            let before = self.errors.len();
            self.constrain_index_equal(register.extent, other.extent, span);
            if self.errors.len() != before || !self.register_element_admitted(&other, span) {
                return self.error_type;
            }
            let joined = self.lift_join_element((register.elem, None), (other.elem, None), span);
            if self.is_error(joined) {
                return self.error_type;
            }
            return self.register_result(register, joined);
        }
        if let LiftSide::Scalar(_) = self.lift_side(arg_ty) {
            self.unify_assign_issue(arg_ty, register.elem, arg.expr.span, ARGUMENT_TYPE_MISMATCH);
            receiver_ty
        } else {
            self.invalid_operand_types(ARGUMENT_TYPE_MISMATCH, span);
            self.error_type
        }
    }

    /// `a ⤒ b` / `a ⤓ b` with a tensor or register receiver: the elementwise
    /// extremum against a scalar of the element type or a same-shape peer.
    pub(in crate::passes::typecheck) fn check_tensor_extremum(
        &mut self,
        receiver_ty: TypeId,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> TypeId {
        if let Some(register) = self.register_operand(receiver_ty) {
            return self.check_register_extremum(&register, receiver_ty, args, span);
        }
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
    /// The mask is a bool tensor, vector or matrix, and so is the result: a
    /// branch is a scalar or an aggregate of the mask's kind and shape.
    /// `mask_ty` is the already-checked condition type.
    pub(in crate::passes::typecheck) fn check_tensor_select(
        &mut self,
        cond: &HirExpression,
        mask_ty: TypeId,
        then_block: &mut HirBlock,
        else_block: Option<&mut HirBlock>,
        expected: Option<TypeId>,
    ) -> TypeId {
        let (mask_elem, frame) = match self.lift_side(mask_ty) {
            LiftSide::Tensor { elem, shape } => (elem, SelectFrame::Tensor(shape)),
            _ => match self.register_operand(mask_ty) {
                Some(register) => (register.elem, SelectFrame::Register(register)),
                None => return self.error_type,
            },
        };
        let span = cond.span;
        let plain_branch = |block: &HirBlock| block.statements.is_empty() && block.expr.is_some();
        let Some(else_block) =
            else_block.filter(|block| plain_branch(block) && plain_branch(then_block))
        else {
            self.report_mask_not_truth_value(mask_ty, span);
            return self.error_type;
        };
        if !self.is_bool(mask_elem) {
            self.invalid_operand_types(ISSUE_CONDITION_MUST_BE_BIVALENS, span);
            return self.error_type;
        }
        let hint = expected.and_then(|ty| match self.lift_side(ty) {
            LiftSide::Tensor { elem, .. } => Some(elem),
            _ => self.register_operand(ty).map(|register| register.elem),
        });
        let then_side = self.check_select_branch(then_block, hint, &frame, span);
        let else_side = self.check_select_branch(else_block, hint, &frame, span);
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
        match frame {
            SelectFrame::Tensor(shape) => self.lift_result(joined, shape),
            SelectFrame::Register(register) => self.register_result(&register, joined),
        }
    }

    /// Check one select branch (a lone tail expression) and return the element
    /// type it contributes with its integer-constant value, if any. An
    /// aggregate branch must be the mask's kind and shape.
    fn check_select_branch(
        &mut self,
        block: &mut HirBlock,
        hint: Option<TypeId>,
        frame: &SelectFrame,
        span: radix_lexer::Span,
    ) -> Option<(TypeId, Option<i128>)> {
        let literal = block.expr.as_deref().is_some_and(is_numeric_literal);
        let ty = self.check_block(block, hint.filter(|_| literal));
        if self.is_error(ty) {
            return None;
        }
        let konst = block.expr.as_deref().and_then(const_int_value);
        if let (SelectFrame::Register(mask), Some(branch)) = (frame, self.register_operand(ty)) {
            if branch.kind != mask.kind {
                self.invalid_operand_types(COMMON_TYPE_MISMATCH, span);
                return None;
            }
            let before = self.errors.len();
            self.constrain_index_equal(mask.extent, branch.extent, span);
            return (self.errors.len() == before).then_some((branch.elem, None));
        }
        match self.lift_side(ty) {
            LiftSide::Tensor {
                elem,
                shape: branch_shape,
            } => {
                let SelectFrame::Tensor(shape) = frame else {
                    self.invalid_operand_types(COMMON_TYPE_MISMATCH, span);
                    return None;
                };
                let before = self.errors.len();
                self.constrain_index_equal(*shape, branch_shape, span);
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

/// What a select mask ranges over: a tensor's shape or a register's kind and
/// extent.
enum SelectFrame {
    Tensor(IndexId),
    Register(RegisterOperand),
}
