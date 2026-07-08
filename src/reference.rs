//! Disk-backed Faber reference pack loader for `faber explain`.

use crate::explain::{Entry, ExplainError, Registry};
use crate::reference_parse::read_exempla_file;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use crate::reference_parse::{entry_from_exempla, legacy_entry_from_redirect};

/// Environment variable that overrides reference pack discovery.
pub const REFERENCE_ROOT_ENV: &str = "FABER_REFERENCE_ROOT";

/// Installed pack layout (`exempla/` subtree) vs repo dev tree (`crates/exempla/corpus/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceLayout {
    /// Release sidecar: `<root>/exempla/**/*.fab`.
    Pack,
    /// Repository tree: `<root>/**/*.fab` with `index.toml` at root.
    Repo,
}

/// Metadata from `PACK.toml` when present; otherwise synthesized from `index.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackMetadata {
    pub faber_version: Option<String>,
    pub generated_on: Option<String>,
    pub fab_count: u32,
    pub registry_terms: u32,
    pub source_commit: Option<String>,
    pub index_generated_on: Option<String>,
}

/// One canonical row from `index.toml` `[[terms]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermRecord {
    pub term: String,
    pub phase: u32,
    pub action: String,
    pub rule: String,
    pub primary: String,
    pub paths: Vec<String>,
    pub tags: Vec<String>,
}

/// One legacy redirect from `legacy-redirects.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRedirect {
    pub term: String,
    pub canonical: String,
    pub message: String,
}

/// Resolved lookup against indexed pack metadata (before exempla parsing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTerm<'a> {
    Canonical(&'a TermRecord),
    Legacy {
        redirect: &'a LegacyRedirect,
        canonical: &'a TermRecord,
    },
}

/// Parsed and indexed reference pack on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferencePack {
    root: PathBuf,
    layout: ReferenceLayout,
    metadata: PackMetadata,
    terms: Vec<TermRecord>,
    term_index: BTreeMap<String, usize>,
    legacy: Vec<LegacyRedirect>,
    legacy_index: BTreeMap<String, usize>,
    /// Non-fatal pack/binary version skew reported on first explain load.
    version_warning: Option<String>,
}

/// Failure loading or validating a reference pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceError {
    pub message: String,
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ReferenceError {}

impl ReferenceError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<ReferenceError> for ExplainError {
    fn from(error: ReferenceError) -> Self {
        ExplainError::new(error.message)
    }
}

#[derive(Debug, Deserialize)]
struct IndexFile {
    #[serde(default)]
    generated_on: Option<String>,
    fab_count: u32,
    registry_terms: u32,
    terms: Vec<IndexTerm>,
}

#[derive(Debug, Deserialize)]
struct IndexTerm {
    term: String,
    phase: u32,
    action: String,
    rule: String,
    primary: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PackFile {
    faber_version: Option<String>,
    generated_on: Option<String>,
    fab_count: u32,
    registry_terms: u32,
    source_commit: Option<String>,
    index_generated_on: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegacyRedirectsFile {
    redirects: Vec<LegacyRedirectRow>,
}

#[derive(Debug, Deserialize)]
struct LegacyRedirectRow {
    term: String,
    canonical: String,
    message: String,
}

impl ReferencePack {
    /// Resolve the reference root, then load indexes from disk.
    pub fn load() -> Result<Self, ReferenceError> {
        let root = resolve_reference_root()?;
        Self::load_from(&root)
    }

    /// Load indexes from an explicit reference root (env override and tests).
    pub fn load_from(root: impl AsRef<Path>) -> Result<Self, ReferenceError> {
        let root = root
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| root.as_ref().to_path_buf());
        if !root.join("index.toml").is_file() {
            return Err(ReferenceError::new(format!(
                "reference root {} is missing index.toml",
                root.display()
            )));
        }

        let layout = detect_layout(&root);
        let index = read_index(&root)?;
        let metadata = read_metadata(&root, &index)?;
        let version_warning = pack_version_skew(&metadata)?;
        let terms = index
            .terms
            .into_iter()
            .map(|row| TermRecord {
                term: row.term,
                phase: row.phase,
                action: row.action,
                rule: row.rule,
                primary: row.primary,
                paths: if row.paths.is_empty() {
                    Vec::new()
                } else {
                    row.paths
                },
                tags: row.tags,
            })
            .collect::<Vec<_>>();

        if terms.len() as u32 != index.registry_terms {
            return Err(ReferenceError::new(format!(
                "index.toml registry_terms={} but [[terms]] has {} rows",
                index.registry_terms,
                terms.len()
            )));
        }

        let mut term_index = BTreeMap::new();
        for (index, term) in terms.iter().enumerate() {
            if term_index.insert(term.term.clone(), index).is_some() {
                return Err(ReferenceError::new(format!(
                    "duplicate term {:?} in index.toml",
                    term.term
                )));
            }
        }

        let legacy = read_legacy_redirects(&root)?;
        let mut legacy_index = BTreeMap::new();
        for (index, redirect) in legacy.iter().enumerate() {
            if legacy_index.insert(redirect.term.clone(), index).is_some() {
                return Err(ReferenceError::new(format!(
                    "duplicate legacy redirect {:?}",
                    redirect.term
                )));
            }
        }

        let pack = Self {
            root,
            layout,
            metadata,
            terms,
            term_index,
            legacy,
            legacy_index,
            version_warning,
        };
        pack.validate_paths()?;
        pack.validate_legacy_targets()?;
        Ok(pack)
    }

    pub fn version_warning(&self) -> Option<&str> {
        self.version_warning.as_deref()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn layout(&self) -> ReferenceLayout {
        self.layout
    }

    pub fn metadata(&self) -> &PackMetadata {
        &self.metadata
    }

    pub fn terms(&self) -> &[TermRecord] {
        &self.terms
    }

    pub fn term_count(&self) -> usize {
        self.terms.len()
    }

    pub fn legacy_redirects(&self) -> &[LegacyRedirect] {
        &self.legacy
    }

    pub fn term(&self, name: &str) -> Option<&TermRecord> {
        self.term_index.get(name).map(|index| &self.terms[*index])
    }

    pub fn legacy_redirect(&self, name: &str) -> Option<&LegacyRedirect> {
        self.legacy_index
            .get(name)
            .map(|index| &self.legacy[*index])
    }

    /// Resolve a canonical term or legacy spelling to indexed metadata.
    pub fn resolve_term(&self, query: &str) -> Option<ResolvedTerm<'_>> {
        if let Some(record) = self.term(query) {
            return Some(ResolvedTerm::Canonical(record));
        }

        let redirect = self.legacy_redirect(query)?;
        let canonical = self.term(&redirect.canonical)?;
        Some(ResolvedTerm::Legacy {
            redirect,
            canonical,
        })
    }

    /// Map an index-relative exempla path to an on-disk `.fab` file.
    pub fn resolve_exempla_path(&self, relative: &str) -> PathBuf {
        self.exempla_base().join(relative)
    }

    fn exempla_base(&self) -> PathBuf {
        match self.layout {
            ReferenceLayout::Pack => self.root.join("exempla"),
            ReferenceLayout::Repo => self.root.clone(),
        }
    }

    fn validate_paths(&self) -> Result<(), ReferenceError> {
        let base = self.exempla_base();
        for term in &self.terms {
            if !base.join(&term.primary).is_file() {
                return Err(ReferenceError::new(format!(
                    "term {:?}: missing primary exempla file {}",
                    term.term,
                    base.join(&term.primary).display()
                )));
            }
            for path in &term.paths {
                if !base.join(path).is_file() {
                    return Err(ReferenceError::new(format!(
                        "term {:?}: missing exempla file {}",
                        term.term,
                        base.join(path).display()
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_legacy_targets(&self) -> Result<(), ReferenceError> {
        for redirect in &self.legacy {
            if self.term(&redirect.canonical).is_none() {
                return Err(ReferenceError::new(format!(
                    "legacy redirect {:?} points to missing canonical term {:?}",
                    redirect.term, redirect.canonical
                )));
            }
        }
        Ok(())
    }

    /// Parse exempla entries and build an explain [`Registry`].
    pub fn build_registry(&self) -> Result<Registry, ExplainError> {
        let mut entries = Vec::new();

        for term_record in &self.terms {
            let path = self.resolve_exempla_path(&term_record.primary);
            let source = read_exempla_file(&path)?;
            let entry = entry_from_exempla(
                &path.display().to_string(),
                &source,
                &term_record.term,
                &term_record.rule,
            )?;
            entries.push(entry);
        }

        let canonical_by_term: BTreeMap<String, Entry> = entries
            .iter()
            .map(|entry| (entry.term.clone(), entry.clone()))
            .collect();

        for redirect in &self.legacy {
            let canonical = canonical_by_term.get(&redirect.canonical).ok_or_else(|| {
                ExplainError::new(format!(
                    "legacy redirect {:?} points to missing canonical term {:?}",
                    redirect.term, redirect.canonical
                ))
            })?;
            entries.push(legacy_entry_from_redirect(
                &redirect.term,
                &redirect.canonical,
                &redirect.message,
                canonical,
            )?);
        }

        Registry::from_entries(
            entries,
            Some(self.root.clone()),
            self.metadata.faber_version.clone(),
        )
    }
}

/// Resolve the reference pack root using env, install layout, then repo fallback.
pub fn resolve_reference_root() -> Result<PathBuf, ReferenceError> {
    if let Ok(path) = std::env::var(REFERENCE_ROOT_ENV) {
        let root = PathBuf::from(path);
        if root.join("index.toml").is_file() {
            return Ok(root);
        }
        return Err(ReferenceError::new(format!(
            "{REFERENCE_ROOT_ENV}={} does not contain index.toml",
            root.display()
        )));
    }

    if let Some(root) = install_sibling_root() {
        return Ok(root);
    }

    if let Some(root) = dev_repo_root() {
        return Ok(root);
    }

    Err(ReferenceError::new(format!(
        "Faber reference pack not found\n\
         hint: set {REFERENCE_ROOT_ENV} to a directory containing index.toml\n\
         hint: install the reference pack at share/faber/reference beside the faber binary\n\
         hint: development trees should run from the faber repository checkout"
    )))
}

fn detect_layout(root: &Path) -> ReferenceLayout {
    if root.join("exempla").is_dir() {
        ReferenceLayout::Pack
    } else {
        ReferenceLayout::Repo
    }
}

fn read_index(root: &Path) -> Result<IndexFile, ReferenceError> {
    let path = root.join("index.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|err| ReferenceError::new(format!("failed to read {}: {err}", path.display())))?;
    toml::from_str(&text).map_err(|err| {
        ReferenceError::new(format!("invalid index.toml at {}: {err}", path.display()))
    })
}

fn read_metadata(root: &Path, index: &IndexFile) -> Result<PackMetadata, ReferenceError> {
    let pack_path = root.join("PACK.toml");
    if pack_path.is_file() {
        let text = std::fs::read_to_string(&pack_path).map_err(|err| {
            ReferenceError::new(format!("failed to read {}: {err}", pack_path.display()))
        })?;
        let pack: PackFile = toml::from_str(&text).map_err(|err| {
            ReferenceError::new(format!(
                "invalid PACK.toml at {}: {err}",
                pack_path.display()
            ))
        })?;
        return Ok(PackMetadata {
            faber_version: pack.faber_version,
            generated_on: pack.generated_on,
            fab_count: pack.fab_count,
            registry_terms: pack.registry_terms,
            source_commit: pack.source_commit,
            index_generated_on: pack.index_generated_on,
        });
    }

    Ok(PackMetadata {
        faber_version: None,
        generated_on: index.generated_on.clone(),
        fab_count: index.fab_count,
        registry_terms: index.registry_terms,
        source_commit: None,
        index_generated_on: index.generated_on.clone(),
    })
}

fn read_legacy_redirects(root: &Path) -> Result<Vec<LegacyRedirect>, ReferenceError> {
    let path = root.join("legacy-redirects.toml");
    if !path.is_file() {
        return Err(ReferenceError::new(format!(
            "reference root {} is missing legacy-redirects.toml",
            root.display()
        )));
    }

    let text = std::fs::read_to_string(&path)
        .map_err(|err| ReferenceError::new(format!("failed to read {}: {err}", path.display())))?;
    let file: LegacyRedirectsFile = toml::from_str(&text).map_err(|err| {
        ReferenceError::new(format!(
            "invalid legacy-redirects.toml at {}: {err}",
            path.display()
        ))
    })?;

    Ok(file
        .redirects
        .into_iter()
        .map(|row| LegacyRedirect {
            term: row.term,
            canonical: row.canonical,
            message: row.message,
        })
        .collect())
}

fn install_sibling_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bin_dir = exe.parent()?;
    for relative in ["../share/faber/reference", "../lib/faber/reference"] {
        let candidate = bin_dir.join(relative);
        if candidate.join("index.toml").is_file() {
            return candidate.canonicalize().ok();
        }
    }
    None
}

/// Compare `PACK.toml` `faber_version` against the running `faber` crate version.
pub fn pack_version_skew(metadata: &PackMetadata) -> Result<Option<String>, ReferenceError> {
    let Some(pack_version) = metadata.faber_version.as_deref() else {
        return Ok(None);
    };

    let Some(pack) = parse_release_version(pack_version) else {
        return Ok(None);
    };
    let binary = parse_release_version(env!("CARGO_PKG_VERSION")).ok_or_else(|| {
        ReferenceError::new(format!(
            "faber binary version {:?} is not a release version",
            env!("CARGO_PKG_VERSION")
        ))
    })?;

    if pack.0 != binary.0 {
        return Err(ReferenceError::new(format!(
            "reference pack faber_version {pack_version} is incompatible with faber {} \
             (major version mismatch); reinstall the matching reference pack",
            env!("CARGO_PKG_VERSION")
        )));
    }

    if pack == binary {
        return Ok(None);
    }

    Ok(Some(format!(
        "reference pack faber_version {pack_version} differs from faber {}; \
         reinstall the matching pack if explain output looks stale",
        env!("CARGO_PKG_VERSION")
    )))
}

fn parse_release_version(value: &str) -> Option<(u32, u32, u32)> {
    let mut parts = value.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch: u32 = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn dev_repo_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        starts.push(PathBuf::from(manifest));
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            starts.push(parent.to_path_buf());
        }
    }

    for mut dir in starts {
        loop {
            for rel in [
                "crates/exempla/corpus/index.toml",
                "radix/crates/exempla/corpus/index.toml",
            ] {
                let index = dir.join(rel);
                if index.is_file() {
                    return Some(index.parent().expect("corpus dir").to_path_buf());
                }
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}
