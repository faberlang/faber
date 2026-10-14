//! Resolve Faber import specifiers to filesystem paths for single-file check/emit.
//!
//! Supports relative paths, `norma:*`, and generic `provider:module` imports.
//! The configured library home (`--library-home` / `[paths] library_home`,
//! via `set_configured_library_home`) supplies `<home>/<provider>/` (with
//! optional top-level `faber.toml` `[`paths`].source`), falling back to the
//! in-tree `<repo>/lib/<provider>/`.

use rustc_hash::FxHashMap;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::driver::Session;

/// Process-wide configured library home: the CLI flag or
/// `[paths].library_home` seam ([`set_configured_library_home`]).
///
/// The test-only thread-local override is checked before this setting, so
/// test isolation is preserved.
static CONFIGURED_LIBRARY_HOME: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Set the process-wide configured library home (`--library-home` /
/// `[paths].library_home`).
pub fn set_configured_library_home(home: Option<PathBuf>) {
    if let Ok(mut setting) = CONFIGURED_LIBRARY_HOME.lock() {
        *setting = home;
    }
}

/// Read the process-wide configured library home (cloned).
#[must_use]
pub fn configured_library_home_setting() -> Option<PathBuf> {
    CONFIGURED_LIBRARY_HOME
        .lock()
        .ok()
        .and_then(|setting| setting.clone())
}

/// Process-wide memo: library home → provider segment → provider repo dir.
///
/// Filled on the first miss of the directory-name convention
/// (`<home>/<provider>`, e.g. `tela:tela` → `home/tela`): the home is
/// scanned once for sibling packages whose `faber.toml` declares
/// `[library] provider = "<segment>"` (e.g. the `faber-web` checkout, which
/// provides the `web` segment from a directory named `faber-web`). The home
/// layout is immutable during a compilation session, so a per-home memo is
/// safe; thread-local test overrides get fresh temp homes and their own memo
/// entries, keyed by the resolved home path.
type ProviderNameMap = FxHashMap<PathBuf, FxHashMap<String, Option<PathBuf>>>;

static PROVIDER_NAME_MEMO: OnceLock<Mutex<ProviderNameMap>> = OnceLock::new();

fn provider_name_memo() -> &'static Mutex<ProviderNameMap> {
    PROVIDER_NAME_MEMO.get_or_init(|| Mutex::new(FxHashMap::default()))
}

/// Clear the provider-name memo (tests only).
#[cfg(test)]
pub(crate) fn clear_provider_name_memo() {
    if let Ok(mut memo) = provider_name_memo().lock() {
        memo.clear();
    }
}

/// Read the optional `[paths].source` root from a TOML manifest.
#[must_use]
fn paths_source_from_manifest(toml_source: &str) -> Option<String> {
    toml::from_str::<toml::Value>(toml_source)
        .ok()?
        .get("paths")?
        .get("source")?
        .as_str()
        .filter(|source| !source.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
pub(crate) use tests::library_home_test_support;

/// Structured resolution failure with a human-readable reason and a stable
/// issue slug so callers can produce targeted diagnostics (F-108).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportResolveError {
    /// Human-readable explanation of why resolution failed.
    pub message: String,
    /// Stable issue identifier for diagnostic routing.
    pub issue: &'static str,
}

impl fmt::Display for ImportResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.issue)
    }
}

/// Resolve an import specifier relative to `from_file`.
///
/// Supports relative paths, `norma:*`, generic `provider:module` imports, and
/// `§name/rest` template paths (when `templates` is `Some`). Template paths are
/// resolved relative to the package root (the parent-most ancestor that contains
/// a `faber.toml`).
///
/// # Errors
///
/// Returns [`ImportResolveError`] when the import path cannot be resolved due
/// to missing infrastructure (library home not configured, template not found,
/// provider directory missing) or when the target file does not exist.
pub fn resolve_namespace_import(
    session: &Session,
    from_file: &Path,
    import_path: &str,
    templates: Option<&BTreeMap<String, String>>,
) -> Result<PathBuf, ImportResolveError> {
    // §name/rest template resolution.
    if let Some(rest) = import_path.strip_prefix('§') {
        return resolve_template_import(session, from_file, rest, templates).ok_or_else(|| ImportResolveError {
            message: format!("template import '§{rest}' could not be resolved"),
            issue: "import_resolve_template_not_found",
        });
    }

    if let Some(module_path) = import_path.strip_prefix("norma:") {
        if let Some(path) = resolve_library_provider_import("norma", module_path) {
            return Ok(path);
        }
        return Err(ImportResolveError {
            message: format!("norma import 'norma:{module_path}' not found"),
            issue: "import_resolve_norma_not_found",
        });
    }

    if let Some((provider, module_path)) = import_path.split_once(':')
        && !provider.is_empty()
        && !module_path.is_empty()
        && !module_path.contains(':')
        && is_valid_provider_segment(provider)
    {
        if let Some(path) = resolve_library_provider_import(provider, module_path) {
            return Ok(path);
        }
        let home = library_home().map_or_else(|| "unset".to_owned(), |h| h.display().to_string());
        return Err(ImportResolveError {
            message: format!("provider import '{import_path}' could not be resolved (library home: {home})"),
            issue: "import_resolve_provider_not_found",
        });
    }

    if !import_path.starts_with('.') {
        return Err(ImportResolveError {
            message: format!("import path '{import_path}' is not a recognized import form"),
            issue: "import_resolve_unknown_form",
        });
    }

    let Some(base) = from_file.parent() else {
        return Err(ImportResolveError {
            message: format!("no parent directory for '{}'", from_file.display()),
            issue: "import_resolve_no_parent",
        });
    };
    let candidate = base.join(import_path);
    let Some(target) = resolve_fab_file(&candidate) else {
        return Err(ImportResolveError {
            message: format!("relative import '{import_path}' not found from '{}'", from_file.display()),
            issue: "import_resolve_file_not_found",
        });
    };
    contain_relative_import(session, from_file, import_path, target)
}

/// Contain a resolved relative import under the resolving package's canonical
/// root.
///
/// Containment is decided on the canonicalized target, never on the spelling:
/// an in-package `../` import resolves, while a `..` escape or a symlink
/// pointing outside the root is rejected before the target is read.
/// Manifestless loose files use their own directory as the root, mirroring
/// package discovery for direct-file input; a file with no directory has no
/// boundary to enforce.
fn contain_relative_import(
    session: &Session,
    from_file: &Path,
    import_path: &str,
    target: PathBuf,
) -> Result<PathBuf, ImportResolveError> {
    let root = find_package_root_with_session(session, from_file).or_else(|| {
        from_file.parent().map(|parent| {
            if parent.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                parent.to_path_buf()
            }
        })
    });
    let Some(root) = root else {
        return Ok(target);
    };
    let escape_error = |reason: String| ImportResolveError {
        message: format!("relative import '{import_path}' {reason}"),
        issue: "import_resolve_escape",
    };
    let canonical_root = root
        .canonicalize()
        .map_err(|error| escape_error(format!("package root {} cannot be resolved: {error}", root.display())))?;
    let canonical_target = target
        .canonicalize()
        .map_err(|error| escape_error(format!("target cannot be resolved: {error}")))?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(escape_error(format!(
            "resolves outside the package root: {}",
            canonical_target.display()
        )));
    }
    Ok(target)
}

/// Resolve a `§name/rest` template import.
///
/// 1. Extract `name` and `rest` by splitting at the first `/`.
/// 2. Look up `name` in the templates map.
/// 3. Join the template value with `rest`.
/// 4. Resolve the joined path relative to the package root (nearest ancestor
///    directory containing `faber.toml`).
/// 5. Call `resolve_fab_file` on the candidate.
fn resolve_template_import(
    session: &Session,
    from_file: &Path,
    spec: &str,
    templates: Option<&BTreeMap<String, String>>,
) -> Option<PathBuf> {
    let templates = templates?;

    let slash_pos = spec.find('/')?;
    let name = &spec[..slash_pos];
    let rest = &spec[slash_pos + 1..];

    if name.is_empty() || rest.is_empty() {
        return None;
    }

    // Reject rest segments that attempt path traversal (F-046).
    if contains_path_traversal(rest) {
        return None;
    }

    let template_value = templates.get(name)?;
    let package_root = find_package_root_with_session(session, from_file)?;

    let candidate = package_root.join(template_value).join(rest);
    resolve_fab_file(&candidate)
}

/// Returns `true` if `path` contains any `..` segment that would escape
/// an intended directory scope.
///
/// Used by the `rest` portion of both `§name/rest` template resolvers (CLI
/// and package loader; F-046 / S10-1).
#[must_use]
pub fn contains_path_traversal(path: &str) -> bool {
    path.starts_with('/') || path == ".." || path.starts_with("../") || path.contains("/../") || path.ends_with("/..")
}

/// Resolve a package-form import (`math`, `lib/util` — bare source-root
/// spelling) against the importing file's package source root.
///
/// The CLI-path mirror of the package machinery's source-root form: the
/// candidate is resolved from the discovered package root's `[paths].source`
/// (`src` by default, `.` for package-root sources; other source roots are
/// not part of the supported set and fail closed). Returns the source root
/// used and the resolved target file, so callers can re-derive the same
/// resolution through the relative-import spelling.
pub(crate) fn resolve_source_root_import(
    session: &Session,
    from_file: &Path,
    import_path: &str,
) -> Option<(PathBuf, PathBuf)> {
    if contains_path_traversal(import_path) {
        return None;
    }
    let package_root = find_package_root_with_session(session, from_file)?;
    let manifest = std::fs::read_to_string(package_root.join("faber.toml")).ok()?;
    let base = match paths_source_from_manifest(&manifest).as_deref() {
        None | Some("src") => package_root.join("src"),
        Some(".") => package_root.clone(),
        Some(_) => return None,
    };
    let resolved = resolve_fab_file(&base.join(import_path))?;
    Some((base, resolved))
}

/// Walk up from `from_file.parent()` until a `faber.toml` is found.
///
/// Results are memoized on the driver-run [`Session`] (ARCH-U3 / S01-9): first
/// resolution for a directory pays the ancestor walk; later calls from the
/// same package return the cached root without additional filesystem probes.
/// Same-package when both files resolve to the same `faber.toml` root.
///
/// Program/discovery helper. Module analysis must not call this; it consumes
/// caller-supplied [`crate::semantic::PassConfig::namespace_seams`] only.
#[must_use]
pub(crate) fn same_package(session: &Session, from_file: &Path, target_file: &Path) -> bool {
    let from = absolute_path(from_file);
    let target = absolute_path(target_file);
    match (
        find_package_root_with_session(session, &from),
        find_package_root_with_session(session, &target),
    ) {
        (Some(from_root), Some(target_root)) => from_root == target_root,
        _ => false,
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    }
}

/// Walk up from `from_file.parent()` until a `faber.toml` is found (PRP-066 /
/// BO-D-1).
///
/// Session-less discovery form: performs the ancestor walk without a memo,
/// because the caller has no driver run to trust (package-graph discovery,
/// product harnesses). Compile-path callers — which own a [`Session`] — use
/// the memoized [`find_package_root_with_session`] so one driver run pays the
/// walk once per directory.
#[must_use]
pub fn find_package_root(from_file: &Path) -> Option<PathBuf> {
    let start = from_file.parent()?.to_path_buf();
    walk_up_to_manifest(&start)
}

/// Session-memoized form of [`find_package_root`] for the compile path
/// (ARCH-U3 / S01-9).
///
/// Every intermediate directory visited during a successful (or failed) walk
/// is filled into the session's package-root cell so later imports from the
/// same package pay O(1) instead of O(depth) `faber.toml` probes, and the
/// memo dies with the driver run that filled it.
#[must_use]
pub fn find_package_root_with_session(session: &Session, from_file: &Path) -> Option<PathBuf> {
    let start = from_file.parent()?.to_path_buf();
    let mut cache = session.memos.package_roots.borrow_mut();
    if let Some(hit) = cache.get(&start) {
        return hit.clone();
    }

    let mut walked = Vec::new();
    let mut candidate = start;
    let result = loop {
        if let Some(hit) = cache.get(&candidate) {
            break hit.clone();
        }
        walked.push(candidate.clone());
        if candidate.join("faber.toml").is_file() {
            break Some(candidate);
        }
        if !candidate.pop() {
            break None;
        }
    };
    for dir in walked {
        cache.insert(dir, result.clone());
    }
    result
}

/// One ancestor walk to the nearest `faber.toml`, shared by both
/// [`find_package_root`] entry forms.
fn walk_up_to_manifest(start: &Path) -> Option<PathBuf> {
    let mut candidate = start.to_path_buf();
    loop {
        if candidate.join("faber.toml").is_file() {
            return Some(candidate);
        }
        if !candidate.pop() {
            return None;
        }
    }
}

fn is_valid_provider_segment(provider: &str) -> bool {
    !provider.is_empty()
        && provider
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn resolve_library_provider_import(provider: &str, module_path: &str) -> Option<PathBuf> {
    let provider_repo = library_homes()
        .iter()
        .find_map(|home| provider_repo_for_segment(home, provider))?;
    let source_root = installed_provider_source_root(&provider_repo)?;
    let candidate = source_root.join(module_path);
    resolve_fab_file(&candidate)
}

/// Locate the provider repo directory for an import segment.
///
/// Directory-name convention first (`<home>/<segment>`). When the convention
/// misses, fall back to a manifest scan: a sibling package whose `faber.toml`
/// declares `[library] provider = "<segment>"` (e.g. `web:dom` → the
/// `faber-web` checkout). The scan result is memoized per (home, segment).
fn provider_repo_for_segment(home: &Path, provider: &str) -> Option<PathBuf> {
    let direct = home.join(provider);
    if direct.is_dir() {
        return Some(direct);
    }
    let Ok(mut memo) = provider_name_memo().lock() else {
        return None;
    };
    let by_segment = memo.entry(home.to_path_buf()).or_default();
    if let Some(cached) = by_segment.get(provider) {
        return cached.clone();
    }
    let found = scan_home_for_provider(home, provider);
    by_segment.insert(provider.to_owned(), found.clone());
    found
}

/// Scan a library home for a sibling package manifest that declares the
/// provider segment in `[library] provider`.
fn scan_home_for_provider(home: &Path, provider: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(home).ok()?.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest_path = dir.join("faber.toml");
        let Ok(manifest) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        if provider_name_from_manifest(&manifest).as_deref() == Some(provider) {
            return Some(dir);
        }
    }
    None
}

/// Parse the `[library] provider` value from a `faber.toml` manifest source.
///
/// The provider segment is the namespace other packages import this library
/// under (e.g. faber-web's manifest declares `provider = "web"` while the
/// checkout directory is `faber-web`). Returns `None` when the manifest has
/// no `[library]` table or no `provider` key.
#[must_use]
fn provider_name_from_manifest(toml_source: &str) -> Option<String> {
    toml::from_str::<toml::Value>(toml_source)
        .ok()?
        .get("library")?
        .get("provider")?
        .as_str()
        .filter(|provider| !provider.is_empty())
        .map(str::to_owned)
}

/// In-tree library home: `<repo>/lib`, located from this crate's manifest
/// directory (`crates/radix-module` → `../../lib`). It holds `norma/`.
#[must_use]
pub fn in_tree_library_home() -> Option<PathBuf> {
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib");
    home.is_dir().then_some(home)
}

/// The configured library home: the process-wide setting
/// ([`set_configured_library_home`]) or, in tests, the thread-local override.
/// `None` when nothing is configured.
fn configured_library_home() -> Option<PathBuf> {
    // Thread-local override (test-only): keeps parallel vertex-source tests
    // from picking up a transient temp path.
    #[cfg(test)]
    {
        if let Some(ref home) = library_home_test_support::get_thread_library_home()
            && home.is_dir()
        {
            return Some(home.clone());
        }
    }
    // Process-wide setting (CLI `--library-home` / `[paths].library_home`).
    configured_library_home_setting().filter(|home| home.is_dir())
}

/// Library homes to search, in order: the configured home first, then the
/// in-tree `<repo>/lib`. A provider resolves from the first home that holds it.
fn library_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(configured) = configured_library_home() {
        homes.push(configured);
    }
    if let Some(in_tree) = in_tree_library_home()
        && !homes.contains(&in_tree)
    {
        homes.push(in_tree);
    }
    homes
}

/// The primary library home: the configured home when set, else the in-tree
/// `<repo>/lib`.
#[must_use]
pub fn library_home() -> Option<PathBuf> {
    library_homes().into_iter().next()
}

fn installed_provider_source_root(provider_repo: &Path) -> Option<PathBuf> {
    let manifest_path = provider_repo.join("faber.toml");
    if manifest_path.is_file()
        && let Ok(toml_str) = std::fs::read_to_string(&manifest_path)
    {
        let config = paths_source_from_manifest(&toml_str);
        if let Some(source_rel) = &config {
            let root = provider_repo.join(source_rel);
            return root.is_dir().then_some(root);
        }
    }
    // faber.toml exists but has no [paths].source — fall through to defaults.
    let fallback = provider_repo.join("src");
    if fallback.is_dir() {
        Some(fallback)
    } else if provider_repo.is_dir() {
        Some(provider_repo.to_path_buf())
    } else {
        None
    }
}

fn resolve_fab_file(path: &Path) -> Option<PathBuf> {
    if path.extension().and_then(|ext| ext.to_str()) == Some("fab") && path.is_file() {
        return Some(path.to_path_buf());
    }

    let with_extension = path.with_extension("fab");
    if with_extension.is_file() {
        return Some(with_extension);
    }

    let init_file = path.join("mod.fab");
    if init_file.is_file() {
        return Some(init_file);
    }

    None
}

#[cfg(test)]
#[path = "import_resolve_test.rs"]
mod tests;
