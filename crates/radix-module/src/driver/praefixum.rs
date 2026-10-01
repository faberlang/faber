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
//! - a construct the runner cannot lower: `praefixum_unsupported`.
//!
//! Faber re-emission (`format`, `convert`) keeps the authored body and never
//! evaluates.

use crate::diagnostics::Diagnostic;
use crate::driver::AnalyzedModule;

/// A build without the MIR runner leaves the bodies in place: analysis and
/// Faber re-emission still work, and every codegen backend refuses an
/// unevaluated `praefixum` (`praefixum_unevaluated`).
#[cfg(not(feature = "runner"))]
pub(crate) fn evaluate(_unit: &mut AnalyzedModule, _name: &str, _body: &str) -> Result<(), Vec<Diagnostic>> {
    Ok(())
}

#[cfg(feature = "runner")]
pub(crate) use evaluation::evaluate;

#[cfg(feature = "runner")]
mod evaluation {
    use super::{AnalyzedModule, Diagnostic};
    use crate::diagnostics::DiagnosticConvert;
    use crate::semantic::{self, SemanticError, SemanticErrorKind};
    use radix_hir::visit::{HirVisitorMut, walk_local_mut};
    use radix_hir::{DefId, HirExpression, HirItemKind, HirLocal};
    use radix_lexer::Span;

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
    pub(crate) fn evaluate(unit: &mut AnalyzedModule, name: &str, body: &str) -> Result<(), Vec<Diagnostic>> {
        let order = match semantic::passes::praefixum::evaluation_order(&unit.hir, &unit.types) {
            Ok(order) => order,
            // Analysis already reported the cycle.
            Err(_) => return Ok(()),
        };
        if order.is_empty() {
            return Ok(());
        }
        let mut errors = Vec::new();
        for site in &order {
            if let Err(error) = evaluate_site(unit, site.def_id, site.span) {
                errors.push(error.with_arg("name", unit.interner.resolve(site.name)));
            }
        }
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

    fn evaluate_site(unit: &mut AnalyzedModule, def_id: DefId, span: Span) -> Result<(), SemanticError> {
        let value = super::run::run_site(unit, def_id, span)?;
        let literal = {
            let declared = with_initializer_mut(unit, def_id, |init| init.ty)
                .flatten()
                .ok_or_else(|| invalid("praefixum_result_not_literal", span))?;
            let mut builder = super::literal::LiteralBuilder::new(unit, span);
            builder.build(&value, declared)?
        };
        let Some(authored) = with_initializer_mut(unit, def_id, |init| std::mem::replace(init, literal)) else {
            return Err(invalid("praefixum_unsupported", span));
        };
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

#[cfg(test)]
#[path = "praefixum_module_constant_test.rs"]
mod module_constant_tests;

#[cfg(test)]
#[path = "praefixum_local_test.rs"]
mod local_tests;
