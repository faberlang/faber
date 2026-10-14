//! ST2-2.7: evaluate one deferred `comptime` site over the linked import
//! closure.
//!
//! Package analysis leaves a module whose `comptime` bodies reach an import
//! unevaluated (ST2-2.4). [`run_deferred_site`] is the evaluator callback the
//! package layer hands to
//! [`radix_module::driver::evaluate_deferred`] (ST2-2.5): it clones the nodes
//! of the module's local import closure, links them with the test-link
//! machinery (ST2-2.3) around the scratch module and runs the site on the MIR
//! runner, returning the value the caller installs as the initializer
//! literal.
//!
//! PERF: the per-site closure clone and link are confined to modules that
//! defer a site (a module with no import-reaching comptime pays nothing); the
//! fact that would change this is a package with many deferred sites measured
//! slow, and the follow-up is one link per module instead of one per site.

use super::{
    AnalyzedProgramNode, BTreeMap, Config, DefId, Diagnostic, LibraryInterfaceCache,
    LibraryResolver, ModuleId, Path, TestPackageScope, driver::link_package_functions,
};
use crate::analyze::{AnalyzedProgram, ModuleGraph};
use crate::discovery::PackageSpec;
use radix_mir_runner::Value;
use radix_module::driver::{AnalyzedModule, run_linked_function};
use radix_module::file_interface::FileInterface;
use radix_module::lexer::Span;
use radix_module::semantic::{SemanticError, SemanticErrorKind};

/// Evaluate one deferred site on a clone of `module`'s local import closure.
///
/// `scratch` is the module clone `evaluate_deferred` built for this site: it
/// holds the synthetic evaluation function under `synthetic` and no entry.
/// The closure's nodes are cloned from the package's `nodes`; `module`'s own
/// metadata (segments, exports) comes from its node when present, and its
/// node is rebuilt around the scratch with the default file interface.
/// Linking failures map to `praefixum_unsupported` with the first diagnostic
/// as `detail`, exactly as the local lowering path reports them.
///
/// # Errors
///
/// A `SemanticError` carrying `praefixum_unsupported` when the closure cannot
/// link or run; runner failures surface through [`run_linked_function`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_deferred_site(
    spec: &PackageSpec,
    imports: &ModuleGraph,
    nodes: &BTreeMap<ModuleId, AnalyzedProgramNode>,
    module: &ModuleId,
    mut scratch: AnalyzedModule,
    anchor_path: &Path,
    synthetic: DefId,
    span: Span,
    config: &Config,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Value, SemanticError> {
    scratch.hir.entry = None;
    scratch.cli_program = None;

    let template = nodes.get(module);
    let closure = imports.local_closure(std::iter::once(module));
    let mut closure_nodes = nodes
        .iter()
        .filter(|(id, _)| closure.contains(*id))
        .map(|(id, node)| (id.clone(), node.clone()))
        .collect::<BTreeMap<_, _>>();
    closure_nodes.insert(
        module.clone(),
        AnalyzedProgramNode {
            path: anchor_path.to_path_buf(),
            module_segments: template.map_or_else(Vec::new, |node| node.module_segments.clone()),
            is_entry: true,
            analysis: scratch,
            file_interface: FileInterface::default(),
            export_names: template.map_or_else(Vec::new, |node| node.export_names.clone()),
            namespace_exports: template
                .map_or_else(BTreeMap::new, |node| node.namespace_exports.clone()),
            expanded_library_imports: template
                .map_or_else(Vec::new, |node| node.expanded_library_imports.clone()),
        },
    );

    let program = AnalyzedProgram {
        spec: spec.clone(),
        entry: Some(module.clone()),
        roots: vec![module.clone()],
        nodes: closure_nodes,
        imports: imports.clone(),
        entry_frontmatter: None,
        diagnostics: Vec::new(),
        library_resolver: library_resolver.clone(),
        routes: crate::route_table::RouteTable::default(),
    };
    let function_key = (anchor_path.to_path_buf(), synthetic);
    let scope = TestPackageScope {
        remap_anchor: anchor_path.to_path_buf(),
        functions: vec![function_key.clone()],
    };
    link_package_functions(
        program,
        &scope,
        config,
        library_cache,
        |validated, function_ids| run_linked_function(validated, function_ids[&function_key], span),
    )
    .map_err(|diagnostics| link_failure(&diagnostics, span))?
}

/// The local path's mapping for a failure before the body runs: one
/// `praefixum_unsupported` issue with the underlying detail.
fn link_failure(diagnostics: &[Diagnostic], span: Span) -> SemanticError {
    let detail = diagnostics.first().map_or_else(
        || "package link failed".to_owned(),
        |diagnostic| diagnostic.message.clone(),
    );
    SemanticError::structured(SemanticErrorKind::PraefixumInvalid, span)
        .with_arg("issue", "praefixum_unsupported")
        .with_arg("detail", detail)
}
