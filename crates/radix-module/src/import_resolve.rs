//! Resolve Faber import specifiers to filesystem paths for single-file check/emit.
//!
//! Supports relative paths, `norma:*`, and generic `provider:module` imports via
//! `$FABER_LIBRARY_HOME/<provider>/` (with optional top-level `faber.toml`
//! `[`paths`].source`).

use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

thread_local! {
    /// Session-scoped memo for [`find_package_root`] (PRP-066 / BO-D-1).
    ///
    /// Keyed by the directory that started the walk. Every intermediate directory
    /// visited during a successful (or failed) walk is filled so later imports
    /// from the same package pay O(1) instead of O(depth) `faber.toml` probes.
    static PACKAGE_ROOT_CACHE: RefCell<FxHashMap<PathBuf, Option<PathBuf>>> =
        RefCell::new(FxHashMap::default());
}

/// Process-wide memo for [`library_home`]'s production path (BO-D-1).
///
/// The env var and the ancestor walk cannot change during a compilation
/// session, so the first provider import pays the walk once and every later
/// call returns the cached value in O(1). The test-only thread-local override
/// is checked before this cache, so test isolation is preserved.
static LIBRARY_HOME_CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();

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
/// Per-library-home provider-segment → provider repo directory memo (tests
/// use fresh temp homes and their own entries).
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

/// Clear the package-root memo (tests only).
#[cfg(test)]
pub(crate) fn clear_package_root_cache() {
    PACKAGE_ROOT_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// Environment variable for installed provider roots (`<home>/<provider>/…`).
pub const FABER_LIBRARY_HOME_ENV: &str = "FABER_LIBRARY_HOME";

/// Structured config parsed from a `faber.toml` manifest's `[paths]` section.
///
/// Holds the optional `source` root and a map of template names (without the
/// `§` glyph) to their resolved relative paths.
#[derive(Debug, Clone)]
pub struct PathsConfig {
    /// Value of `[paths].source`, if present.
    pub source: Option<String>,
    /// Map of template name → relative path from `[paths.templates]`, plus the
    /// implicit `§src` default when `[paths].source` is set.
    #[allow(dead_code)]
    pub templates: BTreeMap<String, String>,
}

/// Parse a TOML manifest source string into a [`PathsConfig`].
///
/// # Defaults
///
/// When `[paths].source` is present, an implicit template entry `"src" →
/// <source>` is added to the templates map unless `"src"` is already explicitly
/// defined in `[paths.templates]`.
///
/// # Template validation
///
/// Template values that start with `/` (absolute) or contain `..` segments
/// are silently rejected — they are not added to the returned templates map.
#[must_use]
pub fn parse_paths_config(toml_source: &str) -> PathsConfig {
    let mut source: Option<String> = None;
    let mut templates: BTreeMap<String, String> = BTreeMap::new();
    let mut in_paths = false;
    let mut in_templates = false;

    for line in toml_source.lines() {
        let trimmed = line.trim();

        // Detect section headers.
        if trimmed.starts_with('[') {
            in_paths = trimmed == "[paths]";
            in_templates = trimmed == "[paths.templates]";
            continue;
        }

        if in_paths && !in_templates {
            // Parse [paths] for `source = "value"`.
            if let Some(rest) = trimmed.strip_prefix("source") {
                let rest = rest.trim_start();
                if let Some(value) = rest.strip_prefix('=') {
                    let value = value.trim().trim_matches('"').trim_matches('\'');
                    if !value.is_empty() {
                        source = Some(value.to_owned());
                    }
                }
            }
        }

        if in_templates {
            // Parse [paths.templates] for `name = "value"`.
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim().to_owned();
                let value_raw = trimmed[eq_pos + 1..].trim();
                let value = value_raw.trim_matches('"').trim_matches('\'');
                if !key.is_empty() && !value.is_empty() {
                    // Reject absolute template values or those with `..` segments.
                    if value.starts_with('/')
                        || value == ".."
                        || value.starts_with("../")
                        || value.contains("/../")
                        || value.ends_with("/..")
                    {
                        continue;
                    }
                    templates.insert(key, value.to_owned());
                }
            }
        }
    }

    // Default: when [paths].source is present, implicitly add §src → source
    // to the templates map, unless `src` was already explicitly defined.
    if let Some(ref src) = source {
        templates
            .entry("src".to_owned())
            .or_insert_with(|| src.clone());
    }

    PathsConfig { source, templates }
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
    from_file: &Path,
    import_path: &str,
    templates: Option<&BTreeMap<String, String>>,
) -> Result<PathBuf, ImportResolveError> {
    // §name/rest template resolution.
    if let Some(rest) = import_path.strip_prefix('§') {
        return resolve_template_import(from_file, rest, templates).ok_or_else(|| ImportResolveError {
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
    resolve_fab_file(&candidate).ok_or_else(|| ImportResolveError {
        message: format!("relative import '{import_path}' not found from '{}'", from_file.display()),
        issue: "import_resolve_file_not_found",
    })
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
    let package_root = find_package_root(from_file)?;

    let candidate = package_root.join(template_value).join(rest);
    resolve_fab_file(&candidate)
}

/// Returns `true` if `path` contains any `..` segment that would escape
/// an intended directory scope.
///
/// This mirrors the template-value sanitization in [`parse_paths_config`],
/// applied to the `rest` portion of a `§name/rest` template import.
fn contains_path_traversal(path: &str) -> bool {
    path.starts_with('/') || path == ".." || path.starts_with("../") || path.contains("/../") || path.ends_with("/..")
}

/// Walk up from `from_file.parent()` until a `faber.toml` is found.
///
/// Results are memoized for the thread (compile session). First resolution for
/// a directory pays the ancestor walk; later calls from the same package return
/// the cached root without additional filesystem probes.
/// Same-package when both files resolve to the same `faber.toml` root.
///
/// Program/discovery helper. Module analysis must not call this; it consumes
/// caller-supplied [`crate::semantic::PassConfig::namespace_seams`] only.
#[must_use]
pub(crate) fn same_package(from_file: &Path, target_file: &Path) -> bool {
    let from = absolute_path(from_file);
    let target = absolute_path(target_file);
    match (find_package_root(&from), find_package_root(&target)) {
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

#[must_use]
pub fn find_package_root(from_file: &Path) -> Option<PathBuf> {
    let start = from_file.parent()?.to_path_buf();
    PACKAGE_ROOT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
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
    })
}

fn is_valid_provider_segment(provider: &str) -> bool {
    !provider.is_empty()
        && provider
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn resolve_library_provider_import(provider: &str, module_path: &str) -> Option<PathBuf> {
    let home = library_home()?;
    let provider_repo = provider_repo_for_segment(&home, provider)?;
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
        if parse_manifest_provider_name(&manifest).as_deref() == Some(provider) {
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
fn parse_manifest_provider_name(toml_source: &str) -> Option<String> {
    let mut in_library = false;
    for line in toml_source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_library = trimmed == "[library]";
            continue;
        }
        if in_library && let Some(rest) = trimmed.strip_prefix("provider") {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix('=') {
                let value = value.trim().trim_matches('"').trim_matches('\'');
                if !value.is_empty() {
                    return Some(value.to_owned());
                }
            }
        }
    }
    None
}

pub fn library_home() -> Option<PathBuf> {
    // Thread-local override (test-only): isolate compile_test env-var mutations
    // so parallel vertex-source tests don't pick up a transient temp path.
    #[cfg(test)]
    {
        if let Some(ref home) = library_home_test_support::get_thread_library_home()
            && home.is_dir()
        {
            return Some(home.clone());
        }
    }
    // Production path: env-var check + ancestor walk, memoized per process.
    // Neither the env var nor the filesystem layout changes during a
    // compilation session, so the walk runs at most once.
    LIBRARY_HOME_CACHE
        .get_or_init(|| {
            if let Some(home) = std::env::var_os(FABER_LIBRARY_HOME_ENV).map(PathBuf::from)
                && home.is_dir()
            {
                return Some(home);
            }
            // Walk out of both normal checkouts and packet worktrees. A packet's
            // compile-time manifest lives below `faberlang/worktrees/<packet>/radix`,
            // so the old two-level probe stopped before the sibling `norma` checkout.
            // Stop only on `norma/`. A nearer `triga/` (packet member or leftover
            // sibling checkout) is not a library home unless Norma is there too.
            // Never accept the container `worktrees/` root itself: it "has"
            // norma only via a stale lane checkout that would shadow the real
            // stdlib for every packet below it.
            let mut candidate = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
            loop {
                let under_worktrees = candidate
                    .components()
                    .any(|c| c.as_os_str() == std::ffi::OsStr::new("worktrees"));
                if !under_worktrees && candidate.join("norma").is_dir() {
                    return Some(candidate);
                }
                if !candidate.pop() {
                    break;
                }
            }
            None
        })
        .clone()
}

fn installed_provider_source_root(provider_repo: &Path) -> Option<PathBuf> {
    let manifest_path = provider_repo.join("faber.toml");
    if manifest_path.is_file()
        && let Ok(toml_str) = std::fs::read_to_string(&manifest_path)
    {
        let config = parse_paths_config(&toml_str);
        if let Some(source_rel) = &config.source {
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
