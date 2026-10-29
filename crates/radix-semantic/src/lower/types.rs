//! Type syntax lowering for the HIR boundary.
//!
//! Parser type expressions carry surface spelling, spans, modifiers, and generic
//! argument syntax. This module turns that syntax into interned semantic
//! [`TypeId`] values that the resolver, typechecker, and code generators can
//! compare cheaply and share consistently.
//!
//! INVARIANTS
//! ==========
//! - Primitive and collection names are recognized by spelling before ordinary
//!   symbol lookup so `textus`, `lista<T>`, and friends remain language types,
//!   not user definitions.
//! - Named user types must resolve to type-bearing symbols: structs, enums,
//!   interfaces, aliases, or type parameters.
//! - Nullable unions use the canonical `T ∪ nihil` boundary: `nihil` members
//!   are stripped, the remaining members are deduplicated, and presence of
//!   `nihil` wraps the result in `Option`.
//! - `ignotum` is treated as an ordinary primitive/top type spelling here, not
//!   as nullability sugar.
//! - Syntax errors in type positions lower to `Type::Error` after recording a
//!   diagnostic so later phases can continue without guessing.
//!
//! MODIFIER ORDER
//! ==============
//! References (`de`/`in`) are applied to the lowered base type before the
//! parser-level nullable flag is applied. That preserves the AST contract
//! exactly; newer source should prefer explicit union nullability where the
//! grammar allows it.

use super::Lowerer;
use crate::{
    CollectionKind, FuncSig, IndexExpr, IndexId, InferVar, ParamType, Primitive,
    SemanticGenericParamDefault, SemanticParamMode, Type, TypeId,
};
use handroll::FxHashMap;
use radix_diagnostics::DiagnosticArg;
use radix_syntax::{
    FiguraExpr, GenericArgExpr, GenericParamKind, Ident, RangeBoundExpr, RangeKind, TypeExpr,
    TypeExprKind,
};
use radix_types::{
    AppliedArg, BuiltinTensorEdgePolicy, compose_index_affine_constant, divide_index_exact,
    evaluate_index_dimension,
};

const FIGURA_LIFT_PRIMARY_MSG: &str =
    "size lift extent expects a natural literal, a name, or a parenthesized expression";

/// Size-type Z3: a bare value name in an annotation extent position is the
/// invisible int→size crossing; the spelled `size <name>` lift is the
/// did-you-mean. Locale rows live under SEM008 beside the slot refusal.
const ISSUE_ANNOTATION_EXTENT_BARE_VALUE_NAME: &str = "annotation_extent_bare_value_name";

fn generic_type_expr(arg: &GenericArgExpr) -> Option<TypeExpr> {
    match arg {
        GenericArgExpr::Type(ty) => Some(ty.clone()),
        GenericArgExpr::Name(name) => Some(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(name.clone(), Vec::new()),
            span: name.span,
            form: radix_syntax::TypeFormLevel::LONG,
            iuncta_label: None,
        }),
        GenericArgExpr::Size(_) | GenericArgExpr::Range(_) => None,
    }
}

fn generic_type_exprs(params: &[GenericArgExpr]) -> Option<Vec<TypeExpr>> {
    params.iter().map(generic_type_expr).collect()
}

impl Lowerer<'_> {
    /// Lower a parser type expression into the interned semantic type table.
    ///
    /// This is the only HIR-lowering entry point that may turn type syntax into
    /// reusable `TypeId`s. It preserves source span context for diagnostics,
    /// normalizes nullable unions, and applies AST modifiers in parser order
    /// without resolving type compatibility. Bind declarations and other
    /// ordinary type positions keep the scalar-marker rejection; parameter
    /// positions use [`Self::lower_parameter_type`] so scalar `ref`/`mut`
    /// markers can describe the parameter ABI.
    pub fn lower_type(&mut self, ty: &TypeExpr) -> TypeId {
        self.lower_type_with_context(ty, false, false)
    }

    /// Lower a type written in a parameter slot.
    ///
    /// `ref`/`mut` on scalar parameters are intentional ABI markers, while
    /// `own`/`copy` remain rejected on scalars and all markers remain rejected
    /// in ordinary binding/type positions.
    pub(super) fn lower_parameter_type(&mut self, ty: &TypeExpr) -> TypeId {
        self.lower_type_with_context(ty, true, false)
    }

    /// Lower an authored type occurrence and record its form tree.
    ///
    /// Performs exactly one recursive lowering with the same positional
    /// context as the presentation-free entries (parameter slots allow scalar
    /// `ref`/`mut`; signature slots reject holes), returns the same `TypeId`
    /// the caller stores today, and pushes the completed [`HirTypeForm`] onto
    /// the presentation sidecar keyed by `site`. Internal and synthesized
    /// types keep using the presentation-free lowering entries.
    pub fn lower_authored_type(
        &mut self,
        site: radix_hir::HirTypeSite,
        ty: &TypeExpr,
        allow_scalar_ref_mut: bool,
        signature_position: bool,
    ) -> TypeId {
        debug_assert!(
            self.form_frames.is_empty(),
            "nested authored type capture is not supported"
        );
        // Marker frame: the root node's own completion (below) lands its form
        // here as the sole argument.
        self.form_frames.push(super::TypeFormFrame::default());
        let ty_id = self.lower_type_with_context(ty, allow_scalar_ref_mut, signature_position);
        let marker = self.form_frames.pop();
        let form = match marker.as_ref().and_then(|frame| frame.args.first()) {
            Some(radix_hir::HirTypeFormArg::Type(form)) => (**form).clone(),
            // Lowering bailed out before the root node completed (e.g. a
            // rejected hole in signature position): fail closed with an
            // Error-typed witness over the authored level.
            _ => radix_hir::HirTypeForm {
                ty: ty_id,
                head: radix_hir::HirTypeFormHead::Semantic(ty.form),
                args: Vec::new(),
                label: None,
            },
        };
        self.presentation
            .authored_types
            .push(radix_hir::HirAuthoredTypeUse { site, form });
        ty_id
    }

    /// Lower a type written in a signature clause (return or error channel).
    ///
    /// Ruling (g): signature slots stay explicit — `_` and a standalone `∪`
    /// are inference holes and are rejected here. Holes stay legal in local
    /// bindings (`const _ v`, `const ∪ v`).
    pub(super) fn lower_signature_type(&mut self, ty: &TypeExpr) -> TypeId {
        self.signature_type_nesting += 1;
        let lowered = self.lower_type_with_context(ty, false, true);
        self.signature_type_nesting -= 1;
        lowered
    }

    /// Lower a type nested inside another type expression.
    ///
    /// Access markers describe acquisition sites, not stored element types.
    /// Reject them before lowering the nested carrier so `lista<copy T>` and
    /// its ref/mut/own counterparts cannot become type-table facts.
    fn lower_nested_type(&mut self, ty: &TypeExpr) -> TypeId {
        if ty.mode.is_some() {
            self.error("nested_access_marker");
            return self.types.intern(Type::Error);
        }
        self.lower_type(ty)
    }

    fn lower_type_with_context(
        &mut self,
        ty: &TypeExpr,
        allow_scalar_ref_mut: bool,
        signature_position: bool,
    ) -> TypeId {
        self.current_span = ty.span;

        if signature_position
            && matches!(
                ty.kind,
                TypeExprKind::Infer | TypeExprKind::UnionHole | TypeExprKind::FieldListHole
            )
        {
            self.error("signature_position_hole");
            return self.types.intern(Type::Error);
        }

        // Authored-form capture: every node inside a recording session owns a
        // frame; child forms are pushed into it by the children's own
        // completion below.
        let capturing_form = !self.form_frames.is_empty();
        if capturing_form {
            self.form_frames.push(super::TypeFormFrame::default());
        }

        let mut hoisted_union_mode = None;
        let mut ty_id = match &ty.kind {
            TypeExprKind::Infer | TypeExprKind::FieldListHole => {
                let infer_id = self.next_def_id().0;
                self.types.intern(Type::Infer(InferVar(infer_id)))
            }
            TypeExprKind::UnionHole => {
                let infer_id = self.next_def_id().0;
                self.types.intern(Type::InferUnion(InferVar(infer_id)))
            }
            TypeExprKind::Tensor(elem, figura) => self.lower_tensor_type(elem, figura),
            TypeExprKind::Vector(elem, figura) => self.lower_vector_type(elem, figura),
            TypeExprKind::Matrix(elem, figura) => self.lower_matrix_type(elem, figura),
            TypeExprKind::Sparsa(elem, figura) => self.lower_sparsa_type(elem, figura),
            TypeExprKind::Atomic(elem) => self.lower_atomic_type(elem),
            TypeExprKind::PromissumFailable(success, alternate) => {
                let success = self.lower_nested_type(success);
                let alternate = self.lower_nested_type(alternate);
                self.types.promissum_failable(success, alternate)
            }
            TypeExprKind::Named(name, params) => self.lower_named_type(name, params),
            TypeExprKind::Qualified(path, params) => self.lower_qualified_type(path, params),
            TypeExprKind::Func(func) => self.lower_function_type(func),
            TypeExprKind::Union(members) => {
                let (union_id, hoisted) = self.lower_union_type(members);
                hoisted_union_mode = hoisted;
                union_id
            }
            TypeExprKind::Intersection(members) => self.lower_intersection_type(members),
        };

        let base_ty = ty_id;
        if let Some(mode) = ty.mode.or(hoisted_union_mode) {
            ty_id = self.apply_access_marker(ty_id, mode, allow_scalar_ref_mut);
        }

        let marked_ty = ty_id;
        if ty.nullable {
            ty_id = self.types.option(ty_id);
        }

        if capturing_form {
            ty_id = self.capture_authored_form(ty, base_ty, marked_ty, ty_id);
        }

        ty_id
    }

    /// Lower a `functio(T, …) → R` type expression into the interned function
    /// shape. Parameters keep the parameter-slot marker rules; the return and
    /// error channels keep the signature-slot hole refusal.
    fn lower_function_type(&mut self, func: &radix_syntax::FuncTypeExpr) -> TypeId {
        let params = func
            .params
            .iter()
            .map(|param| {
                let ty = self.lower_parameter_type(param);
                ParamType {
                    ty,
                    mode: match param.mode {
                        Some(type_mode) => super::type_mode_semantic_param_mode(type_mode),
                        None => SemanticParamMode::Owned,
                    },
                    optional: false,
                    sponte: false,
                }
            })
            .collect();
        let ret = self.lower_signature_type(&func.ret);
        let err = func.err.as_ref().map(|err| self.lower_signature_type(err));
        self.types.function(FuncSig {
            type_params: Vec::new(),
            params,
            ret,
            err,
            is_async: false,
            is_generator: false,
        })
    }

    /// Apply an access marker, rejecting one that cannot modify a scalar.
    fn apply_access_marker(
        &mut self,
        ty_id: TypeId,
        mode: radix_syntax::TypeMode,
        allow_scalar_ref_mut: bool,
    ) -> TypeId {
        let marker = crate::access_marker_from_type_mode(mode);
        if self.types.is_scalar_for_access_marker(ty_id)
            && !(allow_scalar_ref_mut
                && matches!(
                    mode,
                    radix_syntax::TypeMode::De | radix_syntax::TypeMode::In
                ))
        {
            self.error(crate::ISSUE_SCALAR_ACCESS_MARKER);
            self.types.intern(Type::Error)
        } else {
            self.types.access_marker(marker, ty_id)
        }
    }

    /// Attach the authored form captured for one type node to its parent frame.
    ///
    /// Returns the type unchanged when lowering already bailed out and left no
    /// frame on the stack.
    fn capture_authored_form(
        &mut self,
        ty: &TypeExpr,
        base_ty: TypeId,
        marked_ty: TypeId,
        ty_id: TypeId,
    ) -> TypeId {
        // The frame was pushed by the caller; an empty stack means lowering
        // already bailed out, so leave the type without a form.
        let Some(frame) = self.form_frames.pop() else {
            return ty_id;
        };
        let union_form_children = if matches!(ty.kind, TypeExprKind::Union(_))
            && matches!(self.types.get(base_ty), Type::Option(_))
        {
            let children = frame
                .args
                .iter()
                .filter_map(|arg| match arg {
                    radix_hir::HirTypeFormArg::Type(child)
                        if !matches!(
                            self.types.get(child.ty),
                            Type::Primitive(Primitive::Nihil)
                        ) =>
                    {
                        Some(arg.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            (children.len() == 1).then_some(children)
        } else {
            None
        };
        let mut form = Self::complete_type_form(ty, base_ty, frame, union_form_children);
        // Access and nullable modifiers wrap the authored carrier. Keep
        // its name and generic children beneath each semantic wrapper.
        for wrapper_ty in [marked_ty, ty_id] {
            if wrapper_ty != form.ty {
                form = radix_hir::HirTypeForm {
                    ty: wrapper_ty,
                    head: radix_hir::HirTypeFormHead::Semantic(radix_syntax::TypeFormLevel::LONG),
                    args: vec![radix_hir::HirTypeFormArg::Type(Box::new(form))],
                    label: None,
                };
            }
        }
        // The label belongs to this node's position in its parent (the
        // `gx` of `iuncta<gx: int, …>`), so it rides the outermost form.
        form.label = ty.iuncta_label.as_ref().map(|ident| ident.name);
        // A parent frame always exists during capture: the marker frame
        // pushed by `lower_authored_type` sits at the bottom of the stack.
        if let Some(parent) = self.form_frames.last_mut() {
            parent
                .args
                .push(radix_hir::HirTypeFormArg::Type(Box::new(form)));
        }
        ty_id
    }

    /// Complete one authored form node from its frame.
    ///
    /// The witness is the base carrier before access and nullable wrappers;
    /// the head records how the author spelled that fact. Function types,
    /// unions, and intersections are
    /// witness-only: their canonical shapes have no 1:1 authored-child
    /// correspondence (param modes, `∪ nihil` stripping, sort/dedup), so they
    /// keep no child forms and re-emit canonically.
    fn complete_type_form(
        expr: &TypeExpr,
        ty: TypeId,
        frame: super::TypeFormFrame,
        union_form_children: Option<Vec<radix_hir::HirTypeFormArg>>,
    ) -> radix_hir::HirTypeForm {
        let head = match &expr.kind {
            TypeExprKind::Infer | TypeExprKind::FieldListHole => radix_hir::HirTypeFormHead::Infer,
            TypeExprKind::UnionHole => radix_hir::HirTypeFormHead::UnionHole,
            _ => match frame.named {
                Some((def_id, path)) => radix_hir::HirTypeFormHead::Named { def_id, path },
                None => match frame.imported_alias {
                    Some(path) => radix_hir::HirTypeFormHead::ImportedAlias { path },
                    None => radix_hir::HirTypeFormHead::Semantic(expr.form),
                },
            },
        };
        let mut args = match &expr.kind {
            TypeExprKind::Func(_) | TypeExprKind::Intersection(_) => Vec::new(),
            TypeExprKind::Union(_) => union_form_children.unwrap_or_default(),
            _ => frame.args,
        };
        // Range endpoints and the inclusive/exclusive glyph do not contribute
        // to the resolved type's child tree. Keep the source argument beside
        // the witness so the formatter can re-emit the authored surface.
        if let TypeExprKind::Named(_, params) = &expr.kind {
            args.extend(params.iter().filter_map(|param| {
                let GenericArgExpr::Range(range) = param else {
                    return None;
                };
                let bound = |bound: &RangeBoundExpr| match bound {
                    RangeBoundExpr::Literal { value, .. } => {
                        radix_hir::HirRangeBound::Literal(value.to_string())
                    }
                    RangeBoundExpr::Unbounded { negative, .. } => {
                        radix_hir::HirRangeBound::Unbounded {
                            negative: *negative,
                        }
                    }
                    RangeBoundExpr::Name { name, lifted, .. } => radix_hir::HirRangeBound::Name {
                        name: name.name,
                        lifted: *lifted,
                    },
                };
                Some(radix_hir::HirTypeFormArg::Range(
                    radix_hir::HirRangeArgument {
                        lo: bound(&range.lo),
                        hi: bound(&range.hi),
                        inclusive: matches!(range.kind, RangeKind::Inclusive),
                    },
                ))
            }));
        }
        radix_hir::HirTypeForm {
            ty,
            head,
            args,
            // The label is position-owned: `lower_type_with_context` stamps it
            // on the completed node, where the parent frame reads it.
            label: None,
        }
    }

    /// Normalize a source union into the canonical semantic union/option shape.
    ///
    /// `T ∪ nihil` is the source spelling for nullable value types. HIR stores
    /// that as `Option<T>` so typecheck and codegen have one nullability shape
    /// to reason about, while non-null unions remain explicit `Type::Union`
    /// values after duplicate members are removed.
    /// Lower a source union. Returns the union's type id and, when a
    /// `de`/`in` marker hoists onto an optional union (Option A), the hoisted
    /// mode: the caller applies it through the ordinary marker path so the
    /// authored-form capture records the wrapper (`de Node ∪ none`).
    fn lower_union_type(
        &mut self,
        members: &[TypeExpr],
    ) -> (TypeId, Option<radix_syntax::TypeMode>) {
        if members.is_empty() {
            self.error("empty_union_type");
            return (self.types.intern(Type::Error), None);
        }

        // Option A (operator ruling 2026-10-15): a `de`/`in` access marker
        // written before the first member of an optional union — `ref Node ∪
        // none`, `mut Node ∪ none` — applies to the whole optional cell, so
        // the marker wraps the option (`Ref(Option(Node))`). Only the two
        // re-pointable markers hoist, only when exactly one member carries a
        // top-level mode, and only when the union is optional (a `nihil`
        // member is present); any other shape keeps the
        // `nested_access_marker` refusal. An optional scalar is still
        // `scalar_access_marker`: the ruling refuses `ref` over plain scalars
        // and over optional scalars alike.
        let mut marked_member: Option<usize> = None;
        let mut seen_modes = 0;
        for (index, member) in members.iter().enumerate() {
            if let Some(mode) = member.mode {
                seen_modes += 1;
                if matches!(
                    mode,
                    radix_syntax::TypeMode::De | radix_syntax::TypeMode::In
                ) {
                    marked_member = Some(index);
                }
            }
        }

        let mut cleaned: Vec<TypeId> = Vec::with_capacity(members.len());
        let mut had_nihil = false;

        for (index, member) in members.iter().enumerate() {
            let lowered = if marked_member == Some(index) {
                self.lower_hoisted_union_member(member)
            } else {
                self.lower_nested_type(member)
            };
            if let Type::Primitive(Primitive::Nihil) = self.types.get(lowered) {
                had_nihil = true;
                continue;
            }
            cleaned.push(lowered);
        }

        // BO-S-4: collect → sort → adjacent dedup (O(n log n + n·equals))
        // instead of O(n²) pairwise is_distinct_union_member scans.
        //
        // Repair (fbe894af): the sort key must cluster `equals`-true members.
        // `Vec::dedup_by(equals)` only removes *adjacent* equals-equal
        // members, and the raw `TypeId` ordinal does not guarantee that a
        // `Type::Alias` and the type it resolves to land next to each other
        // (aliases are interned at resolve time, so an unrelated member can
        // sort between them). `union_member_sort_key` resolves that: it peels
        // top-level alias chains and normalizes the numerus/fractus coercion
        // pairs, so `equals(a, b)` implies `key(a) == key(b)` and every
        // equals-true pair is adjacent before the dedup.
        cleaned.sort_by_key(|id| (self.types.union_member_sort_key(*id).0, id.0));
        cleaned.dedup_by(|a, b| self.types.equals(*a, *b));

        if cleaned.is_empty() {
            self.error("union_only_nihil");
            return (self.types.intern(Type::Error), None);
        }

        let base = match cleaned.as_slice() {
            [only] => *only,
            many => self.types.intern(Type::Union(many.to_vec())),
        };

        let union_id = if had_nihil {
            self.types.option(base)
        } else {
            base
        };

        let hoisted = marked_member
            .filter(|_| seen_modes == 1 && had_nihil)
            .map(|index| members[index].mode.expect("marked member carries a mode"));
        if marked_member.is_some() && seen_modes == 1 && !had_nihil {
            // A shaped union that is not optional (`ref A ∪ B`) keeps the
            // nested-marker refusal.
            self.error("nested_access_marker");
        }
        (union_id, hoisted)
    }

    /// Lower the mode-carrying member of a hoisted optional union: the
    /// marker moves to the union, so the member lowers bare. The scalar
    /// refusal (`ref int ∪ none`) fires on the member itself — the ruling
    /// refuses `ref` over plain scalars and over optional scalars alike.
    fn lower_hoisted_union_member(&mut self, member: &TypeExpr) -> TypeId {
        let bare = TypeExpr {
            mode: None,
            ..member.clone()
        };
        let lowered = self.lower_nested_type(&bare);
        if self.types.is_scalar_for_access_marker(lowered) {
            self.error(crate::ISSUE_SCALAR_ACCESS_MARKER);
            return self.types.intern(Type::Error);
        }
        lowered
    }

    /// Normalize a source intersection into the canonical semantic shape.
    ///
    /// BO-S-4-identical to union lowering: collect → sort → adjacent dedup,
    /// single-member collapse to the member. R2: operands are nominal-only
    /// (interface/struct/alias-of-nominal); primitive operands are rejected —
    /// the widening-lattice meet is deferred semantics, not sugar.
    fn lower_intersection_type(&mut self, members: &[TypeExpr]) -> TypeId {
        if members.is_empty() {
            self.error("empty_intersection_type");
            return self.types.intern(Type::Error);
        }

        let mut cleaned: Vec<TypeId> = Vec::with_capacity(members.len());
        for member in members {
            let lowered = self.lower_nested_type(member);
            match self.types.get(lowered) {
                Type::Primitive(_) => {
                    self.error("intersection_primitive_operand");
                    return self.types.intern(Type::Error);
                }
                _ => cleaned.push(lowered),
            }
        }

        cleaned.sort_by_key(|id| (self.types.union_member_sort_key(*id).0, id.0));
        cleaned.dedup_by(|a, b| self.types.equals(*a, *b));

        match cleaned.as_slice() {
            [only] => *only,
            many => self.types.intern(Type::Intersection(many.to_vec())),
        }
    }

    fn lower_generic_type_arg(&mut self, arg: &GenericArgExpr) -> TypeId {
        match arg {
            GenericArgExpr::Type(ty) => self.lower_nested_type(ty),
            GenericArgExpr::Name(name) => self.lower_named_type(name, &[]),
            GenericArgExpr::Size(_) => {
                self.error("generic_size_argument_in_type_position");
                self.types.intern(Type::Error)
            }
            GenericArgExpr::Range(_) => {
                self.error(crate::numeric_width::WidthLowerIssue::RANGED_ARITY);
                self.types.intern(Type::Error)
            }
        }
    }

    fn lower_generic_index_arg(&mut self, arg: &GenericArgExpr) -> Option<IndexId> {
        match arg {
            GenericArgExpr::Size(figura) => self.lower_figura_expr(figura).ok(),
            GenericArgExpr::Type(ty) if matches!(ty.kind, TypeExprKind::Infer) => {
                let var = self.types.fresh_index_infer_var();
                Some(self.types.intern_index(IndexExpr::Infer(var)))
            }
            GenericArgExpr::Name(ident) => self.lower_named_index_arg(ident),
            // Bounded-capacity applications keep the explicit `Type` shape, so
            // a bare named index argument arrives here as `Type(Named(…))`;
            // digit spellings and index params lower exactly like `Name`.
            GenericArgExpr::Type(TypeExpr {
                kind: TypeExprKind::Named(ident, nested),
                ..
            }) if nested.is_empty() => {
                if let Some(value) =
                    radix_types::parse_natural_capacity(self.interner.resolve(ident.name))
                {
                    return Some(self.types.intern_index(IndexExpr::Literal(value)));
                }
                self.lower_named_index_arg(ident)
            }
            GenericArgExpr::Type(_) => {
                self.error("generic_type_argument_in_size_position");
                None
            }
            GenericArgExpr::Range(_) => {
                self.error(crate::numeric_width::WidthLowerIssue::RANGED_ARITY);
                None
            }
        }
    }

    /// True when `name` resolves to a declared `IndexParam` symbol.
    fn is_declared_index_param(&self, name: radix_lexer::Symbol) -> bool {
        self.resolver
            .lookup(name)
            .and_then(|def_id| self.resolver.get_symbol(def_id))
            .is_some_and(|symbol| symbol.kind == crate::SymbolKind::IndexParam)
    }

    fn lower_named_index_arg(&mut self, ident: &Ident) -> Option<IndexId> {
        match self.lexical_generic_param_kind(ident.name) {
            Some(GenericParamKind::Magnitudo) => {
                Some(self.types.intern_index(IndexExpr::Param(ident.name)))
            }
            Some(GenericParamKind::Typus) => {
                self.error("generic_type_argument_in_size_position");
                None
            }
            None => {
                if self.is_declared_index_param(ident.name) {
                    Some(self.types.intern_index(IndexExpr::Param(ident.name)))
                } else {
                    self.error("generic_type_argument_in_size_position");
                    None
                }
            }
        }
    }

    pub(super) fn lower_applied_arg(&mut self, arg: &GenericArgExpr) -> AppliedArg {
        match arg {
            GenericArgExpr::Size(_) => self
                .lower_generic_index_arg(arg)
                .map(AppliedArg::Size)
                .unwrap_or_else(|| {
                    AppliedArg::Size(self.types.intern_index(IndexExpr::Unspecified))
                }),
            GenericArgExpr::Type(ty) if matches!(ty.kind, TypeExprKind::Infer) => {
                AppliedArg::Type(self.lower_nested_type(ty))
            }
            GenericArgExpr::Name(ident)
                if matches!(
                    self.lexical_generic_param_kind(ident.name),
                    Some(GenericParamKind::Magnitudo)
                ) =>
            {
                self.lower_generic_index_arg(arg)
                    .map(AppliedArg::Size)
                    .unwrap_or_else(|| {
                        AppliedArg::Size(self.types.intern_index(IndexExpr::Unspecified))
                    })
            }
            GenericArgExpr::Name(ident) => {
                if self.is_declared_index_param(ident.name) {
                    self.lower_generic_index_arg(arg)
                        .map(AppliedArg::Size)
                        .unwrap_or_else(|| {
                            AppliedArg::Size(self.types.intern_index(IndexExpr::Unspecified))
                        })
                } else {
                    AppliedArg::Type(self.lower_generic_type_arg(arg))
                }
            }
            GenericArgExpr::Type(ty) => AppliedArg::Type(self.lower_nested_type(ty)),
            GenericArgExpr::Range(_) => {
                self.error(crate::numeric_width::WidthLowerIssue::RANGED_ARITY);
                AppliedArg::Type(self.types.intern(Type::Error))
            }
        }
    }

    /// True when `def_id` declares at least one generic parameter, by either
    /// descriptor list or kind list.
    fn has_declared_generic_params(&self, def_id: crate::DefId) -> bool {
        self.resolver
            .generic_param_descriptors(def_id)
            .is_some_and(|descriptors| !descriptors.is_empty())
            || self
                .resolver
                .generic_param_kinds(def_id)
                .is_some_and(|kinds| !kinds.is_empty())
    }

    /// Lower a named declaration's explicit prefix and append its declared
    /// trailing defaults. This is the type-application counterpart of the
    /// GPD-3 callable binder: positional explicit arguments are sort-checked,
    /// only a defaulted suffix may be omitted, and a required omission fails
    /// before an incomplete `Type::Applied` can escape lowering.
    fn lower_declared_applied_args(
        &mut self,
        def_id: crate::DefId,
        params: &[GenericArgExpr],
    ) -> Option<Vec<AppliedArg>> {
        let descriptors = self
            .resolver
            .generic_param_descriptors(def_id)
            .map(ToOwned::to_owned);
        let Some(descriptors) = descriptors else {
            let kinds = self
                .resolver
                .generic_param_kinds(def_id)
                .map(ToOwned::to_owned);
            return Some(self.lower_applied_args_directed(kinds.as_deref(), params));
        };

        if !crate::types::generic_explicit_prefix_len_in_range(&descriptors, params.len()) {
            self.error("generic_argument_arity_mismatch");
            return None;
        }

        let kinds: Vec<GenericParamKind> = descriptors
            .iter()
            .map(|param| match param.sort {
                crate::GenericParamSort::Typus => GenericParamKind::Typus,
                crate::GenericParamSort::Magnitudo => GenericParamKind::Magnitudo,
            })
            .collect();
        let mut args = self.lower_applied_args_directed(Some(&kinds[..params.len()]), params);
        for descriptor in descriptors.iter().skip(params.len()) {
            let Some(default) = &descriptor.default else {
                // This is only reachable for a malformed non-trailing
                // declaration default. Keep the binder fail-closed.
                self.error("generic_argument_arity_mismatch");
                return None;
            };
            args.push(match default {
                SemanticGenericParamDefault::Typus(ty) => AppliedArg::Type(*ty),
                SemanticGenericParamDefault::Magnitudo(value) => {
                    AppliedArg::Size(self.types.intern_index(IndexExpr::Literal(*value)))
                }
            });
        }
        Some(args)
    }

    /// Lower applied arguments under an ordered declaration-kind list.
    ///
    /// `None` keeps the undirected legacy path for callees whose declaration
    /// kinds are unknown (builtins, identity-less legacy interfaces).
    fn lower_applied_args_directed(
        &mut self,
        kinds: Option<&[GenericParamKind]>,
        params: &[GenericArgExpr],
    ) -> Vec<AppliedArg> {
        let Some(kinds) = kinds else {
            return params
                .iter()
                .map(|arg| self.lower_applied_arg(arg))
                .collect();
        };
        if kinds.len() != params.len() {
            self.error("generic_argument_arity_mismatch");
        }
        params
            .iter()
            .zip(kinds.iter())
            .map(|(arg, kind)| match kind {
                GenericParamKind::Typus => match arg {
                    GenericArgExpr::Size(_) => {
                        self.error("generic_argument_kind_mismatch");
                        AppliedArg::Type(self.types.intern(Type::Error))
                    }
                    GenericArgExpr::Name(ident) if self.is_declared_index_param(ident.name) => {
                        self.error("generic_argument_kind_mismatch");
                        AppliedArg::Type(self.types.intern(Type::Error))
                    }
                    _ => AppliedArg::Type(self.lower_generic_type_arg(arg)),
                },
                GenericParamKind::Magnitudo => match arg {
                    GenericArgExpr::Type(ty)
                        if !matches!(ty.kind, TypeExprKind::Infer)
                            && !self.type_arg_names_an_index(&ty.kind) =>
                    {
                        self.error("generic_argument_kind_mismatch");
                        AppliedArg::Size(self.types.intern_index(IndexExpr::Unspecified))
                    }
                    GenericArgExpr::Name(ident)
                        if !self.is_declared_index_param(ident.name)
                            && !matches!(
                                self.lexical_generic_param_kind(ident.name),
                                Some(GenericParamKind::Magnitudo)
                            ) =>
                    {
                        self.error("generic_argument_kind_mismatch");
                        AppliedArg::Size(self.types.intern_index(IndexExpr::Unspecified))
                    }
                    _ => self
                        .lower_generic_index_arg(arg)
                        .map(AppliedArg::Size)
                        .unwrap_or_else(|| {
                            AppliedArg::Size(self.types.intern_index(IndexExpr::Unspecified))
                        }),
                },
            })
            .collect()
    }

    /// True when a `Type(Named(…))` argument names a compile-time index — a
    /// NATURAL digit spelling or a declared index/magnitudo param — and so
    /// binds a `magnitudo` slot instead of mismatching.
    fn type_arg_names_an_index(&self, kind: &TypeExprKind) -> bool {
        let TypeExprKind::Named(ident, nested) = kind else {
            return false;
        };
        if !nested.is_empty() {
            return false;
        }
        radix_types::parse_natural_capacity(self.interner.resolve(ident.name)).is_some()
            || self.is_declared_index_param(ident.name)
            || matches!(
                self.lexical_generic_param_kind(ident.name),
                Some(GenericParamKind::Magnitudo)
            )
    }

    /// Resolve a named type spelling before generic application.
    ///
    /// Primitive and collection names are language-owned spellings with fixed
    /// arity rules. User names must resolve to symbols that actually denote
    /// types; values, functions, and unresolved names become `Type::Error`
    /// rather than leaking expression-level definitions into type syntax.
    fn lower_named_type(&mut self, name: &Ident, params: &[GenericArgExpr]) -> TypeId {
        self.current_span = name.span;

        let name_text = self.interner.resolve(name.name).to_owned();
        let name_str = name_text.as_str();

        if let Some(ty) = self.lower_primitive_named_type(name_str, params) {
            return ty;
        }

        if let Some(ty) = self.lower_frame_named_type(name, name_str, params) {
            return ty;
        }

        if self.is_lexical_type_param(name.name) {
            return self.types.intern(Type::Param(name.name));
        }

        // Applied NATURAL (`linear_from_raw<2, 2, 2>`) is interned as Param so
        // call-site typecheck can bind magnitudo slots. Digit spellings are not
        // type names.
        if params.is_empty() && radix_types::parse_natural_capacity(name_str).is_some() {
            return self.types.intern(Type::Param(name.name));
        }

        let Some(def_id) = self.resolver.lookup_type(name.name) else {
            // A selectively imported type (D22.5) is the imported `TypeId`
            // itself, exactly as its qualified `module.Type` path lowers.
            if let Some(type_id) = self.resolver.lookup_selective_import_type(name.name) {
                self.record_imported_named_head(type_id, vec![name.name], !params.is_empty());
                if params.is_empty() {
                    return type_id;
                }
                let decl_kinds = self
                    .resolver
                    .lookup_selective_import_decl_param_kinds(name.name);
                let args = self.lower_applied_args_directed(decl_kinds.as_deref(), params);
                return self.types.intern(Type::Applied(type_id, args));
            }
            self.error("unknown_type_name");
            return self.types.intern(Type::Error);
        };

        // Authored-form capture: record the resolved user definition so the
        // completed head can carry the authored spelling beside its identity.
        if let Some(frame) = self.form_frames.last_mut() {
            frame.named = Some((def_id, vec![name.name]));
        }

        let Some(symbol) = self.resolver.get_symbol(def_id) else {
            self.error("missing_type_symbol_info");
            return self.types.intern(Type::Error);
        };

        let symbol_kind = symbol.kind;
        let symbol_ty = symbol.ty;
        let base = match symbol_kind {
            crate::SymbolKind::Struct => self.types.intern(Type::Struct(def_id)),
            crate::SymbolKind::Enum => self.types.intern(Type::Enum(def_id)),
            crate::SymbolKind::Interface => self.types.intern(Type::Interface(def_id)),
            crate::SymbolKind::TypeAlias => {
                if let Some(resolved) = symbol_ty {
                    let body = crate::index::freshen_bounded_capacity_holes(self.types, resolved);
                    self.types.intern(Type::Alias(def_id, body))
                } else {
                    self.error("missing_resolved_type_alias");
                    self.types.intern(Type::Error)
                }
            }
            crate::SymbolKind::TypeParam => self.types.intern(Type::Param(symbol.name)),
            _ => {
                self.error("type_name_not_type");
                self.types.intern(Type::Error)
            }
        };

        let has_declared_params = self.has_declared_generic_params(def_id);
        if params.is_empty() && (!has_declared_params || self.allow_bare_generic_construction) {
            return base;
        }

        let Some(args) = self.lower_declared_applied_args(def_id, params) else {
            return self.types.intern(Type::Error);
        };
        if args.is_empty() {
            return base;
        }
        if symbol_kind == crate::SymbolKind::TypeAlias && has_declared_params && !params.is_empty()
        {
            let descriptors = self
                .resolver
                .generic_param_descriptors(def_id)
                .map(ToOwned::to_owned);
            if let Some(descriptors) = descriptors {
                let has_size_params = descriptors
                    .iter()
                    .any(|descriptor| descriptor.sort == crate::GenericParamSort::Magnitudo);
                let has_size_args = args.iter().any(|arg| matches!(arg, AppliedArg::Size(_)));
                if !has_size_params && !has_size_args {
                    let vars: FxHashMap<_, _> = descriptors
                        .iter()
                        .zip(&args)
                        .filter_map(|(descriptor, arg)| match arg {
                            AppliedArg::Type(ty) => Some((descriptor.name, *ty)),
                            AppliedArg::Size(_) => None,
                        })
                        .collect();
                    if let Type::Alias(alias, body) = self.types.get(base).clone() {
                        let body = crate::index::substitute_alias_params(self.types, body, &vars);
                        return self.types.intern(Type::Alias(alias, body));
                    }
                }
            }
        }
        self.types.intern(Type::Applied(base, args))
    }

    /// Lower the builtin primitive, word-policy, width, and SQL-row names.
    ///
    /// Returns `None` when `name_str` is not one of these builtins, so the
    /// caller can continue with the frame, shape, and user-type paths.
    fn lower_primitive_named_type(
        &mut self,
        name_str: &str,
        params: &[GenericArgExpr],
    ) -> Option<TypeId> {
        if let Some(ty) = self.lower_locale_pack_type(name_str, params) {
            return Some(ty);
        }

        // The parser canonicalizes localized policy markers to their Latin
        // identities before HIR lowering. Resolve those identities directly
        // into the tagged marker type, even when the source surface was
        // localized (for example English `clamp` → canonical `clampa`).
        if let Some(policy) = BuiltinTensorEdgePolicy::from_latin(name_str) {
            return Some(self.lower_tensor_edge_policy_type(policy, params));
        }

        if let Some(prim) = Primitive::from_name(name_str) {
            return Some(self.lower_primitive_type(prim, params));
        }

        if name_str == "modulus" {
            return Some(self.lower_modular_word_type(params));
        }

        if name_str == "saturatus" {
            return Some(self.lower_saturating_word_type(params));
        }

        if name_str == "exactus" {
            return Some(self.lower_trapping_word_type(params));
        }

        if crate::numeric_width::is_standalone_width_name(name_str) {
            if crate::numeric_width::has_range_arg(params) {
                // The policy word is mandatory with a range (`u8<0‥8>` is
                // refused), so the edge behaviour is always visible.
                self.error(crate::numeric_width::WidthLowerIssue::RangedPolicyWordRequired.issue());
                return Some(self.types.intern(Type::Error));
            }
            let Some(type_params) = generic_type_exprs(params) else {
                self.error("generic_size_argument_in_type_position");
                return Some(self.types.intern(Type::Error));
            };
            let mut issues = Vec::new();
            let Some(ty) = crate::numeric_width::lower_standalone_width_type(
                name_str,
                &type_params,
                self.types,
                &mut |issue| issues.push(issue),
            ) else {
                self.error("standalone_width_internal");
                return Some(self.types.intern(Type::Error));
            };
            if let Some(issue) = issues.first() {
                self.error(issue.issue());
            }
            return Some(ty);
        }

        // SQL row: dynamic column map (faber::Valor per column).
        if name_str == "series" {
            if !params.is_empty() {
                self.error("series_params");
                return Some(self.types.intern(Type::Error));
            }
            let textus = self.types.primitive(Primitive::Textus);
            let valor = self.types.primitive(Primitive::Valor);
            return Some(self.types.map(textus, valor));
        }

        if name_str == "intervallum" {
            if params.len() != 1 && params.len() != 2 {
                self.error("intervallum_arity");
                return Some(self.types.intern(Type::Error));
            }
            let bound = self.lower_generic_type_arg(&params[0]);
            if !crate::is_intervallum_bound_type(self.types, bound) {
                self.error("intervallum_bound");
                return Some(self.types.intern(Type::Error));
            }
            let Some(step_arg) = params.get(1) else {
                return Some(self.types.intervallum(bound));
            };
            let step = match crate::passes::resolve::intervallum_step_index(step_arg, self.types) {
                Ok(step) => step,
                Err(issue) => {
                    self.error(issue);
                    return Some(self.types.intern(Type::Error));
                }
            };
            if !crate::passes::resolve::is_integer_intervallum_bound(self.types, bound) {
                self.error("intervallum_step_bound_not_integer");
                return Some(self.types.intern(Type::Error));
            }
            return Some(self.types.intervallum_with_step(bound, step));
        }

        None
    }

    /// Lower the builtin frame, shape, tuple, ratio, and collection names.
    ///
    /// Returns `None` when `name_str` is not one of these builtins.
    fn lower_frame_named_type(
        &mut self,
        name: &Ident,
        name_str: &str,
        params: &[GenericArgExpr],
    ) -> Option<TypeId> {
        if name_str == "sermo" && !params.is_empty() {
            // `sermo<O, R>` (D6.11); bare `sermo` is `sermo<valor, valor>`
            // and resolves through the ordinary builtin symbol.
            let Some(def_id) = crate::builtins::frame_types::sermo_def_id(self.resolver) else {
                self.error("sermo_missing_builtin");
                return Some(self.types.intern(Type::Error));
            };
            let [opener, reply] = params else {
                self.error("sermo_arity");
                return Some(self.types.intern(Type::Error));
            };
            let opener = self.lower_generic_type_arg(opener);
            let reply = self.lower_generic_type_arg(reply);
            // Authored-form capture: the conversation word names the builtin
            // definition and keeps both authored child forms. Finalize erases
            // `sermo<O, R>` to the bare carrier (D6.11), which canonicalizes
            // the children's witnesses without dropping the authored
            // arguments, so re-emission can print `sermo<numerus, numerus>`.
            if let Some(frame) = self.form_frames.last_mut() {
                frame.named = Some((def_id, vec![name.name]));
            }
            return Some(crate::builtins::frame_types::sermo_typed_type_id(
                self.types, def_id, opener, reply,
            ));
        }

        if name_str == "scrinium" {
            let Some(def_id) = crate::builtins::frame_types::scrinium_def_id(self.resolver) else {
                self.error("scrinium_missing_builtin");
                return Some(self.types.intern(Type::Error));
            };
            let payload_ty = match params {
                [] => self.types.primitive(Primitive::Valor),
                [payload] => self.lower_generic_type_arg(payload),
                _ => {
                    self.error("scrinium_arity");
                    return Some(self.types.intern(Type::Error));
                }
            };
            // Authored-form capture: the frame word names the builtin
            // definition. Its witness stays `scrinium<T>` (unlike the erased
            // conversation type), so the payload child keeps its form.
            if let Some(frame) = self.form_frames.last_mut() {
                frame.named = Some((def_id, vec![name.name]));
            }
            return Some(crate::builtins::frame_types::scrinium_payload_type_id(
                self.types, def_id, payload_ty,
            ));
        }

        if name_str == "atomic" {
            if params.len() == 1 {
                self.error("atomic_compiler_owned_syntax");
            } else {
                self.error("atomic_arity");
            }
            return Some(self.types.intern(Type::Error));
        }

        if name_str == "tensor" {
            if params.is_empty() {
                self.error("tensor_incomplete");
            } else {
                self.error("tensor_missing_figura");
            }
            return Some(self.types.intern(Type::Error));
        }

        if name_str == "sparsa" {
            if params.is_empty() {
                self.error("sparsa_incomplete");
            } else {
                self.error("sparsa_missing_figura");
            }
            return Some(self.types.intern(Type::Error));
        }

        if name_str == "matrix" {
            if params.is_empty() {
                self.error("matrix_incomplete");
            } else {
                self.error("matrix_missing_figura");
            }
            return Some(self.types.intern(Type::Error));
        }

        if name_str == "iuncta" {
            return Some(self.lower_iuncta_type(params));
        }

        if name_str == "ratio" {
            return Some(self.lower_ratio_type(params));
        }

        if let Some(collection) = CollectionKind::from_name(name_str) {
            return Some(self.lower_collection_type(collection, params));
        }

        None
    }

    /// Lower `iuncta<...>` into a monomorphic tuple of element types.
    fn lower_iuncta_type(&mut self, params: &[GenericArgExpr]) -> TypeId {
        let elements: Vec<TypeId> = params
            .iter()
            .map(|param| {
                let GenericArgExpr::Type(param) = param else {
                    self.error("iuncta_type_arg_shape");
                    return self.types.intern(Type::Error);
                };
                if matches!(param.kind, TypeExprKind::UnionHole) {
                    // A standalone ∪ is a heterogeneous accumulation hole for
                    // collection elements, not a valid monomorphic tuple
                    // position. Reject it here so the issue points at the
                    // offending iuncta element instead of leaking WARN017.
                    self.current_span = param.span;
                    self.error("iuncta_union_type_arg_unsupported");
                    self.types.intern(Type::Error)
                } else {
                    self.lower_nested_type(param)
                }
            })
            .collect();
        self.types.tuple(elements)
    }

    /// Lower `ratio<label: T, …>` into interned `Type::Record`.
    ///
    /// Labels become the record field keys and never enter any other type's
    /// identity. Parser already requires a label on every ratio slot.
    /// `_` lowers as a monomorphic hole (solved later from the field witness);
    /// a standalone `∪` is not a valid ratio element and is rejected here so
    /// the issue points at the slot instead of leaking WARN017.
    fn lower_ratio_type(&mut self, params: &[GenericArgExpr]) -> TypeId {
        let mut fields = Vec::with_capacity(params.len());
        for arg in params {
            let GenericArgExpr::Type(param) = arg else {
                self.error("ratio_type_arg_shape");
                continue;
            };
            let Some(label) = param.iuncta_label.as_ref() else {
                continue;
            };
            let ty = if matches!(param.kind, TypeExprKind::UnionHole) {
                self.current_span = param.span;
                self.error("ratio_union_type_arg_unsupported");
                self.types.intern(Type::Error)
            } else {
                self.lower_nested_type(param)
            };
            fields.push((label.name, ty));
        }
        self.types.record(fields)
    }

    fn lower_locale_pack_type(&mut self, name: &str, params: &[GenericArgExpr]) -> Option<TypeId> {
        let binding = self.locale_pack?.type_binding_kind(name)?;
        match binding {
            crate::locale::TypeBindingKind::Primitive(prim) => {
                Some(self.lower_primitive_type(prim, params))
            }
            crate::locale::TypeBindingKind::Collection(collection) => {
                Some(self.lower_collection_type(collection, params))
            }
            crate::locale::TypeBindingKind::ModularWord => {
                Some(self.lower_modular_word_type(params))
            }
            crate::locale::TypeBindingKind::SaturatingWord => {
                Some(self.lower_saturating_word_type(params))
            }
            crate::locale::TypeBindingKind::TrappingWord => {
                Some(self.lower_trapping_word_type(params))
            }
            crate::locale::TypeBindingKind::TensorEdgePolicy(policy) => {
                Some(self.lower_tensor_edge_policy_type(policy, params))
            }
            crate::locale::TypeBindingKind::Ratio => Some(self.lower_ratio_type(params)),
            // The tensor family lower through dedicated shape/width syntax
            // (`TypeExprKind::Tensor(elem, figura)` etc.), never through a flat
            // type-argument list. Fall through to the hardcoded name paths in
            // `lower_named_type`, which report `tensor_incomplete` etc. for
            // bare (incomplete) family spellings.
            // Census relational type names are reader-surface rows only in S2
            // (no HIR/MIR type yet; the real surface is S3/S4). Fall through to
            // the hardcoded name paths in `lower_named_type` (bare `series`
            // keeps its dynamic row-map special case).
            // `erratum<T>` identity lowering lands in the follow-up unit.
            // `trapped<R, T>` lowers by its canonical registry name until the
            // trap unit wires the localized spelling through.
            // Conversation-family names lower through the builtin frame
            // structs by their canonical Latin name in `lower_named_type`.
            crate::locale::TypeBindingKind::TensorFamily(_)
            | crate::locale::TypeBindingKind::Relational(_)
            | crate::locale::TypeBindingKind::Erratum
            | crate::locale::TypeBindingKind::Trapped
            | crate::locale::TypeBindingKind::Frame(_) => None,
        }
    }

    fn lower_tensor_edge_policy_type(
        &mut self,
        policy: crate::BuiltinTensorEdgePolicy,
        params: &[GenericArgExpr],
    ) -> TypeId {
        if !params.is_empty() {
            self.error("primitive_params");
            return self.types.intern(Type::Error);
        }
        self.types.tensor_edge_policy(policy)
    }

    fn lower_primitive_type(&mut self, prim: Primitive, params: &[GenericArgExpr]) -> TypeId {
        // Bounded textus/ascii/octeti capacities are index arguments, not type
        // arguments. Lower this family before the width/type-only helper so a
        // literal capacity reaches the shared IndexExpr arena.
        if matches!(prim, Primitive::Ascii | Primitive::Textus) {
            return self.lower_bounded_primitive(prim, params);
        }
        let Some(type_params) = generic_type_exprs(params) else {
            self.error("generic_size_argument_in_type_position");
            return self.types.intern(Type::Error);
        };
        // WHY: `instans<ms|us|ns>` uses reserved precision markers, parallel to
        // sized `numerus`/`fractus` width markers — not generic type parameters.
        if matches!(prim, Primitive::Instans) {
            let mut messages = Vec::new();
            let ty = crate::instans_precision::lower_sized_instans(
                &type_params,
                self.interner,
                self.types,
                &mut |message| messages.push(message.to_owned()),
            );
            if let Some(message) = messages.first() {
                self.error_from_message(message);
            }
            return ty;
        }
        if !params.is_empty() {
            self.error("primitive_params");
            return self.types.intern(Type::Error);
        }
        self.types.primitive(prim)
    }

    fn lower_bounded_primitive(&mut self, prim: Primitive, params: &[GenericArgExpr]) -> TypeId {
        match params {
            [] => self.types.primitive(prim),
            [capacity]
                if prim == Primitive::Ascii
                    && generic_arg_is_ascii_format_hint(capacity, self.interner) =>
            {
                self.error("ascii_format_hint_not_capacity");
                self.types.intern(Type::Error)
            }
            [capacity] => {
                let Some(capacity) = self.lower_generic_index_arg(capacity) else {
                    return self.types.intern(Type::Error);
                };
                match prim {
                    Primitive::Ascii => self.types.bounded_ascii(capacity),
                    Primitive::Textus => self.types.bounded_textus(capacity),
                    _ => {
                        self.error("primitive_params");
                        self.types.intern(Type::Error)
                    }
                }
            }
            _ => {
                self.error("primitive_params");
                self.types.intern(Type::Error)
            }
        }
    }
}

fn generic_arg_is_ascii_format_hint(
    param: &GenericArgExpr,
    interner: &radix_lexer::Interner,
) -> bool {
    match param {
        GenericArgExpr::Name(ident) => {
            crate::numeric_width::is_ascii_format_hint(interner.resolve(ident.name))
        }
        GenericArgExpr::Type(ty) => match &ty.kind {
            TypeExprKind::Named(ident, nested) if nested.is_empty() => {
                crate::numeric_width::is_ascii_format_hint(interner.resolve(ident.name))
            }
            _ => false,
        },
        GenericArgExpr::Size(_) | GenericArgExpr::Range(_) => false,
    }
}

/// Signature of one sized-word lowering entry ([`crate::numeric_width`]'s
/// `lower_modular_word`/`lower_trapping_word`/`lower_saturating_word`).
type WordLowering = fn(
    &[TypeExpr],
    &radix_lexer::Interner,
    &mut radix_types::TypeTable,
    &mut dyn FnMut(crate::numeric_width::WidthLowerIssue),
) -> TypeId;

impl Lowerer<'_> {
    fn lower_modular_word_type(&mut self, params: &[GenericArgExpr]) -> TypeId {
        self.lower_word_type(
            params,
            radix_types::IntPolicy::Wrapping,
            crate::numeric_width::lower_modular_word,
        )
    }

    fn lower_trapping_word_type(&mut self, params: &[GenericArgExpr]) -> TypeId {
        self.lower_word_type(
            params,
            radix_types::IntPolicy::Trapping,
            crate::numeric_width::lower_trapping_word,
        )
    }

    fn lower_saturating_word_type(&mut self, params: &[GenericArgExpr]) -> TypeId {
        self.lower_word_type(
            params,
            radix_types::IntPolicy::Saturating,
            crate::numeric_width::lower_saturating_word,
        )
    }

    /// Lower one sized word wrapper (`modulus`/`saturatus`/`exactus`) and
    /// report its first width-lowering issue, if any. A range after the width
    /// (`saturating<u32, 0‥8>`) takes the ranged path.
    fn lower_word_type(
        &mut self,
        params: &[GenericArgExpr],
        policy: radix_types::IntPolicy,
        lower: WordLowering,
    ) -> TypeId {
        let mut ranged_issue = None;
        if let Some(ty) = crate::numeric_width::lower_ranged_word(
            policy,
            params,
            self.interner,
            self.types,
            &mut |issue| {
                ranged_issue.get_or_insert(issue);
            },
        ) {
            if let Some(issue) = ranged_issue {
                self.error(issue.issue());
            }
            return ty;
        }
        let Some(type_params) = generic_type_exprs(params) else {
            self.error("generic_size_argument_in_type_position");
            return self.types.intern(Type::Error);
        };
        let mut issues = Vec::new();
        let ty = lower(&type_params, self.interner, self.types, &mut |issue| {
            issues.push(issue);
        });
        if let Some(issue) = issues.first() {
            self.error(issue.issue());
        }
        ty
    }

    fn lower_collection_type(
        &mut self,
        collection: CollectionKind,
        params: &[GenericArgExpr],
    ) -> TypeId {
        if collection == CollectionKind::Lista {
            return self.lower_lista_type(params);
        }
        if matches!(collection, CollectionKind::Queue | CollectionKind::Stack) {
            return self.lower_queue_stack_type(collection, params);
        }
        if !collection.accepts_arity(params.len()) {
            self.error_from_message(collection.arity_error());
            return self.types.intern(Type::Error);
        }
        let lowered = params
            .iter()
            .map(|param| self.lower_generic_type_arg(param))
            .collect::<Vec<_>>();
        collection.lower(self.types, &lowered)
    }

    fn lower_lista_type(&mut self, params: &[GenericArgExpr]) -> TypeId {
        match params {
            [elem] => {
                let elem = self.lower_generic_type_arg(elem);
                self.types.array(elem)
            }
            [elem, GenericArgExpr::Range(range)] => {
                // A length range, `list<T, size lo‥hi>`: always the trapping range.
                let elem = self.lower_generic_type_arg(elem);
                let mut issue = None;
                let ty = radix_types::lower_ranged_list(elem, range, self.types, &mut |found| {
                    issue.get_or_insert(found);
                });
                if let Some(issue) = issue {
                    self.error(issue.issue());
                }
                ty
            }
            [elem, capacity] => {
                let elem = self.lower_generic_type_arg(elem);
                let Some(capacity) = self.lower_generic_index_arg(capacity) else {
                    return self.types.intern(Type::Error);
                };
                self.types.bounded_array(elem, capacity)
            }
            _ => {
                self.error_from_message(CollectionKind::Lista.arity_error());
                self.types.intern(Type::Error)
            }
        }
    }

    fn lower_queue_stack_type(
        &mut self,
        collection: CollectionKind,
        params: &[GenericArgExpr],
    ) -> TypeId {
        match params {
            [elem] => {
                let elem = self.lower_generic_type_arg(elem);
                match collection {
                    CollectionKind::Queue => self.types.queue(elem),
                    CollectionKind::Stack => self.types.stack(elem),
                    _ => {
                        self.error_from_message(collection.arity_error());
                        self.types.intern(Type::Error)
                    }
                }
            }
            [elem, capacity] => {
                let elem = self.lower_generic_type_arg(elem);
                let Some(capacity) = self.lower_generic_index_arg(capacity) else {
                    return self.types.intern(Type::Error);
                };
                match collection {
                    CollectionKind::Queue => self.types.bounded_queue(elem, capacity),
                    CollectionKind::Stack => self.types.bounded_stack(elem, capacity),
                    _ => {
                        self.error_from_message(collection.arity_error());
                        self.types.intern(Type::Error)
                    }
                }
            }
            _ => {
                self.error_from_message(collection.arity_error());
                self.types.intern(Type::Error)
            }
        }
    }

    /// Authored-form capture for an imported named type: record the imported
    /// definition beside the spelled `path` so the completed head is a `Named`
    /// constructor exactly as a locally declared type's is.
    ///
    /// `type_id` is the imported base type; its definition identity is the
    /// struct, enum, interface or alias it names. A base that names no
    /// definition is an imported alias carried as its resolved body: with no
    /// applied arguments it is recorded as an [`ImportedAlias`] head, and an
    /// applied one keeps the semantic head.
    ///
    /// [`ImportedAlias`]: radix_hir::HirTypeFormHead::ImportedAlias
    fn record_imported_named_head(
        &mut self,
        type_id: TypeId,
        path: Vec<radix_lexer::Symbol>,
        applied: bool,
    ) {
        let mut current = type_id;
        let def_id = loop {
            match self.types.get(current) {
                Type::Struct(def_id)
                | Type::Enum(def_id)
                | Type::Interface(def_id)
                | Type::Alias(def_id, _) => break *def_id,
                Type::Applied(base, _) => current = *base,
                _ => {
                    if !applied && let Some(frame) = self.form_frames.last_mut() {
                        frame.imported_alias = Some(path);
                    }
                    return;
                }
            }
        };
        if let Some(frame) = self.form_frames.last_mut() {
            frame.named = Some((def_id, path));
        }
    }

    fn lower_qualified_type(&mut self, path: &[Ident], params: &[GenericArgExpr]) -> TypeId {
        let Some((namespace, type_name)) = path.first().zip(path.last()) else {
            self.error("empty_qualified_type_path");
            return self.types.intern(Type::Error);
        };
        let Some(namespace_def_id) = self.resolver.lookup(namespace.name) else {
            self.error("unknown_namespace");
            return self.types.intern(Type::Error);
        };
        if !matches!(
            self.resolver
                .get_symbol(namespace_def_id)
                .map(|symbol| symbol.kind),
            Some(crate::SymbolKind::Module)
        ) {
            self.error("qualified_type_prefix_not_namespace");
            return self.types.intern(Type::Error);
        }
        if let Some(type_id) = self
            .resolver
            .namespace_file_type(namespace.name, type_name.name)
        {
            self.record_imported_named_head(
                type_id,
                path.iter().map(|segment| segment.name).collect(),
                !params.is_empty(),
            );
            // Imported declarations carry their ordered declaration kinds on
            // the file interface (S0-R1); kind mismatches fail closed instead
            // of silently accepting any argument spelling.
            let decl_kinds = self
                .resolver
                .namespace_file_decl_param_kinds(namespace.name, type_name.name);
            if params.is_empty() {
                // Same rule as the unqualified path: a bare generic name is
                // admitted only where construction infers the witnesses.
                let declares_params = decl_kinds.as_ref().is_some_and(|kinds| !kinds.is_empty());
                if declares_params && !self.allow_bare_generic_construction {
                    self.error("generic_argument_arity_mismatch");
                    return self.types.intern(Type::Error);
                }
                return type_id;
            }
            let args = self.lower_applied_args_directed(decl_kinds.as_deref(), params);
            return self.types.intern(Type::Applied(type_id, args));
        }
        if self
            .resolver
            .namespace_exports_member(namespace.name, type_name.name)
            && let Some((def_id, type_id)) =
                self.resolver
                    .lookup_type(type_name.name)
                    .and_then(|def_id| {
                        self.resolver
                            .get_symbol(def_id)
                            .and_then(|symbol| symbol.ty)
                            .map(|type_id| (def_id, type_id))
                    })
        {
            // Authored-form capture: the exported member is a resolved user
            // definition, recorded under the spelled qualified path.
            if let Some(frame) = self.form_frames.last_mut() {
                frame.named = Some((def_id, path.iter().map(|segment| segment.name).collect()));
            }
            let has_declared_params = self.has_declared_generic_params(def_id);
            if params.is_empty() && !has_declared_params {
                return type_id;
            }
            let Some(args) = self.lower_declared_applied_args(def_id, params) else {
                return self.types.intern(Type::Error);
            };
            if args.is_empty() {
                return type_id;
            }
            return self.types.intern(Type::Applied(type_id, args));
        }
        self.error("unknown qualified type");
        self.types.intern(Type::Error)
    }

    /// Ruling 39(a): `inf` is never a tensor-family element; it has no device
    /// layout. Reports `tensor_element_unbounded` and returns `true` on a hit.
    fn reject_unbounded_element(&mut self, elem_ty: TypeId) -> bool {
        if !crate::numeric_width::is_unbounded_integer_type(self.types, elem_ty) {
            return false;
        }
        self.error(crate::numeric_width::TENSOR_ELEMENT_UNBOUNDED_ISSUE);
        true
    }

    fn lower_tensor_type(&mut self, elem: &TypeExpr, figura: &FiguraExpr) -> TypeId {
        let elem_ty = self.lower_nested_type(elem);
        if self.reject_unbounded_element(elem_ty) {
            return self.types.intern(Type::Error);
        }
        match self.lower_figura_expr(figura) {
            Ok(shape) => {
                if let Some(frame) = self.form_frames.last_mut() {
                    frame.args.push(radix_hir::HirTypeFormArg::Size(shape));
                }
                self.types.tensor_with_shape(elem_ty, shape)
            }
            Err(message) => {
                self.error_from_message(message);
                self.types.intern(Type::Error)
            }
        }
    }

    fn lower_vector_type(&mut self, elem: &TypeExpr, width: &FiguraExpr) -> TypeId {
        let elem_ty = self.lower_nested_type(elem);
        if self.reject_unbounded_element(elem_ty) {
            return self.types.intern(Type::Error);
        }
        match self.lower_vector_width_expr(width) {
            Ok(width_id) => self.types.vector_with_width(elem_ty, width_id),
            Err(message) => {
                self.error_from_message(message);
                self.types.intern(Type::Error)
            }
        }
    }

    fn lower_matrix_type(&mut self, elem: &TypeExpr, shape: &FiguraExpr) -> TypeId {
        let elem_ty = self.lower_nested_type(elem);
        if self.reject_unbounded_element(elem_ty) {
            return self.types.intern(Type::Error);
        }
        match self.lower_matrix_shape_expr(shape) {
            Ok(shape_id) => self.types.matrix_with_shape(elem_ty, shape_id),
            Err(message) => {
                self.error_from_message(message);
                self.types.intern(Type::Error)
            }
        }
    }

    fn lower_sparsa_type(&mut self, elem: &TypeExpr, figura: &FiguraExpr) -> TypeId {
        let elem_ty = self.lower_nested_type(elem);
        if self.reject_unbounded_element(elem_ty) {
            return self.types.intern(Type::Error);
        }
        if !crate::numeric_width::is_tensor_numeric_element(self.types, elem_ty) {
            self.error("sparsa_element_non_numeric");
            return self.types.intern(Type::Error);
        }
        match self.lower_figura_expr(figura) {
            Ok(shape) => self.types.sparsa_with_shape(elem_ty, shape),
            Err(message) => {
                self.error_from_message(message);
                self.types.intern(Type::Error)
            }
        }
    }

    fn lower_atomic_type(&mut self, elem: &TypeExpr) -> TypeId {
        let elem_ty = self.lower_nested_type(elem);
        if !crate::is_atomic_element_type(self.types, elem_ty) {
            self.error("atomic_element");
            return self.types.intern(Type::Error);
        }
        self.types.atomic(elem_ty)
    }

    fn lower_vector_width_expr(&mut self, width: &FiguraExpr) -> Result<IndexId, &'static str> {
        let extent = match width {
            // The unwrapped single dim is still an extent position.
            FiguraExpr::Tuple(dims) if dims.len() == 1 => &dims[0],
            FiguraExpr::Tuple(_) => return Err(crate::VECTOR_WIDTH_SINGLE_DIM_MSG),
            other => other,
        };
        self.figura_extent_nesting += 1;
        let lowered = self.lower_figura_expr(extent);
        self.figura_extent_nesting -= 1;
        lowered
    }

    fn lower_matrix_shape_expr(&mut self, shape: &FiguraExpr) -> Result<IndexId, &'static str> {
        let FiguraExpr::Tuple(dims) = shape else {
            return Err(crate::MATRIX_SHAPE_TWO_DIM_MSG);
        };
        if dims.len() != 2 {
            return Err(crate::MATRIX_SHAPE_TWO_DIM_MSG);
        }
        self.figura_extent_nesting += 1;
        let lowered = (|| {
            let rows = self.lower_figura_expr(&dims[0])?;
            let columns = self.lower_figura_expr(&dims[1])?;
            Ok(self
                .types
                .intern_index(IndexExpr::Tuple(vec![rows, columns])))
        })();
        self.figura_extent_nesting -= 1;
        lowered
    }

    fn lower_figura_expr(&mut self, figura: &FiguraExpr) -> Result<IndexId, &'static str> {
        match figura {
            FiguraExpr::Infer => {
                let var = self.types.fresh_index_infer_var();
                Ok(self.types.intern_index(IndexExpr::Infer(var)))
            }
            FiguraExpr::Literal(value) => Ok(self.types.intern_index(IndexExpr::Literal(*value))),
            FiguraExpr::Var(ident) => match self.lexical_generic_param_kind(ident.name) {
                Some(radix_syntax::GenericParamKind::Magnitudo) => {
                    Ok(self.types.intern_index(IndexExpr::Param(ident.name)))
                }
                Some(radix_syntax::GenericParamKind::Typus) => {
                    Err(crate::FIGURA_TYPE_PARAM_IN_INDEX_MSG)
                }
                None => {
                    // Runtime scalar bindings are valid extent sources in a
                    // type's dimension position. Keep declaration identity;
                    // the backend reads the value at the carrying boundary.
                    let local = self
                        .local_scopes
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(&ident.name).copied());
                    if let Some(binding) = local {
                        // Size-type Z3: a bare value name in an extent
                        // position is the invisible int→size crossing the
                        // same-page test refuses. A declared size stays a
                        // size term, a declared integer list keeps the
                        // whole-shape value reading, a spelled `size` lift is
                        // the spelled store, and a signature clause stands
                        // down for the signature law's own refusal.
                        if self.figura_extent_nesting > 0
                            && self.figura_lift_nesting == 0
                            && self.signature_type_nesting == 0
                            && !self.figura_extent_admissible_locals.contains(&binding)
                        {
                            let name = self.interner.resolve(ident.name).to_owned();
                            self.error_with_args_at(
                                vec![
                                    DiagnosticArg::new(
                                        "issue",
                                        ISSUE_ANNOTATION_EXTENT_BARE_VALUE_NAME,
                                    ),
                                    DiagnosticArg::new("name", name),
                                ],
                                ident.span,
                            );
                        }
                        let source = self.parameter_ordinals.get(&binding).copied().map_or(
                            radix_types::RuntimeExtentId::ScalarBinding(binding),
                            |parameter| radix_types::RuntimeExtentId::ScalarParameter { parameter },
                        );
                        Ok(self.types.intern_index(IndexExpr::Runtime(source)))
                    } else {
                        crate::lower_figura_expr(figura, self.resolver, self.types)
                    }
                }
            },
            FiguraExpr::Lift(expr) => self.lower_figura_lift(expr),
            FiguraExpr::Add(_, _) | FiguraExpr::Subtract(_, _) => {
                self.lower_figura_affine_expr(figura)
            }
            FiguraExpr::Divide(value, divisor) => {
                let numerator = self.lower_figura_expr(value)?;
                divide_index_exact(self.types, numerator, *divisor)
                    .map_err(crate::figura::figura_arithmetic_error_message)
            }
            FiguraExpr::Tuple(dims) => {
                let lowered = dims
                    .iter()
                    .map(|dim| {
                        // Tuple elements are the true extent positions the Z3
                        // refusal guards; the root figura keeps the
                        // whole-shape list reading.
                        self.figura_extent_nesting += 1;
                        let lowered = self.lower_figura_expr(dim);
                        self.figura_extent_nesting -= 1;
                        lowered
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.types.intern_index(IndexExpr::Tuple(lowered)))
            }
        }
    }

    fn lower_figura_lift(
        &mut self,
        extent: &radix_syntax::LiftExtent,
    ) -> Result<IndexId, &'static str> {
        // The spelled lift lowers to the same index term as its payload; the
        // negative-literal refusal already ran at resolve. The payload is the
        // spelled store the same-page test requires, so it keeps its figura
        // admission even where a bare value name is refused (Z3).
        let figura = match extent {
            radix_syntax::LiftExtent::Literal(value) => FiguraExpr::Literal(*value),
            radix_syntax::LiftExtent::Name(ident) => FiguraExpr::Var(ident.clone()),
            radix_syntax::LiftExtent::Paren(term) => (**term).clone(),
            radix_syntax::LiftExtent::Negative { .. } => return Err(FIGURA_LIFT_PRIMARY_MSG),
            radix_syntax::LiftExtent::Shape(_) => return Err(crate::figura::FIGURA_LIFT_SHAPE_MSG),
        };
        self.figura_lift_nesting += 1;
        let lowered = self.lower_figura_expr(&figura);
        self.figura_lift_nesting -= 1;
        lowered
    }

    fn lower_figura_affine_expr(&mut self, figura: &FiguraExpr) -> Result<IndexId, &'static str> {
        let mut offset = 0i128;
        let base_expr = crate::figura::flatten_figura_affine(figura, &mut offset)
            .map_err(crate::figura::figura_arithmetic_error_message)?;
        let base = self.lower_figura_expr(base_expr)?;
        let term = compose_index_affine_constant(self.types, base, offset)
            .map_err(crate::figura::figura_arithmetic_error_message)?;
        match evaluate_index_dimension(term, self.types)
            .map_err(crate::figura::figura_arithmetic_error_message)?
        {
            Some(value) => Ok(self.types.intern_index(IndexExpr::Literal(value))),
            None => Ok(term),
        }
    }
}
