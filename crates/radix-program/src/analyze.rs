//! Multi-unit package analysis composing Radix single-unit analysis (RP3).
//!
//! This is the production analysis spine for a package source graph: load
//! package files, resolve imports/libraries, extract cross-unit file
//! interfaces, and analyze every unit through the radix driver
//! (`analyze_source_with_kernel_template_admissions` with an
//! [`AnalyzeRequest`] and the unit's package kernel-template admissions →
//! [`AnalyzedModule`]).
//! The production [`AnalyzedProgram`] contract is radix-program-owned; Faber's
//! package consumers use it directly.
//!
//! Product orchestration stays faber-side and enters here through explicit
//! [`AnalysisSeams`] (the `RP2c` `LocaleLoader` posture): reader-locale pack
//! loading, strict product-view manifest validation, `--library-home` /
//! `[paths] library_home` and workspace probing, and the G4 linked-crate map
//! from faber's artifact plan. Nothing here reads the ambient environment.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Wall-clock budget for package test analysis. The package test route accepts
/// untrusted source, so a loader defect must become a diagnostic instead of an
/// unbounded multi-unit analysis.
pub const PACKAGE_TEST_ANALYSIS_BUDGET: Duration = Duration::from_secs(30);

use radix_module::diagnostics::Diagnostic;
use radix_module::driver::{
    AnalyzeRequest, AnalyzedModule, CarriedProviderUnit, Config, ModuleImportContract,
    PackageKernelTemplateAdmissions, ProviderUnit, ProviderUnitView, Session,
    analyze_source_with_kernel_template_admissions,
};
use radix_module::lexer::Interner;
use radix_module::program::provider_unit_for_target;
use radix_module::semantic::passes::visibility_annotation::VisibilityTier;
use radix_module::syntax::{ImportDecl, ImportKind, StmtKind};

use crate::discovery::{PackageSpec, discover_build_layout, discover_package};
use crate::frontmatter::manifest_path_for_spec;
use crate::import_graph::{
    ImportResolution, build_mount_plan, detect_import_cycles, import_unsupported_diagnostic,
    library_import_binding, library_import_kind_diagnostic, resolve_import,
};
use crate::library::{
    LibraryInterfaceCache, LibraryResolver, LocaleLoader, analysis_source_for_file,
    attach_library_provenance_with_links, expand_library_imports, library_cached_file_interface,
    library_interface_declaration_surface, library_interface_export_surface,
    library_interface_has_module, library_interface_source, library_module_region_name,
};
use crate::manifest::read_manifest;
use crate::modules::module_segments_for_file;
use crate::paths::normalize_path;
use crate::source_files::{is_proba_source_path, load_package_source, package_source_files};
use crate::test_source_filter::TestSourceFilter;
use crate::{LibraryImportBinding, PackageFile};
use radix_module::program::{program_declaration_surface, program_export_surface};

/// Reused [`artifact_plan::ModuleId`] — the one module identity in this crate.
pub use crate::artifact_plan::ModuleId;

/// Resolved local and library import edges for an [`AnalyzedProgram`].
///
/// Edges are recorded at analysis time from the same `resolve_import` walk
/// that schedules units. Target assembly and MIR linking consume this graph
/// instead of reconstructing it from path scans or unit-vector order.
#[derive(Clone, Default)]
pub struct ModuleGraph {
    /// Local source edges: importer → `(binding, imported module)`.
    local: BTreeMap<ModuleId, Vec<(String, ModuleId)>>,
    /// Library edges: importer → expanded library bindings.
    library: BTreeMap<ModuleId, Vec<LibraryImportBinding>>,
}

impl ModuleGraph {
    pub fn insert_local(&mut self, from: ModuleId, binding: String, to: ModuleId) {
        let edges = self.local.entry(from).or_default();
        if !edges
            .iter()
            .any(|(existing, target)| existing == &binding && target == &to)
        {
            edges.push((binding, to));
        }
    }

    pub fn insert_library(&mut self, from: ModuleId, binding: LibraryImportBinding) {
        let edges = self.library.entry(from).or_default();
        if !edges.iter().any(|existing| {
            existing.binding == binding.binding && existing.module == binding.module
        }) {
            edges.push(binding);
        }
    }

    /// Local import targets from `from`, in recorded order.
    pub fn local_edges(&self, from: &ModuleId) -> &[(String, ModuleId)] {
        self.local.get(from).map_or(&[], Vec::as_slice)
    }

    /// Local successors of `from`.
    pub fn local_successors(&self, from: &ModuleId) -> impl Iterator<Item = &ModuleId> {
        self.local_edges(from).iter().map(|(_, target)| target)
    }

    /// Module ids reachable from `roots` along local edges, roots first.
    pub fn local_closure<'a, I>(&self, roots: I) -> BTreeSet<ModuleId>
    where
        I: IntoIterator<Item = &'a ModuleId>,
    {
        let mut seen = BTreeSet::new();
        let mut pending = VecDeque::new();
        for root in roots {
            if seen.insert(root.clone()) {
                pending.push_back(root.clone());
            }
        }
        while let Some(id) = pending.pop_front() {
            for successor in self.local_successors(&id) {
                if seen.insert(successor.clone()) {
                    pending.push_back(successor.clone());
                }
            }
        }
        seen
    }
}

/// The analyzed whole-program graph: every local module analyzed once, keyed
/// by the reused [`ModuleId`], with explicit entry/roots and import edges.
///
/// `library_resolver` is the resolver analysis already built — not a second
/// stored constructor. Downstream linking threads this value instead of
/// reconstructing a resolver from config after `analyze_package*`.
pub struct AnalyzedProgram {
    pub spec: PackageSpec,
    pub entry: Option<ModuleId>,
    pub roots: Vec<ModuleId>,
    pub nodes: BTreeMap<ModuleId, AnalyzedProgramNode>,
    pub imports: ModuleGraph,
    // Entry frontmatter feeds the hir-rust test-selection merge (rust_target)
    // and the hir-fhir writer. `allow(dead_code)`: dead only in builds with
    // neither `hir-rust` nor `hir-fhir` enabled (minimal target leaves).
    #[allow(dead_code)]
    pub entry_frontmatter: Option<radix_module::driver::FileFrontmatter>,
    pub diagnostics: Vec<Diagnostic>,
    /// Analysis-prepared library resolver (lock + home). Threaded into MIR
    /// linking; not reconstructed from config after analyze.
    pub library_resolver: LibraryResolver,
    /// Program-wide `@ ad` handler table (tier 1 of the router).
    pub routes: crate::route_table::RouteTable,
}

impl AnalyzedProgram {
    /// Empty program for the given spec (tests and FHIR rehydration).
    #[must_use]
    pub fn from_spec(spec: PackageSpec) -> Self {
        Self {
            spec,
            entry: None,
            roots: Vec::new(),
            nodes: BTreeMap::new(),
            imports: ModuleGraph::default(),
            entry_frontmatter: None,
            diagnostics: Vec::new(),
            library_resolver: LibraryResolver::with_optional_home(None),
            routes: crate::route_table::RouteTable::default(),
        }
    }

    /// A copy of this program narrowed to `root` and its local import
    /// closure, with `root` as the only entry and test root. Only the closure's
    /// nodes are cloned; the source program is left untouched so one analysis
    /// can serve every test unit.
    #[must_use]
    pub fn scoped_to_closure(&self, root: &ModuleId) -> Self {
        let included = self.imports.local_closure(std::iter::once(root));
        Self {
            spec: self.spec.clone(),
            entry: Some(root.clone()),
            roots: vec![root.clone()],
            nodes: self
                .nodes
                .iter()
                .filter(|(id, _)| included.contains(*id))
                .map(|(id, node)| (id.clone(), node.clone()))
                .collect(),
            imports: self.imports.clone(),
            entry_frontmatter: self.entry_frontmatter.clone(),
            diagnostics: self.diagnostics.clone(),
            library_resolver: self.library_resolver.clone(),
            routes: self.routes.clone(),
        }
    }

    // Go package assembly (`generate_package_go_result`) consumes the entry
    // node directly. `allow(dead_code)`: dead only in builds without `hir-go`.
    #[allow(dead_code)]
    #[must_use]
    pub fn entry_node(&self) -> Option<&AnalyzedProgramNode> {
        self.entry.as_ref().and_then(|id| self.nodes.get(id))
    }

    #[must_use]
    pub fn node_by_path(&self, path: &Path) -> Option<(&ModuleId, &AnalyzedProgramNode)> {
        self.nodes.iter().find(|(_, node)| node.path == path)
    }

    pub(crate) fn node_by_path_mut(
        &mut self,
        path: &Path,
    ) -> Option<(&ModuleId, &mut AnalyzedProgramNode)> {
        self.nodes.iter_mut().find(|(_, node)| node.path == path)
    }

    /// Named remapping/type-identity host for a package test MIR link.
    ///
    /// Declared package entry wins. Otherwise the remapping host is a test
    /// root ([`Self::roots`], least [`ModuleId`]). A non-test library module
    /// must not be the host: `lower_package_units` treats the host as the
    /// merge entry (original `DefIds`), while sibling namespace calls are
    /// rewritten to synthetics — using the callee as host leaves those
    /// synthetics without a MIR function.
    ///
    /// Missing hosts fall back to [`PackageSpec::entry`] so the linker can
    /// fail closed.
    #[must_use]
    pub fn test_remap_anchor(&self) -> PathBuf {
        if let Some(node) = self.entry_node() {
            return node.path.clone();
        }
        if let Some((_, node)) = self.node_by_path(&self.spec.entry) {
            return node.path.clone();
        }
        if let Some(id) = self.roots.iter().min()
            && let Some(node) = self.nodes.get(id)
        {
            return node.path.clone();
        }
        self.spec.entry.clone()
    }
}

/// One graph node inside an [`AnalyzedProgram`].
///
/// This is the program-node (the graph node, not a second analysis stamp).
/// Path/entry/export/library-import metadata stays here; [`AnalyzedModule`]
/// remains the module-coherent analysis.
#[derive(Clone)]
pub struct AnalyzedProgramNode {
    pub path: PathBuf,
    pub module_segments: Vec<String>,
    pub is_entry: bool,
    pub analysis: AnalyzedModule,
    pub file_interface: radix_module::file_interface::FileInterface,
    pub export_names: Vec<String>,
    pub namespace_exports: BTreeMap<String, BTreeMap<String, VisibilityTier>>,
    pub expanded_library_imports: Vec<LibraryImportBinding>,
}

/// Strict product-view manifest validation seam (FBR-P1-001).
///
/// The product-view parse that rejects unknown product fields is faber product
/// policy; the loader receives the validator instead of duplicating it.
pub type ManifestValidator = fn(&PackageSpec) -> Result<(), Box<Diagnostic>>;

/// Package library-resolver construction seam. `--library-home` /
/// `[paths] library_home` resolution and the workspace-ancestor probe stay
/// faber-side; analysis receives the resolved resolver.
pub type LibraryResolverForPackage = fn(&Config, &Path) -> Result<LibraryResolver, Vec<Diagnostic>>;

/// Faber-supplied package-orchestration seams for multi-unit analysis (RP3).
///
/// Every seam is an explicit function input — no ambient env, locale, or
/// toolchain reads happen inside `radix-program` (package-api.md §3/§5).
#[derive(Clone, Copy)]
pub struct AnalysisSeams {
    pub locale_loader: LocaleLoader,
    pub manifest_validator: ManifestValidator,
    pub resolver_for_package: LibraryResolverForPackage,
}

/// Analyze a package source graph.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`; the seam bundle is
/// the faber product-orchestration input.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn analyze_package(
    config: &Config,
    input: &Path,
    seams: &AnalysisSeams,
) -> Result<AnalyzedProgram, Vec<Diagnostic>> {
    analyze_package_retaining_library_cache(config, input, seams).map(|(package, _)| package)
}

/// [`analyze_package`], also returning the [`LibraryInterfaceCache`] the
/// analysis filled. The MIR preparation that follows analyzes the same
/// libraries under the same target, so it reuses this cache instead of
/// rebuilding every library interface.
pub(crate) fn analyze_package_retaining_library_cache(
    config: &Config,
    input: &Path,
    seams: &AnalysisSeams,
) -> Result<(AnalyzedProgram, LibraryInterfaceCache), Vec<Diagnostic>> {
    let config = effective_package_config(config, input, seams.locale_loader)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let package_root = package_root_for_input(input);
    let library_resolver = (seams.resolver_for_package)(&config, &package_root)?;
    analyze_package_spec_retaining_library_cache(
        &config,
        spec,
        &library_resolver,
        false,
        None,
        seams,
    )
}

/// Check every loaded package module and return diagnostics without codegen.
///
/// The checker mirrors package compilation discovery and CLI mount analysis so
/// `faber check` reports the same import, manifest, and mounted-command policy
/// errors that a package build would encounter.
///
/// RP6: moved from `crates/faber/src/package/compile.rs` (the command-entry
/// adapter); the faber command consumes this entry directly with the product
/// seams.
#[must_use]
pub fn check_package(config: &Config, input: &Path, seams: &AnalysisSeams) -> Vec<Diagnostic> {
    match analyze_package(config, input, seams) {
        Ok(package) => package.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

/// Analyze a package for `faber test`: includes `*.proba` and optional path filters.
///
/// Production [`analyze_package`] keeps `include_proba = false` so build/run graphs
/// stay free of test sources.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn analyze_package_for_tests(
    config: &Config,
    input: &Path,
    proba_filter: Option<&TestSourceFilter>,
    seams: &AnalysisSeams,
) -> Result<AnalyzedProgram, Vec<Diagnostic>> {
    let config = effective_package_config(config, input, seams.locale_loader)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let owned_filter = explicit_proba_path_filter(&spec, input, proba_filter);
    let proba_filter = owned_filter.as_ref().or(proba_filter);
    let package_root = package_root_for_input(input);
    let library_resolver = (seams.resolver_for_package)(&config, &package_root)?;
    analyze_package_spec(&config, spec, &library_resolver, true, proba_filter, seams)
}

/// A named `.proba` file under a package root selects that file only.
fn explicit_proba_path_filter(
    spec: &PackageSpec,
    input: &Path,
    existing: Option<&TestSourceFilter>,
) -> Option<TestSourceFilter> {
    if !is_proba_source_path(input) {
        return None;
    }
    let source_root = if spec.source_root.is_dir() {
        spec.source_root.clone()
    } else {
        spec.entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    Some(TestSourceFilter::restricting_explicit_proba(
        existing,
        &source_root,
        input,
    ))
}

/// Apply the package's reader-locale pack when the config has none yet.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`; the locale-pack
/// loader is the caller-supplied product seam.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn effective_package_config(
    config: &Config,
    input: &Path,
    locale_loader: LocaleLoader,
) -> Result<Config, Vec<Diagnostic>> {
    if config.locale_pack.is_some() {
        return Ok(config.clone());
    }
    match locale_loader(input, None) {
        Ok(Some(pack)) => Ok(config.clone().with_locale_pack(pack)),
        Ok(None) => Ok(config.clone()),
        Err(diag) => Err(vec![*diag]),
    }
}

/// Resolve the package root for a package input, falling back to the input
/// path shape when no build layout is discoverable.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`; consumed by faber's
/// emission half through the package-module adapter.
#[must_use]
pub fn package_root_for_input(input: &Path) -> PathBuf {
    match discover_build_layout(input) {
        Ok(layout) => layout.package_root,
        Err(_) => {
            if input.is_file() {
                input
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            } else {
                input.to_path_buf()
            }
        }
    }
}

/// Analyze a package spec that discovery already resolved: load package
/// files, then run the per-unit analysis loop.
///
/// RP3: moved from `crates/faber/src/package/compile.rs` (the multi-unit
/// analysis core — composing radix single-unit analysis, never a duplicated
/// compiler phase).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn analyze_package_spec(
    config: &Config,
    spec: PackageSpec,
    library_resolver: &LibraryResolver,
    include_proba: bool,
    proba_filter: Option<&TestSourceFilter>,
    seams: &AnalysisSeams,
) -> Result<AnalyzedProgram, Vec<Diagnostic>> {
    analyze_package_spec_retaining_library_cache(
        config,
        spec,
        library_resolver,
        include_proba,
        proba_filter,
        seams,
    )
    .map(|(package, _)| package)
}

fn analyze_package_spec_retaining_library_cache(
    config: &Config,
    spec: PackageSpec,
    library_resolver: &LibraryResolver,
    include_proba: bool,
    proba_filter: Option<&TestSourceFilter>,
    seams: &AnalysisSeams,
) -> Result<(AnalyzedProgram, LibraryInterfaceCache), Vec<Diagnostic>> {
    let deadline = include_proba.then(|| Instant::now() + PACKAGE_TEST_ANALYSIS_BUDGET);
    analyze_package_spec_with_deadline(
        config,
        spec,
        library_resolver,
        include_proba,
        proba_filter,
        deadline,
        seams,
    )
}

fn analyze_package_spec_with_deadline(
    config: &Config,
    spec: PackageSpec,
    library_resolver: &LibraryResolver,
    include_proba: bool,
    proba_filter: Option<&TestSourceFilter>,
    deadline: Option<Instant>,
    seams: &AnalysisSeams,
) -> Result<(AnalyzedProgram, LibraryInterfaceCache), Vec<Diagnostic>> {
    // One package analysis = one driver run (ARCH-U3): the session minted
    // here owns the parse-once memo identity shared by loading, mount
    // planning, and every per-file analysis below (through re-wraps).
    let session = Session::new(config.clone());
    let files = load_package_with_session(
        &spec,
        library_resolver,
        &session,
        include_proba,
        proba_filter,
        seams,
    )?;
    if let Some(diagnostic) = analysis_deadline_diagnostic(&spec, deadline, None, "load") {
        return Err(vec![diagnostic]);
    }
    let entry_frontmatter = files
        .iter()
        .find(|file| file.path == spec.entry)
        .and_then(|file| file.frontmatter.clone());
    // Install [paths.templates] into the driver config so SEM006 accepts §name/…
    // and validation matches package resolution.
    let mut config = config.clone();
    if !spec.templates.is_empty() {
        let mut templates = BTreeMap::new();
        for (name, path) in &spec.templates {
            templates.insert(name.clone(), path.display().to_string());
        }
        config.import_path_templates = Some(templates);
    }
    // Re-wrap, not a fresh run: the templates-bearing config keeps the memo
    // identity the load phase already filled.
    let session = session.re_wrap(config);
    let mount_plan = build_mount_plan(&spec, &files)?;
    let mut diagnostics = Vec::new();
    let mut library_cache =
        LibraryInterfaceCache::with_config(&session.config).with_locale_loader(seams.locale_loader);
    let mut nodes = BTreeMap::new();
    let mut analyzed_interfaces_by_path = BTreeMap::new();
    // A failed provider already has an authoritative diagnostic. Importers
    // that depend on it cannot be analyzed, but should not add the secondary
    // "interface was not analyzed before importer" PKG001 that would hide the
    // provider's actual failure.
    let mut failed_interface_paths = BTreeSet::new();
    // Carriage minted per import target within this package run (ARCH-U4):
    // every edge to the same target shares one analyzed provider unit.
    let mut provider_carriage_minted: BTreeMap<PathBuf, CarriedProviderUnit> = BTreeMap::new();
    // Native-binding crates are derived from the package root and lockfile at
    // the compiler-owned consumer; no product seam or analyzed-package shell
    // is needed to carry this artifact-plan fact.
    let linked_library_crates = crate::artifact_plan::linked_library_crate_map(&spec.package_root);
    let path_to_id = files
        .iter()
        .map(|file| {
            (
                file.path.clone(),
                ModuleId::for_spec(&spec, file.module_segments.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let files_by_path = files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect::<BTreeMap<_, _>>();
    let export_surfaces_by_path = files
        .iter()
        .map(|file| {
            (
                file.path.clone(),
                program_export_surface(&file.program, &file.interner),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let declaration_surfaces_by_path = files
        .iter()
        .map(|file| {
            (
                file.path.clone(),
                program_declaration_surface(&file.program, &file.interner),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let package_name = manifest_path_for_spec(&spec)
        .and_then(|path| read_manifest(&path).ok())
        .map(|manifest| manifest.package.name);
    let imports = build_module_graph(&spec, &files, &path_to_id, library_resolver);

    for file in package_analysis_order(&spec, &files, &files_by_path, library_resolver) {
        if let Some(diagnostic) =
            analysis_deadline_diagnostic(&spec, deadline, Some(&file.path), "unit")
        {
            diagnostics.push(diagnostic);
            break;
        }
        let file_cli = mount_plan.module_cli.get(&file.path).cloned();
        let namespace_exports = match namespace_exports_for_file(
            &spec,
            file,
            &export_surfaces_by_path,
            library_resolver,
            &mut library_cache,
        ) {
            Ok(exports) => exports,
            Err(diag) => {
                diagnostics.push(diag);
                failed_interface_paths.insert(file.path.clone());
                continue;
            }
        };
        let file_interfaces = match file_interfaces_for_file(
            &spec,
            file,
            &files_by_path,
            library_resolver,
            &mut library_cache,
            &analyzed_interfaces_by_path,
            &failed_interface_paths,
        ) {
            Ok(Some(interfaces)) => interfaces,
            Ok(None) => {
                failed_interface_paths.insert(file.path.clone());
                continue;
            }
            Err(diag) => {
                diagnostics.push(diag);
                failed_interface_paths.insert(file.path.clone());
                continue;
            }
        };
        let analysis_source =
            match analysis_source_for_file(file, library_resolver, &mut library_cache) {
                Ok(source) => source,
                Err(diag) => {
                    diagnostics.push(diag);
                    failed_interface_paths.insert(file.path.clone());
                    continue;
                }
            };
        let namespace_seams = namespace_seams_for_file(&spec, file, library_resolver);
        let namespace_declarations = match namespace_declarations_for_file(
            &spec,
            file,
            &declaration_surfaces_by_path,
            library_resolver,
            &mut library_cache,
        ) {
            Ok(declarations) => declarations,
            Err(diag) => {
                diagnostics.push(diag);
                failed_interface_paths.insert(file.path.clone());
                continue;
            }
        };
        // Per-file re-wrap (ARCH-U3): the file's own reader pack config keeps
        // the package run's memo identity, so targets shared across files
        // parse once for the whole package analysis.
        let file_config = match file.locale_pack.as_ref() {
            Some(pack) if file.locale_declared => session
                .config
                .clone()
                .with_resolved_locale_pack(pack.clone()),
            Some(pack) => session.config.clone().with_locale_pack(pack.clone()),
            None => session.config.clone(),
        };
        // Gate J (ST2-2.7): the package hook below evaluates the sites that
        // reach an import; without the runner leaf no driver evaluates them
        // and every backend refuses the unevaluated initializer by name. This
        // is the per-file session only: the provider-carriage session above
        // never defers.
        #[cfg(feature = "runner")]
        let file_config = file_config.with_defer_imported_praefixum();
        let file_session = session.re_wrap(file_config);
        // PKG-ADM: the file's own package export surface is the admission
        // fact for exported size-generic kernel templates — registered here,
        // before the per-unit orphan check, so a package API with no
        // same-unit caller is not mistaken for an orphan. A missing surface
        // stays fail-closed (empty admissions).
        let kernel_template_admissions = export_surfaces_by_path
            .get(&file.path)
            .map(PackageKernelTemplateAdmissions::from_export_surface)
            .unwrap_or_default();
        let analyzed = analyze_source_with_kernel_template_admissions(
            &file_session,
            &file.path.display().to_string(),
            &analysis_source,
            AnalyzeRequest {
                cli_program: file_cli,
                contract: ModuleImportContract {
                    namespace_exports: namespace_exports.clone(),
                    file_interfaces,
                    namespace_seams,
                    namespace_declarations,
                    default_import_names: file.default_import_names.clone(),
                },
            },
            &kernel_template_admissions,
        )
        .and_then(|mut unit| {
            // Importers read this module's exported constants while they are
            // analyzed, so under the neutral target (a manifest's implicit
            // portable default) its `comptime` constants still evaluate here;
            // folding and the import binding stay with the consumer.
            if file_session.config.target == radix_module::codegen::Target::HirFhir {
                radix_module::driver::evaluate_praefixum_for_importers(
                    &mut unit,
                    &file_session.config,
                    &file.path.display().to_string(),
                    &analysis_source,
                )?;
            }
            Ok(unit)
        });
        let mut analysis = match analyzed {
            Ok(mut analysis) => {
                radix_module::driver::adjust_diagnostic_lines_for_body(
                    &mut analysis.diagnostics,
                    &file.raw_source,
                    file.body_byte_offset,
                );
                // Carry the analyzed provider units for this file's import
                // edges (ARCH-U4 parity on the package-analysis path):
                // backend emission maps over the view instead of re-entering
                // the front half from disk, on this surface exactly as on the
                // program-layer walk.
                analysis.provider_units = provider_carriage_for_file(
                    &spec,
                    file,
                    &files_by_path,
                    library_resolver,
                    &mut library_cache,
                    &session,
                    &mut provider_carriage_minted,
                );
                analysis
            }
            Err(mut file_diagnostics) => {
                radix_module::driver::adjust_diagnostic_lines_for_body(
                    &mut file_diagnostics,
                    &file.raw_source,
                    file.body_byte_offset,
                );
                diagnostics.extend(file_diagnostics);
                failed_interface_paths.insert(file.path.clone());
                continue;
            }
        };
        let provenance_imports = file.library_imports.clone();
        if let Err(diag) = attach_library_provenance_with_links(
            &mut analysis,
            &provenance_imports,
            library_resolver,
            &mut library_cache,
            Some(&linked_library_crates),
        ) {
            diagnostics.push(diag);
            failed_interface_paths.insert(file.path.clone());
            continue;
        }

        let is_entry = file.path == spec.entry;
        if !is_entry {
            analysis.hir.entry = None;
        }
        if is_entry && let Some(root_cli) = mount_plan.root_cli.clone() {
            analysis.cli_program = Some(root_cli);
        }

        let export_names = export_surfaces_by_path
            .get(&file.path)
            .map(|surface| surface.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let export_identity = crate::file_interface::ExportIdentityContext {
            provider: "package".to_owned(),
            package: package_name.clone(),
            module_path: file.module_segments.clone(),
        };
        // `allow(unused_mut)`: only the runner-gated deferred evaluation below
        // re-extracts the interface, so builds without `runner` never reassign.
        #[cfg_attr(not(feature = "runner"), allow(unused_mut))]
        let mut file_interface = match crate::file_interface::extract_file_interface_with_identity(
            &analysis,
            &export_names,
            &file.path.display().to_string(),
            Some(&export_identity),
        ) {
            Ok(interface) => interface,
            Err(diag) => {
                diagnostics.push(diag);
                failed_interface_paths.insert(file.path.clone());
                continue;
            }
        };
        analyzed_interfaces_by_path.insert(file.path.clone(), file_interface.clone());

        diagnostics.extend(std::mem::take(&mut analysis.diagnostics));
        // Hoisted above the deferred-site hook: the MIR lowering the hook
        // runs reads the package import identities through the unit.
        analysis.package_import_identities = Some(package_import_identities_for_file(
            &spec,
            file,
            &files_by_path,
            library_resolver,
            &analyzed_interfaces_by_path,
        ));
        let module_id = ModuleId::for_spec(&spec, file.module_segments.clone());

        // Gate J (ST2-2.7): when analysis deferred an import-reaching
        // `comptime` site, the package layer evaluates it now, over the
        // linked local import closure. Faber re-emission keeps the authored
        // bodies and is never evaluated here.
        //
        // PERF: the per-site closure clone and link are confined to modules
        // that defer a site (a module with no import-reaching comptime pays
        // nothing); the fact that would change this is a package with many
        // deferred sites measured slow, and the follow-up is one link per
        // module instead of one per site.
        #[cfg(feature = "runner")]
        if file_session.config.target != radix_module::codegen::Target::HirFaber
            && holds_deferred_praefixum(&analysis)
        {
            // The import-effect resolver answers from the analyzed nodes, so
            // the unit under evaluation must appear there. This node is the
            // template the final insertion below replaces with the evaluated
            // analysis.
            nodes.insert(
                module_id.clone(),
                AnalyzedProgramNode {
                    path: file.path.clone(),
                    module_segments: file.module_segments.clone(),
                    is_entry,
                    analysis: analysis.clone(),
                    file_interface: file_interface.clone(),
                    export_names: export_names.clone(),
                    namespace_exports: namespace_exports.clone(),
                    expanded_library_imports: file.expanded_library_imports.clone(),
                },
            );
            let deferred = crate::mir::praefixum_link::evaluate_deferred_module(
                &crate::mir::praefixum_link::DeferredLink {
                    spec: &spec,
                    imports: &imports,
                    nodes: &nodes,
                    config: &file_session.config,
                    library_resolver,
                },
                &module_id,
                &mut analysis,
                &file.path,
                &analysis_source,
                &mut library_cache,
            );
            if let Err(mut deferred_diagnostics) = deferred {
                nodes.remove(&module_id);
                analyzed_interfaces_by_path.remove(&file.path);
                radix_module::driver::adjust_diagnostic_lines_for_body(
                    &mut deferred_diagnostics,
                    &file.raw_source,
                    file.body_byte_offset,
                );
                diagnostics.extend(deferred_diagnostics);
                failed_interface_paths.insert(file.path.clone());
                continue;
            }
            // ST2-2.8: later importers of this module must carry the analysis
            // the hook just evaluated. A fresh provider re-analysis refuses
            // the import-reaching site this run already folded, so the
            // evaluated unit (with its own nested view) is the carriage.
            provider_carriage_minted.insert(
                file.path.clone(),
                carried_provider_unit_from_evaluated(analysis.clone()),
            );
            // The interface extracted above predates the fold: an exported
            // `comptime` constant still reads as non-constant there. Importers
            // read `analyzed_interfaces_by_path`, so republish the interface
            // from the evaluated analysis.
            file_interface = match crate::file_interface::extract_file_interface_with_identity(
                &analysis,
                &export_names,
                &file.path.display().to_string(),
                Some(&export_identity),
            ) {
                Ok(interface) => interface,
                Err(diag) => {
                    diagnostics.push(diag);
                    analyzed_interfaces_by_path.remove(&file.path);
                    failed_interface_paths.insert(file.path.clone());
                    continue;
                }
            };
            analyzed_interfaces_by_path.insert(file.path.clone(), file_interface.clone());
        }

        nodes.insert(
            module_id,
            AnalyzedProgramNode {
                path: file.path.clone(),
                module_segments: file.module_segments.clone(),
                is_entry,
                analysis,
                file_interface,
                export_names,
                namespace_exports,
                expanded_library_imports: file.expanded_library_imports.clone(),
            },
        );
    }

    if let Some(diagnostic) = analysis_deadline_diagnostic(&spec, deadline, None, "package") {
        diagnostics.push(diagnostic);
    }
    let routes = match program_route_table(
        &nodes,
        package_name.as_ref(),
        library_resolver,
        &mut library_cache,
    ) {
        Ok(routes) => routes,
        Err(route_diagnostics) => {
            diagnostics.extend(route_diagnostics);
            crate::route_table::RouteTable::default()
        }
    };
    if diagnostics.iter().any(radix_module::Diagnostic::is_error) {
        return Err(diagnostics);
    }

    let entry = files
        .iter()
        .find(|file| file.path == spec.entry)
        .map(|file| ModuleId::for_spec(&spec, file.module_segments.clone()))
        .filter(|id| nodes.get(id).is_some_and(|node| node.is_entry));
    let roots = program_roots(&spec, &nodes, include_proba);
    let mut program = AnalyzedProgram {
        spec,
        entry,
        roots,
        nodes,
        imports,
        entry_frontmatter,
        diagnostics,
        library_resolver: library_resolver.clone(),
        routes,
    };

    #[cfg(feature = "hir-rust")]
    {
        let closure_diags = crate::rust_target::check_program_def_id_closure(
            &mut program,
            library_resolver,
            seams.locale_loader,
            true,
        );
        program.diagnostics.extend(closure_diags);
        if program.diagnostics.iter().any(Diagnostic::is_error) {
            return Err(std::mem::take(&mut program.diagnostics));
        }
    }

    #[cfg(feature = "hir-fhir")]
    {
        match crate::nucleum::metadata_for_program(&program) {
            Ok(_) => {}
            Err(diagnostic) => {
                program.diagnostics.push(diagnostic);
                return Err(std::mem::take(&mut program.diagnostics));
            }
        }
    }

    Ok((program, library_cache))
}

/// Whether the unit still holds a `Praefixum` initializer: analysis deferred
/// at least one site because its body reaches an import (ST2-2.4). A module
/// either defers all its sites or none, so one remaining site means every
/// site of the unit is still for [`evaluate_deferred`](radix_module::driver::evaluate_deferred).
#[cfg(feature = "runner")]
pub(crate) fn holds_deferred_praefixum(analysis: &AnalyzedModule) -> bool {
    radix_module::semantic::passes::praefixum::evaluation_order(&analysis.hir, &analysis.types)
        .is_ok_and(|sites| !sites.is_empty())
}

/// Reduce a module the package hook evaluated to the carried provider unit
/// later import edges share (ST2-2.8): the same facts
/// [`provider_unit_for_target`] carries, taken from this run's analysis
/// instead of a re-analysis that cannot see the deferred evaluation.
#[cfg(feature = "runner")]
fn carried_provider_unit_from_evaluated(analysis: AnalyzedModule) -> CarriedProviderUnit {
    let AnalyzedModule {
        hir,
        types,
        interner,
        resolver,
        tensor_edge_policies,
        shape_obligations,
        diagnostics,
        provider_units,
        ..
    } = analysis;
    CarriedProviderUnit::Unit(std::sync::Arc::new(std::sync::Mutex::new(ProviderUnit {
        hir,
        types,
        interner,
        resolver,
        tensor_edge_policies,
        shape_obligations,
        diagnostics,
        nested: provider_units,
    })))
}

/// Merge the `@ ad` routes of every analyzed package module and every
/// library module in their expanded import closure (F11 default; Q6 module
/// granularity). Library modules that fail to analyze were already
/// diagnosed by the import walk and contribute nothing here.
fn program_route_table(
    nodes: &BTreeMap<ModuleId, AnalyzedProgramNode>,
    package_name: Option<&String>,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<crate::route_table::RouteTable, Vec<Diagnostic>> {
    use crate::route_table::{RouteOrigin, RouteTableBuilder};
    let mut builder = RouteTableBuilder::default();
    for node in nodes.values() {
        let origin = RouteOrigin {
            package: package_name.cloned(),
            path: node.path.clone(),
        };
        builder.merge_module(&origin, &node.file_interface.routes);
    }
    let mut interface_errors = Vec::new();
    for import in nodes
        .values()
        .flat_map(|node| node.expanded_library_imports.iter())
    {
        let Ok(analysis) =
            crate::library::library_cached_analysis(import, library_resolver, library_cache)
        else {
            continue;
        };
        let routes = match radix_module::semantic::passes::ad_routes::route_exports(
            &analysis.hir,
            &analysis.types,
            &analysis.interner,
            &analysis.resolver,
            None,
        ) {
            Ok(routes) => routes,
            Err(err) => {
                interface_errors.push(
                    crate::package_diagnostic_error(format!(
                        "library `@ ad` route handler cannot be snapshotted: {err:?}"
                    ))
                    .with_file(import.module.interface_path.display().to_string())
                    .with_arg("issue", "ad_route_snapshot_failed"),
                );
                continue;
            }
        };
        let origin = RouteOrigin {
            package: Some(import.module.package.clone()),
            path: normalize_path(&import.module.interface_path),
        };
        builder.merge_module(&origin, &routes);
    }
    match builder.finish() {
        Ok(table) if interface_errors.is_empty() => Ok(table),
        Ok(_) => Err(interface_errors),
        Err(mut duplicates) => {
            duplicates.extend(interface_errors);
            Err(duplicates)
        }
    }
}

fn analysis_deadline_diagnostic(
    spec: &PackageSpec,
    deadline: Option<Instant>,
    path: Option<&Path>,
    phase: &str,
) -> Option<Diagnostic> {
    let deadline = deadline?;
    if Instant::now() < deadline {
        return None;
    }
    let path = path.unwrap_or(&spec.package_root);
    Some(
        crate::package_diagnostic_error(format!(
            "package test analysis exceeded the {}ms budget during {phase}",
            PACKAGE_TEST_ANALYSIS_BUDGET.as_millis()
        ))
        .with_file(path.display().to_string())
        .with_arg("issue", "package_analysis_budget_exceeded")
        .with_arg(
            "budget_ms",
            PACKAGE_TEST_ANALYSIS_BUDGET.as_millis().to_string(),
        )
        .with_arg("phase", phase),
    )
}

fn program_roots(
    spec: &PackageSpec,
    nodes: &BTreeMap<ModuleId, AnalyzedProgramNode>,
    include_proba: bool,
) -> Vec<ModuleId> {
    if include_proba {
        return nodes
            .iter()
            .filter(|(_, node)| node_is_test_root(node))
            .map(|(id, _)| id.clone())
            .collect();
    }
    if let Some((id, _)) = nodes.iter().find(|(_, node)| node.is_entry) {
        return vec![id.clone()];
    }
    if spec.entry.is_file()
        && let Some((id, _)) = nodes.iter().find(|(_, node)| node.path == spec.entry)
    {
        return vec![id.clone()];
    }
    nodes.keys().cloned().collect()
}

fn node_is_test_root(node: &AnalyzedProgramNode) -> bool {
    if is_proba_source_path(&node.path) {
        return true;
    }
    node.analysis.hir.items.iter().any(|item| {
        matches!(
            &item.kind,
            radix_module::hir::HirItemKind::Function(function) if function.test.is_some()
        )
    })
}

fn build_module_graph(
    spec: &PackageSpec,
    files: &[PackageFile],
    path_to_id: &BTreeMap<PathBuf, ModuleId>,
    library_resolver: &LibraryResolver,
) -> ModuleGraph {
    let mut graph = ModuleGraph::default();
    for file in files {
        let Some(from) = path_to_id.get(&file.path).cloned() else {
            continue;
        };
        for stmt in &file.program.statements {
            let StmtKind::Import(decl) = &stmt.kind else {
                continue;
            };
            if decl.target.is_some() {
                continue;
            }
            let Some(binding) = import_binding(&file.interner, decl) else {
                continue;
            };
            let import_path = file.interner.resolve(decl.path);
            match resolve_import(spec, library_resolver, &file.path, import_path) {
                ImportResolution::Local(target) => {
                    if let Some(to) = path_to_id.get(&target).cloned() {
                        graph.insert_local(from.clone(), binding, to);
                    }
                }
                ImportResolution::Library(module) => {
                    if let Some(library) = library_import_binding(&file.interner, decl, module) {
                        graph.insert_library(from.clone(), library);
                    }
                }
                ImportResolution::Unsupported | ImportResolution::Error(_) => {}
            }
        }
        for import in &file.expanded_library_imports {
            graph.insert_library(from.clone(), import.clone());
        }
    }
    graph
}

/// The package import identities of one file. A local import whose target
/// exports a device function also records the target's interface in
/// `device_interfaces`, as the compile path does (DFV2-3,
/// `radix_module::program::compile`): lowering reads it to learn that a
/// sibling module exports a `⇥` kernel, so handled blocks over that kernel
/// deliver faults at the block instead of syncing on every call. Targets are
/// analyzed before their importers, so `analyzed_interfaces` holds them.
fn package_import_identities_for_file(
    spec: &PackageSpec,
    file: &PackageFile,
    files_by_path: &BTreeMap<PathBuf, &PackageFile>,
    library_resolver: &LibraryResolver,
    analyzed_interfaces: &BTreeMap<PathBuf, radix_module::file_interface::FileInterface>,
) -> radix_module::driver::PackageImportIdentities {
    let product = crate::artifact_plan::package_id_from_spec(spec).name;
    let mut imports = BTreeMap::new();
    let mut device_interfaces = BTreeMap::new();
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let Some(binding) = import_binding(&file.interner, decl) else {
            continue;
        };
        let import_path = file.interner.resolve(decl.path);
        let ImportResolution::Local(target) =
            resolve_import(spec, library_resolver, &file.path, import_path)
        else {
            continue;
        };
        let Some(target_file) = files_by_path.get(&target).copied() else {
            continue;
        };
        if let Some(interface) = analyzed_interfaces.get(&target)
            && interface_bears_device(interface)
        {
            device_interfaces.insert(binding.clone(), interface.clone());
        }
        imports.insert(binding, target_file.module_segments.clone());
    }
    radix_module::driver::PackageImportIdentities {
        product,
        module_segments: file.module_segments.clone(),
        imports,
        device_interfaces,
    }
}

/// Whether `interface` exports a device function (the same test the compile
/// path applies before recording a device interface).
fn interface_bears_device(interface: &radix_module::file_interface::FileInterface) -> bool {
    use radix_module::file_interface::FileExportKind;
    interface
        .exports
        .values()
        .any(|export| matches!(&export.kind, FileExportKind::Function(callable) if callable.device.is_some()))
}

/// Load a package's source graph as [`PackageFile`]s.
///
/// RP3: moved from `crates/faber/src/package/mod.rs` (the loader
/// composition). Manifests are validated through the caller-supplied product
/// seam; source loading, import resolution, library expansion, and cycle
/// detection are compiler-owned here.
///
/// Session-less entry (product harnesses, tests): one call mints its own
/// driver-run session, so its memo scope ends with the load. The package
/// analysis run threads its own shared session through
/// [`load_package_with_session`] instead (ARCH-U3).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn load_package_with_config(
    spec: &PackageSpec,
    library_resolver: &LibraryResolver,
    config: &Config,
    include_proba: bool,
    proba_filter: Option<&TestSourceFilter>,
    seams: &AnalysisSeams,
) -> Result<Vec<PackageFile>, Vec<Diagnostic>> {
    let session = Session::new(config.clone());
    load_package_with_session(
        spec,
        library_resolver,
        &session,
        include_proba,
        proba_filter,
        seams,
    )
}

/// Session-carrying loader half: `session` owns the parse-once memo identity
/// for the load (package-root walks during library-locale merging share the
/// caller's driver run).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
fn load_package_with_session(
    spec: &PackageSpec,
    library_resolver: &LibraryResolver,
    session: &Session,
    include_proba: bool,
    proba_filter: Option<&TestSourceFilter>,
    seams: &AnalysisSeams,
) -> Result<Vec<PackageFile>, Vec<Diagnostic>> {
    // Manifest presence is validated at load (FBR-P1-001: a validated manifest
    // that later disappears or becomes unreadable is a diagnostic, never a
    // silent default); the strict product-view parse also rejects unknown
    // product fields here (faber product seam).
    if let Err(diag) = (seams.manifest_validator)(spec) {
        return Err(vec![*diag]);
    }
    // Compiler-view manifest for source loading (RP2b: frontmatter conflict
    // validation and lib-bodyless parsing moved with `load_package_source`).
    let compiler_manifest = match manifest_path_for_spec(spec) {
        Some(manifest_path) => Some(read_manifest(&manifest_path).map_err(|diag| vec![*diag])?),
        None => None,
    };
    let source_root_for_filter = if spec.source_root.is_dir() {
        spec.source_root.clone()
    } else {
        spec.entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    let proba_allowed = |path: &Path| -> bool {
        if !is_proba_source_path(path) {
            return true;
        }
        match proba_filter {
            Some(filter) if !filter.is_empty() => filter.allows_path(&source_root_for_filter, path),
            _ => true,
        }
    };
    let initial_files = if spec.entry.is_dir() {
        let all = package_source_files(&spec.entry, include_proba)?;
        // When `faber test --include/--exclude` selects proba files, seed only
        // those tests. Product modules enter via local import edges, so a
        // focused math.proba run does not compile the whole library graph.
        let proba_only_seed =
            include_proba && proba_filter.is_some_and(|filter| !filter.is_empty());
        if proba_only_seed {
            all.into_iter()
                .filter(|path| is_proba_source_path(path) && proba_allowed(path))
                .collect::<Vec<_>>()
        } else {
            all.into_iter()
                .filter(|path| proba_allowed(path))
                .collect::<Vec<_>>()
        }
    } else {
        // Single-file entry: allow an explicit `.proba` path on the test path only.
        if is_proba_source_path(&spec.entry) && !include_proba {
            return Err(vec![
                crate::package_diagnostic_error(
                    ".proba files are test sources; use `faber test` to load them",
                )
                .with_file(spec.entry.display().to_string())
                .with_arg("issue", "proba_source_build_forbidden"),
            ]);
        }
        // Product graph starts at the entry file. For `faber test`, also walk
        // `source_root` for `*.proba` so colocated test files are not orphaned
        // when nothing imports them (they must not be importable).
        let mut files = vec![spec.entry.clone()];
        if include_proba {
            for path in package_source_files(&source_root_for_filter, true)? {
                if is_proba_source_path(&path) && proba_allowed(&path) {
                    files.push(path);
                }
            }
        }
        files
    };
    let mut queue = VecDeque::from(initial_files);
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    let mut file_import_targets = Vec::new();
    let mut diagnostics = Vec::new();

    while let Some(path) = queue.pop_front() {
        let canonical = normalize_path(&path);
        if !seen.insert(canonical.clone()) {
            continue;
        }

        let Some(loaded) = load_package_source(
            &canonical,
            compiler_manifest.as_ref(),
            &session.config,
            seams.locale_loader,
            &mut diagnostics,
        ) else {
            continue;
        };

        let mut import_targets = Vec::new();
        for (index, stmt) in loaded.program.statements.iter().enumerate() {
            let StmtKind::Import(decl) = &stmt.kind else {
                continue;
            };
            if decl.target.is_some() {
                continue;
            }
            let import_path = loaded.interner.resolve(decl.path);
            match resolve_import(spec, library_resolver, &canonical, import_path) {
                ImportResolution::Local(target) => {
                    import_targets.push((index, ImportTarget::Local(normalize_path(&target))));
                    // Test sources may import product `.fab` modules; never the reverse.
                    if is_proba_source_path(&target) {
                        diagnostics.push(
                            crate::package_diagnostic_error(
                                ".proba files are test sources and cannot be imported; move shared helpers to a .fab module",
                            )
                            .with_file(canonical.display().to_string())
                            .with_span(decl.span)
                            .with_arg("issue", "proba_import_forbidden")
                            .with_arg("import", import_path.to_owned()),
                        );
                    } else {
                        queue.push_back(target);
                    }
                }
                ImportResolution::Library(module) => {
                    import_targets.push((index, ImportTarget::Library(module)));
                }
                ImportResolution::Unsupported => {
                    diagnostics.push(import_unsupported_diagnostic(&canonical, decl, import_path));
                }
                ImportResolution::Error(diag) => {
                    diagnostics.push(import_error_at_decl(diag, &canonical, &loaded, decl));
                }
            }
        }

        files.push(PackageFile {
            module_segments: module_segments_for_file(
                &spec.source_root,
                &canonical,
                loaded.frontmatter.as_ref(),
            ),
            path: canonical,
            raw_source: loaded.raw_source,
            source: loaded.body,
            body_byte_offset: loaded.body_byte_offset,
            frontmatter: loaded.frontmatter,
            locale_pack: loaded.locale_pack,
            locale_declared: loaded.locale_declared,
            program: loaded.program,
            interner: loaded.interner,
            library_imports: Vec::new(),
            expanded_library_imports: Vec::new(),
            default_import_names: BTreeMap::new(),
        });
        file_import_targets.push(import_targets);
    }

    if diagnostics.iter().any(radix_module::Diagnostic::is_error) {
        Err(diagnostics)
    } else {
        let mut library_cache =
            LibraryInterfaceCache::default().with_locale_loader(seams.locale_loader);
        bind_package_imports(
            &mut files,
            file_import_targets,
            &mut library_cache,
            &mut diagnostics,
        );
        for file in &mut files {
            file.expanded_library_imports = expand_library_imports(
                &file.library_imports,
                library_resolver,
                &mut library_cache,
                &mut diagnostics,
            );
            if let Err(diagnostic) = merge_library_locale_pack_for_file(session, file) {
                diagnostics.push(diagnostic);
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        diagnostics.extend(detect_import_cycles(spec, &files));
        if diagnostics.iter().any(radix_module::Diagnostic::is_error) {
            return Err(diagnostics);
        }
        Ok(files)
    }
}

/// Resolved target of one import statement, recorded while the package source
/// graph loads so default import names can be bound once every target parsed.
enum ImportTarget {
    Local(PathBuf),
    Library(crate::library::ResolvedLibraryModule),
}

/// Pin an import-resolution failure to its import declaration with a physical
/// line. A bare span renders its byte offset as the location (`main.fab:474`
/// for a 100-line file). Spans are body-relative, so the frontmatter's lines
/// are added back.
fn import_error_at_decl(
    diagnostic: Diagnostic,
    file: &Path,
    loaded: &crate::source_files::LoadedPackageSource,
    decl: &ImportDecl,
) -> Diagnostic {
    let mut diagnostic =
        diagnostic.with_source_context(&file.display().to_string(), &loaded.body, decl.span);
    radix_module::driver::adjust_diagnostic_lines_for_body(
        std::slice::from_mut(&mut diagnostic),
        &loaded.raw_source,
        loaded.body_byte_offset,
    );
    diagnostic
}

/// Bind every package file's imports once the whole source graph has loaded.
///
/// D7.7: a default-named import binds its target's declared `regio` name, so
/// bindings wait until every local target is parsed (and library targets are
/// read through the interface cache). The consumer's AST is rewritten in place
/// — every later binding-keyed surface (namespace exports, file interfaces,
/// module graph edges) reads the declared name — and the path → name map rides
/// the file so the driver rewrites its own parse identically.
fn bind_package_imports(
    files: &mut [PackageFile],
    file_import_targets: Vec<Vec<(usize, ImportTarget)>>,
    library_cache: &mut LibraryInterfaceCache,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let local_regions = files
        .iter()
        .filter_map(|file| {
            radix_module::program::program_region_name(&file.program, &file.interner)
                .map(|region| (file.path.clone(), region))
        })
        .collect::<BTreeMap<_, _>>();
    for (file, targets) in files.iter_mut().zip(file_import_targets) {
        let mut names = BTreeMap::new();
        for (index, target) in &targets {
            let StmtKind::Import(decl) = &file.program.statements[*index].kind else {
                continue;
            };
            if decl.target.is_some() {
                continue;
            }
            if !radix_module::program::import_takes_default_name(decl) {
                continue;
            }
            let region = match target {
                ImportTarget::Local(path) => local_regions.get(path).cloned(),
                ImportTarget::Library(module) => {
                    match library_module_region_name(module, library_cache) {
                        Ok(region) => region,
                        Err(diagnostic) => {
                            diagnostics.push(diagnostic);
                            None
                        }
                    }
                }
            };
            if let Some(region) = region {
                names.insert(file.interner.resolve(decl.path).to_owned(), region);
            }
        }
        radix_module::program::apply_default_import_names(
            &mut file.program,
            &mut file.interner,
            &names,
        );
        for (index, target) in targets {
            let (StmtKind::Import(decl), ImportTarget::Library(module)) =
                (&file.program.statements[index].kind, target)
            else {
                continue;
            };
            if let Some(binding) = library_import_binding(&file.interner, decl, module) {
                file.library_imports.push(binding);
            } else {
                diagnostics.push(library_import_kind_diagnostic(
                    &file.path,
                    decl,
                    file.interner.resolve(decl.path),
                ));
            }
        }
        file.default_import_names = names;
    }
}

/// Merge library-owned member rows into the package file's consumer pack.
///
/// The source loader must lex and parse before it can resolve provider imports,
/// so this projection happens after library expansion but before any unit is
/// handed to semantic analysis. The same pack then drives the semantic import
/// projection and keeps the parser's source identities unchanged.
fn merge_library_locale_pack_for_file(
    session: &Session,
    file: &mut PackageFile,
) -> Result<(), Diagnostic> {
    let Some(pack) = file.locale_pack.as_mut() else {
        return Ok(());
    };

    let roots = file
        .library_imports
        .iter()
        .chain(file.expanded_library_imports.iter())
        .filter_map(|import| {
            radix_module::program::find_package_root_with_session(
                session,
                &import.module.interface_path,
            )
        })
        .collect::<BTreeSet<_>>();
    radix_module::locale::merge_library_locale_packs(pack, roots).map_err(|err| {
        crate::package_diagnostic_error(format!(
            "failed to load library locale pack: {}",
            radix_module::locale::format_locale_pack_load_error(&err)
        ))
        .with_phase(radix_module::diagnostics::DiagnosticPhase::Lex)
        .with_code("LOCALE001")
        .with_arg("issue", "library_locale_pack_load_failed")
        .with_file(file.path.display().to_string())
    })
}

/// Order package files for analysis: dependencies before importers.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`.
fn package_analysis_order<'a>(
    spec: &PackageSpec,
    files: &'a [PackageFile],
    files_by_path: &BTreeMap<PathBuf, &'a PackageFile>,
    library_resolver: &LibraryResolver,
) -> Vec<&'a PackageFile> {
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for file in files {
        visit_package_analysis_file(
            spec,
            file,
            files_by_path,
            library_resolver,
            &mut seen,
            &mut ordered,
        );
    }
    ordered
}

/// Depth-first visit of a package file and its local-import dependencies.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`.
fn visit_package_analysis_file<'a>(
    spec: &PackageSpec,
    file: &'a PackageFile,
    files_by_path: &BTreeMap<PathBuf, &'a PackageFile>,
    library_resolver: &LibraryResolver,
    seen: &mut BTreeSet<PathBuf>,
    ordered: &mut Vec<&'a PackageFile>,
) {
    if !seen.insert(file.path.clone()) {
        return;
    }
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let import_path = file.interner.resolve(decl.path);
        let ImportResolution::Local(target) =
            resolve_import(spec, library_resolver, &file.path, import_path)
        else {
            continue;
        };
        if let Some(target_file) = files_by_path.get(&target).copied() {
            visit_package_analysis_file(
                spec,
                target_file,
                files_by_path,
                library_resolver,
                seen,
                ordered,
            );
        }
    }
    ordered.push(file);
}

/// Mint the provider carriage for one file's direct import edges (ARCH-U4
/// parity on the package-analysis path).
///
/// Every edge that resolves to a real target gets its analyzed provider unit
/// — or its fail-closed `Failed` marker — under the same codegen contract the
/// program-layer walk uses: [`provider_unit_for_target`], not a forked
/// config. Local siblings mint from their on-disk source, library modules
/// from the cached interface body the package path already resolves. Edges
/// that stay unresolved (`Unsupported`/`Error`, unreadable library source)
/// install no carriage, matching the surfaceless-by-design posture: their
/// bindings get no file interface either, so provider emission never
/// consults them.
fn provider_carriage_for_file(
    spec: &PackageSpec,
    file: &PackageFile,
    files_by_path: &BTreeMap<PathBuf, &PackageFile>,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    session: &Session,
    minted: &mut BTreeMap<PathBuf, CarriedProviderUnit>,
) -> ProviderUnitView {
    let mut view = ProviderUnitView::default();
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let import_path = file.interner.resolve(decl.path);
        let (target_path, target_source) =
            match resolve_import(spec, library_resolver, &file.path, import_path) {
                ImportResolution::Local(target) => {
                    let Some(target_file) = files_by_path.get(&target) else {
                        continue;
                    };
                    (target, target_file.raw_source.clone())
                }
                ImportResolution::Library(module) => {
                    let Some(import) = library_import_binding(&file.interner, decl, module) else {
                        continue;
                    };
                    match library_interface_source(&import.module, library_cache) {
                        Ok(source) => (import.module.interface_path.clone(), source),
                        Err(_) => continue,
                    }
                }
                ImportResolution::Unsupported | ImportResolution::Error(_) => continue,
            };
        let carried = minted
            .entry(target_path)
            .or_insert_with_key(|path| provider_unit_for_target(session, path, &target_source));
        view.insert(import_path, carried.clone());
    }
    view
}

/// Borrowed graph state shared by direct and transitive interface installs.
struct LocalInterfaceInstallState<'a> {
    analyzed_interfaces_by_path: &'a BTreeMap<PathBuf, radix_module::file_interface::FileInterface>,
    failed_interface_paths: &'a BTreeSet<PathBuf>,
    visited: &'a mut BTreeSet<PathBuf>,
    interfaces: &'a mut BTreeMap<String, radix_module::file_interface::FileInterface>,
}

/// Resolve every import binding's cross-unit interface for a file.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`.
fn file_interfaces_for_file(
    spec: &PackageSpec,
    file: &PackageFile,
    files_by_path: &BTreeMap<PathBuf, &PackageFile>,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    analyzed_interfaces_by_path: &BTreeMap<PathBuf, radix_module::file_interface::FileInterface>,
    failed_interface_paths: &BTreeSet<PathBuf>,
) -> Result<Option<BTreeMap<String, radix_module::file_interface::FileInterface>>, Diagnostic> {
    let mut interfaces = BTreeMap::new();
    let mut transitive_visited: BTreeSet<PathBuf> = BTreeSet::new();
    let mut state = LocalInterfaceInstallState {
        analyzed_interfaces_by_path,
        failed_interface_paths,
        visited: &mut transitive_visited,
        interfaces: &mut interfaces,
    };
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let Some(binding) = import_binding(&file.interner, decl) else {
            continue;
        };
        let import_path = file.interner.resolve(decl.path);
        match resolve_import(spec, library_resolver, &file.path, import_path) {
            ImportResolution::Local(target) => {
                if state.failed_interface_paths.contains(&target) {
                    return Ok(None);
                }
                let Some(interface) = state.analyzed_interfaces_by_path.get(&target).cloned()
                else {
                    return Err(crate::package_diagnostic_error(format!(
                        "local import `{import_path}` interface was not analyzed before importer"
                    ))
                    .with_file(file.path.display().to_string())
                    .with_span(decl.span));
                };
                state.interfaces.insert(binding, interface);
                if !install_transitive_local_interfaces(
                    spec,
                    &target,
                    files_by_path,
                    library_resolver,
                    library_cache,
                    &mut state,
                )? {
                    return Ok(None);
                }
            }
            ImportResolution::Library(module) => {
                let Some(import) = library_import_binding(&file.interner, decl, module) else {
                    continue;
                };
                if library_interface_has_module(&import, library_cache)? {
                    continue;
                }
                state.interfaces.insert(
                    binding,
                    library_cached_file_interface(&import, library_resolver, library_cache)?,
                );
            }
            ImportResolution::Unsupported | ImportResolution::Error(_) => {}
        }
    }
    for import in &file.expanded_library_imports {
        if library_interface_has_module(import, library_cache)? {
            continue;
        }
        state.interfaces.insert(
            import.binding.clone(),
            library_cached_file_interface(import, library_resolver, library_cache)?,
        );
    }
    Ok(Some(std::mem::take(state.interfaces)))
}

/// Install file interfaces for a local import target's own local imports,
/// transitively (MRL-4).
///
/// A package module's public surface can name sibling nominals through its
/// own imports (`list<attribute.BufferAttribute>` in a `data.BufferGeometry`
/// field); a consumer that imports only the outer module cannot remap those
/// fields without the sibling modules' nominal registrations. Mirrors the
/// transitive library-leaf install: sibling namespaces carry the producer's
/// binding but have no consumer HIR import binding, so they stay inert
/// unless a snapshot resolves a nominal through them. A consumer's own
/// direct binding for the same name always wins.
///
/// The target's library imports (and their transitive leaves) install the
/// same way: a local module's public signature can name a library nominal the
/// consumer never imports — `⇥ fault.IoFault` on a loader whose caller
/// imports only the loader module. Without the library interface the
/// canonical-identity lookup misses (`NominalIdentityUnknown`) and the whole
/// export is skipped (WARN014), so the call types as unresolved.
fn install_transitive_local_interfaces(
    spec: &PackageSpec,
    target: &Path,
    files_by_path: &BTreeMap<PathBuf, &PackageFile>,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    state: &mut LocalInterfaceInstallState<'_>,
) -> Result<bool, Diagnostic> {
    let Some(target_file) = files_by_path.get(target).copied() else {
        return Ok(true);
    };
    for stmt in &target_file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let Some(binding) = import_binding(&target_file.interner, decl) else {
            continue;
        };
        let import_path = target_file.interner.resolve(decl.path);
        let inner = match resolve_import(spec, library_resolver, &target_file.path, import_path) {
            ImportResolution::Local(inner) => inner,
            ImportResolution::Library(module) => {
                if let Some(import) = library_import_binding(&target_file.interner, decl, module)
                    && !state.interfaces.contains_key(&binding)
                {
                    state.interfaces.insert(
                        binding,
                        library_cached_file_interface(&import, library_resolver, library_cache)?,
                    );
                }
                continue;
            }
            ImportResolution::Unsupported | ImportResolution::Error(_) => continue,
        };
        if state.failed_interface_paths.contains(&inner) {
            return Ok(false);
        }
        if !state.visited.insert(inner.clone()) {
            continue;
        }
        let Some(interface) = state.analyzed_interfaces_by_path.get(&inner).cloned() else {
            return Err(crate::package_diagnostic_error(format!(
                "local import `{import_path}` interface was not analyzed before importer"
            ))
            .with_file(target_file.path.display().to_string())
            .with_span(decl.span));
        };
        state.interfaces.entry(binding).or_insert(interface);
        if !install_transitive_local_interfaces(
            spec,
            &inner,
            files_by_path,
            library_resolver,
            library_cache,
            state,
        )? {
            return Ok(false);
        }
    }
    for import in &target_file.expanded_library_imports {
        if !state.interfaces.contains_key(&import.binding) {
            state.interfaces.insert(
                import.binding.clone(),
                library_cached_file_interface(import, library_resolver, library_cache)?,
            );
        }
    }
    Ok(true)
}

/// Resolve every import binding's exported namespace surface for a file.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`.
fn namespace_exports_for_file(
    spec: &PackageSpec,
    file: &PackageFile,
    export_surfaces_by_path: &BTreeMap<PathBuf, BTreeMap<String, VisibilityTier>>,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<BTreeMap<String, BTreeMap<String, VisibilityTier>>, Diagnostic> {
    let mut exports = BTreeMap::new();
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let Some(binding) = import_binding(&file.interner, decl) else {
            continue;
        };
        let import_path = file.interner.resolve(decl.path);
        match resolve_import(spec, library_resolver, &file.path, import_path) {
            ImportResolution::Local(target) => {
                let Some(surface) = export_surfaces_by_path.get(&target).cloned() else {
                    continue;
                };
                exports.insert(binding, surface);
            }
            ImportResolution::Library(module) => {
                let Some(import) = library_import_binding(&file.interner, decl, module) else {
                    continue;
                };
                exports.insert(
                    binding,
                    library_interface_export_surface(&import, library_resolver, library_cache)?,
                );
            }
            ImportResolution::Unsupported | ImportResolution::Error(_) => {}
        }
    }
    for import in &file.expanded_library_imports {
        exports.insert(
            import.binding.clone(),
            library_interface_export_surface(import, library_resolver, library_cache)?,
        );
    }
    Ok(exports)
}

/// Per-binding same-package flags from the already-resolved import graph.
///
/// Local package files are same-package. Library bindings are cross-package.
/// This does not walk `faber.toml` or resolve a library home.
fn namespace_seams_for_file(
    spec: &PackageSpec,
    file: &PackageFile,
    library_resolver: &LibraryResolver,
) -> BTreeMap<String, bool> {
    let mut seams = BTreeMap::new();
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let Some(binding) = import_binding(&file.interner, decl) else {
            continue;
        };
        let import_path = file.interner.resolve(decl.path);
        match resolve_import(spec, library_resolver, &file.path, import_path) {
            ImportResolution::Local(_) => {
                seams.insert(binding, true);
            }
            ImportResolution::Library(_) => {
                seams.insert(binding, false);
            }
            ImportResolution::Unsupported | ImportResolution::Error(_) => {}
        }
    }
    for import in &file.expanded_library_imports {
        seams.insert(import.binding.clone(), false);
    }
    seams
}

/// Per-binding declaration surfaces, including module-private names.
fn namespace_declarations_for_file(
    spec: &PackageSpec,
    file: &PackageFile,
    declaration_surfaces_by_path: &BTreeMap<PathBuf, BTreeMap<String, VisibilityTier>>,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<BTreeMap<String, BTreeMap<String, VisibilityTier>>, Diagnostic> {
    let mut declarations = BTreeMap::new();
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let Some(binding) = import_binding(&file.interner, decl) else {
            continue;
        };
        let import_path = file.interner.resolve(decl.path);
        match resolve_import(spec, library_resolver, &file.path, import_path) {
            ImportResolution::Local(target) => {
                let Some(surface) = declaration_surfaces_by_path.get(&target).cloned() else {
                    continue;
                };
                declarations.insert(binding, surface);
            }
            ImportResolution::Library(module) => {
                let Some(import) = library_import_binding(&file.interner, decl, module) else {
                    continue;
                };
                declarations.insert(
                    binding,
                    library_interface_declaration_surface(
                        &import,
                        library_resolver,
                        library_cache,
                    )?,
                );
            }
            ImportResolution::Unsupported | ImportResolution::Error(_) => {}
        }
    }
    for import in &file.expanded_library_imports {
        declarations.insert(
            import.binding.clone(),
            library_interface_declaration_surface(import, library_resolver, library_cache)?,
        );
    }
    Ok(declarations)
}

/// The namespace binding an import declaration introduces.
///
/// RP3: moved from `crates/faber/src/package/compile.rs`.
fn import_binding(interner: &Interner, decl: &ImportDecl) -> Option<String> {
    match &decl.kind {
        ImportKind::Named { name, alias, .. } => Some(
            interner
                .resolve(alias.as_ref().unwrap_or(name).name)
                .to_owned(),
        ),
        ImportKind::Wildcard { alias } => Some(interner.resolve(alias.name).to_owned()),
        // EOT-2M: a selective import introduces no namespace binding; key its
        // interface by the import path itself so the semantic resolve pass
        // (which has no namespace to consult) finds members there. Paths are
        // never valid identifiers, so this key cannot collide with a named or
        // wildcard namespace entry.
        ImportKind::Selective { .. } => Some(interner.resolve(decl.path).to_owned()),
    }
}

#[cfg(test)]
#[path = "analyze_test.rs"]
mod tests;

#[cfg(test)]
#[path = "default_import_name_test.rs"]
mod default_import_name_test;
