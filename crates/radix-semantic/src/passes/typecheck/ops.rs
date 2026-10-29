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

use super::store::StoreSite;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

mod aggregate_compare;
mod lista_lift;
mod queue_stack_lift;
mod tensor_lift;

const INSTANS_ORDERING_OPERANDS_REQUIRED: &str = "instans_ordering_operands_required";
const NUMERIC_VALUE_EQUALITY_OPERANDS_REQUIRED: &str = "numeric_value_equality_operands_required";
const ISSUE_CONVERSIO_ASSIGN_ROUTE_UNSUPPORTED: &str = "conversio_assign_route_unsupported";
const ISSUE_CONVERSIO_ASSIGN_INFERENCE_HOLE_DESTINATION: &str =
    "conversio_assign_inference_hole_destination";

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
        if lhs_shape.zip(rhs_shape).is_some_and(|(lhs, rhs)| {
            lhs.family != rhs.family || (either_policy(lhs, rhs) && lhs.width != rhs.width)
        }) {
            let issue = family_mismatch_issue(lhs_shape, rhs_shape);
            self.invalid_operand_types(issue, span);
            return self.error_type;
        }

        if self.is_fractus(lhs) || self.is_fractus(rhs) {
            self.fractus_type()
        } else {
            self.numerus_type()
        }
    }

    /// Scalar `+ - * / %`: range-typed for trapping integers and integer
    /// constants, family rules for everything else.
    fn scalar_arithmetic(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        expected: Option<TypeId>,
    ) -> TypeId {
        if !self.trapping_float_operands_agree(lhs, rhs, lhs.span) {
            return self.error_type;
        }
        if let Some(result) = self.trapping_float_join(lhs, rhs) {
            return result;
        }
        if let Some(result) =
            self.arithmetic_range_type(op, lhs, rhs, lhs_ty, rhs_ty, expected, lhs.span)
        {
            return result;
        }
        self.numeric_arithmetic_bin(lhs_ty, rhs_ty, lhs.span)
    }

    pub(super) fn numeric_arithmetic_bin(
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

        if !self.current_device_function {
            // d11-6 ruling 16: integer, decimal and float never mix implicitly.
            if lhs_shape.prim != rhs_shape.prim
                || lhs_shape.width.is_decimal() != rhs_shape.width.is_decimal()
            {
                self.invalid_operand_types(
                    integer_range::NUMERIC_FAMILY_CROSSING_REQUIRES_CONVERSIO,
                    span,
                );
                return self.error_type;
            }
            if lhs_shape.family == IntFamily::Plain
                && rhs_shape.family == IntFamily::Plain
                && lhs_shape.width != rhs_shape.width
            {
                // Floats and decimals join by the lossless lattice (ruling 1).
                if NumericWidth::is_widening_assignable(lhs_shape.width, rhs_shape.width) {
                    return rhs_resolved;
                }
                if NumericWidth::is_widening_assignable(rhs_shape.width, lhs_shape.width) {
                    return lhs_resolved;
                }
            }
        }
        if lhs_shape.prim != rhs_shape.prim || lhs_shape.width != rhs_shape.width {
            // D4: a typed mix of a float cell with another float (a bare
            // float, the other cell, another width) names the cell family.
            let issue = if lhs_shape.prim == rhs_shape.prim && either_policy(lhs_shape, rhs_shape) {
                POLICY_FLOAT_FAMILY_MISMATCH
            } else {
                NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL
            };
            self.invalid_operand_types(issue, span);
            return self.error_type;
        }
        if lhs_shape.family != rhs_shape.family {
            let issue = family_mismatch_issue(Some(lhs_shape), Some(rhs_shape));
            self.invalid_operand_types(issue, span);
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
                let glyph_issue = match op {
                    HirBinOp::Mul => {
                        Some(queue_stack_lift::ISSUE_QUEUE_STACK_MULTIPLY_REQUIRES_GLYPH)
                    }
                    HirBinOp::Div => {
                        Some(queue_stack_lift::ISSUE_QUEUE_STACK_DIVIDE_REQUIRES_GLYPH)
                    }
                    _ => None,
                };
                if let Some(issue) = glyph_issue {
                    self.invalid_operand_types_with_args(
                        issue,
                        span,
                        &[("collection", lhs.kind.spelling().to_owned())],
                    );
                    return Some(self.error_type);
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
            (None, None) => return None,
        };

        Some(self.queue_stack_result(kind, element, capacity))
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
        lhs.prim == rhs.prim && lhs.width == rhs.width && lhs.family == rhs.family
    }

    pub(super) fn numeric_shape(&self, ty: TypeId) -> Option<NumericShape> {
        let (_, ty) = self.types.peel_access_marker(ty);
        match self.types.get(ty) {
            Type::Primitive(Primitive::Numerus) => Some(NumericShape {
                prim: Primitive::Numerus,
                width: NumericWidth::I64,
                family: IntFamily::Plain,
            }),
            Type::Primitive(Primitive::Fractus) => Some(NumericShape {
                prim: Primitive::Fractus,
                width: NumericWidth::F64,
                family: IntFamily::Plain,
            }),
            Type::SizedNumeric(prim @ (Primitive::Numerus | Primitive::Fractus), width) => {
                Some(NumericShape {
                    prim: *prim,
                    width: *width,
                    family: IntFamily::Plain,
                })
            }
            Type::ModularWord(width) => Some(NumericShape {
                prim: Primitive::Numerus,
                width: *width,
                family: IntFamily::Modular,
            }),
            Type::SaturatingWord(width) => Some(NumericShape {
                prim: Primitive::Numerus,
                width: *width,
                family: IntFamily::Saturating,
            }),
            // A ranged cell is a value of its width in its policy's family:
            // reads compute as the width, and only a store applies the range.
            Type::RangedInt { policy, width, .. } => Some(NumericShape {
                prim: Primitive::Numerus,
                width: *width,
                family: match policy {
                    radix_types::IntPolicy::Trapping => IntFamily::Plain,
                    radix_types::IntPolicy::Wrapping => IntFamily::Modular,
                    radix_types::IntPolicy::Saturating => IntFamily::Saturating,
                },
            }),
            Type::PolicyFloat(policy, width) => Some(NumericShape {
                prim: Primitive::Fractus,
                width: *width,
                family: IntFamily::Policy(*policy),
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
        // N7e D4: arithmetic over `trapping<W>` operands is the bare float
        // `W`, an expression's value rather than a slot's; the cell it came
        // from is tracked by `trapping_float_origin` and checked at the store.
        if let Some(Type::PolicyFloat(radix_types::FloatPolicy::Trapping, width)) = [lhs, rhs]
            .into_iter()
            .map(|ty| self.types.get(ty).clone())
            .find(|ty| matches!(ty, Type::PolicyFloat(radix_types::FloatPolicy::Trapping, _)))
        {
            return self.bare_float_type(width);
        }
        if matches!(
            self.types.get(lhs),
            Type::SizedNumeric(_, _)
                | Type::ModularWord(_)
                | Type::SaturatingWord(_)
                | Type::PolicyFloat(_, _)
        ) {
            return lhs;
        }
        if matches!(
            self.types.get(rhs),
            Type::SizedNumeric(_, _)
                | Type::ModularWord(_)
                | Type::SaturatingWord(_)
                | Type::PolicyFloat(_, _)
        ) {
            return rhs;
        }
        // A ranged cell computes as the plain form it refines.
        if self.types.ranged_int_parts(lhs).is_some() {
            return self.types.ranged_int_base(lhs);
        }
        if self.types.ranged_int_parts(rhs).is_some() {
            return self.types.ranged_int_base(rhs);
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
    /// the same scaled `d64` join; decimal/fractus is accepted too (ruling 22:
    /// cross-family ordering compares exact values).
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

        // d11-6 ruling 22: ordering accepts every cross-family pair
        // (integer, d64, float); the comparison is on exact values.
        let joined = self.numeric_bin(lhs, rhs, span);
        if joined == self.error_type || joined == self.fractus_type() {
            return joined;
        }
        if decimal_lhs || decimal_rhs {
            return self
                .types
                .sized_numeric(Primitive::Numerus, NumericWidth::D64);
        }
        joined
    }

    /// Allow cross-precision `instans` operands for ordering and `≅`/`≇`.
    ///
    /// WHY: operands need not unify to one precision marker in the type table,
    /// so this only checks that both sides are `instans` and returns the
    /// resolved rhs type; the caller discards it (ordering yields `bivalens`,
    /// and `≅`/`≇` compare at the coarser precision like ordering). Exact
    /// `≡`/`≢` use [`Self::exact_equality_types`] instead.
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

    /// A `wrapping` or `saturating` integer word beside an untyped integer
    /// constant or a plain (trapping) integer: an ordering or `≅`/`≇`
    /// comparison stores nothing and compares exact values (rulings 12, 22).
    /// A wrapping operand is already a reduced value and a saturating operand a
    /// plain value, so no join type is needed.
    fn word_value_comparison(
        &self,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs: &HirExpression,
        rhs_ty: TypeId,
    ) -> bool {
        if self.current_device_function {
            return false;
        }
        let word_or_plain = |expr: &HirExpression, ty: TypeId| {
            let shape = self.numeric_shape(self.resolve_inferred_type(ty))?;
            if shape.prim != Primitive::Numerus || shape.width.is_decimal() {
                return None;
            }
            Some((shape.family, super::integer_range::is_const_int(expr)))
        };
        let (Some((lf, lc)), Some((rf, rc))) =
            (word_or_plain(lhs, lhs_ty), word_or_plain(rhs, rhs_ty))
        else {
            return false;
        };
        let is_word = |f: IntFamily| f != IntFamily::Plain;
        let other_ok = |f: IntFamily, c: bool| f == IntFamily::Plain || c;
        (is_word(lf) && !lc && other_ok(rf, rc)) || (is_word(rf) && !rc && other_ok(lf, lc))
    }

    /// A float cell beside an untyped numeric constant in an ordering or a
    /// value comparison: the comparison stores nothing and compares exact
    /// values, and a written constant stands beside any number (N7e D4).
    fn policy_float_constant_comparison(
        &self,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs: &HirExpression,
        rhs_ty: TypeId,
    ) -> bool {
        if self.current_device_function {
            return false;
        }
        let is_cell = |ty: TypeId| {
            self.numeric_shape(self.resolve_inferred_type(ty))
                .is_some_and(|shape| matches!(shape.family, IntFamily::Policy(_)))
        };
        (is_cell(lhs_ty) && is_numeric_literal(rhs)) || (is_cell(rhs_ty) && is_numeric_literal(lhs))
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
    /// d11-6 ruling 22 (supersedes GT-2): `≅`/`≇`/`≈`/`≉` and ordering accept
    /// every cross-family pair (integer, `d64`, float); `≡`/`≢` stay exact-type.
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
        // d11-6 ruling 22: `≅`/`≇`/`≈`/`≉` accept every cross-family pair.
        self.numeric_bin(lhs, rhs, span);
    }

    /// Enforce exact-type match for `≡`, `≢`, `est`, and `non est`.
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
        let lhs = self.equality_value_marker_carrier(lhs);
        let rhs = self.equality_value_marker_carrier(rhs);
        self.exact_equality_types(lhs, rhs, span);
    }

    /// A binding's `copy` / `own` value-side marker records how the value was
    /// produced, not what it is: equality compares the carrier. `ref` and
    /// `mut` views keep their wrapper (the view contract).
    fn equality_value_marker_carrier(&mut self, ty: TypeId) -> TypeId {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.peel_access_marker(resolved) {
            (Some(crate::AccessMarker::Copy | crate::AccessMarker::Own), carrier) => carrier,
            _ => resolved,
        }
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
        if lhs_shape.family != rhs_shape.family {
            let issue = family_mismatch_issue(Some(lhs_shape), Some(rhs_shape));
            self.invalid_operand_types(issue, span);
            return self.error_type;
        }

        self.preferred_numeric_result_ty(lhs_resolved, rhs_resolved, lhs_shape)
    }

    pub(super) fn integer_shape(&self, ty: TypeId) -> Option<NumericShape> {
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
        // Two tensors whose shapes are equal but interned apart (each branch
        // of `c ✓ a.transpose() ✗ a.transpose()` carries its own derived
        // shape index) are not assignable by id; they join by the same
        // structural shape unification an annotated destination applies.
        if matches!(
            (self.types.get(left), self.types.get(right)),
            (Type::Tensor(..), Type::Tensor(..))
        ) {
            return self.unify_join(a, b, span, COMMON_TYPE_MISMATCH);
        }
        if !self.current_device_function
            && let Some(joined) = self.integer_hull_type(left, right, span)
        {
            // d11-6 ruling 21: the smallest integer type holding both branches.
            return joined;
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
        let (_, payload) = self
            .types
            .peel_access_marker(self.resolve_inferred_type(target_ty));
        if matches!(
            self.types.get(payload),
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal()
        ) {
            self.invalid_operand_types(POSTFIX_INC_DEC_NUMERUS_OPERAND, span);
        }
    }

    /// Check simple assignment and constrain the value from the target context.
    pub fn check_assign(
        &mut self,
        target: &mut HirExpression,
        value: &mut HirExpression,
    ) -> TypeId {
        // The value is evaluated before the store takes effect, so a flow
        // fact on the target still holds for the value expression. Save it,
        // type the lvalue from the declared cell, re-arm the fact for the
        // value check, and drop it again once the store is checked.
        let saved_target_fact = match &target.kind {
            HirExpressionKind::Path(def_id) => {
                self.lookup_binding(*def_id).map(|info| (*def_id, info))
            }
            _ => None,
        };
        self.invalidate_assignment_target(target);
        let target_ty = self.check_lvalue(target);
        if target_ty == self.error_type {
            self.check_expr(value);
            return self.error_type;
        }
        if self.reject_copy_into_value(value) {
            self.check_expr(value);
            return target_ty;
        }
        if self.is_shader_vec3_to_vec4_output_target(target, target_ty) {
            let value_ty = self.check_expr(value);
            if self.is_f32_vector_width(value_ty, 3) {
                return target_ty;
            }
            self.unify_assign_issue(value_ty, target_ty, value.span, ASSIGNMENT_TYPE_MISMATCH);
            return target_ty;
        }
        let site = self.assignment_store_site(target);
        if let Some((def_id, info)) = saved_target_fact {
            self.reinstate_flow_fact(def_id, info);
        }
        let value_ty = self.check_store_expr(value, target_ty, site);
        // The store happens after its right-hand side: prove an indexed target
        // against the facts that survive it.
        self.record_store_index_proof(target);
        self.invalidate_assignment_target(target);
        let unresolved_edge_view = self
            .tensor_edge_policies
            .shifted_expressions
            .contains(&value.id);
        let edge_view_context = self
            .tensor_edge_policies
            .shifted_view_expressions
            .get(&value.id)
            .copied();
        if unresolved_edge_view {
            if let HirExpressionKind::Path(binding) = &target.kind {
                self.tensor_edge_policies.shifted_bindings.insert(*binding);
                if let Some(context) = edge_view_context {
                    self.tensor_edge_policies
                        .shifted_view_bindings
                        .insert(*binding, context);
                }
            } else {
                self.invalid_operand_types(
                    super::intrinsics::ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION,
                    value.span,
                );
                return self.error_type;
            }
        } else if let HirExpressionKind::Path(binding) = &target.kind {
            self.tensor_edge_policies.shifted_bindings.remove(binding);
            self.tensor_edge_policies
                .shifted_view_bindings
                .remove(binding);
        }
        self.unify_carrier_assign(value_ty, target_ty, value.span, ASSIGNMENT_TYPE_MISMATCH);
        if self.types.is_own_view(target_ty) {
            self.check_own_bind_rhs(value);
        }
        target_ty
    }

    /// The store position of `target ← value`: the receiving place's name and,
    /// for a local whose type was inferred, where the type came from.
    fn assignment_store_site(&self, target: &HirExpression) -> StoreSite {
        let site = StoreSite::new(radix_hir::HirStoreKind::Assignment);
        match &target.kind {
            HirExpressionKind::Path(def_id) => {
                let site = site.inferred_from(self.inferred_slot_sources.get(def_id).copied());
                match self.binding_names.get(def_id) {
                    Some(name) => site.named(*name),
                    None => site,
                }
            }
            HirExpressionKind::Field(_, name) => site.named(*name),
            _ => site,
        }
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
        params: &[radix_lexer::Symbol],
        span: radix_lexer::Span,
    ) -> TypeId {
        self.invalidate_assignment_target(target);
        let target_ty = self.check_lvalue(target);
        if target_ty == self.error_type {
            self.check_expr(source);
            return self.error_type;
        }
        if self.reject_copy_into_value(source) {
            self.check_expr(source);
            return target_ty;
        }
        // The destination type must resolve before conversion checking: an
        // inference-hole destination (e.g. `sit xs ← []`) cannot name a
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
            params,
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
            | Type::RangedArray { element: inner, .. }
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Matrix(inner, _)
            | Type::Vector(inner, _)
            | Type::Atomic(inner)
            | Type::Intervallum(inner, _)
            | Type::Option(inner) => self.destination_type_has_inference_hole_inner(*inner, seen),
            Type::Map(key, value) | Type::Ledger(key, value) => {
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
                // `-9223372036854775808` is an exact constant that fits `int`;
                // its magnitude alone does not, so the literal is read wide.
                if matches!(
                    operand.kind,
                    HirExpressionKind::Literal(HirLiteral::Int(magnitude)) if magnitude == 1 << 63
                ) && self.expected_float_slot_type(expected).is_none()
                {
                    self.check_const_operand(operand);
                    return self.numerus_type();
                }
                if let Some(result) = self.bits_negation(operand, None, expected) {
                    self.check_expr(operand);
                    return result;
                }
                let expected = self.unary_operand_expected(operand, expected);
                let expected = self.unary_float_cell_expected(operand, expected);
                let operand_ty = self.check_expr_with_expected(operand, expected);
                if self.is_error(operand_ty) {
                    return self.error_type;
                }
                if let Some(result) = self.bits_negation(operand, Some(operand_ty), None) {
                    return result;
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
                if let Some(result) = self.check_lista_neg(operand, operand_ty) {
                    return result;
                }
                if let Some(result) = self.check_tensor_lift_neg(operand, operand_ty) {
                    return result;
                }
                if !self.is_numeric(operand_ty) {
                    self.invalid_operand_types(NUMERIC_OPERAND_REQUIRED, operand.span);
                }
                self.unary_range_type(op, operand, operand_ty)
                    .unwrap_or(operand_ty)
            }
            radix_hir::HirUnOp::Not => {
                let operand_ty = self.check_expr(operand);
                if self.is_error(operand_ty) {
                    return self.bool_type();
                }
                if let Some(result) = self.check_tensor_lift_not(operand, operand_ty) {
                    return result;
                }
                if let Some(result) = self.check_lista_not(operand, operand_ty) {
                    return result;
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
                let expected = self.unary_operand_expected(operand, expected);
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
                if self.bits_width_of(operand_resolved).is_some() {
                    // `¬` flips exactly N bits and keeps the `bN`.
                    return self.numeric_expected_type(operand_resolved);
                }
                if self.integer_shape(operand_resolved).is_none() {
                    self.invalid_operand_types(INTEGER_OPERAND_REQUIRED, operand.span);
                    return self.error_type;
                }
                self.unary_range_type(op, operand, operand_resolved)
                    .unwrap_or(operand_resolved)
            }
        }
    }

    /// Check the operands of `+ - * / %`.
    ///
    /// Integer constants are untyped exact values (d11-6 ruling 12): beside a
    /// trapping integer they are not shaped to its width. Float, word and
    /// hole peers keep the peer-shaping rule. Device functions keep the
    /// pre-range shaping unchanged.
    fn check_arithmetic_operands(
        &mut self,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> (TypeId, TypeId) {
        if self.current_device_function {
            return self.check_arithmetic_operands_legacy(lhs, rhs, expected);
        }
        match (
            integer_range::is_const_int(lhs),
            integer_range::is_const_int(rhs),
        ) {
            (true, true) if self.const_pair_is_legacy(expected) => {
                self.check_arithmetic_operands_legacy(lhs, rhs, expected)
            }
            (true, true) => (self.check_const_operand(lhs), self.check_const_operand(rhs)),
            (true, false) => {
                let rhs_ty = self.check_expr(rhs);
                if let Some(lhs_ty) = self.bits_peer_literal(lhs, rhs_ty) {
                    return (lhs_ty, rhs_ty);
                }
                let peer = self.arith_peer_type(rhs_ty);
                (self.check_const_beside(lhs, peer, expected), rhs_ty)
            }
            (false, true) => {
                let lhs_ty = self.check_expr(lhs);
                if let Some(rhs_ty) = self.bits_peer_literal(rhs, lhs_ty) {
                    return (lhs_ty, rhs_ty);
                }
                let peer = self.arith_peer_type(lhs_ty);
                (lhs_ty, self.check_const_beside(rhs, peer, expected))
            }
            (false, false) => self.check_arithmetic_operands_legacy(lhs, rhs, expected),
        }
    }

    fn check_arithmetic_operands_legacy(
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
                if let Some(lhs_ty) = self.bits_peer_literal(lhs, rhs_ty) {
                    return (lhs_ty, rhs_ty);
                }
                let peer = self.arith_peer_type(rhs_ty);
                let lhs_expected = self
                    .literal_numeric_expected_from_peer(lhs, peer)
                    .or_else(|| self.literal_numeric_expected(lhs, expected));
                let lhs_ty = self.check_expr_with_expected(lhs, lhs_expected);
                (lhs_ty, rhs_ty)
            }
            (None, Some(_)) => {
                let lhs_ty = self.check_expr(lhs);
                if let Some(rhs_ty) = self.bits_peer_literal(rhs, lhs_ty) {
                    return (lhs_ty, rhs_ty);
                }
                let peer = self.arith_peer_type(lhs_ty);
                let rhs_expected = self
                    .literal_numeric_expected_from_peer(rhs, peer)
                    .or_else(|| self.literal_numeric_expected(rhs, expected));
                let rhs_ty = self.check_expr_with_expected(rhs, rhs_expected);
                (lhs_ty, rhs_ty)
            }
            (None, None) => (self.check_expr(lhs), self.check_expr(rhs)),
        }
    }

    /// F9 ruling 30: in a comparison an untyped constant beside a decimal
    /// operand joins the decimal family by exact value, so `d ≅ 4.2` compares
    /// decimal 4.2 and `d < 0.123456789` is `decimal_literal_out_of_range`.
    /// `None` when `expr` is not a constant or `peer` is not a decimal; the
    /// caller then checks `expr` as before.
    pub(super) fn check_constant_beside_decimal(
        &mut self,
        expr: &mut HirExpression,
        peer: TypeId,
    ) -> Option<TypeId> {
        if self.current_device_function
            || !(is_numeric_literal(expr) || integer_range::is_const_int(expr))
        {
            return None;
        }
        let peer = self.resolve_inferred_type(peer);
        self.expected_decimal_slot_type(Some(peer))?;
        Some(self.check_expr_with_expected(expr, Some(peer)))
    }

    pub(super) fn literal_numeric_expected(
        &self,
        expr: &HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let family = numeric_literal_family(expr)?;
        let expected = self.resolve_inferred_type(expected?);
        self.numeric_shape(expected)
            .filter(|shape| literal_joins_shape(expr, family, shape))
            .map(|_| self.numeric_expected_type(expected))
    }

    pub(super) fn literal_numeric_expected_from_peer(
        &self,
        expr: &HirExpression,
        peer: TypeId,
    ) -> Option<TypeId> {
        let family = numeric_literal_family(expr)?;
        let peer = self.resolve_inferred_type(peer);
        self.numeric_shape(peer)
            .filter(|shape| literal_joins_shape(expr, family, shape))
            .map(|_| self.numeric_expected_type(peer))
    }

    pub(super) fn numeric_expected_type(&self, ty: TypeId) -> TypeId {
        // Numeric shape discovery accepts access-marked places, but literal
        // slot inference (for example, `expected_decimal_slot_type`) needs the
        // value type beneath a `mut`/`ref` parameter marker. Treat a parameter
        // place like a local of the same declared numeric type.
        let (_, ty) = self.types.peel_access_marker(ty);
        match self.types.get(ty) {
            Type::Alias(_, target) => self.numeric_expected_type(*target),
            _ => ty,
        }
    }

    /// Check binary operators and return the expression result type.
    ///
    /// This is where call/access-independent control-flow values are shaped:
    /// comparisons become `bivalens`, arithmetic preserves exact numeric widths,
    /// text `+` requires both operands to be cord text, and coalescing encodes the local
    /// optional/`nihil` result contract.
    pub fn check_binary(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        let result = self.check_binary_unadopted(op, lhs, rhs, expected);
        self.adopt_expected_lift_element(result, expected)
    }

    fn check_binary_unadopted(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        match op {
            HirBinOp::Add => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                if let Some(result) = self.bits_binary(op, lhs_ty, rhs_ty, lhs.span) {
                    return result;
                }
                let lhs_text = self.is_cord_text(lhs_ty);
                let rhs_text = self.is_cord_text(rhs_ty);
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
                    self.check_lista_arithmetic_bin(HirBinOp::Add, lhs, rhs, lhs_ty, rhs_ty)
                {
                    result
                } else if let Some(result) =
                    self.check_addita_plus(HirBinOp::Add, lhs_ty, rhs_ty, lhs.span)
                {
                    // Numeric tensor/vector/matrix `+` is the live `addita` / `.added()` op.
                    result
                } else if let Some(result) =
                    self.check_tensor_lift_arith(HirBinOp::Add, lhs, rhs, lhs_ty, rhs_ty)
                {
                    result
                } else {
                    self.scalar_arithmetic(HirBinOp::Add, lhs, rhs, lhs_ty, rhs_ty, expected)
                }
            }
            HirBinOp::Sub => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                if let Some(result) = self.bits_binary(op, lhs_ty, rhs_ty, lhs.span) {
                    return result;
                }
                if let Some(result) =
                    self.check_queue_stack_arithmetic_bin(HirBinOp::Sub, lhs_ty, rhs_ty, lhs.span)
                {
                    result
                } else if let Some(result) =
                    self.check_lista_arithmetic_bin(HirBinOp::Sub, lhs, rhs, lhs_ty, rhs_ty)
                {
                    result
                } else if let Some(result) =
                    self.check_addita_plus(HirBinOp::Sub, lhs_ty, rhs_ty, lhs.span)
                {
                    // Numeric tensor/vector/matrix `-` is the live `subtracta` / `.subtract()` op.
                    // Same-shape / tensor-broadcast contract as `+` — no extra broadcast.
                    result
                } else if let Some(result) =
                    self.check_tensor_lift_arith(HirBinOp::Sub, lhs, rhs, lhs_ty, rhs_ty)
                {
                    result
                } else {
                    self.scalar_arithmetic(HirBinOp::Sub, lhs, rhs, lhs_ty, rhs_ty, expected)
                }
            }
            HirBinOp::Mul | HirBinOp::Div => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                if let Some(result) = self.bits_binary(op, lhs_ty, rhs_ty, lhs.span) {
                    return result;
                }
                if let Some(result) =
                    self.check_queue_stack_arithmetic_bin(op, lhs_ty, rhs_ty, lhs.span)
                {
                    result
                } else if let Some(result) =
                    self.check_lista_arithmetic_bin(op, lhs, rhs, lhs_ty, rhs_ty)
                {
                    result
                } else if let Some(result) =
                    self.check_tensor_lift_arith(op, lhs, rhs, lhs_ty, rhs_ty)
                {
                    result
                } else {
                    self.scalar_arithmetic(op, lhs, rhs, lhs_ty, rhs_ty, expected)
                }
            }
            HirBinOp::Mod => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                if let Some(result) = self.bits_binary(op, lhs_ty, rhs_ty, lhs.span) {
                    return result;
                }
                if let Some(result) = self.check_lista_arithmetic_bin(op, lhs, rhs, lhs_ty, rhs_ty)
                {
                    return result;
                }
                if let Some(result) = self.check_tensor_lift_arith(op, lhs, rhs, lhs_ty, rhs_ty) {
                    return result;
                }
                self.scalar_arithmetic(op, lhs, rhs, lhs_ty, rhs_ty, expected)
            }
            HirBinOp::TrueDiv => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                if let Some(result) = self.bits_binary(op, lhs_ty, rhs_ty, lhs.span) {
                    return result;
                }
                if let Some(result) = self.check_lista_arithmetic_bin(op, lhs, rhs, lhs_ty, rhs_ty)
                {
                    return result;
                }
                if let Some(result) = self.check_tensor_lift_arith(op, lhs, rhs, lhs_ty, rhs_ty) {
                    return result;
                }
                if !self.trapping_float_operands_agree(lhs, rhs, lhs.span) {
                    return self.error_type;
                }
                if let Some(result) = self.trapping_float_join(lhs, rhs) {
                    return result;
                }
                self.true_division_type(lhs, rhs, lhs_ty, rhs_ty, lhs.span)
            }
            HirBinOp::ApproxEq
            | HirBinOp::ApproxNotEq
            | HirBinOp::FuzzyEq
            | HirBinOp::FuzzyNotEq => self.check_approximate_equality_binary(op, lhs, rhs),
            HirBinOp::Eq
            | HirBinOp::NotEq
            | HirBinOp::StrictEq
            | HirBinOp::StrictNotEq
            | HirBinOp::Is
            | HirBinOp::IsNot => self.check_equality_binary(op, lhs, rhs),
            HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq => {
                self.check_ordering_binary(op, lhs, rhs)
            }
            HirBinOp::InRange | HirBinOp::NotInRange => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                self.check_intra_operand(lhs_ty, rhs_ty, lhs.span, rhs.span);
                self.bool_type()
            }
            HirBinOp::Between | HirBinOp::NotBetween => {
                unreachable!("membership is resolved by check_membership before check_binary")
            }
            HirBinOp::Coalesce => self.check_coalesce(lhs, rhs),
            HirBinOp::And => {
                let lhs_ty = self.check_expr(lhs);
                // `et` short-circuits: the right conjunct is evaluated only
                // when the left holds, so it is checked under the left
                // conjunct's then-facts.
                let rhs_ty = self.check_conjunct_rhs_narrowed(&*lhs, rhs);
                if let Some(result) = self.check_tensor_lift_logic(lhs, lhs_ty, rhs_ty) {
                    return result;
                }
                if let Some(result) = self.check_lista_logic(op, lhs, lhs_ty, rhs_ty) {
                    return result;
                }
                self.bool_bin(lhs_ty, rhs_ty, lhs.span)
            }
            HirBinOp::Or => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                if let Some(result) = self.check_tensor_lift_logic(lhs, lhs_ty, rhs_ty) {
                    return result;
                }
                if let Some(result) = self.check_lista_logic(op, lhs, lhs_ty, rhs_ty) {
                    return result;
                }
                self.bool_bin(lhs_ty, rhs_ty, lhs.span)
            }
            HirBinOp::BitAnd
            | HirBinOp::BitOr
            | HirBinOp::BitXor
            | HirBinOp::Shl
            | HirBinOp::Shr => {
                let (lhs_ty, rhs_ty) = self.check_integer_operands(op, lhs, rhs, expected);
                if self.reject_negative_shift_count(op, rhs) {
                    return self.error_type;
                }
                if let Some(result) = self.bits_binary(op, lhs_ty, rhs_ty, lhs.span) {
                    return result;
                }
                if let Some(result) =
                    self.bitwise_range_type(op, lhs, rhs, lhs_ty, rhs_ty, expected, lhs.span)
                {
                    result
                } else {
                    self.integer_bin(lhs_ty, rhs_ty, lhs.span)
                }
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

    /// Operands and admission law of `≅`, `≇`, `≈`, `≉`.
    ///
    /// A written constant beside a decimal peer joins the decimal family by
    /// exact value before the aggregate and value-comparison rules run.
    fn check_approximate_equality_binary(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let lhs_ty = self.check_expr(lhs);
        let rhs_ty = match self.check_constant_beside_decimal(rhs, lhs_ty) {
            Some(ty) => ty,
            None => self.check_expr(rhs),
        };
        let lhs_ty = self
            .check_constant_beside_decimal(lhs, rhs_ty)
            .unwrap_or(lhs_ty);
        if let Some(result) = self.check_lista_compare(op, lhs_ty, rhs_ty, lhs.span) {
            return result;
        }
        if let Some(result) = self.check_tensor_lift_compare(op, lhs, rhs, lhs_ty, rhs_ty) {
            return result;
        }
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq)
            && let Some(result) = self.check_approx_aggregate(op, lhs_ty, rhs_ty, lhs.span)
        {
            return result;
        }
        if matches!(op, HirBinOp::ApproxEq | HirBinOp::ApproxNotEq)
            && self.is_instans(lhs_ty)
            && self.is_instans(rhs_ty)
        {
            self.instans_join(lhs_ty, rhs_ty, lhs.span);
            return self.bool_type();
        }
        if !self.word_value_comparison(lhs, lhs_ty, rhs, rhs_ty)
            && !self.policy_float_constant_comparison(lhs, lhs_ty, rhs, rhs_ty)
        {
            // The numeric requirement is the operator's admission law for the
            // fuzzy glyphs only, not a fallback for mixed types: `≈`/`≉` have
            // no MIR primitive (`mir_bin_op` returns None), so a same-typed
            // non-numeric pair (text, bivalens, enum, tuple) must be rejected
            // here rather than fall through to `numeric_value_equality_types`,
            // whose equal-type early return would admit it and defer the
            // failure to MIR validation. `≅`/`≇` lower to `Eq`/`NotEq` and keep
            // whole-value equality on same-typed non-numeric operands.
            let lhs_resolved = self.resolve_inferred_type(lhs_ty);
            let rhs_resolved = self.resolve_inferred_type(rhs_ty);
            let unresolved = self.is_infer(lhs_resolved) || self.is_infer(rhs_resolved);
            if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq)
                && lhs_resolved != self.error_type
                && rhs_resolved != self.error_type
                && !unresolved
                && (!self.is_numeric(lhs_ty) || !self.is_numeric(rhs_ty))
            {
                self.invalid_operand_types(NUMERIC_VALUE_EQUALITY_OPERANDS_REQUIRED, lhs.span);
            } else {
                self.numeric_value_equality_types(lhs_ty, rhs_ty, lhs.span);
            }
        }
        self.bool_type()
    }

    /// Operands and admission law of `≡`/`≢`, `est`/`non est`.
    ///
    /// `nihil` peers and written-constant exemptions skip the exact-type
    /// rule; every other pair compares its carrier exactly.
    fn check_equality_binary(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let (lhs_ty, rhs_ty) = self.check_littera_operands(lhs, rhs, false);
        if let Some(result) = self.bits_binary(op, lhs_ty, rhs_ty, lhs.span) {
            return result;
        }
        if !matches!(op, HirBinOp::Is | HirBinOp::IsNot)
            && let Some(result) = self.check_tensor_lift_compare(op, lhs, rhs, lhs_ty, rhs_ty)
        {
            return result;
        }
        let lhs_is_nil = matches!(
            self.types.get(self.resolve_inferred_type(lhs_ty)),
            Type::Primitive(Primitive::Nihil)
        );
        let rhs_is_nil = matches!(
            self.types.get(self.resolve_inferred_type(rhs_ty)),
            Type::Primitive(Primitive::Nihil)
        );
        if !(lhs_is_nil
            || rhs_is_nil
            || self.equality_constant_exempt(lhs, lhs_ty, rhs, rhs_ty)
            || self.policy_float_constant_comparison(lhs, lhs_ty, rhs, rhs_ty)
            || self.word_pair(lhs, rhs))
        {
            self.exact_equality_types_for_operands(lhs, lhs_ty, rhs, rhs_ty, lhs.span);
        }
        self.bool_type()
    }

    /// Operands and comparison join of `<`, `>`, `≤`, `≥`.
    ///
    /// Collection, tensor, contract, and text orderings each have their own
    /// adapter; the scalar remainder compares exact numeric values through
    /// [`Self::numeric_ordering_bin`].
    fn check_ordering_binary(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let (lhs_ty, rhs_ty) = self.check_littera_operands(lhs, rhs, true);
        let lhs_carrier = self.peel_nominal_aliases(lhs_ty);
        let lhs_carrier = self.types.peel_access_marker(lhs_carrier).1;
        let lhs_carrier = self.peel_nominal_aliases(lhs_carrier);
        let rhs_carrier = self.peel_nominal_aliases(rhs_ty);
        let rhs_carrier = self.types.peel_access_marker(rhs_carrier).1;
        let rhs_carrier = self.peel_nominal_aliases(rhs_carrier);
        if matches!(
            self.types.get(lhs_carrier),
            Type::Primitive(Primitive::Ignotum)
        ) || matches!(
            self.types.get(rhs_carrier),
            Type::Primitive(Primitive::Ignotum)
        ) {
            return self.types.primitive(Primitive::Ignotum);
        }
        if let Some(result) = self.bits_binary(op, lhs_ty, rhs_ty, lhs.span) {
            return result;
        }
        if let Some(result) = self.check_lista_compare(op, lhs_ty, rhs_ty, lhs.span) {
            return result;
        }
        if let Some(result) = self.check_tensor_lift_compare(op, lhs, rhs, lhs_ty, rhs_ty) {
            return result;
        }
        if let Some(result) = self.check_contract_ordering(op, lhs, rhs, lhs_ty, rhs_ty) {
            // D1.4: genus/parameter glyphs call `compare`; tuples
            // order lexicographically.
            return result;
        }
        if let Some(result) = self.check_ordo_order_compare(lhs, lhs_ty, rhs_ty) {
            // Operator ruling 2026-10-15: two values of one
            // `ordo` order by their declared discriminants.
            return result;
        }
        if self.is_instans(lhs_ty) && self.is_instans(rhs_ty) {
            self.instans_join(lhs_ty, rhs_ty, lhs.span);
        } else if (self.is_textus(lhs_ty) || self.is_bounded_textus(lhs_ty))
            && (self.is_textus(rhs_ty) || self.is_bounded_textus(rhs_ty))
            || (self.is_ascii(lhs_ty) || self.is_bounded_ascii(lhs_ty))
                && (self.is_ascii(rhs_ty) || self.is_bounded_ascii(rhs_ty))
        {
            // D1.6: whole-string text order is Unicode code-point order; bounded cords
            // use the same law as their unbounded carrier.
        } else if self.scalar_ordered_text_bin(lhs, rhs, lhs_ty, rhs_ty, lhs.span) {
            // Single-scalar Unicode order (Seam B); not lexicographic cord compare.
        } else if self.word_pair(lhs, rhs) {
            // Both sides are values of one `saturating` word, computed
            // exactly: they compare by value.
        } else if self.word_value_comparison(lhs, lhs_ty, rhs, rhs_ty) {
            // A `wrapping`/`saturating` word beside a constant or a
            // plain integer: exact values (rulings 12, 22).
        } else if self.policy_float_constant_comparison(lhs, lhs_ty, rhs, rhs_ty) {
            // A float cell beside a written constant: exact values.
        } else {
            self.numeric_ordering_bin(lhs_ty, rhs_ty, lhs.span);
        }
        self.bool_type()
    }

    /// Admission law for `≺ ≻ ≤ ≥` over two values of one `ordo` enum.
    ///
    /// Operator ruling 2026-10-15: two values of the same
    /// `ordo` order by their declared discriminants, not by declaration
    /// position. The resolved rows are recorded on the analyzed unit keyed by
    /// the comparison's left-operand id, mirroring how `tensor_edge_policies`
    /// is filled; lowering converts both operands to those integers. A
    /// different enum on either side, or a string-valued `ordo` with no
    /// numeric discriminants, keeps the ordinary mismatch path.
    fn check_ordo_order_compare(
        &mut self,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let ordo = self.ordo_def(lhs_ty)?;
        if self.ordo_def(rhs_ty) != Some(ordo) {
            return None;
        }
        let facts = self.ordo_members.get(&ordo)?;
        let mut next = 0i64;
        let mut rows = Vec::with_capacity(facts.len());
        for fact in facts {
            let value = match &fact.value {
                Some(radix_hir::HirOrdoValue::Integer(value)) => *value,
                Some(radix_hir::HirOrdoValue::String(_)) => return None,
                None => next,
            };
            next = value.wrapping_add(1);
            rows.push((fact.variant, value));
        }
        self.tensor_edge_policies
            .ordo_order
            .comparisons
            .insert(lhs.id, rows);
        Some(self.bool_type())
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
                let (result, result_shape) =
                    super::tensor_index::tensor_with_fresh_shape_hole(self, elem);
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
        if let (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) = (
            self.types.get(lhs_resolved).clone(),
            self.types.get(rhs_resolved).clone(),
        ) {
            let Some(_) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                return self.error_type;
            };
            let Some((lhs_n, rhs_n)) = self.glyph_vector_widths(op, lhs_width, rhs_width, lhs.span)
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
        } else {
            self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
            self.error_type
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
        if let (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) = (
            self.types.get(lhs_resolved).clone(),
            self.types.get(rhs_resolved).clone(),
        ) {
            let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
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
            // product symbolic — `vector<f32, size N> ⊗ vector<f32, size 1>` is
            // `matrix<f32, [N, 1]>` instead of demanding literals.
            if let Some(shape) = self.glyph_symbolic_outer_shape(lhs_width, rhs_width) {
                return self.types.matrix_with_shape(elem, shape);
            }
            self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
            self.error_type
        } else {
            self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
            self.error_type
        }
    }

    /// Numeric `+`/`-` on tensor / vector / matrix require equal shapes.
    ///
    /// Tensor glyphs do not inherit the unretired methods' broadcast contract.
    /// `None` leaves scalar arithmetic and scalar lifting to their own paths.
    fn check_addita_plus(
        &mut self,
        op: HirBinOp,
        lhs: TypeId,
        rhs: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        // An access marker (`copy`, `own`, `ref`, `mut`) is a binding fact, not
        // part of the operand's carrier: peel it so a marked tensor adds like an
        // unmarked one.
        let lhs_resolved = self.types.peel_access_marker(lhs_resolved).1;
        let rhs_resolved = self.types.peel_access_marker(rhs_resolved).1;
        match (
            self.types.get(lhs_resolved).clone(),
            self.types.get(rhs_resolved).clone(),
        ) {
            (Type::Tensor(lhs_elem, lhs_shape), Type::Tensor(rhs_elem, rhs_shape)) => Some(
                self.check_tensor_addita_plus(op, lhs_shape, rhs_shape, lhs_elem, rhs_elem, span),
            ),
            (Type::Vector(lhs_elem, _), Type::Vector(rhs_elem, _)) => {
                Some(self.check_same_shape_addita_plus(
                    op,
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
                    op,
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
        op: HirBinOp,
        lhs_shape: IndexId,
        rhs_shape: IndexId,
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
        // The element takes the scalar range rule for `op` (operator ruling
        // 2026-10-15): `tu8 + tu8` is `tu16`, `tu8 - tu8` is `ti16`, and the
        // width applies only at the store.
        let elem = self.lift_range_element(op, (lhs_elem, None), (rhs_elem, None), span);
        if self.is_error(elem) {
            return self.error_type;
        }
        let before = self.errors.len();
        self.constrain_index_equal(lhs_shape, rhs_shape, span);
        if self.errors.len() != before {
            return self.error_type;
        }
        let shape = self.resolve_index(lhs_shape);
        self.types.intern(Type::Tensor(elem, shape))
    }

    #[allow(clippy::too_many_arguments)]
    fn check_same_shape_addita_plus(
        &mut self,
        op: HirBinOp,
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
        // The element takes the scalar range rule for `op` and the width
        // applies only at the store (operator ruling 2026-10-15).
        let elem = self.lift_range_element(op, (lhs_elem, None), (rhs_elem, None), span);
        if self.is_error(elem) {
            return self.error_type;
        }
        if self.types.equals(elem, lhs_elem) {
            return lhs_ty;
        }
        let Some(register) = self.register_operand(lhs_ty) else {
            return lhs_ty;
        };
        self.register_result(&register, elem)
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
                let Some(elem) = self.glyph_tensor_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                // Symbolic tuple shapes and whole-shape params unify exactly;
                // only an explicit expanded view may repair a shape mismatch.
                let (result, result_shape) =
                    super::tensor_index::tensor_with_fresh_shape_hole(self, elem);
                let lhs_shape = self.resolve_index(lhs_shape);
                let rhs_shape = self.resolve_index(rhs_shape);
                let mut unifier = self.index_infer.unifier(self.types);
                if let Ok(merged) = unifier.unify(lhs_shape, rhs_shape) {
                    self.constrain_index_equal(result_shape, merged, lhs.span);
                    result
                } else {
                    self.glyph_invalid_operand(op, GLYPH_HADAMARD_SHAPES_MISMATCH, lhs.span);
                    self.error_type
                }
            }
            (
                Type::Array(..) | Type::BoundedArray { .. } | Type::RangedArray { .. },
                Type::Array(..) | Type::BoundedArray { .. } | Type::RangedArray { .. },
            ) => self
                .check_lista_hadamard(op, lhs_resolved, rhs_resolved, lhs.span)
                .unwrap_or(self.error_type),
            _ => self.check_glyph_hadamard_fallback(op, lhs_resolved, rhs_resolved, lhs.span),
        }
    }

    /// `⊙`/`⊘` outside the vector/matrix/tensor arms: a queue/stack pair
    /// takes the elementwise result; everything else reports the aggregate
    /// scalar peer when one side is an aggregate and the other numeric, or
    /// the unsupported-operand diagnostic.
    fn check_glyph_hadamard_fallback(
        &mut self,
        op: HirBinOp,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        if let Some(result) = self.check_queue_stack_hadamard(op, lhs_ty, rhs_ty, span) {
            return result;
        }
        let kinds = (
            self.glyph_aggregate_kind(lhs_ty),
            self.glyph_aggregate_kind(rhs_ty),
        );
        let scalar_peer = match kinds {
            (Some(kind), None) if self.is_numeric_scalar(rhs_ty) => Some(kind),
            (None, Some(kind)) if self.is_numeric_scalar(lhs_ty) => Some(kind),
            _ => None,
        };
        match scalar_peer {
            Some(kind) => self.invalid_operand_types_with_args(
                GLYPH_SCALAR_OPERAND,
                span,
                &[
                    ("op", op.glyph_spelling().unwrap_or("?").to_owned()),
                    ("kind", kind.to_owned()),
                ],
            ),
            None => self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, span),
        }
        self.error_type
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
        // — `Vector<3> ⊗ Vector<2>` dispatches on the builtin carriers. The
        // declared `copy`/`mut`/`own` access marker on a carrier (`varia
        // tensor<f32, [2, 2]> acc ← copy w`) is an acquisition fact, not part
        // of the value shape, so peel it like the call and access paths do.
        let lhs_resolved = self.value_carrier_type(lhs_ty);
        let rhs_resolved = self.value_carrier_type(rhs_ty);
        let (_, lhs_resolved) = self.types.peel_access_marker(lhs_resolved);
        let (_, rhs_resolved) = self.types.peel_access_marker(rhs_resolved);
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

    /// Element of a tensor `⊙`/`⊘` (F9): numeric elements of one family join
    /// (identical elements stay); a family crossing is the glyph's element
    /// mismatch. Vector and matrix `⊙` keep the strict rule above.
    fn glyph_tensor_element(
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
        if self.types.equals(lhs_elem, rhs_elem) {
            return Some(lhs_elem);
        }
        let one_family = matches!(
            (self.numeric_shape(lhs_elem), self.numeric_shape(rhs_elem)),
            (Some(l), Some(r)) if l.prim == r.prim && l.width.is_decimal() == r.width.is_decimal()
        );
        if !one_family {
            self.glyph_invalid_operand(op, GLYPH_ELEMENT_TYPE_MISMATCH, span);
            return None;
        }
        let joined = self.lift_join_element((lhs_elem, None), (rhs_elem, None), span);
        (!self.is_error(joined)).then_some(joined)
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
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> (TypeId, TypeId) {
        // A shift count is an integer of any width, never a `bN`.
        let shift = matches!(op, HirBinOp::Shl | HirBinOp::Shr);
        if !self.current_device_function {
            match (
                integer_range::is_const_int(lhs),
                integer_range::is_const_int(rhs),
            ) {
                (true, true) if !self.const_pair_is_legacy(expected) => {
                    return (self.check_const_operand(lhs), self.check_const_operand(rhs));
                }
                (true, false) => {
                    let rhs_ty = self.check_expr(rhs);
                    if !shift && let Some(lhs_ty) = self.bits_peer_literal(lhs, rhs_ty) {
                        return (lhs_ty, rhs_ty);
                    }
                    return (self.check_const_beside(lhs, rhs_ty, expected), rhs_ty);
                }
                (false, true) => {
                    let lhs_ty = self.check_expr(lhs);
                    if self.bits_width_of(lhs_ty).is_some() {
                        if shift {
                            return (lhs_ty, self.check_expr(rhs));
                        }
                        if let Some(rhs_ty) = self.bits_peer_literal(rhs, lhs_ty) {
                            return (lhs_ty, rhs_ty);
                        }
                    }
                    return (lhs_ty, self.check_const_beside(rhs, lhs_ty, expected));
                }
                _ => {}
            }
        }
        if is_numeric_literal(lhs) && !is_numeric_literal(rhs) {
            let rhs_ty = self.check_expr(rhs);
            if !shift && let Some(lhs_ty) = self.bits_peer_literal(lhs, rhs_ty) {
                return (lhs_ty, rhs_ty);
            }
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
        if !shift && let Some(rhs_ty) = self.bits_peer_literal(rhs, lhs_ty) {
            return (lhs_ty, rhs_ty);
        }
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
        let failable_before = self.failable_calls_checked;
        let lhs_ty = self.check_expr(lhs);
        // `vel`/`coalesce` supplies a value for an absent (`T ∪ none`) operand.
        // A failable call whose success type is not optional fails through its
        // `⇥` channel, which the optional default never handles: `⊥` does.
        if self.failable_calls_checked > failable_before
            && matches!(
                lhs.kind,
                HirExpressionKind::Call(..) | HirExpressionKind::MethodCall(..)
            )
            && !matches!(
                self.types.get(self.resolve_inferred_type(lhs_ty)),
                Type::Option(_) | Type::Primitive(Primitive::Nihil)
            )
            && self.resolve_inferred_type(lhs_ty) != self.error_type
        {
            self.invalid_operand_types(ISSUE_COALESCE_ON_FAILABLE_CALL, lhs.span);
        }
        if let Some(result) = self.check_tensor_coalesce(lhs_ty, lhs.span, rhs) {
            return result;
        }
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
                self.common_type_with_consts(
                    (lhs_ty, None),
                    (rhs_ty, Self::const_join_range(rhs)),
                    lhs.span,
                )
            }
        }
    }
}

impl TypeChecker<'_> {
    /// The unified membership operator (`∈` / `∉`).
    ///
    /// The parser cannot tell a range from a collection, so it records the
    /// placeholder `Between` / `NotBetween`. The right operand's static type
    /// decides: an `intervallum` retargets the node in place to `InRange` /
    /// `NotInRange` and takes the range rule (numeric left, element unified
    /// with the bound); every other right operand keeps the collection variant
    /// and takes [`Self::check_membership_rhs`]. The recorded operator is the
    /// resolved one, which is what MIR, AIR and every emitter read.
    pub(super) fn check_membership(
        &mut self,
        op: &mut HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let lhs_ty = self.check_expr(lhs);
        let rhs_ty = self.check_expr(rhs);
        if self.intervallum_bound_type(rhs_ty).is_some() {
            *op = match *op {
                HirBinOp::NotBetween => HirBinOp::NotInRange,
                _ => HirBinOp::InRange,
            };
            self.check_intra_operand(lhs_ty, rhs_ty, lhs.span, rhs.span);
        } else {
            self.check_membership_rhs(lhs_ty, rhs_ty, lhs.span, rhs.span);
        }
        self.bool_type()
    }

    /// The right-operand rule of the collection form of membership.
    ///
    /// After peeling aliases and access markers the right operand must be a
    /// `lista` (bounded or not), a `copia` or a `tabula` (membership by key).
    /// A `textus` / `ascii` right operand (bounded or not) is rejected with
    /// `membership_over_text`: membership never applies to text (operator
    /// ruling OD-1), the text method does. Every other resolved type is
    /// rejected with `membership_requires_collection`. Both carry the right
    /// operand's type as `found`.
    ///
    /// A right operand that is not resolved enough to test membership against
    /// (an inference variable, union hole, marker hole, error type, or a
    /// container whose element or key type is still unresolved, such as `[]`)
    /// is reported once as `membership_operand_unresolved` at the right
    /// operand, before the left operand can bind the hole: nothing guesses a
    /// type for it and nothing unresolved reaches lowering.
    pub(super) fn check_membership_rhs(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        lhs_span: radix_lexer::Span,
        rhs_span: radix_lexer::Span,
    ) {
        let (_, payload) = self
            .types
            .peel_access_marker(self.resolve_inferred_type(rhs_ty));
        let payload = self.resolve_inferred_type(payload);
        let shape = self.types.get(payload).clone();
        let probed = match &shape {
            Type::Array(elem)
            | Type::Set(elem)
            | Type::BoundedArray { element: elem, .. }
            | Type::RangedArray { element: elem, .. } => Some(*elem),
            Type::Map(key, _) | Type::Ledger(key, _) => Some(*key),
            _ => None,
        };
        let unresolved = match probed {
            Some(probed) => self.is_unresolved_membership_type(probed, false),
            None => self.is_unresolved_membership_type(payload, true),
        };
        if unresolved {
            self.reject_membership_operand(MEMBERSHIP_OPERAND_UNRESOLVED, rhs_ty, rhs_span);
            return;
        }
        match shape {
            Type::Array(elem)
            | Type::Set(elem)
            | Type::BoundedArray { element: elem, .. }
            | Type::RangedArray { element: elem, .. } => {
                self.unify_assign_issue(lhs_ty, elem, lhs_span, MEMBERSHIP_OPERAND_TYPE_MISMATCH);
            }
            Type::Map(key, _) | Type::Ledger(key, _) => {
                self.unify_assign_issue(lhs_ty, key, lhs_span, MEMBERSHIP_OPERAND_TYPE_MISMATCH);
            }
            Type::Primitive(Primitive::Textus | Primitive::Ascii)
            | Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. } => {
                // OD-1: no membership operator over text; the text method is
                // the one spelling for substring containment.
                self.reject_membership_operand(MEMBERSHIP_OVER_TEXT, rhs_ty, rhs_span);
            }
            _ => self.reject_membership_operand(MEMBERSHIP_REQUIRES_COLLECTION, rhs_ty, rhs_span),
        }
    }

    /// Report a rejected membership right operand with its type as `found`.
    fn reject_membership_operand(
        &mut self,
        issue: &'static str,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) {
        let found = self.format_type_for_diagnostic(rhs_ty);
        self.error_issue_with_args(
            SemanticErrorKind::TypeMismatch,
            issue,
            span,
            &[("found", found)],
        );
    }

    /// Whether a membership right operand (or its element or key type) is an
    /// unresolved inference state rather than a type. `error_counts` adds the
    /// error type, which only the right operand itself reports.
    fn is_unresolved_membership_type(&self, ty: TypeId, error_counts: bool) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved) {
            Type::Infer(_) | Type::InferUnion(_) | Type::MarkerHole { .. } => true,
            Type::Error => error_counts,
            _ => false,
        }
    }
}

pub(super) fn is_numeric_literal(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Literal(
            HirLiteral::Int(_)
            | HirLiteral::BigInt(_)
            | HirLiteral::Float(_)
            | HirLiteral::FloatText { .. },
        ) => true,
        // A negated literal (`-1.0`) is still a literal for propagation purposes.
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => is_numeric_literal(inner),
        _ => false,
    }
}

const NUMERIC_OPERANDS_REQUIRED: &str = "numeric_operands_required";
const TENSOR_MULTIPLY_REQUIRES_GLYPH: &str = "tensor_multiply_requires_glyph";
pub(super) const NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL: &str =
    "numeric_arithmetic_operands_identical";
pub(super) const MODULAR_WORD_FAMILY_MISMATCH: &str = "modular_word_family_mismatch";
pub(super) const SATURATING_WORD_FAMILY_MISMATCH: &str = "saturating_word_family_mismatch";
/// A typed mix of a `trapping<W>` or `saturating<W>` float cell with a bare
/// float, the other cell, or another width (N7e D4).
pub(super) const POLICY_FLOAT_FAMILY_MISMATCH: &str = "policy_float_family_mismatch";
/// A method call on a float cell: the cell's math surface is not built yet
/// (N7e D3, interim fail-closed).
pub(super) const POLICY_FLOAT_MATH_NOT_BUILT: &str = "policy_float_math_not_built";

fn either_policy(lhs: NumericShape, rhs: NumericShape) -> bool {
    [lhs, rhs]
        .iter()
        .any(|shape| matches!(shape.family, IntFamily::Policy(_)))
}

/// Mixed integer families name the saturating family when it is involved,
/// otherwise the modular family (both require an explicit `↦`).
fn family_mismatch_issue(lhs: Option<NumericShape>, rhs: Option<NumericShape>) -> &'static str {
    if [lhs, rhs]
        .into_iter()
        .flatten()
        .any(|shape| matches!(shape.family, IntFamily::Policy(_)))
    {
        return POLICY_FLOAT_FAMILY_MISMATCH;
    }
    let saturating = [lhs, rhs]
        .into_iter()
        .flatten()
        .any(|shape| shape.family == IntFamily::Saturating);
    if saturating {
        SATURATING_WORD_FAMILY_MISMATCH
    } else {
        MODULAR_WORD_FAMILY_MISMATCH
    }
}
// TENSOR-GLYPHS-1B rank-rule diagnostics. Each carries `op` (the glyph
// spelling) so renderers and tests identify the operator without prose.
const GLYPH_OPERANDS_UNSUPPORTED: &str = "glyph_operands_unsupported";
/// `⊙`/`⊘` with an aggregate on one side and a scalar on the other.
const GLYPH_SCALAR_OPERAND: &str = "glyph_scalar_operand";
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
const MEMBERSHIP_REQUIRES_COLLECTION: &str = "membership_requires_collection";
const MEMBERSHIP_OPERAND_UNRESOLVED: &str = "membership_operand_unresolved";
const MEMBERSHIP_OVER_TEXT: &str = "membership_over_text";
const COALESCE_FALLBACK_TYPE_MISMATCH: &str = "coalesce_fallback_type_mismatch";
const ISSUE_COALESCE_ON_FAILABLE_CALL: &str = "coalesce_on_failable_call";
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
pub(super) struct NumericShape {
    pub(super) prim: Primitive,
    pub(super) width: NumericWidth,
    pub(super) family: IntFamily,
}

/// Overflow policy family of a numeric shape: trap, wrap, or clamp for an
/// integer (D11.1), or the policy of a `trapping<W>` / `saturating<W>` float
/// cell (N7e D5). A cell is never `Plain`, so every range rule that is keyed
/// on the plain family leaves it alone.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum IntFamily {
    Plain,
    Modular,
    Saturating,
    Policy(radix_types::FloatPolicy),
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

fn lista_operator_spelling(op: HirBinOp) -> &'static str {
    match op {
        HirBinOp::Add => "+",
        HirBinOp::Sub => "-",
        HirBinOp::Mul => "*",
        HirBinOp::Div => "/",
        HirBinOp::Mod => "%",
        HirBinOp::TrueDiv => "÷",
        HirBinOp::Hadamard => "⊙",
        HirBinOp::HadamardDiv => "⊘",
        HirBinOp::Lt => "≺",
        HirBinOp::Gt => "≻",
        HirBinOp::LtEq => "≤",
        HirBinOp::GtEq => "≥",
        HirBinOp::ApproxEq => "≅",
        HirBinOp::ApproxNotEq => "≇",
        HirBinOp::And => "and",
        HirBinOp::Or => "or",
        _ => unreachable!("non-list operator"),
    }
}

/// Whether a written literal of `family` takes a numeric slot or peer of
/// `shape` (F9 ruling 30: digit text joins a decimal by exact value).
fn literal_joins_shape(expr: &HirExpression, family: Primitive, shape: &NumericShape) -> bool {
    shape.prim == family || (shape.width.is_decimal() && is_decimal_text_literal(expr))
}

/// A source-written fractional literal (`0.1`, `-2.5`): it carries its digits.
fn is_decimal_text_literal(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::FloatText { .. }) => true,
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => is_decimal_text_literal(inner),
        _ => false,
    }
}

pub(super) fn numeric_literal_family(expr: &HirExpression) -> Option<Primitive> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(_) | HirLiteral::BigInt(_)) => {
            Some(Primitive::Numerus)
        }
        HirExpressionKind::Literal(HirLiteral::Float(_) | HirLiteral::FloatText { .. }) => {
            Some(Primitive::Fractus)
        }
        // A negated literal (`-1.0`) inherits the inner literal's family so
        // that peer-type propagation in arithmetic recognizes it.
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => numeric_literal_family(inner),
        _ => None,
    }
}
