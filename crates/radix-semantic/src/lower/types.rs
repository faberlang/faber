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
    CollectionKind, FuncSig, IndexExpr, IndexId, InferVar, ParamType, Primitive, SemanticParamMode,
    Type, TypeId,
};
use radix_syntax::{FiguraExpr, GenericArgExpr, GenericParamKind, Ident, TypeExpr, TypeExprKind};
use radix_types::AppliedArg;

fn generic_type_expr(arg: &GenericArgExpr) -> Option<TypeExpr> {
    match arg {
        GenericArgExpr::Type(ty) => Some(ty.clone()),
        GenericArgExpr::Name(name) => Some(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(name.clone(), Vec::new()),
            span: name.span,
            iuncta_label: None,
        }),
        GenericArgExpr::Size(_) => None,
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
        self.lower_type_with_context(ty, false)
    }

    /// Lower a type written in a parameter slot.
    ///
    /// `ref`/`mut` on scalar parameters are intentional ABI markers, while
    /// `own`/`copy` remain rejected on scalars and all markers remain rejected
    /// in ordinary binding/type positions.
    pub(super) fn lower_parameter_type(&mut self, ty: &TypeExpr) -> TypeId {
        self.lower_type_with_context(ty, true)
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

    fn lower_type_with_context(&mut self, ty: &TypeExpr, allow_scalar_ref_mut: bool) -> TypeId {
        self.current_span = ty.span;

        let mut ty_id = match &ty.kind {
            TypeExprKind::Infer => {
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
            TypeExprKind::Func(func) => {
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
                        }
                    })
                    .collect();
                let ret = self.lower_nested_type(&func.ret);
                let err = func.err.as_ref().map(|err| self.lower_nested_type(err));
                self.types.function(FuncSig {
                    type_params: Vec::new(),
                    type_param_constraints: Vec::new(),
                    params,
                    ret,
                    err,
                    is_async: false,
                    is_generator: false,
                })
            }
            TypeExprKind::Union(members) => self.lower_union_type(members),
        };

        if let Some(mode) = ty.mode {
            let marker = crate::access_marker_from_type_mode(mode);
            if self.types.is_scalar_for_access_marker(ty_id)
                && !(allow_scalar_ref_mut
                    && matches!(
                        mode,
                        radix_syntax::TypeMode::De | radix_syntax::TypeMode::In
                    ))
            {
                self.error(crate::ISSUE_SCALAR_ACCESS_MARKER);
                ty_id = self.types.intern(Type::Error);
            } else {
                ty_id = self.types.access_marker(marker, ty_id);
            }
        }

        if ty.nullable {
            ty_id = self.types.option(ty_id);
        }

        ty_id
    }

    /// Normalize a source union into the canonical semantic union/option shape.
    ///
    /// `T ∪ nihil` is the source spelling for nullable value types. HIR stores
    /// that as `Option<T>` so typecheck and codegen have one nullability shape
    /// to reason about, while non-null unions remain explicit `Type::Union`
    /// values after duplicate members are removed.
    fn lower_union_type(&mut self, members: &[TypeExpr]) -> TypeId {
        if members.is_empty() {
            self.error("empty_union_type");
            return self.types.intern(Type::Error);
        }

        let mut cleaned: Vec<TypeId> = Vec::with_capacity(members.len());
        let mut had_nihil = false;

        for member in members {
            let lowered = self.lower_nested_type(member);
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
            return self.types.intern(Type::Error);
        }

        let base = match cleaned.as_slice() {
            [only] => *only,
            many => self.types.intern(Type::Union(many.to_vec())),
        };

        if had_nihil {
            self.types.option(base)
        } else {
            base
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
        }
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
            None => match self
                .resolver
                .lookup(ident.name)
                .and_then(|def_id| self.resolver.get_symbol(def_id).map(|symbol| symbol.kind))
            {
                Some(crate::SymbolKind::IndexParam) => {
                    Some(self.types.intern_index(IndexExpr::Param(ident.name)))
                }
                _ => {
                    self.error("generic_type_argument_in_size_position");
                    None
                }
            },
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
                let is_index = self
                    .resolver
                    .lookup(ident.name)
                    .and_then(|def_id| self.resolver.get_symbol(def_id))
                    .is_some_and(|symbol| symbol.kind == crate::SymbolKind::IndexParam);
                if is_index {
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
        }
    }

    fn lower_declared_applied_args(
        &mut self,
        def_id: crate::DefId,
        params: &[GenericArgExpr],
    ) -> Vec<AppliedArg> {
        let kinds = self
            .resolver
            .generic_param_kinds(def_id)
            .map(ToOwned::to_owned);
        self.lower_applied_args_directed(kinds.as_deref(), params)
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
                    GenericArgExpr::Name(ident)
                        if self
                            .resolver
                            .lookup(ident.name)
                            .and_then(|id| self.resolver.get_symbol(id))
                            .is_some_and(|symbol| symbol.kind == crate::SymbolKind::IndexParam) =>
                    {
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
                        if !self
                            .resolver
                            .lookup(ident.name)
                            .and_then(|id| self.resolver.get_symbol(id))
                            .is_some_and(|symbol| symbol.kind == crate::SymbolKind::IndexParam)
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
            || self
                .resolver
                .lookup(ident.name)
                .and_then(|id| self.resolver.get_symbol(id))
                .is_some_and(|symbol| symbol.kind == crate::SymbolKind::IndexParam)
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

        if let Some(ty) = self.lower_locale_pack_type(name_str, params) {
            return ty;
        }

        if let Some(prim) = Primitive::from_name(name_str) {
            return self.lower_primitive_type(prim, params);
        }

        if name_str == "modulus" {
            return self.lower_modular_word_type(params);
        }

        // WHY: `octet` is the canonical byte scalar spelling — an alternative
        // name for `numerus<u8>`, symmetric with `octeti` (the byte buffer,
        // which unifies with `lista<numerus<u8>>`). It rejects width params
        // because the width is fixed by definition. See DEFER-029.
        if name_str == "octet" {
            if !params.is_empty() {
                self.error("octet_params");
                return self.types.intern(Type::Error);
            }
            return self
                .types
                .sized_numeric(Primitive::Numerus, crate::NumericWidth::U8);
        }

        if crate::numeric_width::standalone_width_type(name_str).is_some() {
            let Some(type_params) = generic_type_exprs(params) else {
                self.error("generic_size_argument_in_type_position");
                return self.types.intern(Type::Error);
            };
            let mut messages = Vec::new();
            let Some(ty) = crate::numeric_width::lower_standalone_width_type(
                name_str,
                &type_params,
                self.types,
                &mut |message| messages.push(message.to_owned()),
            ) else {
                self.error("standalone_width_internal");
                return self.types.intern(Type::Error);
            };
            if let Some(message) = messages.first() {
                self.error_from_message(message);
            }
            return ty;
        }

        // SQL row: dynamic column map (faber::Valor per column).
        if name_str == "series" {
            if !params.is_empty() {
                self.error("series_params");
                return self.types.intern(Type::Error);
            }
            let textus = self.types.primitive(Primitive::Textus);
            let valor = self.types.primitive(Primitive::Valor);
            return self.types.map(textus, valor);
        }

        if name_str == "intervallum" {
            if params.len() != 1 {
                self.error("intervallum_arity");
                return self.types.intern(Type::Error);
            }
            let bound = self.lower_generic_type_arg(&params[0]);
            if !crate::is_intervallum_bound_type(self.types, bound) {
                self.error("intervallum_bound");
                return self.types.intern(Type::Error);
            }
            return self.types.intervallum(bound);
        }

        if name_str == "scrinium" {
            let Some(def_id) = crate::builtins::frame_types::scrinium_def_id(self.resolver) else {
                self.error("scrinium_missing_builtin");
                return self.types.intern(Type::Error);
            };
            let payload_ty = match params {
                [] => self.types.primitive(Primitive::Valor),
                [payload] => self.lower_generic_type_arg(payload),
                _ => {
                    self.error("scrinium_arity");
                    return self.types.intern(Type::Error);
                }
            };
            return crate::builtins::frame_types::scrinium_payload_type_id(
                self.types, def_id, payload_ty,
            );
        }

        if name_str == "atomic" {
            if params.len() == 1 {
                self.error("atomic_compiler_owned_syntax");
            } else {
                self.error("atomic_arity");
            }
            return self.types.intern(Type::Error);
        }

        if name_str == "tensor" {
            if params.is_empty() {
                self.error("tensor_incomplete");
            } else {
                self.error("tensor_missing_figura");
            }
            return self.types.intern(Type::Error);
        }

        if name_str == "sparsa" {
            if params.is_empty() {
                self.error("sparsa_incomplete");
            } else {
                self.error("sparsa_missing_figura");
            }
            return self.types.intern(Type::Error);
        }

        if name_str == "matrix" {
            if params.is_empty() {
                self.error("matrix_incomplete");
            } else {
                self.error("matrix_missing_figura");
            }
            return self.types.intern(Type::Error);
        }

        if name_str == "iuncta" {
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
            return self.types.tuple(elements);
        }

        if name_str == "ratio" {
            return self.lower_ratio_type(params);
        }

        if let Some(collection) = CollectionKind::from_name(name_str) {
            return self.lower_collection_type(collection, params);
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
            self.error("unknown_type_name");
            return self.types.intern(Type::Error);
        };

        let Some(symbol) = self.resolver.get_symbol(def_id) else {
            self.error("missing_type_symbol_info");
            return self.types.intern(Type::Error);
        };

        let base = match symbol.kind {
            crate::SymbolKind::Struct => self.types.intern(Type::Struct(def_id)),
            crate::SymbolKind::Enum => self.types.intern(Type::Enum(def_id)),
            crate::SymbolKind::Interface => self.types.intern(Type::Interface(def_id)),
            crate::SymbolKind::TypeAlias => {
                if let Some(resolved) = symbol.ty {
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

        if params.is_empty() {
            return base;
        }

        let args = self.lower_declared_applied_args(def_id, params);
        self.types.intern(Type::Applied(base, args))
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
            // The tensor family lower through dedicated shape/width syntax
            // (`TypeExprKind::Tensor(elem, figura)` etc.), never through a flat
            // type-argument list. Fall through to the hardcoded name paths in
            // `lower_named_type`, which report `tensor_incomplete` etc. for
            // bare (incomplete) family spellings.
            crate::locale::TypeBindingKind::TensorFamily(_) => None,
            crate::locale::TypeBindingKind::ModularWord => {
                Some(self.lower_modular_word_type(params))
            }
            // Census relational type names are reader-surface rows only in S2
            // (no HIR/MIR type yet; the real surface is S3/S4). Fall through to
            // the hardcoded name paths in `lower_named_type` (bare `series`
            // keeps its dynamic row-map special case).
            crate::locale::TypeBindingKind::Relational(_) => None,
            crate::locale::TypeBindingKind::Ratio => Some(self.lower_ratio_type(params)),
        }
    }

    fn lower_primitive_type(&mut self, prim: Primitive, params: &[GenericArgExpr]) -> TypeId {
        // Bounded textus/ascii/octeti capacities are index arguments, not type
        // arguments. Lower this family before the width/type-only helper so a
        // literal capacity reaches the shared IndexExpr arena.
        if matches!(
            prim,
            Primitive::Ascii | Primitive::Textus | Primitive::Octeti
        ) {
            return self.lower_bounded_primitive(prim, params);
        }
        let Some(type_params) = generic_type_exprs(params) else {
            self.error("generic_size_argument_in_type_position");
            return self.types.intern(Type::Error);
        };
        if matches!(prim, Primitive::Numerus | Primitive::Fractus) {
            let mut messages = Vec::new();
            let ty = crate::numeric_width::lower_sized_primitive(
                prim,
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

    /// Conversio `n ↦ ascii<N, Hex|Bin|Oct>` peels the format hint first, then
    /// lowers capacity-only `ascii<N>` through this helper.
    pub(crate) fn lower_bounded_ascii_capacity(&mut self, capacity: &GenericArgExpr) -> TypeId {
        self.lower_bounded_primitive(Primitive::Ascii, std::slice::from_ref(capacity))
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
                    Primitive::Octeti => self.types.bounded_octeti(capacity),
                    _ => unreachable!("lower_bounded_primitive only handles ascii/textus/octeti"),
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
        GenericArgExpr::Size(_) => false,
    }
}

impl Lowerer<'_> {
    fn lower_modular_word_type(&mut self, params: &[GenericArgExpr]) -> TypeId {
        let Some(type_params) = generic_type_exprs(params) else {
            self.error("generic_size_argument_in_type_position");
            return self.types.intern(Type::Error);
        };
        let mut messages = Vec::new();
        let ty = crate::numeric_width::lower_modular_word(
            &type_params,
            self.interner,
            self.types,
            &mut |message| {
                messages.push(message.to_owned());
            },
        );
        if let Some(message) = messages.first() {
            self.error_from_message(message);
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
                    _ => unreachable!("queue/stack helper called for another collection"),
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
                    _ => unreachable!("queue/stack helper called for another collection"),
                }
            }
            _ => {
                self.error_from_message(collection.arity_error());
                self.types.intern(Type::Error)
            }
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
            if params.is_empty() {
                return type_id;
            }
            // Imported declarations carry their ordered declaration kinds on
            // the file interface (S0-R1); kind mismatches fail closed instead
            // of silently accepting any argument spelling.
            let decl_kinds = self
                .resolver
                .namespace_file_decl_param_kinds(namespace.name, type_name.name);
            let args = self.lower_applied_args_directed(decl_kinds.as_deref(), params);
            return self.types.intern(Type::Applied(type_id, args));
        }
        if self
            .resolver
            .namespace_exports_member(namespace.name, type_name.name)
        {
            if let Some(type_id) = self
                .resolver
                .lookup_type(type_name.name)
                .and_then(|def_id| self.resolver.get_symbol(def_id))
                .and_then(|symbol| symbol.ty)
            {
                if params.is_empty() {
                    return type_id;
                }
                let args = params
                    .iter()
                    .map(|param| self.lower_applied_arg(param))
                    .collect();
                return self.types.intern(Type::Applied(type_id, args));
            }
        }
        self.error("unknown qualified type");
        self.types.intern(Type::Error)
    }

    fn lower_tensor_type(&mut self, elem: &TypeExpr, figura: &FiguraExpr) -> TypeId {
        let elem_ty = self.lower_nested_type(elem);
        match self.lower_figura_expr(figura) {
            Ok(shape) => self.types.tensor_with_shape(elem_ty, shape),
            Err(message) => {
                self.error_from_message(message);
                self.types.intern(Type::Error)
            }
        }
    }

    fn lower_vector_type(&mut self, elem: &TypeExpr, width: &FiguraExpr) -> TypeId {
        let elem_ty = self.lower_nested_type(elem);
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
        match width {
            FiguraExpr::Tuple(dims) if dims.len() == 1 => self.lower_figura_expr(&dims[0]),
            FiguraExpr::Tuple(_) => Err(crate::VECTOR_WIDTH_SINGLE_DIM_MSG),
            other => self.lower_figura_expr(other),
        }
    }

    fn lower_matrix_shape_expr(&mut self, shape: &FiguraExpr) -> Result<IndexId, &'static str> {
        let FiguraExpr::Tuple(dims) = shape else {
            return Err(crate::MATRIX_SHAPE_TWO_DIM_MSG);
        };
        if dims.len() != 2 {
            return Err(crate::MATRIX_SHAPE_TWO_DIM_MSG);
        }
        let rows = self.lower_figura_expr(&dims[0])?;
        let columns = self.lower_figura_expr(&dims[1])?;
        Ok(self
            .types
            .intern_index(IndexExpr::Tuple(vec![rows, columns])))
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
                None => crate::lower_figura_expr(figura, self.resolver, self.types),
            },
            FiguraExpr::Tuple(dims) => {
                let lowered = dims
                    .iter()
                    .map(|dim| self.lower_figura_expr(dim))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.types.intern_index(IndexExpr::Tuple(lowered)))
            }
        }
    }
}
