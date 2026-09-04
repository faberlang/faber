//! Function, constructor, and method call typing.
//!
//! Calls are where expression inference meets the callable shapes collected
//! earlier in semantic analysis. This file accepts already-resolved HIR callees,
//! validates argument arity and parameter compatibility, builds provisional
//! function types for inference variables, and enforces the local rule that
//! failable calls need an active local handler.
//!
//! INVARIANTS
//! ==========
//! - Known function signatures drive expected-type checking of each argument.
//! - Infer-typed callees are unified with a synthesized function signature from
//!   the actual arguments, allowing call sites to constrain unknown values.
//!   Each argument expression is typechecked exactly once on inferred paths.
//! - `ignotum` callees and receivers keep checking their arguments but return
//!   `ignotum` instead of manufacturing hard errors from an intentional escape.
//! - Backend method translation and stdlib target names are not decided here;
//!   this pass only establishes the semantic result type and diagnostics.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use crate::format_faber_type;
use radix_hir::HirCallArg;

enum DirectionalViewKind {
    Meus,
    Tuus,
}

const ISSUE_ASCII_LITERAL_TEMPLATE_APPLICATION: &str = "ascii_literal_template_application";
const ISSUE_ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";
const ISSUE_CALLEE_NOT_CALLABLE: &str = "callee_not_callable";
const ISSUE_TRANSFER_PAYLOAD_TYPE_MISMATCH: &str = "transfer_payload_type_mismatch";
const ISSUE_TRANSFER_SINK_MUST_BE_UNARY: &str = "transfer_sink_must_be_unary";
const ISSUE_TRANSFER_SINK_MUST_RETURN_VACUUM: &str = "transfer_sink_must_return_vacuum";
const ISSUE_TRANSFER_SINK_NOT_CALLABLE: &str = "transfer_sink_not_callable";
const ISSUE_DIRECTIONAL_VIEW_METHOD_TYPE_ARGS: &str = "directional_view_method_type_args";
const ISSUE_DYNAMIC_CALLEE_TYPE_ARGS: &str = "dynamic_callee_type_args";
const ISSUE_DYNAMIC_RECEIVER_METHOD_TYPE_ARGS: &str = "dynamic_receiver_method_type_args";
const ISSUE_FAILABLE_CALL_ENCLOSING_ERROR_TYPE_MISMATCH: &str =
    "failable_call_enclosing_error_type_mismatch";
pub(crate) const ISSUE_FAILABLE_CALL_CONVERSION_ABSENT: &str = "failable_call_conversion_absent";
const ISSUE_FAILABLE_CALL_HANDLER_ERROR_TYPE_MISMATCH: &str =
    "failable_call_handler_error_type_mismatch";
const ISSUE_FAILABLE_CALL_REQUIRES_HANDLER: &str = "failable_call_requires_handler";
const ISSUE_GPU_BUILTIN_DEVICE_FUNCTION_REQUIRED: &str = "gpu_builtin_device_function_required";
const ISSUE_GPU_BUILTIN_TYPE_ARGS: &str = "gpu_builtin_type_args";
const ISSUE_INFERRED_CALLEE_TYPE_ARGS: &str = "inferred_callee_type_args";
const ISSUE_METHOD_CALL_TYPE_ARGS: &str = "method_call_type_args";
const ISSUE_OCTETI_LITERAL_TEMPLATE_APPLICATION: &str = "octeti_literal_template_application";
const ISSUE_TEMPLATE_APPLICATION_TYPE_ARGS: &str = "template_application_type_args";
const ISSUE_UNION_MEMBER_NOT_COMMON: &str = "union_member_not_common";
const ISSUE_UNION_NOMINAL_MEMBERS: &str = "union_nominal_members";
const ISSUE_UNION_NO_COMMON_MEMBERS: &str = "union_no_common_members";
const ISSUE_UNION_METHOD_SIGNATURE_SHADOWED: &str = "union_method_signature_shadowed";
const ISSUE_VARIANT_CONSTRUCTOR_TYPE_ARGS: &str = "variant_constructor_type_args";

impl TypeChecker<'_> {
    /// Reject explicit type arguments on callee shapes that cannot carry generics.
    fn reject_explicit_type_args_on_dynamic_callee(
        &mut self,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> bool {
        if explicit_type_args.is_empty() {
            return false;
        }
        self.invalid_operand_types(issue, span);
        self.typecheck_call_arg_exprs(args);
        true
    }

    /// Typecheck infer- or ignotum-shaped callees that lack a known signature.
    ///
    /// Returns `Some(result)` when the resolved callee type is handled here;
    /// `None` means the caller should continue with ordinary non-callable recovery.
    fn check_dynamic_callee_call(
        &mut self,
        resolved: TypeId,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if self.is_infer(resolved) {
            if self.reject_explicit_type_args_on_dynamic_callee(
                explicit_type_args,
                args,
                span,
                ISSUE_INFERRED_CALLEE_TYPE_ARGS,
            ) {
                return Some(self.error_type);
            }
            return Some(self.check_inferred_call(resolved, args, span));
        }

        if matches!(
            self.types.get(resolved),
            Type::Primitive(Primitive::Ignotum)
        ) {
            if self.reject_explicit_type_args_on_dynamic_callee(
                explicit_type_args,
                args,
                span,
                ISSUE_DYNAMIC_CALLEE_TYPE_ARGS,
            ) {
                return Some(self.error_type);
            }
            self.typecheck_call_arg_exprs(args);
            return Some(self.types.primitive(Primitive::Ignotum));
        }

        None
    }

    /// Visit every call argument expression for diagnostics without expected context.
    pub fn typecheck_call_arg_exprs(&mut self, args: &mut [HirCallArg]) {
        for arg in args {
            self.check_expr(&mut arg.expr);
        }
    }

    /// Typechecks each call argument exactly once without callee expected context.
    fn typecheck_call_args_once(&mut self, args: &mut [HirCallArg]) -> Vec<ParamType> {
        args.iter_mut()
            .map(|arg| ParamType {
                ty: self.check_expr(&mut arg.expr),
                mode: SemanticParamMode::Owned,
                optional: false,
            })
            .collect()
    }

    fn method_display_name(&self, name: Symbol) -> String {
        self.interner.map_or_else(
            || "method".to_owned(),
            |interner| interner.resolve(name).to_owned(),
        )
    }

    /// Field-name spelling for union member attribution diagnostics (U1R).
    pub(crate) fn member_display_name(&self, name: Symbol) -> String {
        self.interner.map_or_else(
            || "member".to_owned(),
            |interner| interner.resolve(name).to_owned(),
        )
    }

    /// Reports SEM017 when a method's availability is `nondum` for this target.
    ///
    /// Returns `true` when a nondum diagnostic was emitted.
    fn report_nondum_method_call(
        &mut self,
        name: Symbol,
        availability: &[radix_hir::HirAvailability],
        span: radix_lexer::Span,
        args: &mut [HirCallArg],
    ) -> bool {
        let Some(radix_hir::HirAvailability::Nondum { reason, .. }) =
            self.nondum_for_target(availability)
        else {
            return false;
        };
        self.typecheck_call_arg_exprs(args);
        let method_name = self.method_display_name(name);
        let reason = reason.and_then(|reason| {
            self.interner
                .map(|interner| interner.resolve(reason).to_owned())
        });
        self.unavailable_declaration("nondum_method_for_target", span, method_name, reason);
        true
    }

    /// Assembles a provisional function signature from already-checked argument types.
    // moves params into FuncSig
    fn signature_from_arg_types(&mut self, params: Vec<ParamType>) -> FuncSig {
        FuncSig {
            type_params: Vec::new(),
            params,
            ret: self.fresh_infer(),
            err: None,
            is_async: false,
            is_generator: false,
        }
    }

    /// Builds a provisional function signature from actual arguments.
    ///
    /// This is an inference device, not a declaration model. Each argument is
    /// checked once, then the callee's inference variable can be unified with a
    /// callable type that reflects the use site.
    pub fn build_call_signature(&mut self, args: &mut [HirCallArg]) -> FuncSig {
        let params = self.typecheck_call_args_once(args);
        self.signature_from_arg_types(params)
    }

    /// Inferred-callee path: check arguments once, unify callee, return result type.
    ///
    /// Known-signature calls still use [`Self::check_call_args`]; this helper
    /// avoids re-traversing argument expressions after synthesizing a signature.
    fn check_inferred_call(
        &mut self,
        callee_ty: TypeId,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> TypeId {
        let sig = self.build_call_signature(args);
        let func_ty = self.types.function(sig.clone());
        self.unify_assign_issue(callee_ty, func_ty, span, ISSUE_CALLEE_NOT_CALLABLE);
        sig.ret
    }

    /// Returns a callable signature only for resolved function-shaped types.
    pub fn signature_from_type(&self, ty: TypeId) -> Option<FuncSig> {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Func(sig) => Some(sig.clone()),
            _ => None,
        }
    }
    /// Checks a call when the callee type is already known by another access
    /// form, such as optional chaining or non-null call chaining.
    ///
    /// Error recovery mirrors direct calls: known functions validate arguments,
    /// inference variables become provisional functions, `ignotum` preserves the
    /// escape hatch while still visiting arguments, and non-callables emit one
    /// diagnostic before returning the shared error type.
    pub fn check_call_from_type(
        &mut self,
        callee_ty: TypeId,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(callee_ty);
        if let Some(sig) = self.signature_from_type(resolved) {
            return self.check_instantiated_call(&sig, explicit_type_args, args, span, None);
        }

        if let Some(result) =
            self.check_dynamic_callee_call(resolved, explicit_type_args, args, span)
        {
            return result;
        }

        self.typecheck_call_arg_exprs(args);
        self.not_callable("callee_not_callable", span);
        self.error_type
    }
    /// Accepts a single spread-array argument when it can satisfy every required
    /// non-optional parameter in a fixed signature.
    ///
    /// The helper is intentionally conservative: optional parameters disable
    /// this compatibility mode because a single array cannot describe which
    /// trailing arguments were omitted.
    pub fn check_spread_array_compat(&mut self, sig: &FuncSig, arg: &mut HirCallArg) -> bool {
        if !arg.spread {
            return false;
        }
        let arg_ty = self.check_expr(&mut arg.expr);
        let resolved = self.resolve_inferred_type(arg_ty);
        let inner = match self.types.get(resolved) {
            Type::Array(inner) => *inner,
            _ => return false,
        };
        if sig.params.iter().any(|param| param.optional) {
            return false;
        }
        for param in &sig.params {
            self.unify_assign_issue(inner, param.ty, arg.span, ISSUE_ARGUMENT_TYPE_MISMATCH);
        }
        true
    }
    /// Validates argument count and parameter types for a known signature.
    ///
    /// Each ordinary argument is checked with its parameter type as expected
    /// context, which lets aggregates, closures, and empty literals adopt the
    /// shape required by the callee before the final mismatch diagnostic is
    /// considered.
    pub fn check_call_args(
        &mut self,
        sig: &FuncSig,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) {
        let required = sig.params.iter().filter(|param| !param.optional).count();
        let spread_compat = args.len() == 1
            && sig.params.len() > 1
            && self.check_spread_array_compat(sig, &mut args[0]);
        if !spread_compat && (args.len() < required || args.len() > sig.params.len()) {
            self.wrong_arity("wrong_number_of_arguments", span);
        }
        if spread_compat {
            return;
        }

        for (arg, param) in args.iter_mut().zip(sig.params.iter()) {
            let arg_ty = self.check_expr_with_expected(&mut arg.expr, Some(param.ty));
            if self.types.is_ref_view(arg_ty)
                && (matches!(
                    param.mode,
                    SemanticParamMode::MutRef | SemanticParamMode::Move
                ) || self.types.is_mut_or_own_view(param.ty))
            {
                self.error_issue(
                    SemanticErrorKind::ModeMismatch,
                    crate::ISSUE_REF_TO_MUT_OR_OWN_POSITION,
                    arg.span,
                );
            }
            self.unify_carrier_assign(arg_ty, param.ty, arg.span, ISSUE_ARGUMENT_TYPE_MISMATCH);
        }
    }
    /// Typecheck a Path receiver as an imported namespace function, if it is one.
    ///
    /// The HIR Path DefId is the nearest value binding. A local parameter can
    /// therefore own the Path while the member is still a namespace export
    /// (`shape.numel` with parameter `shape`). Lookup is by the Path's source
    /// spelling so the callee signature — including its error channel — is the
    /// imported function, not a method on the local value.
    fn check_namespace_function_call_from_path(
        &mut self,
        receiver: &HirExpression,
        name: radix_lexer::Symbol,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let HirExpressionKind::Path(module_def_id) = &receiver.kind else {
            return None;
        };
        let namespace = self.path_namespace_name(*module_def_id)?;
        let sig = self
            .resolver
            .namespace_file_function_canonical(namespace, name)
            .cloned()?;
        self.set_chorda_scalar_closure_context(name);
        let ret =
            self.check_instantiated_call(&sig, explicit_type_args, args, receiver.span, expected);
        self.validate_interval_bound_call_args(name, &sig, args);
        Some(ret)
    }

    /// Namespace spelling for a Path DefId.
    ///
    /// Import modules live in the resolver symbol table. A local parameter that
    /// shadows the import uses a HIR-local DefId (1_000_000+) and is recovered
    /// from [`TypeChecker::binding_names`].
    fn path_namespace_name(&self, def_id: radix_hir::DefId) -> Option<radix_lexer::Symbol> {
        self.resolver
            .get_symbol(def_id)
            .map(|symbol| symbol.name)
            .or_else(|| self.binding_names.get(&def_id).copied())
    }

    /// Checks method-call syntax after the receiver has been synthesized.
    ///
    /// Declared/interface methods are preferred. The array branch preserves a
    /// small built-in collection-method surface until stdlib-backed method
    /// metadata covers all call shapes. Unknown interface and concrete nominal
    /// members are hard diagnostics; only the explicit `ignotum` escape keeps
    /// unknown receiver methods permissive.
    pub fn check_method_call(
        &mut self,
        receiver: &mut HirExpression,
        name: &mut Symbol,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        expected: Option<TypeId>,
    ) -> TypeId {
        if let Some(ret) = self.check_namespace_function_call_from_path(
            receiver,
            *name,
            explicit_type_args,
            args,
            expected,
        ) {
            return ret;
        }
        let receiver_ty = self
            .check_non_null_method_receiver(receiver, *name)
            .unwrap_or_else(|| self.check_expr(receiver));
        if let Some(def_id) = self
            .struct_def_from_type(receiver_ty)
            .or_else(|| self.interface_def_from_type(receiver_ty))
        {
            if let Some(canonical_name) =
                self.resolver.imported_method_canonical_name(def_id, *name)
            {
                *name = canonical_name;
            }
        }
        let method_name = self.method_display_name(*name);
        let method_name = if method_name == super::tensor_index::GLYPH_TRANSPOSE_METHOD {
            if !explicit_type_args.is_empty() || !args.is_empty() {
                self.typecheck_call_arg_exprs(args);
                self.invalid_operand_types(
                    "glyph_transpose_suffix_takes_no_arguments",
                    receiver.span,
                );
                return self.error_type;
            }
            if let Some(issue) = super::tensor_index::glyph_transpose_rank_issue(self, receiver_ty)
            {
                self.invalid_operand_types(issue, receiver.span);
                return self.error_type;
            }
            // The parser interns the canonical and English pack spellings
            // alongside the private glyph marker. Rewriting to the active
            // pack's surface lets the ordinary intrinsic checker perform its
            // locale mapping while every backend still receives `transpone`.
            let surface = self
                .locale_pack
                .map_or(super::tensor_index::TENSOR_TRANSPONE, |pack| {
                    pack.intrinsic_surface(super::tensor_index::TENSOR_TRANSPONE)
                });
            if let Some(interner) = self.interner {
                if let Some(index) = interner.strings().iter().position(|s| s == surface) {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        *name = radix_lexer::Symbol(index as u32);
                    }
                }
            }
            surface.to_owned()
        } else {
            method_name
        };
        if let Some(result) = self.check_directional_view_method(
            receiver_ty,
            &method_name,
            explicit_type_args,
            args,
            receiver.span,
        ) {
            return result;
        }
        if let Some(result) = self.check_sermo_stream_view_method(
            receiver_ty,
            &method_name,
            explicit_type_args,
            args,
            receiver.span,
        ) {
            return result;
        }
        if let Some(result) = self.check_tensor_receiver_method_call(
            receiver_ty,
            &method_name,
            explicit_type_args,
            args,
            receiver.span,
        ) {
            return result;
        }
        if matches!(
            self.types.get(self.resolve_inferred_type(receiver_ty)),
            Type::Option(_)
        ) {
            self.typecheck_call_arg_exprs(args);
            self.undefined_member("optional_receiver_requires_chain", receiver.span);
            return self.error_type;
        }
        if let Some(sig) = self.lookup_method_signature(receiver_ty, *name) {
            if let Some(method) = self.lookup_interface_method_info(receiver_ty, *name) {
                let availability = method.availability.clone();
                let span = method.span;
                if self.report_nondum_method_call(*name, &availability, span, args) {
                    return self.error_type;
                }
            }
            self.set_chorda_scalar_closure_context(*name);
            let ret = self.check_instantiated_call(
                &sig,
                explicit_type_args,
                args,
                receiver.span,
                expected,
            );
            self.validate_interval_bound_call_args(*name, &sig, args);
            return ret;
        }

        // A member callee normally names a method, but lowering preserves the
        // same MethodCall shape for a function-valued struct field. Probe the
        // field only after method lookup so an explicit method always wins.
        if let Some(field_ty) = self.lookup_field_type(receiver_ty, *name) {
            if let Some(sig) = self.signature_from_type(field_ty) {
                return self.check_instantiated_call(
                    &sig,
                    explicit_type_args,
                    args,
                    receiver.span,
                    expected,
                );
            }
        }

        // U1R common-member rule: a `∪` receiver calls a method only when
        // every constituent declares it with one agreeing signature — a real
        // signature, not the permissive sink this path used to fall through
        // to. A miss names the constituents that lack the method.
        if let Some(members) = self.union_members_of(receiver_ty) {
            if let Some(sig) = self.union_common_method_signature(&members, *name) {
                self.set_chorda_scalar_closure_context(*name);
                let ret = self.check_instantiated_call(
                    &sig,
                    explicit_type_args,
                    args,
                    receiver.span,
                    expected,
                );
                self.validate_interval_bound_call_args(*name, &sig, args);
                return ret;
            }
            self.typecheck_call_arg_exprs(args);
            let lacking = self.union_lacking_constituents(&members, *name);
            self.undefined_member_with_args(
                ISSUE_UNION_MEMBER_NOT_COMMON,
                receiver.span,
                &[
                    ("member", self.method_display_name(*name)),
                    ("lacking", lacking.join(", ")),
                ],
            );
            return self.error_type;
        }

        let resolved_receiver_ty = self.resolve_inferred_type(receiver_ty);
        let (_, intrinsic_ty) = self.types.peel_access_marker(resolved_receiver_ty);
        let _resolved_receiver = self.types.get(intrinsic_ty);
        // S0-G2: applied aliases are value-transparent for intrinsic-method
        // dispatch — the receiver's builtin carrier owns the closed
        // tensor/vector/matrix method surface, including its unknown-method
        // rejection posture.
        let receiver_carrier = self.value_carrier_type(intrinsic_ty);
        let resolved_receiver = self.types.get(receiver_carrier);
        let receiver_is_atomic = matches!(resolved_receiver, Type::Atomic(_));
        let receiver_requires_known_method = matches!(
            resolved_receiver,
            Type::Tensor(_, _) | Type::Vector(_, _) | Type::Matrix(_, _)
        );
        let tensor_higher_order_method = matches!(resolved_receiver, Type::Tensor(_, _))
            && super::super::air_closure_lift::is_higher_order_method(&method_name);
        if super::intrinsics::receiver_supports_intrinsics(resolved_receiver) {
            if let Some(result) = super::intrinsics::check_intrinsic_method_call(
                self,
                receiver,
                receiver_ty,
                name,
                explicit_type_args,
                args,
                receiver.span,
            ) {
                return result;
            }
            self.typecheck_call_arg_exprs(args);
            if receiver_is_atomic {
                self.invalid_operand_types(
                    super::intrinsics::ISSUE_UNSUPPORTED_ATOMIC_METHOD,
                    receiver.span,
                );
                return self.error_type;
            }
            if radix_intrinsics::is_atomic_operator_method(&method_name) {
                self.invalid_operand_types(
                    super::intrinsics::ISSUE_ATOMIC_METHOD_REQUIRES_ATOMIC_RECEIVER,
                    receiver.span,
                );
                return self.error_type;
            }
            if receiver_requires_known_method && !tensor_higher_order_method {
                // Tensor, vector, and matrix methods are closed compiler-owned
                // surfaces. A registry miss must not manufacture an inferred
                // result that survives check and fails only during lowering.
                self.undefined_member("unknown_method", receiver.span);
                return self.error_type;
            }
            // ARC-3 higher-order Tensor methods are consumed by the AIR passes;
            // their callback/result typing belongs to that lane rather than the
            // neutral receiver registry. Preserve the pre-AIR inference path.
            return self.fresh_infer();
        }

        if let HirExpressionKind::Path(module_def_id) = &receiver.kind {
            if let Some(ret) = self.check_namespace_function_call_from_path(
                receiver,
                *name,
                explicit_type_args,
                args,
                expected,
            ) {
                return ret;
            }
            if let Some(namespace) = self
                .resolver
                .get_symbol(*module_def_id)
                .map(|symbol| symbol.name)
            {
                if !self
                    .resolver
                    .namespace_exports_member_canonical(namespace, *name)
                {
                    self.typecheck_call_arg_exprs(args);
                    if let Some(issue) = self
                        .resolver
                        .namespace_member_rejection_issue(namespace, *name)
                    {
                        self.errors.push(crate::passes::import_seam::diagnostic(
                            receiver.span,
                            issue,
                            &method_name,
                        ));
                    } else if self.resolver.namespace_has_file_interface(namespace) {
                        self.undefined_member_with_args(
                            "namespace_missing_export",
                            receiver.span,
                            &[("member", method_name.clone())],
                        );
                    } else {
                        self.undefined_member("unknown_method", receiver.span);
                    }
                    return self.error_type;
                }
            }
            if let Some(result) = self.check_tensor_namespace_method_call(
                *module_def_id,
                &method_name,
                explicit_type_args,
                args,
                receiver.span,
            ) {
                return result;
            }
        }

        let resolved_receiver_ty = self.resolve_inferred_type(receiver_ty);
        if matches!(
            self.types.get(resolved_receiver_ty),
            Type::Primitive(Primitive::Ignotum)
        ) {
            if self.reject_explicit_type_args_on_dynamic_callee(
                explicit_type_args,
                args,
                receiver.span,
                ISSUE_DYNAMIC_RECEIVER_METHOD_TYPE_ARGS,
            ) {
                return self.error_type;
            }
            self.typecheck_call_arg_exprs(args);
            return self.types.primitive(Primitive::Ignotum);
        }

        if self.interface_def_from_type(receiver_ty).is_some() {
            self.typecheck_call_arg_exprs(args);
            self.undefined_member("unknown_method", receiver.span);
            return self.error_type;
        }

        if self.reject_explicit_type_args_on_dynamic_callee(
            explicit_type_args,
            args,
            receiver.span,
            ISSUE_METHOD_CALL_TYPE_ARGS,
        ) {
            return self.error_type;
        }

        self.typecheck_call_arg_exprs(args);
        self.undefined_member("unknown_method", receiver.span);
        self.error_type
    }

    fn check_sermo_stream_view_method(
        &mut self,
        receiver_ty: TypeId,
        method_name: &str,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !self.is_sermo_receiver(receiver_ty) {
            return None;
        }
        let entry = radix_intrinsics::lookup_namespace_method_by_name("sermo", method_name)?;
        let method_label = entry.diagnostic_method.unwrap_or(entry.faber_name);
        if !args.is_empty() {
            self.typecheck_call_arg_exprs(args);
            self.wrong_arity_with_arg("method_accepts_no_arguments", span, "method", method_label);
            return Some(self.error_type);
        }
        if super::intrinsics::reject_catalog_arity(
            self,
            entry.generic,
            entry.arg_count,
            entry.missing_generic_issue,
            entry.excess_generic_issue,
            entry.value_arity_issue,
            entry.diagnostic_method,
            explicit_type_args,
            args,
            span,
        ) {
            return Some(self.error_type);
        }
        let type_params = super::intrinsics::interned_catalog_generics(self, entry.generic);
        let Some(param_name) = type_params.first().map(|param| param.name) else {
            return Some(self.error_type);
        };
        let ret = self.types.intern(Type::Param(param_name));
        let sig = FuncSig {
            type_params,
            params: Vec::new(),
            ret,
            err: None,
            is_async: false,
            is_generator: false,
        };
        let mut bind_args = Vec::new();
        let payload_ty =
            self.check_instantiated_call(&sig, explicit_type_args, &mut bind_args, span, None);
        let view_def = match entry.return_kind {
            radix_intrinsics::NamespaceReturnKind::SermoTuus => {
                crate::builtins::frame_types::tuus_def_id(self.resolver)
            }
            radix_intrinsics::NamespaceReturnKind::SermoMeus => {
                crate::builtins::frame_types::meus_def_id(self.resolver)
            }
            radix_intrinsics::NamespaceReturnKind::GenericTypus => return None,
        };
        let Some(view_def) = view_def else {
            self.missing_type_annotation("directional_view_type_not_registered", span);
            return Some(self.error_type);
        };
        Some(
            crate::builtins::frame_types::directional_view_payload_type_id(
                self.types, view_def, payload_ty,
            ),
        )
    }

    fn check_directional_view_method(
        &mut self,
        receiver_ty: TypeId,
        method_name: &str,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let (view_kind, payload_ty) = self.directional_view_payload(receiver_ty)?;
        if !explicit_type_args.is_empty() {
            self.invalid_operand_types(ISSUE_DIRECTIONAL_VIEW_METHOD_TYPE_ARGS, span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }

        let status_ty = crate::builtins::frame_types::status_type_id(self.types, self.resolver)
            .unwrap_or_else(|| self.types.primitive(Primitive::Ignotum));
        let vacuum_ty = self.types.primitive(Primitive::Vacuum);
        let nihil = self.types.primitive(Primitive::Nihil);

        match (view_kind, method_name) {
            (DirectionalViewKind::Meus, "da") => Some(self.check_known_method_args(
                args,
                vec![ParamType {
                    ty: payload_ty,
                    mode: SemanticParamMode::Move,
                    optional: false,
                }],
                vacuum_ty,
                span,
            )),
            (DirectionalViewKind::Meus, "fini") => {
                if !args.is_empty() {
                    self.typecheck_call_arg_exprs(args);
                    self.wrong_arity_with_arg(
                        "method_accepts_no_arguments",
                        span,
                        "method",
                        "meus.fini",
                    );
                    return Some(self.error_type);
                }
                Some(status_ty)
            }
            (DirectionalViewKind::Tuus, "accipe") => {
                if !args.is_empty() {
                    self.typecheck_call_arg_exprs(args);
                    self.wrong_arity_with_arg(
                        "method_accepts_no_arguments",
                        span,
                        "method",
                        "tuus.accipe",
                    );
                    return Some(self.error_type);
                }
                let Some(scrinium_def) =
                    crate::builtins::frame_types::scrinium_def_id(self.resolver)
                else {
                    self.missing_type_annotation("scrinium_type_not_registered", span);
                    return Some(self.error_type);
                };
                let frame_ty = crate::builtins::frame_types::scrinium_payload_type_id(
                    self.types,
                    scrinium_def,
                    payload_ty,
                );
                Some(self.types.intern(Type::Union(vec![frame_ty, nihil])))
            }
            (DirectionalViewKind::Tuus, "cursor") => {
                if !args.is_empty() {
                    self.typecheck_call_arg_exprs(args);
                    self.wrong_arity_with_arg(
                        "method_accepts_no_arguments",
                        span,
                        "method",
                        "tuus.cursor",
                    );
                    return Some(self.error_type);
                }
                let Some(scrinium_def) =
                    crate::builtins::frame_types::scrinium_def_id(self.resolver)
                else {
                    self.missing_type_annotation("scrinium_type_not_registered", span);
                    return Some(self.error_type);
                };
                let frame_ty = crate::builtins::frame_types::scrinium_payload_type_id(
                    self.types,
                    scrinium_def,
                    payload_ty,
                );
                Some(self.types.cursor(frame_ty))
            }
            (DirectionalViewKind::Tuus, "exhauri") => {
                if !args.is_empty() {
                    self.typecheck_call_arg_exprs(args);
                    self.wrong_arity_with_arg(
                        "method_accepts_no_arguments",
                        span,
                        "method",
                        "tuus.exhauri",
                    );
                    return Some(self.error_type);
                }
                Some(payload_ty)
            }
            (DirectionalViewKind::Tuus, "fini") => {
                if !args.is_empty() {
                    self.typecheck_call_arg_exprs(args);
                    self.wrong_arity_with_arg(
                        "method_accepts_no_arguments",
                        span,
                        "method",
                        "tuus.fini",
                    );
                    return Some(self.error_type);
                }
                Some(status_ty)
            }
            _ => None,
        }
    }

    fn directional_view_payload(
        &self,
        receiver_ty: TypeId,
    ) -> Option<(DirectionalViewKind, TypeId)> {
        let resolved = self.resolve_inferred_type(receiver_ty);
        let Type::Applied(base, args) = self.types.get(resolved) else {
            return None;
        };
        if args.len() != 1 {
            return None;
        }
        let Type::Struct(def_id) = self.types.get(*base) else {
            return None;
        };
        let radix_types::AppliedArg::Type(payload) = args[0] else {
            return None;
        };
        if Some(*def_id) == crate::builtins::frame_types::meus_def_id(self.resolver) {
            return Some((DirectionalViewKind::Meus, payload));
        }
        if Some(*def_id) == crate::builtins::frame_types::tuus_def_id(self.resolver) {
            return Some((DirectionalViewKind::Tuus, payload));
        }
        None
    }

    fn is_sermo_receiver(&self, receiver_ty: TypeId) -> bool {
        let Some(sermo_def_id) = crate::builtins::frame_types::sermo_def_id(self.resolver) else {
            return false;
        };
        matches!(
            self.types.get(self.resolve_inferred_type(receiver_ty)),
            Type::Struct(def_id) if *def_id == sermo_def_id
        )
    }

    // moves params into FuncSig
    pub fn check_known_method_args(
        &mut self,
        args: &mut [HirCallArg],
        params: Vec<ParamType>,
        ret: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        let sig = FuncSig {
            type_params: Vec::new(),
            params,
            ret,
            err: None,
            is_async: false,
            is_generator: false,
        };
        self.check_rigid_method_call(&sig, args, span)
    }

    /// Checks direct call syntax, including enum variant construction.
    ///
    /// Variant calls are constructors whose result is the parent enum type.
    /// Ordinary calls then follow the same signature/inference/`ignotum`
    /// recovery contract as calls from precomputed callee types.
    pub fn check_call(
        &mut self,
        callee: &mut HirExpression,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        expected: Option<TypeId>,
    ) -> TypeId {
        if !explicit_type_args.is_empty()
            && matches!(
                callee.kind,
                HirExpressionKind::Literal(
                    HirLiteral::String(_) | HirLiteral::Ascii(_) | HirLiteral::Octeti(_)
                )
            )
        {
            self.invalid_operand_types(ISSUE_TEMPLATE_APPLICATION_TYPE_ARGS, callee.span);
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }

        if matches!(
            callee.kind,
            HirExpressionKind::Literal(HirLiteral::Ascii(_) | HirLiteral::Octeti(_))
        ) {
            let issue = if matches!(
                callee.kind,
                HirExpressionKind::Literal(HirLiteral::Ascii(_))
            ) {
                ISSUE_ASCII_LITERAL_TEMPLATE_APPLICATION
            } else {
                ISSUE_OCTETI_LITERAL_TEMPLATE_APPLICATION
            };
            self.invalid_operand_types(issue, callee.span);
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }

        let non_null_method = match &callee.kind {
            HirExpressionKind::NonNull(_, radix_hir::HirNonNullKind::Member(name)) => Some(*name),
            _ => None,
        };
        if let Some(mut name) = non_null_method {
            return self.check_method_call(callee, &mut name, explicit_type_args, args, expected);
        }

        if let HirExpressionKind::Path(def_id) = &callee.kind {
            if self.resolver.gpu_builtin(*def_id).is_some() {
                if !explicit_type_args.is_empty() {
                    self.invalid_operand_types(ISSUE_GPU_BUILTIN_TYPE_ARGS, callee.span);
                }
                if !args.is_empty() {
                    self.typecheck_call_arg_exprs(args);
                    self.wrong_arity("gpu_builtin_expects_no_arguments", callee.span);
                    return self.error_type;
                }
                if !self.current_device_function {
                    self.invalid_operand_types(
                        ISSUE_GPU_BUILTIN_DEVICE_FUNCTION_REQUIRED,
                        callee.span,
                    );
                    return self.error_type;
                }
                if let Some(symbol) = self.resolver.get_symbol(*def_id) {
                    if let Some(Type::Func(sig)) = symbol.ty.as_ref().map(|ty| self.types.get(*ty))
                    {
                        return sig.ret;
                    }
                }
                // WHY: registration always attaches a Func signature; this fallback keeps checking alive if lookup fails.
                return self
                    .types
                    .sized_numeric(Primitive::Numerus, NumericWidth::U32);
            }

            if let Some(sig) = self.functions.get(def_id).cloned() {
                if let Some(name) = self.resolver.get_symbol(*def_id).map(|symbol| symbol.name) {
                    self.set_chorda_scalar_closure_context(name);
                    let ret = self.check_instantiated_call(
                        &sig,
                        explicit_type_args,
                        args,
                        callee.span,
                        expected,
                    );
                    self.validate_interval_bound_call_args(name, &sig, args);
                    return ret;
                }
                return self.check_instantiated_call(
                    &sig,
                    explicit_type_args,
                    args,
                    callee.span,
                    expected,
                );
            }

            if let Some(parent) = self.variant_parent.get(def_id).copied() {
                if !explicit_type_args.is_empty() && !self.nominal_is_generic(parent) {
                    self.invalid_operand_types(ISSUE_VARIANT_CONSTRUCTOR_TYPE_ARGS, callee.span);
                }
                let (Some(fields), Some(field_names)) = (
                    self.variant_fields.get(def_id).cloned(),
                    self.variant_field_names.get(def_id).cloned(),
                ) else {
                    self.error_issue(
                        SemanticErrorKind::VariableUndefined,
                        "unknown_variant_fields",
                        callee.span,
                    );
                    self.typecheck_call_arg_exprs(args);
                    return self.error_type;
                };
                if args.len() != fields.len() {
                    self.wrong_arity("wrong_number_of_arguments", callee.span);
                }
                // U-D1: generic union construction instantiates type params.
                // Field payloads are checked against the declared field types
                // with the union's params substituted by the construction's
                // witnesses (explicit type args, the enclosing applied expected
                // type, or fresh inference variables) — never the raw
                // `Type::Param`. The result is the applied union type.
                let witnesses =
                    self.constructor_witnesses(parent, explicit_type_args, expected, callee.span);
                let mut vars = FxHashMap::default();
                if let Some(params) = self.type_params_by_def.get(&parent) {
                    for (param, witness) in params.iter().zip(witnesses.iter()) {
                        vars.insert(*param, *witness);
                    }
                }
                let field_types: Vec<TypeId> = fields
                    .iter()
                    .map(|field_ty| self.substitute_type_params(*field_ty, &mut vars, None))
                    .collect();
                if args.iter().any(|arg| arg.name.is_some()) {
                    let mut seen = FxHashSet::default();
                    for arg in args {
                        let Some(name) = arg.name else {
                            let arg_ty = self.check_expr(&mut arg.expr);
                            self.unify_assign_issue(
                                arg_ty,
                                self.error_type,
                                arg.span,
                                ISSUE_ARGUMENT_TYPE_MISMATCH,
                            );
                            self.undefined_member("missing_variant_field_name", arg.span);
                            continue;
                        };
                        if !seen.insert(name) {
                            self.undefined_member("duplicate_variant_field", arg.span);
                        }
                        let Some(index) = field_names.iter().position(|field| *field == name)
                        else {
                            self.check_expr(&mut arg.expr);
                            self.undefined_member("unknown_variant_field", arg.span);
                            continue;
                        };
                        if let Some(field_ty) = field_types.get(index).copied() {
                            let arg_ty =
                                self.check_expr_with_expected(&mut arg.expr, Some(field_ty));
                            self.unify_assign_issue(
                                arg_ty,
                                field_ty,
                                arg.span,
                                ISSUE_ARGUMENT_TYPE_MISMATCH,
                            );
                        }
                    }
                } else {
                    for (arg, field_ty) in args.iter_mut().zip(field_types.iter()) {
                        let arg_ty = self.check_expr_with_expected(&mut arg.expr, Some(*field_ty));
                        self.unify_assign_issue(
                            arg_ty,
                            *field_ty,
                            arg.span,
                            ISSUE_ARGUMENT_TYPE_MISMATCH,
                        );
                    }
                }
                return if self.nominal_is_generic(parent) {
                    let base = self.types.intern(Type::Enum(parent));
                    let witnesses = witnesses
                        .into_iter()
                        .map(radix_types::AppliedArg::Type)
                        .collect();
                    self.types.intern(Type::Applied(base, witnesses))
                } else {
                    self.types.intern(Type::Enum(parent))
                };
            }
        }

        let callee_ty = self.check_expr(callee);

        let resolved = self.resolve_inferred_type(callee_ty);
        if let Some(sig) = self.signature_from_type(resolved) {
            return self.check_instantiated_call(
                &sig,
                explicit_type_args,
                args,
                callee.span,
                expected,
            );
        }

        if let Some(result) =
            self.check_dynamic_callee_call(resolved, explicit_type_args, args, callee.span)
        {
            return result;
        }

        self.not_callable("callee_not_callable", callee.span);
        self.error_type
    }

    fn conversion_absent_diagnostic(
        &mut self,
        source_ty: TypeId,
        destination_ty: TypeId,
        span: radix_lexer::Span,
    ) {
        let mut args = vec![
            (
                "source",
                self.format_conversion_type_for_diagnostic(source_ty),
            ),
            (
                "destination",
                self.format_conversion_type_for_diagnostic(destination_ty),
            ),
        ];
        // CTR-04: names are the ambiguity escape — when the pair names a
        // helper enrolled in a non-visible scope, suggest it explicitly.
        if let Some(name) = self
            .resolver
            .conversion_name_suggestion(source_ty, destination_ty)
            .and_then(|name| {
                self.interner
                    .map(|interner| interner.resolve(name).to_owned())
            })
        {
            args.push(("suggestion", name));
        }
        self.invalid_operand_types_with_args(ISSUE_FAILABLE_CALL_CONVERSION_ABSENT, span, &args);
    }

    pub(crate) fn format_union_constituent_type(&self, ty: TypeId) -> String {
        let Some(interner) = self.interner else {
            return format!("{:?}", self.types.get(ty));
        };
        format_faber_type(ty, self.types, interner, &|def_id| {
            self.resolver.get_symbol(def_id).map_or_else(
                || format!("Def{}", def_id.0),
                |symbol| interner.resolve(symbol.name).to_owned(),
            )
        })
    }

    /// One agreeing signature for a method every union constituent declares
    /// (U1R common-member rule).
    ///
    /// Same-name methods with incompatible signatures across constituents
    /// are simply not common members — the computation falls out, no extra
    /// rule. Heritage-merged views are consulted through the ordinary
    /// per-constituent `lookup_method_signature`.
    fn union_common_method_signature(&self, members: &[TypeId], name: Symbol) -> Option<FuncSig> {
        let mut common: Option<FuncSig> = None;
        for member in members {
            let sig = self.lookup_method_signature(*member, name)?;
            match &common {
                None => common = Some(sig),
                Some(existing) => {
                    if !self.func_sigs_agree(existing, &sig) {
                        return None;
                    }
                }
            }
        }
        common
    }

    /// Whether two declared signatures expose one callable contract (U1R).
    ///
    /// Compared constituent-to-constituent for the common-member rule:
    /// parameter count, modes, optionality, parameter/return/error-channel
    /// types, and the async/generator markers. Type arguments are compared
    /// alias-transparently through `TypeTable::equals`.
    fn func_sigs_agree(&self, lhs: &FuncSig, rhs: &FuncSig) -> bool {
        if lhs.params.len() != rhs.params.len()
            || lhs.is_async != rhs.is_async
            || lhs.is_generator != rhs.is_generator
        {
            return false;
        }
        match (lhs.err, rhs.err) {
            (None, None) => {}
            (Some(lhs_err), Some(rhs_err)) => {
                if !self.member_type_equals(lhs_err, rhs_err) {
                    return false;
                }
            }
            _ => return false,
        }
        if !self.member_type_equals(lhs.ret, rhs.ret) {
            return false;
        }
        lhs.params.iter().zip(rhs.params.iter()).all(|(l, r)| {
            l.mode == r.mode && l.optional == r.optional && self.member_type_equals(l.ty, r.ty)
        })
    }

    /// Display names of the union constituents that do not declare `name`
    /// (U1R attribution for the common-member miss).
    fn union_lacking_constituents(&self, members: &[TypeId], name: Symbol) -> Vec<String> {
        members
            .iter()
            .copied()
            .filter(|member| self.lookup_method_signature(*member, name).is_none())
            .map(|member| self.display_union_constituent(member))
            .collect()
    }

    /// Union-of-nominal guardrail warnings at a written `∪` annotation
    /// (U1R guardrails 1, 3, 4).
    ///
    /// Guardrail 1 teaches the member rule whenever a written union has two
    /// or more nominal members. Guardrails 3 and 4 compute over the
    /// heritage-merged member views and stay confined to all-nominal unions
    /// — a mixed or primitive union has no honest declared-member view.
    pub fn warn_union_annotation_guardrails(
        &mut self,
        annotation_ty: TypeId,
        span: radix_lexer::Span,
    ) {
        let Some(members) = self.union_members_of(annotation_ty) else {
            return;
        };
        let nominal: Vec<TypeId> = members
            .iter()
            .copied()
            .filter(|member| self.constituent_is_nominal(*member))
            .collect();
        if nominal.len() < 2 {
            return;
        }
        let display = members
            .iter()
            .map(|member| self.display_union_constituent(*member))
            .collect::<Vec<_>>()
            .join(" ∪ ");
        // Guardrail 1: teach the member rule at the moment of writing.
        self.push_warning(
            WarningKind::UnionOfNominalMembers,
            ISSUE_UNION_NOMINAL_MEMBERS,
            span,
            &[("union", display.clone())],
        );
        // Guardrail 3: an all-nominal union sharing no members is store-only.
        if self.union_common_member_names(&members).is_empty() {
            self.push_warning(
                WarningKind::UnionNoCommonMembers,
                ISSUE_UNION_NO_COMMON_MEMBERS,
                span,
                &[("union", display)],
            );
            return;
        }
        // Guardrail 4: same-name methods with incompatible signatures drop
        // out of the common set silently — name the collision.
        for name in self.union_shared_method_names(&members) {
            let signatures: Vec<Option<FuncSig>> = members
                .iter()
                .copied()
                .map(|member| self.lookup_method_signature(member, name))
                .collect();
            let mut agree = true;
            let first = signatures.iter().find_map(std::clone::Clone::clone);
            let Some(first) = first else {
                continue;
            };
            for sig in signatures.iter().flatten() {
                if !self.func_sigs_agree(&first, sig) {
                    agree = false;
                    break;
                }
            }
            if !agree {
                let method = self
                    .interner
                    .map(|interner| interner.resolve(name).to_owned())
                    .unwrap_or_default();
                self.push_warning(
                    WarningKind::UnionMethodSignatureShadowed,
                    ISSUE_UNION_METHOD_SIGNATURE_SHADOWED,
                    span,
                    &[("method", method), ("union", display.clone())],
                );
            }
        }
    }

    /// Push one structured warning into the diagnostics stream.
    pub(crate) fn push_warning(
        &mut self,
        kind: WarningKind,
        issue: &'static str,
        span: radix_lexer::Span,
        args: &[(&'static str, String)],
    ) {
        let mut error = SemanticError::structured(SemanticErrorKind::Warning(kind), span)
            .with_arg("issue", issue);
        for (name, value) in args {
            error = error.with_arg(name, value.clone());
        }
        self.errors.push(error);
    }

    /// Whether a union constituent is a nominal user type (U1R guardrails).
    fn constituent_is_nominal(&self, member: TypeId) -> bool {
        self.struct_def_from_type(member).is_some()
            || self.interface_def_from_type(member).is_some()
            || self.enum_def_from_type(member).is_some()
    }

    /// Declared member names of one union constituent (fields ∪ methods).
    fn constituent_member_names(&self, member: TypeId) -> Vec<Symbol> {
        let mut names: Vec<Symbol> = Vec::new();
        if let Some(struct_def) = self.struct_def_from_type(member) {
            if let Some(info) = self.structs.get(&struct_def) {
                names.extend(info.fields.keys().copied());
                names.extend(info.methods.keys().copied());
            }
            return names;
        }
        if let Some(interface_def) = self.interface_def_from_type(member) {
            if let Some(methods) = self.interfaces.get(&interface_def) {
                names.extend(methods.keys().copied());
            }
            return names;
        }
        if let Some(enum_def) = self.enum_def_from_type(member) {
            names.extend(
                self.resolver
                    .commune_fields(enum_def)
                    .iter()
                    .map(|f| f.name),
            );
        }
        names
    }

    /// Member names every constituent of the union declares (U1R guardrail 3).
    fn union_common_member_names(&self, members: &[TypeId]) -> Vec<Symbol> {
        let mut sets: Vec<rustc_hash::FxHashSet<Symbol>> = members
            .iter()
            .map(|member| self.constituent_member_names(*member).into_iter().collect())
            .collect();
        let Some(first) = sets.pop() else {
            return Vec::new();
        };
        first
            .into_iter()
            .filter(|name| sets.iter().all(|set| set.contains(name)))
            .collect()
    }

    /// Method names two or more constituents of the union declare (U1R
    /// guardrail 4 candidates).
    fn union_shared_method_names(&self, members: &[TypeId]) -> Vec<Symbol> {
        let mut count: rustc_hash::FxHashMap<Symbol, usize> = rustc_hash::FxHashMap::default();
        for member in members {
            for name in self.constituent_member_names(*member) {
                if self.lookup_method_signature(*member, name).is_some() {
                    *count.entry(name).or_insert(0) += 1;
                }
            }
        }
        count
            .into_iter()
            .filter(|(_, hits)| *hits >= 2)
            .map(|(name, _)| name)
            .collect()
    }

    fn format_conversion_type_for_diagnostic(&self, ty: TypeId) -> String {
        self.format_union_constituent_type(ty)
    }

    /// Enforces alternate-exit handling for functions that can produce errors.
    ///
    /// The function signature carries the contract: a failable call propagates
    /// to the nearest alternate-exit sink, matching how bare `↦` conversio
    /// (`convert.rs`) and `iace` throws (`expr.rs`) already behave. A local
    /// `fac`/`cape` sink or a function-level `⇥` declaration both accept the
    /// call after unifying the thrown error type with the sink's expectation.
    /// Neither sink in scope — a bare expression with no propagation context —
    /// is the only case rejected here.
    pub fn reject_failable_call(&mut self, sig: &FuncSig, span: radix_lexer::Span) -> bool {
        let Some(err_ty) = sig.err else {
            return false;
        };
        if sig.is_async {
            return false;
        }

        match self.current_error {
            Some(ErrorSink::Local(handler_ty)) => {
                self.unify_assign_issue(
                    err_ty,
                    handler_ty,
                    span,
                    ISSUE_FAILABLE_CALL_HANDLER_ERROR_TYPE_MISMATCH,
                );
                return false;
            }
            // UT — the third legal answer: the `capta` capture boundary. The
            // trap hole accepts every failable call's error type by
            // accumulation through the existing U-02 arms; no new inference,
            // no channel-specific typing.
            Some(ErrorSink::Trap(var)) => {
                let hole = self.trap_sink_type(var);
                self.unify_assign_issue(
                    err_ty,
                    hole,
                    span,
                    super::control::ISSUE_TRAP_MEMBER_TYPE_MISMATCH,
                );
                return false;
            }
            Some(ErrorSink::Function(func_err_ty)) => {
                let source_ty = self.resolve_inferred_type(err_ty);
                let destination_ty = self.resolve_inferred_type(func_err_ty);
                if !self.types.is_assignable(source_ty, destination_ty) {
                    // CTR-02: registered rows are the authority for their
                    // ordered pair (direct lookup only). A row whose
                    // conversion function declares `⇥ E` composes that
                    // declared channel through the destination channel.
                    if let Some(row) = self.resolver.conversion_row_for(source_ty, destination_ty) {
                        if let Some(fn_id) = row.fn_def_id {
                            if let Some(channel) =
                                self.functions.get(&fn_id).and_then(|sig| sig.err)
                            {
                                if !self.check_registered_row_channel(channel, span) {
                                    return false;
                                }
                            }
                            self.pending_conversion = Some(fn_id);
                            return false;
                        }
                    }
                    self.conversion_absent_diagnostic(source_ty, destination_ty, span);
                    return false;
                }
                self.unify_assign_issue(
                    err_ty,
                    func_err_ty,
                    span,
                    ISSUE_FAILABLE_CALL_ENCLOSING_ERROR_TYPE_MISMATCH,
                );
                return false;
            }
            _ => {}
        }

        self.invalid_operand_types(ISSUE_FAILABLE_CALL_REQUIRES_HANDLER, span);
        true
    }

    /// Checks the `⇇` transfer chain (EOT-3M).
    ///
    /// Ordinary call semantics with two sink constraints: the sink must be a
    /// unary callable returning `vacuum`. Each payload is one ordinary call
    /// argument against the single parameter (per-payload generic
    /// instantiation, exactly like a direct call per payload), and the whole
    /// expression is `vacuum`. Diagnostics are target-neutral: no console,
    /// formatting, or channel wording.
    pub fn check_transfer(
        &mut self,
        sink: &mut HirExpression,
        payloads: &mut [HirExpression],
        span: radix_lexer::Span,
    ) -> TypeId {
        let vacuum = self.vacuum_type();
        let sink_ty = self.check_expr(sink);
        let resolved = self.resolve_inferred_type(sink_ty);

        if self.is_infer(resolved) {
            // Provisional shape, mirroring inferred ordinary callees: a unary
            // callable from the first payload's type to `vacuum`.
            let mut param_ty = self.fresh_infer();
            for (index, payload) in payloads.iter_mut().enumerate() {
                let payload_ty = self.check_expr(payload);
                if index == 0 {
                    param_ty = payload_ty;
                }
            }
            let sig = FuncSig {
                type_params: Vec::new(),
                params: vec![ParamType {
                    ty: param_ty,
                    mode: SemanticParamMode::Owned,
                    optional: false,
                }],
                ret: vacuum,
                err: None,
                is_async: false,
                is_generator: false,
            };
            let func_ty = self.types.function(sig);
            self.unify_assign_issue(sink_ty, func_ty, span, ISSUE_TRANSFER_SINK_NOT_CALLABLE);
            return vacuum;
        }

        if matches!(
            self.types.get(resolved),
            Type::Primitive(Primitive::Ignotum)
        ) {
            for payload in payloads {
                self.check_expr(payload);
            }
            return vacuum;
        }

        let Some(sig) = self.signature_from_type(resolved) else {
            for payload in payloads {
                self.check_expr(payload);
            }
            self.not_callable(ISSUE_TRANSFER_SINK_NOT_CALLABLE, span);
            return self.error_type;
        };

        let unary = sig.params.len() == 1 && !sig.params[0].optional;
        if !unary {
            self.invalid_operand_types(ISSUE_TRANSFER_SINK_MUST_BE_UNARY, span);
        }
        let ret_vacuum = matches!(
            self.types.get(self.resolve_inferred_type(sig.ret)),
            Type::Primitive(Primitive::Vacuum)
        );
        if !ret_vacuum {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_TRANSFER_SINK_MUST_RETURN_VACUUM,
                span,
            );
        }
        if !unary {
            for payload in payloads {
                self.check_expr(payload);
            }
            return self.error_type;
        }

        for payload in payloads {
            // One ordinary call per payload: fresh generic instantiation so
            // each payload drives its own inference, like separate calls.
            let mut vars = FxHashMap::default();
            let instantiated = self.substitute_func_sig(&sig, &mut vars, None);
            let param = &instantiated.params[0];
            let payload_ty = self.check_expr_with_expected(payload, Some(param.ty));
            if self.types.is_ref_view(payload_ty)
                && (matches!(
                    param.mode,
                    SemanticParamMode::MutRef | SemanticParamMode::Move
                ) || self.types.is_mut_or_own_view(param.ty))
            {
                self.error_issue(
                    SemanticErrorKind::ModeMismatch,
                    crate::ISSUE_REF_TO_MUT_OR_OWN_POSITION,
                    payload.span,
                );
            }
            self.unify_carrier_assign(
                payload_ty,
                param.ty,
                payload.span,
                ISSUE_TRANSFER_PAYLOAD_TYPE_MISMATCH,
            );
        }

        // Failable sinks propagate exactly like ordinary failable calls.
        if self.reject_failable_call(&sig, span) {
            return self.error_type;
        }
        vacuum
    }

    /// Whether a nominal def declares generic type parameters.
    pub fn nominal_is_generic(&self, def_id: DefId) -> bool {
        self.type_params_by_def
            .get(&def_id)
            .is_some_and(|params| !params.is_empty())
    }

    /// Resolve the applied witness arguments for a generic nominal construction.
    ///
    /// Priority: explicit type arguments > the enclosing expected applied type
    /// of the same base > fresh inference variables per parameter. Explicit
    /// arity mismatches are reported and the list is truncated/padded so field
    /// checking can still proceed with the available witnesses.
    pub fn constructor_witnesses(
        &mut self,
        def_id: DefId,
        explicit_type_args: &[TypeId],
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> Vec<TypeId> {
        let params = self
            .type_params_by_def
            .get(&def_id)
            .cloned()
            .unwrap_or_default();
        if params.is_empty() {
            return Vec::new();
        }
        let arity = params.len();
        if !explicit_type_args.is_empty() {
            if explicit_type_args.len() != arity {
                self.wrong_arity("wrong_number_of_type_arguments", span);
            }
            return explicit_type_args
                .iter()
                .copied()
                .take(arity)
                .chain((0..arity).map(|_| self.fresh_infer()))
                .collect();
        }
        if let Some(args) = self.applied_args_for_def(expected, def_id) {
            if args.len() == arity {
                return args;
            }
        }
        (0..arity).map(|_| self.fresh_infer()).collect()
    }

    /// When `ty` resolves to `Type::Applied(<def_id>, args)`, return the args.
    pub fn applied_args_for_def(&self, ty: Option<TypeId>, def_id: DefId) -> Option<Vec<TypeId>> {
        let ty = ty?;
        let resolved = self.resolve_inferred_type(ty);
        let Type::Applied(base, args) = self.types.get(resolved) else {
            return None;
        };
        let base_def = match self.types.get(self.resolve_inferred_type(*base)) {
            Type::Enum(def) | Type::Struct(def) => Some(*def),
            _ => None,
        };
        if base_def != Some(def_id) {
            return None;
        }
        args.iter()
            .map(|arg| match arg {
                radix_types::AppliedArg::Type(id) => Some(*id),
                radix_types::AppliedArg::Size(_) => None,
            })
            .collect()
    }
}
