//! Aggregate literals, closures, tuples, and struct construction typing.
//!
//! This file covers expression shapes whose type often depends on surrounding
//! context: object/struct fields, closures, tuples, arrays, and spread forms.
//! The checker uses expected types when they are available, but still records
//! a synthesized result so later HIR consumers see one canonical type per
//! expression node.
//!
//! INFERENCE CONTRACT
//! ==================
//! Empty arrays use contextual element information when available and otherwise
//! keep a fresh inference variable; non-empty arrays derive a common element
//! type from their items. Spread elements are checked as collections first, then
//! contribute their element type to the aggregate. Struct literals are closed
//! over the collected struct definition: unknown fields, missing required
//! fields, computed keys, and spreads are diagnostics, not codegen-time policy
//! choices.

use super::store::StoreSite;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::HirStoreKind;

/// The shape an expected list slot hands an array literal: the element type,
/// the bounded-list capacity, and the length-range type when the slot is one.
type ExpectedListShape = (TypeId, Option<radix_types::IndexId>, Option<TypeId>);

/// A statically known list length outside its `list<T, size lo‥hi>` range.
pub(super) const ISSUE_RANGED_LIST_LENGTH_OUT_OF_RANGE: &str = "ranged_list_length_out_of_range";

const ISSUE_ARRAY_SPREAD_REQUIRES_LISTA: &str = "array_spread_requires_lista";
pub(super) const ISSUE_SHAPE_HOLE_REQUIRES_SHAPE_ARGUMENT: &str =
    "tensor_shape_hole_requires_shape_argument";
const ISSUE_CLOSURE_PARAMETER_TYPE_MISMATCH: &str = "closure_parameter_type_mismatch";
const ISSUE_CLOSURE_RETURN_TYPE_MISMATCH: &str = "closure_return_type_mismatch";
const ISSUE_CONSTRUCTION_SOURCE_TYPE_MISMATCH: &str = "construction_source_type_mismatch";
const ISSUE_FIELD_INITIALIZER_TYPE_MISMATCH: &str = "field_initializer_type_mismatch";
const ISSUE_STRUCT_COMPUTED_KEY_FORBIDDEN: &str = "struct_computed_key_forbidden";
const ISSUE_UNKNOWN_STRUCT: &str = "unknown_struct";

impl TypeChecker<'_> {
    /// Validates object-field syntax against a known struct definition.
    ///
    /// This path is used by conversion/`verte` forms where fields are still in
    /// object-field representation. Each supplied field is checked so bad field
    /// names do not hide type errors in neighboring initializers. Generic class
    /// construction passes the applied construction arguments so field types
    /// have their parameters substituted before initializer checking (U-D1):
    /// `AppliedArg::Type` binds typus slots, `AppliedArg::Size` binds
    /// magnitudo slots (S0-G1 — size witnesses must reach vector/matrix widths).
    ///
    /// A copy-with-changes source (`Genus { … } ex source`, D15.1) arrives as
    /// the `Spread` entry. It must be the same genus as `target` (D15.2) and it
    /// supplies every field the braces omit, so required-field accounting is
    /// skipped when it is present.
    pub fn check_struct_fields(
        &mut self,
        def_id: DefId,
        fields: &mut [HirObjectField],
        args: &[radix_types::AppliedArg],
        target: TypeId,
        span: radix_lexer::Span,
    ) {
        let Some(info) = self.structs.get(&def_id) else {
            self.error_issue(
                SemanticErrorKind::TypeUndefined,
                ISSUE_UNKNOWN_STRUCT,
                fields
                    .first()
                    .and_then(|field| field.value.as_ref().map(|expr| expr.span))
                    .unwrap_or(span),
            );
            return;
        };
        let field_types = info.fields.clone();
        let field_types: FxHashMap<Symbol, StructFieldInfo> = {
            let mut vars = FxHashMap::default();
            let mut shape_vars = FxHashMap::default();
            if let Some(params) = self.type_params_by_def.get(&def_id) {
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
            }
            field_types
                .into_iter()
                .map(|(name, mut info)| {
                    info.ty =
                        self.substitute_type_params(info.ty, &mut vars, Some(&mut shape_vars));
                    (name, info)
                })
                .collect()
        };
        let mut supplied = FxHashSet::default();
        let mut has_source = false;

        for field in fields.iter_mut() {
            match &mut field.key {
                HirObjectKey::Ident(name) | HirObjectKey::String(name) => {
                    let Some(field_info) = field_types.get(name).copied() else {
                        let error_span = field.value.as_ref().map_or(span, |expr| expr.span);
                        self.undefined_member("unknown_field", error_span);
                        continue;
                    };
                    supplied.insert(*name);
                    let Some(value) = &mut field.value else {
                        self.undefined_member("struct_field_requires_value", span);
                        continue;
                    };
                    let site = StoreSite::new(HirStoreKind::Field)
                        .named(*name)
                        .optional_slot(field_info.optional);
                    let value_ty = self.check_store_expr(value, field_info.ty, site);
                    self.unify_carrier_assign(
                        value_ty,
                        field_info.ty,
                        value.span,
                        ISSUE_FIELD_INITIALIZER_TYPE_MISMATCH,
                    );
                }
                HirObjectKey::Computed(expr) => {
                    self.check_expr(expr);
                    self.invalid_operand_types(ISSUE_STRUCT_COMPUTED_KEY_FORBIDDEN, expr.span);
                }
                // Only the `ex source` suffix produces this key; `sparge` left
                // construction literals (D15.4).
                HirObjectKey::Spread(expr) => {
                    has_source = true;
                    self.check_construction_source(def_id, expr, target);
                }
            }
        }

        if has_source {
            return;
        }
        for (name, info) in field_types {
            if info.required && !supplied.contains(&name) {
                self.undefined_member("missing_required_struct_field", info.span);
            }
        }
    }
    /// Check a copy-with-changes source against the constructed genus.
    ///
    /// The source is typed with the target as context, then must name the
    /// same genus (`Genus` or an application of it); any other type — another
    /// genus, a scalar, a nullable carrier — is
    /// `construction_source_type_mismatch`. For the same genus the ordinary
    /// assignment check then binds or compares the type arguments.
    fn check_construction_source(
        &mut self,
        def_id: DefId,
        source: &mut HirExpression,
        target: TypeId,
    ) {
        let source_ty = self.check_expr_with_expected(source, Some(target));
        let resolved = self.resolve_inferred_type(source_ty);
        let resolved = match self.types.get(resolved) {
            Type::Ref(_, inner) => self.resolve_inferred_type(*inner),
            _ => resolved,
        };
        let source_def = match self.types.get(resolved) {
            Type::Struct(source_def) => Some(*source_def),
            Type::Applied(base, _) => match self.types.get(self.resolve_inferred_type(*base)) {
                Type::Struct(source_def) => Some(*source_def),
                _ => None,
            },
            _ => None,
        };
        if source_def == Some(def_id) {
            self.unify_assign_issue(
                resolved,
                target,
                source.span,
                ISSUE_CONSTRUCTION_SOURCE_TYPE_MISMATCH,
            );
        } else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_CONSTRUCTION_SOURCE_TYPE_MISMATCH,
                source.span,
            );
        }
    }

    /// Checks a closure expression and returns its function type.
    ///
    /// Closures are both synthesized and expected-context driven. If an
    /// expected function type exists, parameter and expression-body return
    /// positions use it as context; otherwise the closure body and explicit
    /// annotations determine the signature. Statement-bodied closures only
    /// enable `redde` when the closure spells its own `→` return channel, and
    /// only enable escaping `iace` when the closure spells its own `⇥` channel.
    /// Contextual expected types do not create implicit statement exit channels.
    #[allow(clippy::ptr_arg)]
    pub fn check_closure(
        &mut self,
        params: &mut Vec<HirParam>,
        ret: Option<&mut TypeId>,
        err: Option<&mut TypeId>,
        body: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        let expected_sig = expected.and_then(|ty| self.signature_from_type(ty));

        // The body runs when the closure is called, not where it is written:
        // every fact about a mutable source may be stale by then, so the body
        // starts without them. Facts on `let` bindings, loop binders and tensor
        // extents hold whenever it runs. The outer flow resumes untouched.
        let ranges_outside = self.range_flow.snapshot();
        self.kill_mutable_source_ranges();

        self.push_scope();
        for (idx, param) in params.iter().enumerate() {
            if let Some(sig) = &expected_sig
                && let Some(expected_param) = sig.params.get(idx)
            {
                self.unify_assign_issue(
                    param.ty,
                    expected_param.ty,
                    param.span,
                    ISSUE_CLOSURE_PARAMETER_TYPE_MISMATCH,
                );
            }
            let scalar_text = self.mark_chorda_scalar_closure_param && idx == 0;
            let mutable =
                matches!(param.mode, HirParamMode::In) || self.types.is_exclusive_value(param.ty);
            if !mutable {
                self.read_only_param_roots.insert(param.def_id);
            }
            self.insert_binding_with_scalar(param.def_id, param.ty, scalar_text, mutable);
        }
        self.mark_chorda_scalar_closure_param = false;

        let explicit_ret = ret.as_ref().map(|ty| **ty);
        let explicit_err = err.as_ref().map(|ty| **ty);
        let expected_ret = explicit_ret.or_else(|| expected_sig.as_ref().map(|sig| sig.ret));

        let prev_return = self.current_return;
        let prev_error = self.current_error;
        let prev_main_frame = std::mem::take(&mut self.in_main_frame);
        let prev_await_context = self.current_await_context;
        let prev_await_return_context = self.current_await_return_context;
        let prev_inferred = self.inferred_return;
        let prev_allow_inferred = self.allow_inferred_return;
        let body_uses_statement_returns = matches!(
            body.kind,
            HirExpressionKind::Block(_)
                | HirExpressionKind::Handled { .. }
                | HirExpressionKind::Loop(_)
        );
        self.current_return = if body_uses_statement_returns {
            explicit_ret
        } else {
            expected_ret
        };
        self.current_error = explicit_err.map(ErrorSink::Function);
        self.current_await_context = false;
        self.current_await_return_context = false;
        self.inferred_return = None;
        self.allow_inferred_return = !body_uses_statement_returns;

        let body_ty = match (body_uses_statement_returns, expected_ret) {
            (true, _) => self.check_expr(body),
            (false, Some(ret_slot)) => {
                self.check_store_expr(body, ret_slot, StoreSite::new(HirStoreKind::Return))
            }
            (false, None) => self.check_expr_with_expected(body, None),
        };
        let inferred_return = self.inferred_return.take();

        self.current_return = prev_return;
        self.current_error = prev_error;
        self.in_main_frame = prev_main_frame;
        self.current_await_context = prev_await_context;
        self.current_await_return_context = prev_await_return_context;
        self.inferred_return = prev_inferred;
        self.allow_inferred_return = prev_allow_inferred;

        let ret_ty = match ret {
            Some(ty) => {
                if !body_uses_statement_returns {
                    self.unify_assign_issue(
                        body_ty,
                        *ty,
                        body.span,
                        ISSUE_CLOSURE_RETURN_TYPE_MISMATCH,
                    );
                }
                *ty
            }
            None if body_uses_statement_returns => {
                if let Some(expected) = expected_ret {
                    self.unify_assign_issue(
                        body_ty,
                        expected,
                        body.span,
                        ISSUE_CLOSURE_RETURN_TYPE_MISMATCH,
                    );
                }
                body_ty
            }
            None => expected_ret.or(inferred_return).unwrap_or(body_ty),
        };

        self.pop_scope();
        self.range_flow.restore(&ranges_outside);

        let sig = FuncSig {
            type_params: Vec::new(),
            params: params
                .iter()
                .map(|param| ParamType {
                    ty: param.ty,
                    mode: param_mode_from_hir(param.mode),
                    optional: param.optional,
                    sponte: param.sponte,
                })
                .collect(),
            ret: ret_ty,
            err: explicit_err,
            is_async: false,
            is_generator: false,
        };
        self.types.function(sig)
    }

    /// Builds a typed `Type::Tuple` from a positional aggregate expression.
    ///
    /// When an expected `Type::Tuple` is available the element types are used as
    /// per-element expected types so `iuncta<A, B> [a, b]` construction can
    /// validate each element against the declared type argument.
    ///
    /// When no contextual expected type exists but the source carried explicit
    /// type arguments (`iuncta<A, B>`), those are used instead so the
    /// construction site is self-contained.  Synthesis (neither) derives
    /// element types from the expressions themselves.
    pub fn check_tuple(
        &mut self,
        items: &mut [HirExpression],
        explicit_type_args: Option<&Vec<TypeId>>,
        expected: Option<TypeId>,
    ) -> TypeId {
        let expected_elements: Option<Vec<TypeId>> = expected.and_then(|ty| {
            let resolved = self.resolve_inferred_type(ty);
            match self.types.get(resolved) {
                Type::Tuple(elems) => Some(elems.clone()),
                _ => None,
            }
        });

        // Use explicit type args when no contextual expected tuple is available.
        let element_types: Option<Vec<TypeId>> =
            expected_elements.or_else(|| explicit_type_args.cloned());

        if let Some(elems) = &element_types
            && elems.len() != items.len()
        {
            self.wrong_arity(
                "iuncta_construction_arity_mismatch",
                items
                    .first()
                    .map_or(radix_lexer::Span::default(), |e| e.span),
            );
        }

        let mut types = Vec::new();
        for (i, item) in items.iter_mut().enumerate() {
            let expected_elem = element_types.as_ref().and_then(|e| e.get(i).copied());
            types.push(if let Some(elem) = expected_elem {
                self.check_store_expr(item, elem, StoreSite::new(HirStoreKind::TuplePosition))
            } else {
                self.check_expr(item)
            });
        }

        self.types.intern(Type::Tuple(types))
    }
    /// Checks a lowered struct literal against its collected field table.
    ///
    /// Unknown fields and missing required fields are reported here. The result
    /// type remains the declared struct when the definition exists so codegen
    /// and later diagnostics can still operate on the intended shape. Generic
    /// class construction substitutes the type params with fresh inference
    /// witnesses (no contextual expected type reaches this legacy HIR shape)
    /// and returns the applied struct type (U-D1).
    #[allow(clippy::ptr_arg)]
    pub fn check_struct_literal(
        &mut self,
        def_id: DefId,
        fields: &mut Vec<(Symbol, HirExpression)>,
    ) -> TypeId {
        let Some(info) = self.structs.get(&def_id) else {
            self.error_issue(
                SemanticErrorKind::TypeUndefined,
                ISSUE_UNKNOWN_STRUCT,
                fields
                    .first()
                    .map(|(_, expr)| expr.span)
                    .unwrap_or_default(),
            );
            return self.error_type;
        };
        let field_types = info.fields.clone();
        let witnesses = self.constructor_witnesses(def_id, &[], None, radix_lexer::Span::default());
        let applied_args: Vec<radix_types::AppliedArg> = witnesses
            .iter()
            .copied()
            .map(radix_types::AppliedArg::Type)
            .collect();
        let field_types: FxHashMap<Symbol, StructFieldInfo> = {
            let mut vars = FxHashMap::default();
            if let Some(params) = self.type_params_by_def.get(&def_id) {
                for (param, arg) in params.iter().zip(applied_args.iter()) {
                    if let radix_types::AppliedArg::Type(witness) = arg {
                        vars.insert(*param, *witness);
                    }
                }
            }
            field_types
                .into_iter()
                .map(|(name, mut info)| {
                    info.ty = self.substitute_type_params(info.ty, &mut vars, None);
                    (name, info)
                })
                .collect()
        };

        let mut supplied = FxHashSet::default();
        for (name, value) in fields.iter_mut() {
            let Some(field_info) = field_types.get(name).copied() else {
                self.undefined_member("unknown_field", value.span);
                continue;
            };
            supplied.insert(*name);
            let site = StoreSite::new(HirStoreKind::Field)
                .named(*name)
                .optional_slot(field_info.optional);
            let value_ty = self.check_store_expr(value, field_info.ty, site);
            self.unify_assign_issue(
                value_ty,
                field_info.ty,
                value.span,
                ISSUE_FIELD_INITIALIZER_TYPE_MISMATCH,
            );
        }

        for (name, info) in field_types {
            if info.required && !supplied.contains(&name) {
                self.undefined_member("missing_required_struct_field", info.span);
            }
        }

        if self.nominal_is_generic(def_id) {
            let base = self.types.intern(Type::Struct(def_id));
            let witnesses = witnesses
                .into_iter()
                .map(radix_types::AppliedArg::Type)
                .collect();
            return self.types.intern(Type::Applied(base, witnesses));
        }
        self.types.intern(Type::Struct(def_id))
    }
    /// Chooses a common array element type, flattening one side when spread
    /// analysis has already proven it is an array.
    ///
    /// This keeps `[x, ...ys]` joins about element compatibility rather than
    /// accidentally forming a union between an element and a whole list.
    pub fn array_common_type(&mut self, a: TypeId, b: TypeId, span: radix_lexer::Span) -> TypeId {
        let a_resolved = self.resolve_inferred_type(a);
        let b_resolved = self.resolve_inferred_type(b);

        // U-04: union-hole element joins are accumulations. Under a `∪`-hole
        // expected element type (`lista<∪>`), every element already flowed
        // into the hole var through `check_expr_with_expected`, so joining two
        // such elements is the open hole itself — never the spread-flatten or
        // `common_type` paths, which would reject heterogeneous members.
        if self.infer_union_var_of(a_resolved).is_some()
            || self.infer_union_var_of(b_resolved).is_some()
        {
            return self.unify_join(a, b, span, "common_type_mismatch");
        }

        match (
            self.types.get(a_resolved).clone(),
            self.types.get(b_resolved).clone(),
        ) {
            (
                Type::Array(a_inner),
                Type::Array(b_inner)
                | Type::BoundedArray {
                    element: b_inner, ..
                }
                | Type::RangedArray {
                    element: b_inner, ..
                },
            )
            | (
                Type::BoundedArray {
                    element: a_inner, ..
                }
                | Type::RangedArray {
                    element: a_inner, ..
                },
                Type::Array(b_inner),
            ) => {
                let element = self.common_type(a_inner, b_inner, span);
                return self.types.array(element);
            }
            (
                Type::BoundedArray {
                    element: a_inner,
                    capacity: a_capacity,
                },
                Type::BoundedArray {
                    element: b_inner,
                    capacity: b_capacity,
                },
            ) => {
                let element = self.common_type(a_inner, b_inner, span);
                if self.types.index_equals(a_capacity, b_capacity) {
                    return self.types.bounded_array(element, a_capacity);
                }
                return self.types.array(element);
            }
            _ => {}
        }

        if let Type::Array(a_inner) = self.types.get(a_resolved).clone()
            && !matches!(self.types.get(b_resolved), Type::Array(_))
        {
            return self.common_type(a_inner, b, span);
        }

        if let Type::Array(b_inner) = self.types.get(b_resolved).clone()
            && !matches!(self.types.get(a_resolved), Type::Array(_))
        {
            return self.common_type(a, b_inner, span);
        }

        self.common_type(a, b, span)
    }
    /// The list shape an expected slot type supplies to an array literal:
    /// the slot itself, or the single list member of an optional / union slot
    /// (`list<T> ∪ none` hands the literal `list<T>`; several list members
    /// supply none, like the single-numeric-member rule of a store).
    fn expected_list_shape(&self, expected: TypeId) -> Option<ExpectedListShape> {
        let payload = self
            .types
            .peel_access_marker(self.resolve_inferred_type(expected))
            .1;
        match self.types.get(payload) {
            Type::Array(inner) => Some((*inner, None, None)),
            Type::BoundedArray { element, capacity } => Some((*element, Some(*capacity), None)),
            // A length range: the literal is checked against
            // the range where the list is built.
            Type::RangedArray { element, .. } => Some((*element, None, Some(payload))),
            Type::Option(inner) => self.expected_list_shape(*inner),
            Type::Union(members) => {
                let mut shapes = members.iter().filter_map(|m| self.expected_list_shape(*m));
                let only = shapes.next()?;
                shapes.next().is_none().then_some(only)
            }
            _ => None,
        }
    }

    /// A literal of `length` elements against the length range of `ranged`: the
    /// range type itself when the length fits, else the named refusal. A
    /// statically known length outside the range is a compile-time diagnostic
    /// (the store ruling: a known value has its check at compile time).
    pub(super) fn check_ranged_literal_length(
        &mut self,
        ranged: TypeId,
        length: usize,
        span: radix_lexer::Span,
    ) -> TypeId {
        let Type::RangedArray { min, max, .. } = *self.types.get(ranged) else {
            return ranged;
        };
        let length = length as u64;
        if length >= min && max.is_none_or(|max| length <= max) {
            return ranged;
        }
        self.invalid_operand_types(ISSUE_RANGED_LIST_LENGTH_OUT_OF_RANGE, span);
        self.error_type
    }

    /// Checks an array literal and returns its list type.
    ///
    /// Expected list context supplies the element type for each non-spread item.
    /// Dynamic `lista<T>` context preserves a dynamic carrier; bounded context
    /// checks a literal's exact length against its capacity. Without a concrete
    /// context, an array literal carries its exact bound and infers a common
    /// element type from its checked items.
    pub fn check_array(
        &mut self,
        elements: &mut [HirArrayElement],
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        if let Some(span) = elements.iter().find_map(|element| match element {
            HirArrayElement::Infer(span) => Some(*span),
            _ => None,
        }) {
            self.invalid_operand_types(ISSUE_SHAPE_HOLE_REQUIRES_SHAPE_ARGUMENT, span);
            return self.error_type;
        }
        let expected_is_open =
            expected.is_some_and(|ty| self.is_infer(self.resolve_inferred_type(ty)));
        let expected_shape = expected.and_then(|ty| self.expected_list_shape(ty));
        let expected_elem = expected_shape.map_or_else(|| self.fresh_infer(), |(elem, _, _)| elem);
        let expected_capacity = expected_shape.and_then(|(_, capacity, _)| capacity);
        let expected_ranged = expected_shape.and_then(|(_, _, ranged)| ranged);
        // A literal's length is static only when nothing is spread into it.
        let literal_length = elements
            .iter()
            .all(|element| matches!(element, HirArrayElement::Expr(_)))
            .then_some(elements.len());
        let has_expected_element = expected_shape.is_some();
        let literal_capacity =
            (expected.is_none() || expected_is_open || expected_capacity.is_some())
                && elements
                    .iter()
                    .all(|element| matches!(element, HirArrayElement::Expr(_)));
        let literal_capacity = literal_capacity.then(|| {
            self.types
                .intern_index(IndexExpr::Literal(elements.len() as u64))
        });
        let result_capacity = match expected_capacity {
            Some(capacity) if self.is_unresolved_index_hole(capacity) => Some(capacity),
            // WHY: an untyped `[]` takes the cell's type (ruling 2026-10-10:
            // the double hole), exactly as `empty` does: an empty list in a
            // bounded cell keeps that cell's capacity instead of becoming an
            // exact zero-length witness that no capacity-N cell accepts.
            Some(capacity) if elements.is_empty() => Some(capacity),
            Some(capacity) => {
                // A bounded list's number is its maximum length, not a
                // required literal length. Preserve the declared capacity for
                // a statically known literal that fits; an oversized literal
                // keeps its exact witness so the ordinary assignment check
                // refuses it by name.
                let literal_fits = literal_length.is_some_and(|length| {
                    let capacity = self.resolve_index(capacity);
                    matches!(
                        self.types.get_index(capacity),
                        IndexExpr::Literal(maximum) if length as u64 <= *maximum
                    )
                });
                if literal_fits {
                    Some(capacity)
                } else {
                    literal_capacity.or(Some(capacity))
                }
            }
            None => literal_capacity,
        };

        if elements.is_empty() && !has_expected_element {
            // An unannotated `[]` is still an exact zero-length witness, while
            // `lista<T>` keeps its dynamic carrier when context supplies T.
            if let Some(capacity) = result_capacity {
                return self.types.bounded_array(expected_elem, capacity);
            }
            return self.types.array(expected_elem);
        }

        let mut element_ty = None;
        let mut elements_const = None;
        for element in elements {
            let (expr, spread) = match element {
                HirArrayElement::Expr(expr) => (expr, false),
                HirArrayElement::Spread(expr) => (expr, true),
                HirArrayElement::Infer(span) => {
                    // Rejected before the loop; keep the same diagnostic if reached.
                    self.invalid_operand_types(ISSUE_SHAPE_HOLE_REQUIRES_SHAPE_ARGUMENT, *span);
                    return self.error_type;
                }
            };
            let ty = if spread {
                // Only a literal operand is built for the slot. Any other
                // operand is an existing list: its elements are copied into
                // this literal as stores, so the list itself is not assigned
                // to `lista<expected>` (type arguments are invariant).
                if has_expected_element && matches!(expr.kind, HirExpressionKind::Array(_)) {
                    let expected_array = self.types.array(expected_elem);
                    self.check_expr_with_expected(expr, Some(expected_array))
                } else {
                    self.check_expr(expr)
                }
            } else if has_expected_element {
                // A declared element type makes each element a store; an open
                // element type is bound by the elements (range rule) instead.
                self.check_store_expr(expr, expected_elem, StoreSite::new(HirStoreKind::Element))
            } else {
                self.check_expr(expr)
            };
            let ty = if spread {
                // Exact-bound literals intern as `BoundedArray`; spreading
                // one extracts its element just like an unbounded lista
                // (mirrors `check_verte_array_target`).
                let spread_element =
                    if let Type::Array(inner) = self.types.get(self.resolve_inferred_type(ty)) {
                        *inner
                    } else if let Type::BoundedArray { element, .. }
                    | Type::RangedArray { element, .. } =
                        self.types.get(self.resolve_inferred_type(ty))
                    {
                        *element
                    } else {
                        self.invalid_operand_types(ISSUE_ARRAY_SPREAD_REQUIRES_LISTA, expr.span);
                        self.error_type
                    };
                // Each copied element is a store into the declared element type.
                if has_expected_element
                    && spread_element != self.error_type
                    && self.types.is_assignable(spread_element, expected_elem)
                {
                    expected_elem
                } else {
                    spread_element
                }
            } else {
                ty
            };
            let element_range = if spread {
                None
            } else {
                Self::const_join_range(expr)
            };
            element_ty = Some(match element_ty {
                None => {
                    elements_const = element_range;
                    ty
                }
                Some(existing) => {
                    let joined = match (elements_const, element_range) {
                        (Some(_), None) | (None, Some(_)) => self.common_type_with_consts(
                            (existing, elements_const),
                            (ty, element_range),
                            expr.span,
                        ),
                        _ => self.array_common_type(existing, ty, expr.span),
                    };
                    elements_const = match (elements_const, element_range) {
                        (Some(a), Some(b)) => Some(radix_types::numeric_range::hull(a, b)),
                        _ => None,
                    };
                    joined
                }
            });
        }

        if let (Some(ranged), Some(length)) = (expected_ranged, literal_length) {
            return self.check_ranged_literal_length(ranged, length, span);
        }

        let elem_ty = element_ty.unwrap_or(expected_elem);
        if let Some(capacity) = result_capacity {
            self.types.bounded_array(elem_ty, capacity)
        } else {
            self.types.array(elem_ty)
        }
    }
}
