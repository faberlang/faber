//! Generic type-parameter instantiation at call sites.
//!
//! Function and method signatures may still contain unresolved
//! [`Type::Param`] nodes after collection. Bodies typecheck those params as
//! rigid type variables; call sites substitute each distinct param symbol with a
//! fresh [`Type::Infer`] so argument checking can solve witnesses per call.
//!
//! INVARIANTS
//! ==========
//! - One fresh infer per distinct param symbol within a single call-site
//!   instantiation (repeated `T` in params/ret share one cell).
//! - Generic bodies keep rigid `Type::Param`; only callee signatures at the
//!   call boundary are rewritten.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use crate::format_faber_type;
use crate::IndexId;
use radix_hir::HirCallArg;

const ISSUE_GENERIC_TYPE_ARG_DOMAIN: &str = "generic_type_arg_domain";
const ISSUE_EXPRESSION_TYPE_MISMATCH: &str = "expression_type_mismatch";

/// Diagnostic issue for a shape param left unbound at a call site (D6).
///
/// Reuses the existing `cannot_infer_tensor_shape` issue (SEM014): a shape
/// param determined by neither arguments nor the expected type is exactly a
/// missing tensor shape witness, so it shares the code the goal text pins.
const ISSUE_CANNOT_INFER_TENSOR_SHAPE: &str = "cannot_infer_tensor_shape";

impl TypeChecker<'_> {
    /// Typecheck arguments against a signature with generic params instantiated.
    pub fn check_instantiated_call(
        &mut self,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        // Magnitudo shape params are recognized by symbol identity: they lower
        // exclusively into `IndexExpr::Param` positions inside tensor/vector/
        // matrix/sparsa shapes, so the shape-param set is derived from the
        // signature rather than carried as a `FuncSig` field.
        let shape_symbols = self.collect_shape_param_symbols(sig);
        let (sig, inferred_vars, shape_vars) = if explicit_type_args.is_empty() {
            let mut param_vars = FxHashMap::default();
            let mut shape_vars = self.shape_vars_for(sig, &shape_symbols);
            let sig = self.substitute_func_sig(sig, &mut param_vars, Some(&mut shape_vars));
            (sig, Some(param_vars), shape_vars)
        } else if let Some(error_ty) = self.reject_invalid_explicit_type_args(
            sig,
            &shape_symbols,
            explicit_type_args,
            args,
            span,
        ) {
            return error_ty;
        } else if self.reject_type_arg_domain_misses(sig, explicit_type_args, span) {
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        } else {
            let mut param_vars = FxHashMap::default();
            for (sym, ty) in sig
                .type_params
                .iter()
                .filter(|sym| !shape_symbols.contains(sym))
                .zip(explicit_type_args.iter())
            {
                param_vars.insert(*sym, *ty);
            }
            let mut shape_vars = self.shape_vars_for(sig, &shape_symbols);
            self.bind_explicit_magnitudo_args(sig, explicit_type_args, &mut shape_vars);
            (
                self.substitute_func_sig(sig, &mut param_vars, Some(&mut shape_vars)),
                Some(param_vars),
                shape_vars,
            )
        };

        self.check_call_args(&sig, args, span);
        if let Some(expected) = expected {
            self.unify_carrier_assign(sig.ret, expected, span, ISSUE_EXPRESSION_TYPE_MISMATCH);
        }
        // D6: a shape param bound by neither arguments nor the expected type is
        // an explicit error (SEM014), never a silent guess.
        if self.reject_unbound_shape_params(&shape_vars, span) {
            return self.error_type;
        }
        if let Some(param_vars) = inferred_vars {
            if self.reject_inferred_type_arg_domain_misses(&sig, &param_vars, span) {
                return self.error_type;
            }
        }
        if self.reject_failable_call(&sig, span) {
            return self.error_type;
        }
        self.resolve_inferred_type(sig.ret)
    }

    /// Returns the shared error type when explicit args are invalid for `sig`.
    ///
    /// A NATURAL-only list whose length matches the full param list binds
    /// magnitudo params. Otherwise arity counts Typus params; shape params
    /// then bind from argument/expected shapes (D1).
    fn reject_invalid_explicit_type_args(
        &mut self,
        sig: &FuncSig,
        shape_symbols: &FxHashSet<Symbol>,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        debug_assert!(!explicit_type_args.is_empty());

        if self.explicit_args_are_magnitudo_list(explicit_type_args) {
            if explicit_type_args.len() == sig.type_params.len() {
                return None;
            }
            self.wrong_arity("wrong_number_of_type_arguments", span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }

        let type_param_count = sig
            .type_params
            .iter()
            .filter(|sym| !shape_symbols.contains(sym))
            .count();
        if type_param_count == 0 {
            self.wrong_arity("callee_type_args_unsupported", span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }
        if explicit_type_args.len() != type_param_count {
            self.wrong_arity("wrong_number_of_type_arguments", span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }
        if explicit_type_args
            .iter()
            .any(|ty| self.infer_union_var_of(*ty).is_some())
        {
            // PH-1: `∪` already lowers, but an explicit type-arg slot is a
            // monomorphic param witness. Accumulation does not solve the
            // callee param, so reject with a named issue rather than leaking
            // an unsolved InferUnion as cannot_infer_variable_type.
            self.invalid_operand_types("explicit_union_type_arg_unsupported", span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }
        None
    }

    fn reject_type_arg_domain_misses(
        &mut self,
        sig: &FuncSig,
        witnesses: &[TypeId],
        span: radix_lexer::Span,
    ) -> bool {
        let mut rejected = false;
        for ((param, constraint), witness) in sig
            .type_params
            .iter()
            .zip(sig.type_param_constraints.iter())
            .zip(witnesses.iter())
        {
            if self.type_arg_rejects_domain(*param, constraint, *witness, span) {
                rejected = true;
            }
        }
        rejected
    }

    fn reject_inferred_type_arg_domain_misses(
        &mut self,
        sig: &FuncSig,
        param_vars: &FxHashMap<Symbol, TypeId>,
        span: radix_lexer::Span,
    ) -> bool {
        let mut rejected = false;
        for (param, constraint) in sig
            .type_params
            .iter()
            .zip(sig.type_param_constraints.iter())
        {
            let Some(witness) = param_vars.get(param).copied() else {
                continue;
            };
            if self.is_infer(self.resolve_inferred_type(witness)) {
                continue;
            }
            if self.type_arg_rejects_domain(*param, constraint, witness, span) {
                rejected = true;
            }
        }
        rejected
    }

    fn type_arg_rejects_domain(
        &mut self,
        param: Symbol,
        constraint: &TypeParamConstraint,
        witness: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        let TypeParamConstraint::OneOf(allowed) = constraint else {
            return false;
        };
        let witness = self.resolve_inferred_type(witness);
        if self.is_infer(witness) || self.infer_union_var_of(witness).is_some() {
            return false;
        }
        if allowed
            .iter()
            .any(|allowed_ty| self.types.equals(*allowed_ty, witness))
        {
            return false;
        }

        let rejected = self.format_type_for_diagnostic(witness);
        let accepted = allowed
            .iter()
            .map(|allowed_ty| self.format_type_for_diagnostic(*allowed_ty))
            .collect::<Vec<_>>()
            .join(", ");
        let param_name = self.interner.map_or_else(
            || "T".to_owned(),
            |interner| interner.resolve(param).to_owned(),
        );
        self.invalid_operand_types_with_args(
            ISSUE_GENERIC_TYPE_ARG_DOMAIN,
            span,
            &[
                ("rejected", rejected),
                ("param", param_name),
                ("accepted", accepted),
            ],
        );
        true
    }

    fn format_type_for_diagnostic(&self, ty: TypeId) -> String {
        let Some(interner) = self.interner else {
            return format!("{:?}", self.types.get(ty));
        };
        format_faber_type(ty, self.types, interner, &|def_id| {
            format!("Def{}", def_id.0)
        })
    }

    pub fn substitute_func_sig(
        &mut self,
        sig: &FuncSig,
        vars: &mut FxHashMap<Symbol, TypeId>,
        mut shape_vars: Option<&mut FxHashMap<Symbol, IndexId>>,
    ) -> FuncSig {
        FuncSig {
            type_params: sig.type_params.clone(),
            type_param_constraints: sig.type_param_constraints.clone(),
            params: sig
                .params
                .iter()
                .map(|param| ParamType {
                    ty: self.substitute_type_params(param.ty, vars, shape_vars.as_deref_mut()),
                    mode: param.mode,
                    optional: param.optional,
                })
                .collect(),
            ret: self.substitute_type_params(sig.ret, vars, shape_vars.as_deref_mut()),
            err: sig
                .err
                .map(|err| self.substitute_type_params(err, vars, shape_vars)),
            is_async: sig.is_async,
            is_generator: sig.is_generator,
        }
    }

    pub fn substitute_type_params(
        &mut self,
        ty: TypeId,
        vars: &mut FxHashMap<Symbol, TypeId>,
        mut shape_vars: Option<&mut FxHashMap<Symbol, IndexId>>,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved).clone() {
            Type::Param(sym) => *vars.entry(sym).or_insert_with(|| self.fresh_infer()),
            Type::Array(inner) => {
                self.substitute_unary_type(inner, vars, shape_vars, |this, inner| {
                    this.types.array(inner)
                })
            }
            Type::Map(key, value) => {
                let key = self.substitute_type_params(key, vars, shape_vars.as_deref_mut());
                let value = self.substitute_type_params(value, vars, shape_vars.as_deref_mut());
                self.types.map(key, value)
            }
            Type::Set(inner) => {
                self.substitute_unary_type(inner, vars, shape_vars, |this, inner| {
                    this.types.set(inner)
                })
            }
            Type::Option(inner) => {
                self.substitute_unary_type(inner, vars, shape_vars, |this, inner| {
                    this.types.option(inner)
                })
            }
            Type::Promissum(inner) => {
                self.substitute_unary_type(inner, vars, shape_vars, |this, inner| {
                    this.types.promissum(inner)
                })
            }
            Type::PromissumFailable(success, alternate) => {
                let success = self.substitute_type_params(success, vars, shape_vars.as_deref_mut());
                let alternate =
                    self.substitute_type_params(alternate, vars, shape_vars.as_deref_mut());
                self.types.promissum_failable(success, alternate)
            }
            Type::Cursor(inner) => {
                self.substitute_unary_type(inner, vars, shape_vars, |this, inner| {
                    this.types.cursor(inner)
                })
            }
            Type::AsyncCursor(item, alternate) => {
                let item = self.substitute_type_params(item, vars, shape_vars.as_deref_mut());
                let alternate =
                    self.substitute_type_params(alternate, vars, shape_vars.as_deref_mut());
                self.types.async_cursor(item, alternate)
            }
            Type::Intervallum(inner) => {
                self.substitute_unary_type(inner, vars, shape_vars, |this, inner| {
                    this.types.intervallum(inner)
                })
            }
            Type::Ref(mode, inner) => {
                let inner = self.substitute_type_params(inner, vars, shape_vars.as_deref_mut());
                self.types.reference(mode, inner)
            }
            Type::Tensor(elem, shape) => {
                let element = self.substitute_type_params(elem, vars, shape_vars.as_deref_mut());
                let shape = self.substitute_index_params(shape, shape_vars.as_deref_mut());
                self.types.tensor_with_shape(element, shape)
            }
            Type::Vector(elem, width) => {
                let element = self.substitute_type_params(elem, vars, shape_vars.as_deref_mut());
                let width = self.substitute_index_params(width, shape_vars.as_deref_mut());
                self.types.vector_with_width(element, width)
            }
            Type::Matrix(elem, shape) => {
                let element = self.substitute_type_params(elem, vars, shape_vars.as_deref_mut());
                let shape = self.substitute_index_params(shape, shape_vars.as_deref_mut());
                self.types.matrix_with_shape(element, shape)
            }
            Type::Sparsa(elem, shape) => {
                let element = self.substitute_type_params(elem, vars, shape_vars.as_deref_mut());
                let shape = self.substitute_index_params(shape, shape_vars.as_deref_mut());
                self.types.sparsa_with_shape(element, shape)
            }
            Type::BoundedAscii { capacity } => {
                let capacity = self.substitute_index_params(capacity, shape_vars.as_deref_mut());
                self.types.bounded_ascii(capacity)
            }
            Type::BoundedTextus { capacity } => {
                let capacity = self.substitute_index_params(capacity, shape_vars.as_deref_mut());
                self.types.bounded_textus(capacity)
            }
            Type::BoundedOcteti { capacity } => {
                let capacity = self.substitute_index_params(capacity, shape_vars.as_deref_mut());
                self.types.bounded_octeti(capacity)
            }
            Type::BoundedArray { element, capacity } => {
                let element = self.substitute_type_params(element, vars, shape_vars.as_deref_mut());
                let capacity = self.substitute_index_params(capacity, shape_vars.as_deref_mut());
                self.types.bounded_array(element, capacity)
            }
            Type::Func(sig) => {
                let substituted = self.substitute_func_sig(&sig, vars, shape_vars.as_deref_mut());
                self.types.function(substituted)
            }
            Type::Applied(base, args) => {
                let base = self.substitute_type_params(base, vars, shape_vars.as_deref_mut());
                let args = args
                    .iter()
                    .map(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => radix_types::AppliedArg::Type(
                            self.substitute_type_params(*id, vars, shape_vars.as_deref_mut()),
                        ),
                        radix_types::AppliedArg::Size(id) => radix_types::AppliedArg::Size(
                            self.substitute_index_params(*id, shape_vars.as_deref_mut()),
                        ),
                    })
                    .collect();
                self.types.intern(Type::Applied(base, args))
            }
            Type::Union(types) => {
                let types = types
                    .iter()
                    .map(|member| {
                        self.substitute_type_params(*member, vars, shape_vars.as_deref_mut())
                    })
                    .collect();
                self.types.intern(Type::Union(types))
            }
            Type::Alias(def, inner) => {
                let inner = self.substitute_type_params(inner, vars, shape_vars);
                self.types.intern(Type::Alias(def, inner))
            }
            _ => ty,
        }
    }

    pub fn substitute_unary_type(
        &mut self,
        inner: TypeId,
        vars: &mut FxHashMap<Symbol, TypeId>,
        shape_vars: Option<&mut FxHashMap<Symbol, IndexId>>,
        build: impl FnOnce(&mut Self, TypeId) -> TypeId,
    ) -> TypeId {
        let inner = self.substitute_type_params(inner, vars, shape_vars);
        build(self, inner)
    }

    /// Substitute `IndexExpr::Param` shape nodes using `shape_vars`.
    ///
    /// Symbols absent from `shape_vars` keep their rigid `Param` (they are not
    /// this function's shape params). `None` leaves the shape untouched, which
    /// is the non-generic-call path.
    fn substitute_index_params(
        &mut self,
        id: IndexId,
        shape_vars: Option<&mut FxHashMap<Symbol, IndexId>>,
    ) -> IndexId {
        let Some(shape_vars) = shape_vars else {
            return id;
        };
        match self.types.get_index(id).clone() {
            IndexExpr::Param(sym) => shape_vars.get(&sym).copied().unwrap_or(id),
            IndexExpr::Tuple(dims) => {
                let dims = dims
                    .iter()
                    .map(|dim| self.substitute_index_params(*dim, Some(shape_vars)))
                    .collect();
                self.types.intern_index(IndexExpr::Tuple(dims))
            }
            other => self.types.intern_index(other),
        }
    }

    /// Collect the distinct shape-param symbols of `sig`.
    ///
    /// Magnitudo params lower only into `IndexExpr::Param` positions, so the
    /// symbols found in tensor/vector/matrix/sparsa shapes are exactly the
    /// shape params — no separate kind list is needed.
    fn collect_shape_param_symbols(&self, sig: &FuncSig) -> FxHashSet<Symbol> {
        let mut symbols = FxHashSet::default();
        for param in &sig.params {
            self.collect_shape_params_in_type(param.ty, &mut symbols);
        }
        self.collect_shape_params_in_type(sig.ret, &mut symbols);
        if let Some(err) = sig.err {
            self.collect_shape_params_in_type(err, &mut symbols);
        }
        symbols
    }

    fn magnitudo_literal_of(&self, ty: TypeId) -> Option<u64> {
        let Type::Param(sym) = self.types.get(ty) else {
            return None;
        };
        let name = self.interner?.resolve(*sym);
        radix_types::parse_natural_capacity(name)
    }

    fn explicit_args_are_magnitudo_list(&self, explicit_type_args: &[TypeId]) -> bool {
        !explicit_type_args.is_empty()
            && explicit_type_args
                .iter()
                .all(|ty| self.magnitudo_literal_of(*ty).is_some())
    }

    fn bind_explicit_magnitudo_args(
        &mut self,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        shape_vars: &mut FxHashMap<Symbol, crate::IndexId>,
    ) {
        for (sym, ty) in sig.type_params.iter().zip(explicit_type_args.iter()) {
            if let Some(value) = self.magnitudo_literal_of(*ty) {
                let id = self.types.intern_index(IndexExpr::Literal(value));
                shape_vars.insert(*sym, id);
            }
        }
    }

    fn collect_shape_params_in_type(&self, ty: TypeId, out: &mut FxHashSet<Symbol>) {
        match self.types.get(ty).clone() {
            Type::Tensor(_, shape)
            | Type::Vector(_, shape)
            | Type::Matrix(_, shape)
            | Type::Sparsa(_, shape)
            | Type::BoundedAscii { capacity: shape }
            | Type::BoundedTextus { capacity: shape }
            | Type::BoundedOcteti { capacity: shape } => {
                self.collect_shape_params_in_index(shape, out);
            }
            Type::BoundedArray { element, capacity } => {
                self.collect_shape_params_in_type(element, out);
                self.collect_shape_params_in_index(capacity, out);
            }
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Option(inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Intervallum(inner)
            | Type::Ref(_, inner)
            | Type::Alias(_, inner) => self.collect_shape_params_in_type(inner, out),
            Type::Map(key, value)
            | Type::PromissumFailable(key, value)
            | Type::AsyncCursor(key, value) => {
                self.collect_shape_params_in_type(key, out);
                self.collect_shape_params_in_type(value, out);
            }
            Type::Record(fields) => {
                for field in fields.values() {
                    self.collect_shape_params_in_type(*field, out);
                }
            }
            Type::Tuple(elements) => {
                for element in elements {
                    self.collect_shape_params_in_type(element, out);
                }
            }
            Type::Union(members) => {
                for member in members {
                    self.collect_shape_params_in_type(member, out);
                }
            }
            Type::Func(sig) => {
                for param in &sig.params {
                    self.collect_shape_params_in_type(param.ty, out);
                }
                self.collect_shape_params_in_type(sig.ret, out);
                if let Some(err) = sig.err {
                    self.collect_shape_params_in_type(err, out);
                }
            }
            Type::Applied(base, args) => {
                self.collect_shape_params_in_type(base, out);
                for arg in args {
                    match arg {
                        radix_types::AppliedArg::Type(id) => {
                            self.collect_shape_params_in_type(id, out);
                        }
                        radix_types::AppliedArg::Size(id) => {
                            self.collect_shape_params_in_index(id, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn collect_shape_params_in_index(&self, id: IndexId, out: &mut FxHashSet<Symbol>) {
        match self.types.get_index(id) {
            IndexExpr::Param(sym) => {
                out.insert(*sym);
            }
            IndexExpr::Tuple(dims) => {
                for dim in dims {
                    self.collect_shape_params_in_index(*dim, out);
                }
            }
            _ => {}
        }
    }

    /// Allocate one fresh index hole per distinct shape-param symbol (D1).
    fn shape_vars_for(
        &mut self,
        _sig: &FuncSig,
        shape_symbols: &FxHashSet<Symbol>,
    ) -> FxHashMap<Symbol, IndexId> {
        shape_symbols
            .iter()
            .map(|sym| (*sym, self.fresh_index_hole()))
            .collect()
    }

    /// D6: reject a call whose shape param was bound by neither arguments nor
    /// the expected type. Returns `true` when a diagnostic was emitted.
    fn reject_unbound_shape_params(
        &mut self,
        shape_vars: &FxHashMap<Symbol, IndexId>,
        span: radix_lexer::Span,
    ) -> bool {
        let mut rejected = false;
        for (sym, hole) in shape_vars {
            let resolved = self.resolve_index(*hole);
            let IndexExpr::Infer(var) = self.types.get_index(resolved).clone() else {
                continue;
            };
            // Bound through a constraint chain (even to a hole the caller owns):
            // the caller's own annotation reports the remaining gap.
            if self.index_infer.substitutions().contains_key(&var) {
                continue;
            }
            let param_name = self.interner.map_or_else(
                || "shape".to_owned(),
                |interner| interner.resolve(*sym).to_owned(),
            );
            let error = SemanticError::structured(SemanticErrorKind::TypeAnnotationAbsent, span)
                .with_arg("issue", ISSUE_CANNOT_INFER_TENSOR_SHAPE)
                .with_arg("param", param_name);
            self.errors.push(error);
            rejected = true;
        }
        rejected
    }
}
