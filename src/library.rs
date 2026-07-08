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

use std::path::{Path, PathBuf};

pub(crate) const FABER_LIBRARY_HOME_ENV: &str = "FABER_LIBRARY_HOME";

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
}

/// Resolver for built-in and package-backed Faber library imports.
///
/// `resolve` returns `Ok(None)` when the specifier is not provider-shaped. That
/// lets package loading fall through to local import resolution without
/// treating every plain package path as a library error.
#[derive(Debug, Clone)]
pub(crate) struct LibraryResolver {
    library_home: Option<PathBuf>,
}

impl LibraryResolver {
    /// Build a resolver rooted at an explicit public library home.
    pub(crate) fn new(library_home: impl Into<PathBuf>) -> Self {
        Self {
            library_home: Some(library_home.into()),
        }
    }

    /// Build a resolver from `FABER_LIBRARY_HOME` or the sibling dev layout.
    pub(crate) fn default() -> Self {
        Self {
            library_home: default_library_home(),
        }
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

        let library_home = self.library_home_for(specifier)?;
        let provider_root = library_home.join(provider).join("src");

        if !provider_root.is_dir() {
            return Err(LibraryResolveError::UnknownProvider {
                specifier: specifier.to_owned(),
                provider: provider.to_owned(),
                library_home,
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

fn is_valid_module_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn is_valid_provider_segment(segment: &str) -> bool {
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
