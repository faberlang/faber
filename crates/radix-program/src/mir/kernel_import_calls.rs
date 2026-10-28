//! A kernel calling an imported non-kernel function.
//!
//! Single-module analysis checks a kernel's calls against its own module
//! (`nucleum_host_call`). A call through an import names a function of another
//! unit, whose device role the importing module does not carry, so it passes
//! that check. A host function behind the import cannot run on a device: the
//! `reference` route would run it as a host call, and a device backend can
//! only refuse. The package link knows the callee's role, so the same named
//! diagnostic is raised here, before any backend sees the call.

use super::air_specialize::NamespaceCallCollector;
use super::{
    AnalyzedProgram, DefId, Diagnostic, HirItemKind, LibraryInterfaceCache, LibraryResolver,
    PackageMirLinks, library_cached_analysis,
};
use radix_module::hir::visit::HirVisitor;
use radix_module::hir::{HirFunction, HirModule};

/// Raise `SEM008 nucleum_host_call` for every call, inside a kernel of the
/// package, to an imported function that is not a kernel or a fragment.
pub(super) fn check_kernel_import_calls(
    package: &AnalyzedProgram,
    links: &PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    for unit in package.nodes.values() {
        for item in &unit.analysis.hir.items {
            let HirItemKind::Function(function) = &item.kind else {
                continue;
            };
            if !function.is_kernel {
                continue;
            }
            let Some(body) = &function.body else {
                continue;
            };
            let mut collector = NamespaceCallCollector {
                unit_path: &unit.path,
                interner: &unit.analysis.interner,
                links,
                calls: Vec::new(),
            };
            collector.visit_block(body);
            for call in collector.calls {
                let Some((target_path, target_def)) =
                    links.sources.iter().find_map(|((path, def), synthetic)| {
                        (*synthetic == call.old_target).then(|| (path.clone(), *def))
                    })
                else {
                    continue;
                };
                let callee =
                    if let Some(node) = package.node_by_path(&target_path).map(|(_, node)| node) {
                        host_function(&node.analysis.hir, target_def)
                            .map(|name| node.analysis.interner.resolve(name).to_owned())
                    } else if let Some(import) = links
                        .libraries
                        .iter()
                        .find(|library| library.path == target_path)
                        .map(|library| library.import.clone())
                        && let Ok(analysis) =
                            library_cached_analysis(&import, library_resolver, library_cache)
                    {
                        host_function(&analysis.hir, target_def)
                            .map(|name| analysis.interner.resolve(name).to_owned())
                    } else {
                        None
                    };
                if let Some(callee) = callee {
                    errors.push(host_call_diagnostic(&unit.path, call.call_span, &callee));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The name of function `def` when it is a plain host function: not a kernel
/// and not a nucleum fragment. Anything else (a type, a constant, a kernel,
/// a fragment) is not a host call.
fn host_function(hir: &HirModule, def: DefId) -> Option<radix_module::lexer::Symbol> {
    hir.items.iter().find_map(|item| {
        if item.def_id != def {
            return None;
        }
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        is_host(function).then_some(function.name)
    })
}

fn is_host(function: &HirFunction) -> bool {
    !function.is_kernel && !function.nucleum_fragment
}

fn host_call_diagnostic(
    path: &std::path::Path,
    span: radix_module::lexer::Span,
    callee: &str,
) -> Diagnostic {
    super::diag::mir_error_diag(
        path,
        radix_mir_lower::lower::MirError {
            message: format!("a kernel calls `{callee}`, which is not a kernel"),
            issue: "nucleum_host_call".to_owned(),
            span,
            definition_span: None,
        },
    )
    .with_code("SEM008")
    .with_arg("callee", callee)
    .with_arg("reason", "not_a_kernel")
}
