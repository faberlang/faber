//! Path, assignment target, member, index, and nullability access typing.
//!
//! Access forms sit on the boundary between resolved symbols and expression
//! shape. Name resolution has already chosen `DefId`s; this file decides
//! whether those definitions are usable as values, whether an access is a valid
//! lvalue, and which type is produced by fields, indexes, optional chains, and
//! non-null chains.
//!
//! NULLABILITY AND UNKNOWN POLICY
//! ==============================
//! Optional chaining strips one `optio` layer for the member/index/call check
//! and then wraps the result back in `optio`. Non-null chaining strips one
//! `optio` layer without producing an optional result. `ignotum` and unions are
//! treated as permissive access shapes only where the code explicitly says so;
//! they are recovery/escape behavior, not target-specific codegen policy.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use crate::{AccessMarker, WarningKind};

const ISSUE_TEXTUS_INDEX_MUST_BE_NUMERUS_OR_RANGE: &str = "textus_index_must_be_numerus_or_range";
const ISSUE_UNBOUNDED_TEXTUS_INDEX: &str = "unbounded_textus_index";
const ISSUE_OCTETI_INDEX_MUST_BE_NUMERUS_OR_RANGE: &str = "octeti_index_must_be_numerus_or_range";
const ISSUE_OCTETI_INDEX_OUT_OF_BOUNDS: &str = "octeti_index_out_of_bounds";
const ISSUE_ARRAY_INDEX_MUST_BE_NUMERUS: &str = "array_index_must_be_numerus";
const ISSUE_INDEXING_REQUIRES_ARRAY_OR_MAP: &str = "indexing_requires_array_or_map";
const ISSUE_MAP_INDEX_TYPE_MISMATCH: &str = "map_index_type_mismatch";
const ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS: &str = "tensor_index_vector_integer_elements";
const ISSUE_TENSOR_SCALAR_INDEX_RANK: &str = "tensor_scalar_index_rank";
const ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA: &str = "tensor_bracket_index_integer_or_lista";
const ISSUE_INVALID_ASSIGNMENT_TARGET: &str = "invalid_assignment_target";

impl TypeChecker<'_> {
    /// Checks that an expression can appear on the left side of assignment and
    /// returns the type the assigned value must match.
    ///
    /// Paths must resolve to mutable local bindings; constants report
    /// immutability but still return their declared type so the right-hand side
    /// can be checked. Field and index lvalues reuse the normal access rules.
    pub fn check_lvalue(&mut self, target: &mut HirExpression) -> TypeId {
        let ty = match &mut target.kind {
            HirExpressionKind::Path(def_id) => {
                // WHY: write-once enforcement for `fixum` locals lives in the
                // definite-assignment pass, which sees init state. Typecheck only
                // rejects targets that are never assignable (constants); a
                // `fixum` local may receive its single initializer here.
                if let Some(binding) = self.lookup_binding(*def_id) {
                    binding.ty
                } else if let Some(ty) = self.consts.get(def_id).copied() {
                    self.immutable_assignment("assignment_to_constant", target.span);
                    ty
                } else if let Some(symbol) = self.resolver.get_symbol(*def_id) {
                    // Resolver-registered builtins (e.g. @vertex out_position).
                    if let Some(ty) = symbol.ty {
                        self.insert_binding(*def_id, ty);
                        ty
                    } else {
                        self.error_issue(
                            SemanticErrorKind::AssignmentTargetInvalid,
                            ISSUE_INVALID_ASSIGNMENT_TARGET,
                            target.span,
                        );
                        self.error_type
                    }
                } else {
                    self.error_issue(
                        SemanticErrorKind::AssignmentTargetInvalid,
                        ISSUE_INVALID_ASSIGNMENT_TARGET,
                        target.span,
                    );
                    self.error_type
                }
            }
            HirExpressionKind::Field(object, name) => {
                let ty = self.check_field(object, *name);
                if let Some(object_ty) = object.ty {
                    if let Some(enum_def) = self.enum_def_from_type(object_ty) {
                        if self.resolver.commune_field(enum_def, *name).is_some() {
                            self.immutable_assignment("assignment_to_commune_field", target.span);
                        }
                    }
                }
                ty
            }
            HirExpressionKind::Index(object, index) => self.check_index_lvalue(object, index),
            _ => {
                self.error_issue(
                    SemanticErrorKind::AssignmentTargetInvalid,
                    ISSUE_INVALID_ASSIGNMENT_TARGET,
                    target.span,
                );
                self.error_type
            }
        };
        target.ty = Some(self.resolve_inferred_type(ty));
        ty
    }
    /// Computes the result of indexing an already-typed object with an
    /// already-typed index expression.
    ///
    /// Arrays require numeric indexes, maps unify the index with the map key,
    /// and `textus` accepts numerus/range-like numeric unions while returning
    /// `textus`. Unions and `ignotum` preserve forward progress by producing an
    /// unknown element shape rather than guessing a concrete target layout.
    pub fn check_index_from_type(
        &mut self,
        object_ty: TypeId,
        idx_ty: TypeId,
        object_span: radix_lexer::Span,
        index_span: radix_lexer::Span,
    ) -> TypeId {
        let object_ty = self
            .types
            .peel_access_marker(self.resolve_inferred_type(object_ty))
            .1;
        let resolved = self.resolve_inferred_type(object_ty);
        let resolved_kind = self.types.get(resolved).clone();
        // WHY: textus and ascii scalar index returns `ascii<1>` (D14 / D17),
        // the type of `'x'`. Emit stays `Ascii::from_byte` (zero-alloc).
        // Range indices (e.g. textus[0‥5]) still return a textus substring.
        if matches!(
            resolved_kind,
            Type::Primitive(Primitive::Textus | Primitive::Ascii)
                | Type::BoundedTextus { .. }
                | Type::BoundedAscii { .. }
        ) {
            let valid_index = self.is_textus_index_type(idx_ty);
            if !valid_index {
                self.invalid_operand_types(ISSUE_TEXTUS_INDEX_MUST_BE_NUMERUS_OR_RANGE, index_span);
            }
            if self.is_range_index(idx_ty) {
                return self.types.primitive(Primitive::Textus);
            }
            if valid_index && matches!(&resolved_kind, Type::Primitive(Primitive::Textus)) {
                // WHY: unbounded textus remains UTF-8 and `.chars().nth` is a
                // scalar walk; bounded textus has the O(1) carrier path.
                self.errors.push(
                    SemanticError::structured(
                        SemanticErrorKind::Warning(WarningKind::UnboundedTextusIndex),
                        index_span,
                    )
                    .with_arg("issue", ISSUE_UNBOUNDED_TEXTUS_INDEX),
                );
            }
            return self.ascii_one_type();
        }
        // WHY: octeti[i] is one byte (`numerus<u8>`), O(1). octeti[lo‥hi] is a
        // half-open window that stays `octeti`. Do not reuse the textus return
        // (`ascii` / substring). Bounded `octeti<N>` keeps the same live-byte
        // scalar return; spare `len..N` is not a readable index.
        if matches!(
            resolved_kind,
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. }
        ) {
            if !self.is_textus_index_type(idx_ty) {
                self.invalid_operand_types(ISSUE_OCTETI_INDEX_MUST_BE_NUMERUS_OR_RANGE, index_span);
            }
            if self.is_range_index(idx_ty) {
                return self.types.primitive(Primitive::Octeti);
            }
            return self
                .types
                .sized_numeric(Primitive::Numerus, NumericWidth::U8);
        }
        if let Type::Tensor(elem, shape) = resolved_kind {
            return self.check_tensor_bracket_index(elem, shape, idx_ty, object_span, index_span);
        }
        let kind = match resolved_kind {
            Type::Array(elem) | Type::BoundedArray { element: elem, .. } => {
                Some((Some(elem), None, None))
            }
            Type::Map(key, value) => Some((None, Some(key), Some(value))),
            Type::Tuple(_elements) => {
                // Without the index expression we cannot validate a constant
                // value or determine which element is accessed. Return error
                // type so the caller sees a diagnostic rather than guessing.
                if !self.is_integer(idx_ty) {
                    self.invalid_operand_types(ISSUE_ARRAY_INDEX_MUST_BE_NUMERUS, index_span);
                }
                return self.error_type;
            }
            Type::Union(_) => Some((Some(self.types.primitive(Primitive::Ignotum)), None, None)),
            Type::Primitive(Primitive::Ignotum) => {
                Some((Some(self.types.primitive(Primitive::Ignotum)), None, None))
            }
            Type::Infer(_) => Some((Some(self.fresh_infer()), None, None)),
            _ => None,
        };

        match kind {
            Some((Some(elem), None, None)) => {
                if !self.is_integer(idx_ty) {
                    self.invalid_operand_types(ISSUE_ARRAY_INDEX_MUST_BE_NUMERUS, index_span);
                }
                elem
            }
            Some((None, Some(key), Some(value))) => {
                self.unify_assign_issue(idx_ty, key, index_span, ISSUE_MAP_INDEX_TYPE_MISMATCH);
                value
            }
            _ => {
                self.invalid_operand_types(ISSUE_INDEXING_REQUIRES_ARRAY_OR_MAP, object_span);
                self.error_type
            }
        }
    }
    fn is_textus_index_type(&mut self, idx_ty: TypeId) -> bool {
        if self.is_integer(idx_ty) {
            return true;
        }

        match self.types.get(self.resolve_inferred_type(idx_ty)).clone() {
            Type::Intervallum(bound) => self.is_integer(bound),
            Type::Union(types) => types.into_iter().all(|ty| self.is_integer(ty)),
            _ => false,
        }
    }

    /// Whether the index type is a range (Intervallum), meaning the bracket
    /// access returns a substring rather than a scalar value.
    fn is_range_index(&mut self, idx_ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(idx_ty)),
            Type::Intervallum(_)
        )
    }
    /// DEFER-063: validate a tensor bracket index and return `T ∪ nihil`.
    ///
    /// Bracket access is sugar for `accipe` / `ponde`. The index may be a
    /// scalar integer (rank-1 only) or an integer list (`lista<integer>`).
    /// Scalar index on a statically-known rank != 1 is rejected; non-integer
    /// indices reject with a deliberate diagnostic. Statically provable OOB is
    /// inherited from `accipe` policy (runtime OOB is `nihil` for `accipe`).
    fn check_tensor_bracket_index(
        &mut self,
        elem: TypeId,
        shape: crate::index::IndexId,
        idx_ty: TypeId,
        _object_span: radix_lexer::Span,
        index_span: radix_lexer::Span,
    ) -> TypeId {
        let resolved_idx = self.resolve_inferred_type(idx_ty);
        if crate::tensor_bracket::is_bracket_index_list_type(self.types, resolved_idx) {
            if !crate::numeric_width::is_integer_index_vector(self.types, resolved_idx) {
                self.invalid_operand_types(ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS, index_span);
            }
        } else if crate::numeric_width::is_integer_index_scalar(self.types, resolved_idx) {
            // WHY: a scalar bracket index addresses a single axis, so it is only
            // well-formed on a rank-1 tensor. Rank-N tensors require `[[r, c, …]]`
            // or a `lista<integer>` value. When the shape is an inference hole we
            // defer the rank check to runtime/later inference rather than guess.
            if let Some(rank) = self.static_tensor_rank(shape) {
                if rank != 1 {
                    self.invalid_operand_types(ISSUE_TENSOR_SCALAR_INDEX_RANK, index_span);
                }
            }
        } else {
            self.invalid_operand_types(ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA, index_span);
        }
        if self.systems_device_tensor_bracket_reads_element() {
            return elem;
        }
        self.types.option(elem)
    }

    fn systems_device_tensor_bracket_reads_element(&self) -> bool {
        // WHY: only systems targets consume `@ nucleum` / `@ nucleum fragment`
        // roles as storage-buffer projections. Rust/application targets keep
        // tensor bracket reads nullable via the host `accipe` contract.
        self.current_device_function
            && matches!(
                self.target,
                Target::MirLlvm | Target::MirLlvmHost | Target::MirMetal | Target::MirWgsl
            )
    }

    /// Statically known tensor rank from a shape index, when fully resolved.
    fn static_tensor_rank(&mut self, shape: crate::index::IndexId) -> Option<usize> {
        let resolved = self.resolve_index(shape);
        match self.types.get_index(resolved) {
            crate::index::IndexExpr::Tuple(dims) => Some(dims.len()),
            _ => None,
        }
    }
    /// Resolves field access from a precomputed object type.
    ///
    /// Struct fields come from the collected struct table, maps expose their
    /// value type for property-style access, and records must contain the named
    /// field. Missing record fields are diagnosed here; other unsupported
    /// object shapes return the shared error type so the caller decides whether
    /// a higher-level access form should emit a user-facing diagnostic.
    pub fn check_field_from_type(
        &mut self,
        object_ty: TypeId,
        name: Symbol,
        span: radix_lexer::Span,
    ) -> TypeId {
        if matches!(
            self.types.get(self.resolve_inferred_type(object_ty)),
            Type::Option(_)
        ) {
            self.undefined_member("optional_receiver_requires_chain", span);
            return self.error_type;
        }
        if let Some(struct_def) = self.struct_def_from_type(object_ty) {
            if let Some(info) = self.structs.get(&struct_def) {
                if let Some(field) = info.fields.get(&name).copied() {
                    if let Some(payload_ty) =
                        self.scrinium_payload_field_type(object_ty, struct_def, name)
                    {
                        return payload_ty;
                    }
                    if field.optional {
                        let optional_ty = self.types.option(field.ty);
                        return self.project_ref_view(object_ty, optional_ty);
                    }
                    return self.project_ref_view(object_ty, field.ty);
                }
            }
            self.undefined_member("unknown_struct_field", span);
            return self.error_type;
        }
        if let Some(enum_def) = self.enum_def_from_type(object_ty) {
            if let Some(field) = self.resolver.commune_field(enum_def, name) {
                return self.project_ref_view(object_ty, field.ty);
            }
            if !self.resolver.commune_fields(enum_def).is_empty() {
                self.undefined_member("unknown_union_field", span);
                return self.error_type;
            }
        }
        if matches!(
            self.types.get(self.resolve_inferred_type(object_ty)),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        ) && self
            .interner
            .is_some_and(|interner| interner.resolve(name) == "nuntius")
        {
            return self.types.primitive(Primitive::Textus);
        }
        if let Type::Map(_, value_ty) = self.types.get(self.resolve_inferred_type(object_ty)) {
            return *value_ty;
        }
        if let Type::Record(fields) = self.types.get(self.resolve_inferred_type(object_ty)) {
            if let Some(field_ty) = fields.get(&name).copied() {
                return field_ty;
            }
            self.error_issue(
                SemanticErrorKind::VariableUndefined,
                "unknown_record_field",
                span,
            );
            return self.error_type;
        }
        if matches!(
            self.types.get(self.resolve_inferred_type(object_ty)),
            Type::Tuple(_)
        ) {
            self.undefined_member("unknown_tuple_element_label", span);
            return self.error_type;
        }
        self.error_type
    }

    /// Field projections of a `ref` genus are `ref` views of referential fields.
    fn project_ref_view(&mut self, object_ty: TypeId, field_ty: TypeId) -> TypeId {
        if !self.types.is_ref_view(object_ty) {
            return field_ty;
        }
        if self.types.is_scalar_for_access_marker(field_ty) || self.types.is_ref_view(field_ty) {
            return field_ty;
        }
        self.types.access_marker(AccessMarker::Ref, field_ty)
    }

    /// Own-bind RHS legality (MU-3 / D-6 exclusivity).
    ///
    /// Legal: transfer from `own`/`copy`, or a fresh value. Unmarked paths are
    /// left to the borrow pass (provably-dead is legal; a later use rejects).
    /// Every other shape rejects here with a `copy` suggestion.
    pub(super) fn check_own_bind_rhs(&mut self, rhs: &HirExpression) {
        match &rhs.kind {
            HirExpressionKind::Literal(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::Array(_)
            | HirExpressionKind::Struct(_, _)
            | HirExpressionKind::Tuple(_, _)
            | HirExpressionKind::Call(_, _, _)
            | HirExpressionKind::MethodCall(_, _, _, _)
            | HirExpressionKind::Gradient { .. }
            | HirExpressionKind::Binary(_, _, _)
            | HirExpressionKind::Unary(_, _)
            | HirExpressionKind::Clausura(_, _, _, _, _)
            | HirExpressionKind::Intervallum { .. }
            | HirExpressionKind::Scriptum(_, _)
            | HirExpressionKind::Conversio { .. }
            | HirExpressionKind::Verte { .. }
            | HirExpressionKind::ReadLine(_)
            | HirExpressionKind::Ad { .. }
            | HirExpressionKind::Praefixum(_)
            | HirExpressionKind::TypeCheck { .. }
            | HirExpressionKind::Error => {}
            HirExpressionKind::Path(_) => match self.rhs_access_marker(rhs) {
                Some(AccessMarker::Own | AccessMarker::Copy) | None => {}
                Some(AccessMarker::Ref) => {
                    self.reject_own_bind(crate::ISSUE_REF_INTO_OWN, rhs.span);
                }
                Some(AccessMarker::Mut) => {
                    self.reject_own_bind(crate::ISSUE_MUT_INTO_OWN, rhs.span);
                }
            },
            HirExpressionKind::Field(_, _) => {
                self.reject_own_bind(crate::ISSUE_FIELD_INTO_OWN, rhs.span);
            }
            HirExpressionKind::Index(_, _) => {
                self.reject_own_bind(crate::ISSUE_INDEX_INTO_OWN, rhs.span);
            }
            HirExpressionKind::Deref(_) => {
                self.reject_own_bind(crate::ISSUE_DEREF_INTO_OWN, rhs.span);
            }
            HirExpressionKind::OptionalChain(_, _) | HirExpressionKind::NonNull(_, _) => {
                self.reject_own_bind(crate::ISSUE_PROJECTION_INTO_OWN, rhs.span);
            }
            HirExpressionKind::Ref(_, _) => {
                self.reject_own_bind(crate::ISSUE_REF_INTO_OWN, rhs.span);
            }
            HirExpressionKind::Block(block) => {
                if let Some(expr) = &block.expr {
                    self.check_own_bind_rhs(expr);
                }
            }
            _ => self.reject_own_bind(crate::ISSUE_SHARED_INTO_OWN, rhs.span),
        }
    }

    fn rhs_access_marker(&self, expr: &HirExpression) -> Option<AccessMarker> {
        if let Some(ty) = expr.ty {
            return self
                .types
                .peel_access_marker(self.resolve_inferred_type(ty))
                .0;
        }
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.lookup_binding(*def_id).and_then(|binding| {
                self.types
                    .peel_access_marker(self.resolve_inferred_type(binding.ty))
                    .0
            }),
            _ => None,
        }
    }

    /// Whether `expr` is a declared `ref` root or a projection of one.
    pub(super) fn expr_is_ref_view(&self, expr: &HirExpression) -> bool {
        if let Some(ty) = expr.ty {
            if self.types.is_ref_view(self.resolve_inferred_type(ty)) {
                return true;
            }
        }
        match &expr.kind {
            HirExpressionKind::Field(object, _)
            | HirExpressionKind::Index(object, _)
            | HirExpressionKind::Deref(object) => self.expr_is_ref_view(object),
            HirExpressionKind::Path(def_id) => self
                .lookup_binding(*def_id)
                .is_some_and(|binding| self.types.is_ref_view(binding.ty)),
            _ => false,
        }
    }

    /// Checks a non-null chain segment after removing one optional layer from
    /// the receiver when present.
    ///
    /// This is a type-level assertion for subsequent access only. It does not
    /// insert runtime checks or decide how a backend should represent failed
    /// non-null assertions.
    pub fn check_non_null(
        &mut self,
        object: &mut HirExpression,
        chain: &mut radix_hir::HirNonNullKind,
        span: radix_lexer::Span,
    ) -> TypeId {
        let object_ty = self.check_expr(object);
        let inner_ty = match self.types.get(self.resolve_inferred_type(object_ty)) {
            Type::Option(inner) => *inner,
            _ => object_ty,
        };

        match chain {
            radix_hir::HirNonNullKind::Member(name) => {
                self.check_field_from_type(inner_ty, *name, object.span)
            }
            radix_hir::HirNonNullKind::Index(index) => {
                let idx_ty = self.check_expr_allowing_direct_intervallum_step(index);
                self.check_index_from_type(inner_ty, idx_ty, object.span, index.span)
            }
            radix_hir::HirNonNullKind::Call(args) => {
                self.check_call_from_type(inner_ty, &[], args, span)
            }
        }
    }

    /// Typechecks the receiver in `value!.method()` as a method receiver.
    ///
    /// The parser lowers the non-null member boundary into the callee/receiver
    /// of the surrounding call. It must not be resolved as a field first,
    /// because imported and declared methods are looked up by the ordinary
    /// method-call path. The receiver's resulting type is the asserted payload
    /// type so MIR lowering can preserve the existing non-null assertion.
    pub fn check_non_null_method_receiver(
        &mut self,
        receiver: &mut HirExpression,
        method: Symbol,
    ) -> Option<TypeId> {
        let object = match &mut receiver.kind {
            HirExpressionKind::NonNull(object, radix_hir::HirNonNullKind::Member(name))
                if *name == method =>
            {
                object.as_mut()
            }
            _ => return None,
        };
        let object_ty = self.check_expr(object);
        let inner_ty = match self.types.get(self.resolve_inferred_type(object_ty)) {
            Type::Option(inner) => *inner,
            _ => object_ty,
        };
        receiver.ty = Some(inner_ty);
        Some(inner_ty)
    }
    /// Checks an optional chain segment and wraps the access result in `optio`.
    ///
    /// The receiver is checked once, then an existing option is unwrapped for
    /// the member/index/call rule. A non-optional receiver still produces an
    /// optional result because the syntax's observable contract is nullable
    /// access, not a proof that the receiver was nullable before this point.
    pub fn check_optional_chain(
        &mut self,
        object: &mut HirExpression,
        chain: &mut radix_hir::HirOptionalChainKind,
        span: radix_lexer::Span,
    ) -> TypeId {
        let object_ty = self.check_expr(object);
        let object_resolved = self.resolve_inferred_type(object_ty);

        let inner_ty = match self.types.get(object_resolved) {
            Type::Option(inner) => *inner,
            _ => object_ty,
        };

        let result = match chain {
            radix_hir::HirOptionalChainKind::Member(name) => {
                if matches!(
                    self.types.get(self.resolve_inferred_type(inner_ty)),
                    Type::Primitive(Primitive::Nihil)
                ) {
                    self.types.primitive(Primitive::Ignotum)
                } else {
                    self.check_field_from_type(inner_ty, *name, object.span)
                }
            }
            radix_hir::HirOptionalChainKind::Index(index) => {
                let idx_ty = self.check_expr_allowing_direct_intervallum_step(index);
                self.check_index_from_type(inner_ty, idx_ty, object.span, index.span)
            }
            radix_hir::HirOptionalChainKind::Call(args) => {
                self.check_call_from_type(inner_ty, &[], args, span)
            }
        };

        if matches!(
            self.types.get(self.resolve_inferred_type(result)),
            Type::Option(_)
        ) {
            result
        } else {
            self.types.option(result)
        }
    }
    /// Checks ordinary index syntax by synthesizing both operands first.
    pub fn check_index(&mut self, object: &mut HirExpression, index: &mut HirExpression) -> TypeId {
        let obj_ty = self.check_expr(object);
        let resolved = self.resolve_inferred_type(obj_ty);
        if let Type::Tuple(elements) = self.types.get(resolved).clone() {
            let idx_ty = self.check_expr(index);
            return self.check_tuple_index(&elements, index, object.span, index.span, idx_ty);
        }
        let idx_ty = self.check_expr_allowing_direct_intervallum_step(index);
        let result = self.check_index_from_type(obj_ty, idx_ty, object.span, index.span);
        super::tensor_index::reject_static_bounded_index_oob(self, obj_ty, index);
        self.reject_static_octeti_index_oob(object, index);
        result
    }

    /// Resolve a `Type::Tuple` index access to the element at the given position.
    ///
    /// The index expression must be a compile-time constant integer. Out-of-bounds
    /// and non-constant indices produce diagnostics.
    fn check_tuple_index(
        &mut self,
        elements: &[TypeId],
        index: &HirExpression,
        _object_span: radix_lexer::Span,
        index_span: radix_lexer::Span,
        _idx_ty: TypeId,
    ) -> TypeId {
        let const_idx = if let HirExpressionKind::Literal(HirLiteral::Int(n)) = &index.kind {
            // A literal too large to fit `usize` cannot index any real tuple,
            // so it saturates to `usize::MAX` and falls out of bounds below.
            usize::try_from(*n).unwrap_or(usize::MAX)
        } else {
            self.invalid_operand_types("tuple_index_must_be_constant", index_span);
            return self.error_type;
        };

        if const_idx >= elements.len() {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                "tuple_index_out_of_bounds",
                index_span,
            );
            return self.error_type;
        }

        elements[const_idx]
    }
    /// Check bracket access in assignment position.
    ///
    /// Tensor bracket writes are `ponde` sugar: reads return `T ∪ nihil`, but
    /// writes accept a plain element `T`.
    fn check_index_lvalue(
        &mut self,
        object: &mut HirExpression,
        index: &mut HirExpression,
    ) -> TypeId {
        let obj_ty = self.check_expr(object);
        let idx_ty = self.check_expr_allowing_direct_intervallum_step(index);
        let resolved = self.resolve_inferred_type(obj_ty);
        if let Type::Tensor(elem, shape) = self.types.get(resolved).clone() {
            self.check_tensor_bracket_index(elem, shape, idx_ty, object.span, index.span);
            super::tensor_index::reject_static_bounded_index_oob(self, obj_ty, index);
            return elem;
        }
        if let Type::Tuple(elements) = self.types.get(resolved).clone() {
            return self.check_tuple_index(&elements, index, object.span, index.span, idx_ty);
        }
        let result = self.check_index_from_type(obj_ty, idx_ty, object.span, index.span);
        self.reject_static_octeti_index_oob(object, index);
        result
    }

    /// Reject a compile-time-provable out-of-range index on an octeti literal.
    ///
    /// Runtime OOB still traps. This only fires when both the payload length
    /// and the index (scalar or `lo‥hi`) are constants. A range must lie fully
    /// inside the payload — no short slice, no pad.
    fn reject_static_octeti_index_oob(&mut self, object: &HirExpression, index: &HirExpression) {
        let HirExpressionKind::Literal(HirLiteral::Octeti(sym)) = object.kind else {
            return;
        };
        let Some(raw) = self.interned_literal_payload(sym) else {
            return;
        };
        let Ok(bytes) = radix_lexer::octeti_literal::decode_octeti_hex(raw) else {
            return;
        };
        let len = bytes.len();
        if let Some((start, end)) = static_octeti_index_window(index) {
            if start > end || end > len {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_OCTETI_INDEX_OUT_OF_BOUNDS,
                    index.span,
                );
            }
        }
    }
    /// Checks ordinary field syntax by synthesizing the receiver first.
    pub fn check_field(&mut self, object: &mut HirExpression, name: Symbol) -> TypeId {
        // Namespace const data members (`module.CONST`) are first-class value
        // references: resolve the const's remapped type from the installed
        // file interface, mirroring how the call path resolves namespace
        // functions. Type exports stay type-position-only; structs/enums/
        // aliases referenced as values keep their existing error path.
        if let HirExpressionKind::Path(module_def_id) = &object.kind {
            if let Some(namespace) = self
                .resolver
                .get_symbol(*module_def_id)
                .map(|symbol| symbol.name)
            {
                if let Some(ty) = self
                    .resolver
                    .namespace_file_const_canonical(namespace, name)
                {
                    return ty;
                }
                // Namespace enum-variant value references (`module.VARIANT`)
                // resolve through the imported enum's variant table; the
                // variant's value type is `Type::Enum(consumer enum def)`
                // (codex-gap S1 U2 VALUE members).
                if let Some((_, ty)) = self.resolver.namespace_file_variant(namespace, name) {
                    return ty;
                }
            }
        }
        let obj_ty = self.check_expr(object);
        self.check_field_from_type(obj_ty, name, object.span)
    }

    fn scrinium_payload_field_type(
        &self,
        object_ty: TypeId,
        struct_def: DefId,
        name: Symbol,
    ) -> Option<TypeId> {
        if Some(struct_def) != self.resolver.builtin_scrinium_def_id() {
            return None;
        }
        if self
            .interner
            .is_none_or(|interner| interner.resolve(name) != "data")
        {
            return None;
        }
        match self.types.get(self.resolve_inferred_type(object_ty)) {
            Type::Applied(_, args) => args.first().and_then(|arg| match arg {
                radix_types::AppliedArg::Type(id) => Some(*id),
                radix_types::AppliedArg::Size(_) => None,
            }),
            Type::Struct(_) => Some(self.types.primitive(Primitive::Valor)),
            _ => None,
        }
    }

    /// Converts a resolved definition path into the value type visible at an
    /// expression site.
    ///
    /// Local bindings, constants, functions, enum variants, structs, interfaces,
    /// and modules have different expression roles. Modules deliberately produce
    /// `ignotum` so qualified lookup can keep progressing without pretending the
    /// module itself is a first-class runtime value.
    pub fn check_path(&mut self, def_id: DefId, span: radix_lexer::Span) -> TypeId {
        if let Some(binding) = self.lookup_binding(def_id) {
            return binding.ty;
        }

        if let Some(ty) = self.consts.get(&def_id) {
            return *ty;
        }

        if let Some(sig) = self.functions.get(&def_id) {
            return self.types.function(sig.clone());
        }

        if let Some(parent) = self.variant_parent.get(&def_id).copied() {
            return self.types.intern(Type::Enum(parent));
        }

        // One get_symbol probe; branch on kind / ty without re-hashing DefId.
        if let Some(symbol) = self.resolver.get_symbol(def_id) {
            match symbol.kind {
                crate::SymbolKind::Struct => {
                    return self.types.intern(Type::Struct(def_id));
                }
                crate::SymbolKind::Interface => {
                    return self.types.intern(Type::Interface(def_id));
                }
                crate::SymbolKind::Module => {
                    return self.types.primitive(Primitive::Ignotum);
                }
                _ => {}
            }

            if self.import_receivers.contains(&def_id) {
                return self.types.primitive(Primitive::Ignotum);
            }

            // Resolver-stored types (e.g. @vertex shader position/out_position).
            if let Some(ty) = symbol.ty {
                self.insert_binding(def_id, ty);
                return ty;
            }
        } else if self.import_receivers.contains(&def_id) {
            return self.types.primitive(Primitive::Ignotum);
        }

        self.error_issue(
            SemanticErrorKind::VariableUndefined,
            "unknown_identifier",
            span,
        );
        self.error_type
    }
}

/// Exclusive `[start, end)` window for a constant octeti index, when both
/// endpoints (or the scalar index) are integer literals.
///
/// A scalar `i` is the window `[i, i+1)`. Exclusive `lo‥hi` is `[lo, hi)`.
/// Inclusive `lo…hi` is `[lo, hi+1)`. `None` means the index is not a
/// compile-time integer window.
fn static_octeti_index_window(index: &HirExpression) -> Option<(usize, usize)> {
    match &index.kind {
        HirExpressionKind::Literal(HirLiteral::Int(n)) => {
            let start = usize::try_from(*n).ok()?;
            let end = start.checked_add(1)?;
            Some((start, end))
        }
        HirExpressionKind::Intervallum {
            start, end, kind, ..
        } => {
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
            Some((start, end))
        }
        _ => None,
    }
}
