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

use super::member_privacy::MemberAccess;
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
const ISSUE_WINDOW_REQUIRES_LIST: &str = "window_requires_list";
const ISSUE_MAP_INDEX_TYPE_MISMATCH: &str = "map_index_type_mismatch";
const ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS: &str = "tensor_index_vector_integer_elements";
pub(super) const ISSUE_TENSOR_SCALAR_INDEX_RANK: &str = "tensor_scalar_index_rank";
pub(super) const ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA: &str =
    "tensor_bracket_index_integer_or_lista";
const ISSUE_TENSOR_BRACKET_INDEX_DECIMAL: &str = "tensor_bracket_index_decimal_rejected";
const ISSUE_INVALID_ASSIGNMENT_TARGET: &str = "invalid_assignment_target";

impl TypeChecker<'_> {
    /// Checks that an expression can appear on the left side of assignment and
    /// returns the type the assigned value must match.
    ///
    /// Paths must resolve to mutable local bindings; constants report
    /// immutability but still return their declared type so the right-hand side
    /// can be checked. Field and index lvalues reuse the normal access rules.
    pub fn check_lvalue(&mut self, target: &mut HirExpression) -> TypeId {
        // An index read inside a target is evaluated at the store, so it
        // records no proof here (the store proves its own target).
        self.without_index_proofs(|checker| checker.check_lvalue_place(target))
    }

    fn check_lvalue_place(&mut self, target: &mut HirExpression) -> TypeId {
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
                } else if self.current_size_param_defs.contains(def_id) {
                    // A lexical `magnitudo` param is read-only (size-type Z1).
                    self.immutable_assignment("assignment_to_size_param", target.span);
                    self.types.primitive(Primitive::Numerus)
                } else if let Some(symbol) = self.resolver.get_symbol(*def_id) {
                    // A size param is read-only (size-type Z1 force).
                    if symbol.kind == crate::SymbolKind::IndexParam {
                        self.immutable_assignment("assignment_to_size_param", target.span);
                        self.types.primitive(Primitive::Numerus)
                    // Resolver-registered builtins (e.g. @vertex out_position).
                    } else if let Some(ty) = symbol.ty {
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
                self.place_write = true;
                let ty = self.check_field(object, *name);
                if let Some(object_ty) = object.ty
                    && let Some(enum_def) = self.enum_def_from_type(object_ty)
                    && self.resolver.commune_field(enum_def, *name).is_some()
                {
                    self.immutable_assignment("assignment_to_commune_field", target.span);
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
        self.check_place_write_modifiers(target);
        target.ty = Some(self.resolve_inferred_type(ty));
        ty
    }

    /// Check every field segment an assignment place passes through.
    ///
    /// A `fixum` field behaves like the `fixum` binding form (mutability-engine
    /// D-1): no assignment place may pass through it (`p.x`, `p.inner.y`,
    /// `p.xs[0]`), while mutating methods on it stay legal because they are
    /// calls, not places.
    fn check_place_write_modifiers(&mut self, target: &HirExpression) {
        let span = target.span;
        let mut place = target;
        loop {
            match &place.kind {
                HirExpressionKind::Field(object, name) => {
                    if let Some(object_ty) = object.ty {
                        self.check_field_write_modifier(object_ty, *name, span);
                    }
                    place = object;
                }
                HirExpressionKind::Index(object, _) => place = object,
                _ => break,
            }
        }
    }
    /// Reject a field write the declared modifier forbids (D16.1, D16.3, D9.2).
    ///
    /// Construction literals never reach this path, so a `fixum` field is set
    /// only there. Unmarked fields stay writable until D5c. A union write also
    /// needs one member visibility across the overlap (D9.2, D5.3).
    fn check_field_write_modifier(
        &mut self,
        object_ty: TypeId,
        name: Symbol,
        span: radix_lexer::Span,
    ) {
        let peeled = self
            .types
            .peel_access_marker(self.resolve_inferred_type(object_ty))
            .1;
        if matches!(self.types.get(peeled), Type::Record(_)) {
            self.immutable_assignment_with_args(
                "assignment_to_fixum_field",
                span,
                &[("field", self.member_display_name(name))],
            );
            return;
        }
        if let Some(members) = self.union_members_of(object_ty) {
            // A write through the overlap reaches every member's field, so each
            // one that declares a modifier must declare `varia`.
            let not_varia = members
                .iter()
                .filter(|member| {
                    self.declared_field_modifier(**member, name)
                        .is_some_and(|modifier| modifier != radix_syntax::FieldModifier::Varia)
                })
                .map(|member| self.display_union_constituent(*member))
                .collect::<Vec<_>>();
            self.check_union_write_visibility(&members, name, span);
            if !not_varia.is_empty() {
                self.immutable_assignment_with_args(
                    "union_member_not_varia",
                    span,
                    &[
                        ("member", self.member_display_name(name)),
                        ("members", not_varia.join(", ")),
                    ],
                );
            }
            return;
        }
        match self.declared_field_modifier(object_ty, name) {
            Some(radix_syntax::FieldModifier::Fixum) => {
                self.immutable_assignment_with_args(
                    "assignment_to_fixum_field",
                    span,
                    &[("field", self.member_display_name(name))],
                );
            }
            // A `const` member is a compile-time constant of the type: no
            // store reaches it, through the type name, `self` or an instance.
            Some(radix_syntax::FieldModifier::Const) => {
                self.immutable_assignment_with_args(
                    "assignment_to_const_member",
                    span,
                    &[("field", self.member_display_name(name))],
                );
            }
            _ => {}
        }
    }

    /// The authored modifier of a `genus` field reached through `ty`, if any.
    fn declared_field_modifier(
        &self,
        ty: TypeId,
        name: Symbol,
    ) -> Option<radix_syntax::FieldModifier> {
        let peeled = self
            .types
            .peel_access_marker(self.resolve_inferred_type(ty))
            .1;
        let struct_def = self.struct_def_from_type(peeled)?;
        self.structs.get(&struct_def)?.fields.get(&name)?.modifier
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
        // WHY: `ascii[i]` returns `littera` (D10.3); `textus[i]` returns
        // `ascii<1>` (D14 / D17) until operator fork F5 rules. Range indices (e.g. textus[0‥5]) still return a textus substring.
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
            // D10.3: `ascii[i]` is one scalar (`littera`, O(1)). F5 held:
            // `textus[i]` keeps its `ascii<1>` row (and non-ASCII trap).
            if matches!(
                resolved_kind,
                Type::Primitive(Primitive::Ascii) | Type::BoundedAscii { .. }
            ) {
                return self.text_element_type();
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
            return self.byte_element_type();
        }
        // WHY: `xs[lo‥hi]` is a window on every list (ruling B10 of need
        // 6a13d711): a new `lista<T>` holding the range, fully in bounds or a
        // failure at run time. A range over anything that is not a list, text
        // or octeti is one named issue.
        if self.is_range_index(idx_ty) {
            match &resolved_kind {
                Type::Array(elem)
                | Type::BoundedArray { element: elem, .. }
                | Type::RangedArray { element: elem, .. } => {
                    if !self.is_textus_index_type(idx_ty) {
                        self.invalid_operand_types(ISSUE_ARRAY_INDEX_MUST_BE_NUMERUS, index_span);
                    }
                    return self.types.array(*elem);
                }
                Type::Tensor(..) | Type::Vector(..) | Type::Tuple(_) | Type::Primitive(_) => {
                    self.invalid_operand_types(ISSUE_WINDOW_REQUIRES_LIST, object_span);
                    return self.error_type;
                }
                _ => {}
            }
        }
        if let Type::Tensor(elem, shape) | Type::Matrix(elem, shape) = resolved_kind {
            return self.check_tensor_bracket_index(elem, shape, idx_ty, index_span);
        }
        // S0-G8: vector lane access. A register vector lane is an O(1) scalar
        // read — `v[i]` yields the element directly (no nullable wrap), at
        // any width including parametric ones; the index must be integer.
        if let Type::Vector(elem, _) = resolved_kind {
            if !self.is_integer(idx_ty) {
                self.invalid_operand_types(ISSUE_ARRAY_INDEX_MUST_BE_NUMERUS, index_span);
            }
            return elem;
        }
        let kind = match resolved_kind {
            Type::Array(elem)
            | Type::BoundedArray { element: elem, .. }
            | Type::RangedArray { element: elem, .. } => Some((Some(elem), None, None)),
            Type::Map(key, value) | Type::Ledger(key, value) => {
                Some((None, Some(key), Some(value)))
            }
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
            Type::Intervallum(bound, _) => self.is_integer(bound),
            Type::Union(types) => types.into_iter().all(|ty| self.is_integer(ty)),
            _ => false,
        }
    }

    /// Whether the index type is a range (Intervallum), meaning the bracket
    /// access returns a substring rather than a scalar value.
    fn is_range_index(&mut self, idx_ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(idx_ty)),
            Type::Intervallum(..)
        )
    }
    /// Validate a tensor bracket index and return the element type `T`.
    ///
    /// A bracket read always yields `T` or traps, like a list index (operator
    /// ruling 2026-10-02): a computed bad index traps at runtime and a literal
    /// bad index is a compile error. The index may be a scalar integer (rank-1
    /// only) or an integer list (`lista<integer>`). Scalar index on a
    /// statically-known rank != 1 is rejected; non-integer indices reject with
    /// a deliberate diagnostic. The nullable `.accipe(...)` method is separate.
    fn check_tensor_bracket_index(
        &mut self,
        elem: TypeId,
        shape: crate::index::IndexId,
        idx_ty: TypeId,
        index_span: radix_lexer::Span,
    ) -> TypeId {
        let resolved_idx = self.resolve_inferred_type(idx_ty);
        // decimal-widths D2 (wildcard audit): decimal index scalars reject
        // with their own teaching issue, not the generic integer-or-lista one.
        if crate::numeric_width::is_decimal_index_scalar(self.types, resolved_idx) {
            self.invalid_operand_types(ISSUE_TENSOR_BRACKET_INDEX_DECIMAL, index_span);
        } else if crate::tensor_bracket::is_bracket_index_list_type(self.types, resolved_idx) {
            if !crate::numeric_width::is_integer_index_vector(self.types, resolved_idx) {
                self.invalid_operand_types(ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS, index_span);
            }
        } else if crate::numeric_width::is_integer_index_scalar(self.types, resolved_idx) {
            // WHY: a scalar bracket index addresses a single axis, so it is only
            // well-formed on a rank-1 tensor. Rank-N tensors require `[[r, c, …]]`
            // or a `lista<integer>` value. When the shape is an inference hole we
            // defer the rank check to runtime/later inference rather than guess.
            if let Some(rank) = self.static_tensor_rank(shape)
                && rank != 1
            {
                self.invalid_operand_types(ISSUE_TENSOR_SCALAR_INDEX_RANK, index_span);
            }
        } else {
            self.invalid_operand_types(ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA, index_span);
        }
        elem
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
        self.check_field_access_from_type(object_ty, name, span, MemberAccess::Read)
    }

    /// Field access from a precomputed object type, gated by the member
    /// visibility of each genus the access reaches (D5.3).
    pub(super) fn check_field_access_from_type(
        &mut self,
        object_ty: TypeId,
        name: Symbol,
        span: radix_lexer::Span,
        access: MemberAccess,
    ) -> TypeId {
        self.check_field_visibility(object_ty, name, access, span);
        self.field_type_from_type(object_ty, name, span)
    }

    fn field_type_from_type(
        &mut self,
        object_ty: TypeId,
        name: Symbol,
        span: radix_lexer::Span,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(object_ty);
        // An access-marker wrapper rides the receiver (Option A: a `ref`
        // cursor over `T ∪ none` reads through the marker); the optionality
        // test sees the payload.
        let (_, payload) = self.types.peel_access_marker(resolved);
        if matches!(self.types.get(payload), Type::Option(_)) {
            self.undefined_member("optional_receiver_requires_chain", span);
            return self.error_type;
        }
        if let Some(field_ty) = self.variant_field_from_type(object_ty, name) {
            return self.project_ref_view(object_ty, field_ty);
        }
        if let Some(struct_def) = self.struct_def_from_type(object_ty) {
            if let Some(info) = self.structs.get(&struct_def)
                && let Some(field) = info.fields.get(&name).copied()
            {
                if let Some(payload_ty) =
                    self.scrinium_payload_field_type(object_ty, struct_def, name)
                {
                    return payload_ty;
                }
                // S0-G8: an applied generic nominal projects its field
                // types through the application — `Box<3>.minima` is
                // `vector<f32, size 3>`, not the declared `vector<f32, size N>`.
                let field_ty = self.applied_field_type(object_ty, struct_def, field.ty);
                if field.optional {
                    let optional_ty = self.types.option(field_ty);
                    return self.project_ref_view(object_ty, optional_ty);
                }
                return self.project_ref_view(object_ty, field_ty);
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
        if let Some(field_ty) = self.nuntius_field_type(object_ty, name) {
            return field_ty;
        }
        if let Type::Map(_, value_ty) | Type::Ledger(_, value_ty) = self.types.get(resolved) {
            return *value_ty;
        }
        if let Type::Record(fields) = self.types.get(resolved) {
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
        if matches!(self.types.get(resolved), Type::Tuple(_)) {
            self.undefined_member("unknown_tuple_element_label", span);
            return self.error_type;
        }
        // U1R common-member rule: a `∪` receiver exposes only members every
        // constituent exposes with one agreeing type. The miss is an
        // attributed error naming the constituents that lack the member —
        // not the silent ignotum sink this path used to fall through to.
        if let Some(members) = self.union_members_of(object_ty) {
            return self.check_union_field_from_type(&members, name, span);
        }
        self.report_fieldless_type_access(object_ty, name, span);
        self.error_type
    }

    /// Quiet genus-field projection: the struct arm of
    /// [`Self::field_type_from_type`] with no diagnostic anywhere.
    ///
    /// The field-path narrowing producer needs a named genus field's type on a
    /// best-effort basis; any shape that is not an ordinary struct field
    /// (optional receiver, union, map, record, unknown field) yields `None`
    /// instead of an error.
    pub(super) fn try_genus_field_type(
        &mut self,
        object_ty: TypeId,
        name: Symbol,
    ) -> Option<TypeId> {
        let resolved = self.resolve_inferred_type(object_ty);
        if matches!(self.types.get(resolved), Type::Option(_)) {
            return None;
        }
        let struct_def = self.struct_def_from_type(object_ty)?;
        let field = self.structs.get(&struct_def)?.fields.get(&name).copied()?;
        let field_ty = self.applied_field_type(object_ty, struct_def, field.ty);
        let field_ty = if field.optional {
            self.types.option(field_ty)
        } else {
            field_ty
        };
        Some(self.project_ref_view(object_ty, field_ty))
    }

    /// The `.nuntius` built-in field read: `textus` and `ignotum` receivers
    /// expose the message text as a field. Shared by the ordinary field rule
    /// and the union common-member rule (U1R); one of the two hard-coded
    /// field exceptions named in [`Self::report_fieldless_type_access`].
    fn nuntius_field_type(&self, receiver_ty: TypeId, name: Symbol) -> Option<TypeId> {
        if !matches!(
            self.types.get(self.resolve_inferred_type(receiver_ty)),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        ) {
            return None;
        }
        if self
            .interner
            .is_none_or(|interner| interner.resolve(name) != "nuntius")
        {
            return None;
        }
        Some(self.types.primitive(Primitive::Textus))
    }

    /// Named diagnostic for a field access on a type that has no such field.
    ///
    /// This is the final fall-through of [`Self::field_type_from_type`]: the
    /// receiver is a built-in or otherwise fieldless type (`1.nonsense`,
    /// `"a".length`, `xs.length`). Built-in types expose methods, not fields.
    /// Two exceptions are hard-coded earlier in the function and never reach
    /// here: `.nuntius` on `textus`/`ignotum` and dot-key lookup on map types.
    /// A receiver that is already an error, an unsolved hole, or an unsubstituted
    /// parameter stays silent so the first error is not cascaded.
    fn report_fieldless_type_access(
        &mut self,
        object_ty: TypeId,
        name: Symbol,
        span: radix_lexer::Span,
    ) {
        let resolved = self.resolve_inferred_type(object_ty);
        let receiver = self.types.get(resolved).clone();
        if matches!(
            receiver,
            Type::Error
                | Type::Infer(_)
                | Type::InferUnion(_)
                | Type::MarkerHole { .. }
                | Type::Param(_)
        ) {
            return;
        }
        let member = self.member_display_name(name);
        let is_length = matches!(member.as_str(), "length" | "longitudo");
        let sequence_receiver = matches!(
            receiver,
            Type::Primitive(Primitive::Textus | Primitive::Ascii)
                | Type::BoundedTextus { .. }
                | Type::BoundedAscii { .. }
                | Type::Array(_)
                | Type::BoundedArray { .. }
                | Type::RangedArray { .. }
        );
        if is_length && sequence_receiver {
            self.undefined_member("length_is_method", span);
            return;
        }
        let type_name = self.display_union_constituent(resolved);
        self.undefined_member_with_args(
            "type_has_no_field",
            span,
            &[("member", member), ("type", type_name)],
        );
    }

    /// Overlap field access on a `∪` receiver (U1R, D9.2).
    ///
    /// Every constituent must expose the field (through the same declared
    /// member views single-receiver access uses) with exactly the same type.
    /// A constituent that lacks the field is named in
    /// `union_member_not_common`; when every constituent has it but the types
    /// disagree, `union_member_differs` names each constituent's type.
    fn check_union_field_from_type(
        &mut self,
        members: &[TypeId],
        name: Symbol,
        span: radix_lexer::Span,
    ) -> TypeId {
        let mut lacking: Vec<String> = Vec::new();
        let mut found: Vec<(TypeId, TypeId)> = Vec::new();
        for member in members {
            match self.union_constituent_field_type(*member, name) {
                Some(field_ty) => found.push((*member, field_ty)),
                None => lacking.push(self.display_union_constituent(*member)),
            }
        }
        if !lacking.is_empty() {
            self.undefined_member_with_args(
                "union_member_not_common",
                span,
                &[
                    ("member", self.member_display_name(name)),
                    ("lacking", lacking.join(", ")),
                ],
            );
            return self.error_type;
        }
        let Some(&(_, common)) = found.first() else {
            return self.error_type;
        };
        if found
            .iter()
            .all(|(_, field_ty)| self.member_type_equals(common, *field_ty))
        {
            return common;
        }
        let detail = found
            .iter()
            .map(|(member, field_ty)| {
                format!(
                    "`{}` in {}",
                    self.format_union_constituent_type(*field_ty),
                    self.display_union_constituent(*member)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        self.undefined_member_with_args(
            "union_member_differs",
            span,
            &[
                ("member", self.member_display_name(name)),
                ("detail", detail),
            ],
        );
        self.error_type
    }

    /// Project a field carried by one enum variant represented as a nominal
    /// `Type::Struct(variant_def)`. Variant payload metadata is collected in
    /// parallel tables because variants are values of their parent enum at
    /// construction sites, not standalone declarations in the struct table.
    fn variant_field_from_type(&self, object_ty: TypeId, name: Symbol) -> Option<TypeId> {
        let resolved = self.resolve_inferred_type(object_ty);
        let Type::Struct(variant_def) = self.types.get(resolved) else {
            return None;
        };
        let field_index = self
            .variant_field_names
            .get(variant_def)?
            .iter()
            .position(|field_name| *field_name == name)?;
        self.variant_fields
            .get(variant_def)?
            .get(field_index)
            .copied()
    }

    /// One union constituent's field type for the common-member computation,
    /// or `None` when the constituent does not expose the field (U1R).
    fn union_constituent_field_type(&mut self, member: TypeId, name: Symbol) -> Option<TypeId> {
        if let Some(field_ty) = self.variant_field_from_type(member, name) {
            return Some(field_ty);
        }
        if let Some(struct_def) = self.struct_def_from_type(member) {
            let field = self.structs.get(&struct_def)?.fields.get(&name).copied()?;
            return Some(self.applied_field_type(member, struct_def, field.ty));
        }
        if let Some(enum_def) = self.enum_def_from_type(member) {
            return self.resolver.commune_field(enum_def, name).map(|f| f.ty);
        }
        if let Some(field_ty) = self.nuntius_field_type(member, name) {
            return Some(field_ty);
        }
        if let Type::Map(_, value_ty) | Type::Ledger(_, value_ty) =
            self.types.get(self.resolve_inferred_type(member))
        {
            return Some(*value_ty);
        }
        if let Type::Record(fields) = self.types.get(self.resolve_inferred_type(member)) {
            return fields.get(&name).copied();
        }
        None
    }

    /// Display name of a union constituent for attribution diagnostics (U1R).
    pub(crate) fn display_union_constituent(&self, member: TypeId) -> String {
        let resolved = self.resolve_inferred_type(member);
        match self.types.get(resolved) {
            Type::Struct(def_id) | Type::Interface(def_id) | Type::Enum(def_id) => self
                .resolver
                .get_symbol(*def_id)
                .and_then(|symbol| {
                    self.interner
                        .map(|interner| interner.resolve(symbol.name).to_owned())
                })
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| self.format_union_constituent_type(resolved)),
            _ => self.format_union_constituent_type(resolved),
        }
    }

    /// Substitute an applied generic nominal's arguments into a field type.
    ///
    /// S0-G8: `Type::Applied` size arguments bind magnitudo slots exactly as
    /// construction does (S0-G1) — `Box<3>` fields surface with width 3.
    /// Non-applied receivers keep the declared field type unchanged.
    pub(super) fn applied_field_type(
        &mut self,
        object_ty: TypeId,
        struct_def: DefId,
        field_ty: TypeId,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(object_ty);
        let Type::Applied(_, args) = self.types.get(resolved).clone() else {
            return field_ty;
        };
        let Some(params) = self.type_params_by_def.get(&struct_def) else {
            return field_ty;
        };
        let (mut vars, mut shape_vars) = applied_argument_bindings(params, &args);
        if vars.is_empty() && shape_vars.is_empty() {
            return field_ty;
        }
        self.substitute_type_params(field_ty, &mut vars, Some(&mut shape_vars))
    }

    /// Instantiate an applied generic nominal's arguments into a method
    /// signature.
    ///
    /// A method of `class Box<T, size N>` declares its parameter and return
    /// types over the class's `T` and `N`; on a `Box<int, 3>` receiver those
    /// are the receiver's type and size witnesses, exactly as
    /// `applied_field_type` projects them for fields. The method's own
    /// generics keep their declared form (identity); non-applied receivers
    /// return the signature unchanged.
    pub(super) fn applied_method_signature(
        &mut self,
        receiver_ty: TypeId,
        sig: FuncSig,
    ) -> FuncSig {
        let resolved = self.resolve_inferred_type(receiver_ty);
        let (_, resolved) = self.types.peel_access_marker(resolved);
        let Type::Applied(_, args) = self.types.get(resolved).clone() else {
            return sig;
        };
        let Some(struct_def) = self.struct_def_from_type(receiver_ty) else {
            return sig;
        };
        let Some(params) = self.type_params_by_def.get(&struct_def).cloned() else {
            return sig;
        };
        let (mut vars, mut shape_vars) = applied_argument_bindings(&params, &args);
        if vars.is_empty() && shape_vars.is_empty() {
            return sig;
        }
        // A method's own generic of the same name shadows the class's.
        for param in &sig.type_params {
            shape_vars.remove(&param.name);
            let identity = self.types.intern(Type::Param(param.name));
            vars.insert(param.name, identity);
        }
        self.substitute_func_sig(&sig, &mut vars, Some(&mut shape_vars))
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
            | HirExpressionKind::Call(_, _, _, _)
            | HirExpressionKind::MethodCall(_, _, _, _, _)
            | HirExpressionKind::Gradient { .. }
            | HirExpressionKind::Binary(_, _, _)
            | HirExpressionKind::Unary(_, _)
            | HirExpressionKind::Clausura(..)
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

    /// The place a method call writes through. In `value!.method()` the
    /// receiver is the assertion `NonNull(value, Value)`; the place is
    /// `value`. Any other receiver is its own place.
    pub(super) fn method_receiver_place(receiver: &HirExpression) -> &HirExpression {
        match &receiver.kind {
            HirExpressionKind::NonNull(object, radix_hir::HirNonNullKind::Value) => object,
            _ => receiver,
        }
    }

    /// Whether `expr` is a declared `ref` root or a projection of one.
    pub(super) fn expr_is_ref_view(&self, expr: &HirExpression) -> bool {
        if let Some(ty) = expr.ty
            && self.types.is_ref_view(self.resolve_inferred_type(ty))
        {
            return true;
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
            radix_hir::HirNonNullKind::Value => inner_ty,
            radix_hir::HirNonNullKind::Member(name) => {
                self.check_field_from_type(inner_ty, *name, object.span)
            }
            radix_hir::HirNonNullKind::Index(index) => {
                if let Some(element) = self.check_tuple_receiver_index(inner_ty, object.span, index)
                {
                    return element;
                }
                let idx_ty = self.check_expr_allowing_direct_intervallum_step(index);
                let result = self.check_index_from_type(inner_ty, idx_ty, object.span, index.span);
                super::tensor_index::check_vector_lane_index(self, inner_ty, index);
                result
            }
            radix_hir::HirNonNullKind::Call(args) => {
                self.check_call_from_type(inner_ty, &[], args, span)
            }
        }
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
                if let Some(element) = self.check_tuple_receiver_index(inner_ty, object.span, index)
                {
                    element
                } else {
                    let idx_ty = self.check_expr_allowing_direct_intervallum_step(index);
                    let result =
                        self.check_index_from_type(inner_ty, idx_ty, object.span, index.span);
                    super::tensor_index::check_vector_lane_index(self, inner_ty, index);
                    result
                }
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
        if self
            .tensor_edge_policies
            .shifted_expressions
            .contains(&object.id)
        {
            self.check_expr(index);
            self.invalid_operand_types(
                super::intrinsics::ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION,
                object.span,
            );
            return self.error_type;
        }
        if let Some(element) = self.check_tuple_receiver_index(obj_ty, object.span, index) {
            return element;
        }
        let resolved = self.resolve_inferred_type(obj_ty);
        // WHY: a bracket range on a tensor is a sliced tensor, not a list
        // window, so it routes to the tensor range checker before the
        // composite-key path (a range is never a tuple key) and before the
        // window decline in `check_index_from_type`.
        let (_, payload) = self.types.peel_access_marker(resolved);
        if let Type::Tensor(elem, shape) = self.types.get(payload).clone() {
            if matches!(index.kind, HirExpressionKind::Intervallum { .. }) {
                return super::tensor_index::check_tensor_bracket_range_index(
                    self, obj_ty, elem, shape, index,
                );
            }
            if matches!(index.kind, HirExpressionKind::Array(_)) {
                return super::tensor_index::check_tensor_bracket_slot_list_index(
                    self, obj_ty, elem, shape, index,
                );
            }
        }
        if let Type::Matrix(elem, shape) = self.types.get(payload).clone()
            && let HirExpressionKind::Array(elements) = &index.kind
            && elements.iter().any(|element| {
                matches!(
                    element,
                    HirArrayElement::Spread(_) | HirArrayElement::Infer(_)
                ) || matches!(
                    element,
                    HirArrayElement::Expr(expr)
                        if matches!(expr.kind, HirExpressionKind::Intervallum { .. })
                )
            })
        {
            return super::tensor_index::check_matrix_bracket_slot_list_index(
                self, obj_ty, elem, shape, index,
            );
        }
        // WHY: `v[lo ‥ hi]` on a register vector is a vector slice whose width
        // is computed at compile time from literal bounds (operator ruling
        // 2026-10-17: a slice stays in its family), not a `lista` window, so it
        // routes before the window decline in `check_index_from_type`.
        if let Type::Vector(elem, width) = self.types.get(payload).clone()
            && matches!(index.kind, HirExpressionKind::Intervallum { .. })
        {
            return super::tensor_index::check_vector_bracket_range_index(self, elem, width, index);
        }
        let idx_ty = match self.check_tuple_key_bracket(resolved, index) {
            Some(ty) => ty,
            None => self.check_expr_allowing_direct_intervallum_step(index),
        };
        let result = self.check_index_from_type(obj_ty, idx_ty, object.span, index.span);
        super::tensor_index::reject_static_bounded_index_oob(self, obj_ty, index);
        super::tensor_index::warn_unproven_kernel_index(self, obj_ty, index);
        super::tensor_index::check_vector_lane_index(self, obj_ty, index);
        self.reject_static_octeti_index_oob(object, index);
        result
    }

    /// Composite-key lookup (D3.2/D3.3): on a `tabula<iuncta<K1, …, Kn>, V>`
    /// the bracket list is the key, so `m[[k1, …, kn]]` reads the entry for
    /// that `iuncta`. A plain list literal of exactly n elements becomes the
    /// tuple construction checked against the key type; any other index
    /// shape returns `None` and keeps the ordinary map-index rule (which
    /// reports a wrong part count as `map_index_type_mismatch`).
    fn check_tuple_key_bracket(
        &mut self,
        object_ty: TypeId,
        index: &mut HirExpression,
    ) -> Option<TypeId> {
        let map_ty = self
            .types
            .peel_access_marker(self.resolve_inferred_type(object_ty))
            .1;
        let (Type::Map(key, _) | Type::Ledger(key, _)) = self.types.get(map_ty).clone() else {
            return None;
        };
        let mut peeled = key;
        while let Type::Alias(_, inner) = self.types.get(peeled) {
            peeled = *inner;
        }
        let Type::Tuple(parts) = self.types.get(peeled) else {
            return None;
        };
        let HirExpressionKind::Array(elements) = &mut index.kind else {
            return None;
        };
        if elements.len() != parts.len()
            || elements.iter().any(|element| {
                matches!(
                    element,
                    HirArrayElement::Spread(_) | HirArrayElement::Infer(_)
                )
            })
        {
            return None;
        }
        // Spreads and holes were excluded above, so every element is an expression.
        let items = std::mem::take(elements)
            .into_iter()
            .filter_map(|element| match element {
                HirArrayElement::Expr(expr) => Some(expr),
                HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => None,
            })
            .collect();
        index.kind = HirExpressionKind::Tuple(items, None);
        let ty = self.check_expr_with_expected(index, Some(key));
        index.ty = Some(ty);
        Some(ty)
    }

    /// Index access on a tuple receiver, shared by `t[i]`, `t![i]` and `t?[i]`.
    ///
    /// Returns `None` when the receiver is not a tuple, leaving the ordinary
    /// index rule to the caller. `check_index_from_type` cannot resolve a
    /// tuple element on its own: it never sees the index expression, so the
    /// constant position is only available here.
    fn check_tuple_receiver_index(
        &mut self,
        receiver_ty: TypeId,
        object_span: radix_lexer::Span,
        index: &mut HirExpression,
    ) -> Option<TypeId> {
        let resolved = self.resolve_inferred_type(receiver_ty);
        let Type::Tuple(elements) = self.types.get(resolved).clone() else {
            return None;
        };
        let idx_ty = self.check_expr_allowing_direct_intervallum_step(index);
        if self.is_range_index(idx_ty) {
            self.invalid_operand_types(ISSUE_WINDOW_REQUIRES_LIST, object_span);
            return Some(self.error_type);
        }
        Some(self.check_tuple_index(&elements, index, index.span))
    }

    /// Resolve a `Type::Tuple` index access to the element at the given position.
    ///
    /// The index expression must be a compile-time constant integer. Out-of-bounds
    /// and non-constant indices produce diagnostics.
    fn check_tuple_index(
        &mut self,
        elements: &[TypeId],
        index: &HirExpression,
        index_span: radix_lexer::Span,
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
    /// Tensor bracket writes are `ponde` sugar; reads and writes both use the
    /// plain element `T`.
    fn check_index_lvalue(
        &mut self,
        object: &mut HirExpression,
        index: &mut HirExpression,
    ) -> TypeId {
        // `p.xs[0] ← …` writes through the field `xs`.
        self.place_write = matches!(object.kind, HirExpressionKind::Field(..));
        let obj_ty = self.check_expr(object);
        self.place_write = false;
        let idx_ty = match self.check_tuple_key_bracket(obj_ty, index) {
            Some(ty) => ty,
            None => self.check_expr_allowing_direct_intervallum_step(index),
        };
        // The access marker (`copy`, `own`, ...) is a binding fact: peel it so
        // a marked tensor takes the same element write as an unmarked one.
        let resolved = self
            .types
            .peel_access_marker(self.resolve_inferred_type(obj_ty))
            .1;
        if let Type::Tensor(elem, shape) | Type::Matrix(elem, shape) =
            self.types.get(resolved).clone()
        {
            self.check_tensor_bracket_index(elem, shape, idx_ty, index.span);
            super::tensor_index::reject_static_bounded_index_oob(self, obj_ty, index);
            super::tensor_index::warn_unproven_kernel_index(self, obj_ty, index);
            return elem;
        }
        if let Type::Tuple(elements) = self.types.get(resolved).clone() {
            return self.check_tuple_index(&elements, index, index.span);
        }
        // A window is a read: a range index on a list write keeps the
        // integer-index refusal.
        if self.is_range_index(idx_ty)
            && matches!(
                self.types.get(resolved),
                Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
            )
        {
            self.invalid_operand_types(ISSUE_ARRAY_INDEX_MUST_BE_NUMERUS, index.span);
            return self.error_type;
        }
        let result = self.check_index_from_type(obj_ty, idx_ty, object.span, index.span);
        super::tensor_index::check_vector_lane_index(self, obj_ty, index);
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
        if let Some((start, end)) = static_octeti_index_window(index)
            && (start > end || end > len)
        {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_OCTETI_INDEX_OUT_OF_BOUNDS,
                index.span,
            );
        }
    }
    /// Checks ordinary field syntax by synthesizing the receiver first.
    ///
    /// Inside an assignment place (`place_write`, set by `check_lvalue`) the
    /// access is a write, and so is every field segment the place passes
    /// through (`p.inner.y ← …` writes through `inner`).
    pub fn check_field(&mut self, object: &mut HirExpression, name: Symbol) -> TypeId {
        let write = std::mem::take(&mut self.place_write);
        // Namespace const data members (`module.CONST`) are first-class value
        // references: resolve the const's remapped type from the installed
        // file interface, mirroring how the call path resolves namespace
        // functions. Type exports stay type-position-only; structs/enums/
        // aliases referenced as values keep their existing error path.
        if let HirExpressionKind::Path(module_def_id) = &object.kind
            && let Some(namespace) = self
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
            // A namespace function referenced as a value: an `@ nondum`
            // export is refused here, the same as a call through the namespace.
            if let Some(gates) = self
                .resolver
                .namespace_file_unavailable_canonical(namespace, name)
                .map(<[_]>::to_vec)
            {
                let qualified = self.qualified_function_name(namespace, name);
                if self.report_unavailable_function(&gates, qualified, object.span) {
                    return self.error_type;
                }
            }
            // Namespace enum-variant value references (`module.VARIANT`)
            // resolve through the imported enum's variant table; the
            // variant's value type is `Type::Enum(consumer enum def)`
            // (codex-gap S1 U2 VALUE members).
            if let Some((_, ty)) = self.resolver.namespace_file_variant(namespace, name) {
                return ty;
            }
        }
        // WHY: `ego` also lowers to `Path(genus)`, so inside the genus's own
        // members the path is ambiguous between the type name and the receiver
        // and keeps the receiver reading; everywhere else it is the type name.
        let type_name_def = match &object.kind {
            HirExpressionKind::Path(def)
                if self
                    .resolver
                    .get_symbol(*def)
                    .is_some_and(|symbol| symbol.kind == crate::SymbolKind::Struct) =>
            {
                Some(*def)
            }
            _ => None,
        };
        if !write
            && let Some(type_def) = type_name_def
            && self.current_member_owner != Some(type_def)
        {
            return self.check_static_field_read(object, type_def, name);
        }
        self.place_write = write && matches!(object.kind, HirExpressionKind::Field(..));
        let obj_ty = self.check_expr(object);
        self.place_write = false;
        // In a program bound to a target by a target-native import, member
        // access on an `ignotum` value is admitted unchecked: the target's own
        // compiler or runtime judges it.
        if !self.trusted_imports.is_empty()
            && matches!(
                self.types.get(self.resolve_inferred_type(obj_ty)),
                Type::Primitive(Primitive::Ignotum)
            )
        {
            return self.types.primitive(Primitive::Ignotum);
        }
        if self
            .tensor_edge_policies
            .shifted_expressions
            .contains(&object.id)
        {
            self.invalid_operand_types(
                super::intrinsics::ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION,
                object.span,
            );
            return self.error_type;
        }
        if !write
            && type_name_def.is_none()
            && self.declared_field_modifier(obj_ty, name)
                == Some(radix_syntax::FieldModifier::Const)
        {
            self.undefined_member_with_args(
                "static_field_read_through_instance",
                object.span,
                &[("field", self.member_display_name(name))],
            );
            return self.error_type;
        }
        let access = if write {
            MemberAccess::Write
        } else {
            MemberAccess::Read
        };
        // A field-path null fact proven for this whole access node (need
        // c58fff9c case 2) refines the receiver inside the branch body that
        // proved it: the fact is keyed by the root binding plus the
        // root-to-leaf name path (`base.a.b`). Everything above still runs, so
        // namespace/static/generis and tensor-edge handling keep their say.
        if !write && let Some((base, path)) = super::control::field_path_key(object) {
            let mut key_path = path;
            key_path.push(name);
            if !key_path.is_empty()
                && let Some(&ty) = self.field_facts.get(&(base, key_path))
            {
                return ty;
            }
        }
        self.check_field_access_from_type(obj_ty, name, object.span, access)
    }

    /// Type a read through a genus name (`Tab.Z`): only a `const` member is
    /// readable there, and its type is the declared field type.
    fn check_static_field_read(
        &mut self,
        object: &mut HirExpression,
        type_def: DefId,
        name: Symbol,
    ) -> TypeId {
        let object_ty = self.check_expr(object);
        let span = object.span;
        let Some(field) = self
            .structs
            .get(&type_def)
            .and_then(|info| info.fields.get(&name))
            .copied()
        else {
            self.undefined_member("unknown_struct_field", span);
            return self.error_type;
        };
        if field.modifier != Some(radix_syntax::FieldModifier::Const) {
            self.undefined_member_with_args(
                "instance_field_read_through_type",
                span,
                &[("field", self.member_display_name(name))],
            );
            return self.error_type;
        }
        self.check_field_visibility(object_ty, name, MemberAccess::Read, span);
        field.ty
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

    /// The type of a size-name read (size-type Z1 force). A size name reads as
    /// a read-only `int`; inside a kernel it reads in the portable signed
    /// carrier the lane coordinates use, because the launch requirements prove
    /// each extent fits that carrier.
    fn size_value_type(&mut self) -> TypeId {
        if self.current_nucleum_entry {
            self.width_type(radix_types::NumericWidth::I32)
        } else {
            self.types.primitive(Primitive::Numerus)
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
        // Any use of an `@ nondum` function — call target or value — is refused.
        if self.report_unavailable_path(def_id, span) {
            return self.error_type;
        }

        if let Some(binding) = self.lookup_binding(def_id) {
            #[cfg(test)]
            super::flow_range_probe::observe(self, def_id);
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
                // Size-type Z1 force: a size name reads as a read-only int
                // (safe widening at use); assignment is rejected in check_lvalue.
                crate::SymbolKind::IndexParam => return self.size_value_type(),
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

        // Size-type Z1 force: a lexical `magnitudo` param of the active
        // function has a HirTypeParam def and no resolver symbol; it reads
        // as a read-only int (assignment is rejected in check_lvalue).
        if self.current_size_param_defs.contains(&def_id) {
            return self.size_value_type();
        }

        // A selective import that binds a type (D22.5) names no value.
        let issue = if self.resolver.selective_import_type(def_id).is_some() {
            "name_not_value"
        } else {
            "unknown_identifier"
        };
        self.error_issue(SemanticErrorKind::VariableUndefined, issue, span);
        self.error_type
    }
}

/// Bind an applied nominal's arguments to its declared generic parameters
/// (S0-G8): `Type` witnesses fill type slots, `Size` witnesses fill magnitudo
/// slots. The lists are zipped, so a length mismatch leaves the tail unbound.
fn applied_argument_bindings(
    params: &[Symbol],
    args: &[radix_types::AppliedArg],
) -> (
    FxHashMap<Symbol, TypeId>,
    FxHashMap<Symbol, crate::index::IndexId>,
) {
    let mut vars = FxHashMap::default();
    let mut shape_vars = FxHashMap::default();
    for (param, arg) in params.iter().zip(args.iter()) {
        match arg {
            radix_types::AppliedArg::Type(witness) => {
                vars.insert(*param, *witness);
            }
            radix_types::AppliedArg::Size(witness) => {
                shape_vars.insert(*param, *witness);
            }
        }
    }
    (vars, shape_vars)
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

#[cfg(test)]
#[path = "static_field_read_test.rs"]
mod static_field_read_test;
