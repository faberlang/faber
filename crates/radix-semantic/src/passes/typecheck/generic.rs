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
use crate::IndexId;
use crate::format_faber_type;
use radix_hir::HirCallArg;
use radix_types::{add_index_constant, divide_index_exact};

const ISSUE_GENERIC_TYPE_ARG_DOMAIN: &str = "generic_type_arg_domain";
const ISSUE_EXPRESSION_TYPE_MISMATCH: &str = "expression_type_mismatch";
/// GPD-3: an explicit generic argument whose spelling does not match the
/// declared sort of the positional parameter it would bind.
const ISSUE_GENERIC_TYPE_ARG_SORT: &str = "generic_type_arg_sort";
/// A magnitudo default that cannot unify with the already-inferred hole shares
/// the shape-constraint sites' incompatibility code.
const ISSUE_INCOMPATIBLE_TENSOR_INDEX: &str = "incompatible_tensor_index";

/// Diagnostic issue for a shape param left unbound at a call site (D6).
///
/// Reuses the existing `cannot_infer_tensor_shape` issue (SEM014): a shape
/// param determined by neither arguments nor the expected type is exactly a
/// missing tensor shape witness, so it shares the code the goal text pins.
const ISSUE_CANNOT_INFER_TENSOR_SHAPE: &str = "cannot_infer_tensor_shape";

/// Z1: a size-slot argument is a lift store at the call, so its `>= 0`
/// obligation is checked there. A negative compile-time constant is rejected
/// with this issue; a negative runtime value raises it where MIR consumes the
/// sidecar binding.
pub(super) const ISSUE_SIZE_LIFT_NEGATIVE: &str = "size_lift_negative";

impl TypeChecker<'_> {
    /// Whether `ty` names one of the active function's type parameters.
    ///
    /// Tensor edge policies are compile-time selectors, so only a type-sorted
    /// generic parameter can stand in for one. `Type::Param` is also used for
    /// magnitudo witnesses; accepting those here would defer an invalid policy
    /// until a later compiler stage.
    pub fn is_tensor_edge_policy_type_param(&self, ty: TypeId) -> bool {
        let mut resolved = self.resolve_inferred_type(ty);
        let mut seen = FxHashSet::default();
        while let Type::Alias(_, inner) = self.types.get(resolved) {
            if !seen.insert(resolved) {
                return false;
            }
            resolved = *inner;
        }
        match self.types.get(resolved) {
            Type::Param(symbol) => {
                self.current_generic_param_kinds.get(symbol)
                    == Some(&radix_syntax::GenericParamKind::Typus)
            }
            _ => false,
        }
    }

    /// Typecheck arguments against a signature with generic params instantiated.
    pub fn check_instantiated_call(
        &mut self,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        self.check_instantiated_call_impl(
            sig,
            explicit_type_args,
            args,
            span,
            expected,
            true,
            None,
            None,
            false,
            false,
        )
    }

    /// [`Self::check_instantiated_call`] for a callee with a source-declared
    /// signature that is not a named function (a method without a stable id,
    /// an interface method, a function value or a function-typed field):
    /// call-site access markers are enforced against its parameter modes.
    pub fn check_instantiated_call_marked(
        &mut self,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        self.check_instantiated_call_impl(
            sig,
            explicit_type_args,
            args,
            span,
            expected,
            true,
            None,
            None,
            false,
            true,
        )
    }

    /// Typecheck a call to a library function reached through a namespace: a
    /// user function for the K5 lifting diagnostic, without a stable callee id.
    pub fn check_user_function_call(
        &mut self,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        expected: Option<TypeId>,
        policy_callee: Option<radix_hir::DefId>,
    ) -> TypeId {
        self.check_instantiated_call_impl(
            sig,
            explicit_type_args,
            args,
            span,
            expected,
            true,
            None,
            policy_callee,
            true,
            true,
        )
    }

    /// Typecheck a direct call while retaining its declaration identity for
    /// later concrete-instance planning. The ordinary wrapper remains useful
    /// for synthesized signatures and method paths without a stable id here.
    pub fn check_instantiated_call_for_callee(
        &mut self,
        callee: radix_hir::DefId,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        self.check_instantiated_call_impl(
            sig,
            explicit_type_args,
            args,
            span,
            expected,
            true,
            Some(callee),
            Some(callee),
            true,
            true,
        )
    }

    /// Typecheck a resolved method call while retaining the method identity
    /// needed by deferred semantic obligations. This preserves ordinary
    /// method-call behavior, including its non-lifting argument checks.
    pub fn check_instantiated_method_call_for_callee(
        &mut self,
        callee: radix_hir::DefId,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        self.check_instantiated_call_impl(
            sig,
            explicit_type_args,
            args,
            span,
            expected,
            true,
            None,
            Some(callee),
            false,
            true,
        )
    }

    /// Intrinsic-method argument checking with rigid shape terms.
    ///
    /// S0-G8: the intrinsic path builds its signature from the receiver's
    /// type, so an `IndexExpr::Param` width inside it is the enclosing
    /// generic body's rigid size param — not a callee shape param this call
    /// must solve. Solving it trips the D6 unbound-shape rejection
    /// (SEM014) on every parametric-receiver method call; here the shape
    /// machinery is skipped and the parametric width stays rigid.
    pub fn check_rigid_method_call(
        &mut self,
        sig: &FuncSig,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> TypeId {
        self.check_instantiated_call_impl(
            sig,
            &[],
            args,
            span,
            None,
            false,
            None,
            None,
            false,
            false,
        )
    }

    fn check_instantiated_call_impl(
        &mut self,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        expected: Option<TypeId>,
        solve_shape_params: bool,
        callee: Option<radix_hir::DefId>,
        policy_callee: Option<radix_hir::DefId>,
        user_function: bool,
        markers: bool,
    ) -> TypeId {
        // Size-type Z3 (Q3): `_` extent holes in a callee signature are
        // anonymous size parameters chosen per call. The declared signature
        // term is interned once, so its holes must be re-instantiated at the
        // call boundary; otherwise the first call's unification binds the
        // shared hole program-wide (the g06/c1 first-call defect). The rigid
        // method path keeps its signature untouched (S0-G8).
        let sig = if solve_shape_params {
            self.instantiate_anonymous_extents(sig)
        } else {
            sig.clone()
        };
        // Magnitudo shape params are carried by the signature's generic
        // descriptors: the declared sort is authoritative, so a size param
        // is a shape param even when no signature shape mentions it.
        let shape_symbols = if solve_shape_params {
            sig.type_params
                .iter()
                .filter(|param| param.sort == GenericParamSort::Magnitudo)
                .map(|param| param.name)
                .collect()
        } else {
            FxHashSet::default()
        };
        let (sig, inferred_vars, mut shape_vars, explicit_bound) = if explicit_type_args.is_empty()
        {
            let mut param_vars = FxHashMap::default();
            // Pre-create one infer per Typus descriptor so a param that
            // occurs in no signature type still has a bindable slot for
            // defaults and for the resolved-binding record.
            for param in &sig.type_params {
                if param.sort == GenericParamSort::Typus {
                    let infer = self.fresh_infer();
                    param_vars.insert(param.name, infer);
                }
            }
            let mut shape_vars = self.shape_vars_for(&shape_symbols);
            let sig = self.substitute_func_sig(&sig, &mut param_vars, Some(&mut shape_vars));
            (sig, Some(param_vars), shape_vars, FxHashSet::default())
        } else if let Some(error_ty) =
            self.reject_invalid_explicit_type_args(&sig, explicit_type_args, args, span)
        {
            return error_ty;
        } else if self.reject_type_arg_domain_misses(&sig, explicit_type_args, span) {
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        } else {
            let mut param_vars = FxHashMap::default();
            let mut explicit_bound = FxHashSet::default();
            let size_prefix = self.explicit_args_are_magnitudo_list(explicit_type_args)
                || (sig
                    .type_params
                    .iter()
                    .all(|param| param.sort != GenericParamSort::Typus)
                    && self.explicit_args_are_size_witnesses(explicit_type_args));
            if size_prefix {
                // Positional size prefix: every zipped descriptor is a
                // magnitudo param (sort-checked by the arity validator).
                let mut shape_vars = self.shape_vars_for(&shape_symbols);
                self.bind_explicit_magnitudo_args(
                    &sig,
                    explicit_type_args,
                    &mut shape_vars,
                    &mut explicit_bound,
                );
                for param in &sig.type_params {
                    if param.sort == GenericParamSort::Typus {
                        let infer = self.fresh_infer();
                        param_vars.insert(param.name, infer);
                    }
                }
                (
                    self.substitute_func_sig(&sig, &mut param_vars, Some(&mut shape_vars)),
                    Some(param_vars),
                    shape_vars,
                    explicit_bound,
                )
            } else {
                // Type prefix binds the leading typus params; the rest are
                // instantiated as fresh infers so inference can solve them.
                for (param, ty) in sig
                    .type_params
                    .iter()
                    .filter(|param| param.sort == GenericParamSort::Typus)
                    .zip(explicit_type_args.iter())
                {
                    param_vars.insert(param.name, *ty);
                    explicit_bound.insert(param.name);
                }
                for param in &sig.type_params {
                    if param.sort == GenericParamSort::Typus {
                        param_vars
                            .entry(param.name)
                            .or_insert_with(|| self.fresh_infer());
                    }
                }
                let mut shape_vars = self.shape_vars_for(&shape_symbols);
                (
                    self.substitute_func_sig(&sig, &mut param_vars, Some(&mut shape_vars)),
                    Some(param_vars),
                    shape_vars,
                    explicit_bound,
                )
            }
        };

        let invalid_arithmetic = sig
            .params
            .iter()
            .find_map(|param| self.first_index_arithmetic_error_in_type(param.ty))
            .or_else(|| self.first_index_arithmetic_error_in_type(sig.ret))
            .or_else(|| {
                sig.err
                    .and_then(|error| self.first_index_arithmetic_error_in_type(error))
            });
        if let Some(error) = invalid_arithmetic {
            let message = crate::figura::figura_arithmetic_error_message(error);
            let reason = crate::figura::figura_arithmetic_reason(message)
                .unwrap_or("invalid dimension arithmetic");
            self.invalid_operand_types_with_args(
                "figura_arithmetic_invalid",
                span,
                &[("reason", reason.to_owned())],
            );
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }

        // A lifting attempt (a scalar user function called on a tensor) is one
        // diagnosed error. The scalar result type is not the tensor the
        // caller meant, so the call takes the type its context expects
        // instead of cascading a second mismatch.
        let axis_key = callee.or(if user_function { None } else { policy_callee });
        let axis = axis_key.and_then(|key| self.axis_params.get(&key).cloned());
        if self.check_call_args_lifting(&sig, args, span, user_function, markers, axis.as_deref()) {
            return expected.unwrap_or(self.error_type);
        }
        // A result that only needs a checked integer store to reach the slot
        // (`int` into `u8`) is not a mismatch: the caller applies the store
        // (rulings 17b, 21).
        if let Some(expected) = expected
            && self.store_narrowing_target(sig.ret, expected).is_none()
        {
            // A return-only shape param (D1) leaves the return type's whole
            // shape as the callee's own unbound inference hole. Binding it
            // from the expected type is inference, not a run-time shape
            // boundary of the enclosing store, return, or argument slot (those
            // are checked on the call's result afterwards), so suspend the
            // boundary contexts: otherwise the hole meeting a ranked
            // expectation reports `cannot_infer_tensor_shape`. Returns whose
            // shape is not a bare callee hole keep the boundary path.
            let binds_return_hole = self.return_shape_is_unbound_hole(sig.ret);
            let saved = binds_return_hole.then(|| {
                (
                    self.current_shape_argument.take(),
                    self.current_shape_return_expr.take(),
                    self.current_store_expr.take(),
                )
            });
            self.unify_carrier_assign(sig.ret, expected, span, ISSUE_EXPRESSION_TYPE_MISMATCH);
            if let Some((argument, return_expr, store_expr)) = saved {
                self.current_shape_argument = argument;
                self.current_shape_return_expr = return_expr;
                self.current_store_expr = store_expr;
            }
        }
        // GPD-3: inference is complete, so declared defaults now bind every
        // still-unbound parameter and the complete ordered vector is recorded
        // for downstream specialization. A defaulted hole counts as bound, so
        // the rejections below only fire for genuinely unresolved slots.
        if solve_shape_params
            && let Some(param_vars) = &inferred_vars
            && let Some(mut bindings) = self.resolve_generic_bindings(
                &sig,
                param_vars,
                &mut shape_vars,
                &explicit_bound,
                span,
            )
        {
            let runtime_size_symbols =
                self.capture_runtime_call_size_bindings(&sig.type_params, &shape_vars, span);
            self.reject_negative_constant_size_args(
                &sig.type_params,
                &shape_vars,
                &runtime_size_symbols,
                span,
            );
            // Runtime extent identity belongs in the sidecar operand map, not
            // in the instance key. Keep the callee's formal generic symbol as
            // the key witness while its instantiated signature above retains
            // the caller's concrete runtime source.
            for binding in &mut bindings {
                if runtime_size_symbols.contains(&binding.name)
                    && let GenericWitness::Magnitudo(_) = binding.witness
                {
                    binding.witness = GenericWitness::Magnitudo(
                        self.types.intern_index(IndexExpr::Param(binding.name)),
                    );
                }
            }
            let resolved_call = ResolvedGenericCall {
                call_span: span,
                callee,
                bindings,
            };
            if !resolved_call.bindings.is_empty() {
                self.tensor_edge_generic_calls.push(
                    super::tensor_edge_policy_generic::GenericPolicyCall {
                        caller: self.current_function_def,
                        callee: policy_callee,
                        span,
                        bindings: resolved_call.bindings.clone(),
                    },
                );
            }
            self.types.resolved_generic_calls.push(resolved_call);
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
            // D1.3: every witness nominally declares its parameter's bounds.
            // The call's result type stays known, so a miss reports once
            // without cascading into the surrounding binding.
            self.reject_bound_misses(&sig, &param_vars, span);
        }
        if self.reject_failable_call(&sig, span) {
            return self.error_type;
        }
        self.resolve_inferred_type(sig.ret)
    }

    /// Z1: reject a size-slot argument whose compile-time value is negative.
    ///
    /// The size law's lift rule makes a runtime int in a generic call's size
    /// slot a lift store, so its `>= 0` obligation fires at the call. A
    /// constant witness already below zero can never satisfy the store, so it
    /// is a compile error here; runtime witnesses keep the sidecar binding
    /// consumed by MIR call lowering.
    fn reject_negative_constant_size_args(
        &mut self,
        params: &[radix_types::GenericParamDescriptor],
        shape_vars: &FxHashMap<Symbol, IndexId>,
        runtime_symbols: &FxHashSet<Symbol>,
        span: radix_lexer::Span,
    ) {
        for param in params {
            if param.sort != GenericParamSort::Magnitudo || runtime_symbols.contains(&param.name) {
                continue;
            }
            let Some(witness) = shape_vars.get(&param.name).copied() else {
                continue;
            };
            let witness = self.resolve_index(witness);
            let negative = match self.types.get_index(witness) {
                IndexExpr::Affine { base, offset } => match self.types.get_index(*base) {
                    IndexExpr::Literal(value) => {
                        let total = i128::from(*value) + *offset;
                        (total < 0).then_some(total)
                    }
                    // Z1: a static `const size N = <literal>` folds here, so
                    // `N - 1` with `N = 0` fails the same lift obligation.
                    // Declared sizes reach figura terms as
                    // Runtime(ScalarBinding(def)); forwarded params resolve
                    // through the scope stack.
                    IndexExpr::Param(sym) => self
                        .hir_value_binding_def(*sym)
                        .and_then(|def| self.static_size_values.get(&def).copied())
                        .map(|value| i128::from(value) + *offset)
                        .filter(|total| *total < 0),
                    IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def)) => self
                        .static_size_values
                        .get(def)
                        .map(|value| i128::from(*value) + *offset)
                        .filter(|total| *total < 0),
                    _ => None,
                },
                _ => None,
            };
            if let Some(total) = negative {
                self.invalid_operand_types_with_args(
                    ISSUE_SIZE_LIFT_NEGATIVE,
                    span,
                    &[("value", total.to_string())],
                );
            }
        }
    }

    /// Z1: fold static `const size` values into a store annotation's affine
    /// extents. `N - 1` under `const size N = 0` is provably negative, so the
    /// store's lift obligation fails here, mirroring the call-slot rule above.
    pub(crate) fn reject_negative_static_affine_extents(
        &mut self,
        shape: IndexId,
        span: radix_lexer::Span,
    ) {
        let mut work = vec![shape];
        while let Some(id) = work.pop() {
            match self.types.get_index(id).clone() {
                IndexExpr::Tuple(dims) => work.extend(dims.iter().rev().copied()),
                IndexExpr::Affine { base, offset } => {
                    // A declared size reaches figura terms as
                    // Runtime(ScalarBinding(def)); a forwarded size param
                    // stays Param(sym) and resolves through the scope stack.
                    let binding = match self.types.get_index(base).clone() {
                        IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def)) => Some(def),
                        IndexExpr::Param(sym) => self.hir_value_binding_def(sym),
                        _ => None,
                    };
                    let Some(total) = binding
                        .and_then(|def| self.static_size_values.get(&def).copied())
                        .map(|value| i128::from(value) + offset)
                        .filter(|total| *total < 0)
                    else {
                        continue;
                    };
                    self.invalid_operand_types_with_args(
                        ISSUE_SIZE_LIFT_NEGATIVE,
                        span,
                        &[("value", total.to_string())],
                    );
                }
                _ => {}
            }
        }
    }

    /// Returns the shared error type when explicit args are invalid for `sig`.
    ///
    /// A NATURAL-only list binds the leading magnitudo descriptors
    /// positionally; a non-natural list counts Typus params. Either way the
    /// prefix may omit only a trailing defaulted suffix (GPD-3), and every
    /// bound argument must match its descriptor's declared sort.
    fn reject_invalid_explicit_type_args(
        &mut self,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        debug_assert!(!explicit_type_args.is_empty());

        if self.explicit_args_are_magnitudo_list(explicit_type_args) {
            for param in sig
                .type_params
                .iter()
                .zip(explicit_type_args.iter())
                .map(|(param, _)| param)
            {
                if param.sort == GenericParamSort::Typus {
                    self.reject_type_arg_sort(param, span);
                    self.typecheck_call_arg_exprs(args);
                    return Some(self.error_type);
                }
            }
            if !self.explicit_prefix_len_in_range(&sig.type_params, explicit_type_args.len()) {
                self.wrong_arity("wrong_number_of_type_arguments", span);
                self.typecheck_call_arg_exprs(args);
                return Some(self.error_type);
            }
            return None;
        }

        let type_param_count = sig
            .type_params
            .iter()
            .filter(|param| param.sort == GenericParamSort::Typus)
            .count();
        // Trailing defaults make the defaulted Typus params a suffix of the
        // Typus list, so the required prefix is the non-defaulted Typus count.
        let typus_min = sig
            .type_params
            .iter()
            .filter(|param| param.sort == GenericParamSort::Typus)
            .take_while(|param| param.default.is_none())
            .count();
        if type_param_count == 0 {
            // GSF-1: a size-only callee also accepts caller-owned symbolic
            // magnitudo witnesses (`sink<T, D>(x)` inside
            // `forward<magnitudo T, magnitudo D>`). Non-`Param` witnesses
            // keep the literal-surface rejection below.
            if !sig.type_params.is_empty()
                && self.explicit_args_are_size_witnesses(explicit_type_args)
            {
                if !self.explicit_prefix_len_in_range(&sig.type_params, explicit_type_args.len()) {
                    self.wrong_arity("wrong_number_of_type_arguments", span);
                    self.typecheck_call_arg_exprs(args);
                    return Some(self.error_type);
                }
                return None;
            }
            if sig.type_params.iter().any(|param| {
                param.sort == GenericParamSort::Magnitudo
                    && explicit_type_args.iter().any(|ty| {
                        matches!(self.types.get(*ty), Type::Param(sym)
                            if self.magnitudo_literal_of(*ty).is_none()
                                && !self.is_size_param_symbol(*sym))
                    })
            }) {
                self.reject_type_arg_sort(&sig.type_params[0], span);
                self.typecheck_call_arg_exprs(args);
                return Some(self.error_type);
            }
            self.wrong_arity("callee_type_args_unsupported", span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }
        let len = explicit_type_args.len();
        if len < typus_min || len > type_param_count {
            self.wrong_arity("wrong_number_of_type_arguments", span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }
        for (param, ty) in sig
            .type_params
            .iter()
            .filter(|param| param.sort == GenericParamSort::Typus)
            .zip(explicit_type_args.iter())
        {
            if self.magnitudo_literal_of(*ty).is_some() {
                self.reject_type_arg_sort(param, span);
                self.typecheck_call_arg_exprs(args);
                return Some(self.error_type);
            }
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

    /// Whether an explicit prefix of `len` arguments is an admissible length
    /// for `params` in positional order: it must cover every non-defaulted
    /// parameter and may omit only the defaulted trailing suffix.
    fn explicit_prefix_len_in_range(
        &self,
        params: &[radix_types::GenericParamDescriptor],
        len: usize,
    ) -> bool {
        crate::types::generic_explicit_prefix_len_in_range(params, len)
    }

    /// Structured wrong-sort rejection for one explicit generic argument.
    fn reject_type_arg_sort(
        &mut self,
        param: &radix_types::GenericParamDescriptor,
        span: radix_lexer::Span,
    ) {
        let param_name = self.interner.map_or_else(
            || "T".to_owned(),
            |interner| interner.resolve(param.name).to_owned(),
        );
        let expected = match param.sort {
            GenericParamSort::Typus => "a type argument",
            GenericParamSort::Magnitudo => "a size argument",
        };
        self.invalid_operand_types_with_args(
            ISSUE_GENERIC_TYPE_ARG_SORT,
            span,
            &[("param", param_name), ("expected", expected.to_owned())],
        );
    }

    fn reject_type_arg_domain_misses(
        &mut self,
        sig: &FuncSig,
        witnesses: &[TypeId],
        span: radix_lexer::Span,
    ) -> bool {
        let mut rejected = false;
        for (param, witness) in sig.type_params.iter().zip(witnesses.iter()) {
            if self.type_arg_rejects_domain(param.name, &param.constraint, *witness, span) {
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
        for param in &sig.type_params {
            let Some(witness) = param_vars.get(&param.name).copied() else {
                continue;
            };
            if self.is_infer(self.resolve_inferred_type(witness)) {
                continue;
            }
            if self.type_arg_rejects_domain(param.name, &param.constraint, witness, span) {
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

    pub(crate) fn format_type_for_diagnostic(&self, ty: TypeId) -> String {
        let Some(interner) = self.interner else {
            return format!("{:?}", self.types.get(ty));
        };
        format_faber_type(ty, self.types, interner, &|def_id| {
            format!("Def{}", def_id.0)
        })
    }

    /// Size-type Z3 (Q3): give every `_` extent hole in a callee signature's
    /// parameter figuras a fresh per-call hole. Repeated occurrences of one
    /// declared hole share the fresh instance within this call; distinct
    /// holes stay distinct. Return figuras keep their declared holes: an
    /// opaque `→ tf32[_, _]` is not a per-call parameter — its witnesses come
    /// from the callee body and are pinned at the caller's store (Z2 dw8).
    fn instantiate_anonymous_extents(&mut self, sig: &FuncSig) -> FuncSig {
        let mut holes: FxHashMap<radix_types::IndexVar, IndexId> = FxHashMap::default();
        let mut freshen =
            |this: &mut Self, ty: TypeId| this.freshen_anonymous_type_holes(ty, &mut holes);
        FuncSig {
            type_params: sig.type_params.clone(),
            params: sig
                .params
                .iter()
                .map(|param| ParamType {
                    ty: freshen(self, param.ty),
                    mode: param.mode,
                    optional: param.optional,
                })
                .collect(),
            ret: sig.ret,
            err: sig.err,
            is_async: sig.is_async,
            is_generator: sig.is_generator,
        }
    }

    /// Rewrite unresolved `IndexExpr::Infer` leaves in a signature type's
    /// extent terms to this call's anonymous holes. Composite leaves rebuild
    /// exactly like [`Self::substitute_type_params`; generic symbols and all
    /// non-figura leaves keep their identity.
    fn freshen_anonymous_type_holes(
        &mut self,
        ty: TypeId,
        holes: &mut FxHashMap<radix_types::IndexVar, IndexId>,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved).clone() {
            Type::Param(_) => ty,
            Type::Array(inner) => {
                let inner = self.freshen_anonymous_type_holes(inner, holes);
                self.types.array(inner)
            }
            Type::Map(key, value) => {
                let key = self.freshen_anonymous_type_holes(key, holes);
                let value = self.freshen_anonymous_type_holes(value, holes);
                self.types.map(key, value)
            }
            Type::Set(inner) => {
                let inner = self.freshen_anonymous_type_holes(inner, holes);
                self.types.set(inner)
            }
            Type::Option(inner) => {
                let inner = self.freshen_anonymous_type_holes(inner, holes);
                self.types.option(inner)
            }
            Type::Promissum(inner) => {
                let inner = self.freshen_anonymous_type_holes(inner, holes);
                self.types.promissum(inner)
            }
            Type::PromissumFailable(success, alternate) => {
                let success = self.freshen_anonymous_type_holes(success, holes);
                let alternate = self.freshen_anonymous_type_holes(alternate, holes);
                self.types.promissum_failable(success, alternate)
            }
            Type::Cursor(inner) => {
                let inner = self.freshen_anonymous_type_holes(inner, holes);
                self.types.cursor(inner)
            }
            Type::AsyncCursor(item, alternate) => {
                let item = self.freshen_anonymous_type_holes(item, holes);
                let alternate = self.freshen_anonymous_type_holes(alternate, holes);
                self.types.async_cursor(item, alternate)
            }
            Type::Intervallum(inner) => {
                let inner = self.freshen_anonymous_type_holes(inner, holes);
                self.types.intervallum(inner)
            }
            Type::Ref(mode, inner) => {
                let inner = self.freshen_anonymous_type_holes(inner, holes);
                self.types.reference(mode, inner)
            }
            Type::Tensor(elem, shape) => {
                let element = self.freshen_anonymous_type_holes(elem, holes);
                let shape = self.freshen_anonymous_index_holes(shape, holes);
                self.types.tensor_with_shape(element, shape)
            }
            Type::Vector(elem, width) => {
                let element = self.freshen_anonymous_type_holes(elem, holes);
                let width = self.freshen_anonymous_index_holes(width, holes);
                self.types.vector_with_width(element, width)
            }
            Type::Matrix(elem, shape) => {
                let element = self.freshen_anonymous_type_holes(elem, holes);
                let shape = self.freshen_anonymous_index_holes(shape, holes);
                self.types.matrix_with_shape(element, shape)
            }
            Type::Sparsa(elem, shape) => {
                let element = self.freshen_anonymous_type_holes(elem, holes);
                let shape = self.freshen_anonymous_index_holes(shape, holes);
                self.types.sparsa_with_shape(element, shape)
            }
            Type::BoundedAscii { capacity } => {
                let capacity = self.freshen_anonymous_index_holes(capacity, holes);
                self.types.bounded_ascii(capacity)
            }
            Type::BoundedTextus { capacity } => {
                let capacity = self.freshen_anonymous_index_holes(capacity, holes);
                self.types.bounded_textus(capacity)
            }
            Type::BoundedOcteti { capacity } => {
                let capacity = self.freshen_anonymous_index_holes(capacity, holes);
                self.types.bounded_octeti(capacity)
            }
            Type::BoundedArray { element, capacity } => {
                let element = self.freshen_anonymous_type_holes(element, holes);
                let capacity = self.freshen_anonymous_index_holes(capacity, holes);
                self.types.bounded_array(element, capacity)
            }
            Type::Func(sig) => {
                let instantiated = self.instantiate_anonymous_extents(&sig);
                self.types.function(instantiated)
            }
            Type::Applied(base, args) => {
                let base = self.freshen_anonymous_type_holes(base, holes);
                let args = args
                    .iter()
                    .map(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => radix_types::AppliedArg::Type(
                            self.freshen_anonymous_type_holes(*id, holes),
                        ),
                        radix_types::AppliedArg::Size(id) => radix_types::AppliedArg::Size(
                            self.freshen_anonymous_index_holes(*id, holes),
                        ),
                    })
                    .collect();
                self.types.intern(Type::Applied(base, args))
            }
            Type::Union(types) => {
                let types = types
                    .iter()
                    .map(|member| self.freshen_anonymous_type_holes(*member, holes))
                    .collect();
                self.types.intern(Type::Union(types))
            }
            Type::Tuple(elements) => {
                let elements = elements
                    .iter()
                    .map(|element| self.freshen_anonymous_type_holes(*element, holes))
                    .collect();
                self.types.intern(Type::Tuple(elements))
            }
            Type::Alias(def, inner) => {
                let inner = self.freshen_anonymous_type_holes(inner, holes);
                self.types.intern(Type::Alias(def, inner))
            }
            _ => ty,
        }
    }

    fn freshen_anonymous_index_holes(
        &mut self,
        id: IndexId,
        holes: &mut FxHashMap<radix_types::IndexVar, IndexId>,
    ) -> IndexId {
        match self.types.get_index(id).clone() {
            IndexExpr::Infer(var) => match holes.get(&var) {
                Some(fresh) => *fresh,
                None => {
                    let fresh = self.fresh_index_hole();
                    holes.insert(var, fresh);
                    fresh
                }
            },
            IndexExpr::Tuple(dims) => {
                let dims = dims
                    .iter()
                    .map(|dim| self.freshen_anonymous_index_holes(*dim, holes))
                    .collect();
                self.types.intern_index(IndexExpr::Tuple(dims))
            }
            IndexExpr::Affine { base, offset } => {
                let base = self.freshen_anonymous_index_holes(base, holes);
                add_index_constant(self.types, base, offset)
                    .unwrap_or_else(|_| self.types.intern_index(IndexExpr::Affine { base, offset }))
            }
            IndexExpr::Divide { numerator, divisor } => {
                let numerator = self.freshen_anonymous_index_holes(numerator, holes);
                divide_index_exact(self.types, numerator, divisor).unwrap_or_else(|_| {
                    self.types
                        .intern_index(IndexExpr::Divide { numerator, divisor })
                })
            }
            _ => id,
        }
    }

    pub fn substitute_func_sig(
        &mut self,
        sig: &FuncSig,
        vars: &mut FxHashMap<Symbol, TypeId>,
        mut shape_vars: Option<&mut FxHashMap<Symbol, IndexId>>,
    ) -> FuncSig {
        FuncSig {
            type_params: sig.type_params.clone(),
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
            Type::Tuple(elements) => {
                let elements = elements
                    .iter()
                    .map(|element| {
                        self.substitute_type_params(*element, vars, shape_vars.as_deref_mut())
                    })
                    .collect();
                self.types.intern(Type::Tuple(elements))
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
            IndexExpr::Affine { base, offset } => {
                let base = self.substitute_index_params(base, Some(shape_vars));
                add_index_constant(self.types, base, offset)
                    .unwrap_or_else(|_| self.types.intern_index(IndexExpr::Affine { base, offset }))
            }
            IndexExpr::Divide { numerator, divisor } => {
                let numerator = self.substitute_index_params(numerator, Some(shape_vars));
                divide_index_exact(self.types, numerator, divisor).unwrap_or_else(|_| {
                    self.types
                        .intern_index(IndexExpr::Divide { numerator, divisor })
                })
            }
            IndexExpr::Literal(_)
            | IndexExpr::Runtime(_)
            | IndexExpr::Infer(_)
            | IndexExpr::Unspecified => id,
        }
    }

    /// Find a concrete invalid affine/exact-division term anywhere inside a
    /// substituted signature type. Symbolic parameters and runtime extents
    /// remain obligations and return no error here.
    pub(crate) fn first_index_arithmetic_error_in_type(
        &self,
        ty: TypeId,
    ) -> Option<radix_types::IndexArithmeticError> {
        first_index_arithmetic_error_in_type(
            ty,
            self.types,
            &mut FxHashSet::default(),
            &mut FxHashSet::default(),
        )
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

    /// True when every explicit argument is a `Param`-spelled size witness —
    /// a NATURAL literal or a caller-owned symbolic magnitudo param (GSF-1).
    fn explicit_args_are_size_witnesses(&self, explicit_type_args: &[TypeId]) -> bool {
        !explicit_type_args.is_empty()
            && explicit_type_args
                .iter()
                .all(|ty| match self.types.get(*ty) {
                    Type::Param(sym) => {
                        self.magnitudo_literal_of(*ty).is_some()
                            || self.is_size_param_symbol(*sym)
                            || self.hir_value_binding_def(*sym).is_some()
                    }
                    _ => false,
                })
    }

    /// Whether `sym` names a caller-owned compile-time size parameter. HIR
    /// keeps generic type and size parameters in the same `Type::Param` form,
    /// so the active function declaration supplies the lexical sort while the
    /// resolver covers imported index parameters.
    fn is_size_param_symbol(&self, sym: Symbol) -> bool {
        if let Some(kind) = self.current_generic_param_kinds.get(&sym) {
            return *kind == radix_syntax::GenericParamKind::Magnitudo;
        }
        self.resolver
            .lookup(sym)
            .and_then(|def_id| self.resolver.get_symbol(def_id))
            .is_some_and(|symbol| symbol.kind == crate::SymbolKind::IndexParam)
    }

    /// Index witness of an explicit magnitudo argument: NATURAL spellings
    /// lower to literal indices, symbolic params forward as rigid
    /// `IndexExpr::Param` so the caller's shape identity is preserved.
    fn explicit_size_witness_index(&mut self, ty: TypeId) -> Option<crate::IndexId> {
        let Type::Param(sym) = self.types.get(ty) else {
            return None;
        };
        let sym = *sym;
        let name = self.interner?.resolve(sym);
        match radix_types::parse_natural_capacity(name) {
            Some(value) => Some(self.types.intern_index(IndexExpr::Literal(value))),
            None if self.is_size_param_symbol(sym) => {
                Some(self.types.intern_index(IndexExpr::Param(sym)))
            }
            None => self.hir_value_binding_def(sym).map(|def_id| {
                self.types
                    .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def_id)))
            }),
        }
    }

    /// DefId of the in-scope HIR value binding spelled `sym`, if any. HIR
    /// locals and parameters are keyed in the scope stack, not the resolver,
    /// so a spelled size-lift slot argument encoded as `Type::Param(sym)`
    /// resolves its runtime witness here.
    pub(super) fn hir_value_binding_def(
        &self,
        sym: radix_lexer::Symbol,
    ) -> Option<radix_hir::DefId> {
        self.binding_shadow
            .keys()
            .copied()
            .find(|def_id| self.binding_names.get(def_id) == Some(&sym))
    }

    fn bind_explicit_magnitudo_args(
        &mut self,
        sig: &FuncSig,
        explicit_type_args: &[TypeId],
        shape_vars: &mut FxHashMap<Symbol, crate::IndexId>,
        explicit_bound: &mut FxHashSet<Symbol>,
    ) {
        for (param, ty) in sig.type_params.iter().zip(explicit_type_args.iter()) {
            if let Some(index) = self.explicit_size_witness_index(*ty) {
                shape_vars.insert(param.name, index);
                explicit_bound.insert(param.name);
            }
        }
    }

    /// GPD-3 ordered binding result: explicit prefix, then inference, then the
    /// declared default. Returns `None` when any slot stays unresolved after
    /// all three stages, in which case no complete vector is recorded.
    ///
    /// A defaulted type param binds its still-unsolved infer var, and a
    /// defaulted size param solves its still-unsolved index hole, so later
    /// checks (constraints, D6 shape rejection) observe the default witness.
    fn resolve_generic_bindings(
        &mut self,
        sig: &FuncSig,
        param_vars: &FxHashMap<Symbol, TypeId>,
        shape_vars: &mut FxHashMap<Symbol, IndexId>,
        explicit_bound: &FxHashSet<Symbol>,
        span: radix_lexer::Span,
    ) -> Option<Vec<ResolvedGenericBinding>> {
        let mut bindings = Vec::with_capacity(sig.type_params.len());
        for param in &sig.type_params {
            let (source, witness) = if explicit_bound.contains(&param.name) {
                (
                    GenericBindingSource::Explicit,
                    self.witness_of(param, param_vars, shape_vars)?,
                )
            } else {
                match param.sort {
                    GenericParamSort::Typus => {
                        let var_ty = *param_vars.get(&param.name)?;
                        let resolved = self.resolve_inferred_type(var_ty);
                        if !self.is_infer(resolved) {
                            (
                                GenericBindingSource::Inferred,
                                GenericWitness::Typus(resolved),
                            )
                        } else if let Some(SemanticGenericParamDefault::Typus(default_ty)) =
                            param.default
                        {
                            let bound = self.bind_type_param_default(var_ty, default_ty, span)?;
                            (
                                GenericBindingSource::Defaulted,
                                GenericWitness::Typus(bound),
                            )
                        } else {
                            return None;
                        }
                    }
                    GenericParamSort::Magnitudo => {
                        let hole = *shape_vars.get(&param.name)?;
                        let resolved = self.resolve_index(hole);
                        if !matches!(self.types.get_index(resolved), IndexExpr::Infer(_)) {
                            (
                                GenericBindingSource::Inferred,
                                GenericWitness::Magnitudo(resolved),
                            )
                        } else if let Some(SemanticGenericParamDefault::Magnitudo(value)) =
                            param.default
                        {
                            let bound = self.bind_size_param_default(resolved, value, span)?;
                            (
                                GenericBindingSource::Defaulted,
                                GenericWitness::Magnitudo(bound),
                            )
                        } else {
                            return None;
                        }
                    }
                }
            };
            bindings.push(ResolvedGenericBinding {
                name: param.name,
                source,
                witness,
            });
        }
        Some(bindings)
    }

    /// Witness for an explicitly bound parameter, read back from the binding
    /// maps. Returns `None` when the maps lack the slot (never expected for a
    /// bound param, but keeps the caller total).
    fn witness_of(
        &mut self,
        param: &radix_types::GenericParamDescriptor,
        param_vars: &FxHashMap<Symbol, TypeId>,
        shape_vars: &mut FxHashMap<Symbol, IndexId>,
    ) -> Option<GenericWitness> {
        match param.sort {
            GenericParamSort::Typus => {
                let ty = self.resolve_inferred_type(*param_vars.get(&param.name)?);
                Some(GenericWitness::Typus(ty))
            }
            GenericParamSort::Magnitudo => {
                let id = self.resolve_index(*shape_vars.get(&param.name)?);
                Some(GenericWitness::Magnitudo(id))
            }
        }
    }

    /// Solve one still-unsolved type infer var to its declared default.
    /// Returns `None` when the bind failed and reported its own diagnostic.
    fn bind_type_param_default(
        &mut self,
        var_ty: TypeId,
        default_ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if let Some(var) = self.infer_var_of(var_ty) {
            self.bind_infer_exact_issue(var, default_ty, span, ISSUE_GENERIC_TYPE_ARG_DOMAIN);
        }
        let bound = self.resolve_inferred_type(var_ty);
        if self.is_infer(bound) {
            return None;
        }
        Some(bound)
    }

    /// Solve one still-unsolved index hole to its declared natural default.
    /// Returns `None` when the bind failed and reported its own diagnostic.
    fn bind_size_param_default(
        &mut self,
        hole: IndexId,
        value: u64,
        span: radix_lexer::Span,
    ) -> Option<IndexId> {
        let literal = self.types.intern_index(IndexExpr::Literal(value));
        let mut unifier = self.index_infer.unifier(self.types);
        match unifier.unify(hole, literal) {
            Ok(bound) => Some(bound),
            Err(_) => {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                    span,
                );
                None
            }
        }
    }

    /// True when `ty` is a tensor whose whole shape is still an unbound
    /// inference hole (the callee's return-only shape param, D1).
    fn return_shape_is_unbound_hole(&mut self, ty: TypeId) -> bool {
        let Type::Tensor(_, shape) = self.types.get(ty).clone() else {
            return false;
        };
        let shape = self.resolve_index(shape);
        matches!(self.types.get_index(shape), IndexExpr::Infer(_))
    }

    /// Allocate one fresh index hole per distinct shape-param symbol (D1).
    fn shape_vars_for(&mut self, shape_symbols: &FxHashSet<Symbol>) -> FxHashMap<Symbol, IndexId> {
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

fn first_index_arithmetic_error_in_type(
    ty: TypeId,
    types: &TypeTable,
    seen_types: &mut FxHashSet<TypeId>,
    seen_indices: &mut FxHashSet<IndexId>,
) -> Option<radix_types::IndexArithmeticError> {
    if !seen_types.insert(ty) {
        return None;
    }
    let check_type =
        |ty, seen_types: &mut FxHashSet<TypeId>, seen_indices: &mut FxHashSet<IndexId>| {
            first_index_arithmetic_error_in_type(ty, types, seen_types, seen_indices)
        };
    let check_index = |id, seen_indices: &mut FxHashSet<IndexId>| {
        first_index_arithmetic_error_in_index(id, types, seen_indices)
    };

    match types.get(ty) {
        radix_types::Type::Array(inner)
        | radix_types::Type::Queue(inner)
        | radix_types::Type::Stack(inner)
        | radix_types::Type::Set(inner)
        | radix_types::Type::Promissum(inner)
        | radix_types::Type::Cursor(inner)
        | radix_types::Type::Atomic(inner)
        | radix_types::Type::Intervallum(inner)
        | radix_types::Type::Option(inner)
        | radix_types::Type::Ref(_, inner)
        | radix_types::Type::Alias(_, inner) => check_type(*inner, seen_types, seen_indices),
        radix_types::Type::BoundedArray { element, capacity }
        | radix_types::Type::BoundedQueue { element, capacity }
        | radix_types::Type::BoundedStack { element, capacity } => {
            check_index(*capacity, seen_indices)
                .or_else(|| check_type(*element, seen_types, seen_indices))
        }
        radix_types::Type::BoundedTextus { capacity }
        | radix_types::Type::BoundedAscii { capacity }
        | radix_types::Type::BoundedOcteti { capacity } => check_index(*capacity, seen_indices),
        radix_types::Type::Map(key, value) => check_type(*key, seen_types, seen_indices)
            .or_else(|| check_type(*value, seen_types, seen_indices)),
        radix_types::Type::Record(fields) => fields
            .values()
            .find_map(|field| check_type(*field, seen_types, seen_indices)),
        radix_types::Type::PromissumFailable(success, alternate)
        | radix_types::Type::AsyncCursor(success, alternate) => {
            check_type(*success, seen_types, seen_indices)
                .or_else(|| check_type(*alternate, seen_types, seen_indices))
        }
        radix_types::Type::Tensor(element, shape)
        | radix_types::Type::Vector(element, shape)
        | radix_types::Type::Matrix(element, shape)
        | radix_types::Type::Sparsa(element, shape) => check_index(*shape, seen_indices)
            .or_else(|| check_type(*element, seen_types, seen_indices)),
        radix_types::Type::Func(signature) => signature
            .params
            .iter()
            .find_map(|param| check_type(param.ty, seen_types, seen_indices))
            .or_else(|| check_type(signature.ret, seen_types, seen_indices))
            .or_else(|| {
                signature
                    .err
                    .and_then(|err| check_type(err, seen_types, seen_indices))
            }),
        radix_types::Type::Applied(base, args) => check_type(*base, seen_types, seen_indices)
            .or_else(|| {
                args.iter().find_map(|arg| match arg {
                    radix_types::AppliedArg::Type(ty) => check_type(*ty, seen_types, seen_indices),
                    radix_types::AppliedArg::Size(index) => check_index(*index, seen_indices),
                })
            }),
        radix_types::Type::Union(members)
        | radix_types::Type::Intersection(members)
        | radix_types::Type::Tuple(members) => members
            .iter()
            .find_map(|member| check_type(*member, seen_types, seen_indices)),
        _ => None,
    }
}

fn first_index_arithmetic_error_in_index(
    id: IndexId,
    types: &TypeTable,
    seen_indices: &mut FxHashSet<IndexId>,
) -> Option<radix_types::IndexArithmeticError> {
    if !seen_indices.insert(id) {
        return None;
    }
    let expression = types.get_index(id);
    match expression {
        IndexExpr::Tuple(dims) => dims
            .iter()
            .find_map(|dim| first_index_arithmetic_error_in_index(*dim, types, seen_indices)),
        IndexExpr::Affine { base, .. } => radix_types::evaluate_index_dimension(id, types)
            .err()
            .or_else(|| first_index_arithmetic_error_in_index(*base, types, seen_indices)),
        IndexExpr::Divide { numerator, .. } => radix_types::evaluate_index_dimension(id, types)
            .err()
            .or_else(|| first_index_arithmetic_error_in_index(*numerator, types, seen_indices)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "size_default_unify_test.rs"]
mod size_default_unify_test;

#[cfg(test)]
#[path = "size_lift_negative_test.rs"]
mod size_lift_negative_test;
