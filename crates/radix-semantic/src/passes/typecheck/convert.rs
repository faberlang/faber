//! Conversion, cast, and reference-shape checks.
//!
//! This file owns expression forms that intentionally change the apparent type
//! of a value: dereference, runtime conversion (`↦` / `conversio`), and typed
//! conversion/constructor syntax (`∷` / `verte`). The pass validates only the
//! semantic contracts it can prove from HIR and the `TypeTable`; representation
//! details, fallback coercions, and target-specific conversion code remain
//! backend work.
//!
//! ## `↦` (conversio)
//!
//! Operand checking is intentionally light: backends own parse/truthiness/narrowing
//! lowering. Regex targets are the exception — no literal form, so `⇥` recovery
//! is rejected here.
//!
//! ## `∷` (verte)
//!
//! Target shape drives validation. `valor ∷ T` is rejected for every `T` —
//! dynamic JSON shape is not assertable at compile time; use `↦ T` (with `⇥`
//! for recovery) instead.
//!
//! ERROR AND ESCAPE POLICY
//! =======================
//! Invalid operands emit diagnostics and return the shared error type where the
//! expression cannot be trusted. Explicit targets are otherwise respected,
//! including `ignotum` and union-like map values when the surrounding code has
//! intentionally chosen an escape or broad shape.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

use crate::lista_tensor_conversio::{
    analyze_rectangular_lista_literal, ensure_lista_numerus_type, validate_lista_to_tensor_source,
    ListaToTensorError, RectangularListaLiteralError,
};

const ISSUE_INTERVALLUM_CLAMP_RECOVERY: &str = "intervallum_clamp_recovery_unsupported";
const ISSUE_INTERVALLUM_TARGET_NOT_RANGE: &str = "intervallum_target_not_range";
const ISSUE_INTERVALLUM_CLAMP_BOUND_MISMATCH: &str = "intervallum_clamp_bound_mismatch";
const ISSUE_INTERVALLUM_HELPER_BOUND_UNSUPPORTED: &str = "intervallum_helper_bound_unsupported";
const ISSUE_SERMO_MATERIALIZE_TARGET_UNSUPPORTED: &str = "sermo_materialize_target_unsupported";
const ISSUE_INTERVALLUM_MATERIALIZE_LISTA_ELEMENT_MISMATCH: &str =
    "intervallum_materialize_lista_element_mismatch";
const ISSUE_INTERVALLUM_MATERIALIZE_TENSOR_ELEMENT_MISMATCH: &str =
    "intervallum_materialize_tensor_element_mismatch";
const ISSUE_VALOR_TO_REGEX_UNSUPPORTED: &str = "valor_to_regex_unsupported";
const ISSUE_VALOR_TO_SIZED_NUMERIC_UNSUPPORTED: &str = "valor_to_sized_numeric_unsupported";
const ISSUE_VALOR_TO_CURSOR_BOUNDARY: &str = "valor_to_cursor_boundary";
const ISSUE_VALOR_TO_PROMISSUM_BOUNDARY: &str = "valor_to_promissum_boundary";
const ISSUE_PROMISSUM_TO_VALOR_BOUNDARY: &str = "promissum_to_valor_boundary";
const ISSUE_CURSOR_TO_VALOR_BOUNDARY: &str = "cursor_to_valor_boundary";
const ISSUE_NUMERIC_TO_OCTETI_UNSPECIFIED: &str = "numeric_to_octeti_unspecified";
const ISSUE_OCTETI_TO_NUMERUS_UNSPECIFIED: &str = "octeti_to_numerus_unspecified";
const ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH: &str = "octeti_endian_width_mismatch";
const ISSUE_ASCII_FORMAT_HINT_NOT_CAPACITY: &str = "ascii_format_hint_not_capacity";
const ISSUE_ASCII_PACK_WIDTH_OVERFLOW: &str = "ascii_pack_width_overflow";
const ISSUE_ASCII_PACK_NEGATIVE_SOURCE: &str = "ascii_pack_negative_source";
const ISSUE_OCTETI_TO_FRACTUS_UNSPECIFIED: &str = "octeti_to_fractus_unspecified";
const ISSUE_BITS_PAIR_WIDTH_MISMATCH: &str = "bits_pair_width_mismatch";
const ISSUE_OCTETI_TO_BIVALENS_UNSUPPORTED: &str = "octeti_to_bivalens_unsupported";
const ISSUE_PROMISSUM_TARGET_BOUNDARY: &str = "promissum_target_boundary";
const ISSUE_PROMISSUM_SOURCE_BOUNDARY: &str = "promissum_source_boundary";
const ISSUE_SPARSA_COLLECTION_UNSUPPORTED: &str = "sparsa_collection_unsupported";
const ISSUE_SPARSA_TENSOR_ELEMENT_MISMATCH: &str = "sparsa_tensor_element_mismatch";
const ISSUE_SPARSA_TENSOR_SHAPE_MISMATCH: &str = "sparsa_tensor_shape_mismatch";
const ISSUE_TENSOR_TO_SCALAR_UNSUPPORTED: &str = "tensor_to_scalar_unsupported";
const ISSUE_SCALAR_TO_TENSOR_UNSUPPORTED: &str = "scalar_to_tensor_unsupported";
const ISSUE_TENSOR_SHAPE_MISMATCH: &str = "tensor_shape_mismatch";
const ISSUE_TENSOR_ELEMENT_MISMATCH: &str = "tensor_element_mismatch";
const ISSUE_INTERVALLUM_CLAMP_OPERAND_TYPE_MISMATCH: &str =
    "intervallum_clamp_operand_type_mismatch";
const ISSUE_CONVERSIO_FAILURE_CHANNEL_TYPE_MISMATCH: &str =
    "conversio_failure_channel_type_mismatch";
const ISSUE_CONVERSIO_RECOVERY_TYPE_MISMATCH: &str = "conversio_recovery_type_mismatch";
const ISSUE_BIVALENS_RECOVERY_UNSUPPORTED: &str = "bivalens_recovery_unsupported";
const ISSUE_REGEX_CONVERSION_OPERAND_TYPE_MISMATCH: &str = "regex_conversion_operand_type_mismatch";
const ISSUE_CONVERSIO_FAILURE_CHANNEL_TEXTUS_REQUIRED: &str =
    "conversio_failure_channel_textus_required";
const ISSUE_REGEX_OPERAND_UNSUPPORTED: &str = "regex_operand_unsupported";
const ISSUE_REGEX_RECOVERY_UNSUPPORTED: &str = "regex_recovery_unsupported";
const ISSUE_REGEX_PATTERN_TOO_LONG: &str = "regex_pattern_too_long";
const ISSUE_UNSUPPORTED_CONVERSIO: &str = "unsupported_conversio";
const ISSUE_RECORD_CONVERSION_REQUIRES_RECONSTRUCTION: &str =
    "record_conversion_requires_reconstruction";
const ISSUE_RATIO_FIELD_MISSING: &str = "ratio_field_missing";
const ISSUE_UNKNOWN_RECORD_FIELD: &str = "unknown_record_field";
const ISSUE_FIELD_INITIALIZER_TYPE_MISMATCH: &str = "field_initializer_type_mismatch";
const ISSUE_VALOR_VERTE_UNSUPPORTED: &str = "valor_verte_unsupported";
const ISSUE_INVALID_CAST: &str = "invalid_cast";
const ISSUE_ARRAY_ELEMENT_TYPE_MISMATCH: &str = "array_element_type_mismatch";
const ISSUE_ARRAY_SPREAD_ELEMENT_TYPE_MISMATCH: &str = "array_spread_element_type_mismatch";
const ISSUE_VECTOR_ELEMENT_TYPE_MISMATCH: &str = "vector_element_type_mismatch";
const ISSUE_MAP_KEY_TYPE_MISMATCH: &str = "map_key_type_mismatch";
const ISSUE_MAP_SPREAD_KEY_TYPE_MISMATCH: &str = "map_spread_key_type_mismatch";
const ISSUE_MAP_SPREAD_VALUE_TYPE_MISMATCH: &str = "map_spread_value_type_mismatch";
const ISSUE_MAP_VALUE_TYPE_MISMATCH: &str = "map_value_type_mismatch";
const ISSUE_LISTA_TO_TABULA_UNSUPPORTED: &str = "lista_to_tabula_unsupported";
const ISSUE_CROSS_COLLECTION_UNSUPPORTED: &str = "cross_collection_unsupported";
const ISSUE_LISTA_TO_TENSOR_NESTED: &str = "lista_to_tensor_nested";
const ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH: &str = "lista_to_tensor_element_mismatch";
const ISSUE_RECTANGULAR_LISTA_LITERAL_TARGET: &str = "rectangular_lista_literal_target";
const ISSUE_RECTANGULAR_LISTA_LITERAL_RAGGED: &str = "rectangular_lista_literal_ragged";
const ISSUE_RECTANGULAR_LISTA_LITERAL_ROW_LENGTH: &str = "rectangular_lista_literal_row_length";
const ISSUE_RECTANGULAR_LISTA_LITERAL_SHAPE: &str = "rectangular_lista_literal_shape";
const ISSUE_RECTANGULAR_LISTA_LITERAL_SPREAD: &str = "rectangular_lista_literal_spread";
const ISSUE_RECTANGULAR_LISTA_LITERAL_LEAF: &str = "rectangular_lista_literal_leaf";
const ISSUE_LISTA_TO_VECTOR_LITERAL_REQUIRED: &str = "lista_to_vector_literal_required";
const ISSUE_LISTA_TO_VECTOR_ELEMENT_MISMATCH: &str = "lista_to_vector_element_mismatch";
const ISSUE_LISTA_TO_VECTOR_LITERAL_WIDTH: &str = "lista_to_vector_literal_width";
const ISSUE_MATRIX_MATERIALIZE_UNSUPPORTED: &str = "matrix_materialize_unsupported";
const ISSUE_DEREF_REQUIRES_REFERENCE: &str = "deref_requires_reference";
const ISSUE_ARRAY_SPREAD_REQUIRES_LISTA: &str = "array_spread_requires_lista";
const ISSUE_VECTOR_CONSTRUCTOR_FORBIDS_SPREAD: &str = "vector_constructor_forbids_spread";
const ISSUE_OBJECT_SPREAD_REQUIRES_TABULA: &str = "object_spread_requires_tabula";
const ISSUE_OBJECT_FIELD_REQUIRES_VALUE: &str = "object_field_requires_value";
const ISSUE_JSON_GENUS_CONVERSION_REQUIRES_JSON: &str =
    "json_genus_conversion_requires_json_annotation";

/// Byte cap for literal regex patterns.
///
/// The runtime carrier is infallible and no regex engine exists yet, so this is
/// a policy boundary ahead of executable matching: it bounds the pattern bytes
/// a literal `↦ regex` (or folded `HirLiteral::Regex`) can carry. Measured on
/// the interned literal payload — the exact bytes codegen stores in
/// `faber::Regex::new(...)` — excluding inline flags. Dynamic (non-literal)
/// textus sources are unaffected. The complexity/ReDoS guard remains deferred.
pub(super) const REGEX_PATTERN_MAX_BYTES: usize = 1024;

enum PrimitiveConversioFamily {
    Numerus,
    Fractus,
    Bivalens,
    Textus,
    Ascii,
    Octeti,
    Regex,
    Other,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum CollectionConversioFamily {
    Array,
    Set,
    Map,
    Tensor,
    Cursor,
    Promissum,
}

impl CollectionConversioFamily {
    fn name(self) -> &'static str {
        match self {
            Self::Array => "lista",
            Self::Set => "copia",
            Self::Map => "tabula",
            Self::Tensor => "tensor",
            Self::Cursor => "cursor",
            Self::Promissum => "promissum",
        }
    }
}

fn lista_to_tensor_issue(error: ListaToTensorError) -> Option<&'static str> {
    match error {
        ListaToTensorError::NotListaToTensor => None,
        ListaToTensorError::NestedLista => Some(ISSUE_LISTA_TO_TENSOR_NESTED),
        ListaToTensorError::ElementMismatch => Some(ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH),
    }
}

fn endian_read_width_bytes(ty: &Type) -> Option<usize> {
    match ty {
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I16 | NumericWidth::U16) => Some(2),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I32 | NumericWidth::U32) => Some(4),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64 | NumericWidth::U64) => Some(8),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => Some(4),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => Some(8),
        _ => None,
    }
}

fn static_integer_magnitude(source: &HirExpression) -> Option<i128> {
    match &source.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => i128::try_from(*value).ok(),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => match &inner.kind {
            HirExpressionKind::Literal(HirLiteral::Int(value)) => {
                i128::try_from(*value).ok().map(|value| -value)
            }
            _ => None,
        },
        _ => None,
    }
}

fn ascii_pack_digit_count(value: u128, radix: u32) -> u128 {
    if value == 0 {
        return 1;
    }
    let mut remaining = value;
    let mut digits = 0;
    while remaining > 0 {
        remaining /= u128::from(radix);
        digits += 1;
    }
    digits
}

fn static_exclusive_range_len(index: &HirExpression) -> Option<usize> {
    let HirExpressionKind::Intervallum {
        start, end, kind, ..
    } = &index.kind
    else {
        return None;
    };
    let HirExpressionKind::Literal(HirLiteral::Int(start)) = start.kind else {
        return None;
    };
    let HirExpressionKind::Literal(HirLiteral::Int(end)) = end.kind else {
        return None;
    };
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    let end = match kind {
        radix_hir::HirRangeKind::Exclusive => end,
        radix_hir::HirRangeKind::Inclusive => end.checked_add(1)?,
    };
    end.checked_sub(start)
}

fn rectangular_lista_literal_issue(error: RectangularListaLiteralError) -> &'static str {
    match error {
        RectangularListaLiteralError::NotNestedLiteral => ISSUE_LISTA_TO_TENSOR_NESTED,
        RectangularListaLiteralError::NonLiteralTarget => ISSUE_RECTANGULAR_LISTA_LITERAL_TARGET,
        RectangularListaLiteralError::Ragged => ISSUE_RECTANGULAR_LISTA_LITERAL_RAGGED,
        RectangularListaLiteralError::RowLengthMismatch => {
            ISSUE_RECTANGULAR_LISTA_LITERAL_ROW_LENGTH
        }
        RectangularListaLiteralError::ShapeMismatch => ISSUE_RECTANGULAR_LISTA_LITERAL_SHAPE,
        RectangularListaLiteralError::Spread => ISSUE_RECTANGULAR_LISTA_LITERAL_SPREAD,
        RectangularListaLiteralError::NonLiteralLeaf => ISSUE_RECTANGULAR_LISTA_LITERAL_LEAF,
        RectangularListaLiteralError::MissingLeafType
        | RectangularListaLiteralError::ElementMismatch => ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH,
    }
}

impl TypeChecker<'_> {
    pub fn check_deref(&mut self, expr: &mut HirExpression, span: radix_lexer::Span) -> TypeId {
        let expr_ty = self.check_expr(expr);
        if let Type::Ref(_, inner) = self.types.get(self.resolve_inferred_type(expr_ty)) {
            *inner
        } else {
            self.invalid_operand_types(ISSUE_DEREF_REQUIRES_REFERENCE, span);
            self.error_type
        }
    }

    /// Checks a runtime value conversion (`↦` / `conversio`).
    ///
    /// The source is always visited for diagnostics. An inline `⇥` recovery
    /// expression must be a value of the declared target type `T`. This function
    /// returns the declared target type rather than inventing a common type
    /// between source and recovery.
    pub fn check_conversio(
        &mut self,
        source: &mut HirExpression,
        target: &mut HirConversioTarget,
        params: &[Symbol],
        recovery: Option<&mut HirExpression>,
        span: radix_lexer::Span,
    ) -> TypeId {
        let source_ty = self.check_expr(source);
        if let HirConversioTarget::Intervallum(operand) = target {
            if recovery.is_some() {
                self.invalid_conversion(ISSUE_INTERVALLUM_CLAMP_RECOVERY, span);
            }
            return self.check_intervallum_clamp_conversio(source_ty, operand, span);
        }

        let HirConversioTarget::Type(target) = target else {
            return self.error_type;
        };
        let target_resolved = self.resolve_inferred_type(*target);

        if let Some(result_ty) = self.check_json_genus_conversio(source_ty, target_resolved, span) {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            } else {
                self.check_conversio_propagation_channel(source_ty, target_resolved, span);
            }
            return result_ty;
        }

        if let Some(result_ty) =
            self.check_sermo_materialize_conversio(source_ty, target_resolved, span)
        {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return result_ty;
        }

        if let Some(result_ty) =
            self.check_intervallum_materialize_conversio(source_ty, target_resolved, span)
        {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return result_ty;
        }

        if let Some(error_ty) = self.check_valor_conversio(source_ty, target_resolved, span) {
            return error_ty;
        }

        if let Some(result_ty) = self.check_ascii_format_pack_conversio(
            source,
            source_ty,
            target_resolved,
            params,
            recovery.is_some(),
            span,
        ) {
            if !self.types.equals(result_ty, self.error_type) {
                if let Some(recovery) = recovery {
                    self.note_conversio_recovery_checked(recovery, target_resolved);
                } else {
                    self.check_conversio_propagation_channel(source_ty, target_resolved, span);
                }
            }
            return result_ty;
        }

        if self.is_bounded_ascii_conversio(source_ty, target_resolved) {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            } else {
                self.check_conversio_propagation_channel(source_ty, target_resolved, span);
            }
            return target_resolved;
        }

        if self.is_bounded_textus_conversio(source_ty, target_resolved) {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            } else {
                self.check_conversio_propagation_channel(source_ty, target_resolved, span);
            }
            return target_resolved;
        }

        if self.is_bounded_octeti_conversio(source_ty, target_resolved) {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            } else {
                self.check_conversio_propagation_channel(source_ty, target_resolved, span);
            }
            return target_resolved;
        }

        if self.is_bounded_lista_conversio(source_ty, target_resolved) {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            } else {
                self.check_conversio_propagation_channel(source_ty, target_resolved, span);
            }
            return target_resolved;
        }

        if matches!(
            self.types.get(target_resolved),
            Type::Primitive(Primitive::Regex)
        ) {
            if let Some(error_ty) =
                self.check_regex_conversio(source, source_ty, recovery.is_some(), span)
            {
                return error_ty;
            }
        }

        if let Some(error_ty) = self.check_rejected_primitive_conversio(
            source,
            source_ty,
            target_resolved,
            params,
            span,
        ) {
            return error_ty;
        }

        if let Some(result_ty) =
            self.check_octeti_to_lista_conversio(source_ty, target_resolved, span)
        {
            if !self.types.equals(result_ty, self.error_type) {
                if let Some(recovery) = recovery {
                    self.note_conversio_recovery_checked(recovery, target_resolved);
                } else {
                    self.check_conversio_propagation_channel(source_ty, target_resolved, span);
                }
            }
            return result_ty;
        }

        if recovery.is_some() && self.is_infallible_truthiness_conversio(source_ty, target_resolved)
        {
            self.invalid_conversion(ISSUE_BIVALENS_RECOVERY_UNSUPPORTED, span);
        }

        if let Some(error_ty) =
            self.check_promissum_conversio_boundary(source_ty, target_resolved, span)
        {
            return error_ty;
        }

        if let Some(error_ty) = self.check_sparse_tensor_conversio(source_ty, target_resolved, span)
        {
            return error_ty;
        }

        if let Some(result_ty) = self.check_lista_to_matrix_conversio(source, target_resolved, span)
        {
            if !self.types.equals(result_ty, self.error_type) {
                if let Some(recovery) = recovery {
                    self.note_conversio_recovery_checked(recovery, target_resolved);
                }
            }
            return result_ty;
        }

        if let Some(result_ty) =
            self.check_lista_to_vector_conversio(source, source_ty, target_resolved, span)
        {
            if !self.types.equals(result_ty, self.error_type) {
                if let Some(recovery) = recovery {
                    self.note_conversio_recovery_checked(recovery, target_resolved);
                }
            }
            return result_ty;
        }

        if let Some(error_ty) =
            self.check_collection_conversio(source, source_ty, target_resolved, span)
        {
            return error_ty;
        }

        if let Some(error_ty) = self.check_tensor_conversio(source_ty, target_resolved, span) {
            return error_ty;
        }

        if self.record_conversion_requires_reconstruction(source_ty, target_resolved) {
            self.invalid_conversion(ISSUE_RECORD_CONVERSION_REQUIRES_RECONSTRUCTION, span);
            return self.error_type;
        }

        // All specialized checks above handle known conversion paths; anything
        // reaching here must be between scalar-like types or identical types.
        if !self
            .types
            .equals(self.resolve_inferred_type(source_ty), target_resolved)
            && !self.conversio_fallback_allowed(source_ty, target_resolved)
        {
            self.invalid_conversion(ISSUE_UNSUPPORTED_CONVERSIO, span);
            return self.error_type;
        }

        if let Some(recovery) = recovery {
            self.note_conversio_recovery_checked(recovery, target_resolved);
        } else {
            self.check_conversio_propagation_channel(source_ty, target_resolved, span);
        }
        target_resolved
    }

    /// `x ↦ intervallum` / `r1 ↦ r2`: clamp; value clamp yields bound `T`, range clamp yields `intervallum<T>`.
    fn check_intervallum_clamp_conversio(
        &mut self,
        source_ty: TypeId,
        operand: &mut HirExpression,
        span: radix_lexer::Span,
    ) -> TypeId {
        let operand_ty = self.check_expr(operand);
        let Some(target_intervallum) = self.resolve_intervallum_type(operand_ty, operand.span)
        else {
            self.invalid_conversion(ISSUE_INTERVALLUM_TARGET_NOT_RANGE, operand.span);
            return self.error_type;
        };

        if let Some(source_intervallum) = self.intervallum_type_id(source_ty) {
            if !self.types.equals(source_intervallum, target_intervallum) {
                self.invalid_conversion(ISSUE_INTERVALLUM_CLAMP_BOUND_MISMATCH, span);
                return self.error_type;
            }
            let Some(bound) = self.intervallum_bound_type(target_intervallum) else {
                return self.error_type;
            };
            if !self.is_intervallum_runtime_numerus_bound(bound) {
                self.invalid_operand_types(ISSUE_INTERVALLUM_HELPER_BOUND_UNSUPPORTED, span);
                return self.error_type;
            }
            return target_intervallum;
        }

        let Some(bound) = self.intervallum_bound_type(target_intervallum) else {
            return self.error_type;
        };
        if !self.is_intervallum_runtime_numerus_bound(bound) {
            self.invalid_operand_types(ISSUE_INTERVALLUM_HELPER_BOUND_UNSUPPORTED, span);
            return self.error_type;
        }
        self.require_intervallum_bound(source_ty, span);
        self.unify_assign_issue(
            source_ty,
            bound,
            span,
            ISSUE_INTERVALLUM_CLAMP_OPERAND_TYPE_MISMATCH,
        );
        bound
    }

    /// `sermo ↦ T` materialization. Source must be a `sermo` handle; `T` must
    /// be a type the materializer policy supports.
    fn check_sermo_materialize_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !self.is_sermo_type(source_ty) {
            return None;
        }
        if self.is_sermo_materialize_target(target_resolved) {
            return Some(target_resolved);
        }
        self.invalid_conversion(ISSUE_SERMO_MATERIALIZE_TARGET_UNSUPPORTED, span);
        Some(self.error_type)
    }

    fn is_sermo_type(&self, ty: TypeId) -> bool {
        let Some(sermo_def_id) = crate::builtins::frame_types::sermo_def_id(self.resolver) else {
            return false;
        };
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Struct(def_id) if *def_id == sermo_def_id
        )
    }

    fn is_sermo_materialize_target(&self, ty: TypeId) -> bool {
        let ty = self.resolve_inferred_type(ty);
        matches!(
            self.types.get(ty),
            Type::Param(_)
                | Type::Primitive(
                    Primitive::Vacuum
                        | Primitive::Valor
                        | Primitive::Numerus
                        | Primitive::Fractus
                        | Primitive::Bivalens
                        | Primitive::Textus
                        | Primitive::Ascii
                        | Primitive::Octeti
                        | Primitive::Instans
                )
                | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
                | Type::SizedInstans(_)
                | Type::Array(_)
        )
    }

    /// `intervallum<T> ↦ lista<T>` / `intervallum<T> ↦ tensor<T>` materialization.
    fn check_intervallum_materialize_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_intervallum = self.intervallum_type_id(source_ty)?;
        let bound = self.intervallum_bound_type(source_intervallum)?;
        match self.types.get(target_resolved) {
            Type::Array(elem) if self.types.equals(bound, *elem) => {
                if self.check_intervallum_materialize_runtime_bound(bound, span) {
                    Some(target_resolved)
                } else {
                    Some(self.error_type)
                }
            }
            Type::Tensor(elem, _) if self.types.equals(bound, *elem) => {
                if self.check_intervallum_materialize_runtime_bound(bound, span) {
                    Some(target_resolved)
                } else {
                    Some(self.error_type)
                }
            }
            Type::Array(_) => {
                self.invalid_conversion(ISSUE_INTERVALLUM_MATERIALIZE_LISTA_ELEMENT_MISMATCH, span);
                Some(self.error_type)
            }
            Type::Tensor(_, _) => {
                self.invalid_conversion(
                    ISSUE_INTERVALLUM_MATERIALIZE_TENSOR_ELEMENT_MISMATCH,
                    span,
                );
                Some(self.error_type)
            }
            _ => None,
        }
    }

    fn check_intervallum_materialize_runtime_bound(
        &mut self,
        bound: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        if self.is_intervallum_runtime_numerus_bound(bound) {
            return true;
        }
        self.invalid_operand_types(ISSUE_INTERVALLUM_HELPER_BOUND_UNSUPPORTED, span);
        false
    }

    /// Reject `valor` conversio holdouts and async/lazy boundary crossings.
    fn check_valor_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let source_is_valor = matches!(
            self.types.get(source_resolved),
            Type::Primitive(Primitive::Valor)
        );
        let target_is_valor = matches!(
            self.types.get(target_resolved),
            Type::Primitive(Primitive::Valor)
        );

        let issue = if source_is_valor {
            match self.types.get(target_resolved) {
                Type::Primitive(Primitive::Regex) => Some(ISSUE_VALOR_TO_REGEX_UNSUPPORTED),
                _ if self.valor_extraction_contains_unsupported_sized_numeric(target_resolved) => {
                    Some(ISSUE_VALOR_TO_SIZED_NUMERIC_UNSUPPORTED)
                }
                Type::Cursor(_) | Type::AsyncCursor(_, _) => Some(ISSUE_VALOR_TO_CURSOR_BOUNDARY),
                Type::Promissum(_) | Type::PromissumFailable(_, _) => {
                    Some(ISSUE_VALOR_TO_PROMISSUM_BOUNDARY)
                }
                _ => None,
            }
        } else if target_is_valor {
            match self.types.get(source_resolved) {
                Type::Promissum(_) | Type::PromissumFailable(_, _) => {
                    Some(ISSUE_PROMISSUM_TO_VALOR_BOUNDARY)
                }
                Type::Cursor(_) | Type::AsyncCursor(_, _) => Some(ISSUE_CURSOR_TO_VALOR_BOUNDARY),
                _ => None,
            }
        } else {
            None
        };

        if let Some(issue) = issue {
            self.invalid_conversion(issue, span);
            return Some(self.error_type);
        }
        None
    }

    fn check_json_genus_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        match (
            self.types.get(source_resolved),
            self.types.get(target_resolved),
        ) {
            (Type::Struct(def_id), Type::Primitive(Primitive::Json))
            | (Type::Primitive(Primitive::Json), Type::Struct(def_id)) => {
                if self.json_structs.contains(def_id) {
                    Some(target_resolved)
                } else {
                    self.invalid_conversion(ISSUE_JSON_GENUS_CONVERSION_REQUIRES_JSON, span);
                    Some(self.error_type)
                }
            }
            _ => None,
        }
    }

    fn valor_extraction_contains_unsupported_sized_numeric(&self, target: TypeId) -> bool {
        let mut seen = rustc_hash::FxHashSet::default();
        self.valor_extraction_type_needs_missing_width_runtime(target, false, &mut seen)
    }

    fn valor_extraction_type_needs_missing_width_runtime(
        &self,
        ty: TypeId,
        inside_from_valor_runtime: bool,
        seen: &mut rustc_hash::FxHashSet<(TypeId, bool)>,
    ) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        if !seen.insert((resolved, inside_from_valor_runtime)) {
            return false;
        }

        match self.types.get(resolved) {
            Type::SizedNumeric(Primitive::Fractus, _) => true,
            Type::SizedNumeric(Primitive::Numerus, _) => inside_from_valor_runtime,
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Matrix(inner, _)
            | Type::Vector(inner, _)
            | Type::Option(inner)
            | Type::Ref(_, inner)
            | Type::Alias(_, inner)
            | Type::Intervallum(inner)
            | Type::Atomic(inner) => {
                self.valor_extraction_type_needs_missing_width_runtime(*inner, true, seen)
            }
            Type::Map(key, value) => {
                self.valor_extraction_type_needs_missing_width_runtime(*key, true, seen)
                    || self.valor_extraction_type_needs_missing_width_runtime(*value, true, seen)
            }
            Type::Struct(def_id) => self.structs.get(def_id).is_some_and(|fields| {
                fields.fields.values().any(|field| {
                    self.valor_extraction_type_needs_missing_width_runtime(field.ty, false, seen)
                })
            }),
            Type::Union(types) => types.iter().any(|ty| {
                self.valor_extraction_type_needs_missing_width_runtime(
                    *ty,
                    inside_from_valor_runtime,
                    seen,
                )
            }),
            Type::Applied(base, args) => {
                self.valor_extraction_type_needs_missing_width_runtime(
                    *base,
                    inside_from_valor_runtime,
                    seen,
                ) || args.iter().any(|arg| {
                    self.valor_extraction_type_needs_missing_width_runtime(
                        *arg,
                        inside_from_valor_runtime,
                        seen,
                    )
                })
            }
            _ => false,
        }
    }

    /// `octeti ↦ lista<numerus<u8>>` is a representation change only.
    /// Other lista element types fail closed — no silent widen, no pad.
    fn check_octeti_to_lista_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !matches!(
            self.types.get(self.resolve_inferred_type(source_ty)),
            Type::Primitive(Primitive::Octeti)
        ) {
            return None;
        }
        let Type::Array(elem) = self.types.get(target_resolved).clone() else {
            return None;
        };
        if self.is_numerus_u8(elem) {
            return Some(target_resolved);
        }
        self.invalid_conversion(ISSUE_UNSUPPORTED_CONVERSIO, span);
        Some(self.error_type)
    }

    fn is_numerus_u8(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8)
        )
    }

    /// Reject primitive `↦` pairs with no single obvious meaning (byte reinterpretation, etc.).
    fn check_rejected_primitive_conversio(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let issue = match (
            self.primitive_conversio_family(source_ty),
            self.primitive_conversio_family(target_resolved),
        ) {
            (
                PrimitiveConversioFamily::Numerus
                | PrimitiveConversioFamily::Fractus
                | PrimitiveConversioFamily::Bivalens,
                PrimitiveConversioFamily::Octeti,
            ) => Some(ISSUE_NUMERIC_TO_OCTETI_UNSPECIFIED),
            (PrimitiveConversioFamily::Octeti, PrimitiveConversioFamily::Numerus) => {
                self.octeti_to_numerus_issue(source, target_resolved, params)
            }
            (PrimitiveConversioFamily::Octeti, PrimitiveConversioFamily::Fractus) => {
                self.octeti_to_fractus_issue(source, target_resolved, params)
            }
            (PrimitiveConversioFamily::Octeti, PrimitiveConversioFamily::Bivalens) => {
                Some(ISSUE_OCTETI_TO_BIVALENS_UNSUPPORTED)
            }
            (
                PrimitiveConversioFamily::Numerus,
                PrimitiveConversioFamily::Fractus,
            )
            | (
                PrimitiveConversioFamily::Fractus,
                PrimitiveConversioFamily::Numerus,
            ) => self.bits_pair_issue(source_ty, target_resolved, params),
            _ => None,
        };

        if let Some(issue) = issue {
            self.invalid_conversion(issue, span);
            return Some(self.error_type);
        }
        None
    }

    /// `octeti ↦ numerus<W, Be|Le>` is allowed when width is 2/4/8 and a
    /// static window matches. Without Be/Le, or on i8/u8/unsized numerus,
    /// the family reject stays.
    fn octeti_to_numerus_issue(
        &self,
        source: &HirExpression,
        target_resolved: TypeId,
        params: &[Symbol],
    ) -> Option<&'static str> {
        if !self.conversio_has_endian_hint(params) {
            return Some(ISSUE_OCTETI_TO_NUMERUS_UNSPECIFIED);
        }
        let Some(need) = endian_read_width_bytes(self.types.get(target_resolved)) else {
            return Some(ISSUE_OCTETI_TO_NUMERUS_UNSPECIFIED);
        };
        match self.static_octeti_window_len(source) {
            Some(got) if got != need => Some(ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH),
            _ => None,
        }
    }

    /// `octeti ↦ fractus<W, Be|Le>` is allowed only for the exact IEEE width
    /// and a static window that matches it. Without Be/Le, or on f16/unsized
    /// fractus, the family reject stays.
    fn octeti_to_fractus_issue(
        &self,
        source: &HirExpression,
        target_resolved: TypeId,
        params: &[Symbol],
    ) -> Option<&'static str> {
        if !self.conversio_has_endian_hint(params) {
            return Some(ISSUE_OCTETI_TO_FRACTUS_UNSPECIFIED);
        }
        let Some(need) = endian_read_width_bytes(self.types.get(target_resolved)) else {
            return Some(ISSUE_OCTETI_TO_FRACTUS_UNSPECIFIED);
        };
        match self.static_octeti_window_len(source) {
            Some(got) if got != need => Some(ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH),
            _ => None,
        }
    }

    fn conversio_has_endian_hint(&self, params: &[Symbol]) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        params
            .iter()
            .any(|param| matches!(interner.resolve(*param), "Be" | "Le"))
    }

    /// `Bits` is an exact-width IEEE bitcast: `u32↔f32` and `u64↔f64` only.
    /// Any other pair (or unsized primitive) fail-closes; without the hint the
    /// ordinary value-conversion families stay in charge.
    fn bits_pair_issue(
        &self,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
    ) -> Option<&'static str> {
        if !self.conversio_has_bits_hint(params) {
            return None;
        }
        let source = self.types.get(self.resolve_inferred_type(source_ty));
        let target = self.types.get(target_resolved);
        let exact_pair = matches!(
            (source, target),
            (
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32),
                Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
            ) | (
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U64),
                Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64)
            ) | (
                Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
            ) | (
                Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U64)
            )
        );
        if exact_pair {
            None
        } else {
            Some(ISSUE_BITS_PAIR_WIDTH_MISMATCH)
        }
    }

    fn conversio_has_bits_hint(&self, params: &[Symbol]) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        params
            .iter()
            .any(|param| matches!(interner.resolve(*param), "Bits"))
    }

    /// `n ↦ ascii<N, Hex|Bin|Oct>`: N is digit width; Hex/Bin/Oct are slot-2
    /// format hints peeled into conversio params. Float sources reject; a
    /// const-foldable negative or overflowing source fail-closes at typecheck
    /// unless `⇥` recovery is present (runtime still fail-closes).
    fn check_ascii_format_pack_conversio(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        has_recovery: bool,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !self.conversio_has_ascii_format_hint(params) {
            return None;
        }
        match self.types.get(target_resolved) {
            Type::BoundedAscii { capacity } => {
                let capacity = *capacity;
                match self.primitive_conversio_family(source_ty) {
                    PrimitiveConversioFamily::Numerus => {}
                    _ => {
                        self.invalid_conversion(ISSUE_UNSUPPORTED_CONVERSIO, span);
                        return Some(self.error_type);
                    }
                }

                // AP-2: a hole is a const-folding witness, not a runtime
                // capacity inference. A literal numerus source supplies the
                // digit count through the shared index unifier; a runtime
                // source keeps the hole open so finalization reports the
                // established bounded_capacity_unsolved diagnostic.
                let resolved_capacity = self.resolve_index(capacity);
                if self.is_unresolved_index_hole(resolved_capacity) {
                    if let Some(value) = static_integer_magnitude(source) {
                        if value >= 0 {
                            let radix = self
                                .ascii_format_radix(params)
                                .expect("ascii format hint checked above");
                            let digits = ascii_pack_digit_count(
                                u128::try_from(value).expect("non-negative i128 fits u128"),
                                radix,
                            );
                            let witness = self.types.intern_index(IndexExpr::Literal(
                                u64::try_from(digits).expect("digit count fits u64"),
                            ));
                            self.constrain_index_equal(capacity, witness, span);
                        } else if !has_recovery {
                            self.invalid_conversion(ISSUE_ASCII_PACK_NEGATIVE_SOURCE, span);
                            return Some(self.error_type);
                        }
                    }
                    return Some(target_resolved);
                }

                if !has_recovery {
                    if let Some(issue) =
                        self.ascii_format_pack_const_issue(source, resolved_capacity, params)
                    {
                        self.invalid_conversion(issue, span);
                        return Some(self.error_type);
                    }
                }
                Some(target_resolved)
            }
            Type::Primitive(Primitive::Ascii) => {
                self.invalid_conversion(ISSUE_ASCII_FORMAT_HINT_NOT_CAPACITY, span);
                Some(self.error_type)
            }
            _ => None,
        }
    }

    fn conversio_has_ascii_format_hint(&self, params: &[Symbol]) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        params
            .iter()
            .any(|param| crate::numeric_width::is_ascii_format_hint(interner.resolve(*param)))
    }

    fn ascii_format_radix(&self, params: &[Symbol]) -> Option<u32> {
        let interner = self.interner?;
        params
            .iter()
            .find_map(|param| match interner.resolve(*param) {
                "Hex" => Some(16),
                "Bin" => Some(2),
                "Oct" => Some(8),
                _ => None,
            })
    }

    fn ascii_format_pack_const_issue(
        &self,
        source: &HirExpression,
        capacity: radix_types::IndexId,
        params: &[Symbol],
    ) -> Option<&'static str> {
        let value = static_integer_magnitude(source)?;
        if value < 0 {
            return Some(ISSUE_ASCII_PACK_NEGATIVE_SOURCE);
        }
        let IndexExpr::Literal(width) = self.types.get_index(capacity) else {
            return None;
        };
        let radix = self.ascii_format_radix(params)?;
        let digits = ascii_pack_digit_count(u128::try_from(value).ok()?, radix);
        if digits > u128::from(*width) {
            Some(ISSUE_ASCII_PACK_WIDTH_OVERFLOW)
        } else {
            None
        }
    }

    /// Compile-time window length: a whole octeti literal, or a constant
    /// `lo‥hi` / `lo…hi` index. Unknown length is a runtime check.
    fn static_octeti_window_len(&self, source: &HirExpression) -> Option<usize> {
        match &source.kind {
            HirExpressionKind::Literal(HirLiteral::Octeti(sym)) => {
                let raw = self.interned_literal_payload(*sym)?;
                radix_lexer::octeti_literal::decode_octeti_hex(raw)
                    .ok()
                    .map(|bytes| bytes.len())
            }
            HirExpressionKind::Index(_, index) => static_exclusive_range_len(index),
            _ => None,
        }
    }

    /// Reject async/sync `promissum` boundaries unless both sides are `promissum`.
    fn check_promissum_conversio_boundary(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let source_promissum = matches!(self.types.get(source_resolved), Type::Promissum(_));
        let target_promissum = matches!(self.types.get(target_resolved), Type::Promissum(_));

        let issue = match (source_promissum, target_promissum) {
            (false, true) => Some(ISSUE_PROMISSUM_TARGET_BOUNDARY),
            (true, false) => Some(ISSUE_PROMISSUM_SOURCE_BOUNDARY),
            _ => None,
        };

        if let Some(issue) = issue {
            self.invalid_conversion(issue, span);
            return Some(self.error_type);
        }
        None
    }

    /// Validate dense/sparse representation `↦` pairs.
    fn check_sparse_tensor_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let source = self.types.get(source_resolved);
        let target = self.types.get(target_resolved);

        let Some((from_elem, from_shape, to_elem, to_shape)) = sparse_tensor_pair(source, target)
        else {
            if matches!(source, Type::Sparsa(_, _)) || matches!(target, Type::Sparsa(_, _)) {
                self.invalid_conversion(ISSUE_SPARSA_COLLECTION_UNSUPPORTED, span);
                return Some(self.error_type);
            }
            return None;
        };

        if !self.types.equals(from_elem, to_elem) {
            self.invalid_conversion(ISSUE_SPARSA_TENSOR_ELEMENT_MISMATCH, span);
            return Some(self.error_type);
        }

        if !self.index_shapes_unify(from_shape, to_shape) {
            self.invalid_conversion(ISSUE_SPARSA_TENSOR_SHAPE_MISMATCH, span);
            return Some(self.error_type);
        }

        None
    }

    /// Validate tensor `↦` pairs: element-width bridges, scalar-extract holdouts.
    fn check_tensor_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let tensor_pair = match (
            self.types.get(source_resolved),
            self.types.get(target_resolved),
        ) {
            (Type::Tensor(from_elem, from_shape), Type::Tensor(to_elem, to_shape)) => {
                Some((*from_elem, *from_shape, *to_elem, *to_shape))
            }
            _ => None,
        };
        let shapes_unify = tensor_pair.is_none_or(|(_, from_shape, _, to_shape)| {
            self.index_shapes_unify(from_shape, to_shape)
        });
        let issue = match (
            self.types.get(source_resolved),
            self.types.get(target_resolved),
        ) {
            (Type::Tensor(_, _), target) if self.is_tensor_scalar_extract_target(target) => {
                Some(ISSUE_TENSOR_TO_SCALAR_UNSUPPORTED)
            }
            (source, Type::Tensor(_, _)) if self.is_tensor_scalar_extract_target(source) => {
                Some(ISSUE_SCALAR_TO_TENSOR_UNSUPPORTED)
            }
            (Type::Tensor(_, _), Type::Tensor(_, _)) if !shapes_unify => {
                Some(ISSUE_TENSOR_SHAPE_MISMATCH)
            }
            (Type::Tensor(from_elem, _), Type::Tensor(to_elem, _))
                if !crate::numeric_width::tensor_element_conversio_allowed(
                    self.types, *from_elem, *to_elem,
                ) =>
            {
                Some(ISSUE_TENSOR_ELEMENT_MISMATCH)
            }
            _ => None,
        };

        if let Some(issue) = issue {
            self.invalid_conversion(issue, span);
            return Some(self.error_type);
        }
        None
    }

    fn is_tensor_scalar_extract_target(&self, ty: &Type) -> bool {
        matches!(
            ty,
            Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
        )
    }

    fn index_shapes_unify(&mut self, from_shape: crate::IndexId, to_shape: crate::IndexId) -> bool {
        let mut subs = rustc_hash::FxHashMap::default();
        let mut index_unifier = crate::IndexUnifier::new(self.types, &mut subs);
        index_unifier.unify(from_shape, to_shape).is_ok()
    }

    /// Validate cross-collection `↦` pairs. Returns `Some(error_type)` when the
    /// conversion is rejected; `None` when checking should continue.
    fn check_collection_conversio(
        &mut self,
        source: &mut HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        match validate_lista_to_tensor_source(source_resolved, target_resolved, self.types) {
            Ok(()) => {
                self.register_lista_to_tensor_conversio_prereqs();
                return None;
            }
            Err(ListaToTensorError::NotListaToTensor) => {}
            Err(ListaToTensorError::NestedLista) => {
                return self.check_rectangular_lista_literal_conversio(
                    source,
                    target_resolved,
                    span,
                );
            }
            Err(error) => return self.reject_lista_to_tensor_conversio(error, span),
        }
        let source_shape = self.collection_conversio_shape(source_resolved)?;
        let target_shape = self.collection_conversio_shape(target_resolved)?;
        if source_shape.family == target_shape.family {
            return None;
        }

        if self.is_allowed_collection_conversio(&source_shape, &target_shape) {
            return None;
        }

        let issue = self.collection_conversio_rejection_issue(&source_shape, &target_shape);
        self.invalid_conversion_with_args(
            issue,
            span,
            &[
                ("source_family", source_shape.family.name()),
                ("target_family", target_shape.family.name()),
            ],
        );
        Some(self.error_type)
    }

    fn check_lista_to_vector_conversio(
        &mut self,
        source: &mut HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Type::Vector(target_elem, width) = self.types.get(target_resolved).clone() else {
            return None;
        };

        if crate::literal_vector_width(width, self.types).is_none() {
            self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_WIDTH, span);
            return Some(self.error_type);
        }

        if matches!(source.kind, HirExpressionKind::Array(_)) {
            self.check_verte_vector_target(source, target_elem, width, span);
            return Some(target_resolved);
        }

        let source_resolved = self.resolve_inferred_type(source_ty);
        let Type::Array(source_elem) = self.types.get(source_resolved).clone() else {
            self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_REQUIRED, span);
            return Some(self.error_type);
        };
        if !self.types.equals(source_elem, target_elem) {
            self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_ELEMENT_MISMATCH, span);
            return Some(self.error_type);
        }
        self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_REQUIRED, span);
        Some(self.error_type)
    }

    /// Nested `lista` literal → register `matrix<T, [R, C]>` (CPU subset ≤4×4).
    ///
    /// Non-literal sources and non-matrix targets leave this path; oversized or
    /// ragged shapes reject with matrix materialize diagnostics.
    fn check_lista_to_matrix_conversio(
        &mut self,
        source: &mut HirExpression,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Type::Matrix(target_elem, shape) = self.types.get(target_resolved).clone() else {
            return None;
        };

        let Some((rows, columns)) = crate::literal_matrix_shape(shape, self.types) else {
            self.invalid_conversion(ISSUE_MATRIX_MATERIALIZE_UNSUPPORTED, span);
            return Some(self.error_type);
        };
        if rows > 4 || columns > 4 {
            self.invalid_conversion(ISSUE_MATRIX_MATERIALIZE_UNSUPPORTED, span);
            return Some(self.error_type);
        }

        let HirExpressionKind::Array(row_elements) = &mut source.kind else {
            self.invalid_conversion(ISSUE_MATRIX_MATERIALIZE_UNSUPPORTED, span);
            return Some(self.error_type);
        };
        if row_elements.len() != usize::from(rows) {
            self.invalid_conversion(ISSUE_RECTANGULAR_LISTA_LITERAL_SHAPE, span);
            return Some(self.error_type);
        }

        for row in row_elements.iter_mut() {
            match row {
                HirArrayElement::Spread(expr) => {
                    self.check_expr(expr);
                    self.invalid_conversion(ISSUE_RECTANGULAR_LISTA_LITERAL_SPREAD, span);
                    return Some(self.error_type);
                }
                HirArrayElement::Expr(row_expr) => {
                    let HirExpressionKind::Array(cells) = &mut row_expr.kind else {
                        self.check_expr(row_expr);
                        self.invalid_conversion(ISSUE_LISTA_TO_TENSOR_NESTED, span);
                        return Some(self.error_type);
                    };
                    if cells.len() != usize::from(columns) {
                        self.invalid_conversion(ISSUE_RECTANGULAR_LISTA_LITERAL_ROW_LENGTH, span);
                        return Some(self.error_type);
                    }
                    for cell in cells.iter_mut() {
                        match cell {
                            HirArrayElement::Spread(expr) => {
                                self.check_expr(expr);
                                self.invalid_conversion(
                                    ISSUE_RECTANGULAR_LISTA_LITERAL_SPREAD,
                                    span,
                                );
                                return Some(self.error_type);
                            }
                            HirArrayElement::Expr(cell_expr) => {
                                let cell_ty =
                                    self.check_expr_with_expected(cell_expr, Some(target_elem));
                                self.unify_join(
                                    cell_ty,
                                    target_elem,
                                    cell_expr.span,
                                    ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH,
                                );
                            }
                        }
                    }
                }
            }
        }
        Some(target_resolved)
    }

    fn check_rectangular_lista_literal_conversio(
        &mut self,
        source: &mut HirExpression,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !matches!(source.kind, HirExpressionKind::Array(_)) {
            self.invalid_conversion(
                rectangular_lista_literal_issue(RectangularListaLiteralError::NotNestedLiteral),
                span,
            );
            return Some(self.error_type);
        }

        if let Some(expected) = self.expected_nested_lista_literal_type(target_resolved) {
            self.check_expr_with_expected(source, Some(expected));
        }

        match analyze_rectangular_lista_literal(source, target_resolved, self.types) {
            Ok(_) => {
                self.register_lista_to_tensor_conversio_prereqs();
                None
            }
            Err(error) => {
                self.invalid_conversion(rectangular_lista_literal_issue(error), span);
                Some(self.error_type)
            }
        }
    }

    fn register_lista_to_tensor_conversio_prereqs(&mut self) {
        ensure_lista_numerus_type(self.types);
    }

    fn reject_lista_to_tensor_conversio(
        &mut self,
        error: ListaToTensorError,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let issue = lista_to_tensor_issue(error)?;
        self.invalid_conversion(issue, span);
        Some(self.error_type)
    }

    fn expected_nested_lista_literal_type(&mut self, target_resolved: TypeId) -> Option<TypeId> {
        let Type::Tensor(elem, _) = self.types.get(target_resolved).clone() else {
            return None;
        };
        let inner = self.types.array(elem);
        Some(self.types.array(inner))
    }

    fn collection_conversio_shape(&self, ty: TypeId) -> Option<CollectionConversioShape> {
        match self.types.get(ty) {
            Type::Array(elem) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Array,
                elem: Some(*elem),
                key: None,
                value: None,
            }),
            Type::Set(elem) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Set,
                elem: Some(*elem),
                key: None,
                value: None,
            }),
            Type::Map(key, value) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Map,
                elem: None,
                key: Some(*key),
                value: Some(*value),
            }),
            Type::Tensor(elem, _) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Tensor,
                elem: Some(*elem),
                key: None,
                value: None,
            }),
            Type::Cursor(elem) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Cursor,
                elem: Some(*elem),
                key: None,
                value: None,
            }),
            Type::Promissum(elem) | Type::PromissumFailable(elem, _) => {
                Some(CollectionConversioShape {
                    family: CollectionConversioFamily::Promissum,
                    elem: Some(*elem),
                    key: None,
                    value: None,
                })
            }
            _ => None,
        }
    }

    fn is_allowed_collection_conversio(
        &self,
        source: &CollectionConversioShape,
        target: &CollectionConversioShape,
    ) -> bool {
        match (source.family, target.family) {
            (CollectionConversioFamily::Array, CollectionConversioFamily::Set) => {
                self.collection_elems_match(source.elem, target.elem)
            }
            (CollectionConversioFamily::Set, CollectionConversioFamily::Array) => {
                self.collection_elems_match(source.elem, target.elem)
            }
            (CollectionConversioFamily::Tensor, CollectionConversioFamily::Array) => {
                self.collection_elems_match(source.elem, target.elem)
            }
            (CollectionConversioFamily::Array, CollectionConversioFamily::Tensor) => {
                self.collection_elems_match(source.elem, target.elem)
            }
            (CollectionConversioFamily::Cursor, CollectionConversioFamily::Array) => {
                self.collection_elems_match(source.elem, target.elem)
            }
            (CollectionConversioFamily::Array, CollectionConversioFamily::Cursor) => {
                self.collection_elems_match(source.elem, target.elem)
            }
            (CollectionConversioFamily::Map, CollectionConversioFamily::Array) => {
                let (Some(key), Some(value), Some(elem)) = (source.key, source.value, target.elem)
                else {
                    return false;
                };
                self.types.equals(key, elem) || self.types.equals(value, elem)
            }
            _ => false,
        }
    }

    fn collection_elems_match(&self, left: Option<TypeId>, right: Option<TypeId>) -> bool {
        match (left, right) {
            (Some(a), Some(b)) => self.types.equals(a, b),
            _ => false,
        }
    }

    fn collection_conversio_rejection_issue(
        &self,
        source: &CollectionConversioShape,
        target: &CollectionConversioShape,
    ) -> &'static str {
        match (source.family, target.family) {
            (CollectionConversioFamily::Array, CollectionConversioFamily::Map) => {
                ISSUE_LISTA_TO_TABULA_UNSUPPORTED
            }
            _ => ISSUE_CROSS_COLLECTION_UNSUPPORTED,
        }
    }

    /// D7's explicit ASCII buffer bridges. Bounded values copy their valid
    /// prefix into an unbounded carrier; the reverse direction is fallible and
    /// keeps the declared capacity for the Rust carrier to enforce.
    fn is_bounded_ascii_conversio(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        matches!(
            (
                self.types.get(self.resolve_inferred_type(source_ty)),
                self.types.get(self.resolve_inferred_type(target_ty)),
            ),
            (
                Type::BoundedAscii { .. },
                Type::Primitive(Primitive::Ascii | Primitive::Textus)
            ) | (Type::Primitive(Primitive::Ascii), Type::BoundedAscii { .. })
        )
    }

    /// D7's explicit bounded-byte bridges. Bounded → unbounded copies the
    /// initialized prefix; unbounded → bounded keeps `N` for the host carrier
    /// to enforce with a fail-closed overflow when `len > N`.
    fn is_bounded_octeti_conversio(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        matches!(
            (
                self.types.get(self.resolve_inferred_type(source_ty)),
                self.types.get(self.resolve_inferred_type(target_ty)),
            ),
            (
                Type::BoundedOcteti { .. },
                Type::Primitive(Primitive::Octeti)
            ) | (
                Type::Primitive(Primitive::Octeti),
                Type::BoundedOcteti { .. }
            )
        )
    }

    /// D7's explicit bounded-text bridges. Bounded → unbounded copies the
    /// initialized scalar prefix; unbounded → bounded keeps `N` for the host
    /// carrier to enforce with a fail-closed overflow.
    fn is_bounded_textus_conversio(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        matches!(
            (
                self.types.get(self.resolve_inferred_type(source_ty)),
                self.types.get(self.resolve_inferred_type(target_ty)),
            ),
            (
                Type::BoundedTextus { .. },
                Type::Primitive(Primitive::Textus)
            ) | (
                Type::Primitive(Primitive::Textus),
                Type::BoundedTextus { .. }
            )
        )
    }

    /// D7's explicit bounded-list bridges. Bounded → unbounded copies the
    /// initialized prefix; unbounded → bounded keeps `N` for the host carrier
    /// to enforce with a fail-closed overflow.
    fn is_bounded_lista_conversio(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        match (
            self.types.get(self.resolve_inferred_type(source_ty)),
            self.types.get(self.resolve_inferred_type(target_ty)),
        ) {
            (Type::BoundedArray { element: from, .. }, Type::Array(to))
            | (Type::Array(from), Type::BoundedArray { element: to, .. }) => {
                self.types.equals(*from, *to)
            }
            _ => false,
        }
    }

    fn primitive_conversio_family(&self, ty: TypeId) -> PrimitiveConversioFamily {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
                PrimitiveConversioFamily::Numerus
            }
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                PrimitiveConversioFamily::Fractus
            }
            Type::Primitive(Primitive::Bivalens) => PrimitiveConversioFamily::Bivalens,
            Type::Primitive(Primitive::Textus) => PrimitiveConversioFamily::Textus,
            Type::Primitive(Primitive::Ascii) => PrimitiveConversioFamily::Ascii,
            // D17 / BB-2: `'x'` is `ascii<1>`. Same conversio family as bare
            // ascii so `'a' ↦ textus` / `s[i] ↦ numerus` stay explicit widens.
            // Multi-char `ascii<N>` stays out until BB-4.
            Type::BoundedAscii { capacity }
                if matches!(self.types.get_index(*capacity), IndexExpr::Literal(1)) =>
            {
                PrimitiveConversioFamily::Ascii
            }
            Type::Primitive(Primitive::Octeti) => PrimitiveConversioFamily::Octeti,
            Type::Primitive(Primitive::Regex) => PrimitiveConversioFamily::Regex,
            _ => PrimitiveConversioFamily::Other,
        }
    }

    /// Fallthrough conversio: scalar-to-scalar, valor extraction, collection
    /// conversions, and same-type conversions are allowed by backend-defined
    /// rules. Nominal types (struct, enum, func) converted to unrelated
    /// scalars are rejected here.
    fn conversio_fallback_allowed(&self, source_ty: TypeId, target_resolved: TypeId) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        let target = target_resolved;

        if self.types.equals(source, target) {
            return true;
        }

        if self.is_record_type(source) || self.is_record_type(target) {
            return false;
        }

        for ty in [source, target] {
            match self.types.get(ty) {
                Type::Primitive(Primitive::Valor | Primitive::Ignotum)
                | Type::Param(_)
                | Type::Infer(_) => return true,
                _ => {}
            }
        }

        // Collection and structural types have their own conversion rules
        // handled by earlier specialized checks — don't block them here.
        if self.is_structural_or_collection_type(source)
            || self.is_structural_or_collection_type(target)
        {
            return true;
        }

        let source_family = self.primitive_conversio_family(source);
        let target_family = self.primitive_conversio_family(target);
        let is_scalar = |family: PrimitiveConversioFamily, ty: TypeId| -> bool {
            !matches!(family, PrimitiveConversioFamily::Other)
                || matches!(
                    self.types.get(ty),
                    Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
                )
        };
        if is_scalar(source_family, source) && is_scalar(target_family, target) {
            return true;
        }

        false
    }

    /// Specialized conversio rules for these types are handled earlier.
    fn is_structural_or_collection_type(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(ty),
            Type::Array(_)
                | Type::Set(_)
                | Type::Map(_, _)
                | Type::Tensor(_, _)
                | Type::Cursor(_)
                | Type::AsyncCursor(_, _)
                | Type::Promissum(_)
                | Type::PromissumFailable(_, _)
                | Type::Vector(_, _)
                | Type::Matrix(_, _)
                | Type::Sparsa(_, _)
                | Type::Option(_)
                | Type::Ref(_, _)
                | Type::Atomic(_)
                | Type::Intervallum(_)
                | Type::Alias(_, _)
                | Type::Union(_)
                | Type::Tuple(_)
                | Type::ModularWord(_)
        )
    }

    fn is_record_type(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Record(_)
        )
    }

    /// Non-identity conversio involving `Type::Record` must reconstruct.
    ///
    /// Identical Record↔Record `↦` stays (equals). Ratio↔genus, genus↔ratio,
    /// and mismatched Record↔Record have no structural conversion path.
    fn record_conversion_requires_reconstruction(
        &self,
        source_ty: TypeId,
        target_resolved: TypeId,
    ) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        if self.types.equals(source, target_resolved) {
            return false;
        }
        self.is_record_type(source) || self.is_record_type(target_resolved)
    }

    /// Bare failable `↦` propagates through the active alternate-exit channel.
    /// The propagated payload is text in v1, so reject incompatible channels at
    /// typecheck instead of letting Rust codegen or the MIR runner rediscover it.
    fn check_conversio_propagation_channel(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) {
        if !self.conversio_can_propagate_failure(source_ty, target_resolved) {
            return;
        }
        let Some(error_ty) = self.current_error.map(|sink| match sink {
            ErrorSink::Function(ty) | ErrorSink::Local(ty) => ty,
        }) else {
            return;
        };
        let error_ty = self.resolve_inferred_type(error_ty);
        if self.is_infer(error_ty) {
            let textus = self.textus_type();
            self.unify_assign_issue(
                textus,
                error_ty,
                span,
                ISSUE_CONVERSIO_FAILURE_CHANNEL_TYPE_MISMATCH,
            );
            return;
        }
        if matches!(
            self.types.get(error_ty),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        ) {
            return;
        }
        self.invalid_conversion(ISSUE_CONVERSIO_FAILURE_CHANNEL_TEXTUS_REQUIRED, span);
    }

    fn conversio_can_propagate_failure(&self, source_ty: TypeId, target_resolved: TypeId) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        let target = self.resolve_inferred_type(target_resolved);
        if matches!(self.types.get(source), Type::Primitive(Primitive::Valor))
            && !matches!(self.types.get(target), Type::Primitive(Primitive::Valor))
        {
            return true;
        }
        if matches!(self.types.get(target), Type::Primitive(Primitive::Json)) {
            return matches!(
                self.types.get(source),
                Type::Primitive(Primitive::Textus | Primitive::Valor) | Type::Struct(_)
            );
        }
        if matches!(self.types.get(source), Type::Primitive(Primitive::Json))
            && matches!(self.types.get(target), Type::Struct(_))
        {
            return true;
        }
        if self.is_instans_type(target) {
            return matches!(
                self.types.get(source),
                Type::Primitive(Primitive::Textus | Primitive::Valor)
            );
        }
        match (
            self.primitive_conversio_family(source),
            self.primitive_conversio_family(target),
        ) {
            (
                PrimitiveConversioFamily::Textus | PrimitiveConversioFamily::Ascii,
                PrimitiveConversioFamily::Numerus | PrimitiveConversioFamily::Fractus,
            )
            | (
                PrimitiveConversioFamily::Textus | PrimitiveConversioFamily::Octeti,
                PrimitiveConversioFamily::Ascii,
            )
            | (PrimitiveConversioFamily::Octeti, PrimitiveConversioFamily::Textus) => true,
            _ => matches!(
                (self.types.get(source), self.types.get(target)),
                (Type::Array(_), Type::Tensor(_, _)) | (Type::Array(_), Type::BoundedArray { .. })
            ),
        }
    }

    fn is_infallible_truthiness_conversio(
        &self,
        source_ty: TypeId,
        target_resolved: TypeId,
    ) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        let target = self.resolve_inferred_type(target_resolved);
        matches!(
            (
                self.primitive_conversio_family(source),
                self.primitive_conversio_family(target)
            ),
            (
                PrimitiveConversioFamily::Numerus
                    | PrimitiveConversioFamily::Fractus
                    | PrimitiveConversioFamily::Textus
                    | PrimitiveConversioFamily::Ascii,
                PrimitiveConversioFamily::Bivalens
            )
        )
    }

    fn is_instans_type(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
        )
    }

    /// Validate object-literal `∷` field entries before typing the placeholder source.
    ///
    /// Returns `Some(target)` when entries own the evidence (struct/map shapes).
    fn check_verte_object_entries(
        &mut self,
        target_resolved: TypeId,
        entries: Option<&mut Vec<HirObjectField>>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let entries = entries?;
        match self.types.get(target_resolved).clone() {
            Type::Struct(def_id) => {
                // Bare non-generic target (or a generic target whose witnesses
                // could not be resolved): validate against the declared fields.
                self.check_struct_fields(def_id, entries, &[], span);
                Some(target_resolved)
            }
            Type::Applied(base, args) => {
                // The applied target carries the type args; substitute them
                // into the field types.
                if let Type::Struct(def_id) =
                    self.types.get(self.resolve_inferred_type(base)).clone()
                {
                    self.check_struct_fields(def_id, entries, &args, span);
                    return Some(target_resolved);
                }
                None
            }
            Type::Map(key_ty, value_ty) => Some(
                self.check_verte_map_target(entries, key_ty, value_ty, span)
                    .unwrap_or(target_resolved),
            ),
            Type::Record(fields) => {
                self.check_record_fields(&fields, entries, span);
                Some(target_resolved)
            }
            _ => None,
        }
    }

    /// Exact field-set construction for `Type::Record` (ratio and CLI records).
    ///
    /// Unknown init fields reuse `unknown_record_field`; missing labels raise
    /// `ratio_field_missing`; each initializer typechecks against its slot.
    fn check_record_fields(
        &mut self,
        fields: &FxHashMap<Symbol, TypeId>,
        entries: &mut [HirObjectField],
        span: radix_lexer::Span,
    ) {
        let mut supplied = FxHashSet::default();
        for field in entries.iter_mut() {
            match &mut field.key {
                HirObjectKey::Ident(name) | HirObjectKey::String(name) => {
                    let Some(field_ty) = fields.get(name).copied() else {
                        let error_span = field.value.as_ref().map_or(span, |expr| expr.span);
                        self.error_issue(
                            SemanticErrorKind::UndefinedVariable,
                            ISSUE_UNKNOWN_RECORD_FIELD,
                            error_span,
                        );
                        continue;
                    };
                    supplied.insert(*name);
                    let Some(value) = &mut field.value else {
                        self.invalid_operand_types(ISSUE_OBJECT_FIELD_REQUIRES_VALUE, span);
                        continue;
                    };
                    let value_ty = self.check_expr_with_expected(value, Some(field_ty));
                    self.unify_carrier_assign(
                        value_ty,
                        field_ty,
                        value.span,
                        ISSUE_FIELD_INITIALIZER_TYPE_MISMATCH,
                    );
                }
                HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => {
                    self.check_expr(expr);
                    self.error_issue(
                        SemanticErrorKind::UndefinedVariable,
                        ISSUE_UNKNOWN_RECORD_FIELD,
                        expr.span,
                    );
                }
            }
        }
        for name in fields.keys() {
            if !supplied.contains(name) {
                self.undefined_member(ISSUE_RATIO_FIELD_MISSING, span);
            }
        }
    }

    /// Require inline `⇥` recovery to be a value of the conversio target type `T`.
    fn note_conversio_recovery_checked(
        &mut self,
        recovery: &mut HirExpression,
        target_resolved: TypeId,
    ) {
        let recovery_ty = self.check_expr(recovery);
        self.unify_assign_issue(
            recovery_ty,
            target_resolved,
            recovery.span,
            ISSUE_CONVERSIO_RECOVERY_TYPE_MISMATCH,
        );
    }

    /// Validates `↦ regex` operand shapes and rejects structurally empty `⇥` recovery.
    ///
    /// Returns `Some(error_type)` when the operand type is invalid; `None` when
    /// checking should continue toward the declared regex target.
    fn check_regex_conversio(
        &mut self,
        source: &mut HirExpression,
        source_ty: TypeId,
        recovery_present: bool,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        if self.is_ascii(source_resolved) {
            let textus = self.textus_type();
            self.unify_assign_issue(
                source_ty,
                textus,
                source.span,
                ISSUE_REGEX_CONVERSION_OPERAND_TYPE_MISMATCH,
            );
        } else if !self.is_textus(source_resolved) {
            self.invalid_conversion(ISSUE_REGEX_OPERAND_UNSUPPORTED, span);
            return Some(self.error_type);
        }

        // WHY: the `⇥` recovery slot is structurally empty for `↦ regex`.
        // Every coherent `↦ T ⇥ <value>` form requires a recovery of type T
        // (e.g. `"bad" ↦ numerus ⇥ 0`). `regex` has no literal form; its
        // only constructor is the fallible conversion itself, so a recovery
        // would have to be another `pat ↦ regex` — recursion with no base.
        // Pattern validity is a downstream target-engine concern, not a
        // Faber-level conversion failure. A typed fallible regex channel
        // (`⇥`) is a separate, future decision.
        if recovery_present {
            self.invalid_conversion(ISSUE_REGEX_RECOVERY_UNSUPPORTED, span);
        }
        None
    }

    /// Enforce the byte-length cap on literal regex patterns.
    ///
    /// Only folded `HirLiteral::Regex` values reach this check, so dynamic
    /// textus sources keep flowing unchecked. The cap is a policy boundary
    /// ahead of any executable regex engine; it does not make `↦ regex`
    /// fallible and does not inspect pattern shape.
    pub fn check_regex_literal(&mut self, sym: Symbol, span: radix_lexer::Span) -> TypeId {
        let Some(payload) = self.interned_literal_payload(sym) else {
            return self.regex_type();
        };
        let length = payload.len();
        if length > REGEX_PATTERN_MAX_BYTES {
            self.errors.push(
                SemanticError::structured(SemanticErrorKind::InvalidConversion, span)
                    .with_arg("issue", ISSUE_REGEX_PATTERN_TOO_LONG)
                    .with_arg("length", length.to_string())
                    .with_arg("max", REGEX_PATTERN_MAX_BYTES.to_string()),
            );
            return self.error_type;
        }
        self.regex_type()
    }

    /// Checks `∷` / `verte` typed conversion and construction forms.
    ///
    /// The resolved target type decides the validation mode. Inference targets
    /// unify with the source because there is no concrete shape to validate yet.
    /// Constructor sources receive the effective nominal target as expected
    /// context so generic union/class construction substitutes applied type
    /// args into field payloads. The enclosing `expected` supplies witnesses
    /// when the declared target is a bare generic name.
    pub fn check_verte(
        &mut self,
        source: &mut HirExpression,
        target: TypeId,
        entries: Option<&mut Vec<HirObjectField>>,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> TypeId {
        let target_resolved = self.resolve_inferred_type(target);
        // Only nominal struct/enum targets drive constructor substitution.
        // Interface-boxing ascriptions (`Civis { … } ∷ Nominatum`) stay
        // trusting: the concrete construction is checked against its own
        // fields and the interface target is adopted by the cast.
        let target_is_nominal = self.is_nominal_construction_target(target_resolved);
        let constructor_source = target_is_nominal
            && (entries.is_some()
                || self.source_is_variant_constructor(source)
                || matches!(
                    &source.kind,
                    HirExpressionKind::Verte {
                        entries: Some(_),
                        ..
                    }
                ));
        let effective_target =
            self.effective_constructor_target(target_resolved, expected, constructor_source);

        if let Some(result) = self.check_verte_object_entries(effective_target, entries, span) {
            return result;
        }

        let source_expected = constructor_source.then_some(effective_target);
        let source_ty = if matches!(self.types.get(effective_target), Type::ModularWord(_)) {
            // WHY: modular ascription shapes integer literals so the ordinary
            // literal checker enforces the u32 range. Non-literal numerus
            // values remain a different family and require explicit `↦`.
            self.check_expr_with_expected(source, Some(effective_target))
        } else {
            self.check_expr_with_expected(source, source_expected)
        };
        let source_resolved = self.resolve_inferred_type(source_ty);

        if matches!(
            self.types.get(source_resolved),
            Type::Primitive(Primitive::Valor)
        ) {
            self.invalid_conversion(ISSUE_VALOR_VERTE_UNSUPPORTED, span);
            return self.error_type;
        }

        // WHY: an infer target carries no conversion shape, so the source use
        // site is the only evidence available to constrain it.
        if self.is_infer(effective_target) {
            return self.unify_join(source_ty, target, source.span, ISSUE_INVALID_CAST);
        }

        match self.types.get(effective_target).clone() {
            Type::Array(elem_ty) => {
                self.check_verte_array_target(source, elem_ty);
            }
            Type::Vector(elem_ty, width) => {
                self.check_verte_vector_target(source, elem_ty, width, span);
            }
            Type::ModularWord(_) => {}
            // Non-collection targets already typechecked the source above
            // (with the effective target as constructor context when the
            // source is a constructor); re-checking here would duplicate
            // diagnostics for ill-typed sources.
            _ => {}
        }
        effective_target
    }

    /// Whether a `∷` source is a variant-constructor call (finge-lowered
    /// `Call` whose callee path resolves to a variant).
    fn source_is_variant_constructor(&self, source: &HirExpression) -> bool {
        let HirExpressionKind::Call(callee, _, _) = &source.kind else {
            return false;
        };
        let HirExpressionKind::Path(def_id) = &callee.kind else {
            return false;
        };
        self.variant_parent.contains_key(def_id)
    }

    /// Whether a resolved type is a struct/enum construction target (bare or
    /// applied nominal). Interface/collection/other targets are not.
    fn is_nominal_construction_target(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved) {
            Type::Struct(_) | Type::Enum(_) => true,
            Type::Applied(base, _) => matches!(
                self.types.get(self.resolve_inferred_type(*base)),
                Type::Struct(_) | Type::Enum(_)
            ),
            _ => false,
        }
    }

    /// Compute the effective nominal target for a constructor `∷` form.
    ///
    /// An already-applied target is used as-is. A bare `Enum`/`Struct` target
    /// whose def declares type params adopts the enclosing expected type's
    /// witnesses (or fresh inference variables) so payload/field checking can
    /// substitute them. Non-constructor conversions keep the trusting
    /// bare-target behavior.
    fn effective_constructor_target(
        &mut self,
        target_resolved: TypeId,
        expected: Option<TypeId>,
        constructor_source: bool,
    ) -> TypeId {
        if !constructor_source || matches!(self.types.get(target_resolved), Type::Applied(..)) {
            return target_resolved;
        }
        let (def_id, base) = match self.types.get(target_resolved).clone() {
            Type::Enum(def_id) => (def_id, Type::Enum(def_id)),
            Type::Struct(def_id) => (def_id, Type::Struct(def_id)),
            _ => return target_resolved,
        };
        if !self.nominal_is_generic(def_id) {
            return target_resolved;
        }
        let witnesses =
            self.constructor_witnesses(def_id, &[], expected, radix_lexer::Span::default());
        let base_ty = self.types.intern(base);
        self.types.intern(Type::Applied(base_ty, witnesses))
    }

    fn check_verte_array_target(&mut self, source: &mut HirExpression, elem_ty: TypeId) {
        let HirExpressionKind::Array(elements) = &mut source.kind else {
            self.check_expr(source);
            return;
        };

        for element in elements {
            match element {
                HirArrayElement::Expr(expr) => {
                    let element_ty = self.check_expr(expr);
                    self.unify_join(
                        element_ty,
                        elem_ty,
                        expr.span,
                        ISSUE_ARRAY_ELEMENT_TYPE_MISMATCH,
                    );
                }
                HirArrayElement::Spread(expr) => {
                    let spread_ty = self.check_expr(expr);
                    match self.types.get(self.resolve_inferred_type(spread_ty)) {
                        Type::Array(inner) => {
                            self.unify_join(
                                *inner,
                                elem_ty,
                                expr.span,
                                ISSUE_ARRAY_SPREAD_ELEMENT_TYPE_MISMATCH,
                            );
                        }
                        _ => {
                            self.invalid_operand_types(
                                ISSUE_ARRAY_SPREAD_REQUIRES_LISTA,
                                expr.span,
                            );
                        }
                    }
                }
            }
        }
    }

    fn check_verte_vector_target(
        &mut self,
        source: &mut HirExpression,
        elem_ty: TypeId,
        width: crate::IndexId,
        span: radix_lexer::Span,
    ) {
        let HirExpressionKind::Array(elements) = &mut source.kind else {
            self.check_expr(source);
            return;
        };

        if let Some(width) = crate::literal_vector_width(width, self.types) {
            if elements.len() != usize::from(width) {
                self.wrong_arity("vector_constructor_width_arity", span);
            }
        }

        for element in elements {
            match element {
                HirArrayElement::Expr(expr) => {
                    let element_ty = self.check_expr_with_expected(expr, Some(elem_ty));
                    self.unify_join(
                        element_ty,
                        elem_ty,
                        expr.span,
                        ISSUE_VECTOR_ELEMENT_TYPE_MISMATCH,
                    );
                }
                HirArrayElement::Spread(expr) => {
                    self.check_expr(expr);
                    self.invalid_operand_types(ISSUE_VECTOR_CONSTRUCTOR_FORBIDS_SPREAD, expr.span);
                }
            }
        }
    }

    /// Validate tabula `verte` entries. Returns a fully inferred map type when
    /// the declared value slot is still `infer` and entry values supplied evidence.
    fn check_verte_map_target(
        &mut self,
        entries: &mut [HirObjectField],
        key_ty: TypeId,
        value_ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let mut inferred_values = Vec::new();
        for field in entries {
            match &mut field.key {
                HirObjectKey::Ident(_) | HirObjectKey::String(_) => {
                    let textus = self.textus_type();
                    self.unify_join(textus, key_ty, span, ISSUE_MAP_KEY_TYPE_MISMATCH);
                }
                HirObjectKey::Computed(key) => {
                    let actual_key_ty = self.check_expr(key);
                    self.unify_join(actual_key_ty, key_ty, key.span, ISSUE_MAP_KEY_TYPE_MISMATCH);
                }
                HirObjectKey::Spread(expr) => {
                    let spread_ty = self.check_expr(expr);
                    match self
                        .types
                        .get(self.resolve_inferred_type(spread_ty))
                        .clone()
                    {
                        Type::Map(spread_key_ty, spread_value_ty) => {
                            self.unify_join(
                                spread_key_ty,
                                key_ty,
                                expr.span,
                                ISSUE_MAP_SPREAD_KEY_TYPE_MISMATCH,
                            );
                            inferred_values.push(self.resolve_inferred_type(spread_value_ty));
                            if !self.is_infer(self.resolve_inferred_type(value_ty)) {
                                self.unify_join(
                                    spread_value_ty,
                                    value_ty,
                                    expr.span,
                                    ISSUE_MAP_SPREAD_VALUE_TYPE_MISMATCH,
                                );
                            }
                        }
                        _ => {
                            self.invalid_operand_types(
                                ISSUE_OBJECT_SPREAD_REQUIRES_TABULA,
                                expr.span,
                            );
                        }
                    }
                    continue;
                }
            }

            let Some(value) = &mut field.value else {
                self.invalid_operand_types(ISSUE_OBJECT_FIELD_REQUIRES_VALUE, span);
                continue;
            };
            let value_ty_actual = self.check_expr(value);
            inferred_values.push(self.resolve_inferred_type(value_ty_actual));
            let value_resolved = self.resolve_inferred_type(value_ty);
            // WHY: infer/ignotum/union value targets are already broad enough to
            // accept the entry while preserving the explicit map shape.
            if !(self.is_infer(value_resolved)
                || matches!(
                    self.types.get(value_resolved),
                    Type::Primitive(Primitive::Ignotum)
                )
                || matches!(self.types.get(value_resolved), Type::Union(_)))
            {
                self.unify_join(
                    value_ty_actual,
                    value_ty,
                    value.span,
                    ISSUE_MAP_VALUE_TYPE_MISMATCH,
                );
            }
        }

        if !self.is_infer(self.resolve_inferred_type(value_ty)) {
            return None;
        }

        let inferred_value_ty = match inferred_values.as_slice() {
            [] => value_ty,
            [single] => *single,
            _ => self.types.intern(Type::Union(inferred_values)),
        };
        Some(self.types.map(key_ty, inferred_value_ty))
    }
}

fn sparse_tensor_pair(
    source: &Type,
    target: &Type,
) -> Option<(TypeId, crate::IndexId, TypeId, crate::IndexId)> {
    match (source, target) {
        (Type::Tensor(from_elem, from_shape), Type::Sparsa(to_elem, to_shape))
        | (Type::Sparsa(from_elem, from_shape), Type::Tensor(to_elem, to_shape)) => {
            Some((*from_elem, *from_shape, *to_elem, *to_shape))
        }
        _ => None,
    }
}

struct CollectionConversioShape {
    family: CollectionConversioFamily,
    elem: Option<TypeId>,
    key: Option<TypeId>,
    value: Option<TypeId>,
}

// Lane-gated: compiles through the full driver to the Rust target; a
// core-only build excludes the module.
