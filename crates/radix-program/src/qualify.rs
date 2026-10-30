//! Target qualification of a package in the neutral FHIR form.
//!
//! A FHIR package is analyzed and typechecked, but written without a build
//! target, so it carries the authored float expressions and `comptime` bodies
//! unevaluated. The same holds for a package analyzed for the neutral target
//! (a manifest's implicit portable default). The consumer qualifies it for its
//! build target once, after load or analysis ([`qualify_program`]), through the
//! same per-unit step direct compilation ends analysis with
//! ([`radix_module::driver::qualify_for_target`]), then evaluates the
//! `comptime` sites that reach an import over the linked package, exactly as
//! package analysis does for a source package (Gate J). A package already
//! analyzed for a concrete target is unchanged by it.

use crate::analyze::AnalyzedProgram;
use radix_module::diagnostics::Diagnostic;
use radix_module::driver::{Config, qualify_for_target};

/// Enforce annotated contract environments before a package executes or emits
/// an executable artifact. Source analysis, FHIR preservation and inspection
/// keep their existing target-neutral qualification path.
///
/// # Errors
/// Returns a named mismatch for every unit requiring another environment.
pub(crate) fn require_contract_execution_targets(
    program: &AnalyzedProgram,
    target: radix_module::codegen::Target,
) -> Result<(), Vec<Diagnostic>> {
    let diagnostics: Vec<_> = program
        .nodes
        .values()
        .filter_map(|node| {
            radix_module::driver::require_contract_execution_target(&node.analysis, target)
                .err()
                .map(|diagnostic| (*diagnostic).with_file(node.path.display().to_string()))
        })
        .collect();
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

/// Qualify every unit of a package in the neutral form for the build
/// target in `config`.
///
/// Per unit, in package order: refuse a target-native import bound to another
/// target, fold literal-only float expressions, evaluate the `comptime` sites
/// that stay inside the unit. Then, with the `runner` leaf, evaluate the
/// sites that reach an import, importees first, over the linked import
/// closure. A loaded unit has no source text, so diagnostics carry spans only.
///
/// # Errors
///
/// The diagnostics of every refused binding or failed site; the package must
/// not reach codegen then.
pub fn qualify_program(
    program: &mut AnalyzedProgram,
    config: &Config,
) -> Result<(), Vec<Diagnostic>> {
    #[cfg(feature = "runner")]
    let config = &config.clone().with_defer_imported_praefixum();
    let mut diagnostics = Vec::new();
    for node in program.nodes.values_mut() {
        let name = node.path.display().to_string();
        if let Err(errors) = qualify_for_target(&mut node.analysis, config, &name, "") {
            diagnostics.extend(errors);
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    #[cfg(feature = "runner")]
    evaluate_deferred_sites(program, config)?;
    Ok(())
}

/// Importees-first order over the local import edges.
#[cfg(feature = "runner")]
fn dependency_order(program: &AnalyzedProgram) -> Vec<crate::analyze::ModuleId> {
    fn visit(
        program: &AnalyzedProgram,
        id: &crate::analyze::ModuleId,
        seen: &mut std::collections::BTreeSet<crate::analyze::ModuleId>,
        order: &mut Vec<crate::analyze::ModuleId>,
    ) {
        if !seen.insert(id.clone()) {
            return;
        }
        for successor in program.imports.local_successors(id) {
            visit(program, successor, seen, order);
        }
        order.push(id.clone());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut order = Vec::new();
    for id in program.nodes.keys() {
        visit(program, id, &mut seen, &mut order);
    }
    order
}

#[cfg(feature = "runner")]
fn evaluate_deferred_sites(
    program: &mut AnalyzedProgram,
    config: &Config,
) -> Result<(), Vec<Diagnostic>> {
    if radix_module::driver::keeps_authored_form(config.target) {
        return Ok(());
    }
    let mut library_cache = crate::library::LibraryInterfaceCache::with_config(config);
    let mut diagnostics = Vec::new();
    for id in dependency_order(program) {
        let Some(node) = program.nodes.get(&id) else {
            continue;
        };
        if !crate::analyze::holds_deferred_praefixum(&node.analysis) {
            continue;
        }
        let path = node.path.clone();
        let mut analysis = node.analysis.clone();
        let evaluated = crate::mir::praefixum_link::evaluate_deferred_module(
            &crate::mir::praefixum_link::DeferredLink {
                spec: &program.spec,
                imports: &program.imports,
                nodes: &program.nodes,
                config,
                library_resolver: &program.library_resolver,
            },
            &id,
            &mut analysis,
            &path,
            "",
            &mut library_cache,
        );
        match evaluated {
            Ok(()) => {
                if let Some(node) = program.nodes.get_mut(&id) {
                    node.analysis = analysis;
                }
            }
            Err(errors) => diagnostics.extend(errors),
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}
