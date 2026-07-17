//! Library import resolution for package compilation.
//!
//! `faber` treats public libraries as source-level interfaces installed under a
//! shared library home, not as hard-coded compiler magic. The resolver
//! translates an import specifier such as `norma:solum` into the `.fab`
//! interface that package loading can parse, typecheck, and wire into generated
//! output.
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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) const FABER_LIBRARY_HOME_ENV: &str = "FABER_LIBRARY_HOME";
pub(crate) const FABER_DISABLE_WORKSPACE_LIBRARY_PROBE_ENV: &str =
    "FABER_DISABLE_WORKSPACE_LIBRARY_PROBE";

/// Locked package interface root used for build-time resolution.
///
/// Paths are absolute file-system locations from `faber.lock`. Faber does not
/// discover package-manager store roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockedLibraryPackage {
    /// Declared package/provider name.
    pub name: String,
    /// Exact version pin from the lock.
    pub version: String,
    /// Directory containing `.fab` interface files for this package.
    pub interface_root: PathBuf,
}

/// Origin class for a resolved library module.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum LibraryProviderKind {
    /// Package-managed library resolved from the public library home.
    PackageDependency,
}

/// Resolved import target for a source-level library module.
///
/// The path points at the Faber interface source, not at generated Rust or a
/// runtime artifact. Downstream package loading relies on that distinction so
/// imported APIs go through the same parser and typechecker as local sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedLibraryModule {
    /// Provider package name, such as `norma`.
    pub package: String,

    /// Module path inside the provider, split on `/`.
    pub module_path: Vec<String>,

    /// Source interface file consumed by package loading.
    pub interface_path: PathBuf,

    /// Provider class used by diagnostics and future resolver routing.
    pub provider: LibraryProviderKind,
}

impl ResolvedLibraryModule {
    pub(crate) fn new(
        package: impl Into<String>,
        module_path: Vec<String>,
        interface_path: impl Into<PathBuf>,
        provider: LibraryProviderKind,
    ) -> Self {
        Self {
            package: package.into(),
            module_path,
            interface_path: interface_path.into(),
            provider,
        }
    }

    /// Return the terminal module segment expected to match named imports.
    pub(crate) fn module_name(&self) -> Option<&str> {
        self.module_path.last().map(String::as_str)
    }
}

/// Errors that mean a specifier selected a library provider but not a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LibraryResolveError {
    /// The old built-in slash form was used for a Norma module.
    OldBuiltinNormaSpecifier {
        /// Original import specifier from source.
        specifier: String,

        /// Provider-qualified replacement.
        replacement: String,
    },

    /// The specifier is provider-shaped but malformed.
    InvalidProviderSpecifier {
        /// Original import specifier from source.
        specifier: String,

        /// Targeted reason for the invalid shape.
        reason: String,
    },

    /// The provider separator selected a provider that is not implemented.
    UnknownProvider {
        /// Original import specifier from source.
        specifier: String,

        /// Provider segment from the source specifier.
        provider: String,

        /// Library home used for provider lookup.
        library_home: PathBuf,
    },

    /// A provider repo exists but does not expose a readable top-level manifest.
    InvalidInstalledManifest {
        /// Original import specifier from source.
        specifier: String,

        /// Provider package selected by the specifier.
        provider: String,

        /// Manifest path that failed validation.
        manifest_path: PathBuf,

        /// Targeted reason for the invalid package install.
        reason: String,
    },

    /// A provider manifest points at a source root that does not exist.
    MissingInstalledSourceRoot {
        /// Original import specifier from source.
        specifier: String,

        /// Provider package selected by the specifier.
        provider: String,

        /// Source root computed from the installed top-level manifest.
        source_root: PathBuf,
    },

    /// A provider package was named, but no matching interface exists.
    UnknownModule {
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
    MissingLibraryHome {
        /// Original import specifier from source.
        specifier: String,

        /// Setup hint for the operator.
        hint: String,
    },

    /// Provider is declared in faber.toml but missing from faber.lock.
    MissingLockedPackage {
        /// Original import specifier from source.
        specifier: String,

        /// Provider package name.
        provider: String,

        /// Declared version pin, when known.
        version: Option<String>,
    },

    /// Provider-qualified import is not declared in faber.toml dependencies
    /// and is not available from the bundled library home.
    UndeclaredProvider {
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
#[derive(Debug, Clone)]
pub(crate) struct LibraryResolver {
    library_home: Option<PathBuf>,
    locked: BTreeMap<String, LockedLibraryPackage>,
    declared: BTreeMap<String, String>,
}

impl LibraryResolver {
    /// Build a resolver rooted at an explicit public library home.
    pub(crate) fn new(library_home: impl Into<PathBuf>) -> Self {
        Self {
            library_home: Some(library_home.into()),
            locked: BTreeMap::new(),
            declared: BTreeMap::new(),
        }
    }

    /// Build a resolver from `FABER_LIBRARY_HOME` or, unless disabled, the sibling dev layout.
    pub(crate) fn default() -> Self {
        Self {
            library_home: default_library_home(),
            locked: BTreeMap::new(),
            declared: BTreeMap::new(),
        }
    }

    /// Attach declared dependencies and locked package interface roots.
    pub(crate) fn with_package_lock(
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
    pub(crate) fn resolve(
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

        if provider.is_empty() {
            return Err(LibraryResolveError::InvalidProviderSpecifier {
                specifier: specifier.to_owned(),
                reason: "provider segment must not be empty".to_owned(),
            });
        }

        if !is_valid_provider_segment(provider) {
            return Err(LibraryResolveError::InvalidProviderSpecifier {
                specifier: specifier.to_owned(),
                reason: "provider segment must contain only ASCII letters, numbers, underscore, or hyphen".to_owned(),
            });
        }

        if module_path.is_empty() {
            return Err(LibraryResolveError::InvalidProviderSpecifier {
                specifier: specifier.to_owned(),
                reason: "module path segment must not be empty".to_owned(),
            });
        }

        if module_path.contains(':') {
            return Err(LibraryResolveError::InvalidProviderSpecifier {
                specifier: specifier.to_owned(),
                reason: "library specifier must contain exactly one provider separator".to_owned(),
            });
        }

        let segments = module_path.split('/').collect::<Vec<_>>();
        if !segments
            .iter()
            .all(|segment| is_valid_module_segment(segment))
        {
            return Err(LibraryResolveError::InvalidProviderSpecifier {
                specifier: specifier.to_owned(),
                reason: "module path must not contain empty, dot, or dot-dot segments".to_owned(),
            });
        }

        if let Some(locked) = self.locked.get(provider) {
            let interface_path = locked.interface_root.join(format!("{module_path}.fab"));
            if !interface_path.exists() {
                return Err(LibraryResolveError::UnknownModule {
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
                LibraryProviderKind::PackageDependency,
            )));
        }

        if self.declared.contains_key(provider) {
            return Err(LibraryResolveError::MissingLockedPackage {
                specifier: specifier.to_owned(),
                provider: provider.to_owned(),
                version: self.declared.get(provider).cloned(),
            });
        }

        let library_home = self.library_home_for(specifier)?;
        let provider_repo = library_home.join(provider);

        if !provider_repo.is_dir() {
            // Prefer an explicit dependency-declaration diagnostic when the
            // package has a [dependencies] table and this provider is not Norma
            // library-home layout.
            if !self.declared.is_empty() && provider != "norma" {
                return Err(LibraryResolveError::UndeclaredProvider {
                    specifier: specifier.to_owned(),
                    provider: provider.to_owned(),
                });
            }
            return Err(LibraryResolveError::UnknownProvider {
                specifier: specifier.to_owned(),
                provider: provider.to_owned(),
                library_home,
            });
        }

        let provider_root = installed_provider_source_root(specifier, provider, &provider_repo)?;
        let interface_path = provider_root.join(format!("{module_path}.fab"));
        if !interface_path.exists() {
            return Err(LibraryResolveError::UnknownModule {
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
            LibraryProviderKind::PackageDependency,
        )))
    }

    fn library_home_for(&self, specifier: &str) -> Result<PathBuf, LibraryResolveError> {
        self.library_home.clone().ok_or_else(|| {
            LibraryResolveError::MissingLibraryHome {
                specifier: specifier.to_owned(),
                hint: format!(
                    "set {FABER_LIBRARY_HOME_ENV} to a directory containing provider repos, such as <home>/norma/src"
                ),
            }
        })
    }
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
        return Err(LibraryResolveError::InvalidInstalledManifest {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path,
            reason: "missing top-level faber.toml".to_owned(),
        });
    }

    crate::package::discover_package(provider_repo).map_err(|diag| {
        LibraryResolveError::InvalidInstalledManifest {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path: manifest_path.clone(),
            reason: diag.message,
        }
    })?;
    let manifest = crate::package::read_manifest(&manifest_path).map_err(|diag| {
        LibraryResolveError::InvalidInstalledManifest {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path: manifest_path.clone(),
            reason: diag.message,
        }
    })?;

    if manifest.build.kind != "lib" {
        return Err(LibraryResolveError::InvalidInstalledManifest {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path,
            reason: "faber.toml build.kind must be \"lib\"".to_owned(),
        });
    }

    let Some(library) = manifest.library.as_ref() else {
        return Err(LibraryResolveError::InvalidInstalledManifest {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path,
            reason: "faber.toml [library] is required".to_owned(),
        });
    };

    if library.provider != provider {
        return Err(LibraryResolveError::InvalidInstalledManifest {
            specifier: specifier.to_owned(),
            provider: provider.to_owned(),
            manifest_path,
            reason: format!(
                "faber.toml library.provider is `{}`, expected `{provider}`",
                library.provider
            ),
        });
    }

    let source_root = provider_repo.join(&manifest.paths.source);
    if !source_root.is_dir() {
        return Err(LibraryResolveError::MissingInstalledSourceRoot {
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

pub(crate) fn is_valid_provider_segment(segment: &str) -> bool {
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

fn default_library_home() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(FABER_LIBRARY_HOME_ENV) {
        return Some(PathBuf::from(value));
    }
    if std::env::var_os(FABER_DISABLE_WORKSPACE_LIBRARY_PROBE_ENV).is_some() {
        return None;
    }

    // Public faber repo is a sibling of norma under faberlang/.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."));
    for candidate in workspace_root.ancestors() {
        let norma_src = candidate.join("norma/src");
        if norma_src.is_dir() && norma_src.join("solum.fab").is_file() {
            return Some(candidate.to_path_buf());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct EnvRestore {
        home: Option<std::ffi::OsString>,
        disable: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn capture() -> Self {
            Self {
                home: std::env::var_os(FABER_LIBRARY_HOME_ENV),
                disable: std::env::var_os(FABER_DISABLE_WORKSPACE_LIBRARY_PROBE_ENV),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            restore_env(FABER_LIBRARY_HOME_ENV, self.home.take());
            restore_env(
                FABER_DISABLE_WORKSPACE_LIBRARY_PROBE_ENV,
                self.disable.take(),
            );
        }
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn workspace_library_probe_can_be_disabled_for_store_only_resolution() {
        let _guard = env_guard();
        let _env = EnvRestore::capture();
        std::env::remove_var(FABER_LIBRARY_HOME_ENV);
        std::env::set_var(FABER_DISABLE_WORKSPACE_LIBRARY_PROBE_ENV, "1");

        assert_eq!(default_library_home(), None);
    }

    #[test]
    fn explicit_library_home_wins_over_probe_disable() {
        let _guard = env_guard();
        let _env = EnvRestore::capture();
        let explicit = PathBuf::from("/tmp/faber-explicit-library-home-test");
        std::env::set_var(FABER_LIBRARY_HOME_ENV, &explicit);
        std::env::set_var(FABER_DISABLE_WORKSPACE_LIBRARY_PROBE_ENV, "1");

        assert_eq!(default_library_home(), Some(explicit));
    }
}
