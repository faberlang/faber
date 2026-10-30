//! Item-level typechecking for declarations that establish function bodies and
//! global values.
//!
//! This file is the point where HIR items become checked bodies rather than
//! collected signatures. Earlier typecheck setup has already registered
//! callable, constant, struct, interface, and enum metadata; the routines here
//! use those tables to validate the bodies and to write inferred declaration
//! types back into both the HIR and the checker-side signature maps.
//!
//! INVARIANTS
//! ==========
//! - Function parameters and CLI argument bindings are scoped only to the
//!   function body.
//! - `current_return` tracks the normal return channel, while `current_error`
//!   tracks the alternate-exit channel used by `iace`/catchable control flow.
//! - Async-consuming forms are scoped to async functions/entries; closures do
//!   not implicitly inherit await permission.
//! - Bodyful named functions without `→` are effect-only. They finalize to
//!   `vacuum` and reject `redde` instead of inferring a value return.
//! - Struct field defaults are checked against their declared field types; they
//!   do not create fresh field types.

use super::store::StoreSite;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::HirStoreKind;

const ISSUE_PARAMETER_DEFAULT_TYPE_MISMATCH: &str = "parameter_default_type_mismatch";
const ISSUE_CONSTANT_ANNOTATION_MISMATCH: &str = "constant_annotation_mismatch";
const ISSUE_FIELD_DEFAULT_TYPE_MISMATCH: &str = "field_default_type_mismatch";

impl TypeChecker<'_> {
    /// Check a function body under its declared signature context.
    ///
    /// This temporarily installs the function's normal and alternate-exit
    /// result types so nested `redde` and `iace` expressions validate against
    /// the signature rather than against an enclosing body. When the normal
    /// return type is omitted, the function is effect-only: it accepts
    /// statement-only bodies, rejects `redde`, and finalizes the normal return
    /// channel to `vacuum`.
    pub fn check_function(
        &mut self,
        def_id: DefId,
        annotations: &[radix_hir::HirAnnotation],
        func: &mut HirFunction,
        genus_type_params: &[radix_hir::HirTypeParam],
    ) {
        self.reject_json_annotations_on_site(annotations, "functio");
        let previous_function_def = self.current_function_def.replace(def_id);
        let previous_shape_sources = std::mem::take(&mut self.current_shape_param_sources);
        let previous_shape_call_sources =
            std::mem::take(&mut self.current_shape_param_call_sources);
        let func_name = self.function_display_name(def_id).unwrap_or_default();
        let previous_generic_param_kinds = std::mem::replace(
            &mut self.current_generic_param_kinds,
            func.type_params
                .iter()
                .map(|param| (param.name, param.kind))
                .collect(),
        );
        let previous_genus_type_params = std::mem::replace(
            &mut self.current_genus_type_params,
            genus_type_params.iter().map(|param| param.name).collect(),
        );
        let previous_size_param_defs = std::mem::replace(
            &mut self.current_size_param_defs,
            func.type_params
                .iter()
                .filter(|param| matches!(param.kind, radix_syntax::GenericParamKind::Magnitudo))
                .map(|param| param.def_id)
                .collect(),
        );
        let previous_size_param_names = std::mem::replace(
            &mut self.current_size_param_by_name,
            func.type_params
                .iter()
                .filter(|param| matches!(param.kind, radix_syntax::GenericParamKind::Magnitudo))
                .map(|param| (param.name, param.def_id))
                .collect(),
        );
        let previous_generic_bounds = self.enter_generic_bounds(&func.type_params);
        self.push_scope();
        // Size-type Z3: signature positions may not name value parameters in
        // figuras; collect the spellings once for the did-you-mean.
        let value_param_names: Vec<radix_lexer::Symbol> =
            func.params.iter().map(|param| param.name).collect();
        for (idx, param) in func.params.iter_mut().enumerate() {
            self.reject_tensor_edge_policy_value_type(param.ty, param.span);
            self.reject_signature_value_name_extents(param.ty, &value_param_names, param.span);
            if let Some(default) = &mut param.default {
                let site = StoreSite::new(HirStoreKind::ParameterDefault).named(param.name);
                let default_ty = self.check_store_expr(default, param.ty, site);
                self.unify_assign_issue(
                    default_ty,
                    param.ty,
                    default.span,
                    ISSUE_PARAMETER_DEFAULT_TYPE_MISMATCH,
                );
            }
            let scalar_text = self.interval_bound_param_is_scalar_text(&func_name, idx, param.ty);
            // `mut` writes the caller's value; `own`/`copy` write the callee's own
            // value (OWN-A). Unmarked and `ref` stay read-only.
            let mutable =
                matches!(param.mode, HirParamMode::In) || self.types.is_exclusive_value(param.ty);
            if !mutable {
                self.read_only_param_roots.insert(param.def_id);
            }
            // A `copy` parameter duplicates at entry (D-6); refuse a declared
            // payload that holds a member with identity. A generic payload is
            // decided at each call site instead.
            self.refuse_copy_slot_of_nonduplicable(param.ty, param.span);
            self.warn_union_annotation_guardrails(param.ty, param.span);
            self.check_hashable_keys(param.ty, param.span);
            self.record_binding_name(param.def_id, param.name);
            self.insert_binding_with_scalar(param.def_id, param.ty, scalar_text, mutable);
        }
        if let Some(param) = &func.cli_args {
            self.record_binding_name(param.def_id, param.name);
            self.insert_binding(param.def_id, param.ty);
        }

        let prev_return = self.current_return;
        let prev_error = self.current_error;
        let prev_main_frame = std::mem::take(&mut self.in_main_frame);
        let prev_device_function = self.current_device_function;
        let prev_air_function = self.current_air_function;
        let prev_nucleum_entry = self.current_nucleum_entry;
        let prev_shader_function = self.current_shader_function;
        let prev_generator_function = self.current_generator_function;
        let prev_await_context = self.current_await_context;
        let prev_await_return_context = self.current_await_return_context;
        let prev_inferred = self.inferred_return;
        let prev_allow_inferred = self.allow_inferred_return;
        self.current_return = func.ret_ty;
        self.current_error = func.err_ty.map(ErrorSink::Function);
        self.current_device_function = func.is_kernel || func.nucleum_fragment;
        self.install_function_shape_sources(&func.type_params, &func.params);
        self.current_air_function = self.air_lane_defs.contains(&def_id);
        self.current_nucleum_entry = func.is_kernel;
        self.current_shader_function = func.is_vertex || func.is_fragment;
        self.current_generator_function = func.is_generator;
        if let Some(ret) = func.ret_ty {
            self.reject_tensor_edge_policy_value_type(ret, radix_lexer::Span::default());
            // Z3: `→ tf32[h, w]` over value parameters is the same remote
            // binding as a parameter figura.
            self.reject_signature_value_name_extents(
                ret,
                &value_param_names,
                radix_lexer::Span::default(),
            );
        }
        if let Some(err) = func.err_ty {
            self.reject_tensor_edge_policy_value_type(err, radix_lexer::Span::default());
        }
        self.current_await_context = func.is_async;
        self.current_await_return_context = func.is_async && !func.is_generator;
        self.inferred_return = None;
        self.allow_inferred_return = false;

        if let Some(body) = &mut func.body {
            self.check_block(body, None);
        }

        let inferred = self.inferred_return.take();
        if func.ret_ty.is_none() {
            let ret_ty = inferred.unwrap_or(self.vacuum_type());
            func.ret_ty = Some(ret_ty);
            let (ret, err) = self.callable_boundary_return(
                ret_ty,
                func.err_ty,
                func.is_async,
                func.is_generator,
            );
            if let Some(sig) = self.functions.get_mut(&def_id) {
                sig.ret = ret;
                sig.err = err;
            }
        }

        self.current_return = prev_return;
        self.current_error = prev_error;
        self.in_main_frame = prev_main_frame;
        self.current_device_function = prev_device_function;
        self.current_air_function = prev_air_function;
        self.current_nucleum_entry = prev_nucleum_entry;
        self.current_shader_function = prev_shader_function;
        self.current_generator_function = prev_generator_function;
        self.current_await_context = prev_await_context;
        self.current_await_return_context = prev_await_return_context;
        self.inferred_return = prev_inferred;
        self.allow_inferred_return = prev_allow_inferred;
        self.pop_scope();
        self.current_generic_param_kinds = previous_generic_param_kinds;
        self.current_genus_type_params = previous_genus_type_params;
        self.current_size_param_defs = previous_size_param_defs;
        self.current_size_param_by_name = previous_size_param_names;
        self.current_generic_bounds = previous_generic_bounds;
        self.current_shape_param_sources = previous_shape_sources;
        self.current_shape_param_call_sources = previous_shape_call_sources;
        self.current_function_def = previous_function_def;
    }

    /// Check a constant initializer and record the resulting global type.
    ///
    /// Constants may either state their type or synthesize it from the
    /// initializer. The HIR and `consts` table are kept in lockstep because path
    /// lookup during the rest of typechecking reads from the table, while later
    /// phases read the annotated HIR.
    pub fn check_const(&mut self, def_id: DefId, const_item: &mut radix_hir::HirConst) {
        if let Some(ty) = const_item.ty {
            self.reject_tensor_edge_policy_value_type(ty, const_item.value.span);
        }
        let ty = if let Some(annotated) = const_item.ty {
            // `const T X = e` states a compile-time fact (an `=` position);
            // a top-level `sit T x ← e` is an ordinary store (SEM062).
            let site = StoreSite::new(HirStoreKind::Declaration).named(const_item.name);
            let site = if const_item.statica.is_some() {
                site.compile_time()
            } else {
                site
            };
            // Typed `sit T x ↤ y` at item scope is conversion-directed
            // initialization, not assignment to a constant. Expose the
            // binding only while the initializer is checked so lvalue
            // resolution does not fire SEM020, then drop it so later
            // assignment still reports assignment_to_constant.
            let value_ty = if matches!(
                const_item.value.kind,
                HirExpressionKind::ConversioAssign { .. }
            ) {
                self.push_scope();
                self.insert_binding_with_scalar(def_id, annotated, false, const_item.mutable);
                let value_ty = self.check_store_expr(&mut const_item.value, annotated, site);
                self.pop_scope();
                value_ty
            } else {
                self.check_store_expr(&mut const_item.value, annotated, site)
            };
            self.unify_assign_issue(
                value_ty,
                annotated,
                const_item.value.span,
                ISSUE_CONSTANT_ANNOTATION_MISMATCH,
            );
            annotated
        } else {
            self.check_expr(&mut const_item.value)
        };

        const_item.ty = Some(ty);
        self.consts.insert(def_id, ty);
    }

    /// Check a static's `praefixum { … }` body (D8.4) as the body of a
    /// synchronous function returning the declared type: `redde` yields the
    /// static's value and must type-check as `T`. There is no error channel
    /// and no await context; the purity pass names the forbidden effects.
    pub fn check_praefixum(
        &mut self,
        inner: &mut radix_hir::HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        let HirExpressionKind::Block(body) = &mut inner.kind else {
            return self.check_expr(inner);
        };
        let prev_return = self.current_return;
        let prev_error = self.current_error;
        let prev_main_frame = std::mem::take(&mut self.in_main_frame);
        let prev_generator_function = self.current_generator_function;
        let prev_await_context = self.current_await_context;
        let prev_await_return_context = self.current_await_return_context;
        let prev_inferred = self.inferred_return;
        let prev_allow_inferred = self.allow_inferred_return;
        self.current_return = expected;
        self.current_error = None;
        self.current_generator_function = false;
        self.current_await_context = false;
        self.current_await_return_context = false;
        self.inferred_return = None;
        self.allow_inferred_return = expected.is_none();
        self.push_scope();
        self.check_block(body, None);
        self.pop_scope();
        let inferred = self.inferred_return.take();
        self.current_return = prev_return;
        self.current_error = prev_error;
        self.in_main_frame = prev_main_frame;
        self.current_generator_function = prev_generator_function;
        self.current_await_context = prev_await_context;
        self.current_await_return_context = prev_await_return_context;
        self.inferred_return = prev_inferred;
        self.allow_inferred_return = prev_allow_inferred;
        let ty = expected.or(inferred).unwrap_or_else(|| self.vacuum_type());
        inner.ty = Some(ty);
        ty
    }

    /// Dispatch item checking after the type metadata collection pass.
    ///
    /// Only items with executable or value-bearing bodies need work here.
    /// Metadata-only declarations were already harvested into lookup tables, and
    /// their structural contracts are enforced where expressions or patterns use
    /// them.
    pub fn check_item(&mut self, item: &mut HirItem) {
        match &mut item.kind {
            HirItemKind::Function(func) => {
                self.check_function(item.def_id, &item.annotations, func, &[]);
            }
            HirItemKind::Constant(const_item) => self.check_const(item.def_id, const_item),
            HirItemKind::Struct(struct_item) => {
                // Field defaults and methods are the type's own code: its
                // `@ privata` members are reachable here (D5.3).
                let outer_owner = self.current_member_owner.replace(item.def_id);
                self.check_json_struct_contract(item.def_id, struct_item);
                for field in &mut struct_item.fields {
                    self.reject_tensor_edge_policy_value_type(field.ty, item.span);
                    self.check_hashable_keys(field.ty, item.span);
                    if let Some(init) = &mut field.init {
                        let site = StoreSite::new(HirStoreKind::Field)
                            .named(field.name)
                            .compile_time();
                        let init_ty = self.check_store_expr(init, field.ty, site);
                        self.unify_assign_issue(
                            init_ty,
                            field.ty,
                            init.span,
                            ISSUE_FIELD_DEFAULT_TYPE_MISMATCH,
                        );
                    }
                }
                for method in &mut struct_item.methods {
                    self.check_function(
                        method.def_id,
                        &method.annotations,
                        &mut method.func,
                        &struct_item.type_params,
                    );
                }
                self.current_member_owner = outer_owner;
            }
            _ => {}
        }
    }
}
