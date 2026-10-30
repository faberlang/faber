//! Final HIR type annotation pass for typechecking.
//!
//! Checking can leave HIR nodes with aliases or inference variables while
//! constraints are still being gathered. Finalization is the cleanup pass that
//! walks the already-checked HIR, resolves checker-local substitutions and
//! semantic aliases, and writes the stable table-local `TypeId`s back onto nodes.
//!
//! RESPONSIBILITY BOUNDARY
//! =======================
//! Finalization does not introduce language behavior. It does not make
//! `ignotum` concrete, infer missing public contracts by policy, or reinterpret
//! `nihil` as optional. If an inference variable cannot be resolved by the
//! constraints already discovered, finalization reports a missing annotation and
//! leaves the erroneous state visible to the caller through diagnostics.

use crate::{WarningKind, format_faber_type};
use radix_hir::HirField;
use radix_hir::visit::{
    HirVisitorMut, walk_expr_mut, walk_field_mut, walk_function_mut, walk_local_mut, walk_stmt_mut,
};
use radix_lexer::Span;

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

// Per-`InferVar` guard against double WARN016/WARN017 emission across the
// decl + init HIR nodes that share one hole (THU-U-05). Finalize is the
// single solve seam, so each hole variable is classified exactly once per
// program; cleared at the start of every `finalize_hir` run.
thread_local! {
    static WARNED_HOLE_VARS: RefCell<std::collections::HashSet<InferVar>> =
        RefCell::new(std::collections::HashSet::new());
}

impl TypeChecker<'_> {
    /// Resolve all type annotations on the checked HIR.
    pub fn finalize_hir(&mut self, hir: &mut HirModule) {
        WARNED_HOLE_VARS.with(|cell| cell.borrow_mut().clear());
        self.visit_program_mut(hir);
    }

    /// Canonicalize the semantic witnesses of recorded authored type forms
    /// after inference, so the sidecar never depends on checker-local
    /// substitution state. This writes only the witness `TypeId`s inside form
    /// records; semantic decisions never consult presentation.
    pub fn finalize_authored_forms(&mut self, presentation: &mut HirPresentation) {
        for authored in &mut presentation.authored_types {
            self.canonicalize_form(&mut authored.form);
        }
    }

    fn canonicalize_form(&mut self, form: &mut HirTypeForm) {
        form.ty = self.canonicalize_type(form.ty);
        for arg in &mut form.args {
            if let HirTypeFormArg::Type(child) = arg {
                self.canonicalize_form(child);
            }
        }
    }

    /// Canonicalize a type after inference, rebuilding compound shapes so nested
    /// inference variables do not survive into MIR lowering.
    ///
    /// BO-S-5: memoize by input `TypeId` (pure over frozen substitutions).
    pub(super) fn canonicalize_type(&mut self, ty: TypeId) -> TypeId {
        if let Some(&cached) = self.canonical_cache.get(&ty) {
            return cached;
        }
        let result = self.canonicalize_type_uncached(ty);
        self.canonical_cache.insert(ty, result);
        result
    }

    fn canonicalize_type_uncached(&mut self, ty: TypeId) -> TypeId {
        let resolved = self.resolve_inferred_type(ty);
        // D6.11 erasure: `sermo<O, R>` is compile-time only; the finalized
        // HIR carries the unchanged carrier every backend matches.
        if let Some(erased) = crate::builtins::frame_types::erase_conversation_type(
            self.types,
            self.resolver,
            resolved,
        ) {
            return erased;
        }
        match self.types.get(resolved).clone() {
            Type::Infer(var) => {
                let subst = self.substitutions.borrow().get(&var).copied();
                if let Some(subst) = subst {
                    self.canonicalize_type(subst)
                } else {
                    resolved
                }
            }
            Type::Array(inner) => {
                let inner = self.canonicalize_type(inner);
                self.types.array(inner)
            }
            Type::Map(key, value) => {
                let key = self.canonicalize_type(key);
                let value = self.canonicalize_type(value);
                self.types.map(key, value)
            }
            Type::Ledger(key, value) => {
                let key = self.canonicalize_type(key);
                let value = self.canonicalize_type(value);
                self.types.ledger(key, value)
            }
            Type::Set(inner) => {
                let inner = self.canonicalize_type(inner);
                self.types.set(inner)
            }
            Type::Promissum(inner) => {
                let inner = self.canonicalize_type(inner);
                self.types.promissum(inner)
            }
            Type::PromissumFailable(success, alternate) => {
                let success = self.canonicalize_type(success);
                let alternate = self.canonicalize_type(alternate);
                self.types.promissum_failable(success, alternate)
            }
            Type::Cursor(inner) => {
                let inner = self.canonicalize_type(inner);
                self.types.cursor(inner)
            }
            Type::AsyncCursor(item, alternate) => {
                let item = self.canonicalize_type(item);
                let alternate = self.canonicalize_type(alternate);
                self.types.async_cursor(item, alternate)
            }
            Type::Option(inner) => {
                let inner = self.canonicalize_type(inner);
                self.types.option(inner)
            }
            Type::Ref(mutability, inner) => {
                let inner = self.canonicalize_type(inner);
                self.types.reference(mutability, inner)
            }
            Type::Record(fields) => {
                let canonical = fields
                    .into_iter()
                    .map(|(name, field_ty)| (name, self.canonicalize_type(field_ty)))
                    .collect();
                self.types.intern(Type::Record(canonical))
            }
            Type::Tuple(elements) => {
                let canonical = elements
                    .into_iter()
                    .map(|elem| self.canonicalize_type(elem))
                    .collect();
                self.types.intern(Type::Tuple(canonical))
            }
            Type::Union(members) => {
                let canonical = members
                    .into_iter()
                    .map(|member| self.canonicalize_type(member))
                    .collect();
                self.types.intern(Type::Union(canonical))
            }
            Type::Func(sig) => {
                let params = sig
                    .params
                    .into_iter()
                    .map(|param| ParamType {
                        ty: self.canonicalize_type(param.ty),
                        ..param
                    })
                    .collect();
                let ret = self.canonicalize_type(sig.ret);
                let err = sig.err.map(|err_ty| self.canonicalize_type(err_ty));
                self.types.function(FuncSig {
                    type_params: sig.type_params.clone(),
                    params,
                    ret,
                    err,
                    is_async: sig.is_async,
                    is_generator: sig.is_generator,
                })
            }
            Type::Tensor(elem, shape) => {
                let elem = self.canonicalize_type(elem);
                let shape = self.canonicalize_index(shape);
                self.types.tensor_with_shape(elem, shape)
            }
            Type::Vector(elem, width) => {
                let elem = self.canonicalize_type(elem);
                let width = self.canonicalize_index(width);
                self.types.vector_with_width(elem, width)
            }
            Type::Matrix(elem, shape) => {
                let elem = self.canonicalize_type(elem);
                let shape = self.canonicalize_index(shape);
                self.types.matrix_with_shape(elem, shape)
            }
            Type::Sparsa(elem, shape) => {
                let elem = self.canonicalize_type(elem);
                let shape = self.canonicalize_index(shape);
                self.types.sparsa_with_shape(elem, shape)
            }
            Type::BoundedAscii { capacity } => {
                let capacity = self.canonicalize_index(capacity);
                self.types.bounded_ascii(capacity)
            }
            Type::BoundedTextus { capacity } => {
                let capacity = self.canonicalize_index(capacity);
                self.types.bounded_textus(capacity)
            }
            Type::BoundedOcteti { capacity } => {
                let capacity = self.canonicalize_index(capacity);
                self.types.bounded_octeti(capacity)
            }
            Type::BoundedArray { element, capacity } => {
                let element = self.canonicalize_type(element);
                let capacity = self.canonicalize_index(capacity);
                self.types.bounded_array(element, capacity)
            }
            Type::Applied(base, args) => {
                // Rebuild the application so nested holes in the arguments
                // (a `capta` boundary's `T` slot rides here as
                // `trapped<R, ∪>`) solve with the rest of the type.
                let base = self.canonicalize_type(base);
                let args = args
                    .into_iter()
                    .map(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => {
                            radix_types::AppliedArg::Type(self.canonicalize_type(id))
                        }
                        other => other,
                    })
                    .collect::<Vec<_>>();
                self.types.intern(Type::Applied(base, args))
            }
            Type::InferUnion(var) => {
                // THU-U-05 solve: the accumulated member set is the answer.
                // `union_members` is frozen once checking converges; finalize
                // is the only reader. Members may still contain pending `_`
                // holes (cross-shape flows), so each member is canonicalized
                // before the solve.
                let members = self.union_members_snapshot(var);
                if members.is_empty() {
                    // No constraints reached the hole: leave it visible so
                    // `report_unresolved_type_inference` can raise the
                    // `cannot_infer_union_type` error instead of leaking an
                    // open hole past finalize.
                    return resolved;
                }
                let canonical = self.canonicalize_union_members(members);
                match canonical.len() {
                    // All members errored out: keep the hole unresolved so the
                    // missing-annotation error still fires.
                    0 => resolved,
                    // Degenerate solve: `∪` collapsed to one inhabitant.
                    // WARN017 is emitted by the finalize visitor site (it owns
                    // the span); the solve itself stays silent here.
                    1 => canonical[0],
                    _ => self.types.intern(Type::Union(canonical)),
                }
            }
            _ => resolved,
        }
    }

    /// Read the accumulated member snapshot for a `∪` hole. `union_members`
    /// is frozen once checking converges; finalize is the only reader.
    fn union_members_snapshot(&self, var: InferVar) -> Vec<TypeId> {
        let borrowed = self.union_members.borrow();
        borrowed.get(&var).cloned().unwrap_or_default()
    }

    /// Canonicalize a `∪` hole's accumulated member set: drop errored
    /// members, canonicalize each survivor, and deduplicate while preserving
    /// accumulation order. Shared by the solve ([`Self::canonicalize_type_uncached`])
    /// and the WARN017 shape reader ([`Self::collect_hole_shape_warnings`]).
    fn canonicalize_union_members(&mut self, members: Vec<TypeId>) -> Vec<TypeId> {
        let mut canonical: Vec<TypeId> = Vec::new();
        for member in members {
            if member == self.error_type {
                continue;
            }
            let c = self.canonicalize_type(member);
            if c == self.error_type {
                continue;
            }
            if !canonical
                .iter()
                .any(|existing| self.types.equals(*existing, c))
            {
                canonical.push(c);
            }
        }
        canonical
    }

    /// Finalize a declaration type that may be part of a public contract.
    ///
    /// Declarations cannot silently keep unresolved inference variables because
    /// later passes and generated code treat their `TypeId`s as stable contract
    /// facts. Missing constraints become missing-annotation diagnostics.
    fn finalize_decl_type(&mut self, ty: &mut Option<TypeId>, span: Span, issue: &'static str) {
        if let Some(current) = *ty {
            self.collect_hole_shape_warnings(current, span);
            let canonical = self.canonicalize_type(current);
            self.report_unresolved_type_inference(canonical, span, issue);
            *ty = Some(canonical);
        }
    }

    fn finalize_required_type(&mut self, ty: &mut TypeId, span: Span, issue: &'static str) {
        self.collect_hole_shape_warnings(*ty, span);
        let canonical = self.canonicalize_type(*ty);
        self.report_unresolved_type_inference(canonical, span, issue);
        *ty = canonical;
    }

    /// Finalize a parameter slot: write the solver's normalized result back
    /// onto the HIR param's type. A solved `∪` hole materializes as its
    /// normalized member set so exported signatures show the solve (N4a
    /// writeback → N4b seam). Unsolved holes stay visible: the file-interface
    /// seam fails closed on `Type::Infer*`, and body-use diagnostics already
    /// own the unresolved case.
    fn finalize_param_type(&mut self, param: &mut HirParam) {
        param.ty = self.canonicalize_type(param.ty);
    }

    /// Finalize an expression annotation after all surrounding constraints ran.
    fn finalize_expr_type(&mut self, ty: &mut Option<TypeId>, span: Span) {
        if let Some(current) = *ty {
            self.collect_hole_shape_warnings(current, span);
            let canonical = self.canonicalize_type(current);
            if !self.report_unresolved_type_inference(
                canonical,
                span,
                "cannot_infer_expression_type",
            ) {
                *ty = Some(canonical);
            }
        }
    }

    /// An empty list may acquire its element type from later uses. Diagnose
    /// only after those constraints have converged; conversion to `valor`
    /// erases the list shape and supplies no element type of its own.
    fn finalize_empty_list_type(&mut self, expr: &HirExpression) {
        let HirExpressionKind::Array(elements) = &expr.kind else {
            return;
        };
        if !elements.is_empty() {
            return;
        }
        let Some(ty) = expr.ty else {
            return;
        };
        let ty = self.canonicalize_type(ty);
        let element = match self.types.get(ty) {
            Type::Array(element)
            | Type::BoundedArray { element, .. }
            | Type::RangedArray { element, .. } => *element,
            _ => return,
        };
        if self.is_infer(element) {
            self.missing_type_annotation("cannot_infer_empty_list_element_type", expr.span);
        }
    }

    /// Report missing type or index inference; returns `true` when a diagnostic was emitted.
    fn report_unresolved_type_inference(
        &mut self,
        ty: TypeId,
        span: Span,
        issue: &'static str,
    ) -> bool {
        if self.is_infer(ty) {
            self.missing_type_annotation(issue, span);
            return true;
        }
        if matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::MarkerHole { .. }
        ) {
            self.missing_type_annotation("marker_width_unsolved", span);
            return true;
        }
        // THU-U-05: an unsolved `∪` hole is a missing-annotation *error*, never
        // a warning. The check walks compound shapes so a nested hole (e.g.
        // `lista<∪>` with no element flows) cannot survive past finalize.
        if self.type_has_unsolved_union_hole(ty) {
            self.missing_type_annotation("cannot_infer_union_type", span);
            return true;
        }
        if self.tensor_shape_has_unresolved_hole(ty) {
            let peeled = self.resolve_inferred_type(ty);
            let issue = match self.types.get(peeled) {
                Type::BoundedAscii { .. }
                | Type::BoundedTextus { .. }
                | Type::BoundedOcteti { .. }
                | Type::BoundedArray { .. } => "bounded_capacity_unsolved",
                _ => "cannot_infer_tensor_shape",
            };
            self.missing_type_annotation(issue, span);
            return true;
        }
        false
    }

    /// Whether any nested position of `ty` still names a `∪` hole.
    ///
    /// A solved hole canonicalizes to its member set (or a single member) and
    /// no longer appears here; only genuinely unsolved holes are found.
    fn type_has_unsolved_union_hole(&self, ty: TypeId) -> bool {
        match self.types.get(ty) {
            Type::InferUnion(_) => true,
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Option(inner)
            | Type::Ref(_, inner) => self.type_has_unsolved_union_hole(*inner),
            Type::Map(key, value) | Type::Ledger(key, value) => {
                self.type_has_unsolved_union_hole(*key) || self.type_has_unsolved_union_hole(*value)
            }
            Type::PromissumFailable(success, alternate) => {
                self.type_has_unsolved_union_hole(*success)
                    || self.type_has_unsolved_union_hole(*alternate)
            }
            Type::AsyncCursor(item, alternate) => {
                self.type_has_unsolved_union_hole(*item)
                    || self.type_has_unsolved_union_hole(*alternate)
            }
            Type::Record(fields) => fields
                .values()
                .any(|field_ty| self.type_has_unsolved_union_hole(*field_ty)),
            Type::Tuple(elements) => elements
                .iter()
                .any(|elem| self.type_has_unsolved_union_hole(*elem)),
            Type::Union(members) => members
                .iter()
                .any(|member| self.type_has_unsolved_union_hole(*member)),
            Type::Func(sig) => {
                sig.params
                    .iter()
                    .any(|param| self.type_has_unsolved_union_hole(param.ty))
                    || self.type_has_unsolved_union_hole(sig.ret)
                    || sig
                        .err
                        .is_some_and(|err| self.type_has_unsolved_union_hole(err))
            }
            Type::Tensor(elem, _)
            | Type::Vector(elem, _)
            | Type::Matrix(elem, _)
            | Type::Sparsa(elem, _)
            | Type::Atomic(elem)
            | Type::Intervallum(elem, _) => self.type_has_unsolved_union_hole(*elem),
            Type::Applied(base, args) => {
                self.type_has_unsolved_union_hole(*base)
                    || args.iter().any(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => self.type_has_unsolved_union_hole(*id),
                        radix_types::AppliedArg::Size(_) => false,
                    })
            }
            _ => false,
        }
    }

    /// Multi-member shape rule for the WARN016 side of the hole contract
    /// (goal req 10, locked): after alias peeling, a type is multi-member
    /// when it is a `Union` with two or more **non-`nihil`** structural
    /// members. Canonical nullable `T ∪ nihil` (`Type::Option`) is a single
    /// optional inhabitant and is never multi-member.
    fn is_multi_member_union(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved) {
            Type::Union(members) => {
                members
                    .iter()
                    .filter(|member| {
                        !matches!(self.types.get(**member), Type::Primitive(Primitive::Nihil))
                    })
                    .count()
                    >= 2
            }
            _ => false,
        }
    }

    /// Faber source spelling of a solved type for the warning message args
    /// (U-03 catalog contract: WARN016 reads `{union}`, WARN017 reads `{type}`).
    fn format_type_value(&self, ty: TypeId) -> String {
        match self.interner {
            Some(interner) => format_faber_type(ty, self.types, interner, &|def_id| {
                format!("Def{}", def_id.0)
            }),
            None => format!("{:?}", self.types.get(ty)),
        }
    }

    /// Emit one shape warning for a hole variable, deduplicated per `InferVar`
    /// so the decl + init nodes sharing the hole produce a single diagnostic.
    fn warn_hole_shape_once(
        &mut self,
        var: InferVar,
        kind: WarningKind,
        issue: &'static str,
        type_arg: &'static str,
        span: Span,
        value: &str,
    ) {
        let first = WARNED_HOLE_VARS.with(|cell| {
            let mut warned = cell.borrow_mut();
            if warned.contains(&var) {
                false
            } else {
                warned.insert(var);
                true
            }
        });
        if !first {
            return;
        }
        let error = SemanticError::structured(SemanticErrorKind::Warning(kind), span)
            .with_arg("issue", issue)
            .with_arg(type_arg, value.to_owned());
        self.errors.push(error);
    }

    /// Classify the hole vars reachable from an annotation/expression type and
    /// emit the cross-shape warnings. Called from the finalize visitor sites
    /// (which own the annotation spans) before canonicalization replaces the
    /// holes with their solutions.
    pub fn collect_hole_shape_warnings(&mut self, ty: TypeId, span: Span) {
        match self.types.get(ty).clone() {
            Type::Infer(var) => {
                // Only source `_` holes qualify for WARN016: checker-internal
                // inference vars (`fresh_infer`) are not annotation holes and
                // must never warn when a flow happens to resolve them to a
                // union.
                if self.infer_ids.contains_key(&var) {
                    return;
                }
                let subst = self.substitutions.borrow().get(&var).copied();
                if let Some(subst) = subst {
                    let canonical = self.canonicalize_type(subst);
                    if self.is_multi_member_union(canonical) {
                        let display = self.format_type_value(canonical);
                        self.warn_hole_shape_once(
                            var,
                            WarningKind::MonomorphicHoleSolvedToUnion,
                            "monomorphic_hole_solved_to_union",
                            "union",
                            span,
                            &display,
                        );
                    }
                }
            }
            Type::InferUnion(var) => {
                // A `capta` boundary's `T` slot solves to exactly the escaping
                // error types: one member is the healthy single-error-channel
                // case, never a cross-shape surprise, so WARN017 stays silent
                // (the empty channel carries `trap_error_channel_empty`).
                if self.trap_error_holes.contains(&var) {
                    return;
                }
                let members = self.union_members_snapshot(var);
                let canonical = self.canonicalize_union_members(members);
                if canonical.len() == 1 {
                    let display = self.format_type_value(canonical[0]);
                    self.warn_hole_shape_once(
                        var,
                        WarningKind::UnionHoleSolvedMonomorphically,
                        "union_hole_solved_monomorphically",
                        "type",
                        span,
                        &display,
                    );
                }
            }
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Option(inner)
            | Type::Ref(_, inner) => self.collect_hole_shape_warnings(inner, span),
            Type::Map(key, value) | Type::Ledger(key, value) => {
                self.collect_hole_shape_warnings(key, span);
                self.collect_hole_shape_warnings(value, span);
            }
            Type::PromissumFailable(success, alternate) => {
                self.collect_hole_shape_warnings(success, span);
                self.collect_hole_shape_warnings(alternate, span);
            }
            Type::AsyncCursor(item, alternate) => {
                self.collect_hole_shape_warnings(item, span);
                self.collect_hole_shape_warnings(alternate, span);
            }
            Type::Record(fields) => {
                for (_, field_ty) in fields {
                    self.collect_hole_shape_warnings(field_ty, span);
                }
            }
            Type::Tuple(elements) => {
                for elem in elements {
                    self.collect_hole_shape_warnings(elem, span);
                }
            }
            Type::Union(members) => {
                for member in members {
                    self.collect_hole_shape_warnings(member, span);
                }
            }
            Type::Func(sig) => {
                for param in sig.params {
                    self.collect_hole_shape_warnings(param.ty, span);
                }
                self.collect_hole_shape_warnings(sig.ret, span);
                if let Some(err) = sig.err {
                    self.collect_hole_shape_warnings(err, span);
                }
            }
            Type::Tensor(elem, _)
            | Type::Vector(elem, _)
            | Type::Matrix(elem, _)
            | Type::Sparsa(elem, _)
            | Type::Atomic(elem)
            | Type::Intervallum(elem, _) => self.collect_hole_shape_warnings(elem, span),
            Type::Applied(base, args) => {
                self.collect_hole_shape_warnings(base, span);
                for arg in args {
                    if let radix_types::AppliedArg::Type(id) = arg {
                        self.collect_hole_shape_warnings(id, span);
                    }
                }
            }
            // Aliases and all non-holey shapes: explicit alias uses are never
            // holes, and the remaining variants carry no nested types.
            _ => {}
        }
    }

    /// Resolve optional metadata types that do not require an annotation error.
    ///
    /// Handler binding annotations are derived from local control-flow analysis;
    /// if they exist, they should be canonicalized for downstream passes.
    fn resolve_optional_type(&mut self, ty: &mut Option<TypeId>) {
        if let Some(current) = *ty {
            *ty = Some(self.canonicalize_type(current));
        }
    }

    fn finalize_function(&mut self, function: &mut HirFunction, span: Span) {
        self.finalize_decl_type(&mut function.ret_ty, span, "cannot_infer_return_type");
        self.finalize_decl_type(
            &mut function.err_ty,
            span,
            "cannot_infer_alternate_exit_type",
        );
        for param in &mut function.params {
            self.finalize_param_type(param);
        }

        walk_function_mut(self, function);
    }

    /// Solve a bounded-list capacity hole when its initializer is an exact
    /// array-literal witness. Dynamic expressions and spread elements decline;
    /// they retain the existing SEM014 behavior instead of inventing a bound.
    fn solve_bounded_array_capacity_from_literal(&mut self, ty: TypeId, expr: &HirExpression) {
        let resolved = self.resolve_inferred_type(ty);
        let Type::BoundedArray { capacity, .. } = self.types.get(resolved).clone() else {
            return;
        };
        if !self.is_unresolved_index_hole(capacity) {
            return;
        }
        let HirExpressionKind::Array(elements) = &expr.kind else {
            return;
        };
        if elements
            .iter()
            .any(|element| matches!(element, HirArrayElement::Spread(_)))
        {
            return;
        }
        let witness = self
            .types
            .intern_index(IndexExpr::Literal(elements.len() as u64));
        self.constrain_index_equal(capacity, witness, expr.span);
    }

    fn finalize_local(&mut self, local: &mut HirLocal, span: Span) {
        if let (Some(ty), Some(init)) = (local.ty, local.init.as_ref()) {
            self.solve_bounded_array_capacity_from_literal(ty, init);
        }
        self.finalize_decl_type(&mut local.ty, span, "cannot_infer_variable_type");

        walk_local_mut(self, local);
    }
}

impl HirVisitorMut for TypeChecker<'_> {
    /// Finalize item-level type contracts and then descend into bodies.
    fn visit_item_mut(&mut self, item: &mut HirItem) {
        match &mut item.kind {
            HirItemKind::Function(function) => self.finalize_function(function, item.span),
            HirItemKind::Constant(const_item) => {
                self.finalize_decl_type(
                    &mut const_item.ty,
                    const_item.value.span,
                    "cannot_infer_constant_type",
                );
                self.visit_expr_mut(&mut const_item.value);
            }
            HirItemKind::Struct(struct_item) => {
                for field in &mut struct_item.fields {
                    self.visit_field_mut(field);
                }
                for method in &mut struct_item.methods {
                    self.finalize_function(&mut method.func, method.span);
                }
            }
            _ => {}
        }
    }

    /// Finalize function return and alternate-exit contracts before body nodes.
    fn visit_function_mut(&mut self, function: &mut HirFunction) {
        let span = function
            .body
            .as_ref()
            .map_or(Span::default(), |body| body.span);
        self.finalize_function(function, span);
    }

    fn visit_field_mut(&mut self, field: &mut HirField) {
        self.finalize_required_type(&mut field.ty, field.span, "cannot_infer_field_type");

        walk_field_mut(self, field);
    }

    fn visit_stmt_mut(&mut self, stmt: &mut HirStatement) {
        if let HirStatementKind::Local(local) = &mut stmt.kind {
            self.finalize_local(local, stmt.span);
            return;
        }

        walk_stmt_mut(self, stmt);
    }

    /// Finalize local declaration types before visiting their initializer.
    fn visit_local_mut(&mut self, local: &mut HirLocal) {
        let span = local
            .init
            .as_ref()
            .map_or(Span::default(), |expr| expr.span);
        self.finalize_local(local, span);
    }

    /// Finalize expression type annotations after inference has converged.
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        self.finalize_expr_type(&mut expr.ty, expr.span);
        self.finalize_empty_list_type(expr);
        // WHY: a closure parameter written with a hole type (`_ m`) is solved
        // from the expected signature (the regex `muta` closure, whose `match`
        // type has no source name, RD-3); write the solved type back so MIR
        // sees the concrete parameter, as for a function parameter.
        if let HirExpressionKind::Clausura(params, ..) = &mut expr.kind {
            for param in params {
                self.finalize_param_type(param);
            }
        }

        walk_expr_mut(self, expr);
    }

    /// Canonicalize handler binding types derived from failable control flow.
    fn visit_cape_mut(&mut self, cape: &mut HirCape) {
        self.resolve_optional_type(&mut cape.binding_ty);

        radix_hir::visit::walk_cape_mut(self, cape);
    }
}
