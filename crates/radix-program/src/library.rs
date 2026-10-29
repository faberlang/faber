//! Library import resolution, expansion, and interface caching for package
//! compilation.
//!
//! Public libraries are source-level interfaces installed under an explicit
//! library home — never an ambient read: the resolver is constructed from the
//! concrete roots the caller resolved (CTO memo), so the same
//! explicit inputs produce the same resolution. `resolve` translates an
//! import specifier such as `norma:solum` into the `.fab` interface that
//! package loading can parse, typecheck, and wire into generated output.
//!
//! INVARIANTS
//! ==========
//! - A resolver returns `Ok(None)` when an import is not provider-shaped; local
//!   package import resolution owns those paths.
//! - Once a provider is selected, malformed, missing-provider, or missing-module
//!   failures are diagnostics with concrete setup paths and known-module hints.
//! - Resolved modules always point at `.fab` interface files, keeping stdlib
//!   APIs on the normal parse/typecheck path.
//! - Old built-in slash forms such as `norma/json` are rejected instead of
//!   silently reinterpreted.
//! - The library home is explicit: this crate receives the home resolved by
//!   faber and falls back to the in-tree `<repo>/lib`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Locked package interface root used for build-time resolution.
///
/// Paths are absolute file-system locations from `faber.lock`. Faber does not
/// discover package-manager store roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedLibraryPackage {
    /// Declared package/provider name.
    pub name: String,
    /// Exact version pin from the lock.
    pub version: String,
    /// Directory containing `.fab` interface files for this package.
    pub interface_root: PathBuf,
}

/// Resolved import target for a source-level library module.
///
/// The path points at the Faber interface source, not at generated Rust or a
/// runtime artifact. Downstream package loading relies on that distinction so
/// imported APIs go through the same parser and typechecker as local sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLibraryModule {
    /// Provider package name, such as `norma`.
    pub package: String,

    /// Module path inside the provider, split on `/`.
    pub module_path: Vec<String>,

    /// Source interface file consumed by package loading.
    pub interface_path: PathBuf,
}

impl ResolvedLibraryModule {
    pub fn new(
        package: impl Into<String>,
        module_path: Vec<String>,
        interface_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            package: package.into(),
            module_path,
            interface_path: interface_path.into(),
        }
    }

    /// Return the terminal module segment expected to match named imports.
    pub fn module_name(&self) -> Option<&str> {
        self.module_path.last().map(String::as_str)
    }
}

/// Errors that mean a specifier selected a library provider but not a module.
///
/// Variants use a fact-noun grammar, naming the failed library fact or state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryResolveError {
    /// The old built-in slash form was used for a Norma module.
    OldBuiltinNormaSpecifier {
        /// Original import specifier from source.
        specifier: String,

        /// Provider-qualified replacement.
        replacement: String,
    },

    /// The specifier is provider-shaped but malformed.
    ProviderSpecifierInvalid {
        /// Original import specifier from source.
        specifier: String,

        /// Targeted reason for the invalid shape.
        reason: String,
    },

    /// The provider separator selected a provider that is not implemented.
    ProviderUnknown {
        /// Original import specifier from source.
        specifier: String,

        /// Provider segment from the source specifier.
        provider: String,

        /// Library home used for provider lookup.
        library_home: PathBuf,
    },

    /// A provider repo exists but does not expose a readable top-level manifest.
    InstalledManifestInvalid {
        /// Original import specifier from source.
        specifier: String,

        /// Provider package selected by the specifier.
        provider: String,

        /// Manifest path that failed validation.
        manifest_path: PathBuf,

        /// Targeted reason for the invalid package install.
        reason: String,

        /// Stable issue of the manifest-validation diagnostic that caused the
        /// failure, when one exists (boxed to keep the error small).
        cause_issue: Option<Box<str>>,
    },

    /// A provider manifest points at a source root that does not exist.
    InstalledSourceRootMissing {
        /// Original import specifier from source.
        specifier: String,

        /// Provider package selected by the specifier.
        provider: String,

        /// Source root computed from the installed top-level manifest.
        source_root: PathBuf,
    },

    /// A provider package was named, but no matching interface exists.
    ModuleUnknown {
        /// Original import specifier from source.
        specifier: String,

        /// Provider package selected by the specifier.
        package: String,

        /// Module file expected for the import.
        expected_path: PathBuf,

        /// Known modules for corrective diagnostics.
        known_modules: Vec<String>,
    },

    /// No public library home could be found.
    LibraryHomeMissing {
        /// Original import specifier from source.
        specifier: String,

        /// Setup hint for the operator.
        hint: String,
    },

    /// Provider is declared in faber.toml but missing from faber.lock.
    LockedPackageMissing {
        /// Original import specifier from source.
        specifier: String,

        /// Provider package name.
        provider: String,

        /// Declared version pin, when known.
        version: Option<String>,
    },

    /// Provider-qualified import is not declared in faber.toml dependencies
    /// and is not available from the bundled library home.
    ProviderUndeclared {
        /// Original import specifier from source.
        specifier: String,

        /// Provider package name.
        provider: String,
    },
}

/// Resolver for built-in and package-backed Faber library imports.
///
/// `resolve` returns `Ok(None)` when the specifier is not provider-shaped. That
/// lets package loading fall through to local import resolution without
/// treating every plain package path as a library error.
///
/// Locked packages (from `faber.lock`) are preferred over library-home layout.
/// Bundled Norma continues to resolve from library home until it is itself
/// locked. Faber never discovers package-manager store roots.
///
/// The resolver carries explicit inputs only — a concrete home (or `None`),
/// locked package roots, and declared dependencies. The faber side resolves
/// `--library-home` / `[paths] library_home` and passes the result here (CTO
/// memo).
#[derive(Debug, Clone)]
pub struct LibraryResolver {
    library_home: Option<PathBuf>,
    locked: BTreeMap<String, LockedLibraryPackage>,
    declared: BTreeMap<String, String>,
}

impl LibraryResolver {
    /// Build a resolver rooted at an explicit public library home.
    pub fn new(library_home: impl Into<PathBuf>) -> Self {
        Self {
            library_home: Some(library_home.into()),
            locked: BTreeMap::new(),
            declared: BTreeMap::new(),
        }
    }

    /// Build a resolver from an explicit optional home.
    ///
    /// `None` means no library home is available (deterministic for the same
    /// input); the faber side resolves `--library-home` / `[paths] library_home`
    /// and passes the result here; the in-tree `<repo>/lib` is searched after it.
    #[must_use]
    pub fn with_optional_home(library_home: Option<PathBuf>) -> Self {
        Self {
            library_home,
            locked: BTreeMap::new(),
            declared: BTreeMap::new(),
        }
    }

    /// Attach declared dependencies and locked package interface roots.
    #[must_use]
    pub fn with_package_lock(
        mut self,
        declared: BTreeMap<String, String>,
        locked: BTreeMap<String, LockedLibraryPackage>,
    ) -> Self {
        self.declared = declared;
        self.locked = locked;
        self
    }

    /// Resolve a Faber import specifier to a library interface, if applicable.
    ///
    /// The resolver claims provider-qualified specifiers and old built-in Norma
    /// slash specifiers. For `norma`, malformed paths and missing interface
    /// files are reported as library diagnostics because the user clearly
    /// selected the built-in provider and should see available module names.
    /// # Errors
    /// Returns an error when the input or persisted artifact is invalid.
    pub fn resolve(
        &self,
        specifier: &str,
    ) -> Result<Option<ResolvedLibraryModule>, LibraryResolveError> {
        if specifier.starts_with("./") || specifier.starts_with("../") {
            return Ok(None);
        }

        if let Some(module_path) = specifier.strip_prefix("norma/") {
            return Err(LibraryResolveError::OldBuiltinNormaSpecifier {
                specifier: specifier.to_owned(),
                replacement: format!("norma:{module_path}"),
            });
        }

        let Some((provider, module_path)) = specifier.split_once(':') else {
            return Ok(None);
        };

        validate_library_specifier(specifier, provider, module_path)?;

        let segments = module_path.split('/').collect::<Vec<_>>();
        if !segments
            .iter()
            .all(|segment| is_valid_module_segment(segment))
        {
            return Err(LibraryResolveError::ProviderSpecifierInvalid {
                specifier: specifier.to_owned(),
                reason: "module path must not contain empty, dot, or dot-dot segments".to_owned(),
            });
        }

        if let Some(locked) = self.locked.get(provider) {
            let interface_path = locked.interface_root.join(format!("{module_path}.fab"));
            if !interface_path.exists() {
                return Err(LibraryResolveError::ModuleUnknown {
                    specifier: specifier.to_owned(),
                    package: provider.to_owned(),
                    expected_path: interface_path,
                    known_modules: known_modules(&locked.interface_root),
                });
            }
            return Ok(Some(ResolvedLibraryModule::new(
                provider,
                segments
                    .iter()
                    .map(|segment| (*segment).to_owned())
                    .collect(),
                interface_path,
            )));
        }

        if self.declared.contains_key(provider) {
            return Err(LibraryResolveError::LockedPackageMissing {
                specifier: specifier.to_owned(),
                provider: provider.to_owned(),
                version: self.declared.get(provider).cloned(),
            });
        }

        // Search the configured home first, then the in-tree `<repo>/lib`. A
        // provider resolves from the first home that holds `<home>/<provider>`.
        let in_tree_home = in_tree_library_home();
        let found_repo = self
            .library_home
            .iter()
            .chain(in_tree_home.iter())
            .map(|home| home.join(provider))
            .find(|repo| repo.is_dir());
        let Some(provider_repo) = found_repo else {
            let library_home = self.library_home_missing(specifier)?;
            // Prefer an explicit dependency-declaration diagnostic when the
            // package has a [dependencies] table and this provider is not Norma
            // library-home layout.
            if !self.declared.is_empty() && provider != "norma" {
                return Err(LibraryResolveError::ProviderUndeclared {
                    specifier: specifier.to_owned(),
                    provider: provider.to_owned(),
                });
            }
            return Err(LibraryResolveError::ProviderUnknown {
                specifier: specifier.to_owned(),
                provider: provider.to_owned(),
                library_home,
            });
        };

        let provider_root = installed_provider_source_root(specifier, provider, &provider_repo)?;
        let interface_path = provider_root.join(format!("{module_path}.fab"));
        if !interface_path.exists() {
            return Err(LibraryResolveError::ModuleUnknown {
                specifier: specifier.to_owned(),
                package: provider.to_owned(),
                expected_path: interface_path,
                known_modules: known_modules(&provider_root),
            });
        }

        Ok(Some(ResolvedLibraryModule::new(
            provider,
            segments
                .iter()
                .map(|segment| (*segment).to_owned())
                .collect(),
            interface_path,
        )))
    }

    /// The configured home, for a provider no home holds. With no configured
    /// home there is nothing to report against: the caller must configure one.
    fn library_home_missing(&self, specifier: &str) -> Result<PathBuf, LibraryResolveError> {
        self.library_home.clone().ok_or_else(|| {
            LibraryResolveError::LibraryHomeMissing {
                specifier: specifier.to_owned(),
                hint: "pass --library-home <DIR> (or set [paths] library_home in faber.toml) to a directory containing provider repos, such as <home>/gradus/src (the faber binary embeds norma and needs no home for it)".to_string(),
            }
        })
    }
}

fn validate_library_specifier(
    specifier: &str,
    provider: &str,
    module_path: &str,
) -> Result<(), LibraryResolveError> {
    if provider.is_empty() {
        return Err(LibraryResolveError::ProviderSpecifierInvalid {
            specifier: specifier.to_owned(),
            reason: "provider segment must not be empty".to_owned(),
        });
    }

    if !is_valid_provider_segment(provider) {
        return Err(LibraryResolveError::ProviderSpecifierInvalid {
            specifier: specifier.to_owned(),
            reason:
                "provider segment must contain only ASCII letters, numbers, underscore, or hyphen"
                    .to_owned(),
        });
    }

    if module_path.is_empty() {
        return Err(LibraryResolveError::ProviderSpecifierInvalid {
            specifier: specifier.to_owned(),
            reason: "module path segment must not be empty".to_owned(),
        });
    }

    if module_path.contains(':') {
        return Err(LibraryResolveError::ProviderSpecifierInvalid {
            specifier: specifier.to_owned(),
            reason: "library specifier must contain exactly one provider separator".to_owned(),
        });
    }

    Ok(())
}

/// In-tree library home: `<repo>/lib`, located from this crate's manifest
/// directory (`crates/radix-program` → `../../lib`). It holds `norma/`.
fn in_tree_library_home() -> Option<PathBuf> {
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib");
    home.is_dir().then_some(home)
}

fn installed_provider_source_root(
    specifier: &str,
    provider: &str,
    provider_repo: &Path,
) -> Result<PathBuf, LibraryResolveError> {
    let manifest_path = provider_repo.join("faber.toml");
    if !manifest_path.is_file() {
        let fallback = provider_repo.join("src");
        if fallback.is_dir() {
            return Ok(fallback);
        }
        return Err(LibraryResolveError::InstalledManifestInvalid {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path,
            reason: "missing top-level faber.toml".to_owned(),
            cause_issue: None,
        });
    }

    crate::discovery::discover_package(provider_repo).map_err(|diag| {
        LibraryResolveError::InstalledManifestInvalid {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path: manifest_path.clone(),
            cause_issue: diag.issue().map(Box::from),
            reason: diag.message,
        }
    })?;
    let manifest = crate::manifest::read_manifest(&manifest_path).map_err(|diag| {
        LibraryResolveError::InstalledManifestInvalid {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path: manifest_path.clone(),
            cause_issue: diag.issue().map(Box::from),
            reason: diag.message,
        }
    })?;

    if manifest.build.kind != "lib" {
        return Err(LibraryResolveError::InstalledManifestInvalid {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path,
            reason: "faber.toml build.kind must be \"lib\"".to_owned(),
            cause_issue: None,
        });
    }

    let Some(library) = manifest.library.as_ref() else {
        return Err(LibraryResolveError::InstalledManifestInvalid {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path,
            reason: "faber.toml [library] is required".to_owned(),
            cause_issue: None,
        });
    };

    if library.provider != provider {
        return Err(LibraryResolveError::InstalledManifestInvalid {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path,
            reason: format!(
                "faber.toml library.provider is `{}`, expected `{provider}`",
                library.provider
            ),
            cause_issue: None,
        });
    }

    let source_root = provider_repo.join(&manifest.paths.source);
    if !source_root.is_dir() {
        return Err(LibraryResolveError::InstalledSourceRootMissing {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            source_root,
        });
    }
    Ok(source_root)
}

fn is_valid_module_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

#[must_use]
pub fn is_valid_provider_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn known_modules(provider_root: &Path) -> Vec<String> {
    let mut modules = Vec::new();
    collect_fab_modules(provider_root, provider_root, &mut modules);
    modules.sort();
    modules
}

fn collect_fab_modules(root: &Path, dir: &Path, modules: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fab_modules(root, &path, modules);
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("fab") {
            continue;
        }

        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let mut module = relative
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        if module.starts_with('/') {
            module.remove(0);
        }
        modules.push(module);
    }
}

// ── Library interface cache, expansion, and provenance ─────────────────────

use std::collections::BTreeSet;
use std::fs;
use std::hash::{Hash, Hasher};

use radix_module::diagnostics::Diagnostic;
use radix_module::driver::{
    AnalyzeRequest, AnalyzedModule, Config, ModuleImportContract, PackageKernelTemplateAdmissions,
    Session, analyze_source_with_kernel_template_admissions, peel_raw_source,
    source_load_diagnostic,
};
use radix_module::file_interface::FileInterface;
use radix_module::hir::{
    DefId, HirItemKind, LibraryBinding, LibraryIdentity, LibraryItem, LibraryItemKind,
    LibraryProvider,
};
use radix_module::lexer::{Interner, Symbol};
use radix_module::parser;
use radix_module::program::program_export_surface;
use radix_module::semantic::passes::visibility_annotation::{
    VisibilityTier, has_private_visibility_annotation,
};
use radix_module::syntax::{Expr, ExprKind, Program, StmtKind, Visibility, Visitor, walk_expr};

use crate::import_graph::{
    inner_library_import_unresolved_diagnostic, library_import_binding,
    library_import_kind_diagnostic, library_resolve_diagnostic,
};
use crate::{LibraryImportBinding, PackageFile};
use radix_module::diagnostics::DiagnosticConvert;

struct LibraryInterfaceItem {
    exported_name: String,
    local_name: String,
    kind: LibraryItemKind,
    is_failable: bool,
    is_async: bool,
}

struct CachedLibraryInterface {
    peeled_body: String,
    program: Program,
    interner: Interner,
    analysis: Option<AnalyzedModule>,
    file_interface: Option<FileInterface>,
    expanded_imports: Vec<LibraryImportBinding>,
    /// Reader locale pack the module's own package (or file frontmatter)
    /// selects; used to lex and analyze the interface regardless of the
    /// consuming package's locale.
    locale_pack: Option<radix_module::locale::LocalePack>,
}

type LibraryIdentityKey = (u8, String, Vec<String>);

/// Reader-locale-pack loading seam.
///
/// The library cache decides *which* locale a library interface is read under
/// (file frontmatter → module manifest `[reader] locale` → Latin). Loading the
/// pack itself is product layout (installed packs beside the binary, manifest
/// `[locale] pack` overrides) and stays faber-side (locale is a faber product
/// surface); the caller supplies that loader. `None` means no loader is
/// available, and a declared locale is a diagnostic instead of a silent Latin
/// fallback.
pub type LocaleLoader =
    fn(&Path, Option<&str>) -> Result<Option<radix_module::locale::LocalePack>, Box<Diagnostic>>;

pub struct LibraryInterfaceCache {
    entries: BTreeMap<LibraryIdentityKey, CachedLibraryInterface>,
    analysis_config: Config,
    locale_loader: Option<LocaleLoader>,
    /// Modules whose `analyze_cached_library_interface` frame is live.
    /// Re-entry before `file_interface` is stored would restart analysis of
    /// the same key and never terminate.
    analyzing: BTreeSet<LibraryIdentityKey>,
}

impl Default for LibraryInterfaceCache {
    fn default() -> Self {
        Self::with_config(&Config::default())
    }
}

impl LibraryInterfaceCache {
    #[must_use]
    pub fn with_config(config: &Config) -> Self {
        Self {
            entries: BTreeMap::new(),
            analysis_config: Config::default()
                .with_target(config.target)
                .with_bodyless_functions(),
            locale_loader: None,
            analyzing: BTreeSet::new(),
        }
    }

    /// Attach the caller's reader-locale-pack loader (faber product seam).
    #[must_use]
    pub fn with_locale_loader(mut self, loader: LocaleLoader) -> Self {
        self.locale_loader = Some(loader);
        self
    }

    /// True when `other` would cache byte-identical analysis results: the same
    /// analysis target (the only [`Config`] field `with_config` forwards) and
    /// the same reader-locale loader. A cache carried from one pipeline stage
    /// into the next is reusable only under this equality.
    pub(crate) fn analyzes_like(&self, other: &Self) -> bool {
        self.analysis_config.target == other.analysis_config.target
            && match (self.locale_loader, other.locale_loader) {
                (Some(left), Some(right)) => std::ptr::fn_addr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

#[cfg(test)]
thread_local! {
    /// Test counter: one `(target, library identity)` record per library
    /// interface analysis performed on this thread.
    pub(crate) static LIBRARY_ANALYSIS_LOG: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn analysis_source_for_file(
    file: &PackageFile,
    _library_resolver: &LibraryResolver,
    _library_cache: &mut LibraryInterfaceCache,
) -> Result<String, Diagnostic> {
    Ok(file.source.clone())
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn library_interface_export_surface(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<BTreeMap<String, VisibilityTier>, Diagnostic> {
    let cached = load_cached_library_interface(&import.module, library_cache)?;
    let mut surface = program_export_surface(&cached.program, &cached.interner);
    for child in public_library_imports(import, library_resolver, library_cache)? {
        surface.insert(child.binding.clone(), VisibilityTier::Publica);
        for (child_member, tier) in
            library_interface_export_surface(&child, library_resolver, library_cache)?
        {
            surface.insert(format!("{}.{}", child.binding, child_member), tier);
        }
    }
    Ok(surface)
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn library_interface_declaration_surface(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<BTreeMap<String, VisibilityTier>, Diagnostic> {
    // Library interfaces are export-only. Private names are not declarations
    // at this boundary, so later `namespace_missing_export` /
    // `qualified_type_not_exported` fire instead of SEM006.
    library_interface_export_surface(import, library_resolver, library_cache)
}

fn library_module_label(import: &LibraryImportBinding) -> String {
    import
        .module
        .module_name()
        .map_or_else(|| import.module.package.clone(), str::to_owned)
}

fn library_identity_key(identity: &LibraryIdentity) -> LibraryIdentityKey {
    match &identity.provider {
        LibraryProvider::Builtin(package) => (0, package.clone(), identity.module_path.clone()),
        LibraryProvider::Package(package) => (1, package.clone(), identity.module_path.clone()),
    }
}

#[must_use]
pub fn synthetic_library_item_def_id(
    module: &ResolvedLibraryModule,
    exported_name: &str,
    kind: &LibraryItemKind,
) -> DefId {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    module.package.hash(&mut hasher);
    module.module_path.hash(&mut hasher);
    exported_name.hash(&mut hasher);
    kind.hash(&mut hasher);
    DefId(0x8000_0000 | ((hasher.finish() & 0x7fff_ffff) as u32))
}

fn library_generated_call_path_with_crate(
    module: &ResolvedLibraryModule,
    exported_name: &str,
    linked_crate: Option<&str>,
) -> String {
    let mut path = match linked_crate {
        Some(crate_name) => rust_ident(crate_name),
        None => String::from("crate"),
    };
    for segment in &module.module_path {
        path.push_str("::");
        path.push_str(&rust_ident(segment));
    }
    path.push_str("::");
    path.push_str(exported_name);
    path
}

fn library_reexport_path_with_crate(
    module: &ResolvedLibraryModule,
    linked_crate: Option<&str>,
) -> String {
    let mut path = match linked_crate {
        Some(crate_name) => rust_ident(crate_name),
        None => String::from("crate"),
    };
    for segment in &module.module_path {
        path.push_str("::");
        path.push_str(&rust_ident(segment));
    }
    path
}

fn rust_ident(name: &str) -> String {
    // Shared with package module path emission so call paths match `pub mod` trees.
    crate::modules::sanitize_rust_module_ident(name)
}

struct TransitiveLibraryWalk {
    ordered: Vec<LibraryImportBinding>,
    seen: BTreeSet<LibraryIdentityKey>,
    seen_bindings: BTreeMap<LibraryIdentityKey, String>,
    visiting: BTreeSet<LibraryIdentityKey>,
    stack: Vec<String>,
    cycle_detected: bool,
}

pub fn expand_library_imports(
    direct: &[LibraryImportBinding],
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<LibraryImportBinding> {
    let mut walk = TransitiveLibraryWalk {
        ordered: Vec::new(),
        seen: BTreeSet::new(),
        seen_bindings: BTreeMap::new(),
        visiting: BTreeSet::new(),
        stack: Vec::new(),
        cycle_detected: false,
    };

    for import in direct {
        visit_transitive_library_import(
            import,
            library_resolver,
            library_cache,
            &mut walk,
            diagnostics,
        );
    }

    if walk.cycle_detected || diagnostics.iter().any(Diagnostic::is_error) {
        Vec::new()
    } else {
        walk.ordered
    }
}

fn take_first_error(diagnostics: &mut Vec<Diagnostic>) -> Option<Diagnostic> {
    let index = diagnostics.iter().position(Diagnostic::is_error)?;
    Some(diagnostics.remove(index))
}

fn visit_transitive_library_import(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    walk: &mut TransitiveLibraryWalk,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if walk.cycle_detected {
        return;
    }

    let identity = library_identity(&import.module);
    let key = library_identity_key(&identity);
    let module_label = library_module_label(import);

    if walk.seen.contains(&key) {
        if let Some(existing) = walk.seen_bindings.get(&key)
            && existing != &import.binding
        {
            diagnostics.push(
                    crate::package_diagnostic_error(format!(
                        "library module `{module_label}` is imported under conflicting aliases `{existing}` and `{}`",
                        import.binding
                    ))
                    .with_file(import.module.interface_path.display().to_string())
                    .with_arg("issue", "library_conflicting_aliases")
                    .with_arg("module", module_label.clone())
                    .with_arg("existing_alias", existing.clone())
                    .with_arg("alias", import.binding.clone())
                    .with_span(import.import_span),
                );
        }
        return;
    }

    if walk.visiting.contains(&key) {
        let mut cycle = walk.stack.clone();
        cycle.push(module_label);
        diagnostics.push(
            crate::package_diagnostic_error(format!(
                "library import cycle detected: {}",
                cycle.join(" -> ")
            ))
            .with_file(import.module.interface_path.display().to_string())
            .with_arg("issue", "library_import_cycle")
            .with_arg("cycle", cycle.join(" -> "))
            .with_span(import.import_span),
        );
        walk.cycle_detected = true;
        return;
    }

    walk.visiting.insert(key.clone());
    walk.stack.push(module_label);

    let (inner_imports, _) =
        library_imports_for_module(&import.module, library_resolver, library_cache, diagnostics);
    for inner in &inner_imports {
        visit_transitive_library_import(inner, library_resolver, library_cache, walk, diagnostics);
        if walk.cycle_detected {
            walk.stack.pop();
            walk.visiting.remove(&key);
            return;
        }
    }

    walk.stack.pop();
    walk.visiting.remove(&key);
    walk.seen.insert(key.clone());
    walk.seen_bindings.insert(key, import.binding.clone());
    walk.ordered.push(import.clone());
}

/// Resolve the reader locale pack a library interface module should be read
/// under: the file's own `+++ locale = … +++` frontmatter wins, otherwise the
/// module package's manifest `[reader] locale` applies, otherwise Latin.
///
/// The consuming package's locale is deliberately NOT consulted: each module
/// is read in its own repo's language (manifest-level default, per-file
/// frontmatter override), which is what makes mixed-language consumers and
/// libraries compose.
fn resolve_library_interface_locale_pack(
    module: &ResolvedLibraryModule,
    frontmatter: Option<&radix_module::driver::FileFrontmatter>,
    locale_loader: Option<LocaleLoader>,
) -> Result<Option<radix_module::locale::LocalePack>, Diagnostic> {
    match frontmatter.and_then(radix_module::driver::FileFrontmatter::locale_result) {
        Some(Err(())) => Err(crate::package_diagnostic_error(
            "frontmatter 'locale' key must be a string",
        )
        .with_file(module.interface_path.display().to_string())
        .with_arg("issue", "frontmatter_locale_type")),
        Some(Ok(locale)) if !locale.trim().is_empty() => {
            load_library_locale_pack(locale_loader, &module.interface_path, Some(locale.trim()))
        }
        Some(Ok(_)) | None => match library_declared_reader_locale(&module.interface_path) {
            Some(locale) => {
                load_library_locale_pack(locale_loader, &module.interface_path, Some(&locale))
            }
            // The library declares no locale: the caller reads the interface
            // under the no-locale default (`en`).
            None => Ok(None),
        },
    }
}

fn load_library_locale_pack(
    locale_loader: Option<LocaleLoader>,
    interface_path: &Path,
    locale: Option<&str>,
) -> Result<Option<radix_module::locale::LocalePack>, Diagnostic> {
    let Some(loader) = locale_loader else {
        return Err(crate::package_diagnostic_error(format!(
            "library interface `{}` declares reader locale `{}` but no locale-pack loader is available",
            interface_path.display(),
            locale.unwrap_or_default()
        ))
        .with_file(interface_path.display().to_string()));
    };
    loader(interface_path, locale).map_err(|err| *err)
}

/// The `[reader] locale` the module's own package manifest declares, when any.
///
/// This never falls back to the product default: `None` means the library
/// package declares no locale, and the interface seam then applies the
/// no-locale default itself.
fn library_declared_reader_locale(interface_path: &Path) -> Option<String> {
    let spec = crate::discovery::discover_package(interface_path).ok()?;
    let manifest_path = crate::frontmatter::manifest_path_for_spec(&spec)?;
    let manifest = crate::manifest::read_manifest(&manifest_path).ok()?;
    manifest
        .locale
        .locale
        .map(|locale| locale.trim().to_owned())
        .filter(|locale| !locale.is_empty())
}

fn read_and_parse_library_interface(
    module: &ResolvedLibraryModule,
    library_cache: &LibraryInterfaceCache,
) -> Result<CachedLibraryInterface, Diagnostic> {
    let raw_source = fs::read_to_string(&module.interface_path)
        .map_err(|err| Diagnostic::io_error(&module.interface_path, &err))?;
    let display_name = module.interface_path.display().to_string();
    let peeled = peel_raw_source(&display_name, &raw_source)
        .map_err(|error| source_load_diagnostic(&display_name, &error))?;
    let locale_pack = resolve_library_interface_locale_pack(
        module,
        peeled.frontmatter.as_ref(),
        library_cache.locale_loader,
    )?;
    // With no declared locale the interface reads the no-locale default (`en`).
    // The resolved pack is cached so interface analysis reads the same surface.
    let locale_pack = match locale_pack {
        Some(pack) => pack,
        None => radix_module::driver::default_code_pack(&radix_module::driver::Config::default())
            .map_err(|diagnostic| *diagnostic)?,
    };
    // Library interfaces may declare bodyless functions bound by target manifests (G4).
    let tokens = radix_module::lexer::lex_with_locale_pack(peeled.body, &locale_pack);
    let parse = parser::parse_with_options(
        tokens,
        parser::ParseOptions {
            allow_bodyless_functions: true,
        },
    );
    if !parse.success() {
        let mut message = format!(
            "library interface `{}` failed to parse",
            module.interface_path.display()
        );
        if let Some(err) = parse.errors.first() {
            let diagnostic = Diagnostic::from_parse_error(&display_name, peeled.body, err);
            message.push_str(": ");
            message.push_str(&diagnostic.message);
        }
        return Err(crate::package_diagnostic_error(message)
            .with_file(module.interface_path.display().to_string()));
    }

    let Some(program) = parse.program else {
        return Err(crate::package_diagnostic_error(
            "successful library interface parse result missing program",
        )
        .with_file(module.interface_path.display().to_string()));
    };

    Ok(CachedLibraryInterface {
        peeled_body: peeled.body.to_owned(),
        program,
        interner: parse.interner,
        analysis: None,
        file_interface: None,
        expanded_imports: Vec::new(),
        locale_pack: Some(locale_pack),
    })
}

fn load_cached_library_interface<'a>(
    module: &ResolvedLibraryModule,
    library_cache: &'a mut LibraryInterfaceCache,
) -> Result<&'a CachedLibraryInterface, Diagnostic> {
    let key = library_identity_key(&library_identity(module));
    if !library_cache.entries.contains_key(&key) {
        let cached = read_and_parse_library_interface(module, library_cache)?;
        library_cache.entries.insert(key.clone(), cached);
    }
    library_cache.entries.get(&key).ok_or_else(|| {
        crate::package_diagnostic_error("library interface cache missing entry after insert")
            .with_file(module.interface_path.display().to_string())
    })
}

/// The declared `regio` name of one library module (D7.7), read through the
/// interface cache so the parse is shared with later interface work.
///
/// # Errors
/// Returns an error when the interface cannot be read or parsed.
pub fn library_module_region_name(
    module: &ResolvedLibraryModule,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Option<String>, Diagnostic> {
    let cached = load_cached_library_interface(module, library_cache)?;
    Ok(radix_module::program::program_region_name(
        &cached.program,
        &cached.interner,
    ))
}

/// Peeled Faber body of one cached library interface module — the provider
/// source the package-analysis carriage analyzes under the codegen contract.
pub(crate) fn library_interface_source(
    module: &ResolvedLibraryModule,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<String, Diagnostic> {
    let cached = load_cached_library_interface(module, library_cache)?;
    Ok(cached.peeled_body.clone())
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn library_cached_file_interface(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<FileInterface, Diagnostic> {
    let key = library_identity_key(&library_identity(&import.module));
    if let Some(interface) = library_cache
        .entries
        .get(&key)
        .and_then(|cached| cached.file_interface.clone())
    {
        return Ok(interface);
    }

    analyze_cached_library_interface(import, library_resolver, library_cache)?;
    library_cache
        .entries
        .get(&key)
        .and_then(|cached| cached.file_interface.clone())
        .ok_or_else(|| {
            crate::package_diagnostic_error(
                "library interface cache missing extracted file interface",
            )
            .with_file(import.module.interface_path.display().to_string())
        })
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn library_cached_analysis<'a>(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &'a mut LibraryInterfaceCache,
) -> Result<&'a AnalyzedModule, Diagnostic> {
    let key = library_identity_key(&library_identity(&import.module));
    if library_cache
        .entries
        .get(&key)
        .is_none_or(|cached| cached.analysis.is_none())
    {
        analyze_cached_library_interface(import, library_resolver, library_cache)?;
    }
    library_cache
        .entries
        .get(&key)
        .and_then(|cached| cached.analysis.as_ref())
        .ok_or_else(|| {
            crate::package_diagnostic_error("library interface cache missing analyzed unit")
                .with_file(import.module.interface_path.display().to_string())
        })
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
#[cfg(feature = "hir-rust")]
pub fn library_cached_expanded_imports(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Vec<LibraryImportBinding>, Diagnostic> {
    let key = library_identity_key(&library_identity(&import.module));
    if library_cache
        .entries
        .get(&key)
        .is_none_or(|cached| cached.analysis.is_none())
    {
        analyze_cached_library_interface(import, library_resolver, library_cache)?;
    }
    library_cache
        .entries
        .get(&key)
        .map(|cached| cached.expanded_imports.clone())
        .ok_or_else(|| {
            crate::package_diagnostic_error("library interface cache missing expanded imports")
                .with_file(import.module.interface_path.display().to_string())
        })
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn with_library_cached_analysis_mut<T>(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    f: impl FnOnce(
        &mut AnalyzedModule,
        &mut LibraryInterfaceCache,
    ) -> Result<T, radix_module::codegen::CodegenError>,
) -> Result<T, Diagnostic> {
    let key = library_identity_key(&library_identity(&import.module));
    if library_cache
        .entries
        .get(&key)
        .is_none_or(|cached| cached.analysis.is_none())
    {
        analyze_cached_library_interface(import, library_resolver, library_cache)?;
    }
    let mut cached = library_cache.entries.remove(&key).ok_or_else(|| {
        crate::package_diagnostic_error("library interface cache missing entry after insert")
            .with_file(import.module.interface_path.display().to_string())
    })?;
    let result = match cached.analysis.as_mut() {
        Some(analysis) => f(analysis, library_cache).map_err(|err| {
            Diagnostic::codegen_error(&err.message)
                .with_file(import.module.interface_path.display().to_string())
                .with_args(err.args)
        }),
        None => Err(crate::package_diagnostic_error(
            "library interface cache missing analyzed unit",
        )
        .with_file(import.module.interface_path.display().to_string())),
    };
    library_cache.entries.insert(key, cached);
    result
}

#[cfg(feature = "hir-rust")]
#[must_use]
pub fn library_module_segments(import: &LibraryImportBinding) -> Vec<String> {
    import.module.module_path.clone()
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
#[cfg(feature = "hir-rust")]
pub fn library_generates_rust_module(
    import: &LibraryImportBinding,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<bool, Diagnostic> {
    let cached = load_cached_library_interface(&import.module, library_cache)?;
    Ok(cached.program.statements.iter().any(|stmt| {
        !has_private_visibility_annotation(stmt, &cached.interner)
            && matches!(
                stmt.kind,
                StmtKind::Func(_)
                    | StmtKind::Class(_)
                    | StmtKind::Enum(_)
                    | StmtKind::Union(_)
                    | StmtKind::TypeAlias(_)
                    | StmtKind::Var(_)
            )
    }))
}

fn analyze_cached_library_interface(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Diagnostic> {
    let key = library_identity_key(&library_identity(&import.module));
    if !library_cache.analyzing.insert(key.clone()) {
        return Err(library_analysis_reentry_diagnostic(import));
    }
    let result =
        analyze_cached_library_interface_body(import, library_resolver, library_cache, &key);
    library_cache.analyzing.remove(&key);
    result
}

fn library_analysis_reentry_diagnostic(import: &LibraryImportBinding) -> Diagnostic {
    let module_label = library_module_label(import);
    crate::package_diagnostic_error(format!(
        "library interface analysis re-entered for `{module_label}` before the cached file interface was stored"
    ))
    .with_file(import.module.interface_path.display().to_string())
    .with_arg("issue", "library_analysis_reentry")
    .with_arg("module", module_label)
}

fn analyze_cached_library_interface_body(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    key: &LibraryIdentityKey,
) -> Result<(), Diagnostic> {
    #[cfg(test)]
    LIBRARY_ANALYSIS_LOG.with(|log| {
        log.borrow_mut().push((
            format!("{:?}", library_cache.analysis_config.target),
            format!("{key:?}"),
        ));
    });
    let mut diagnostics = Vec::new();
    let expanded = expand_library_imports(
        std::slice::from_ref(import),
        library_resolver,
        library_cache,
        &mut diagnostics,
    );
    if let Some(error) = take_first_error(&mut diagnostics) {
        return Err(error);
    }

    for dependency in &expanded {
        let dependency_key = library_identity_key(&library_identity(&dependency.module));
        if &dependency_key != key {
            library_cached_file_interface(dependency, library_resolver, library_cache)?;
        }
    }

    let (direct_imports, default_import_names) = library_imports_for_module(
        &import.module,
        library_resolver,
        library_cache,
        &mut diagnostics,
    );
    if let Some(error) = take_first_error(&mut diagnostics) {
        return Err(error);
    }

    let (namespace_exports, file_interfaces) = library_import_interface_maps(
        &expanded,
        &direct_imports,
        key,
        library_resolver,
        library_cache,
    )?;

    let analysis_source = {
        let cached = load_cached_library_interface(&import.module, library_cache)?;
        cached.peeled_body.clone()
    };

    let (session, namespace_seams, namespace_declarations, kernel_template_admissions) =
        library_analysis_session(import, &direct_imports, library_resolver, library_cache)?;
    let mut analysis = match analyze_source_with_kernel_template_admissions(
        &session,
        &import.module.interface_path.display().to_string(),
        &analysis_source,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract {
                namespace_exports,
                file_interfaces,
                namespace_seams,
                namespace_declarations,
                default_import_names,
            },
        },
        &kernel_template_admissions,
    ) {
        Ok(analysis) => analysis,
        Err(diagnostics) => {
            return Err(diagnostics
                .into_iter()
                .find(Diagnostic::is_error)
                .unwrap_or_else(|| {
                    crate::package_diagnostic_error(
                        "library interface analysis failed without diagnostics",
                    )
                    .with_file(import.module.interface_path.display().to_string())
                }));
        }
    };
    attach_library_provenance(
        &mut analysis,
        &direct_imports,
        library_resolver,
        library_cache,
    )?;
    install_library_file_interface(
        import,
        key,
        analysis,
        expanded,
        library_resolver,
        library_cache,
    )
}

fn install_library_file_interface(
    import: &LibraryImportBinding,
    key: &LibraryIdentityKey,
    analysis: AnalyzedModule,
    expanded: Vec<LibraryImportBinding>,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Diagnostic> {
    let export_names = {
        let cached = load_cached_library_interface(&import.module, library_cache)?;
        program_export_surface(&cached.program, &cached.interner)
            .into_keys()
            .collect::<Vec<_>>()
    };
    let export_identity = crate::file_interface::ExportIdentityContext {
        provider: "package".to_owned(),
        package: Some(import.module.package.clone()),
        module_path: import.module.module_path.clone(),
    };
    let mut file_interface = crate::file_interface::extract_file_interface_with_identity(
        &analysis,
        &export_names,
        &import.module.interface_path.display().to_string(),
        Some(&export_identity),
    )?;
    // `publica` imports surface as nested namespace bindings on the consumer
    // (`parent.child.member()`); install the re-exported members on this
    // module's interface so consumers typecheck them with the child's
    // signature instead of falling back to `Ignotum`.
    for child in public_library_imports(import, library_resolver, library_cache)? {
        let child_interface =
            library_cached_file_interface(&child, library_resolver, library_cache)?;
        for (member, export) in &child_interface.exports {
            let mut reexport = export.clone();
            reexport.name = format!("{}.{}", child.binding, member);
            file_interface.insert(reexport);
        }
    }

    let cached = library_cache.entries.get_mut(key).ok_or_else(|| {
        crate::package_diagnostic_error("library interface cache missing entry after analysis")
            .with_file(import.module.interface_path.display().to_string())
    })?;
    cached.analysis = Some(analysis);
    cached.file_interface = Some(file_interface);
    cached.expanded_imports = expanded;
    Ok(())
}

type LibraryImportInterfaceMaps = (
    BTreeMap<String, BTreeMap<String, VisibilityTier>>,
    BTreeMap<String, FileInterface>,
);

fn library_import_interface_maps(
    expanded: &[LibraryImportBinding],
    direct_imports: &[LibraryImportBinding],
    key: &LibraryIdentityKey,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<LibraryImportInterfaceMaps, Diagnostic> {
    let mut namespace_exports = BTreeMap::new();
    for dependency in direct_imports {
        namespace_exports.insert(
            dependency.binding.clone(),
            library_interface_export_surface(dependency, library_resolver, library_cache)?,
        );
    }
    let mut file_interfaces = BTreeMap::new();
    for dependency in direct_imports {
        file_interfaces.insert(
            dependency.binding.clone(),
            library_cached_file_interface(dependency, library_resolver, library_cache)?,
        );
    }
    // Transitive library leaves install under their producer-side bindings,
    // mirroring the file-mode and package-unit paths (B10): a direct
    // import's public surface can name a nominal from a module this unit
    // does not import (`tensor.TensorError.ShapeFailure { shape.ShapeError
    // inner }`), and without that module's nominal registrations the payload
    // fails remap, so the variant loses its own fields and drops out of
    // coverage. A direct binding for the same name always wins; the extra
    // namespaces stay inert (no HIR import binding).
    for dependency in expanded {
        let dependency_key = library_identity_key(&library_identity(&dependency.module));
        if &dependency_key == key || file_interfaces.contains_key(&dependency.binding) {
            continue;
        }
        file_interfaces.insert(
            dependency.binding.clone(),
            library_cached_file_interface(dependency, library_resolver, library_cache)?,
        );
    }
    Ok((namespace_exports, file_interfaces))
}

type LibraryAnalysisSession = (
    Session,
    BTreeMap<String, bool>,
    BTreeMap<String, BTreeMap<String, VisibilityTier>>,
    PackageKernelTemplateAdmissions,
);

fn library_analysis_session(
    import: &LibraryImportBinding,
    direct_imports: &[LibraryImportBinding],
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<LibraryAnalysisSession, Diagnostic> {
    // Library interface analysis follows the package target. Browser products
    // must not inherit Rust-only borrow qualification from Config::default().
    // The module's own reader locale (resolved at parse time) overrides any
    // consuming-package locale: a Latin consumer must analyze an en library
    // under the en pack, and vice versa.
    let mut analysis_config = library_cache.analysis_config.clone();
    if let Some(pack) = load_cached_library_interface(&import.module, library_cache)?
        .locale_pack
        .as_ref()
    {
        analysis_config = analysis_config.with_locale_pack(pack.clone());
    } else {
        analysis_config.locale_pack = None;
    }
    let session = Session::new(analysis_config);
    let mut namespace_seams = BTreeMap::new();
    let mut namespace_declarations = BTreeMap::new();
    for dependency in direct_imports {
        namespace_seams.insert(
            dependency.binding.clone(),
            dependency.module.package == import.module.package,
        );
        namespace_declarations.insert(
            dependency.binding.clone(),
            library_interface_declaration_surface(dependency, library_resolver, library_cache)?,
        );
    }
    // PKG-ADM (library seam): a library unit's own export surface is its
    // kernel-template admission fact, mirroring the package path
    // (analyze.rs): an exported `@ publica` size-generic `@ kernel` template
    // is demand-admissible because consumers instantiate it from their own
    // units. Without this, every linked-library kernel API with no same-unit
    // caller orphans (ADM001) the moment a consumer analyzes the library.
    // `@ interna`/`@ privata` and unexported templates stay fail-closed.
    let kernel_template_admissions = {
        let cached = load_cached_library_interface(&import.module, library_cache)?;
        PackageKernelTemplateAdmissions::from_export_surface(&program_export_surface(
            &cached.program,
            &cached.interner,
        ))
    };
    Ok((
        session,
        namespace_seams,
        namespace_declarations,
        kernel_template_admissions,
    ))
}

/// A library module's own import bindings, with default names bound (D7.7).
///
/// Returns the bindings and the import path → declared `regio` name map the
/// module's analysis contract carries, so the driver's parse of the module
/// binds the same names.
fn library_imports_for_module(
    module: &ResolvedLibraryModule,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<LibraryImportBinding>, BTreeMap<String, String>) {
    let (mut imports, defaults) = match load_cached_library_interface(module, library_cache) {
        Ok(cached) => library_imports_from_cached(
            cached,
            &module.interface_path,
            library_resolver,
            diagnostics,
        ),
        Err(diag) => {
            diagnostics.push(diag);
            return (Vec::new(), BTreeMap::new());
        }
    };
    let mut names = BTreeMap::new();
    for (index, import_path) in defaults {
        match library_module_region_name(&imports[index].module, library_cache) {
            Ok(Some(region)) => {
                imports[index].binding.clone_from(&region);
                names.insert(import_path, region);
            }
            Ok(None) => {}
            Err(diag) => diagnostics.push(diag),
        }
    }
    (imports, names)
}

/// Import bindings of one cached library module, plus the index and import
/// path of every default-named binding (D7.7) for the caller to bind.
fn library_imports_from_cached(
    cached: &CachedLibraryInterface,
    interface_path: &Path,
    library_resolver: &LibraryResolver,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<LibraryImportBinding>, Vec<(usize, String)>) {
    let mut imports = Vec::new();
    let mut defaults = Vec::new();
    for stmt in &cached.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let import_path = cached.interner.resolve(decl.path);
        match library_resolver.resolve(import_path) {
            Ok(Some(resolved)) => {
                if let Some(binding) = library_import_binding(&cached.interner, decl, resolved) {
                    if radix_module::program::import_takes_default_name(decl) {
                        defaults.push((imports.len(), import_path.to_owned()));
                    }
                    imports.push(binding);
                } else {
                    diagnostics.push(library_import_kind_diagnostic(
                        interface_path,
                        decl,
                        import_path,
                    ));
                }
            }
            Ok(None) => {
                diagnostics.push(inner_library_import_unresolved_diagnostic(
                    interface_path,
                    decl,
                    import_path,
                ));
            }
            // Source context, not a bare span: a bare span renders its byte
            // offset as the location (`file:2589` for line 40), sending the
            // reader to a line that does not exist. Spans
            // are peeled-body relative — the cached program was lexed from
            // `peeled_body` — so the mapping source is the peeled body.
            Err(err) => diagnostics.push(
                library_resolve_diagnostic(interface_path, err).with_source_context(
                    &interface_path.display().to_string(),
                    &cached.peeled_body,
                    decl.span,
                ),
            ),
        }
    }

    (imports, defaults)
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn library_interface_has_module(
    _import: &LibraryImportBinding,
    _library_cache: &mut LibraryInterfaceCache,
) -> Result<bool, Diagnostic> {
    Ok(false)
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn attach_library_provenance(
    analysis: &mut radix_module::driver::AnalyzedModule,
    imports: &[LibraryImportBinding],
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Diagnostic> {
    attach_library_provenance_with_links(analysis, imports, library_resolver, library_cache, None)
}

/// Attach library provenance, routing native-binding package deps to external crates (G4).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn attach_library_provenance_with_links(
    analysis: &mut radix_module::driver::AnalyzedModule,
    imports: &[LibraryImportBinding],
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    linked_library_crates: Option<&BTreeMap<String, String>>,
) -> Result<(), Diagnostic> {
    if imports.is_empty() {
        return Ok(());
    }

    let (hir_items, hir_import_bindings) = library_hir_name_index(analysis);
    for import in imports {
        let identity = library_identity(&import.module);
        let linked_crate = linked_library_crates.and_then(|map| map.get(&import.module.package));
        let interface_items = library_interface_items(import, library_cache)?;
        let exported_members =
            library_interface_export_surface(import, library_resolver, library_cache)?;
        let has_namespace_binding = library_interface_has_module(import, library_cache)?;

        let (binding_def_id, imported_type_def_ids) =
            library_import_binding_def_ids(analysis, import, &hir_items, &hir_import_bindings);

        if let Some(binding_def_id) = binding_def_id {
            analysis.libraries.bindings.insert(
                binding_def_id,
                LibraryBinding {
                    local_def_id: binding_def_id,
                    identity: identity.clone(),
                    rust_runtime_module: None,
                    rust_runtime_methods: BTreeMap::new(),
                },
            );
            analysis
                .libraries
                .exports
                .insert(binding_def_id, exported_members.keys().cloned().collect());
            for child_import in public_library_imports(import, library_resolver, library_cache)? {
                let child_crate = linked_library_crates
                    .and_then(|map| map.get(&child_import.module.package))
                    .map(String::as_str);
                analysis.libraries.reexports.insert(
                    (binding_def_id, child_import.binding),
                    library_reexport_path_with_crate(&child_import.module, child_crate),
                );
            }
        } else if has_namespace_binding {
            return Err(crate::package_diagnostic_error(format!(
                "library import `{}` did not produce binding `{}` in analyzed HIR",
                import.module.package, import.binding
            ))
            .with_span(import.import_span));
        } else if !exported_members.is_empty() {
            ensure_library_members_have_bindings(import, &exported_members, &interface_items)?;
        }

        for interface_item in interface_items {
            let rust_runtime_type = library_item_runtime_path_with_crate(
                &import.module,
                &interface_item,
                linked_crate.map(String::as_str),
            );
            let def_id = lookup_library_item_def_id(
                &hir_items,
                &interface_item.local_name,
                &interface_item.kind,
            )
            .or_else(|| {
                imported_type_def_ids
                    .get(&interface_item.exported_name)
                    .copied()
            })
            .unwrap_or_else(|| {
                synthetic_library_item_def_id(
                    &import.module,
                    &interface_item.exported_name,
                    &interface_item.kind,
                )
            });

            analysis.libraries.items.insert(
                def_id,
                LibraryItem {
                    def_id,
                    identity: identity.clone(),
                    exported_name: interface_item.exported_name,
                    kind: interface_item.kind,
                    is_failable: interface_item.is_failable,
                    is_async: interface_item.is_async,
                    rust_runtime_type,
                    elide_rust_decl: false,
                },
            );
        }
    }

    Ok(())
}

fn library_import_binding_def_ids(
    analysis: &radix_module::driver::AnalyzedModule,
    import: &LibraryImportBinding,
    hir_items: &[(String, LibraryItemKind, DefId)],
    hir_import_bindings: &[(String, Symbol, DefId)],
) -> (Option<DefId>, BTreeMap<String, DefId>) {
    let binding_def_id = hir_import_bindings
        .iter()
        .find(|(name, _, _)| name.as_str() == import.binding)
        .map(|(_, _, def_id)| *def_id)
        .or_else(|| {
            hir_items
                .iter()
                .find(|(name, _, _)| name.as_str() == import.binding)
                .map(|(_, _, def_id)| *def_id)
        });
    let imported_type_def_ids = hir_import_bindings
        .iter()
        .find(|(name, _, _)| name.as_str() == import.binding)
        .map(|(_, binding, _)| {
            analysis
                .resolver
                .imported_file_type_exports(*binding)
                .into_iter()
                .map(|export| {
                    (
                        analysis.interner.resolve(export.member).to_owned(),
                        export.def_id,
                    )
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    (binding_def_id, imported_type_def_ids)
}

fn ensure_library_members_have_bindings(
    import: &LibraryImportBinding,
    exported_members: &BTreeMap<String, VisibilityTier>,
    interface_items: &[LibraryInterfaceItem],
) -> Result<(), Diagnostic> {
    for exported_member in exported_members.keys() {
        if interface_items
            .iter()
            .all(|item| item.exported_name != *exported_member)
        {
            return Err(crate::package_diagnostic_error(format!(
                "library import `{}` did not produce binding `{}` in analyzed HIR",
                import.module.package, import.binding
            ))
            .with_span(import.import_span));
        }
    }
    Ok(())
}

fn library_hir_name_index(
    analysis: &radix_module::driver::AnalyzedModule,
) -> (
    Vec<(String, LibraryItemKind, DefId)>,
    Vec<(String, Symbol, DefId)>,
) {
    let hir_items = analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            hir_item_name_and_kind(item, &analysis.interner)
                .map(|(name, kind)| (name, kind, item.def_id))
        })
        .collect::<Vec<_>>();
    let hir_import_bindings = analysis
        .hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Import(import) => Some(import),
            _ => None,
        })
        .flat_map(|import| {
            import.items.iter().map(|item| {
                let binding = item.alias.unwrap_or(item.name);
                (
                    analysis.interner.resolve(binding).to_owned(),
                    binding,
                    item.def_id,
                )
            })
        })
        .collect::<Vec<_>>();
    (hir_items, hir_import_bindings)
}

fn public_library_imports(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Vec<LibraryImportBinding>, Diagnostic> {
    let mut diagnostics = Vec::new();
    let imports = library_imports_for_module(
        &import.module,
        library_resolver,
        library_cache,
        &mut diagnostics,
    )
    .0
    .into_iter()
    .filter(|binding| binding.visibility == Visibility::Publica)
    .collect::<Vec<_>>();
    if let Some(error) = take_first_error(&mut diagnostics) {
        return Err(error);
    }
    Ok(imports)
}

fn library_item_runtime_path_with_crate(
    module: &ResolvedLibraryModule,
    item: &LibraryInterfaceItem,
    linked_crate: Option<&str>,
) -> Option<String> {
    if item.kind == LibraryItemKind::Function {
        return Some(library_generated_call_path_with_crate(
            module,
            &item.exported_name,
            linked_crate,
        ));
    }
    None
}

fn lookup_library_item_def_id(
    hir_items: &[(String, LibraryItemKind, DefId)],
    local_name: &str,
    kind: &LibraryItemKind,
) -> Option<DefId> {
    hir_items
        .iter()
        .find(|(name, item_kind, _)| name == local_name && item_kind == kind)
        .map(|(_, _, def_id)| *def_id)
}

fn library_identity(module: &ResolvedLibraryModule) -> LibraryIdentity {
    let provider = if module.package == "norma" {
        LibraryProvider::Builtin(module.package.clone())
    } else {
        LibraryProvider::Package(module.package.clone())
    };
    LibraryIdentity {
        provider,
        module_path: module.module_path.clone(),
    }
}

fn library_interface_items(
    import: &LibraryImportBinding,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Vec<LibraryInterfaceItem>, Diagnostic> {
    let cached = load_cached_library_interface(&import.module, library_cache)?;

    let mut items = Vec::new();
    for stmt in &cached.program.statements {
        if has_private_visibility_annotation(stmt, &cached.interner) {
            continue;
        }
        if let Some(mut item) = library_interface_item(stmt, &cached.interner)? {
            item.is_async = cached
                .file_interface
                .as_ref()
                .and_then(|interface| interface.exports.get(&item.exported_name))
                .is_some_and(|export| {
                    matches!(
                        &export.kind,
                        radix_module::file_interface::FileExportKind::Function(callable) if callable.is_async
                    )
                });
            items.push(item);
        }
    }
    Ok(items)
}

/// True when a function body opens a host stream (`ad`) so package codegen can
/// mark the library item failable (Result + fac/cape) without requiring ⇥.
///
/// Fails closed: the walker refusing to descend past its depth limit is
/// reported, not read as "no `ad`", because this answer decides the exported
/// interface's failure shape.
fn function_body_contains_ad(
    body: Option<&radix_module::syntax::BlockStmt>,
) -> Result<bool, Diagnostic> {
    struct AdFinder {
        found: bool,
        /// Depth the walker refused to enter, if it stopped short.
        depth_exceeded: Option<radix_module::syntax::DepthExceeded>,
    }
    impl Visitor for AdFinder {
        fn on_depth_exceeded(&mut self, exceeded: radix_module::syntax::DepthExceeded) {
            self.depth_exceeded = Some(exceeded);
        }

        fn visit_expr(&mut self, expr: &Expr) {
            if self.found {
                return;
            }
            if matches!(expr.kind, ExprKind::Ad(_)) {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }
    }
    let Some(body) = body else {
        return Ok(false);
    };
    let mut finder = AdFinder {
        found: false,
        depth_exceeded: None,
    };
    finder.visit_block(body);
    if let Some(exceeded) = finder.depth_exceeded {
        return Err(crate::package_diagnostic_error(format!(
            "function body exceeds the AST walk depth limit ({}): cannot decide whether it opens a host stream",
            exceeded.limit
        )));
    }
    Ok(finder.found)
}

fn library_interface_item(
    stmt: &radix_module::syntax::Stmt,
    interner: &Interner,
) -> Result<Option<LibraryInterfaceItem>, Diagnostic> {
    let (name, kind, is_failable) = match &stmt.kind {
        StmtKind::Interface(interface) => (
            interner.resolve(interface.name.name),
            LibraryItemKind::Interface,
            false,
        ),
        StmtKind::Func(func) => (
            interner.resolve(func.name.name),
            LibraryItemKind::Function,
            // Declared ⇥ or host `ad` materialize (codegen-failable for fac/cape).
            func.err.is_some() || function_body_contains_ad(func.body.as_ref())?,
        ),
        StmtKind::TypeAlias(alias) => (
            interner.resolve(alias.name.name),
            LibraryItemKind::TypeAlias,
            false,
        ),
        StmtKind::Class(class) => (
            interner.resolve(class.name.name),
            LibraryItemKind::Struct,
            false,
        ),
        StmtKind::Enum(enm) => (
            interner.resolve(enm.name.name),
            LibraryItemKind::Enum,
            false,
        ),
        StmtKind::Union(union) => (
            interner.resolve(union.name.name),
            LibraryItemKind::Enum,
            false,
        ),
        StmtKind::Var(var) => {
            let radix_module::syntax::BindingPattern::Ident(ident) = &var.binding else {
                return Ok(None);
            };
            (interner.resolve(ident.name), LibraryItemKind::Const, false)
        }
        _ => return Ok(None),
    };
    Ok(Some(LibraryInterfaceItem {
        exported_name: name.to_owned(),
        local_name: name.to_owned(),
        kind,
        is_failable,
        is_async: false,
    }))
}

fn hir_item_name_and_kind(
    item: &radix_module::hir::HirItem,
    interner: &Interner,
) -> Option<(String, LibraryItemKind)> {
    match &item.kind {
        HirItemKind::Interface(interface) => Some((
            interner.resolve(interface.name).to_owned(),
            LibraryItemKind::Interface,
        )),
        HirItemKind::Function(func) => Some((
            interner.resolve(func.name).to_owned(),
            LibraryItemKind::Function,
        )),
        HirItemKind::TypeAlias(alias) => Some((
            interner.resolve(alias.name).to_owned(),
            LibraryItemKind::TypeAlias,
        )),
        HirItemKind::Struct(strukt) => Some((
            interner.resolve(strukt.name).to_owned(),
            LibraryItemKind::Struct,
        )),
        HirItemKind::Enum(enm) => {
            Some((interner.resolve(enm.name).to_owned(), LibraryItemKind::Enum))
        }
        HirItemKind::Constant(konst) => Some((
            interner.resolve(konst.name).to_owned(),
            LibraryItemKind::Const,
        )),
        HirItemKind::Schema(_) | HirItemKind::Import(_) => None,
    }
}

#[cfg(test)]
#[path = "library_test.rs"]
mod tests;

#[cfg(test)]
#[path = "library_transitive_nominal_test.rs"]
mod transitive_nominal_tests;

#[cfg(test)]
#[path = "library_air_import_boundary_test.rs"]
mod air_import_boundary_tests;

// In-file pin for the interface scan above.
#[cfg(test)]
#[path = "ad_finder_budget_test.rs"]
mod ad_finder_budget_test;
