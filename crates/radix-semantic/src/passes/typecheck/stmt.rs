//! Statement and block typing for local control surfaces inside HIR bodies.
//!
//! This module owns the places where typechecking mutates lexical binding
//! state: local declarations, block scopes, and `redde` statements. Expression
//! checking handles most value forms, but statements decide when a synthesized
//! value is intentionally ignored, when a local declaration needs a fresh
//! inference variable, and how return statements contribute to the enclosing
//! function's normal result type.
//!
//! INVARIANTS
//! ==========
//! - A local binding is inserted only after its declared or inferred type has
//!   been established.
//! - `redde` contributes only to an explicitly declared normal return channel.
//!   Expression-bodied closures can infer a result from their expression without
//!   using `redde`. Alternate exits are checked through `current_error` in
//!   expression/control-flow code.
//! - Blocks always introduce a lexical scope and type to their trailing
//!   expression when present, otherwise to `vacuum`.
//! - Ignored expression statements must not leave unresolved inference variables
//!   dangling; finalization should only see genuinely unresolved declarations or
//!   expressions.

use super::store::StoreSite;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::HirStoreKind;

const ISSUE_IGNORED_EXPR_RESULT_INFER: &str = "ignored_expression_result_must_resolve";
const ISSUE_INITIALIZER_ANNOTATION_MISMATCH: &str = "initializer_annotation_mismatch";
const ISSUE_AWAIT_BIND_REQUIRES_ASYNC_CONTEXT: &str = "await_bind_requires_async_context";
const ISSUE_AWAIT_BIND_REQUIRES_PROMISSUM: &str = "await_bind_requires_promissum";
const ISSUE_AWAIT_FAILABLE_REQUIRES_ALTERNATE_EXIT: &str = "await_failable_requires_alternate_exit";
const ISSUE_AWAIT_FAILABLE_ALTERNATE_TYPE_MISMATCH: &str = "await_failable_alternate_type_mismatch";
const ISSUE_INCOMPATIBLE_RETURN_TYPES: &str = "incompatible_return_types";
const ISSUE_RETURN_TYPE_MISMATCH: &str = "return_type_mismatch";
const ISSUE_CONST_SHARED_INTO_VAR: &str = "const_shared_into_var";

impl TypeChecker<'_> {
    /// Validate a `redde` statement against the current normal return contract.
    ///
    /// Annotated functions use their declared return type as an expected type so
    /// nested expressions can benefit from top-down information. Bodyful named
    /// functions and statement-bodied closures without `→` are effect-only and
    /// reject `redde`; expression surfaces that opt into inferred returns still
    /// collect the first observed return type and unify later returns against it.
    pub fn check_return(&mut self, value: Option<&mut HirExpression>, span: radix_lexer::Span) {
        if self.current_return.is_none() && !self.allow_inferred_return {
            if let Some(expr) = value {
                self.check_expr(expr);
            }
            self.errors.push(
                SemanticError::structured(SemanticErrorKind::ReturnAbsent, span)
                    .with_arg("issue", "redde_requires_return_type")
                    .with_arg("keyword", "redde"),
            );
            return;
        }

        let previous_shape_return = self.current_shape_return_expr;
        let value_span = value
            .as_deref()
            .map_or(radix_lexer::Span::default(), |expr| expr.span);
        let unresolved_edge_value = value.as_deref().is_some_and(|expr| {
            self.tensor_edge_policies
                .shifted_expressions
                .contains(&expr.id)
        });
        if let Some(expr) = value.as_deref() {
            self.current_shape_return_expr = Some(expr.id);
        }
        let value_ty = match value {
            Some(expr) => {
                if let Some(expected) = self.current_return {
                    self.check_store_expr(expr, expected, StoreSite::new(HirStoreKind::Return))
                } else {
                    self.check_expr(expr)
                }
            }
            None => self.vacuum_type(),
        };

        if unresolved_edge_value {
            self.invalid_operand_types(
                super::intrinsics::ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION,
                value_span,
            );
            self.current_shape_return_expr = previous_shape_return;
            return;
        }

        if let Some(expected) = self.current_return {
            self.unify_carrier_assign(value_ty, expected, span, ISSUE_RETURN_TYPE_MISMATCH);
            self.current_shape_return_expr = previous_shape_return;
            return;
        }
        self.current_shape_return_expr = previous_shape_return;

        match self.inferred_return {
            None => self.inferred_return = Some(value_ty),
            Some(existing) => {
                // WHY: inferred returns join peer branches symmetrically, not assignment.
                self.unify_join(value_ty, existing, span, ISSUE_INCOMPATIBLE_RETURN_TYPES);
            }
        }
    }

    /// The initializer's span, or the declaration site when a local has none.
    fn local_init_span(local: &HirLocal) -> radix_lexer::Span {
        local
            .init
            .as_ref()
            .map_or(radix_lexer::Span::default(), |init| init.span)
    }

    /// Establish a local declaration's type and install its binding.
    ///
    /// The four local shapes deliberately map to different inference policies:
    /// annotated initializers are checked bidirectionally, bare annotations are
    /// accepted as-is, initializer-only locals synthesize from the expression,
    /// and declarations without either side receive a fresh inference variable
    /// that must be resolved before finalization.
    pub fn check_local(&mut self, local: &mut HirLocal) {
        // Z1: a static `const size N = 3` records its literal value so affine
        // extents over `N` fold at the stores that name it.
        if local.size_decl
            && local.size_static
            && let Some(radix_hir::HirExpression {
                kind: radix_hir::HirExpressionKind::Literal(radix_hir::HirLiteral::Int(value)),
                ..
            }) = local.init.as_ref()
        {
            self.static_size_values.insert(local.def_id, *value);
        }
        if let Some(ty) = local.ty {
            let span = Self::local_init_span(local);
            self.reject_tensor_edge_policy_value_type(ty, span);
        }
        if local.is_await {
            self.check_await_local(local);
            return;
        }
        if let Some(init) = &local.init {
            self.reject_copy_into_value(init);
        }

        let inferred = match (&local.ty, &mut local.init) {
            (Some(ty), Some(init)) => {
                // A conversion-directed initializer (`fixum T x ↤ value`) has
                // the binding itself as its assignment target; the written type
                // is the conversion destination. Make the binding visible to
                // lvalue resolution while the initializer is checked.
                if matches!(init.kind, HirExpressionKind::ConversioAssign { .. }) {
                    self.insert_binding_with_scalar(local.def_id, *ty, false, local.mutable);
                }
                let inferred_slot = self.is_infer(self.resolve_inferred_type(*ty));
                let site = StoreSite::new(HirStoreKind::Declaration).named(local.name);
                // `fixum T x = e` states a compile-time fact (an `=` position).
                let site = if self.compile_time_locals.contains(&local.def_id) {
                    site.compile_time()
                } else {
                    site
                };
                // Z2: a declaration store is a pin boundary — opaque
                // initializer axes record runtime obligations against the
                // annotation's named size terms instead of failing the check.
                let prev_store_expr = self.current_store_expr.replace(init.id);
                let init_ty = self.check_store_expr(init, *ty, site);
                // Z1: an annotated declaration store folds static
                // `const size` values into its affine extents; a provably
                // negative extent fails the size lift at this store.
                if let Type::Tensor(_, shape) = self.types.get(*ty) {
                    let shape = *shape;
                    self.reject_negative_static_affine_extents(shape, init.span);
                }
                if inferred_slot {
                    self.inferred_slot_sources.insert(local.def_id, init.span);
                }
                self.unify_carrier_assign(
                    init_ty,
                    *ty,
                    init.span,
                    ISSUE_INITIALIZER_ANNOTATION_MISMATCH,
                );
                self.current_store_expr = prev_store_expr;
                if self.types.is_own_view(*ty) {
                    self.check_own_bind_rhs(init);
                }
                super::tensor_index::record_shape_witness_from_init(self, local.def_id, init);
                self.warn_union_annotation_guardrails(*ty, init.span);
                *ty
            }
            (Some(ty), None) => {
                // Bare-annotation locals carry no own span; the declaration
                // site is the best available teach location.
                self.warn_union_annotation_guardrails(*ty, radix_lexer::Span::default());
                *ty
            }
            (None, Some(init)) => {
                let inferred = self.check_expr(init);
                let site = StoreSite::new(HirStoreKind::Declaration).named(local.name);
                let inferred = self.word_inferred_slot(init, inferred, site);
                self.inferred_slot_sources.insert(local.def_id, init.span);
                super::tensor_index::record_shape_witness_from_init(self, local.def_id, init);
                inferred
            }
            (None, None) => self.fresh_infer(),
        };
        if let Some(init) = local.init.as_ref() {
            self.invalidate_assignments_in_expr(init);
        }

        if local.ty.is_none() {
            local.ty = Some(inferred);
        }

        if let Some(init) = local.init.as_ref() {
            if self
                .tensor_edge_policies
                .shifted_expressions
                .contains(&init.id)
            {
                self.tensor_edge_policies
                    .shifted_bindings
                    .insert(local.def_id);
            }
            if let Some(context) = self
                .tensor_edge_policies
                .shifted_view_expressions
                .get(&init.id)
                .copied()
            {
                self.tensor_edge_policies
                    .shifted_view_bindings
                    .insert(local.def_id, context);
            }
        }

        let scalar_text = local
            .init
            .as_ref()
            .is_some_and(|init| self.is_provably_single_scalar_text(init));
        self.reject_read_only_share_into_var(local, inferred);
        self.record_binding_name(local.def_id, local.name);
        self.insert_binding_with_scalar(local.def_id, inferred, scalar_text, local.mutable);
        if !local.mutable {
            self.const_locals.insert(local.def_id);
        }
    }

    /// `const` is deep (need, rule 2): a plain `var T y ← x` of a
    /// shareable (container or genus) value read through a `const` local
    /// would make `y` a writable name for state `x` promises not to change.
    /// The legal spellings are `← copy x` (an independent value) and
    /// `← own x` (take it over); scalars copy by value and stay legal.
    ///
    /// A `ref` parameter source is not rejected here: the borrow pass already
    /// tracks the alias and refuses its mutation (SEM056).
    fn reject_read_only_share_into_var(&mut self, local: &HirLocal, slot_ty: TypeId) {
        if !local.mutable {
            return;
        }
        let Some(init) = local.init.as_ref() else {
            return;
        };
        let resolved = self.resolve_inferred_type(slot_ty);
        let (marker, payload) = self.types.peel_access_marker(resolved);
        // `copy`/`own` are the legal spellings; `mut`/`ref` views are the
        // borrow pass's rules.
        if marker.is_some() || !self.is_shareable_payload(payload) {
            return;
        }
        let Some(root) = Self::place_root(init) else {
            return;
        };
        if !self.const_locals.contains(&root) {
            return;
        }
        let name_of = |checker: &Self, symbol: Option<Symbol>| {
            symbol
                .and_then(|symbol| checker.interner.map(|interner| interner.resolve(symbol)))
                .unwrap_or("value")
                .to_owned()
        };
        let binding = name_of(self, self.binding_names.get(&root).copied());
        let name = name_of(self, Some(local.name));
        self.error_issue_with_args(
            SemanticErrorKind::ConstSharedIntoVar,
            ISSUE_CONST_SHARED_INTO_VAR,
            init.span,
            &[("binding", binding), ("name", name)],
        );
    }

    /// A container or genus instance: a value a plain bind shares rather than
    /// copies.
    fn is_shareable_payload(&self, ty: TypeId) -> bool {
        let ty = self.resolve_inferred_type(ty);
        if self.types.is_aliasing_container_through_option(ty) {
            return true;
        }
        match self.types.get(ty) {
            Type::Struct(_) | Type::Interface(_) => true,
            Type::Option(inner) | Type::Alias(_, inner) => self.is_shareable_payload(*inner),
            _ => false,
        }
    }

    /// The local at the root of a path/field/index/deref chain.
    fn place_root(expr: &HirExpression) -> Option<radix_hir::DefId> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => Some(*def_id),
            HirExpressionKind::Field(object, _)
            | HirExpressionKind::Index(object, _)
            | HirExpressionKind::Deref(object) => Self::place_root(object),
            _ => None,
        }
    }

    fn check_await_local(&mut self, local: &mut HirLocal) {
        if !self.current_await_context {
            let span = Self::local_init_span(local);
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_AWAIT_BIND_REQUIRES_ASYNC_CONTEXT,
                span,
            );
        }

        let awaited = if let Some(init) = &mut local.init {
            let future_ty = self.check_expr(init);
            let awaited =
                self.await_success_type(future_ty, init.span, ISSUE_AWAIT_BIND_REQUIRES_PROMISSUM);
            super::tensor_index::record_shape_witness_from_init(self, local.def_id, init);
            self.invalidate_assignments_in_expr(init);
            awaited
        } else {
            self.error_type
        };

        let local_ty = if let Some(ty) = local.ty {
            // Row 2: the awaited value is a store into the slot. A narrowing
            // integer result is checked under the slot's policy after the
            // await; the store rides the local (the awaited value has no node).
            if self.store_narrowing_target(awaited, ty).is_some() {
                let site = StoreSite::new(HirStoreKind::AwaitBinding).named(local.name);
                local.await_store = Some(self.hir_store_site(site));
                ty
            } else {
                self.unify_assign_issue(
                    awaited,
                    ty,
                    Self::local_init_span(local),
                    ISSUE_INITIALIZER_ANNOTATION_MISMATCH,
                );
                ty
            }
        } else {
            local.ty = Some(awaited);
            awaited
        };

        let scalar_text = local
            .init
            .as_ref()
            .is_some_and(|init| self.is_provably_single_scalar_text(init));
        self.record_binding_name(local.def_id, local.name);
        self.insert_binding_with_scalar(local.def_id, local_ty, scalar_text, local.mutable);
    }

    pub fn await_success_type(
        &mut self,
        future_ty: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        match self.types.get(self.resolve_inferred_type(future_ty)) {
            Type::Promissum(inner) => *inner,
            Type::PromissumFailable(success, alternate) => {
                let success = *success;
                let alternate = *alternate;
                self.check_await_alternate_channel(alternate, span);
                success
            }
            _ => {
                self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
                self.error_type
            }
        }
    }

    fn check_await_alternate_channel(&mut self, alternate: TypeId, span: radix_lexer::Span) {
        let Some(error_ty) = self.current_error.map(|sink| match sink {
            ErrorSink::Function(ty) | ErrorSink::Local(ty) => ty,
            // UT: an awaited failable's alternate exit accumulates into the
            // trap hole — same funnel.
            ErrorSink::Trap(var) => self.trap_sink_type(var),
        }) else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_AWAIT_FAILABLE_REQUIRES_ALTERNATE_EXIT,
                span,
            );
            return;
        };
        self.unify_assign_issue(
            alternate,
            error_ty,
            span,
            ISSUE_AWAIT_FAILABLE_ALTERNATE_TYPE_MISMATCH,
        );
    }

    /// Check one statement for side effects on type state and lexical scope.
    ///
    /// Most expression-like constructs delegate to expression checking. The
    /// statement layer mainly enforces that discarded values are resolved and
    /// that `redde` is routed through the function-return accumulator instead of
    /// being treated as an ordinary expression result.
    pub fn check_stmt(&mut self, stmt: &mut HirStatement) {
        match &mut stmt.kind {
            HirStatementKind::Local(local) => {
                if let Some(ty) = local.ty {
                    self.check_hashable_keys(ty, stmt.span);
                }
                self.check_local(local);
            }
            HirStatementKind::Expr(expr) => {
                let expr_ty = self.check_expr(expr);
                // U1R: an `adfirma` whose condition narrows a binding holds
                // for the rest of the block (failure aborts before control
                // continues), so register the override after checking it.
                self.apply_assert_refinement(expr);
                self.invalidate_assignments_in_expr(expr);
                if self.is_infer(self.resolve_inferred_type(expr_ty)) {
                    let vacuum = self.vacuum_type();
                    self.unify_assign_issue(
                        expr_ty,
                        vacuum,
                        expr.span,
                        ISSUE_IGNORED_EXPR_RESULT_INFER,
                    );
                }
            }
            HirStatementKind::Redde(value) => self.check_return(value.as_mut(), stmt.span),
            HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc_dec) => {
                self.check_inc_dec(inc_dec.op, &mut inc_dec.target, stmt.span);
                self.invalidate_assignments_in_expr(&inc_dec.target);
            }
            HirStatementKind::Custodi(custodi) => {
                for clause in &mut custodi.clauses {
                    self.check_condition(&mut clause.cond);
                    self.check_block(&mut clause.body, None);
                }
            }
        }
    }

    /// Check a lexical block and return its value type.
    ///
    /// Blocks are expression-compatible in Faber, so callers can pass an
    /// expected type that guides the trailing expression. Non-expression blocks
    /// produce `vacuum`; this keeps statement-only bodies explicit without
    /// inventing a bottom type for control-flow statements.
    pub fn check_block(&mut self, block: &mut HirBlock, expected: Option<TypeId>) -> TypeId {
        self.push_scope();
        self.push_assert_refinement_frame();
        for stmt in &mut block.statements {
            self.check_stmt(stmt);
        }
        let ty = if let Some(expr) = &mut block.expr {
            self.check_expr_with_expected(expr, expected)
        } else {
            self.vacuum_type()
        };
        self.pop_assert_refinement_frame();
        self.pop_scope();
        ty
    }
}
