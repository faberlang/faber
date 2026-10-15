//! Shared package-boundary MIR canonicalization.
//!
//! LLVM package emission and the package-MIR driver both need the same
//! post-lowering rewrite.  Keeping the helper outside the package-MIR module
//! lets target-only lanes use the package LLVM builder without admitting the
//! package runner/FMIR dependency set.

use radix_mir::{MirCallee, MirStatementKind};
use radix_mir_lower::lower::LoweredMirUnit;
use radix_module::diagnostics::{Diagnostic, DiagnosticPhase};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Canonicalize direct `Definition` callees into `Function` callees.
///
/// The AIR lowering emits `Definition` edges for calls.  An untyped edge that
/// resolves to exactly one function in the lowered program is a direct call,
/// so rewrite it before the boundary-ABI emitter validates the program.
/// Generic or ambiguous edges remain definitions and are rejected by that
/// boundary when they cross it.
pub(crate) fn canonicalize_direct_definition_callees(
    mut lowered: LoweredMirUnit<'_>,
    diagnostic_path: impl AsRef<Path>,
) -> Result<LoweredMirUnit<'_>, Vec<Diagnostic>> {
    let diagnostic_path = diagnostic_path.as_ref();
    let mut direct_sources = HashMap::new();
    let mut ambiguous = HashSet::new();
    for function in &lowered.validated.program().functions {
        let Some(source) = function.source else {
            continue;
        };
        if ambiguous.contains(&source) {
            continue;
        }
        if direct_sources.insert(source, function.id).is_some() {
            direct_sources.remove(&source);
            ambiguous.insert(source);
        }
    }

    let (mut program, mut context) = lowered.validated.into_parts();
    for function in &mut program.functions {
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                let MirStatementKind::Call { callee, .. } = &mut statement.kind else {
                    continue;
                };
                let MirCallee::Definition { source, type_args } = callee else {
                    continue;
                };
                if !type_args.is_empty() {
                    continue;
                }
                if let Some(id) = direct_sources.get(source) {
                    *callee = MirCallee::Function(*id);
                }
            }
        }
    }

    context.closure_environments.clear();
    for environment in &lowered.closure_environments {
        context
            .closure_environments
            .insert(environment.id, environment.clone());
    }
    lowered.validated = radix_mir::ValidatedMir::new(program, context).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| {
                let diagnostic = crate::package_diagnostic_error(error.message)
                    .with_file(diagnostic_path.display().to_string())
                    .with_phase(DiagnosticPhase::Mir)
                    .with_span(error.span);
                if error.issue.is_empty() {
                    diagnostic
                } else {
                    diagnostic.with_arg("issue", error.issue)
                }
            })
            .collect::<Vec<_>>()
    })?;
    Ok(lowered)
}
