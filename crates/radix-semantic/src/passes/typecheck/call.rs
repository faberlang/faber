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

use super::member_privacy::MemberAccess;
use super::store::{StoreSite, argument_site};
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use crate::format_faber_type;
use radix_hir::{HirCallArg, HirStoreKind};

enum DirectionalViewKind {
    Meus,
    Tuus,
}

const ISSUE_ASCII_LITERAL_TEMPLATE_APPLICATION: &str = "ascii_literal_template_application";
const ISSUE_ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";
const ISSUE_MUT_ARGUMENT_TYPE_MISMATCH: &str = "mut_argument_type_mismatch";
/// K5 (gpu-reset rule 15): a function over one number called on a tensor.
pub(crate) const ISSUE_TENSOR_FUNCTION_NOT_LIFTED: &str = "tensor_function_not_lifted";
const ISSUE_CALLEE_NOT_CALLABLE: &str = "callee_not_callable";
/// R1: the two sides of `⇇` differ in element type or shape.
pub(crate) const ISSUE_COPY_INTO_TYPE_MISMATCH: &str = "copy_into_type_mismatch";
/// R2: the left side of `⇇` is not tensor storage (scalar, list, matrix, ...).
pub(crate) const ISSUE_COPY_INTO_TARGET_NOT_TENSOR: &str = "copy_into_target_not_tensor";
/// R4/R5: a `⇇` statement has no result, so it is not a value to store.
pub(crate) const ISSUE_COPY_INTO_VALUE_USED: &str = "copy_into_value_used";
/// Sinking rule 8: the left side of `⇜` does not conform to `Sink<T>`.
pub(crate) const ISSUE_SINK_INTO_RECEIVER_NOT_SINK: &str = "sink_into_receiver_not_sink";
/// Sinking rule 3: the item is not exactly the sink's `T`.
pub(crate) const ISSUE_SINK_INTO_ITEM_TYPE_MISMATCH: &str = "sink_into_item_type_mismatch";
/// Sinking rule 6: `⇜` into a sink reached through a `let` (const) name is a
/// mutation of what that name reaches; it carries the same identity as the
/// mutating-intrinsic gate.
const ISSUE_MUTATING_RECEIVER_THROUGH_CONST: &str = "mutating_receiver_through_const";
/// Sinking rule 5: a `⇜` statement has no result, so it is not a value to store.
pub(crate) const ISSUE_SINK_INTO_VALUE_USED: &str = "sink_into_value_used";
/// R7: the callable-sink form `sink ⇇ payload` is retired.
// FABER-2-SUNSET: delete with the retired-word diagnostics.
pub(crate) const ISSUE_TRANSFER_SINK_RETIRED: &str = "transfer_sink_retired";
pub(super) const ISSUE_DIRECTIONAL_VIEW_METHOD_TYPE_ARGS: &str =
    "directional_view_method_type_args";
const ISSUE_DYNAMIC_CALLEE_TYPE_ARGS: &str = "dynamic_callee_type_args";
const ISSUE_DYNAMIC_RECEIVER_METHOD_TYPE_ARGS: &str = "dynamic_receiver_method_type_args";
const ISSUE_FAILABLE_CALL_ENCLOSING_ERROR_TYPE_MISMATCH: &str =
    "failable_call_enclosing_error_type_mismatch";
pub(crate) const ISSUE_FAILABLE_CALL_CONVERSION_ABSENT: &str = "failable_call_conversion_absent";
const ISSUE_FAILABLE_CALL_HANDLER_ERROR_TYPE_MISMATCH: &str =
    "failable_call_handler_error_type_mismatch";
pub(super) const ISSUE_FAILABLE_CALL_REQUIRES_HANDLER: &str = "failable_call_requires_handler";
const ISSUE_GPU_BUILTIN_DEVICE_FUNCTION_REQUIRED: &str = "gpu_builtin_device_function_required";
const ISSUE_GPU_BUILTIN_TYPE_ARGS: &str = "gpu_builtin_type_args";
const ISSUE_INFERRED_CALLEE_TYPE_ARGS: &str = "inferred_callee_type_args";
const ISSUE_METHOD_CALL_TYPE_ARGS: &str = "method_call_type_args";
const ISSUE_OCTETI_LITERAL_TEMPLATE_APPLICATION: &str = "octeti_literal_template_application";
const ISSUE_TEMPLATE_APPLICATION_TYPE_ARGS: &str = "template_application_type_args";
const ISSUE_UNION_MEMBER_NOT_COMMON: &str = "union_member_not_common";
const ISSUE_UNION_NO_COMMON_MEMBERS: &str = "union_no_common_members";
const ISSUE_UNION_MEMBER_DIFFERS: &str = "union_member_differs";
const ISSUE_UNION_METHOD_SIGNATURE_SHADOWED: &str = "union_method_signature_shadowed";
const ISSUE_VARIANT_CONSTRUCTOR_TYPE_ARGS: &str = "variant_constructor_type_args";

/// The argument of an `axis` parameter was written without the `axis` word.
pub(crate) const ISSUE_AXIS_MARKER_REQUIRED: &str = "axis_marker_required";
/// `axis` was written before an argument whose parameter is not `axis`.
pub(crate) const ISSUE_AXIS_MARKER_UNEXPECTED: &str = "axis_marker_unexpected";

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
            if self
                .tensor_edge_policies
                .shifted_expressions
                .contains(&arg.expr.id)
            {
                self.invalid_operand_types(
                    super::intrinsics::ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION,
                    arg.span,
                );
            }
        }
    }

    /// Typechecks each call argument exactly once without callee expected context.
    fn typecheck_call_args_once(&mut self, args: &mut [HirCallArg]) -> Vec<ParamType> {
        args.iter_mut()
            .map(|arg| ParamType {
                ty: self.check_expr(&mut arg.expr),
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
            })
            .collect()
    }

    /// Whether `method` (canonical or the active locale's surface spelling) is
    /// on the registry surface of the bare float of `width`.
    fn is_float_registry_method(&self, width: NumericWidth, method: &str) -> bool {
        let bare = Type::SizedNumeric(Primitive::Fractus, width);
        radix_intrinsics::receivers_for_type(&bare)
            .iter()
            .any(|family| {
                radix_intrinsics::lookup_method_named(*family, method).is_some()
                    || self
                        .locale_pack
                        .is_some_and(|pack| pack.intrinsic_canonical(*family, method).is_some())
            })
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

    /// Reports SEM017 `nondum_function_for_target` for a use of a free
    /// function whose `@ nondum` gates cover this target.
    ///
    /// A gate without a target covers every target. `name` is the spelling the
    /// source used (`parse`, or `toml.parse` through a namespace). Returns
    /// `true` when the diagnostic was emitted.
    pub(super) fn report_unavailable_function(
        &mut self,
        gates: &[crate::file_interface::InterfaceUnavailable],
        name: impl Into<String>,
        span: radix_lexer::Span,
    ) -> bool {
        let Some(gate) = crate::file_interface::InterfaceUnavailable::applicable(gates, |target| {
            self.target_name_matches(target)
        }) else {
            return false;
        };
        let reason = gate.reason.clone();
        let mut error = SemanticError::structured(SemanticErrorKind::UnavailableDeclaration, span)
            .with_arg("issue", "nondum_function_for_target")
            .with_arg("function", name);
        if let Some(reason) = reason {
            error = error.with_arg("reason", reason);
        }
        self.errors.push(error);
        true
    }

    /// Reports SEM017 `nondum_function_for_target` for a call of a `genus`
    /// method marked `@ nondum` on this target. `receiver_ty` is the type the
    /// method is called on; the diagnostic names the method as `Owner.method`.
    /// Returns `true` when the diagnostic was emitted.
    pub(super) fn report_unavailable_method(
        &mut self,
        receiver_ty: TypeId,
        name: Symbol,
        span: radix_lexer::Span,
    ) -> bool {
        if self.unavailable_methods.is_empty() {
            return false;
        }
        let Some(owner) = self.struct_def_from_type(receiver_ty) else {
            return false;
        };
        let Some(gates) = self.unavailable_methods.get(&(owner, name)).cloned() else {
            return false;
        };
        let owner_name = self.resolver.get_symbol(owner).map_or_else(
            || "genus".to_owned(),
            |symbol| self.method_display_name(symbol.name),
        );
        let qualified = format!("{owner_name}.{}", self.method_display_name(name));
        self.report_unavailable_function(&gates, qualified, span)
    }

    /// `namespace.member` spelling for diagnostics about an imported function.
    pub(super) fn qualified_function_name(&self, namespace: Symbol, member: Symbol) -> String {
        format!(
            "{}.{}",
            self.method_display_name(namespace),
            self.method_display_name(member)
        )
    }

    /// Reports SEM017 for a use of the local function or selective-import
    /// binding `def_id` when it is marked `@ nondum` for this target.
    pub(super) fn report_unavailable_path(
        &mut self,
        def_id: DefId,
        span: radix_lexer::Span,
    ) -> bool {
        if self.unavailable_functions.is_empty() && !self.resolver.has_unavailable_bindings() {
            return false;
        }
        let Some(gates) = self
            .unavailable_functions
            .get(&def_id)
            .cloned()
            .or_else(|| self.resolver.binding_unavailable(def_id).map(<[_]>::to_vec))
        else {
            return false;
        };
        let name = self.resolver.get_symbol(def_id).map_or_else(
            || "function".to_owned(),
            |symbol| self.method_display_name(symbol.name),
        );
        self.report_unavailable_function(&gates, name, span)
    }

    /// Builds a provisional function signature from actual arguments.
    ///
    /// This is an inference device, not a declaration model. Each argument is
    /// checked once, then the callee's inference variable can be unified with a
    /// callable type that reflects the use site.
    pub fn build_call_signature(&mut self, args: &mut [HirCallArg]) -> FuncSig {
        let params = self.typecheck_call_args_once(args);
        FuncSig {
            type_params: Vec::new(),
            params,
            ret: self.fresh_infer(),
            err: None,
            is_async: false,
            is_generator: false,
        }
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
            return self.check_instantiated_call_marked(
                &sig,
                explicit_type_args,
                args,
                span,
                None,
                None,
            );
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
        let errors_before = self.errors.len();
        let arg_ty = self.check_expr(&mut arg.expr);
        let resolved = self.resolve_inferred_type(arg_ty);
        let inner = match self.types.get(resolved) {
            Type::Array(inner) if !sig.params.iter().any(|param| param.optional) => *inner,
            _ => {
                // WHY: declining the compat mode sends the argument back
                // through the per-parameter loop, which checks it again;
                // drop this pass's diagnostics so each one is reported once.
                self.errors.truncate(errors_before);
                return false;
            }
        };
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
        self.check_call_args_lifting(sig, args, span, false, false, None);
    }

    /// [`Self::check_call_args`] for a call that may be a lifting attempt.
    ///
    /// `user_function` is true for a call to a user-defined or library
    /// function (never an intrinsic row): passing a tensor where the function
    /// takes one number is then the K5 diagnostic (gpu-reset rule 15), not a
    /// bare argument mismatch. Returns whether a lifting attempt was
    /// diagnosed, so the caller can stop the result-type cascade.
    pub(super) fn check_call_args_lifting(
        &mut self,
        sig: &FuncSig,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        user_function: bool,
        markers: bool,
        axis: Option<&[bool]>,
    ) -> bool {
        let mut lifted = false;
        let required = sig.params.iter().filter(|param| !param.optional).count();
        let spread_compat = args.len() == 1
            && sig.params.len() > 1
            && self.check_spread_array_compat(sig, &mut args[0]);
        if !spread_compat && (args.len() < required || args.len() > sig.params.len()) {
            self.wrong_arity("wrong_number_of_arguments", span);
        }
        if spread_compat {
            return false;
        }

        for (argument, (arg, param)) in args.iter_mut().zip(sig.params.iter()).enumerate() {
            let previous_shape_argument = self.current_shape_argument.replace(argument as u32);
            let site = argument_site(arg.name).optional_slot(param.sponte);
            let errors_before = self.errors.len();
            if markers {
                self.check_call_marker(arg, param);
            }
            if let Some(axis) = axis {
                let declared = axis.get(argument).copied().unwrap_or(false);
                self.check_axis_marker(arg, declared);
            }
            let arg_ty = self.check_store_expr(&mut arg.expr, param.ty, site);
            if self
                .tensor_edge_policies
                .shifted_expressions
                .contains(&arg.expr.id)
            {
                self.invalid_operand_types(
                    super::intrinsics::ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION,
                    arg.span,
                );
                self.current_shape_argument = previous_shape_argument;
                continue;
            }
            if user_function
                && self.errors.len() > errors_before
                && self
                    .last_synthesized_type
                    .is_some_and(|ty| self.is_lifting_attempt(ty, param.ty))
            {
                self.replace_mismatch_with_lift_issue(errors_before, arg.span);
                lifted = true;
                self.current_shape_argument = previous_shape_argument;
                continue;
            }
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
            // A generic `copy T` parameter duplicates at entry with the
            // concrete argument the caller echoed, so the argument decides;
            // a concrete declared payload was refused at its declaration.
            if matches!(
                self.types
                    .peel_access_marker(self.resolve_inferred_type(param.ty))
                    .0,
                Some(radix_types::AccessMarker::Copy)
            ) && let Some(declared) = self.current_call_declared_params.get(argument)
            {
                let declared_payload = self
                    .types
                    .peel_access_marker(self.resolve_inferred_type(*declared))
                    .1;
                if self.type_is_generic_carrier(declared_payload) {
                    let argument_payload = self
                        .types
                        .peel_access_marker(self.resolve_inferred_type(arg_ty))
                        .1;
                    self.refuse_copy_of_nonduplicable(argument_payload, arg.span);
                }
            }
            let errors_before_unify = self.errors.len();
            self.unify_carrier_assign(arg_ty, param.ty, arg.span, ISSUE_ARGUMENT_TYPE_MISMATCH);
            if self.errors.len() == errors_before_unify {
                self.check_mut_scalar_argument_is_exact(param, arg_ty, arg.span);
            }
            self.current_shape_argument = previous_shape_argument;
        }
        lifted
    }

    /// Call-site marker agreement (callmark trial): a parameter declared
    /// `mut`, `own` or `copy` requires the same marker on the argument; a
    /// read-only parameter takes no marker except the optional explicit `ref`.
    fn check_call_marker(&mut self, arg: &HirCallArg, param: &ParamType) {
        use radix_hir::HirArgMarker as Written;
        use radix_types::AccessMarker as Declared;
        if arg.spread {
            return;
        }
        let declared = self
            .types
            .peel_access_marker(self.resolve_inferred_type(param.ty))
            .0;
        let required = match declared {
            Some(Declared::Mut) => Some(Written::Mut),
            Some(Declared::Own) => Some(Written::Own),
            Some(Declared::Copy) => Some(Written::Copy),
            Some(Declared::Ref) | None => None,
        };
        let issue = match (required, arg.marker) {
            (Some(_), None) => crate::ISSUE_CALL_MARKER_MISSING,
            (Some(need), Some(got)) if need != got => crate::ISSUE_CALL_MARKER_MISMATCH,
            (None, Some(Written::Mut | Written::Own | Written::Copy)) => {
                crate::ISSUE_CALL_MARKER_ON_READ_ONLY
            }
            _ => return,
        };
        let name = |marker: Option<Written>| match marker {
            Some(Written::Ref) => "ref",
            Some(Written::Mut) => "mut",
            Some(Written::Own) => "own",
            Some(Written::Copy) => "copy",
            Some(Written::Axis) => "axis",
            None => "none",
        };
        self.error_issue_with_args(
            SemanticErrorKind::ModeMismatch,
            issue,
            arg.span,
            &[
                ("expected", name(required).to_owned()),
                ("found", name(arg.marker).to_owned()),
            ],
        );
    }

    /// `axis` echo rule: a parameter declared `axis int a` is
    /// called `axis <expr>`, and `axis <expr>` is refused for a plain
    /// parameter. Inside the function the parameter is an ordinary int, so
    /// passing it on writes the word again.
    pub(super) fn check_axis_marker(&mut self, arg: &HirCallArg, declared: bool) {
        if arg.spread {
            return;
        }
        let written = arg.marker == Some(radix_hir::HirArgMarker::Axis);
        let issue = match (declared, written) {
            (true, false) => ISSUE_AXIS_MARKER_REQUIRED,
            (false, true) => ISSUE_AXIS_MARKER_UNEXPECTED,
            _ => return,
        };
        self.error_issue(SemanticErrorKind::ModeMismatch, issue, arg.span);
    }

    /// A `mut` scalar parameter borrows the caller's cell: a store through it
    /// lands in that cell, and the store check runs against the parameter's
    /// type, not the cell's. A narrower numeric cell (`u8`) passed to a wider
    /// `mut` parameter (`int`) would therefore take values it cannot hold, so
    /// the argument must have exactly the parameter's type. (Width limits
    /// apply at the store; this keeps the store's slot and the cell one type.)
    fn check_mut_scalar_argument_is_exact(
        &mut self,
        param: &ParamType,
        arg_ty: TypeId,
        span: radix_lexer::Span,
    ) {
        if !matches!(param.mode, SemanticParamMode::MutRef) {
            return;
        }
        let param_ty = self.peel_nominal_aliases(
            self.types
                .peel_access_marker(self.resolve_inferred_type(param.ty))
                .1,
        );
        if !self.types.is_scalar_for_access_marker(param_ty) {
            return;
        }
        let arg_ty = self.peel_nominal_aliases(
            self.types
                .peel_access_marker(self.resolve_inferred_type(arg_ty))
                .1,
        );
        if !self.types.equals(arg_ty, param_ty) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_MUT_ARGUMENT_TYPE_MISMATCH,
                span,
            );
        }
    }

    /// Whether passing `arg_ty` for a `param_ty` slot is an attempt to lift a
    /// scalar function over a tensor (gpu-reset rule 15): the parameter is one
    /// number and the argument is a tensor, vector or matrix of numbers.
    fn is_lifting_attempt(&self, arg_ty: TypeId, param_ty: TypeId) -> bool {
        if !self.is_numeric(param_ty) {
            return false;
        }
        let resolved = self.resolve_inferred_type(arg_ty);
        let (_, mut payload) = self.types.peel_access_marker(resolved);
        while let Type::Alias(_, inner) = self.types.get(payload) {
            payload = *inner;
        }
        match self.types.get(payload) {
            Type::Tensor(element, _) | Type::Vector(element, _) | Type::Matrix(element, _) => {
                crate::numeric_width::is_tensor_numeric_element(self.types, *element)
            }
            _ => false,
        }
    }

    /// Swap the argument-mismatch cascade a lifting attempt just produced for
    /// the one structured diagnostic. Only the two mismatch issues the failed
    /// store check raises are dropped; any other error in the argument stays.
    fn replace_mismatch_with_lift_issue(&mut self, errors_before: usize, span: radix_lexer::Span) {
        let mut index = 0;
        self.errors.retain(|error| {
            let keep = index < errors_before
                || !matches!(
                    error.issue(),
                    Some("expression_type_mismatch" | ISSUE_ARGUMENT_TYPE_MISMATCH)
                );
            index += 1;
            keep
        });
        self.invalid_operand_types(ISSUE_TENSOR_FUNCTION_NOT_LIFTED, span);
    }

    /// Typecheck a Path receiver as an imported namespace function, if it is one.
    ///
    /// The HIR Path `DefId` is the nearest value binding. A local parameter can
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
        if let Some(gates) = self
            .resolver
            .namespace_file_unavailable_canonical(namespace, name)
            .map(<[_]>::to_vec)
        {
            let qualified = self.qualified_function_name(namespace, name);
            if self.report_unavailable_function(&gates, qualified, receiver.span) {
                self.typecheck_call_arg_exprs(args);
                return Some(self.error_type);
            }
        }
        let imported_policy = self
            .resolver
            .namespace_file_tensor_edge_policy_requirements_canonical(namespace, name)
            .map(|(def_id, requirements)| (def_id, requirements.to_vec()));
        if let Some((def_id, requirements)) = &imported_policy {
            self.tensor_edge_policy_imported_requirements
                .insert(*def_id, requirements.clone());
        }
        // An imported function has no consumer-local declaration `DefId` for
        // `axis_params`, so the echo rule reads the interface's exported
        // parameter flags here (polish A-202).
        let imported_axis = self
            .resolver
            .namespace_file_axis_params_canonical(namespace, name)
            .map(<[_]>::to_vec);
        self.set_chorda_scalar_closure_context(name);
        let ret = self.check_user_function_call(
            &sig,
            explicit_type_args,
            args,
            receiver.span,
            expected,
            imported_policy.map(|(def_id, _)| def_id),
            imported_axis.as_deref(),
        );
        self.validate_interval_bound_call_args(name, &sig, args);
        Some(ret)
    }

    /// Namespace spelling for a Path `DefId`.
    ///
    /// Import modules live in the resolver symbol table. A local parameter that
    /// shadows the import uses a HIR-local `DefId` (`1_000_000+`) and is recovered
    /// from [`TypeChecker::binding_names`].
    fn path_namespace_name(&self, def_id: radix_hir::DefId) -> Option<radix_lexer::Symbol> {
        self.resolver
            .get_symbol(def_id)
            .map(|symbol| symbol.name)
            .or_else(|| self.binding_names.get(&def_id).copied())
    }

    /// Active-locale surface spelling for a canonical intrinsic name.
    ///
    /// Rewrites `name` to the interned surface symbol when the active pack has
    /// one, so the ordinary intrinsic checker performs its locale mapping while
    /// every backend still receives the canonical Latin name.
    fn intrinsic_surface_name(&self, canonical: &str, name: &mut Symbol) -> String {
        let surface = self
            .locale_pack
            .map_or(canonical, |pack| pack.intrinsic_surface(canonical));
        if let Some(interner) = self.interner
            && let Some(symbol) = interner.lookup(surface)
        {
            *name = symbol;
        }
        surface.to_owned()
    }

    /// The float width a method call's expected type hands down to an unsized
    /// float literal receiver (`(8.0).cbrt()` stored into an `f32` slot).
    ///
    /// A float literal has no width until it is stored, and a `SameAsReceiver`
    /// float method returns its receiver's own type, so the store's width
    /// applies to the literal exactly as it does to the operands of
    /// `2.0 * 3.0`: one rule for every such method, resolved through the
    /// registry. A method chain over a literal (`(8.0).cbrt().abs()`) passes
    /// the width down to its own receiver the same way. A bare `f64` slot, a
    /// policy cell, a decimal or a missing expectation leave the receiver to
    /// default as before.
    fn float_literal_receiver_expected(
        &mut self,
        receiver: &HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let expected = self.resolve_inferred_type(expected?);
        let Type::SizedNumeric(Primitive::Fractus, width) = *self.types.get(expected) else {
            return None;
        };
        if width.is_decimal() || !self.is_unsized_float_literal_chain(receiver) {
            return None;
        }
        Some(self.numeric_expected_type(expected))
    }

    /// A float literal (possibly negated), or a `SameAsReceiver` float method
    /// call whose receiver is one.
    fn is_unsized_float_literal_chain(&mut self, expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::MethodCall(inner, name, _, args, _) => {
                self.is_unsized_float_literal_chain(inner)
                    && super::intrinsics::is_float_same_as_receiver_method(self, *name, args.len())
            }
            _ => super::ops::numeric_literal_family(expr) == Some(Primitive::Fractus),
        }
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
        // Constructor seed (size-type Z2): the synthesized `create` call on a
        // vacua receiver carries the claimed tensor type pre-lowered on the
        // receiver. Typing adopts it instead of re-checking the vacua
        // literal, whose expected-context route would constrain `_`
        // annotation holes to rank 0.
        let receiver_ty = if matches!(receiver.kind, HirExpressionKind::Vacua)
            && let Some(claimed) = receiver.ty
        {
            claimed
        } else {
            let receiver_expected = self.float_literal_receiver_expected(receiver, expected);
            self.check_expr_with_expected(receiver, receiver_expected)
        };
        if let Some(def_id) = self
            .struct_def_from_type(receiver_ty)
            .or_else(|| self.interface_def_from_type(receiver_ty))
            && let Some(canonical_name) =
                self.resolver.imported_method_canonical_name(def_id, *name)
        {
            *name = canonical_name;
        }
        if self.canonicalize_sermo_view_method(receiver_ty, name)
            || self.canonicalize_directional_view_method(receiver_ty, name)
        {
            // WHY: under a non-la pack the canonical Latin spelling of a
            // channel view method is a wrong-locale miss, the same compile
            // error an ordinary intrinsic gets (no Latin fallback).
            self.typecheck_call_arg_exprs(args);
            self.undefined_member("unknown_method", receiver.span);
            return self.error_type;
        }
        let method_name = self.method_display_name(*name);
        let method_name = if let Some(canonical) = method_name.strip_prefix("__faber_glyph_round:")
        {
            // Rounding glyphs lift per element over tensors in principle; the
            // build is deferred, so a tensor-shaped operand is a named error.
            let resolved = self.resolve_inferred_type(receiver_ty);
            let (_, payload) = self.types.peel_access_marker(resolved);
            if matches!(
                self.types.get(payload),
                Type::Tensor(..) | Type::Matrix(..) | Type::Vector(..)
            ) {
                self.invalid_operand_types("glyph_rounding_tensor_unsupported", receiver.span);
                return self.error_type;
            }
            self.intrinsic_surface_name(canonical, name)
        } else if method_name == super::tensor_index::GLYPH_TRANSPOSE_METHOD {
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
            self.intrinsic_surface_name(super::tensor_index::TENSOR_TRANSPONE, name)
        } else {
            method_name
        };
        if self
            .tensor_edge_policies
            .shifted_expressions
            .contains(&receiver.id)
        {
            let edge_intrinsic = self
                .locale_pack
                .and_then(|pack| {
                    pack.intrinsic_canonical(
                        radix_intrinsics::IntrinsicReceiver::Tensor,
                        &method_name,
                    )
                })
                .unwrap_or(&method_name);
            if !matches!(edge_intrinsic, "limes" | "disloco") {
                self.typecheck_call_arg_exprs(args);
                self.invalid_operand_types(
                    super::intrinsics::ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION,
                    receiver.span,
                );
                return self.error_type;
            }
        }
        if let Some(result) = self.check_directional_view_method(
            receiver_ty,
            &method_name,
            explicit_type_args,
            args,
            receiver.span,
        ) {
            return result;
        }
        if let Some(result) = self.check_sermo_close(
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
            receiver,
            receiver_ty,
            name,
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
        // D1.1: a bounded type parameter receiver reaches its bound
        // contracts' methods.
        if let Some(ret) = self.check_bound_method_call(receiver_ty, *name, args, receiver.span) {
            return ret;
        }
        if let Some(sig) = self.lookup_method_signature(receiver_ty, *name) {
            let sig = self.applied_method_signature(receiver_ty, sig);
            self.check_method_visibility(receiver_ty, *name, receiver.span);
            if self.report_unavailable_method(receiver_ty, *name, receiver.span) {
                self.typecheck_call_arg_exprs(args);
                return self.error_type;
            }
            if let Some(method) = self.lookup_interface_method_info(receiver_ty, *name) {
                let availability = method.availability.clone();
                let span = method.span;
                if self.report_nondum_method_call(*name, &availability, span, args) {
                    return self.error_type;
                }
            }
            // Contract methods have no stable `DefId` to key `axis_params`,
            // so their exported/declared `axis` flags ride the method info
            // (polish A-202).
            let interface_method_axis = self
                .lookup_interface_method_info(receiver_ty, *name)
                .map(|method| method.axis.clone());
            self.set_chorda_scalar_closure_context(*name);
            let method_def = self
                .struct_def_from_type(receiver_ty)
                .and_then(|def| self.method_defs.get(&(def, *name)).copied());
            let ret = if let Some(method_def) = method_def {
                self.check_instantiated_method_call_for_callee(
                    method_def,
                    &sig,
                    explicit_type_args,
                    args,
                    receiver.span,
                    expected,
                )
            } else {
                self.check_instantiated_call_marked(
                    &sig,
                    explicit_type_args,
                    args,
                    receiver.span,
                    expected,
                    interface_method_axis.as_deref(),
                )
            };
            self.validate_interval_bound_call_args(*name, &sig, args);
            return ret;
        }

        // A member callee normally names a method, but lowering preserves the
        // same MethodCall shape for a function-valued struct field. Probe the
        // field only after method lookup so an explicit method always wins.
        if let Some(field_ty) = self.lookup_field_type(receiver_ty, *name)
            && let Some(sig) = self.signature_from_type(field_ty)
        {
            self.check_field_visibility(receiver_ty, *name, MemberAccess::Read, receiver.span);
            return self.check_instantiated_call_marked(
                &sig,
                explicit_type_args,
                args,
                receiver.span,
                expected,
                None,
            );
        }

        // U1R common-member rule: a `∪` receiver calls a method only when
        // every constituent declares it with one agreeing signature — a real
        // signature, not the permissive sink this path used to fall through
        // to. A miss names the constituents that lack the method.
        if let Some(members) = self.union_members_of(receiver_ty) {
            if let Some(sig) = self.union_common_method_signature(&members, *name) {
                self.check_method_visibility(receiver_ty, *name, receiver.span);
                for member in &members {
                    if self.report_unavailable_method(*member, *name, receiver.span) {
                        self.typecheck_call_arg_exprs(args);
                        return self.error_type;
                    }
                }
                self.set_chorda_scalar_closure_context(*name);
                let ret = self.check_instantiated_call_marked(
                    &sig,
                    explicit_type_args,
                    args,
                    receiver.span,
                    expected,
                    None,
                );
                self.validate_interval_bound_call_args(*name, &sig, args);
                return ret;
            }
            self.typecheck_call_arg_exprs(args);
            let lacking = self.union_lacking_constituents(&members, *name);
            if lacking.is_empty() {
                // D9.2: every member declares the method, but the
                // signatures differ — name each member's signature.
                let detail = self.union_method_signature_detail(&members, *name);
                self.undefined_member_with_args(
                    ISSUE_UNION_MEMBER_DIFFERS,
                    receiver.span,
                    &[
                        ("member", self.method_display_name(*name)),
                        ("detail", detail),
                    ],
                );
            } else {
                self.undefined_member_with_args(
                    ISSUE_UNION_MEMBER_NOT_COMMON,
                    receiver.span,
                    &[
                        ("member", self.method_display_name(*name)),
                        ("lacking", lacking.join(", ")),
                    ],
                );
            }
            return self.error_type;
        }

        let resolved_receiver_ty = self.resolve_inferred_type(receiver_ty);
        let (_, intrinsic_ty) = self.types.peel_access_marker(resolved_receiver_ty);
        // S0-G2: applied aliases are value-transparent for intrinsic-method
        // dispatch — the receiver's builtin carrier owns the closed
        // tensor/vector/matrix method surface, including its unknown-method
        // rejection posture.
        let receiver_carrier = self.value_carrier_type(intrinsic_ty);
        let resolved_receiver = self.types.get(receiver_carrier);
        let receiver_is_atomic = matches!(resolved_receiver, Type::Atomic(_));
        let tensor_higher_order_method = matches!(resolved_receiver, Type::Tensor(_, _))
            && super::super::air_closure_lift::is_higher_order_method(&method_name);
        // N7e D3 (interim): a method of the bare float's registry surface on a
        // float cell is not built yet. Other names keep the unknown-method miss.
        if let Type::PolicyFloat(_, width) = *resolved_receiver
            && self.is_float_registry_method(width, &method_name)
        {
            self.typecheck_call_arg_exprs(args);
            self.invalid_operand_types(super::ops::POLICY_FLOAT_MATH_NOT_BUILT, receiver.span);
            return self.error_type;
        }
        // Every receiver with an intrinsic family (scalar numerics, list, map,
        // set, text, queue, stack, tensor family) is a closed, compiler-owned
        // method surface: a registry miss is `unknown_method` here rather than
        // an inferred result that every emitter prints as a verbatim call.
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
            if !tensor_higher_order_method {
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
                && !self
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
        if let Some(view_ty) =
            self.check_typed_sermo_view(receiver_ty, entry.return_kind, explicit_type_args, span)
        {
            return Some(view_ty);
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
            (DirectionalViewKind::Meus, "sink") if self.is_closed_meus(receiver_ty) => {
                self.typecheck_call_arg_exprs(args);
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    super::sermo_route::ISSUE_AD_HANDLER_SEND_CLOSED,
                    span,
                );
                Some(self.error_type)
            }
            (DirectionalViewKind::Meus, "sink") => Some(self.check_known_method_args(
                args,
                vec![ParamType {
                    ty: payload_ty,
                    mode: SemanticParamMode::Move,
                    optional: false,
                    sponte: false,
                }],
                vacuum_ty,
                span,
            )),
            (DirectionalViewKind::Meus, "flush") => {
                if self.reject_view_method_args(args, span, "meus.flush") {
                    return Some(self.error_type);
                }
                Some(vacuum_ty)
            }
            (DirectionalViewKind::Meus, "fini") => {
                if self.reject_view_method_args(args, span, "meus.fini") {
                    return Some(self.error_type);
                }
                Some(status_ty)
            }
            (DirectionalViewKind::Tuus, "accipe") => {
                if self.reject_view_method_args(args, span, "tuus.accipe") {
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
                if self.reject_view_method_args(args, span, "tuus.cursor") {
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
                if self.reject_view_method_args(args, span, "tuus.exhauri") {
                    return Some(self.error_type);
                }
                Some(payload_ty)
            }
            (DirectionalViewKind::Tuus, "fini") => {
                if self.reject_view_method_args(args, span, "tuus.fini") {
                    return Some(self.error_type);
                }
                Some(status_ty)
            }
            _ => None,
        }
    }

    /// Rejects arguments on a zero-argument directional view method.
    ///
    /// Returns `true` after reporting `method_accepts_no_arguments`; the caller
    /// then yields the shared error type without typing the view.
    fn reject_view_method_args(
        &mut self,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
        method: &str,
    ) -> bool {
        if args.is_empty() {
            return false;
        }
        self.typecheck_call_arg_exprs(args);
        self.wrong_arity_with_arg("method_accepts_no_arguments", span, "method", method);
        true
    }

    fn directional_view_payload(
        &self,
        receiver_ty: TypeId,
    ) -> Option<(DirectionalViewKind, TypeId)> {
        let resolved = self.resolve_inferred_type(receiver_ty);
        let Type::Applied(base, args) = self.types.get(resolved) else {
            return None;
        };
        // A second argument is the typecheck-only closed-`meus` marker.
        if args.is_empty() || args.len() > 2 {
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

    /// Rewrite a pack spelling of a sermo view method (`s.recv<T>()` under
    /// en) to its canonical Latin symbol (`tuus`) so lint, MIR lowering and
    /// every backend keep matching canonical names. The view methods share
    /// the directional view type rows (DECISIONS D6.16).
    ///
    /// Returns `true` when the method is the canonical Latin spelling under a
    /// non-`la` pack that declares its own surface for the view: a wrong-locale
    /// miss the caller reports as `unknown_method`.
    fn canonicalize_sermo_view_method(&self, receiver_ty: TypeId, name: &mut Symbol) -> bool {
        let (Some(pack), Some(interner)) = (self.locale_pack, self.interner) else {
            return false;
        };
        if !self.is_sermo_receiver(receiver_ty) {
            return false;
        }
        let spelling = interner.resolve(*name);
        if let Some(crate::locale::TypeBindingKind::Frame(
            view @ (crate::locale::FrameTypeKind::Meus | crate::locale::FrameTypeKind::Tuus),
        )) = pack.type_binding_kind(spelling)
        {
            if let Some(canonical) = interner.lookup(view.latin()) {
                *name = canonical;
            }
            return false;
        }
        let Some(view @ (crate::locale::FrameTypeKind::Meus | crate::locale::FrameTypeKind::Tuus)) =
            crate::locale::FrameTypeKind::from_latin(spelling)
        else {
            return false;
        };
        pack.metadata.id != "la"
            && pack.type_surface(crate::locale::TypeBindingKind::Frame(view)) != view.latin()
    }

    /// Rewrite a pack spelling of a directional view method (`t.get()` under
    /// en) to its canonical Latin symbol (`accipe`). `sink` is not a projected
    /// row and passes through unchanged.
    ///
    /// Returns `true` for the canonical Latin spelling under a non-`la` pack
    /// (a wrong-locale miss), mirroring [`Self::canonicalize_sermo_view_method`].
    fn canonicalize_directional_view_method(&self, receiver_ty: TypeId, name: &mut Symbol) -> bool {
        let (Some(pack), Some(interner)) = (self.locale_pack, self.interner) else {
            return false;
        };
        if pack.metadata.id == "la" {
            return false;
        }
        let Some((view, _)) = self.directional_view_payload(receiver_ty) else {
            return false;
        };
        let view = match view {
            DirectionalViewKind::Meus => radix_intrinsics::SermoViewKind::Meus,
            DirectionalViewKind::Tuus => radix_intrinsics::SermoViewKind::Tuus,
        };
        let spelling = interner.resolve(*name);
        if let Some(canonical) = pack.sermo_view_method_canonical(view, spelling)
            && let Some(symbol) = interner.lookup(canonical)
        {
            *name = symbol;
            return false;
        }
        radix_intrinsics::SERMO_VIEW_METHODS
            .iter()
            .any(|entry| entry.view == view && entry.faber_name == spelling)
    }

    /// `s.tuus<T>()` / `s.meus<T>()` on `sermo<O, R>` (D6.11): the payload is
    /// checked against, or inferred from, `R` / `O`. The `meus` view toward a
    /// Faber handler is closed for sending. `None` for a bare conversation.
    fn check_typed_sermo_view(
        &mut self,
        receiver_ty: TypeId,
        return_kind: radix_intrinsics::NamespaceReturnKind,
        explicit_type_args: &[TypeId],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let view = match return_kind {
            radix_intrinsics::NamespaceReturnKind::SermoTuus => super::sermo_route::SermoView::Tuus,
            radix_intrinsics::NamespaceReturnKind::SermoMeus => super::sermo_route::SermoView::Meus,
            radix_intrinsics::NamespaceReturnKind::GenericTypus => return None,
        };
        let payload = self.typed_sermo_view_payload(receiver_ty, view, explicit_type_args, span)?;
        if payload == self.error_type {
            return Some(self.error_type);
        }
        let view_def = match view {
            super::sermo_route::SermoView::Tuus => {
                crate::builtins::frame_types::tuus_def_id(self.resolver)
            }
            super::sermo_route::SermoView::Meus => {
                crate::builtins::frame_types::meus_def_id(self.resolver)
            }
        };
        let Some(view_def) = view_def else {
            self.missing_type_annotation("directional_view_type_not_registered", span);
            return Some(self.error_type);
        };
        Some(match view {
            super::sermo_route::SermoView::Tuus => {
                crate::builtins::frame_types::directional_view_payload_type_id(
                    self.types, view_def, payload,
                )
            }
            super::sermo_route::SermoView::Meus => {
                crate::builtins::frame_types::closed_meus_type_id(self.types, view_def, payload)
            }
        })
    }

    fn is_sermo_receiver(&self, receiver_ty: TypeId) -> bool {
        self.sermo_shape(receiver_ty).is_some()
    }

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

        // An optional-chain member that is then called (`a?.method()`) parses
        // as a `Call` whose callee is `OptionalChain(object, Member(name))`.
        // A declared method on the nullable object's inner type is a valid
        // callee; the chain keeps the call result option-wrapped. A miss on
        // method lookup falls through to the generic callee path, so field
        // access and the existing diagnostics are unchanged.
        if let HirExpressionKind::OptionalChain(
            object,
            radix_hir::HirOptionalChainKind::Member(name),
        ) = &mut callee.kind
        {
            let name = *name;
            let object_ty = self.check_expr(object);
            let inner_ty = match self.types.get(self.resolve_inferred_type(object_ty)) {
                Type::Option(inner) => *inner,
                _ => object_ty,
            };
            if let Some(sig) = self.lookup_method_signature(inner_ty, name) {
                let sig = self.applied_method_signature(inner_ty, sig);
                self.check_method_visibility(inner_ty, name, callee.span);
                if self.report_unavailable_method(inner_ty, name, callee.span) {
                    self.typecheck_call_arg_exprs(args);
                    return self.error_type;
                }
                let method_def = self
                    .struct_def_from_type(inner_ty)
                    .and_then(|def| self.method_defs.get(&(def, name)).copied());
                let ret = if let Some(method_def) = method_def {
                    self.check_instantiated_method_call_for_callee(
                        method_def,
                        &sig,
                        explicit_type_args,
                        args,
                        callee.span,
                        expected,
                    )
                } else {
                    self.check_instantiated_call_marked(
                        &sig,
                        explicit_type_args,
                        args,
                        callee.span,
                        expected,
                        None,
                    )
                };
                let wrapped = if matches!(
                    self.types.get(self.resolve_inferred_type(ret)),
                    Type::Option(_)
                ) {
                    ret
                } else {
                    self.types.option(ret)
                };
                callee.ty = Some(wrapped);
                return wrapped;
            }
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
                if let Some(symbol) = self.resolver.get_symbol(*def_id)
                    && let Some(Type::Func(sig)) = symbol.ty.as_ref().map(|ty| self.types.get(*ty))
                {
                    return sig.ret;
                }
                // WHY: registration always attaches a Func signature; this fallback keeps checking alive if lookup fails.
                return self
                    .types
                    .sized_numeric(Primitive::Numerus, NumericWidth::U32);
            }

            if self.report_unavailable_path(*def_id, callee.span) {
                self.typecheck_call_arg_exprs(args);
                return self.error_type;
            }

            if let Some(sig) = self.functions.get(def_id).cloned() {
                if let Some(name) = self.resolver.get_symbol(*def_id).map(|symbol| symbol.name) {
                    self.set_chorda_scalar_closure_context(name);
                    let ret = self.check_instantiated_call_for_callee(
                        *def_id,
                        &sig,
                        explicit_type_args,
                        args,
                        callee.span,
                        expected,
                    );
                    self.validate_interval_bound_call_args(name, &sig, args);
                    return ret;
                }
                return self.check_instantiated_call_for_callee(
                    *def_id,
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
                            let arg_ty = self.check_store_expr(
                                &mut arg.expr,
                                field_ty,
                                StoreSite::new(HirStoreKind::Field).named(name),
                            );
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
                        let arg_ty = self.check_store_expr(
                            &mut arg.expr,
                            *field_ty,
                            StoreSite::new(HirStoreKind::Field),
                        );
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
            return self.check_instantiated_call_marked(
                &sig,
                explicit_type_args,
                args,
                callee.span,
                expected,
                None,
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
            ("source", self.format_union_constituent_type(source_ty)),
            (
                "destination",
                self.format_union_constituent_type(destination_ty),
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

    /// Per-member signature listing for `union_member_differs` (D9.2):
    /// "`() → fractus` in Circulus, `() → numerus` in Quadratum".
    fn union_method_signature_detail(&self, members: &[TypeId], name: Symbol) -> String {
        members
            .iter()
            .filter_map(|member| {
                let sig = self.lookup_method_signature(*member, name)?;
                Some(format!(
                    "`{}` in {}",
                    self.format_func_sig(&sig),
                    self.display_union_constituent(*member)
                ))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Faber-surface spelling of a method signature for diagnostics.
    pub(super) fn format_func_sig(&self, sig: &FuncSig) -> String {
        let params = sig
            .params
            .iter()
            .map(|param| self.format_union_constituent_type(param.ty))
            .collect::<Vec<_>>()
            .join(", ");
        let mut text = format!(
            "({params}) → {}",
            self.format_union_constituent_type(sig.ret)
        );
        if let Some(err) = sig.err {
            text.push_str(" ⇥ ");
            text.push_str(&self.format_union_constituent_type(err));
        }
        text
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
    /// (U1R guardrails 3, 4).
    ///
    /// Guardrails 3 and 4 compute over the declared member views and stay
    /// confined to unions with two or more nominal members — a mixed or
    /// primitive union has no honest declared-member view. The former
    /// guardrail 1 (WARN024, "narrow to recover per-member access") retired
    /// with D9.2: overlap access is plain and access outside it is an error.
    ///
    /// A declared enum is not a written `∪`: its variants are views for
    /// refinement, not store-only constituents, so an annotation that names
    /// an enum (bare or as `E ∪ none`) never draws these warnings.
    pub fn warn_union_annotation_guardrails(
        &mut self,
        annotation_ty: TypeId,
        span: radix_lexer::Span,
    ) {
        if self.annotation_names_enum(annotation_ty) {
            return;
        }
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
            let Some(first) = signatures.iter().find_map(std::clone::Clone::clone) else {
                continue;
            };
            if signatures
                .iter()
                .flatten()
                .any(|sig| !self.func_sigs_agree(&first, sig))
            {
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

    /// Whether an annotation, through aliases and a `∪ none` wrapper, names a
    /// declared enum.
    fn annotation_names_enum(&mut self, ty: TypeId) -> bool {
        let mut current = self.resolve_inferred_type(ty);
        for _ in 0..8 {
            match self.types.get(current).clone() {
                Type::Alias(_, inner) | Type::Option(inner) => {
                    current = self.resolve_inferred_type(inner);
                }
                Type::Enum(_) => return true,
                _ => return false,
            }
        }
        false
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
        let mut sets: Vec<handroll::FxHashSet<Symbol>> = members
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
        let mut count: handroll::FxHashMap<Symbol, usize> = handroll::FxHashMap::default();
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
        self.failable_calls_checked += 1;

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
                    if let Some(row) = self.resolver.conversion_row_for(source_ty, destination_ty)
                        && let Some(fn_id) = row.fn_def_id
                    {
                        if let Some(channel) = self.functions.get(&fn_id).and_then(|sig| sig.err)
                            && !self.check_registered_row_channel(channel, span)
                        {
                            return false;
                        }
                        self.pending_conversion = Some(fn_id);
                        return false;
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

        // D17.8: an unhandled failure directly in `main {}` panics — every
        // backend lowers the call as an implicit abort (fi6c). Helpers,
        // closures and praefixum clear the main frame and stay errors.
        if self.in_main_frame {
            return false;
        }
        self.invalid_operand_types(ISSUE_FAILABLE_CALL_REQUIRES_HANDLER, span);
        true
    }

    /// Checks the `⇇` copy-into statement: `target ⇇ value` copies every value
    /// of `value` into the existing storage of `target`.
    ///
    /// The type rules live here; the access rules (writable storage, no
    /// alias, view target) live in definite assignment and borrow analysis.
    ///
    /// - R1: element type and shape match exactly. Shapes compare in the
    ///   declared-symbol space (`index_equals`), so a generic
    ///   `tensor<f32, [d, T]>` meets the transpose of `[T, d]` without any
    ///   runtime witness.
    /// - R2: the target is a tensor; scalars, lists, matrices and vectors are
    ///   rejected (a value type is already written by `←`).
    /// - R7: a callable target is the retired sink form.
    ///
    /// The statement result is `vacuum`.
    pub fn check_copy_into(
        &mut self,
        target: &mut HirExpression,
        value: &mut HirExpression,
        span: radix_lexer::Span,
    ) -> TypeId {
        let vacuum = self.vacuum_type();

        // A field target goes through the lvalue rules (privacy and `fixum`
        // field modifiers); every other target is typed as a plain read.
        let target_ty = if matches!(target.kind, HirExpressionKind::Field(..)) {
            self.check_lvalue(target)
        } else {
            self.check_expr(target)
        };
        let resolved = self.resolve_inferred_type(target_ty);

        if resolved == self.error_type
            || matches!(
                self.types.get(resolved),
                Type::Primitive(Primitive::Ignotum)
            )
        {
            self.check_expr(value);
            return self.error_type;
        }

        if self.signature_from_type(resolved).is_some() {
            // FABER-2-SUNSET: delete with the retired-word diagnostics.
            self.check_expr(value);
            self.not_callable(ISSUE_TRANSFER_SINK_RETIRED, span);
            return self.error_type;
        }

        if self.is_infer(resolved) {
            // The target is still open: the value decides its type.
            let value_ty = self.check_expr(value);
            self.unify_carrier_assign(
                value_ty,
                target_ty,
                value.span,
                ISSUE_COPY_INTO_TYPE_MISMATCH,
            );
            return vacuum;
        }

        if !crate::tensor_bracket::is_tensor_type(self.types, resolved) {
            self.check_expr(value);
            self.invalid_operand_types(ISSUE_COPY_INTO_TARGET_NOT_TENSOR, target.span);
            return self.error_type;
        }

        if let HirExpressionKind::Path(def_id) = &target.kind
            && self.lookup_binding(*def_id).is_none()
            && self.consts.contains_key(def_id)
        {
            self.immutable_assignment("assignment_to_constant", target.span);
        }

        let value_ty =
            self.check_store_expr(value, target_ty, StoreSite::new(HirStoreKind::Assignment));
        if self.resolve_inferred_type(value_ty) == self.error_type {
            return self.error_type;
        }
        let unified = self.unify_carrier_assign(
            value_ty,
            target_ty,
            value.span,
            ISSUE_COPY_INTO_TYPE_MISMATCH,
        );
        if unified == self.error_type {
            return self.error_type;
        }
        self.require_exact_tensor_element(value_ty, target_ty, value.span);
        vacuum
    }

    /// `sink ⇜ item` — the sink-into statement.
    ///
    /// - Rule 8: the left operand must conform to the sinking contract
    ///   (`Sink<T>`), recognized by the contract role, never by the name.
    ///   Lists, bounded lists and sets sink their elements; tensors, vectors
    ///   and matrices are not sinks (`⇇` stays their write).
    /// - Rule 3: the item is exactly the contract's `T`. The item type is the
    ///   contract argument, never the receiver's own type
    ///   (`class Box implements Sink<string>` sinks strings), and there is no
    ///   implicit conversion, so `console.out ⇜ "§"(n)`, never
    ///   `console.out ⇜ n`.
    /// - Rule 6: `const` is deep — a sink reached through a `let` name is
    ///   refused, as a mutating method call there is.
    /// - Rule 7: `⇜` into a collection an enclosing loop iterates is the same
    ///   shape-change error `append` reports there.
    /// - A channel's send view (`s.meus<T>()`, en `send<T>`) implements
    ///   `Sink<T>`, so `⇜` works on any send view. A recv view does not.
    ///
    /// The statement result is `vacuum`; rule 5 (no use as a value) is
    /// enforced where the surrounding expression asks for one.
    pub fn check_sink_into(
        &mut self,
        sink: &mut HirExpression,
        item: &mut HirExpression,
        span: radix_lexer::Span,
    ) -> TypeId {
        // Rule 7 runs before the receiver resolution so the tighter error wins
        // (the same identity a shape-changing method call reports). The normal
        // checks still run, so the receiver and item are typed.
        if let HirExpressionKind::Path(def_id) = &TypeChecker::method_receiver_place(sink).kind
            && self.loop_iterated.contains(def_id)
        {
            let binding = self.binding_display_name(*def_id);
            self.error_issue_with_args(
                SemanticErrorKind::OperandTypesInvalid,
                super::loop_shape::ISSUE_LOOP_COLLECTION_SHAPE_CHANGE,
                span,
                &[("binding", binding), ("method", "sink".to_owned())],
            );
        }

        let sink_ty = self.check_expr(sink);
        let resolved = self.resolve_inferred_type(sink_ty);

        if resolved == self.error_type
            || matches!(
                self.types.get(resolved),
                Type::Primitive(Primitive::Ignotum)
            )
        {
            self.check_expr(item);
            return self.error_type;
        }

        // The send view is a sink: `m ⇜ "ping"` on `s.meus<T>()`. A `tuus`
        // (recv) view is not, so it falls through to rule 8.
        if let Some((DirectionalViewKind::Meus, payload_ty)) =
            self.directional_view_payload(resolved)
        {
            if self.is_closed_meus(resolved) {
                self.check_expr(item);
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    super::sermo_route::ISSUE_AD_HANDLER_SEND_CLOSED,
                    span,
                );
                return self.error_type;
            }
            if self.check_sink_into_item(item, payload_ty).is_none() {
                return self.error_type;
            }
            return self.vacuum_type();
        }

        let Some(expected) = self.sink_contract_item(resolved) else {
            self.check_expr(item);
            self.invalid_operand_types(ISSUE_SINK_INTO_RECEIVER_NOT_SINK, sink.span);
            return self.error_type;
        };

        // Rule 6: `const` is deep, so a sink reached through a `let` name is
        // refused exactly as a mutating method call on that name is.
        if let HirExpressionKind::Path(def_id) = &sink.kind
            && !self.path_binding_is_mutable(sink)
        {
            let binding = self.binding_display_name(*def_id);
            self.error_issue_with_args(
                SemanticErrorKind::ImmutableAssignment,
                ISSUE_MUTATING_RECEIVER_THROUGH_CONST,
                span,
                &[("binding", binding), ("method", "sink".to_owned())],
            );
        }

        if self.check_sink_into_item(item, expected).is_none() {
            return self.error_type;
        }
        self.vacuum_type()
    }

    /// Rules 3 and 6: the item is exactly `expected`, up to aliases. Returns
    /// the item type, or `None` after reporting the mismatch.
    fn check_sink_into_item(
        &mut self,
        item: &mut HirExpression,
        expected: TypeId,
    ) -> Option<TypeId> {
        let checked = self.check_expr(item);
        let item_ty = self.resolve_inferred_type(checked);
        if item_ty == self.error_type {
            return None;
        }
        let exact = self.types.is_assignable(item_ty, expected)
            && self.types.is_assignable(expected, item_ty);
        if !exact {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SINK_INTO_ITEM_TYPE_MISMATCH,
                item.span,
            );
            return None;
        }
        Some(item_ty)
    }

    /// The source name of a value binding, for diagnostic arguments.
    fn binding_display_name(&self, def_id: DefId) -> String {
        self.binding_names
            .get(&def_id)
            .or_else(|| self.resolver.get_symbol(def_id).map(|symbol| &symbol.name))
            .and_then(|symbol| self.interner.map(|interner| interner.resolve(*symbol)))
            .unwrap_or("value")
            .to_owned()
    }

    /// Rule 8: the item type `resolved` accepts as a `Sink<T>`, if it is one.
    ///
    /// A class conformance (`implements Sink<string>`) or a generic parameter
    /// bound (`<T implements Sink<T>>`) names the item in its contract
    /// argument. A conformance whose argument is the receiver itself is not a
    /// sink shape: sinking has no `Self` position. Built-in collection sinks
    /// use their element type directly and do not depend on a declared role.
    fn sink_contract_item(&self, resolved: TypeId) -> Option<TypeId> {
        let item_of = |args: &[TypeId]| match args {
            [arg] => Some(self.resolve_inferred_type(*arg)),
            _ => None,
        };
        let (_, inner) = self.types.peel_access_marker(resolved);
        match self.types.get(inner).clone() {
            Type::Struct(def) => {
                let contract = self.types.sinking_contract?;
                self.implemented_contracts
                    .get(&def)?
                    .iter()
                    .find(|declared| declared.contract == contract)
                    .and_then(|declared| item_of(&declared.args))
            }
            Type::Param(sym) => {
                let contract = self.types.sinking_contract?;
                self.current_generic_bounds
                    .get(&sym)?
                    .iter()
                    .find(|bound| bound.contract == contract)
                    .and_then(|bound| item_of(&bound.args))
            }
            Type::Array(elem) | Type::Set(elem) => Some(self.resolve_inferred_type(elem)),
            Type::BoundedArray { element, .. } | Type::RangedArray { element, .. } => {
                Some(self.resolve_inferred_type(element))
            }
            _ => None,
        }
    }

    /// R1 asks for the same element type, not a widening one. Tensor element
    /// types are invariant in assignment, and this check reads the two element
    /// types directly so a copy never depends on that relation.
    fn require_exact_tensor_element(
        &mut self,
        value_ty: TypeId,
        target_ty: TypeId,
        span: radix_lexer::Span,
    ) {
        let value_ty = self.resolve_inferred_type(value_ty);
        let target_ty = self.resolve_inferred_type(target_ty);
        let value_inner = self.types.peel_access_marker(value_ty).1;
        let target_inner = self.types.peel_access_marker(target_ty).1;
        let (Type::Tensor(value_elem, _), Type::Tensor(target_elem, _)) = (
            self.types.get(value_inner).clone(),
            self.types.get(target_inner).clone(),
        ) else {
            return;
        };
        let value_elem = self.resolve_inferred_type(value_elem);
        let target_elem = self.resolve_inferred_type(target_elem);
        // Exact up to aliases: each element assigns to the other.
        let exact = self.types.is_assignable(value_elem, target_elem)
            && self.types.is_assignable(target_elem, value_elem);
        if !exact {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_COPY_INTO_TYPE_MISMATCH,
                span,
            );
        }
    }

    /// R4/R5: report a `⇇` or `⇜` statement used as a value (`x ← a ⇇ b`,
    /// `sit t ← a ⇇ b`, `sit v ← s ⇜ x`). The statement has no result to store
    /// or chain.
    pub fn reject_copy_into_value(&mut self, value: &HirExpression) -> bool {
        let issue = match &value.kind {
            HirExpressionKind::CopyInto { .. } => ISSUE_COPY_INTO_VALUE_USED,
            HirExpressionKind::SinkInto { .. } => ISSUE_SINK_INTO_VALUE_USED,
            _ => return false,
        };
        self.error_issue(SemanticErrorKind::TypeMismatch, issue, value.span);
        true
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
            // WHY: pad only the parameters the explicit list leaves open; a
            // witness list longer than the arity yields a different applied
            // type than the one the caller wrote.
            let given = explicit_type_args.len().min(arity);
            return explicit_type_args[..given]
                .iter()
                .copied()
                .chain((given..arity).map(|_| self.fresh_infer()))
                .collect();
        }
        if let Some(args) = self.applied_args_for_def(expected, def_id)
            && args.len() == arity
        {
            return args;
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
