//! Operator and assignment rules for HIR typechecking.
//!
//! The expression dispatcher delegates here when surface syntax has policy that
//! is narrower than general assignment compatibility: arithmetic, text
//! concatenation, equality, ordering, coalescing, boolean logic, bit operations,
//! and assignment forms. These helpers both infer operand types and enforce the
//! language's operator contracts.
//!
//! ERROR STRATEGY
//! ==============
//! Unknown operands may be unified with the strongest type implied by an
//! operator, such as `numerus` for arithmetic or `bivalens` for logical
//! negation. Concrete mismatches produce diagnostics and return the checker
//! error type where the result cannot be trusted.
//!
//! NULLABILITY
//! ===========
//! `nihil` comparisons are intentionally accepted without forcing both sides to
//! the same concrete type. Coalescing follows optional/`nihil` policy locally so
//! it can return either an optional type or the fallback's concrete type without
//! changing the global assignability relation.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

const INSTANS_ORDERING_OPERANDS_REQUIRED: &str = "instans_ordering_operands_required";
const NUMERIC_VALUE_EQUALITY_OPERANDS_REQUIRED: &str = "numeric_value_equality_operands_required";
const ISSUE_CONVERSIO_ASSIGN_ROUTE_UNSUPPORTED: &str = "conversio_assign_route_unsupported";
const ISSUE_CONVERSIO_ASSIGN_INFERENCE_HOLE_DESTINATION: &str =
    "conversio_assign_inference_hole_destination";

/// `x est T` where `T` names an imported type (DECISIONS D18.1).
const ISSUE_EST_IMPORTED_TYPE: &str = "est_imported_type";

#[cold]
fn fresh_tensor_result_missing_shape() -> ! {
    unreachable!("fresh tensor result carries a shape");
}

impl TypeChecker<'_> {
    fn is_error(&self, ty: TypeId) -> bool {
        self.resolve_inferred_type(ty) == self.error_type
    }

    /// Check numeric operands and return the widened numeric join type.
    ///
    /// This helper remains for comparison-like joins that intentionally compare
    /// numeric values after widening. Arithmetic uses
    /// [`Self::numeric_arithmetic_bin`] so explicit scalar widths are preserved.
    pub fn numeric_bin(&mut self, lhs: TypeId, rhs: TypeId, span: radix_lexer::Span) -> TypeId {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        if lhs_resolved == self.error_type || rhs_resolved == self.error_type {
            return self.error_type;
        }
        if self.is_infer(lhs_resolved) || self.is_infer(rhs_resolved) {
            let numerus = self.numerus_type();
            if self.is_infer(lhs_resolved) {
                self.unify_assign_issue(lhs, numerus, span, NUMERIC_OPERANDS_REQUIRED);
            }
            if self.is_infer(rhs_resolved) {
                self.unify_assign_issue(rhs, numerus, span, NUMERIC_OPERANDS_REQUIRED);
            }
        }
        if !self.is_numeric(lhs) || !self.is_numeric(rhs) {
            self.invalid_operand_types(NUMERIC_OPERANDS_REQUIRED, span);
            return self.error_type;
        }

        let lhs_shape = self.numeric_shape(self.resolve_inferred_type(lhs));
        let rhs_shape = self.numeric_shape(self.resolve_inferred_type(rhs));
        if lhs_shape
            .zip(rhs_shape)
            .is_some_and(|(lhs, rhs)| lhs.modular != rhs.modular)
        {
            self.invalid_operand_types(MODULAR_WORD_FAMILY_MISMATCH, span);
            return self.error_type;
        }

        if self.is_fractus(lhs) || self.is_fractus(rhs) {
            self.fractus_type()
        } else {
            self.numerus_type()
        }
    }

    fn numeric_arithmetic_bin(
        &mut self,
        lhs: TypeId,
        rhs: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        if lhs_resolved == self.error_type || rhs_resolved == self.error_type {
            return self.error_type;
        }

        if self.is_infer(lhs_resolved) && self.is_infer(rhs_resolved) {
            let numerus = self.numerus_type();
            self.unify_assign_issue(lhs, numerus, span, NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL);
            self.unify_assign_issue(rhs, numerus, span, NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL);
            return numerus;
        }
        if self.is_infer(lhs_resolved) {
            if self.numeric_shape(rhs_resolved).is_some() {
                return self.unify_assign_issue(
                    lhs,
                    rhs_resolved,
                    span,
                    NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL,
                );
            }
            self.invalid_operand_types(NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL, span);
            return self.error_type;
        }
        if self.is_infer(rhs_resolved) {
            if self.numeric_shape(lhs_resolved).is_some() {
                return self.unify_assign_issue(
                    rhs,
                    lhs_resolved,
                    span,
                    NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL,
                );
            }
            self.invalid_operand_types(NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL, span);
            return self.error_type;
        }

        let Some(lhs_shape) = self.numeric_shape(lhs_resolved) else {
            self.invalid_operand_types(NUMERIC_OPERANDS_REQUIRED, span);
            return self.error_type;
        };
        let Some(rhs_shape) = self.numeric_shape(rhs_resolved) else {
            self.invalid_operand_types(NUMERIC_OPERANDS_REQUIRED, span);
            return self.error_type;
        };

        if lhs_shape.prim != rhs_shape.prim || lhs_shape.width != rhs_shape.width {
            self.invalid_operand_types(NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL, span);
            return self.error_type;
        }
        if lhs_shape.modular != rhs_shape.modular {
            self.invalid_operand_types(MODULAR_WORD_FAMILY_MISMATCH, span);
            return self.error_type;
        }

        self.preferred_numeric_result_ty(lhs_resolved, rhs_resolved, lhs_shape)
    }

    /// Check the C5 elementwise queue/stack arithmetic law.
    ///
    /// `None` means neither operand is a queue or stack, so the scalar/list
    /// caller continues with its existing arithmetic rule. `Some` means a
    /// queue/stack pair or scalar broadcast was handled, including a
    /// deliberate error for mixed container kinds.
    fn check_queue_stack_arithmetic_bin(
        &mut self,
        op: HirBinOp,
        lhs: TypeId,
        rhs: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let lhs_collection = self.queue_stack_arithmetic_shape(lhs);
        let rhs_collection = self.queue_stack_arithmetic_shape(rhs);
        if lhs_collection.is_none() && rhs_collection.is_none() {
            return None;
        }

        let operator = lista_operator_spelling(op);
        let law_error = |checker: &mut Self, collection: &str| {
            checker.invalid_operand_types_with_args(
                NUMERIC_QUEUE_STACK_OPERATOR_LAW,
                span,
                &[
                    ("collection", collection.to_owned()),
                    ("operator", operator.to_owned()),
                ],
            );
            checker.error_type
        };

        let (kind, element, capacity) = match (lhs_collection, rhs_collection) {
            (Some(lhs), Some(rhs)) => {
                if lhs.kind != rhs.kind {
                    return Some(law_error(self, "queue/stack"));
                }
                if !self.is_numeric_queue_stack_element(lhs.element)
                    || !self.is_numeric_queue_stack_element(rhs.element)
                {
                    return Some(law_error(self, lhs.kind.spelling()));
                }
                if !self.numeric_shapes_match(lhs.element, rhs.element) {
                    return Some(law_error(self, lhs.kind.spelling()));
                }
                let Some(capacity) = self.queue_stack_min_capacity(lhs.capacity, rhs.capacity)
                else {
                    return Some(law_error(self, lhs.kind.spelling()));
                };
                (lhs.kind, lhs.element, capacity)
            }
            (Some(lhs), None) => {
                if !self.is_numeric_queue_stack_element(lhs.element) || !self.is_numeric_scalar(rhs)
                {
                    return Some(law_error(self, lhs.kind.spelling()));
                }
                if !self.numeric_shapes_match(lhs.element, rhs) {
                    return Some(law_error(self, lhs.kind.spelling()));
                }
                (lhs.kind, lhs.element, lhs.capacity)
            }
            (None, Some(rhs)) => {
                if !self.is_numeric_queue_stack_element(rhs.element) || !self.is_numeric_scalar(lhs)
                {
                    return Some(law_error(self, rhs.kind.spelling()));
                }
                if !self.numeric_shapes_match(lhs, rhs.element) {
                    return Some(law_error(self, rhs.kind.spelling()));
                }
                (rhs.kind, rhs.element, rhs.capacity)
            }
            (None, None) => unreachable!("queue/stack arithmetic arm requires a collection"),
        };

        Some(match (kind, capacity) {
            (QueueStackArithmeticKind::Queue, Some(capacity)) => {
                self.types.bounded_queue(element, capacity)
            }
            (QueueStackArithmeticKind::Queue, None) => self.types.queue(element),
            (QueueStackArithmeticKind::Stack, Some(capacity)) => {
                self.types.bounded_stack(element, capacity)
            }
            (QueueStackArithmeticKind::Stack, None) => self.types.stack(element),
        })
    }

    fn queue_stack_arithmetic_shape(&mut self, ty: TypeId) -> Option<QueueStackArithmeticShape> {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved).clone() {
            Type::Queue(element) => Some(QueueStackArithmeticShape {
                kind: QueueStackArithmeticKind::Queue,
                element,
                capacity: None,
            }),
            Type::BoundedQueue { element, capacity } => Some(QueueStackArithmeticShape {
                kind: QueueStackArithmeticKind::Queue,
                element,
                capacity: Some(capacity),
            }),
            Type::Stack(element) => Some(QueueStackArithmeticShape {
                kind: QueueStackArithmeticKind::Stack,
                element,
                capacity: None,
            }),
            Type::BoundedStack { element, capacity } => Some(QueueStackArithmeticShape {
                kind: QueueStackArithmeticKind::Stack,
                element,
                capacity: Some(capacity),
            }),
            Type::Alias(_, target) => self.queue_stack_arithmetic_shape(target),
            _ => None,
        }
    }

    fn is_numeric_queue_stack_element(&self, ty: TypeId) -> bool {
        self.numeric_shape(ty).is_some()
    }

    // WHY: outer Option = combinability, inner = derived capacity
    #[allow(clippy::option_option)]
    fn queue_stack_min_capacity(
        &mut self,
        lhs: Option<radix_types::IndexId>,
        rhs: Option<radix_types::IndexId>,
    ) -> Option<Option<radix_types::IndexId>> {
        match (lhs, rhs) {
            (None, None) => Some(None),
            (Some(capacity), None) | (None, Some(capacity)) => Some(Some(capacity)),
            (Some(lhs), Some(rhs)) if self.types.index_equals(lhs, rhs) => Some(Some(lhs)),
            (Some(lhs), Some(rhs)) => {
                let (radix_types::IndexExpr::Literal(lhs), radix_types::IndexExpr::Literal(rhs)) =
                    (self.types.get_index(lhs), self.types.get_index(rhs))
                else {
                    return None;
                };
                Some(Some(self.types.intern_index(
                    radix_types::IndexExpr::Literal((*lhs).min(*rhs)),
                )))
            }
        }
    }

    /// Check the A1 elementwise list arithmetic law.
    ///
    /// `None` means neither operand is a `lista`, so the scalar/tensor caller
    /// continues with its existing arithmetic rule. `Some` means a list pair
    /// or list/scalar broadcast was handled, including a deliberate error.
    fn check_lista_arithmetic_bin(
        &mut self,
        op: HirBinOp,
        lhs: TypeId,
        rhs: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let lhs_lista = self.lista_arithmetic_shape(lhs);
        let rhs_lista = self.lista_arithmetic_shape(rhs);
        if lhs_lista.is_none() && rhs_lista.is_none() {
            return None;
        }

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
            (Some(lhs), Some(rhs)) => {
                if !self.is_numeric_lista_element(lhs.element)
                    || !self.is_numeric_lista_element(rhs.element)
                {
                    return Some(law_error(self));
                }
                if !self.numeric_shapes_match(lhs.element, rhs.element) {
                    return Some(law_error(self));
                }
                if let (Some(lhs_capacity), Some(rhs_capacity)) = (lhs.capacity, rhs.capacity) {
                    let lhs_capacity = self.resolve_index(lhs_capacity);
                    let rhs_capacity = self.resolve_index(rhs_capacity);
                    let lhs_static =
                        matches!(self.types.get_index(lhs_capacity), IndexExpr::Literal(_));
                    let rhs_static =
                        matches!(self.types.get_index(rhs_capacity), IndexExpr::Literal(_));
                    if lhs_static
                        && rhs_static
                        && !self.types.index_equals(lhs_capacity, rhs_capacity)
                    {
                        return Some(law_error(self));
                    }
                }
                (lhs.element, lhs.capacity)
            }
            (Some(lhs), None) => {
                if !self.is_numeric_lista_element(lhs.element) || !self.is_numeric_scalar(rhs) {
                    return Some(law_error(self));
                }
                if !self.numeric_shapes_match(lhs.element, rhs) {
                    return Some(law_error(self));
                }
                (lhs.element, lhs.capacity)
            }
            (None, Some(rhs)) => {
                if !self.is_numeric_lista_element(rhs.element) || !self.is_numeric_scalar(lhs) {
                    return Some(law_error(self));
                }
                if !self.numeric_shapes_match(lhs, rhs.element) {
                    return Some(law_error(self));
                }
                (rhs.element, rhs.capacity)
            }
            (None, None) => unreachable!("list arithmetic arm requires a lista operand"),
        };

        let result = match result_capacity {
            Some(capacity) => self.types.bounded_array(result_element, capacity),
            None => self.types.array(result_element),
        };
        Some(result)
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

    fn is_numeric_lista_element(&self, ty: TypeId) -> bool {
        self.numeric_shape(ty).is_some()
    }

    fn is_numeric_scalar(&mut self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        !self.is_infer(resolved) && self.numeric_shape(resolved).is_some()
    }

    fn numeric_shapes_match(&self, lhs: TypeId, rhs: TypeId) -> bool {
        let Some(lhs) = self.numeric_shape(lhs) else {
            return false;
        };
        let Some(rhs) = self.numeric_shape(rhs) else {
            return false;
        };
        lhs.prim == rhs.prim && lhs.width == rhs.width && lhs.modular == rhs.modular
    }

    fn numeric_shape(&self, ty: TypeId) -> Option<NumericShape> {
        let (_, ty) = self.types.peel_access_marker(ty);
        match self.types.get(ty) {
            Type::Primitive(Primitive::Numerus) => Some(NumericShape {
                prim: Primitive::Numerus,
                width: NumericWidth::I64,
                modular: false,
            }),
            Type::Primitive(Primitive::Fractus) => Some(NumericShape {
                prim: Primitive::Fractus,
                width: NumericWidth::F64,
                modular: false,
            }),
            Type::SizedNumeric(prim @ (Primitive::Numerus | Primitive::Fractus), width) => {
                Some(NumericShape {
                    prim: *prim,
                    width: *width,
                    modular: false,
                })
            }
            Type::ModularWord(width) => Some(NumericShape {
                prim: Primitive::Numerus,
                width: *width,
                modular: true,
            }),
            Type::Alias(_, target) => self.numeric_shape(*target),
            _ => None,
        }
    }

    fn preferred_numeric_result_ty(
        &mut self,
        lhs: TypeId,
        rhs: TypeId,
        shape: NumericShape,
    ) -> TypeId {
        if matches!(
            self.types.get(lhs),
            Type::SizedNumeric(_, _) | Type::ModularWord(_)
        ) {
            return lhs;
        }
        if matches!(
            self.types.get(rhs),
            Type::SizedNumeric(_, _) | Type::ModularWord(_)
        ) {
            return rhs;
        }
        match shape.prim {
            Primitive::Numerus => self.numerus_type(),
            Primitive::Fractus => self.fractus_type(),
            _ => self.error_type,
        }
    }

    fn bool_bin(&mut self, lhs: TypeId, rhs: TypeId, span: radix_lexer::Span) -> TypeId {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        if lhs_resolved == self.error_type || rhs_resolved == self.error_type {
            return self.bool_type();
        }

        let bivalens = self.bool_type();
        if self.is_infer(lhs_resolved) {
            self.unify_assign_issue(lhs, bivalens, span, BOOLEAN_OPERANDS_REQUIRED);
        }
        if self.is_infer(rhs_resolved) {
            self.unify_assign_issue(rhs, bivalens, span, BOOLEAN_OPERANDS_REQUIRED);
        }
        if !self.is_bool(lhs) || !self.is_bool(rhs) {
            self.invalid_operand_types(BOOLEAN_OPERANDS_REQUIRED, span);
        }
        self.bool_type()
    }

    /// Check numeric ordering operands and choose their lossless comparison join.
    ///
    /// Decimal carriers are scaled integers, so a raw comparison is meaningful
    /// only after decimal widths join at `d64`. Decimal/integer ordering uses
    /// the same scaled `d64` join; decimal/fractus remains fail-closed because
    /// no exact cross-family carrier is established by the language contract.
    fn numeric_ordering_bin(
        &mut self,
        lhs: TypeId,
        rhs: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        let lhs_shape = self.numeric_shape(lhs_resolved);
        let rhs_shape = self.numeric_shape(rhs_resolved);
        let decimal_lhs = lhs_shape.is_some_and(|shape| shape.width.is_decimal());
        let decimal_rhs = rhs_shape.is_some_and(|shape| shape.width.is_decimal());
        let fractus_lhs = lhs_shape.is_some_and(|shape| shape.prim == Primitive::Fractus);
        let fractus_rhs = rhs_shape.is_some_and(|shape| shape.prim == Primitive::Fractus);

        if (decimal_lhs && fractus_rhs) || (decimal_rhs && fractus_lhs) {
            self.invalid_operand_types(MIXED_DECIMAL_FRACTUS_ORDERING, span);
            return self.error_type;
        }

        let joined = self.numeric_bin(lhs, rhs, span);
        if joined == self.error_type {
            return joined;
        }
        if decimal_lhs || decimal_rhs {
            return self
                .types
                .sized_numeric(Primitive::Numerus, NumericWidth::D64);
        }
        joined
    }

    /// Allow cross-precision `instans` operands for ordering only.
    ///
    /// WHY: ordering compares at the coarser declared precision; operands need
    /// not unify to one precision marker in the type table. Equality uses
    /// [`Self::exact_equality_types`] instead.
    fn instans_join(&mut self, lhs: TypeId, rhs: TypeId, span: radix_lexer::Span) -> TypeId {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        if lhs_resolved == self.error_type || rhs_resolved == self.error_type {
            return self.error_type;
        }
        if self.is_infer(lhs_resolved) || self.is_infer(rhs_resolved) {
            let bare = self.types.primitive(Primitive::Instans);
            if self.is_infer(lhs_resolved) {
                self.unify_assign_issue(lhs, bare, span, INSTANS_ORDERING_OPERANDS_REQUIRED);
            }
            if self.is_infer(rhs_resolved) {
                self.unify_assign_issue(rhs, bare, span, INSTANS_ORDERING_OPERANDS_REQUIRED);
            }
        }
        if !self.is_instans(lhs) || !self.is_instans(rhs) {
            self.invalid_operand_types(INSTANS_ORDERING_OPERANDS_REQUIRED, span);
            return self.error_type;
        }
        rhs_resolved
    }

    /// Enforce the numeric-equality admission law for `≅`/`≇` and `≈`/`≉`.
    ///
    /// THE ≅/≈ ADMISSION LAW (contract record, E1 equality-tier-respelling)
    /// ====================================================================
    /// `≅`/`≇` are promoted-exact: the operand widths join through
    /// [`Self::numeric_bin`]'s comparison-join contract (`ops.rs:44-46`), then
    /// the lowerings compare exactly at the joined representation. `≈`/`≉`
    /// apply the `math.isclose` default in that same joined representation
    /// (OQ3 settled default: `2^53 ≈ 2^53+1` verdicts verum). `≡` stays
    /// fail-closed — mixed widths are rejected by
    /// [`Self::exact_equality_types`].
    ///
    /// LOSSLESS-JOIN REQUIREMENT: the join must never wrap-identify distinct
    /// values. Named acceptance rows — the runner is the semantics of record
    /// (`values_equal`, `crates/radix-mir-runner/src/value.rs:982-1018`; the
    /// operator need ruling is BINDING): `−1 ∷ int<i64> ≅
    /// 18446744073709551615 ∷ int<u64>` and the cross-width
    /// `9007199254740993 ∷ int<i64> ≅ 9007199254740992 ∷ int<u64>` both
    /// verdict **falsum** on every surface that can compile a mixed-width
    /// `≅`. The int↔fractus f64 join stays accepted (lossy at 2^53 is the
    /// operator-accepted bound, so `2^53+1 ≅ 2^52×2` verdicts verum).
    ///
    /// WHY: exact `≡` rejects cross-width numeric pairs; both relaxed levels
    /// reuse the same widening policy as arithmetic/ordering (`numeric_bin`),
    /// then compare at the joined type in their respective lowerings.
    ///
    /// GT-2: decimal × fractus equality mirrors the ordering gate
    /// ([`Self::numeric_ordering_bin`] rejects the same family pair as
    /// `mixed_decimal_fractus_ordering`). No exact cross-family carrier is
    /// established by the language contract, so `≅`/`≈` reject the pair
    /// instead of comparing a widened scaled integer against a float.
    pub fn numeric_value_equality_types(
        &mut self,
        lhs: TypeId,
        rhs: TypeId,
        span: radix_lexer::Span,
    ) {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        if lhs_resolved == self.error_type || rhs_resolved == self.error_type {
            return;
        }

        if self.types.equals(lhs_resolved, rhs_resolved) {
            return;
        }
        if !self.is_numeric(lhs) || !self.is_numeric(rhs) {
            self.invalid_operand_types(NUMERIC_VALUE_EQUALITY_OPERANDS_REQUIRED, span);
            return;
        }
        let lhs_shape = self.numeric_shape(lhs_resolved);
        let rhs_shape = self.numeric_shape(rhs_resolved);
        let decimal_lhs = lhs_shape.is_some_and(|shape| shape.width.is_decimal());
        let decimal_rhs = rhs_shape.is_some_and(|shape| shape.width.is_decimal());
        let fractus_lhs = lhs_shape.is_some_and(|shape| shape.prim == Primitive::Fractus);
        let fractus_rhs = rhs_shape.is_some_and(|shape| shape.prim == Primitive::Fractus);
        if (decimal_lhs && fractus_rhs) || (decimal_rhs && fractus_lhs) {
            self.invalid_operand_types(MIXED_DECIMAL_FRACTUS_EQUALITY, span);
            return;
        }
        self.numeric_bin(lhs, rhs, span);
    }

    /// Enforce exact-type match for `≡`, `≠`, `est`, and `non est`.
    ///
    /// WHY: assignability may widen across representations; equality must not
    /// erase deliberate annotations (`ascii`, sized numerics, precision-tagged
    /// `instans`, …). Authors widen explicitly with `↦` before comparing.
    pub fn exact_equality_types(&mut self, lhs: TypeId, rhs: TypeId, span: radix_lexer::Span) {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        if lhs_resolved == self.error_type || rhs_resolved == self.error_type {
            return;
        }

        if let Some(var) = self.infer_var_of(lhs_resolved) {
            self.bind_infer_exact_issue(var, rhs_resolved, span, EQUALITY_EXACT_TYPES_REQUIRED);
            return;
        }
        if let Some(var) = self.infer_var_of(rhs_resolved) {
            self.bind_infer_exact_issue(var, lhs_resolved, span, EQUALITY_EXACT_TYPES_REQUIRED);
            return;
        }

        if !self.types.equals(lhs_resolved, rhs_resolved) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                EQUALITY_EXACT_TYPES_REQUIRED,
                span,
            );
        }
    }

    /// Exact equality compares the carrier of a `ref` field read, not its
    /// access-discipline view. Direct `ref` values still retain their wrapper
    /// so equality does not generally erase the view contract.
    fn exact_equality_types_for_operands(
        &mut self,
        lhs_expr: &HirExpression,
        lhs: TypeId,
        rhs_expr: &HirExpression,
        rhs: TypeId,
        span: radix_lexer::Span,
    ) {
        let lhs = self.equality_field_read_carrier(lhs_expr, lhs);
        let rhs = self.equality_field_read_carrier(rhs_expr, rhs);
        self.exact_equality_types(lhs, rhs, span);
    }

    fn equality_field_read_carrier(&mut self, expr: &HirExpression, ty: TypeId) -> TypeId {
        if !matches!(expr.kind, HirExpressionKind::Field(_, _)) {
            return ty;
        }
        let resolved = self.resolve_inferred_type(ty);
        if self.types.is_ref_view(resolved) {
            self.types.peel_access_marker(resolved).1
        } else {
            resolved
        }
    }

    fn integer_bin(&mut self, lhs: TypeId, rhs: TypeId, span: radix_lexer::Span) -> TypeId {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        if lhs_resolved == self.error_type || rhs_resolved == self.error_type {
            return self.error_type;
        }

        let numerus = self.numerus_type();
        if self.is_infer(lhs_resolved) && self.is_infer(rhs_resolved) {
            self.unify_assign_issue(lhs, numerus, span, INTEGER_OPERANDS_REQUIRED);
            self.unify_assign_issue(rhs, numerus, span, INTEGER_OPERANDS_REQUIRED);
            return numerus;
        }
        if self.is_infer(lhs_resolved) {
            if self.integer_shape(rhs_resolved).is_some() {
                return self.unify_assign_issue(lhs, rhs_resolved, span, INTEGER_OPERANDS_REQUIRED);
            }
            self.invalid_operand_types(INTEGER_OPERANDS_REQUIRED, span);
            return self.error_type;
        }
        if self.is_infer(rhs_resolved) {
            if self.integer_shape(lhs_resolved).is_some() {
                return self.unify_assign_issue(rhs, lhs_resolved, span, INTEGER_OPERANDS_REQUIRED);
            }
            self.invalid_operand_types(INTEGER_OPERANDS_REQUIRED, span);
            return self.error_type;
        }

        let Some(lhs_shape) = self.integer_shape(lhs_resolved) else {
            self.invalid_operand_types(INTEGER_OPERANDS_REQUIRED, span);
            return self.error_type;
        };
        let Some(rhs_shape) = self.integer_shape(rhs_resolved) else {
            self.invalid_operand_types(INTEGER_OPERANDS_REQUIRED, span);
            return self.error_type;
        };

        if lhs_shape.width != rhs_shape.width {
            self.invalid_operand_types(SAME_WIDTH_INTEGER_OPERANDS_REQUIRED, span);
            return self.error_type;
        }
        if lhs_shape.modular != rhs_shape.modular {
            self.invalid_operand_types(MODULAR_WORD_FAMILY_MISMATCH, span);
            return self.error_type;
        }

        self.preferred_numeric_result_ty(lhs_resolved, rhs_resolved, lhs_shape)
    }

    fn integer_shape(&self, ty: TypeId) -> Option<NumericShape> {
        let shape = self.numeric_shape(ty)?;
        (shape.prim == Primitive::Numerus).then_some(shape)
    }

    /// Choose a shared result type for branch, match, and coalescing flows.
    ///
    /// The direction matters: if one side can be assigned to the other, the
    /// destination type is preserved. Numeric peers use normal arithmetic
    /// widening. All other incompatible flows are reported as type mismatches.
    pub fn common_type(&mut self, a: TypeId, b: TypeId, span: radix_lexer::Span) -> TypeId {
        let left = self.resolve_inferred_type(a);
        let right = self.resolve_inferred_type(b);

        // U-04: union-hole join. When either side of a branch/array/match join
        // is a `∪` hole, the join is an accumulation: the other side
        // contributes a member and the hole stays open (finalize solves it).
        // Monomorphic and concrete expectations never widen — the body below
        // is byte-for-byte today's behavior.
        if self.infer_union_var_of(left).is_some() || self.infer_union_var_of(right).is_some() {
            return self.unify_join(a, b, span, COMMON_TYPE_MISMATCH);
        }

        // F-009: Option preservation — when one side is Option<T> and the other
        // side (T or nihil) can be lifted into it, the join must produce
        // Option<T>, not unwrap to T.
        if let Type::Option(_) = self.types.get(left)
            && self.types.is_assignable(right, left)
        {
            return left;
        }
        if let Type::Option(_) = self.types.get(right)
            && self.types.is_assignable(left, right)
        {
            return right;
        }

        if self.types.is_assignable(left, right) {
            return right;
        }
        if self.types.is_assignable(right, left) {
            return left;
        }
        if self.is_numeric(left) && self.is_numeric(right) {
            return self.numeric_bin(left, right, span);
        }
        self.error_issue(SemanticErrorKind::TypeMismatch, COMMON_TYPE_MISMATCH, span);
        self.error_type
    }

    /// Check postfix increment/decrement (`place ↑` / `place ↓`).
    pub fn check_inc_dec(
        &mut self,
        _op: radix_hir::HirIncDecOp,
        target: &mut HirExpression,
        span: radix_lexer::Span,
    ) {
        self.invalidate_assignment_target(target);
        let target_ty = self.check_lvalue(target);
        if !self.is_integer(target_ty) {
            self.invalid_operand_types(POSTFIX_INC_DEC_NUMERUS_OPERAND, span);
        }
    }

    /// Check simple assignment and constrain the value from the target context.
    pub fn check_assign(
        &mut self,
        target: &mut HirExpression,
        value: &mut HirExpression,
    ) -> TypeId {
        self.invalidate_assignment_target(target);
        let target_ty = self.check_lvalue(target);
        if target_ty == self.error_type {
            self.check_expr(value);
            return self.error_type;
        }
        if self.is_shader_vec3_to_vec4_output_target(target, target_ty) {
            let value_ty = self.check_expr(value);
            if self.is_f32_vector_width(value_ty, 3) {
                return target_ty;
            }
            self.unify_assign_issue(value_ty, target_ty, value.span, ASSIGNMENT_TYPE_MISMATCH);
            return target_ty;
        }
        let value_ty = self.check_expr_with_expected(value, Some(target_ty));
        self.unify_carrier_assign(value_ty, target_ty, value.span, ASSIGNMENT_TYPE_MISMATCH);
        if self.types.is_own_view(target_ty) {
            self.check_own_bind_rhs(value);
        }
        target_ty
    }

    /// Check conversion-directed assignment (`target ↤ value`, U+21A4).
    ///
    /// The left side goes through the ordinary lvalue rules: mutable place
    /// with a resolved static type, no inference from the right value. The
    /// right side is checked through the **same conversion registry** as an
    /// explicit `value ↦ TargetType` ([`Self::check_conversio`], the
    /// `convert.rs` deny-list + allow-list) using the place's resolved type as
    /// the destination. Inline `⊥` default is typed before the store and must
    /// satisfy the same target-type rule as explicit `↦` recovery. The result
    /// type is the destination type (the stored value's type), matching
    /// ordinary assignment.
    pub fn check_conversio_assign(
        &mut self,
        target: &mut HirExpression,
        source: &mut HirExpression,
        recovery: Option<&mut HirExpression>,
        span: radix_lexer::Span,
    ) -> TypeId {
        self.invalidate_assignment_target(target);
        let target_ty = self.check_lvalue(target);
        if target_ty == self.error_type {
            self.check_expr(source);
            return self.error_type;
        }
        // The destination type must resolve before conversion checking: an
        // inference-hole destination (e.g. `fixum _ xs ← []`) cannot name a
        // conversion target.
        let target_resolved = self.resolve_inferred_type(target_ty);
        if self.destination_type_has_inference_hole(target_resolved) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_CONVERSIO_ASSIGN_INFERENCE_HOLE_DESTINATION,
                target.span,
            );
            self.check_expr(source);
            return self.error_type;
        }
        let mut conversion_target = HirConversioTarget::Type(target_ty);
        let mut route = None;
        let result_ty = self.check_conversio(
            source,
            &mut conversion_target,
            &[],
            recovery,
            &mut route,
            span,
        );
        // `↤` carries no conversion row to its backends yet: an `ordo` row
        // fails closed here instead of reaching emission unrouted. Write
        // `target ← source ↦ T` instead.
        // A registered `@ conversio` row keeps its pre-D9.3 `↤` typecheck
        // admission (CTR-02 channel composition).
        if matches!(route, Some(radix_hir::HirConversioRoute::Ordo { .. }))
            && result_ty != self.error_type
        {
            self.invalid_conversion(ISSUE_CONVERSIO_ASSIGN_ROUTE_UNSUPPORTED, span);
            return self.error_type;
        }
        target_ty
    }

    /// Whether a destination type still contains an unresolved inference hole.
    ///
    /// The destination of a `↤` must be a concrete conversion target. A bare
    /// top-level `Infer` is covered by [`Self::is_infer`]; this walks the
    /// container shapes so `lista<_>` / `tabula<_, _>` element holes are
    /// caught too.
    fn destination_type_has_inference_hole(&self, ty: TypeId) -> bool {
        let mut seen = FxHashSet::default();
        self.destination_type_has_inference_hole_inner(ty, &mut seen)
    }

    fn destination_type_has_inference_hole_inner(
        &self,
        ty: TypeId,
        seen: &mut FxHashSet<TypeId>,
    ) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        if !seen.insert(resolved) {
            return false;
        }
        match self.types.get(resolved) {
            Type::Infer(_) => true,
            Type::Array(inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Matrix(inner, _)
            | Type::Vector(inner, _)
            | Type::Atomic(inner)
            | Type::Intervallum(inner)
            | Type::Option(inner) => self.destination_type_has_inference_hole_inner(*inner, seen),
            Type::Map(key, value) => {
                self.destination_type_has_inference_hole_inner(*key, seen)
                    || self.destination_type_has_inference_hole_inner(*value, seen)
            }
            Type::Ref(_, inner) | Type::Alias(_, inner) => {
                self.destination_type_has_inference_hole_inner(*inner, seen)
            }
            Type::Union(types) => types
                .iter()
                .any(|member| self.destination_type_has_inference_hole_inner(*member, seen)),
            Type::Tuple(types) => types
                .iter()
                .any(|member| self.destination_type_has_inference_hole_inner(*member, seen)),
            Type::Applied(base, args) => {
                self.destination_type_has_inference_hole_inner(*base, seen)
                    || args.iter().any(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => {
                            self.destination_type_has_inference_hole_inner(*id, seen)
                        }
                        radix_types::AppliedArg::Size(_) => false,
                    })
            }
            _ => false,
        }
    }

    fn is_shader_vec3_to_vec4_output_target(
        &self,
        target: &HirExpression,
        target_ty: TypeId,
    ) -> bool {
        self.current_shader_function
            && matches!(target.kind, HirExpressionKind::Path(_))
            && self.is_f32_vector_width(target_ty, 4)
    }

    fn is_f32_vector_width(&self, ty: TypeId, width: u64) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        let Some(f32_id) = self
            .types
            .find_sized_numeric(Primitive::Fractus, NumericWidth::F32)
        else {
            return false;
        };
        matches!(
            self.types.get(resolved),
            Type::Vector(elem, index) if *elem == f32_id && matches!(self.types.get_index(*index), IndexExpr::Literal(n) if *n == width)
        )
    }

    /// Check unary operators and return the operator result type.
    ///
    /// `Neg` and `BitNot` preserve the operand's width, so the expected type
    /// propagates to the operand to let bare literals infer the correct width
    /// (e.g., `-1.0` in an `f32` context). `Not` returns `bivalens` regardless
    /// of context, so its operand is checked without an expected type.
    pub fn check_unary(
        &mut self,
        op: radix_hir::HirUnOp,
        operand: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        match op {
            radix_hir::HirUnOp::Neg => {
                let operand_ty = self.check_expr_with_expected(operand, expected);
                if self.is_error(operand_ty) {
                    return self.error_type;
                }
                if self.is_infer(self.resolve_inferred_type(operand_ty)) {
                    let numerus = self.numerus_type();
                    return self.unify_assign_issue(
                        operand_ty,
                        numerus,
                        operand.span,
                        NUMERIC_OPERAND_REQUIRED,
                    );
                }
                if !self.is_numeric(operand_ty) {
                    self.invalid_operand_types(NUMERIC_OPERAND_REQUIRED, operand.span);
                }
                operand_ty
            }
            radix_hir::HirUnOp::Not => {
                let operand_ty = self.check_expr(operand);
                if self.is_error(operand_ty) {
                    return self.bool_type();
                }
                if self.is_infer(self.resolve_inferred_type(operand_ty)) {
                    let bivalens = self.bool_type();
                    self.unify_assign_issue(
                        operand_ty,
                        bivalens,
                        operand.span,
                        BOOLEAN_OPERAND_REQUIRED,
                    );
                    return self.bool_type();
                }
                if !self.is_bool(operand_ty) {
                    self.invalid_operand_types(BOOLEAN_OPERAND_REQUIRED, operand.span);
                }
                self.bool_type()
            }
            radix_hir::HirUnOp::BitNot => {
                let operand_ty = self.check_expr_with_expected(operand, expected);
                if self.is_error(operand_ty) {
                    return self.error_type;
                }
                let operand_resolved = self.resolve_inferred_type(operand_ty);
                if self.is_infer(operand_resolved) {
                    let numerus = self.numerus_type();
                    self.unify_assign_issue(
                        operand_ty,
                        numerus,
                        operand.span,
                        INTEGER_OPERAND_REQUIRED,
                    );
                    return self.numerus_type();
                }
                if self.integer_shape(operand_resolved).is_none() {
                    self.invalid_operand_types(INTEGER_OPERAND_REQUIRED, operand.span);
                    return self.error_type;
                }
                operand_resolved
            }
        }
    }

    fn check_arithmetic_operands(
        &mut self,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> (TypeId, TypeId) {
        match (numeric_literal_family(lhs), numeric_literal_family(rhs)) {
            (Some(_), Some(_)) => {
                let lhs_ty = self
                    .check_expr_with_expected(lhs, self.literal_numeric_expected(lhs, expected));
                let rhs_expected = self
                    .literal_numeric_expected_from_peer(rhs, lhs_ty)
                    .or_else(|| self.literal_numeric_expected(rhs, expected));
                let rhs_ty = self.check_expr_with_expected(rhs, rhs_expected);
                (lhs_ty, rhs_ty)
            }
            (Some(_), None) => {
                let rhs_ty = self.check_expr(rhs);
                let lhs_expected = self
                    .literal_numeric_expected_from_peer(lhs, rhs_ty)
                    .or_else(|| self.literal_numeric_expected(lhs, expected));
                let lhs_ty = self.check_expr_with_expected(lhs, lhs_expected);
                (lhs_ty, rhs_ty)
            }
            (None, Some(_)) => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_expected = self
                    .literal_numeric_expected_from_peer(rhs, lhs_ty)
                    .or_else(|| self.literal_numeric_expected(rhs, expected));
                let rhs_ty = self.check_expr_with_expected(rhs, rhs_expected);
                (lhs_ty, rhs_ty)
            }
            (None, None) => (self.check_expr(lhs), self.check_expr(rhs)),
        }
    }

    fn literal_numeric_expected(
        &self,
        expr: &HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let family = numeric_literal_family(expr)?;
        let expected = self.resolve_inferred_type(expected?);
        self.numeric_shape(expected)
            .filter(|shape| shape.prim == family)
            .map(|_| self.numeric_expected_type(expected))
    }

    fn literal_numeric_expected_from_peer(
        &self,
        expr: &HirExpression,
        peer: TypeId,
    ) -> Option<TypeId> {
        let family = numeric_literal_family(expr)?;
        let peer = self.resolve_inferred_type(peer);
        self.numeric_shape(peer)
            .filter(|shape| shape.prim == family)
            .map(|_| self.numeric_expected_type(peer))
    }

    fn numeric_expected_type(&self, ty: TypeId) -> TypeId {
        match self.types.get(ty) {
            Type::Alias(_, target) => self.numeric_expected_type(*target),
            _ => ty,
        }
    }

    /// Check binary operators and return the expression result type.
    ///
    /// This is where call/access-independent control-flow values are shaped:
    /// comparisons become `bivalens`, arithmetic preserves exact numeric widths,
    /// text `+` requires both operands to be `textus`, and coalescing encodes the local
    /// optional/`nihil` result contract.
    pub fn check_binary(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        if matches!(op, HirBinOp::Coalesce) {
            return self.check_coalesce(lhs, rhs);
        }

        match op {
            HirBinOp::Add => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                let lhs_text = self.is_textus(lhs_ty) || self.is_ascii(lhs_ty);
                let rhs_text = self.is_textus(rhs_ty) || self.is_ascii(rhs_ty);
                if self.is_infer(self.resolve_inferred_type(lhs_ty)) && rhs_text {
                    let textus = self.textus_type();
                    self.unify_assign_issue(lhs_ty, textus, lhs.span, STRING_OPERANDS_REQUIRED);
                }
                if self.is_infer(self.resolve_inferred_type(rhs_ty)) && lhs_text {
                    let textus = self.textus_type();
                    self.unify_assign_issue(rhs_ty, textus, rhs.span, STRING_OPERANDS_REQUIRED);
                }
                if lhs_text || rhs_text {
                    if !lhs_text || !rhs_text {
                        self.invalid_operand_types(STRING_OPERANDS_REQUIRED, lhs.span);
                        return self.error_type;
                    }
                    self.textus_type()
                } else if let Some(result) =
                    self.check_queue_stack_arithmetic_bin(HirBinOp::Add, lhs_ty, rhs_ty, lhs.span)
                {
                    result
                } else if let Some(result) =
                    self.check_lista_arithmetic_bin(HirBinOp::Add, lhs_ty, rhs_ty, lhs.span)
                {
                    result
                } else if let Some(result) = self.check_addita_plus(lhs_ty, rhs_ty, lhs.span) {
                    // Numeric tensor/vector/matrix `+` is the live `addita` / `.added()` op.
                    result
                } else {
                    self.numeric_arithmetic_bin(lhs_ty, rhs_ty, lhs.span)
                }
            }
            HirBinOp::Sub => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                if let Some(result) =
                    self.check_queue_stack_arithmetic_bin(HirBinOp::Sub, lhs_ty, rhs_ty, lhs.span)
                {
                    result
                } else if let Some(result) =
                    self.check_lista_arithmetic_bin(HirBinOp::Sub, lhs_ty, rhs_ty, lhs.span)
                {
                    result
                } else if let Some(result) = self.check_addita_plus(lhs_ty, rhs_ty, lhs.span) {
                    // Numeric tensor/vector/matrix `-` is the live `subtrahe` / `.subtract()` op.
                    // Same-shape / tensor-broadcast contract as `+` — no extra broadcast.
                    result
                } else {
                    self.numeric_arithmetic_bin(lhs_ty, rhs_ty, lhs.span)
                }
            }
            HirBinOp::Mul | HirBinOp::Div => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                if let Some(result) =
                    self.check_queue_stack_arithmetic_bin(op, lhs_ty, rhs_ty, lhs.span)
                {
                    result
                } else if let Some(result) =
                    self.check_lista_arithmetic_bin(op, lhs_ty, rhs_ty, lhs.span)
                {
                    result
                } else {
                    self.numeric_arithmetic_bin(lhs_ty, rhs_ty, lhs.span)
                }
            }
            HirBinOp::Mod => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                self.numeric_arithmetic_bin(lhs_ty, rhs_ty, lhs.span)
            }
            HirBinOp::ApproxEq
            | HirBinOp::ApproxNotEq
            | HirBinOp::FuzzyEq
            | HirBinOp::FuzzyNotEq => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                self.numeric_value_equality_types(lhs_ty, rhs_ty, lhs.span);
                self.bool_type()
            }
            HirBinOp::Eq
            | HirBinOp::NotEq
            | HirBinOp::StrictEq
            | HirBinOp::StrictNotEq
            | HirBinOp::Is
            | HirBinOp::IsNot => {
                if matches!(op, HirBinOp::Is | HirBinOp::IsNot)
                    && let Some(type_name) = self.imported_type_est_target(rhs)
                {
                    self.check_expr(lhs);
                    let type_name = self.member_display_name(type_name);
                    self.invalid_operand_types_with_args(
                        ISSUE_EST_IMPORTED_TYPE,
                        rhs.span,
                        &[("type", type_name)],
                    );
                    return self.bool_type();
                }
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                let lhs_is_nil = matches!(
                    self.types.get(self.resolve_inferred_type(lhs_ty)),
                    Type::Primitive(Primitive::Nihil)
                );
                let rhs_is_nil = matches!(
                    self.types.get(self.resolve_inferred_type(rhs_ty)),
                    Type::Primitive(Primitive::Nihil)
                );
                if !(lhs_is_nil || rhs_is_nil) {
                    self.exact_equality_types_for_operands(lhs, lhs_ty, rhs, rhs_ty, lhs.span);
                }
                self.bool_type()
            }
            HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                if self.is_instans(lhs_ty) && self.is_instans(rhs_ty) {
                    self.instans_join(lhs_ty, rhs_ty, lhs.span);
                } else if self.is_textus(lhs_ty) && self.is_textus(rhs_ty) {
                    // D1.6: whole-string `textus` order is Unicode code-point order.
                } else if self.scalar_ordered_text_bin(lhs, rhs, lhs_ty, rhs_ty, lhs.span) {
                    // Single-scalar Unicode order (Seam B); not lexicographic cord compare.
                } else {
                    self.numeric_ordering_bin(lhs_ty, rhs_ty, lhs.span);
                }
                self.bool_type()
            }
            HirBinOp::InRange => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                self.check_intra_operand(lhs_ty, rhs_ty, lhs.span, rhs.span);
                self.bool_type()
            }
            HirBinOp::Between => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                match self.types.get(self.resolve_inferred_type(rhs_ty)) {
                    Type::Array(elem) | Type::Set(elem) => {
                        self.unify_assign_issue(
                            lhs_ty,
                            *elem,
                            lhs.span,
                            MEMBERSHIP_OPERAND_TYPE_MISMATCH,
                        );
                    }
                    Type::Map(key, _) => {
                        self.unify_assign_issue(
                            lhs_ty,
                            *key,
                            lhs.span,
                            MEMBERSHIP_OPERAND_TYPE_MISMATCH,
                        );
                    }
                    _ => {}
                }
                self.bool_type()
            }
            HirBinOp::Coalesce => unreachable!("coalesce is handled before eager rhs checking"),
            HirBinOp::And | HirBinOp::Or => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                self.bool_bin(lhs_ty, rhs_ty, lhs.span)
            }
            HirBinOp::BitAnd
            | HirBinOp::BitOr
            | HirBinOp::BitXor
            | HirBinOp::Shl
            | HirBinOp::Shr => {
                let (lhs_ty, rhs_ty) = self.check_integer_operands(lhs, rhs);
                self.integer_bin(lhs_ty, rhs_ty, lhs.span)
            }
            // TENSOR-GLYPHS-1B rank rules: the glyph products dispatch on the
            // built-in static-shape families. Operand shapes that are not
            // statically known (shape-generic widths, inference holes) fail
            // closed — the library-generic overload surface is deferred to
            // Phase 2 and must never silently pass as scalar arithmetic.
            HirBinOp::Dot => self.check_glyph_dot(op, lhs, rhs),
            HirBinOp::Cross => self.check_glyph_cross(op, lhs, rhs),
            HirBinOp::TensorProduct => self.check_glyph_tensor_product(op, lhs, rhs),
            HirBinOp::Hadamard | HirBinOp::HadamardDiv => self.check_glyph_hadamard(op, lhs, rhs),
        }
    }

    /// `·` inner product: contracts the inner dimension(s). vec·vec → scalar;
    /// matrix·matrix and the two matvec forms keep the pinned rank-1 result;
    /// rank-2 tensor·tensor contracts through the matmul shape contract.
    fn check_glyph_dot(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let Some((lhs_resolved, rhs_resolved)) = self.glyph_operand_types(op, lhs, rhs) else {
            return self.error_type;
        };
        match (
            self.types.get(lhs_resolved).clone(),
            self.types.get(rhs_resolved).clone(),
        ) {
            (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span)
                else {
                    return self.error_type;
                };
                let Some((lhs_n, rhs_n)) =
                    self.glyph_vector_widths(op, lhs_width, rhs_width, lhs.span)
                else {
                    return self.error_type;
                };
                if lhs_n != rhs_n {
                    self.glyph_invalid_operand(op, GLYPH_INNER_DIM_MISMATCH, lhs.span);
                    return self.error_type;
                }
                elem
            }
            (Type::Tensor(lhs_elem, _), Type::Tensor(rhs_elem, _)) => {
                let Some(elem) =
                    self.glyph_matmul_numeric_element(op, lhs_elem, rhs_elem, lhs.span)
                else {
                    return self.error_type;
                };
                // WHY: rank-2 receiver × rank-1 tensor is the GEMV promotion
                // arm. It routes through the same matmul contract as the
                // `matmul` intrinsic; the contract promotes the RHS to [K, 1]
                // for dispatch and demotes the result back to rank 1. Whole
                // shapes, rank-1 receivers, and higher ranks still fail closed
                // with the contract diagnostics.
                let result = super::tensor_index::tensor_with_fresh_shape(self, elem);
                let result_shape = match self.types.get(result) {
                    Type::Tensor(_, shape) => *shape,
                    _ => fresh_tensor_result_missing_shape(),
                };
                super::tensor_index::constrain_glyph_matmul_result_shape(
                    self,
                    lhs_resolved,
                    rhs_resolved,
                    result_shape,
                    lhs.span,
                );
                result
            }
            (Type::Matrix(lhs_elem, lhs_shape), Type::Matrix(rhs_elem, rhs_shape)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span)
                else {
                    return self.error_type;
                };
                let (Some((lhs_m, lhs_k)), Some((rhs_k, rhs_p))) = (
                    crate::literal_matrix_shape(lhs_shape, self.types),
                    crate::literal_matrix_shape(rhs_shape, self.types),
                ) else {
                    // S0-G7: glyph-static but parametric operands
                    // (`matrix<f32, [R, K]> · matrix<f32, [K, C]>` in a
                    // size-generic body) check symbolically instead of
                    // demanding literal shapes: the inner dimensions must be
                    // the same index term, and the result keeps the operand
                    // outer dimensions.
                    if let (Some((lhs_rows, lhs_k)), Some((rhs_k, rhs_cols))) = (
                        self.matrix_shape_dims(lhs_shape),
                        self.matrix_shape_dims(rhs_shape),
                    ) && self.types.get_index(lhs_k) == self.types.get_index(rhs_k)
                    {
                        let shape = self
                            .types
                            .intern_index(IndexExpr::Tuple(vec![lhs_rows, rhs_cols]));
                        return self.types.matrix_with_shape(elem, shape);
                    }
                    self.glyph_invalid_operand(op, GLYPH_INNER_DIM_MISMATCH, lhs.span);
                    return self.error_type;
                };
                if lhs_k != rhs_k {
                    self.glyph_invalid_operand(op, GLYPH_INNER_DIM_MISMATCH, lhs.span);
                    return self.error_type;
                }
                self.glyph_matrix_type(elem, lhs_m, rhs_p)
            }
            (Type::Matrix(lhs_elem, lhs_shape), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span)
                else {
                    return self.error_type;
                };
                let Some((lhs_m, lhs_k)) = crate::literal_matrix_shape(lhs_shape, self.types)
                else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                let Some(rhs_n) = crate::literal_vector_width(rhs_width, self.types) else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                if lhs_k != rhs_n {
                    self.glyph_invalid_operand(op, GLYPH_INNER_DIM_MISMATCH, lhs.span);
                    return self.error_type;
                }
                self.glyph_vector_type(elem, lhs_m)
            }
            (Type::Vector(lhs_elem, lhs_width), Type::Matrix(rhs_elem, rhs_shape)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span)
                else {
                    return self.error_type;
                };
                let Some(lhs_n) = crate::literal_vector_width(lhs_width, self.types) else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                let Some((rhs_k, rhs_p)) = crate::literal_matrix_shape(rhs_shape, self.types)
                else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                if lhs_n != rhs_k {
                    self.glyph_invalid_operand(op, GLYPH_INNER_DIM_MISMATCH, lhs.span);
                    return self.error_type;
                }
                self.glyph_vector_type(elem, rhs_p)
            }
            _ => {
                self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
                self.error_type
            }
        }
    }

    /// `×` cross product: 3-vectors only (width-3 pin), same shape result.
    fn check_glyph_cross(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let Some((lhs_resolved, rhs_resolved)) = self.glyph_operand_types(op, lhs, rhs) else {
            return self.error_type;
        };
        match (
            self.types.get(lhs_resolved).clone(),
            self.types.get(rhs_resolved).clone(),
        ) {
            (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(_) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_n, rhs_n)) =
                    self.glyph_vector_widths(op, lhs_width, rhs_width, lhs.span)
                else {
                    return self.error_type;
                };
                if lhs_n != rhs_n {
                    self.glyph_invalid_operand(op, GLYPH_CROSS_WIDTH_MISMATCH, lhs.span);
                    return self.error_type;
                }
                if lhs_n != 3 {
                    self.glyph_invalid_operand(op, GLYPH_CROSS_WIDTH_THREE_REQUIRED, lhs.span);
                    return self.error_type;
                }
                lhs_resolved
            }
            _ => {
                self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
                self.error_type
            }
        }
    }

    /// `⊗` outer product: vector⊗vector → matrix (rank sum). Kronecker on
    /// matrices/tensors is deferred to Phase 2 and fails closed here.
    fn check_glyph_tensor_product(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let Some((lhs_resolved, rhs_resolved)) = self.glyph_operand_types(op, lhs, rhs) else {
            return self.error_type;
        };
        match (
            self.types.get(lhs_resolved).clone(),
            self.types.get(rhs_resolved).clone(),
        ) {
            (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span)
                else {
                    return self.error_type;
                };
                let (lhs_lit, rhs_lit) = (
                    crate::literal_vector_width(lhs_width, self.types),
                    crate::literal_vector_width(rhs_width, self.types),
                );
                if let (Some(lhs_n), Some(rhs_m)) = (lhs_lit, rhs_lit) {
                    return self.glyph_matrix_type(elem, lhs_n, rhs_m);
                }
                // S0-G8: glyph-static but parametric widths keep the outer
                // product symbolic — `vector<f32, N> ⊗ vector<f32, 1>` is
                // `matrix<f32, [N, 1]>` instead of demanding literals.
                if let Some(shape) = self.glyph_symbolic_outer_shape(lhs_width, rhs_width) {
                    return self.types.matrix_with_shape(elem, shape);
                }
                self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                self.error_type
            }
            _ => {
                self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
                self.error_type
            }
        }
    }

    /// Numeric `+`/`-` on tensor / vector / matrix: live `addita` / `subtrahe`.
    ///
    /// One arm covers the three families for both glyphs. Tensor broadcast is
    /// `addita`'s `Figura'` (`constrain_elementwise_result_shape`); minus
    /// shares that exact contract. Vector/matrix follow the live same-shape
    /// contracts. `None` means this is not an addita-family pair — scalar
    /// arithmetic continues.
    fn check_addita_plus(
        &mut self,
        lhs: TypeId,
        rhs: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        match (
            self.types.get(lhs_resolved).clone(),
            self.types.get(rhs_resolved).clone(),
        ) {
            (Type::Tensor(lhs_elem, _), Type::Tensor(rhs_elem, _)) => Some(
                self.check_tensor_addita_plus(lhs_resolved, rhs_resolved, lhs_elem, rhs_elem, span),
            ),
            (Type::Vector(lhs_elem, _), Type::Vector(rhs_elem, _)) => {
                Some(self.check_same_shape_addita_plus(
                    lhs_resolved,
                    rhs_resolved,
                    lhs_elem,
                    rhs_elem,
                    TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED,
                    span,
                ))
            }
            (Type::Matrix(lhs_elem, _), Type::Matrix(rhs_elem, _)) => {
                Some(self.check_same_shape_addita_plus(
                    lhs_resolved,
                    rhs_resolved,
                    lhs_elem,
                    rhs_elem,
                    MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED,
                    span,
                ))
            }
            _ => None,
        }
    }

    fn check_tensor_addita_plus(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        lhs_elem: TypeId,
        rhs_elem: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        if !crate::numeric_width::is_tensor_numeric_element(self.types, lhs_elem)
            || !crate::numeric_width::is_tensor_numeric_element(self.types, rhs_elem)
        {
            self.invalid_operand_types(TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED, span);
            return self.error_type;
        }
        if !self.types.equals(lhs_elem, rhs_elem) {
            self.invalid_operand_types(ARGUMENT_TYPE_MISMATCH, span);
            return self.error_type;
        }
        let result = super::tensor_index::tensor_with_fresh_shape(self, lhs_elem);
        let result_shape = match self.types.get(result) {
            Type::Tensor(_, shape) => *shape,
            _ => fresh_tensor_result_missing_shape(),
        };
        super::tensor_index::constrain_elementwise_result_shape(
            self,
            lhs_ty,
            rhs_ty,
            result_shape,
            span,
        );
        result
    }

    fn check_same_shape_addita_plus(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        lhs_elem: TypeId,
        rhs_elem: TypeId,
        non_numeric_issue: &'static str,
        span: radix_lexer::Span,
    ) -> TypeId {
        if !crate::numeric_width::is_tensor_numeric_element(self.types, lhs_elem)
            || !crate::numeric_width::is_tensor_numeric_element(self.types, rhs_elem)
        {
            self.invalid_operand_types(non_numeric_issue, span);
            return self.error_type;
        }
        if !self.types.equals(lhs_ty, rhs_ty) {
            self.invalid_operand_types(ARGUMENT_TYPE_MISMATCH, span);
            return self.error_type;
        }
        lhs_ty
    }

    /// `⊙`/`⊘` Hadamard arithmetic: identical built-in shapes produce the identical shape.
    fn check_glyph_hadamard(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let Some((lhs_resolved, rhs_resolved)) = self.glyph_operand_types(op, lhs, rhs) else {
            return self.error_type;
        };
        match (
            self.types.get(lhs_resolved).clone(),
            self.types.get(rhs_resolved).clone(),
        ) {
            (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(_) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_n, rhs_n)) =
                    self.glyph_vector_widths(op, lhs_width, rhs_width, lhs.span)
                else {
                    return self.error_type;
                };
                self.glyph_same_shape_result(op, lhs_resolved, lhs_n == rhs_n, lhs.span)
            }
            (Type::Matrix(lhs_elem, lhs_shape), Type::Matrix(rhs_elem, rhs_shape)) => {
                let Some(_) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_rows, lhs_cols)) = crate::literal_matrix_shape(lhs_shape, self.types)
                else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                let Some((rhs_rows, rhs_cols)) = crate::literal_matrix_shape(rhs_shape, self.types)
                else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                self.glyph_same_shape_result(
                    op,
                    lhs_resolved,
                    (lhs_rows, lhs_cols) == (rhs_rows, rhs_cols),
                    lhs.span,
                )
            }
            (Type::Tensor(lhs_elem, lhs_shape), Type::Tensor(rhs_elem, rhs_shape)) => {
                let Some(_) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                // WHY: shape-generics Phase 2 — the tensor arm consumes the
                // same-shape/broadcast contract instead of requiring literal
                // shapes. Symbolic tuple shapes and whole-shape params merge
                // through `unify_broadcast`; shapes that cannot merge reject
                // with the glyph mismatch diagnostic.
                let result = super::tensor_index::tensor_with_fresh_shape(self, lhs_elem);
                let result_shape = match self.types.get(result) {
                    Type::Tensor(_, shape) => *shape,
                    _ => fresh_tensor_result_missing_shape(),
                };
                let lhs_shape = self.resolve_index(lhs_shape);
                let rhs_shape = self.resolve_index(rhs_shape);
                let mut unifier = self.index_infer.unifier(self.types);
                match unifier.unify_broadcast(lhs_shape, rhs_shape) {
                    Ok(merged) => {
                        self.constrain_index_equal(result_shape, merged, lhs.span);
                        result
                    }
                    Err(_) => {
                        self.glyph_invalid_operand(op, GLYPH_HADAMARD_SHAPES_MISMATCH, lhs.span);
                        self.error_type
                    }
                }
            }
            _ => {
                self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
                self.error_type
            }
        }
    }

    /// Check both glyph operands and resolve; `None` carries an already
    /// reported error so the caller returns `error_type`.
    fn glyph_operand_types(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> Option<(TypeId, TypeId)> {
        let lhs_ty = self.check_expr(lhs);
        let rhs_ty = self.check_expr(rhs);
        if self.is_error(lhs_ty) || self.is_error(rhs_ty) {
            return None;
        }
        // S0-G2: glyph operands are value-transparent through applied aliases
        // — `Vector<3> ⊗ Vector<2>` dispatches on the builtin carriers.
        let lhs_resolved = self.value_carrier_type(lhs_ty);
        let rhs_resolved = self.value_carrier_type(rhs_ty);
        if self.is_infer(lhs_resolved) || self.is_infer(rhs_resolved) {
            self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
            return None;
        }
        Some((lhs_resolved, rhs_resolved))
    }

    /// Require numeric elements for tensor matmul, admitting only the packed
    /// BF16-weight × F32-input combination. The result of that bounded arm is
    /// F32; every other mixed pair remains fail-closed.
    fn glyph_matmul_numeric_element(
        &mut self,
        op: HirBinOp,
        lhs_elem: TypeId,
        rhs_elem: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !crate::numeric_width::is_tensor_numeric_element(self.types, lhs_elem)
            || !crate::numeric_width::is_tensor_numeric_element(self.types, rhs_elem)
        {
            self.glyph_invalid_operand(op, GLYPH_OP_NUMERIC_ELEMENT_REQUIRED, span);
            return None;
        }
        let mixed_bf16_f32 = op == HirBinOp::Dot
            && matches!(
                (
                    crate::numeric_width::effective_width(self.types, lhs_elem),
                    crate::numeric_width::effective_width(self.types, rhs_elem),
                ),
                (
                    Some(crate::numeric_width::NumericWidth::BF16),
                    Some(crate::numeric_width::NumericWidth::F32),
                )
            );
        if mixed_bf16_f32 {
            return Some(rhs_elem);
        }
        self.glyph_numeric_element(op, lhs_elem, rhs_elem, span)
    }

    /// Require numeric, strictly equal elements for a glyph product.
    fn glyph_numeric_element(
        &mut self,
        op: HirBinOp,
        lhs_elem: TypeId,
        rhs_elem: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !crate::numeric_width::is_tensor_numeric_element(self.types, lhs_elem)
            || !crate::numeric_width::is_tensor_numeric_element(self.types, rhs_elem)
        {
            self.glyph_invalid_operand(op, GLYPH_OP_NUMERIC_ELEMENT_REQUIRED, span);
            return None;
        }
        if !self.types.equals(lhs_elem, rhs_elem) {
            self.glyph_invalid_operand(op, GLYPH_ELEMENT_TYPE_MISMATCH, span);
            return None;
        }
        Some(lhs_elem)
    }

    /// Both vector widths as literals; non-static widths fail closed.
    fn glyph_vector_widths(
        &mut self,
        op: HirBinOp,
        lhs: crate::IndexId,
        rhs: crate::IndexId,
        span: radix_lexer::Span,
    ) -> Option<(u8, u8)> {
        let (Some(lhs_n), Some(rhs_n)) = (
            crate::literal_vector_width(lhs, self.types),
            crate::literal_vector_width(rhs, self.types),
        ) else {
            self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, span);
            return None;
        };
        Some((lhs_n, rhs_n))
    }

    /// Result of `⊙`: the operand shape when both shapes are identical.
    fn glyph_same_shape_result(
        &mut self,
        op: HirBinOp,
        operand: TypeId,
        identical: bool,
        span: radix_lexer::Span,
    ) -> TypeId {
        if identical {
            operand
        } else {
            self.glyph_invalid_operand(op, GLYPH_HADAMARD_SHAPES_MISMATCH, span);
            self.error_type
        }
    }

    fn glyph_vector_type(&mut self, elem: TypeId, width: u8) -> TypeId {
        let width = self
            .types
            .intern_index(IndexExpr::Literal(u64::from(width)));
        self.types.vector_with_width(elem, width)
    }

    /// Symbolic `⊗` result shape `[lhs, rhs]` when both widths are
    /// glyph-static terms (literal or param); anything less static stays a
    /// genuine `glyph_static_shape_required` rejection from the caller.
    fn glyph_symbolic_outer_shape(
        &mut self,
        lhs_width: crate::IndexId,
        rhs_width: crate::IndexId,
    ) -> Option<crate::IndexId> {
        let glyph_static =
            |expr: &IndexExpr| matches!(expr, IndexExpr::Literal(_) | IndexExpr::Param(_));
        if !glyph_static(self.types.get_index(lhs_width))
            || !glyph_static(self.types.get_index(rhs_width))
        {
            return None;
        }
        Some(
            self.types
                .intern_index(IndexExpr::Tuple(vec![lhs_width, rhs_width])),
        )
    }

    /// The two raw dimension terms of a matrix shape, literal or parametric.
    fn matrix_shape_dims(
        &self,
        index_id: radix_types::IndexId,
    ) -> Option<(radix_types::IndexId, radix_types::IndexId)> {
        let IndexExpr::Tuple(dims) = self.types.get_index(index_id) else {
            return None;
        };
        let [rows, columns] = dims.as_slice() else {
            return None;
        };
        Some((*rows, *columns))
    }

    fn glyph_matrix_type(&mut self, elem: TypeId, rows: u8, columns: u8) -> TypeId {
        let rows = self.types.intern_index(IndexExpr::Literal(u64::from(rows)));
        let columns = self
            .types
            .intern_index(IndexExpr::Literal(u64::from(columns)));
        let shape = self
            .types
            .intern_index(IndexExpr::Tuple(vec![rows, columns]));
        self.types.matrix_with_shape(elem, shape)
    }

    fn glyph_invalid_operand(
        &mut self,
        op: HirBinOp,
        issue: &'static str,
        span: radix_lexer::Span,
    ) {
        self.invalid_operand_types_with_args(
            issue,
            span,
            &[("op", op.glyph_spelling().unwrap_or("?").to_owned())],
        );
    }

    fn check_integer_operands(
        &mut self,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> (TypeId, TypeId) {
        if is_numeric_literal(lhs) && !is_numeric_literal(rhs) {
            let rhs_ty = self.check_expr(rhs);
            let lhs_ty = if self
                .integer_shape(self.resolve_inferred_type(rhs_ty))
                .is_some()
            {
                self.check_expr_with_expected(lhs, Some(rhs_ty))
            } else {
                self.check_expr(lhs)
            };
            return (lhs_ty, rhs_ty);
        }

        let lhs_ty = self.check_expr(lhs);
        let rhs_ty = if is_numeric_literal(rhs)
            && self
                .integer_shape(self.resolve_inferred_type(lhs_ty))
                .is_some()
        {
            self.check_expr_with_expected(rhs, Some(lhs_ty))
        } else {
            self.check_expr(rhs)
        };
        (lhs_ty, rhs_ty)
    }

    fn check_coalesce(&mut self, lhs: &mut HirExpression, rhs: &mut HirExpression) -> TypeId {
        let lhs_ty = self.check_expr(lhs);
        let lhs_kind = self.types.get(self.resolve_inferred_type(lhs_ty)).clone();
        match lhs_kind {
            Type::Option(inner) => {
                let rhs_ty = if is_numeric_literal(rhs) {
                    self.check_expr_with_expected(rhs, Some(inner))
                } else {
                    self.check_expr(rhs)
                };
                match self.types.get(self.resolve_inferred_type(rhs_ty)).clone() {
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
                            rhs_ty,
                            inner,
                            rhs.span,
                            COALESCE_FALLBACK_TYPE_MISMATCH,
                        );
                        inner
                    }
                }
            }
            Type::Primitive(Primitive::Nihil) => self.check_expr(rhs),
            _ => {
                let rhs_ty = self.check_expr(rhs);
                self.common_type(lhs_ty, rhs_ty, lhs.span)
            }
        }
    }
}

impl TypeChecker<'_> {
    /// The imported type an `est` right-hand side names, if any.
    ///
    /// The parser classifies `est T` as a type test only for types declared
    /// in the same file; an imported type reaches here as `Binary(Is)` value
    /// equality. Until `type-algebra-error-lifecycle` open row 8 designs
    /// cross-file type tests, that shape is rejected by name (D18.1) rather
    /// than compared as a value. Const exports stay values. Both spellings
    /// qualify: the qualified `module.Type` path and a selective import
    /// binding that names a type (D22.5).
    fn imported_type_est_target(&self, rhs: &HirExpression) -> Option<Symbol> {
        if let HirExpressionKind::Path(def_id) = rhs.kind {
            self.resolver.selective_import_type(def_id)?;
            return self.resolver.get_symbol(def_id).map(|symbol| symbol.name);
        }
        let HirExpressionKind::Field(base, member) = &rhs.kind else {
            return None;
        };
        let HirExpressionKind::Path(base_def) = base.kind else {
            return None;
        };
        let symbol = self.resolver.get_symbol(base_def)?;
        if symbol.kind != crate::SymbolKind::Module {
            return None;
        }
        let namespace = symbol.name;
        (self
            .resolver
            .namespace_file_type(namespace, *member)
            .is_some()
            && self
                .resolver
                .namespace_file_const(namespace, *member)
                .is_none())
        .then_some(*member)
    }
}

fn is_numeric_literal(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Literal(
            HirLiteral::Int(_) | HirLiteral::Float(_) | HirLiteral::FloatText { .. },
        ) => true,
        // A negated literal (`-1.0`) is still a literal for propagation purposes.
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => is_numeric_literal(inner),
        _ => false,
    }
}

const NUMERIC_OPERANDS_REQUIRED: &str = "numeric_operands_required";
const MIXED_DECIMAL_FRACTUS_ORDERING: &str = "mixed_decimal_fractus_ordering";
const MIXED_DECIMAL_FRACTUS_EQUALITY: &str = "mixed_decimal_fractus_equality";
const NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL: &str = "numeric_arithmetic_operands_identical";
const MODULAR_WORD_FAMILY_MISMATCH: &str = "modular_word_family_mismatch";
// TENSOR-GLYPHS-1B rank-rule diagnostics. Each carries `op` (the glyph
// spelling) so renderers and tests identify the operator without prose.
const GLYPH_OPERANDS_UNSUPPORTED: &str = "glyph_operands_unsupported";
const GLYPH_STATIC_SHAPE_REQUIRED: &str = "glyph_static_shape_required";
const GLYPH_OP_NUMERIC_ELEMENT_REQUIRED: &str = "glyph_op_numeric_element_required";
const GLYPH_ELEMENT_TYPE_MISMATCH: &str = "glyph_element_type_mismatch";
const GLYPH_INNER_DIM_MISMATCH: &str = "glyph_inner_dim_mismatch";
const GLYPH_CROSS_WIDTH_MISMATCH: &str = "glyph_cross_width_mismatch";
const GLYPH_CROSS_WIDTH_THREE_REQUIRED: &str = "glyph_cross_width_three_required";
const GLYPH_HADAMARD_SHAPES_MISMATCH: &str = "glyph_hadamard_shapes_mismatch";
const EQUALITY_EXACT_TYPES_REQUIRED: &str = "equality_exact_types_required";
const COMMON_TYPE_MISMATCH: &str = "common_type_mismatch";
const ASSIGNMENT_TYPE_MISMATCH: &str = "assignment_type_mismatch";
const MEMBERSHIP_OPERAND_TYPE_MISMATCH: &str = "membership_operand_type_mismatch";
const COALESCE_FALLBACK_TYPE_MISMATCH: &str = "coalesce_fallback_type_mismatch";
const BOOLEAN_OPERANDS_REQUIRED: &str = "boolean_operands_required";
const INTEGER_OPERANDS_REQUIRED: &str = "integer_operands_required";
const SAME_WIDTH_INTEGER_OPERANDS_REQUIRED: &str = "same_width_integer_operands_required";
const POSTFIX_INC_DEC_NUMERUS_OPERAND: &str = "postfix_inc_dec_numerus_operand";
const NUMERIC_OPERAND_REQUIRED: &str = "numeric_operand_required";
const BOOLEAN_OPERAND_REQUIRED: &str = "boolean_operand_required";
const INTEGER_OPERAND_REQUIRED: &str = "integer_operand_required";
const STRING_OPERANDS_REQUIRED: &str = "string_operands_required";
const NUMERIC_LISTA_OPERATOR_LAW: &str = "numeric_lista_operator_law";
const NUMERIC_QUEUE_STACK_OPERATOR_LAW: &str = "numeric_queue_stack_operator_law";
const TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED: &str =
    "tensor_arithmetic_numeric_element_required";
const MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED: &str =
    "matrix_arithmetic_numeric_element_required";
const ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";

#[derive(Clone, Copy)]
struct NumericShape {
    prim: Primitive,
    width: NumericWidth,
    modular: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueueStackArithmeticKind {
    Queue,
    Stack,
}

impl QueueStackArithmeticKind {
    fn spelling(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Stack => "stack",
        }
    }
}

#[derive(Clone, Copy)]
struct QueueStackArithmeticShape {
    kind: QueueStackArithmeticKind,
    element: TypeId,
    capacity: Option<radix_types::IndexId>,
}

#[derive(Clone, Copy)]
struct ListaArithmeticShape {
    element: TypeId,
    capacity: Option<radix_types::IndexId>,
}

fn lista_operator_spelling(op: HirBinOp) -> &'static str {
    match op {
        HirBinOp::Add => "+",
        HirBinOp::Sub => "-",
        HirBinOp::Mul => "*",
        HirBinOp::Div => "/",
        _ => unreachable!("non-list arithmetic operator"),
    }
}

fn numeric_literal_family(expr: &HirExpression) -> Option<Primitive> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(_)) => Some(Primitive::Numerus),
        HirExpressionKind::Literal(HirLiteral::Float(_) | HirLiteral::FloatText { .. }) => {
            Some(Primitive::Fractus)
        }
        // A negated literal (`-1.0`) inherits the inner literal's family so
        // that peer-type propagation in arithmetic recognizes it.
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => numeric_literal_family(inner),
        _ => None,
    }
}

// U-04 integration tests live here (not `mod.rs`, which is U-02's frozen
// surface): the join under a `∪`-hole expectation is owned by this module's
// `common_type` path and `check_array`/`check_if` sibling callers.
