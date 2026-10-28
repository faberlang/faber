//! Target qualification: the work that needs the build target.
//!
//! Analysis produces a typechecked, target-neutral unit. Qualification then
//! settles what only a build target can: it refuses a target-native import
//! bound to another target (SEM017), folds literal-only float expressions to
//! one literal (F9 ruling 31), and evaluates `praefixum` initializers on the
//! MIR runner so every backend receives literals (D8.5).
//!
//! One function serves every route. Direct compilation calls it at the end of
//! analysis; a unit loaded from a FHIR artifact, which is written without a
//! build target and carries the authored constant expressions and unevaluated
//! `praefixum` bodies, calls it after load. Each step is a no-op on a unit
//! that already went through it, so a route that qualifies twice changes
//! nothing the second time.
//!
//! Source-preserving forms are never qualified: Faber re-emission keeps the
//! authored text, and the FHIR artifact is the neutral form itself. The
//! exception is the target-native import check, which Faber re-emission still
//! owes (a program bound to Python is not a Faber program for any other
//! build).

use crate::codegen::Target;
use crate::diagnostics::{Diagnostic, DiagnosticConvert};
use crate::driver::{AnalyzedModule, Config, praefixum};
use crate::semantic::passes::{float_literal_fold, import_target};

/// Whether `target` keeps the authored form of constant expressions and
/// `praefixum` bodies (Faber re-emission) or is the neutral artifact itself
/// (FHIR). Neither folds floats nor evaluates anything.
#[must_use]
pub fn keeps_authored_form(target: Target) -> bool {
    matches!(target, Target::HirFaber | Target::HirFhir)
}

/// Evaluate the `praefixum` initializers of a unit analyzed for the neutral
/// target, leaving every other qualification step to the consumer.
///
/// Package analysis needs this: an importing module reads the exported
/// constants of its providers while it is analyzed, so a provider's `comptime`
/// constant must have its value before the importer's analysis, even though
/// the importer's own build target is not known yet. Single-unit analysis for
/// the neutral target never calls it.
///
/// # Errors
///
/// The unit's diagnostics extended with the sites that failed to evaluate.
pub fn evaluate_praefixum_for_importers(
    unit: &mut AnalyzedModule,
    config: &Config,
    name: &str,
    body: &str,
) -> Result<(), Vec<Diagnostic>> {
    praefixum::evaluate(unit, name, body, config.defer_imported_praefixum).map_err(|errors| {
        let mut diagnostics = std::mem::take(&mut unit.diagnostics);
        diagnostics.extend(errors);
        diagnostics
    })
}

/// Qualify `unit` for the build target in `config`.
///
/// `name` and `body` give the diagnostics their file and source context; a
/// loaded unit has no source text, so it passes an empty `body` and its
/// diagnostics carry spans only.
///
/// With `config.defer_imported_praefixum` a unit whose `praefixum` sites reach
/// an import is left unevaluated for the package layer, which links the
/// imports (`evaluate_deferred`); without it each such site is refused by name
/// (`praefixum_import_unavailable`).
///
/// # Errors
///
/// Returns the diagnostics of a refused import binding or a `praefixum` site
/// that failed to evaluate; the unit must not reach codegen then.
pub fn qualify_for_target(
    unit: &mut AnalyzedModule,
    config: &Config,
    name: &str,
    body: &str,
) -> Result<(), Vec<Diagnostic>> {
    let target = config.target;
    if target == Target::HirFhir {
        return Ok(());
    }
    let refusals = import_target::check(&unit.hir, &unit.interner, target);
    if !refusals.is_empty() {
        return Err(refusals
            .iter()
            .map(|error| Diagnostic::from_semantic_error(name, body, error))
            .collect());
    }
    if keeps_authored_form(target) {
        return Ok(());
    }
    if !config.keep_authored_constants {
        float_literal_fold::fold(&mut unit.hir, &unit.types, &unit.interner);
    }
    praefixum::evaluate(unit, name, body, config.defer_imported_praefixum)
}

#[cfg(test)]
#[path = "qualify_test.rs"]
mod tests;
