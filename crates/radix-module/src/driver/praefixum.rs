//! Build-time evaluation of `praefixum { … }` constants (DECISIONS D8.4–D8.6).
//!
//! Semantic analysis has already proved each body pure, acyclic and
//! standalone (`passes::praefixum`). Here, in dependency order, each body
//! becomes the body of a synthetic zero-argument function that returns the
//! declared type; the module is lowered to MIR with only that function and the
//! functions it reaches, and the MIR runner evaluates it under a fixed step
//! budget. The value is rebuilt as a HIR literal of the declared type and
//! replaces the initializer, so every backend sees an ordinary constant.
//! Constants that read a `praefixum` constant fold afterwards
//! (`statica::fold` runs again).
//!
//! The sites are module constants, genus field defaults, and block-level
//! constants inside function bodies (`PraefixumSiteKind::Local`). A local
//! site is found by its definition id wherever it sits, and keeps no authored
//! copy: Faber re-emission never evaluates, so it prints the body itself.
//!
//! FAILURES
//! ========
//! Every failure is a build error on the static (SEM064):
//! - `mori` or a propagated `⇥`: `praefixum_failed`, carrying the message;
//! - the step budget: `praefixum_step_limit`;
//! - no `redde` value: `praefixum_no_result`;
//! - a value with no literal form on every backend (closure, handle, float
//!   infinity, generic genus): `praefixum_result_not_literal`;
//! - a construct the runner cannot lower: `praefixum_unsupported`;
//! - a body that reaches an import (`ns.f()`, `ns.K`, directly or through a
//!   local helper): `praefixum_import_unavailable`, unless the session defers
//!   such modules to the package layer (`Config::defer_imported_praefixum`),
//!   in which case no site of the module evaluates here.
//!
//! Faber re-emission (`format`, `convert`) keeps the authored body and never
//! evaluates.

use crate::diagnostics::Diagnostic;
use crate::driver::AnalyzedModule;

/// A build without the MIR runner leaves the bodies in place: analysis and
/// Faber re-emission still work, and every codegen backend refuses an
/// unevaluated `praefixum` (`praefixum_unevaluated`).
#[cfg(not(feature = "runner"))]
pub(crate) fn evaluate(
    _unit: &mut AnalyzedModule,
    _name: &str,
    _body: &str,
    _defer_imported: bool,
) -> Result<(), Vec<Diagnostic>> {
    Ok(())
}

#[cfg(feature = "runner")]
pub(crate) use evaluation::evaluate;
#[cfg(feature = "runner")]
pub use evaluation::evaluate_deferred;
#[cfg(feature = "runner")]
pub use run::run_linked_function;

#[cfg(feature = "runner")]
mod evaluation {
    use super::{AnalyzedModule, Diagnostic};
    use crate::diagnostics::DiagnosticConvert;
    use crate::semantic::passes::praefixum::{
        ImportedCallee, ImportedEffects, ImportedReference, PraefixumSite, imported_references,
    };
    use crate::semantic::{self, SemanticError, SemanticErrorKind};
    use radix_hir::visit::{HirVisitor, HirVisitorMut, walk_expr, walk_local_mut, walk_stmt};
    use radix_hir::{
        DefId, HirBlock, HirExpression, HirId, HirItemKind, HirLocal, HirOwnerRef, HirSourceAnchorId, HirStatement,
    };
    use radix_lexer::{Span, Symbol};
    use radix_mir_runner::Value;

    /// Step budget for one `praefixum` body. One step is one MIR basic-block
    /// visit; a real initializer stays far below it, an endless loop reaches
    /// it in well under a second.
    pub(crate) const PRAEFIXUM_STEP_BUDGET: u64 = 10_000_000;

    /// Evaluate every `praefixum` static in `unit`, replacing each
    /// initializer with its literal value.
    ///
    /// # Errors
    ///
    /// Returns the diagnostics for every static that failed to evaluate; the
    /// unit must not reach codegen then.
    ///
    /// A site that reaches an import never runs here. With `defer_imported`
    /// the whole module is left unevaluated for the package layer; without
    /// it each such site is refused and the rest evaluate.
    pub(crate) fn evaluate(
        unit: &mut AnalyzedModule,
        name: &str,
        body: &str,
        defer_imported: bool,
    ) -> Result<(), Vec<Diagnostic>> {
        let Ok(order) = semantic::passes::praefixum::evaluation_order(&unit.hir, &unit.types) else {
            // Analysis already reported the cycle.
            return Ok(());
        };
        if order.is_empty() {
            return Ok(());
        }
        let reached: Vec<Option<ImportedReference>> = order
            .iter()
            .map(|site| first_import_reached(unit, site.def_id))
            .collect();
        if defer_imported && reached.iter().any(Option::is_some) {
            return Ok(());
        }
        let mut errors = Vec::new();
        for (site, import) in order.iter().zip(&reached) {
            let outcome = match import {
                Some(reference) => Err(invalid("praefixum_import_unavailable", site.span)
                    .with_arg("callee", spell_import(unit, reference))),
                None => evaluate_site(unit, site.def_id, site.span),
            };
            if let Err(error) = outcome {
                errors.push(error.with_arg("name", unit.interner.resolve(site.name)));
            }
        }
        finish(unit, name, body, errors)
    }

    /// The deferred counterpart of [`evaluate`]: the package layer evaluates
    /// the sites of a module that analysis left unevaluated because they
    /// reach an import.
    ///
    /// Each site, in evaluation order, first sends its import references
    /// (calls, methods, and data members of an import namespace) to `imports`: an
    /// `Effect` is `praefixum_forbidden` with the call chain, an
    /// `Unavailable` is `praefixum_import_unavailable`. A site that passes
    /// goes to `run` as a scratch clone of the unit, filtered and holding the
    /// synthetic evaluation function exactly as the local path builds it, with
    /// the definition id of that function and the site's span. The returned
    /// value is installed by the literal builder the local path uses. After
    /// the last site `statica::fold` runs.
    ///
    /// The scratch clone is the one deep copy of an analyzed module in this
    /// path: the runner takes the module by value and the real unit must keep
    /// its HIR.
    ///
    /// # Errors
    ///
    /// Returns the diagnostics for every site that failed; the unit must not
    /// reach codegen then.
    pub fn evaluate_deferred(
        unit: &mut AnalyzedModule,
        name: &str,
        body: &str,
        imports: &mut dyn ImportedEffects,
        run: &mut dyn FnMut(AnalyzedModule, DefId, Span) -> Result<Value, SemanticError>,
    ) -> Result<(), Vec<Diagnostic>> {
        let Ok(order) = semantic::passes::praefixum::evaluation_order(&unit.hir, &unit.types) else {
            // Analysis already reported the cycle.
            return Ok(());
        };
        let mut errors = Vec::new();
        for site in &order {
            if let Err(error) = evaluate_deferred_site(unit, site, imports, run) {
                errors.push(error.with_arg("name", unit.interner.resolve(site.name)));
            }
        }
        finish(unit, name, body, errors)
    }

    fn evaluate_deferred_site(
        unit: &mut AnalyzedModule,
        site: &PraefixumSite,
        imports: &mut dyn ImportedEffects,
        run: &mut dyn FnMut(AnalyzedModule, DefId, Span) -> Result<Value, SemanticError>,
    ) -> Result<(), SemanticError> {
        let references = imported_references(&unit.hir, &unit.types, site.def_id);
        for reference in references {
            match imports.callee(&reference) {
                ImportedCallee::Pure => {}
                ImportedCallee::Effect { effect, chain } => {
                    let mut hops = vec![
                        unit.interner.resolve(site.name).to_owned(),
                        spell_import(unit, &reference),
                    ];
                    hops.extend(chain.into_iter().skip(1));
                    return Err(invalid("praefixum_forbidden", reference.span)
                        .with_arg("effect", effect)
                        .with_arg("chain", hops.join(" → ")));
                }
                ImportedCallee::Unavailable => {
                    return Err(invalid("praefixum_import_unavailable", site.span)
                        .with_arg("callee", spell_import(unit, &reference)));
                }
            }
        }
        let mut scratch = unit.clone();
        scratch.hir.items = super::run::evaluation_items(&mut scratch, site.def_id, site.span)?;
        scratch.hir.entry = None;
        let value = run(scratch, super::run::EVALUATION_DEF_ID, site.span)?;
        install_value(unit, site.def_id, site.span, &value)
    }

    /// Refold the constants that read an evaluated site, then map every error
    /// to a diagnostic.
    fn finish(
        unit: &mut AnalyzedModule,
        name: &str,
        body: &str,
        mut errors: Vec<SemanticError>,
    ) -> Result<(), Vec<Diagnostic>> {
        if errors.is_empty() {
            let hir = &mut unit.hir;
            errors.extend(semantic::passes::statica::fold(hir, &unit.types, &mut unit.interner));
        }
        if errors.is_empty() {
            return Ok(());
        }
        Err(errors
            .iter()
            .map(|error| Diagnostic::from_semantic_error(name, body, error))
            .collect())
    }

    /// The first import reference of a site that would need another
    /// module's code: a call, a method of a genus declared elsewhere, or a
    /// data member of an import namespace.
    fn first_import_reached(unit: &AnalyzedModule, site: DefId) -> Option<ImportedReference> {
        imported_references(&unit.hir, &unit.types, site)
            .into_iter()
            .next()
    }

    /// `<binding>.<member>` for an import reference.
    fn spell_import(unit: &AnalyzedModule, reference: &ImportedReference) -> String {
        let binding = unit.hir.items.iter().find_map(|item| match &item.kind {
            HirItemKind::Import(import) if !import.selective => import
                .items
                .iter()
                .find(|bound| bound.def_id == reference.namespace)
                .map(|bound| bound.alias.unwrap_or(bound.name)),
            _ => None,
        });
        let member = unit.interner.resolve(reference.member);
        match binding {
            Some(binding) => format!("{}.{member}", unit.interner.resolve(binding)),
            None => member.to_owned(),
        }
    }

    pub(crate) fn invalid(issue: &'static str, span: Span) -> SemanticError {
        SemanticError::structured(SemanticErrorKind::PraefixumInvalid, span).with_arg("issue", issue)
    }

    /// Run `f` on the initializer slot of a site: a top-level constant, a
    /// genus field default, or a block-level constant anywhere in a body.
    pub(crate) fn with_initializer_mut<R>(
        unit: &mut AnalyzedModule,
        def_id: DefId,
        f: impl FnOnce(&mut HirExpression) -> R,
    ) -> Option<R> {
        for item in &mut unit.hir.items {
            match &mut item.kind {
                HirItemKind::Constant(konst) if item.def_id == def_id => return Some(f(&mut konst.value)),
                HirItemKind::Struct(strukt) => {
                    if let Some(field) = strukt
                        .fields
                        .iter_mut()
                        .find(|field| field.def_id == def_id)
                    {
                        return field.init.as_mut().map(f);
                    }
                }
                _ => {}
            }
        }
        let mut finder = LocalFinder { def_id, f: Some(f), out: None };
        finder.visit_program_mut(&mut unit.hir);
        finder.out
    }

    /// Finds a block-level constant by definition id.
    struct LocalFinder<F, R> {
        def_id: DefId,
        f: Option<F>,
        out: Option<R>,
    }

    impl<F: FnOnce(&mut HirExpression) -> R, R> HirVisitorMut for LocalFinder<F, R> {
        fn visit_local_mut(&mut self, local: &mut HirLocal) {
            if local.def_id != self.def_id {
                walk_local_mut(self, local);
            } else if let (Some(init), Some(f)) = (local.init.as_mut(), self.f.take()) {
                self.out = Some(f(init));
            }
        }
    }

    /// The HIR node ids and definition ids a praefixum fold kills: when
    /// `install_value` replaces the initializer, every node of the authored body
    /// leaves the live HIR.
    struct DeadNodes {
        hirs: handroll::FxHashSet<HirId>,
        defs: handroll::FxHashSet<DefId>,
        block_ends: handroll::FxHashSet<HirSourceAnchorId>,
    }

    impl DeadNodes {
        fn of(body: &HirExpression) -> DeadNodes {
            let mut dead = DeadNodes {
                hirs: handroll::FxHashSet::default(),
                defs: handroll::FxHashSet::default(),
                block_ends: handroll::FxHashSet::default(),
            };
            dead.visit_expr(body);
            dead
        }
    }

    impl radix_hir::visit::HirVisitor for DeadNodes {
        fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
            self.defs.insert(def_id);
        }
        fn visit_block(&mut self, block: &HirBlock) {
            if let Some(end) = &block.end {
                self.block_ends.insert(end.id.clone());
            }
            radix_hir::visit::walk_block(self, block);
        }
        fn visit_stmt(&mut self, stmt: &HirStatement) {
            self.hirs.insert(stmt.id);
            walk_stmt(self, stmt);
        }
        fn visit_expr(&mut self, expr: &HirExpression) {
            self.hirs.insert(expr.id);
            walk_expr(self, expr);
        }
    }

    fn evaluate_site(unit: &mut AnalyzedModule, def_id: DefId, span: Span) -> Result<(), SemanticError> {
        let value = super::run::run_site(unit, def_id, span)?;
        install_value(unit, def_id, span, &value)
    }

    /// Replace the initializer of the site `def_id` with the literal of
    /// `value`, keeping the authored body for Faber re-emission.
    fn install_value(unit: &mut AnalyzedModule, def_id: DefId, span: Span, value: &Value) -> Result<(), SemanticError> {
        let literal = {
            let declared = with_initializer_mut(unit, def_id, |init| init.ty)
                .flatten()
                .ok_or_else(|| invalid("praefixum_result_not_literal", span))?;
            let mut builder = super::literal::LiteralBuilder::new(unit, span);
            builder.build(value, declared)?
        };
        let Some(authored) = with_initializer_mut(unit, def_id, |init| std::mem::replace(init, literal)) else {
            return Err(invalid("praefixum_unsupported", span));
        };
        // The authored body left the live HIR: drop presentation and
        // resolution records its dead Defs/Hirs/anchors own, or FHIR
        // validation dangles on them at load.
        let dead = DeadNodes::of(&authored);
        unit.presentation
            .guard_si_ids
            .retain(|id| !dead.hirs.contains(id));
        unit.presentation
            .elige_expr_ids
            .retain(|id| !dead.hirs.contains(id));
        unit.presentation
            .glyph_ids
            .retain(|id| !dead.hirs.contains(id));
        unit.presentation
            .radical_quarta_ids
            .retain(|id| !dead.hirs.contains(id));
        unit.presentation
            .numeric_twin_ids
            .retain(|id| !dead.hirs.contains(id));
        unit.presentation
            .empty_dense_ids
            .retain(|id| !dead.hirs.contains(id));
        unit.presentation
            .inferred_constructor_ids
            .retain(|id| !dead.hirs.contains(id));
        unit.presentation
            .axis_sugar
            .retain(|record| !dead.hirs.contains(&record.id));
        unit.presentation
            .discard_binding_stmts
            .retain(|record| !dead.hirs.contains(&record.expr_id));
        unit.presentation
            .tuple_label_accesses
            .retain(|record| !dead.hirs.contains(&record.expr_id));
        unit.presentation
            .insere_exprs
            .retain(|record| !dead.hirs.contains(&record.expr));
        unit.presentation
            .template_holes
            .retain(|record| !dead.hirs.contains(&record.expr));
        unit.presentation
            .destructure_bindings
            .retain(|record| !record.stmt_ids.iter().any(|id| dead.hirs.contains(id)));
        unit.presentation
            .await_assignment_bindings
            .retain(|def| !dead.defs.contains(def));
        unit.presentation
            .function_modifier_bindings
            .retain(|record| !dead.defs.contains(&record.local));
        unit.presentation
            .owners
            .retain(|owner| match &owner.semantic {
                Some(HirOwnerRef::Hir(id)) => !dead.hirs.contains(id),
                Some(HirOwnerRef::Def(id)) => !dead.defs.contains(id),
                None => owner.kind != radix_hir::HirTriviaOwnerKind::BlockEnd || !dead.block_ends.contains(&owner.id),
            });
        let live_anchors: handroll::FxHashSet<_> = unit
            .presentation
            .owners
            .iter()
            .map(|owner| owner.id.clone())
            .collect();
        unit.presentation
            .attachments
            .retain(|attachment| live_anchors.contains(&attachment.anchor));
        unit.presentation
            .block_ends
            .retain(|end| !dead.block_ends.contains(&end.id));
        unit.presentation
            .authored_types
            .retain(|record| match &record.site.owner {
                HirOwnerRef::Hir(id) => !dead.hirs.contains(id),
                HirOwnerRef::Def(id) => !dead.defs.contains(id),
            });
        unit.presentation
            .compile_time_locals
            .retain(|def_id| !dead.defs.contains(def_id));
        // Resolved-use facts recorded for the same body name locals that just
        // left the live HIR; a stale record fails `check_def` at FHIR load.
        let uses: Vec<semantic::ResolvedUse> = unit
            .resolved_uses
            .to_snapshot()
            .into_iter()
            .filter(|use_site| {
                let local = use_site.local.or(match &use_site.key {
                    semantic::ResolvedUseKey::Local(def_id) => Some(*def_id),
                    semantic::ResolvedUseKey::Portable(_) => None,
                });
                !local.is_some_and(|def_id| dead.defs.contains(&def_id))
                    && !use_site
                        .enclosing
                        .is_some_and(|def_id| dead.defs.contains(&def_id))
            })
            .collect();
        unit.resolved_uses = semantic::ResolvedUseIndex::from_snapshot(uses);
        // Faber re-emission prints the authored body, not the value.
        for item in &mut unit.hir.items {
            if let HirItemKind::Constant(konst) = &mut item.kind
                && item.def_id == def_id
                && let Some(statica) = konst.statica.as_mut()
            {
                statica.authored = Some(Box::new(authored));
                break;
            }
        }
        Ok(())
    }
}

#[cfg(feature = "runner")]
#[path = "praefixum_run.rs"]
mod run;

#[cfg(feature = "runner")]
#[path = "praefixum_literal.rs"]
mod literal;

#[cfg(test)]
#[path = "praefixum_test.rs"]
mod tests;

#[cfg(all(test, feature = "runner"))]
#[path = "praefixum_deferred_test.rs"]
mod deferred;

#[cfg(test)]
#[path = "praefixum_module_constant_test.rs"]
mod module_constant_tests;

#[cfg(test)]
#[path = "praefixum_local_test.rs"]
mod local_tests;
