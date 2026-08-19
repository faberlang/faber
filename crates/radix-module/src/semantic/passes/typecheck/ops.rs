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
const ISSUE_CONVERSIO_ASSIGN_INFERENCE_HOLE_DESTINATION: &str = "conversio_assign_inference_hole_destination";

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
    pub(super) fn numeric_bin(&mut self, lhs: TypeId, rhs: TypeId, span: crate::lexer::Span) -> TypeId {
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

    fn numeric_arithmetic_bin(&mut self, lhs: TypeId, rhs: TypeId, span: crate::lexer::Span) -> TypeId {
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
                return self.unify_assign_issue(lhs, rhs_resolved, span, NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL);
            }
            self.invalid_operand_types(NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL, span);
            return self.error_type;
        }
        if self.is_infer(rhs_resolved) {
            if self.numeric_shape(lhs_resolved).is_some() {
                return self.unify_assign_issue(rhs, lhs_resolved, span, NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL);
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

    fn numeric_shape(&self, ty: TypeId) -> Option<NumericShape> {
        match self.types.get(ty) {
            Type::Primitive(Primitive::Numerus) => {
                Some(NumericShape { prim: Primitive::Numerus, width: NumericWidth::I64, modular: false })
            }
            Type::Primitive(Primitive::Fractus) => {
                Some(NumericShape { prim: Primitive::Fractus, width: NumericWidth::F64, modular: false })
            }
            Type::SizedNumeric(prim @ (Primitive::Numerus | Primitive::Fractus), width) => {
                Some(NumericShape { prim: *prim, width: *width, modular: false })
            }
            Type::ModularWord(width) => Some(NumericShape { prim: Primitive::Numerus, width: *width, modular: true }),
            Type::Alias(_, target) => self.numeric_shape(*target),
            _ => None,
        }
    }

    fn preferred_numeric_result_ty(&mut self, lhs: TypeId, rhs: TypeId, shape: NumericShape) -> TypeId {
        if matches!(self.types.get(lhs), Type::SizedNumeric(_, _) | Type::ModularWord(_)) {
            return lhs;
        }
        if matches!(self.types.get(rhs), Type::SizedNumeric(_, _) | Type::ModularWord(_)) {
            return rhs;
        }
        match shape.prim {
            Primitive::Numerus => self.numerus_type(),
            Primitive::Fractus => self.fractus_type(),
            _ => self.error_type,
        }
    }

    fn bool_bin(&mut self, lhs: TypeId, rhs: TypeId, span: crate::lexer::Span) -> TypeId {
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

    /// Allow cross-precision `instans` operands for ordering only.
    ///
    /// WHY: ordering compares at the coarser declared precision; operands need
    /// not unify to one precision marker in the type table. Equality uses
    /// [`Self::exact_equality_types`] instead.
    fn instans_join(&mut self, lhs: TypeId, rhs: TypeId, span: crate::lexer::Span) -> TypeId {
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

    /// Enforce exact-type match for `≡`, `≠`, `est`, and `non est`.
    ///
    /// WHY: assignability may widen across representations; equality must not
    /// erase deliberate annotations (`ascii`, sized numerics, precision-tagged
    /// `instans`, …). Authors widen explicitly with `↦` before comparing.
    /// Enforce numeric promotion join for `≈` and `≉`.
    ///
    /// WHY: exact `≡` rejects cross-width numeric pairs; `≈` reuses the same
    /// widening policy as arithmetic/ordering (`numeric_bin`) then compares at
    /// the joined type in codegen.
    pub(super) fn numeric_value_equality_types(&mut self, lhs: TypeId, rhs: TypeId, span: crate::lexer::Span) {
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
        self.numeric_bin(lhs, rhs, span);
    }

    pub(super) fn exact_equality_types(&mut self, lhs: TypeId, rhs: TypeId, span: crate::lexer::Span) {
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
            self.error_issue(SemanticErrorKind::TypeMismatch, EQUALITY_EXACT_TYPES_REQUIRED, span);
        }
    }

    fn integer_bin(&mut self, lhs: TypeId, rhs: TypeId, span: crate::lexer::Span) -> TypeId {
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
    pub(super) fn common_type(&mut self, a: TypeId, b: TypeId, span: crate::lexer::Span) -> TypeId {
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
        if let Type::Option(_) = self.types.get(left) {
            if self.types.assignable(right, left) {
                return left;
            }
        }
        if let Type::Option(_) = self.types.get(right) {
            if self.types.assignable(left, right) {
                return right;
            }
        }

        if self.types.assignable(left, right) {
            return right;
        }
        if self.types.assignable(right, left) {
            return left;
        }
        if self.is_numeric(left) && self.is_numeric(right) {
            return self.numeric_bin(left, right, span);
        }
        self.error_issue(SemanticErrorKind::TypeMismatch, COMMON_TYPE_MISMATCH, span);
        self.error_type
    }

    /// Check postfix increment/decrement (`place ↑` / `place ↓`).
    pub(super) fn check_inc_dec(
        &mut self,
        _op: crate::hir::HirIncDecOp,
        target: &mut HirExpression,
        span: crate::lexer::Span,
    ) {
        let target_ty = self.check_lvalue(target);
        if !self.is_integer(target_ty) {
            self.invalid_operand_types(POSTFIX_INC_DEC_NUMERUS_OPERAND, span);
        }
    }

    /// Check simple assignment and constrain the value from the target context.
    pub(super) fn check_assign(&mut self, target: &mut HirExpression, value: &mut HirExpression) -> TypeId {
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
        self.unify_assign_issue(value_ty, target_ty, value.span, ASSIGNMENT_TYPE_MISMATCH);
        target_ty
    }

    /// Check conversion-directed assignment (`target ↤ value`, U+21A4).
    ///
    /// The left side goes through the ordinary lvalue rules: mutable place
    /// with a resolved static type, no inference from the right value. The
    /// right side is checked through the **same conversion registry** as an
    /// explicit `value ↦ TargetType` ([`Self::check_conversio`], the
    /// `convert.rs` deny-list + allow-list) using the place's resolved type as
    /// the destination. Inline `⇥` recovery is typed before the store and must
    /// satisfy the same target-type rule as explicit `↦` recovery. The result
    /// type is the destination type (the stored value's type), matching
    /// ordinary assignment.
    pub(super) fn check_conversio_assign(
        &mut self,
        target: &mut HirExpression,
        source: &mut HirExpression,
        recovery: Option<&mut HirExpression>,
        span: crate::lexer::Span,
    ) -> TypeId {
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
        self.check_conversio(source, &mut conversion_target, &[], recovery, span);
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

    fn destination_type_has_inference_hole_inner(&self, ty: TypeId, seen: &mut FxHashSet<TypeId>) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        if !seen.insert(resolved) {
            return false;
        }
        match self.types.get(resolved) {
            Type::Infer(_) => true,
            Type::Array(inner)
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
            Type::Ref(_, inner) | Type::Alias(_, inner) => self.destination_type_has_inference_hole_inner(*inner, seen),
            Type::Union(types) => types
                .iter()
                .any(|member| self.destination_type_has_inference_hole_inner(*member, seen)),
            Type::Tuple(types) => types
                .iter()
                .any(|member| self.destination_type_has_inference_hole_inner(*member, seen)),
            Type::Applied(base, args) => {
                self.destination_type_has_inference_hole_inner(*base, seen)
                    || args
                        .iter()
                        .any(|arg| self.destination_type_has_inference_hole_inner(*arg, seen))
            }
            _ => false,
        }
    }

    fn is_shader_vec3_to_vec4_output_target(&self, target: &HirExpression, target_ty: TypeId) -> bool {
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
    pub(super) fn check_unary(
        &mut self,
        op: crate::hir::HirUnOp,
        operand: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        match op {
            crate::hir::HirUnOp::Neg => {
                let operand_ty = self.check_expr_with_expected(operand, expected);
                if self.is_error(operand_ty) {
                    return self.error_type;
                }
                if self.is_infer(self.resolve_inferred_type(operand_ty)) {
                    let numerus = self.numerus_type();
                    return self.unify_assign_issue(operand_ty, numerus, operand.span, NUMERIC_OPERAND_REQUIRED);
                }
                if !self.is_numeric(operand_ty) {
                    self.invalid_operand_types(NUMERIC_OPERAND_REQUIRED, operand.span);
                }
                operand_ty
            }
            crate::hir::HirUnOp::Not => {
                let operand_ty = self.check_expr(operand);
                if self.is_error(operand_ty) {
                    return self.bool_type();
                }
                if self.is_infer(self.resolve_inferred_type(operand_ty)) {
                    let bivalens = self.bool_type();
                    self.unify_assign_issue(operand_ty, bivalens, operand.span, BOOLEAN_OPERAND_REQUIRED);
                    return self.bool_type();
                }
                if !self.is_bool(operand_ty) {
                    self.invalid_operand_types(BOOLEAN_OPERAND_REQUIRED, operand.span);
                }
                self.bool_type()
            }
            crate::hir::HirUnOp::BitNot => {
                let operand_ty = self.check_expr_with_expected(operand, expected);
                if self.is_error(operand_ty) {
                    return self.error_type;
                }
                let operand_resolved = self.resolve_inferred_type(operand_ty);
                if self.is_infer(operand_resolved) {
                    let numerus = self.numerus_type();
                    self.unify_assign_issue(operand_ty, numerus, operand.span, INTEGER_OPERAND_REQUIRED);
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
                let lhs_ty = self.check_expr_with_expected(lhs, self.literal_numeric_expected(lhs, expected));
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

    fn literal_numeric_expected(&self, expr: &HirExpression, expected: Option<TypeId>) -> Option<TypeId> {
        let family = numeric_literal_family(expr)?;
        let expected = self.resolve_inferred_type(expected?);
        self.numeric_shape(expected)
            .filter(|shape| shape.prim == family)
            .map(|_| self.numeric_expected_type(expected))
    }

    fn literal_numeric_expected_from_peer(&self, expr: &HirExpression, peer: TypeId) -> Option<TypeId> {
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
    pub(super) fn check_binary(
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
                } else if let Some(result) = self.check_addita_plus(lhs_ty, rhs_ty, lhs.span) {
                    // Numeric tensor/vector/matrix `+` is the live `addita` / `.added()` op.
                    result
                } else {
                    self.numeric_arithmetic_bin(lhs_ty, rhs_ty, lhs.span)
                }
            }
            HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div | HirBinOp::Mod => {
                let (lhs_ty, rhs_ty) = self.check_arithmetic_operands(lhs, rhs, expected);
                self.numeric_arithmetic_bin(lhs_ty, rhs_ty, lhs.span)
            }
            HirBinOp::ApproxEq | HirBinOp::ApproxNotEq => {
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
                    self.exact_equality_types(lhs_ty, rhs_ty, lhs.span);
                }
                self.bool_type()
            }
            HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                if self.is_instans(lhs_ty) && self.is_instans(rhs_ty) {
                    self.instans_join(lhs_ty, rhs_ty, lhs.span);
                } else if self.scalar_ordered_text_bin(lhs, rhs, lhs_ty, rhs_ty, lhs.span) {
                    // Single-scalar Unicode order (Seam B); not lexicographic cord compare.
                } else {
                    self.numeric_bin(lhs_ty, rhs_ty, lhs.span);
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
                        self.unify_assign_issue(lhs_ty, *elem, lhs.span, MEMBERSHIP_OPERAND_TYPE_MISMATCH);
                    }
                    Type::Map(key, _) => {
                        self.unify_assign_issue(lhs_ty, *key, lhs.span, MEMBERSHIP_OPERAND_TYPE_MISMATCH);
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
            HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor | HirBinOp::Shl | HirBinOp::Shr => {
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
            HirBinOp::Hadamard => self.check_glyph_hadamard(op, lhs, rhs),
        }
    }

    /// `·` inner product: contracts the inner dimension(s). vec·vec → scalar;
    /// matrix·matrix and the two matvec forms keep the pinned rank-1 result;
    /// rank-2 tensor·tensor contracts through the matmul shape contract.
    fn check_glyph_dot(&mut self, op: HirBinOp, lhs: &mut HirExpression, rhs: &mut HirExpression) -> TypeId {
        let Some((lhs_resolved, rhs_resolved)) = self.glyph_operand_types(op, lhs, rhs) else {
            return self.error_type;
        };
        match (self.types.get(lhs_resolved).clone(), self.types.get(rhs_resolved).clone()) {
            (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_n, rhs_n)) = self.glyph_vector_widths(op, lhs_width, rhs_width, lhs.span) else {
                    return self.error_type;
                };
                if lhs_n != rhs_n {
                    self.glyph_invalid_operand(op, GLYPH_INNER_DIM_MISMATCH, lhs.span);
                    return self.error_type;
                }
                elem
            }
            (Type::Tensor(lhs_elem, _), Type::Tensor(rhs_elem, _)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                // WHY: shape-generics Phase 2 pin — rank-2 tensor×tensor only
                // (delivery §7 OQ1 default). Routes through the same matmul
                // contract as the `matmul` intrinsic, so whole-shape and
                // non-rank-2 tensor operands fail closed with the contract's
                // diagnostics and the two surfaces can never diverge.
                let result = super::tensor_index::tensor_with_fresh_shape(self, elem);
                let result_shape = match self.types.get(result) {
                    Type::Tensor(_, shape) => *shape,
                    _ => fresh_tensor_result_missing_shape(),
                };
                super::tensor_index::constrain_matmul_result_shape(
                    self,
                    lhs_resolved,
                    rhs_resolved,
                    result_shape,
                    lhs.span,
                );
                result
            }
            (Type::Matrix(lhs_elem, lhs_shape), Type::Matrix(rhs_elem, rhs_shape)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let (Some((lhs_m, lhs_k)), Some((rhs_k, rhs_p))) = (
                    crate::semantic::literal_matrix_shape(lhs_shape, self.types),
                    crate::semantic::literal_matrix_shape(rhs_shape, self.types),
                ) else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                if lhs_k != rhs_k {
                    self.glyph_invalid_operand(op, GLYPH_INNER_DIM_MISMATCH, lhs.span);
                    return self.error_type;
                }
                self.glyph_matrix_type(elem, lhs_m, rhs_p)
            }
            (Type::Matrix(lhs_elem, lhs_shape), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_m, lhs_k)) = crate::semantic::literal_matrix_shape(lhs_shape, self.types) else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                let Some(rhs_n) = crate::semantic::literal_vector_width(rhs_width, self.types) else {
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
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some(lhs_n) = crate::semantic::literal_vector_width(lhs_width, self.types) else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                let Some((rhs_k, rhs_p)) = crate::semantic::literal_matrix_shape(rhs_shape, self.types) else {
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
    fn check_glyph_cross(&mut self, op: HirBinOp, lhs: &mut HirExpression, rhs: &mut HirExpression) -> TypeId {
        let Some((lhs_resolved, rhs_resolved)) = self.glyph_operand_types(op, lhs, rhs) else {
            return self.error_type;
        };
        match (self.types.get(lhs_resolved).clone(), self.types.get(rhs_resolved).clone()) {
            (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(_) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_n, rhs_n)) = self.glyph_vector_widths(op, lhs_width, rhs_width, lhs.span) else {
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
    fn check_glyph_tensor_product(&mut self, op: HirBinOp, lhs: &mut HirExpression, rhs: &mut HirExpression) -> TypeId {
        let Some((lhs_resolved, rhs_resolved)) = self.glyph_operand_types(op, lhs, rhs) else {
            return self.error_type;
        };
        match (self.types.get(lhs_resolved).clone(), self.types.get(rhs_resolved).clone()) {
            (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(elem) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_n, rhs_m)) = self.glyph_vector_widths(op, lhs_width, rhs_width, lhs.span) else {
                    return self.error_type;
                };
                self.glyph_matrix_type(elem, lhs_n, rhs_m)
            }
            _ => {
                self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
                self.error_type
            }
        }
    }

    /// Numeric `+` on tensor / vector / matrix: the live `addita` / `.added()` op.
    ///
    /// One Add arm covers the three families. Tensor broadcast is `addita`'s
    /// `Figura'` (`constrain_elementwise_result_shape`). Vector/matrix follow
    /// their live `addita` same-shape contracts. `None` means this is not an
    /// addita-family pair — scalar arithmetic continues.
    fn check_addita_plus(&mut self, lhs: TypeId, rhs: TypeId, span: crate::lexer::Span) -> Option<TypeId> {
        let lhs_resolved = self.resolve_inferred_type(lhs);
        let rhs_resolved = self.resolve_inferred_type(rhs);
        match (self.types.get(lhs_resolved).clone(), self.types.get(rhs_resolved).clone()) {
            (Type::Tensor(lhs_elem, _), Type::Tensor(rhs_elem, _)) => {
                Some(self.check_tensor_addita_plus(lhs_resolved, rhs_resolved, lhs_elem, rhs_elem, span))
            }
            (Type::Vector(lhs_elem, _), Type::Vector(rhs_elem, _)) => Some(self.check_same_shape_addita_plus(
                lhs_resolved,
                rhs_resolved,
                lhs_elem,
                rhs_elem,
                TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED,
                span,
            )),
            (Type::Matrix(lhs_elem, _), Type::Matrix(rhs_elem, _)) => Some(self.check_same_shape_addita_plus(
                lhs_resolved,
                rhs_resolved,
                lhs_elem,
                rhs_elem,
                MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED,
                span,
            )),
            _ => None,
        }
    }

    fn check_tensor_addita_plus(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        lhs_elem: TypeId,
        rhs_elem: TypeId,
        span: crate::lexer::Span,
    ) -> TypeId {
        if !crate::semantic::numeric_width::is_tensor_numeric_element(self.types, lhs_elem)
            || !crate::semantic::numeric_width::is_tensor_numeric_element(self.types, rhs_elem)
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
        super::tensor_index::constrain_elementwise_result_shape(self, lhs_ty, rhs_ty, result_shape, span);
        result
    }

    fn check_same_shape_addita_plus(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        lhs_elem: TypeId,
        rhs_elem: TypeId,
        non_numeric_issue: &'static str,
        span: crate::lexer::Span,
    ) -> TypeId {
        if !crate::semantic::numeric_width::is_tensor_numeric_element(self.types, lhs_elem)
            || !crate::semantic::numeric_width::is_tensor_numeric_element(self.types, rhs_elem)
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

    /// `⊙` Hadamard: identical built-in shapes produce the identical shape.
    fn check_glyph_hadamard(&mut self, op: HirBinOp, lhs: &mut HirExpression, rhs: &mut HirExpression) -> TypeId {
        let Some((lhs_resolved, rhs_resolved)) = self.glyph_operand_types(op, lhs, rhs) else {
            return self.error_type;
        };
        match (self.types.get(lhs_resolved).clone(), self.types.get(rhs_resolved).clone()) {
            (Type::Vector(lhs_elem, lhs_width), Type::Vector(rhs_elem, rhs_width)) => {
                let Some(_) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_n, rhs_n)) = self.glyph_vector_widths(op, lhs_width, rhs_width, lhs.span) else {
                    return self.error_type;
                };
                self.glyph_same_shape_result(op, lhs_resolved, lhs_n == rhs_n, lhs.span)
            }
            (Type::Matrix(lhs_elem, lhs_shape), Type::Matrix(rhs_elem, rhs_shape)) => {
                let Some(_) = self.glyph_numeric_element(op, lhs_elem, rhs_elem, lhs.span) else {
                    return self.error_type;
                };
                let Some((lhs_rows, lhs_cols)) = crate::semantic::literal_matrix_shape(lhs_shape, self.types) else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                let Some((rhs_rows, rhs_cols)) = crate::semantic::literal_matrix_shape(rhs_shape, self.types) else {
                    self.glyph_invalid_operand(op, GLYPH_STATIC_SHAPE_REQUIRED, lhs.span);
                    return self.error_type;
                };
                self.glyph_same_shape_result(op, lhs_resolved, (lhs_rows, lhs_cols) == (rhs_rows, rhs_cols), lhs.span)
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
        let lhs_resolved = self.resolve_inferred_type(lhs_ty);
        let rhs_resolved = self.resolve_inferred_type(rhs_ty);
        if self.is_infer(lhs_resolved) || self.is_infer(rhs_resolved) {
            self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, lhs.span);
            return None;
        }
        Some((lhs_resolved, rhs_resolved))
    }

    /// Require numeric, strictly equal elements for a glyph product.
    fn glyph_numeric_element(
        &mut self,
        op: HirBinOp,
        lhs_elem: TypeId,
        rhs_elem: TypeId,
        span: crate::lexer::Span,
    ) -> Option<TypeId> {
        if !crate::semantic::numeric_width::is_tensor_numeric_element(self.types, lhs_elem)
            || !crate::semantic::numeric_width::is_tensor_numeric_element(self.types, rhs_elem)
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
        lhs: crate::semantic::IndexId,
        rhs: crate::semantic::IndexId,
        span: crate::lexer::Span,
    ) -> Option<(u8, u8)> {
        let (Some(lhs_n), Some(rhs_n)) = (
            crate::semantic::literal_vector_width(lhs, self.types),
            crate::semantic::literal_vector_width(rhs, self.types),
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
        span: crate::lexer::Span,
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

    fn glyph_invalid_operand(&mut self, op: HirBinOp, issue: &'static str, span: crate::lexer::Span) {
        self.invalid_operand_types_with_args(issue, span, &[("op", op.glyph_spelling().unwrap_or("?").to_owned())]);
    }

    fn check_integer_operands(&mut self, lhs: &mut HirExpression, rhs: &mut HirExpression) -> (TypeId, TypeId) {
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
                        self.unify_assign_issue(rhs_inner, inner, rhs.span, COALESCE_FALLBACK_TYPE_MISMATCH);
                        self.types.option(inner)
                    }
                    Type::Primitive(Primitive::Nihil) => self.types.option(inner),
                    _ => {
                        self.unify_assign_issue(rhs_ty, inner, rhs.span, COALESCE_FALLBACK_TYPE_MISMATCH);
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

fn is_numeric_literal(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(_) | HirLiteral::Float(_)) => true,
        // A negated literal (`-1.0`) is still a literal for propagation purposes.
        HirExpressionKind::Unary(crate::hir::HirUnOp::Neg, inner) => is_numeric_literal(inner),
        _ => false,
    }
}

const NUMERIC_OPERANDS_REQUIRED: &str = "numeric_operands_required";
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
const TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED: &str = "tensor_arithmetic_numeric_element_required";
const MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED: &str = "matrix_arithmetic_numeric_element_required";
const ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";

#[derive(Clone, Copy)]
struct NumericShape {
    prim: Primitive,
    width: NumericWidth,
    modular: bool,
}

fn numeric_literal_family(expr: &HirExpression) -> Option<Primitive> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(_)) => Some(Primitive::Numerus),
        HirExpressionKind::Literal(HirLiteral::Float(_)) => Some(Primitive::Fractus),
        // A negated literal (`-1.0`) inherits the inner literal's family so
        // that peer-type propagation in arithmetic recognizes it.
        HirExpressionKind::Unary(crate::hir::HirUnOp::Neg, inner) => numeric_literal_family(inner),
        _ => None,
    }
}

// U-04 integration tests live here (not `mod.rs`, which is U-02's frozen
// surface): the join under a `∪`-hole expectation is owned by this module's
// `common_type` path and `check_array`/`check_if` sibling callers.
#[cfg(test)]
#[path = "type_hole_union_join_test.rs"]
mod type_hole_union_join_test;
