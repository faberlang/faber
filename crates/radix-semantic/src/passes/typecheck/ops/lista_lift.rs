//! Elementwise arithmetic over numeric lists (A1 and the 2026-10-04 aggregate
//! operator rulings).
//!
//! `+ - * / % ÷` take a list and a scalar on either side (operand order is
//! kept) and unary `-` negates each element. Two lists pair by position:
//! `+ - ÷` and the glyph forms `⊙ ⊘` apply per element, while `*` and `/`
//! between two lists are errors that name `⊙` and `⊘`, exactly as for a
//! vector or a tensor. `%` takes a scalar on one side only.
//!
//! The element type of a list/scalar pair is the F9 join of the list element
//! and the scalar, so an untyped literal adopts the list's element type and a
//! constant joins by value; two typed lists keep the strict rule that their
//! elements are one type (`↦` aligns widths). `÷` takes its width from ruling
//! 23. A fixed capacity is part of the type (D4): two lists of different
//! literal capacities are a check-time error, and the live lengths pair at run
//! time (a capacity never stands in for a length). Every other target either
//! runs the form or fails closed (`lista_lift_gate`).

use super::tensor_lift::{ISSUE_TENSOR_APPROX_COMPARISON_DEFERRED, approx_glyph};
use super::{
    BOOLEAN_OPERAND_REQUIRED, BOOLEAN_OPERANDS_REQUIRED, GLYPH_ELEMENT_TYPE_MISMATCH,
    GLYPH_OP_NUMERIC_ELEMENT_REQUIRED, HirBinOp, HirExpression, IndexExpr,
    NUMERIC_LISTA_OPERATOR_LAW, NUMERIC_OPERAND_REQUIRED, Type, TypeChecker, TypeId,
    lista_operator_spelling,
};
use crate::passes::typecheck::integer_range::const_int_value;

/// `list * list` is not elementwise; the glyph says which product is meant.
const ISSUE_LISTA_MULTIPLY_REQUIRES_GLYPH: &str = "lista_multiply_requires_glyph";
/// `list / list` is not elementwise; the glyphs name the two divisions.
const ISSUE_LISTA_DIVIDE_REQUIRES_GLYPH: &str = "lista_divide_requires_glyph";
/// `list % list` has no pairing; `%` takes a scalar on one side.
const ISSUE_LISTA_SCALAR_REQUIRED: &str = "lista_scalar_required";
/// Two lists with different literal capacities are different types (D4).
const ISSUE_LISTA_CAPACITY_MISMATCH: &str = "lista_capacity_mismatch";
/// Bits are not numbers: a `lista<bN>` has whole-value equality only.
const ISSUE_LISTA_BITS_WHOLE_EQUALITY_ONLY: &str = "lista_bits_whole_equality_only";
/// `≈ ≉` compare numbers; a list, set, map or other aggregate that is not a
/// numeric tensor is refused by name (`approx-aggregate-named-reject`).
const ISSUE_APPROX_REQUIRES_NUMERIC_OPERANDS: &str = "approx_requires_numeric_operands";
/// Decimal elements have no `÷` yet.
const ISSUE_LISTA_DECIMAL_ELEMENT: &str = "lista_decimal_element_unsupported";

#[derive(Clone, Copy)]
struct ListaArithmeticShape {
    element: TypeId,
    capacity: Option<radix_types::IndexId>,
}

impl TypeChecker<'_> {
    /// The type a literal beside a list or a tensor/vector takes as its peer:
    /// the numeric element, so `xs * 0.5` on an `f32` list stays `f32`.
    pub(in crate::passes::typecheck) fn arith_peer_type(&mut self, ty: TypeId) -> TypeId {
        match self.lista_arithmetic_shape(ty) {
            Some(shape) if self.is_numeric_lista_element(shape.element) => {
                self.resolve_inferred_type(shape.element)
            }
            _ => self.scalar_peer_type(ty),
        }
    }

    /// A lifted operator builds a fresh container, so its elements are stores
    /// into the destination's element type, exactly as a literal's are. The
    /// exact range the operator gave each element (`list<u8> + list<u8>` is a
    /// list of `0…510`) is not a type the author can spell, and no name shares
    /// its storage, so type-argument invariance (ruling 21 row 25) does not
    /// apply to it: a result whose element fits the expected container's
    /// element takes that element type. A named container never does.
    pub(in crate::passes::typecheck) fn adopt_expected_lift_element(
        &mut self,
        result: TypeId,
        expected: Option<TypeId>,
    ) -> TypeId {
        let Some(expected) = expected else {
            return result;
        };
        let result = self.resolve_inferred_type(result);
        let expected = self.peel_nominal_aliases(expected);
        let expected = self.types.peel_access_marker(expected).1;
        let expected = self.peel_nominal_aliases(expected);
        let adopted = match (
            self.types.get(result).clone(),
            self.types.get(expected).clone(),
        ) {
            (Type::Array(from), Type::Array(to)) => self
                .lift_element_fits(from, to)
                .then(|| self.types.array(to)),
            (
                Type::BoundedArray {
                    element: from,
                    capacity: from_cap,
                },
                Type::BoundedArray {
                    element: to,
                    capacity: to_cap,
                },
            ) if self.types.index_equals(from_cap, to_cap) => self
                .lift_element_fits(from, to)
                .then(|| self.types.bounded_array(to, to_cap)),
            (Type::Tensor(from, from_shape), Type::Tensor(to, to_shape))
                if self.types.index_equals(from_shape, to_shape) =>
            {
                self.lift_element_fits(from, to)
                    .then(|| self.types.tensor_with_shape(to, to_shape))
            }
            (Type::Vector(from, from_shape), Type::Vector(to, to_shape))
                if self.types.index_equals(from_shape, to_shape) =>
            {
                self.lift_element_fits(from, to)
                    .then(|| self.types.vector_with_width(to, to_shape))
            }
            (Type::Matrix(from, from_shape), Type::Matrix(to, to_shape))
                if self.types.index_equals(from_shape, to_shape) =>
            {
                self.lift_element_fits(from, to)
                    .then(|| self.types.matrix_with_shape(to, to_shape))
            }
            _ => None,
        };
        adopted.unwrap_or(result)
    }

    /// Whether a lifted element widens into the destination element: a
    /// different numeric type that the ordinary scalar store accepts.
    fn lift_element_fits(&self, from: TypeId, to: TypeId) -> bool {
        let from = self.resolve_inferred_type(from);
        let to = self.resolve_inferred_type(to);
        !self.types.equals(from, to)
            && self.numeric_shape(from).is_some()
            && self.numeric_shape(to).is_some()
            && self.types.is_assignable(from, to)
    }

    /// Check the elementwise list arithmetic law for `+ - * / % ÷`.
    ///
    /// `None` means neither operand is a `lista`, so the scalar/tensor caller
    /// continues with its existing arithmetic rule. `Some` means a list pair
    /// or list/scalar broadcast was handled, including a deliberate error.
    pub(in crate::passes::typecheck) fn check_lista_arithmetic_bin(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let lhs_lista = self.lista_arithmetic_shape(lhs_ty);
        let rhs_lista = self.lista_arithmetic_shape(rhs_ty);
        if lhs_lista.is_none() && rhs_lista.is_none() {
            return None;
        }
        let span = lhs.span;
        let operator = lista_operator_spelling(op);
        let law_error = |checker: &mut Self| {
            checker.invalid_operand_types_with_args(
                NUMERIC_LISTA_OPERATOR_LAW,
                span,
                &[("operator", operator.to_owned())],
            );
            checker.error_type
        };

        let (result_element, result_capacity) = match (lhs_lista, rhs_lista) {
            (Some(left), Some(right)) => {
                match op {
                    HirBinOp::Mul => {
                        self.invalid_operand_types(ISSUE_LISTA_MULTIPLY_REQUIRES_GLYPH, span);
                        return Some(self.error_type);
                    }
                    HirBinOp::Div => {
                        self.invalid_operand_types(ISSUE_LISTA_DIVIDE_REQUIRES_GLYPH, span);
                        return Some(self.error_type);
                    }
                    HirBinOp::Mod => {
                        self.invalid_operand_types_with_args(
                            ISSUE_LISTA_SCALAR_REQUIRED,
                            span,
                            &[("operator", operator.to_owned())],
                        );
                        return Some(self.error_type);
                    }
                    _ => {}
                }
                let Some(capacity) = self.lista_pair_capacity(left, right, span, op) else {
                    return Some(self.error_type);
                };
                let Some(element) =
                    self.lista_pair_element(op, lhs, rhs, left.element, right.element, span)
                else {
                    return Some(law_error(self));
                };
                if self.is_error(element) {
                    return Some(self.error_type);
                }
                (element, capacity)
            }
            (Some(list), None) | (None, Some(list)) => {
                let list_first = lhs_lista.is_some();
                let scalar_ty = if list_first { rhs_ty } else { lhs_ty };
                if !self.is_numeric_lista_element(list.element)
                    || !self.is_numeric_scalar(scalar_ty)
                {
                    return Some(law_error(self));
                }
                let element = self.lista_scalar_element(
                    op,
                    (lhs, lhs_ty),
                    (rhs, rhs_ty),
                    list.element,
                    list_first,
                );
                if self.is_error(element) {
                    return Some(self.error_type);
                }
                if op == HirBinOp::TrueDiv
                    && self.lista_decimal_true_div(list.element, scalar_ty, span)
                {
                    return Some(self.error_type);
                }
                (element, list.capacity)
            }
            (None, None) => return None,
        };
        Some(self.lista_result(result_element, result_capacity))
    }

    /// `≺ ≻ ≤ ≥ ≅ ≇` over numeric lists (advisor rulings 2026-10-04, Q-B): the
    /// glyph applies per element, between two lists or a list and a scalar in
    /// either order, and gives a `lista<bivalens>` of the list shape. The
    /// element rule is the scalar rule for the glyph (the same one a tensor
    /// uses). `None` means the pair is not lifted here: no list, or a list
    /// whose elements are not numbers (whole-value `≅`, or the scalar rule's
    /// own refusal). A `lista<bN>` ordering names the whole-value-only rule.
    pub(in crate::passes::typecheck) fn check_lista_compare(
        &mut self,
        op: HirBinOp,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let left = self.lista_arithmetic_shape(lhs_ty);
        let right = self.lista_arithmetic_shape(rhs_ty);
        if left.is_none() && right.is_none() {
            return None;
        }
        let ordering = matches!(
            op,
            HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq
        );
        let lists = [left, right];
        if ordering
            && lists
                .iter()
                .flatten()
                .any(|shape| self.bits_width_of(shape.element).is_some())
        {
            self.invalid_operand_types(ISSUE_LISTA_BITS_WHOLE_EQUALITY_ONLY, span);
            return Some(self.error_type);
        }
        let numeric = |checker: &Self, element: TypeId| {
            let resolved = checker.resolve_inferred_type(element);
            checker.numeric_shape(resolved).is_some()
        };
        if lists
            .iter()
            .flatten()
            .any(|shape| !numeric(self, shape.element))
        {
            return None;
        }
        // `≈ ≉` over a numeric list wait for the tolerance ruling (L7), like
        // a tensor: fail closed at check time.
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq) {
            self.invalid_operand_types_with_args(
                ISSUE_TENSOR_APPROX_COMPARISON_DEFERRED,
                span,
                &[
                    ("op", approx_glyph(op).to_owned()),
                    ("kind", "list".to_owned()),
                ],
            );
            return Some(self.error_type);
        }
        let (l_elem, r_elem, capacity) = match (left, right) {
            (Some(left), Some(right)) => {
                let Some(capacity) = self.lista_pair_capacity(left, right, span, op) else {
                    return Some(self.error_type);
                };
                (left.element, right.element, capacity)
            }
            (Some(list), None) => (list.element, rhs_ty, list.capacity),
            (None, Some(list)) => (lhs_ty, list.element, list.capacity),
            (None, None) => return None,
        };
        if self.is_error(l_elem) || self.is_error(r_elem) {
            return Some(self.error_type);
        }
        let before = self.errors.len();
        if ordering {
            self.numeric_ordering_bin(l_elem, r_elem, span);
        } else {
            self.numeric_value_equality_types(l_elem, r_elem, span);
        }
        if self.errors.len() != before {
            return Some(self.error_type);
        }
        let bivalens = self.bool_type();
        Some(self.lista_result(bivalens, capacity))
    }

    /// `⊙` and `⊘` over two lists: the element is the shared element type and
    /// the result keeps the list shape. `None` means the operands are not two
    /// lists, so the caller reports the glyph's own operand diagnostic.
    pub(in crate::passes::typecheck) fn check_lista_hadamard(
        &mut self,
        op: HirBinOp,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let (left, right) = (
            self.lista_arithmetic_shape(lhs_ty)?,
            self.lista_arithmetic_shape(rhs_ty)?,
        );
        let Some(capacity) = self.lista_pair_capacity(left, right, span, op) else {
            return Some(self.error_type);
        };
        if !self.is_numeric_lista_element(left.element)
            || !self.is_numeric_lista_element(right.element)
        {
            self.glyph_invalid_operand(op, GLYPH_OP_NUMERIC_ELEMENT_REQUIRED, span);
            return Some(self.error_type);
        }
        if !self.numeric_shapes_match(left.element, right.element) {
            self.glyph_invalid_operand(op, GLYPH_ELEMENT_TYPE_MISMATCH, span);
            return Some(self.error_type);
        }
        Some(self.lista_result(left.element, capacity))
    }

    /// Unary `-` over a list keeps the element type and the list shape; each
    /// element is stored under it, so negating the minimum of a signed width
    /// traps at the store.
    pub(in crate::passes::typecheck) fn check_lista_neg(
        &mut self,
        operand: &HirExpression,
        operand_ty: TypeId,
    ) -> Option<TypeId> {
        let shape = self.lista_arithmetic_shape(operand_ty)?;
        if !self.is_numeric_lista_element(shape.element) {
            self.invalid_operand_types(NUMERIC_OPERAND_REQUIRED, operand.span);
            return Some(self.error_type);
        }
        Some(self.lista_result(shape.element, shape.capacity))
    }

    /// `non` over a `lista<bivalens>` negates each element (Q-C of the
    /// 2026-10-04 comparisons ruling). `None` means the operand is not a list.
    pub(in crate::passes::typecheck) fn check_lista_not(
        &mut self,
        operand: &HirExpression,
        operand_ty: TypeId,
    ) -> Option<TypeId> {
        let shape = self.lista_arithmetic_shape(operand_ty)?;
        if !self.lista_bool_element(shape.element, operand.span, BOOLEAN_OPERAND_REQUIRED) {
            return Some(self.error_type);
        }
        let bivalens = self.bool_type();
        Some(self.lista_result(bivalens, shape.capacity))
    }

    /// `et` / `aut` over `lista<bivalens>` pair per element, a list against a
    /// scalar bool in either order broadcasts; both sides are always
    /// evaluated. `None` means neither operand is a list.
    pub(in crate::passes::typecheck) fn check_lista_logic(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let left = self.lista_arithmetic_shape(lhs_ty);
        let right = self.lista_arithmetic_shape(rhs_ty);
        if left.is_none() && right.is_none() {
            return None;
        }
        let span = lhs.span;
        let (capacity, scalar_ty) = match (left, right) {
            (Some(left), Some(right)) => {
                let Some(capacity) = self.lista_pair_capacity(left, right, span, op) else {
                    return Some(self.error_type);
                };
                (capacity, None)
            }
            (Some(list), None) => (list.capacity, Some(rhs_ty)),
            (None, Some(list)) => (list.capacity, Some(lhs_ty)),
            (None, None) => return None,
        };
        let mut admitted = true;
        for shape in [left, right].into_iter().flatten() {
            admitted &= self.lista_bool_element(shape.element, span, BOOLEAN_OPERANDS_REQUIRED);
        }
        if let Some(scalar_ty) = scalar_ty {
            admitted &= self.lista_bool_element(scalar_ty, span, BOOLEAN_OPERANDS_REQUIRED);
        }
        if !admitted {
            return Some(self.error_type);
        }
        let bivalens = self.bool_type();
        Some(self.lista_result(bivalens, capacity))
    }

    /// Whether `ty` is a bool (an inferred slot is unified with bool); a
    /// non-bool reports `issue` and gives `false`.
    fn lista_bool_element(
        &mut self,
        ty: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        if self.is_error(resolved) {
            return false;
        }
        if self.is_infer(resolved) {
            let bivalens = self.bool_type();
            self.unify_assign_issue(ty, bivalens, span, issue);
            return true;
        }
        if self.is_bool(ty) {
            return true;
        }
        self.invalid_operand_types(issue, span);
        false
    }

    /// `≈ ≉` over a list, set, map, queue, stack or ledger that reached here
    /// is not a numeric tensor (numeric lists, tensors, registers and sparsa
    /// answered earlier): the author gets a named error at check time, not
    /// an `invalid MIR` failure at run time. `None` means no aggregate operand.
    pub(in crate::passes::typecheck) fn check_approx_aggregate(
        &mut self,
        op: HirBinOp,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let kind = [lhs_ty, rhs_ty]
            .into_iter()
            .find_map(|ty| self.approx_aggregate_kind(ty))?;
        self.invalid_operand_types_with_args(
            ISSUE_APPROX_REQUIRES_NUMERIC_OPERANDS,
            span,
            &[
                ("op", approx_glyph(op).to_owned()),
                ("kind", kind.to_owned()),
            ],
        );
        Some(self.error_type)
    }

    fn approx_aggregate_kind(&mut self, ty: TypeId) -> Option<&'static str> {
        let resolved = self.value_carrier_type(ty);
        let (_, payload) = self.types.peel_access_marker(resolved);
        match self.types.get(payload).clone() {
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. } => Some("list"),
            Type::Queue(_) | Type::BoundedQueue { .. } => Some("queue"),
            Type::Stack(_) | Type::BoundedStack { .. } => Some("stack"),
            Type::Set(_) => Some("set"),
            Type::Map(..) => Some("map"),
            Type::Ledger(..) => Some("ledger"),
            Type::Alias(_, target) => self.approx_aggregate_kind(target),
            _ => None,
        }
    }

    fn lista_arithmetic_shape(&mut self, ty: TypeId) -> Option<ListaArithmeticShape> {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved).clone() {
            Type::Array(element) => Some(ListaArithmeticShape {
                element,
                capacity: None,
            }),
            Type::BoundedArray { element, capacity } => Some(ListaArithmeticShape {
                element,
                capacity: Some(capacity),
            }),
            Type::Alias(_, target) => self.lista_arithmetic_shape(target),
            _ => None,
        }
    }

    /// The element type of a `lista` operand, `None` for any other type.
    pub(super) fn lista_element_type(&mut self, ty: TypeId) -> Option<TypeId> {
        self.lista_arithmetic_shape(ty).map(|shape| shape.element)
    }

    /// The literal-or-symbolic capacity of a bounded `lista` operand.
    pub(super) fn lista_capacity(&mut self, ty: TypeId) -> Option<radix_types::IndexId> {
        self.lista_arithmetic_shape(ty)
            .and_then(|shape| shape.capacity)
    }

    fn is_numeric_lista_element(&self, ty: TypeId) -> bool {
        self.numeric_shape(ty).is_some()
    }

    fn lista_result(&mut self, element: TypeId, capacity: Option<radix_types::IndexId>) -> TypeId {
        match capacity {
            Some(capacity) => self.types.bounded_array(element, capacity),
            None => self.types.array(element),
        }
    }

    /// The capacity of a list pair's result. Two literal capacities that
    /// differ are different types (D4): a named check-time error, `None`.
    fn lista_pair_capacity(
        &mut self,
        left: ListaArithmeticShape,
        right: ListaArithmeticShape,
        span: radix_lexer::Span,
        op: HirBinOp,
    ) -> Option<Option<radix_types::IndexId>> {
        if let (Some(left_capacity), Some(right_capacity)) = (left.capacity, right.capacity) {
            let left_capacity = self.resolve_index(left_capacity);
            let right_capacity = self.resolve_index(right_capacity);
            if let (IndexExpr::Literal(left_n), IndexExpr::Literal(right_n)) = (
                self.types.get_index(left_capacity).clone(),
                self.types.get_index(right_capacity).clone(),
            ) && !self.types.index_equals(left_capacity, right_capacity)
            {
                self.invalid_operand_types_with_args(
                    ISSUE_LISTA_CAPACITY_MISMATCH,
                    span,
                    &[
                        ("operator", lista_operator_spelling(op).to_owned()),
                        ("lhs", left_n.to_string()),
                        ("rhs", right_n.to_string()),
                    ],
                );
                return None;
            }
        }
        Some(left.capacity)
    }

    /// The element of two typed lists: one numeric type on both sides. `÷`
    /// takes its width from ruling 23. `None` is the list law error.
    fn lista_pair_element(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        left: TypeId,
        right: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !self.is_numeric_lista_element(left)
            || !self.is_numeric_lista_element(right)
            || !self.numeric_shapes_match(left, right)
        {
            return None;
        }
        if op != HirBinOp::TrueDiv {
            return Some(left);
        }
        if self.lista_decimal_true_div(left, right, span) {
            return Some(self.error_type);
        }
        Some(self.true_division_type(lhs, rhs, left, right, span))
    }

    /// The element of a list against a scalar: the F9 join of the list
    /// element and the scalar (a constant joins by value, an untyped literal
    /// has already adopted the element type), `÷` by ruling 23.
    fn lista_scalar_element(
        &mut self,
        op: HirBinOp,
        (lhs, lhs_ty): (&HirExpression, TypeId),
        (rhs, rhs_ty): (&HirExpression, TypeId),
        element: TypeId,
        list_first: bool,
    ) -> TypeId {
        let span = lhs.span;
        let element = self.resolve_inferred_type(element);
        let (left, right) = if list_first {
            ((element, None), (rhs_ty, const_int_value(rhs)))
        } else {
            ((lhs_ty, const_int_value(lhs)), (element, None))
        };
        if op == HirBinOp::TrueDiv {
            return self.true_division_type(lhs, rhs, left.0, right.0, span);
        }
        self.lift_join_element(left, right, span)
    }

    /// `÷` over a decimal element is rejected: the runner's true division
    /// converts carriers to floats and would drop the scale.
    fn lista_decimal_true_div(
        &mut self,
        left: TypeId,
        right: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        let decimal = |checker: &Self, ty: TypeId| {
            checker
                .numeric_shape(checker.resolve_inferred_type(ty))
                .is_some_and(|shape| shape.width.is_decimal())
        };
        if decimal(self, left) || decimal(self, right) {
            self.invalid_operand_types(ISSUE_LISTA_DECIMAL_ELEMENT, span);
            return true;
        }
        false
    }
}
