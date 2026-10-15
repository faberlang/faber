//! Embedded Faber reference for `faber explain`.
//!
//! The build script embeds every corpus exemplum whose frontmatter declares
//! `explains = [...]`, plus `legacy-redirects.toml`. The reference is therefore
//! always the corpus of the commit this binary was built from: there is no
//! on-disk pack, no index file, and nothing to drift from the compiler.

use crate::explain::{Entry, ExplainError, Registry};
use radix::module::driver::split_frontmatter;
use serde::Deserialize;
use std::collections::BTreeMap;

pub use crate::reference_parse::{entry_from_exempla, legacy_entry_from_redirect};

include!(concat!(env!("OUT_DIR"), "/reference_embedded.rs"));

/// One explain term and the exemplum that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermRecord {
    pub term: String,
    /// Corpus-relative path of the exemplum whose `explains` lists this term.
    pub primary: String,
}

/// One legacy redirect from `legacy-redirects.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRedirect {
    pub term: String,
    pub canonical: String,
    pub message: String,
}

/// Resolved lookup against the term table (before exempla parsing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTerm<'a> {
    Canonical(&'a TermRecord),
    Legacy {
        redirect: &'a LegacyRedirect,
        canonical: &'a TermRecord,
    },
}

/// Term table built from exempla frontmatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferencePack {
    terms: Vec<TermRecord>,
    term_index: BTreeMap<String, usize>,
    sources: BTreeMap<String, String>,
    legacy: Vec<LegacyRedirect>,
    legacy_index: BTreeMap<String, usize>,
}

/// Failure building the reference from its sources.
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

#[derive(Debug, Default, Deserialize)]
struct ExplainsFrontmatter {
    #[serde(default)]
    explains: Vec<String>,
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
    /// The reference compiled into this binary.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded exempla or legacy-redirect sources fail
    /// to parse (see [`Self::from_sources`]).
    pub fn embedded() -> Result<Self, ReferenceError> {
        Self::from_sources(EMBEDDED_EXEMPLA, EMBEDDED_LEGACY_REDIRECTS)
    }

    /// Build the term table from `(corpus-relative path, source)` pairs.
    ///
    /// A file contributes the terms its frontmatter lists under `explains`;
    /// files without that key are ignored. Terms are ordered by term string so
    /// the registry does not depend on file order.
    ///
    /// # Errors
    ///
    /// Returns an error when a source's frontmatter cannot be split or parsed as
    /// TOML, when one term is explained by more than one file, when the legacy
    /// redirects do not parse, when a legacy redirect is duplicated, or when it
    /// points at a term no file explains.
    pub fn from_sources(files: &[(&str, &str)], legacy_toml: &str) -> Result<Self, ReferenceError> {
        let mut terms = Vec::new();
        let mut sources = BTreeMap::new();
        for (path, source) in files {
            let explains = explains_of(path, source)?;
            if explains.is_empty() {
                continue;
            }
            sources.insert((*path).to_owned(), (*source).to_owned());
            for term in explains {
                terms.push(TermRecord {
                    term,
                    primary: (*path).to_owned(),
                });
            }
        }
        terms.sort_by(|left, right| left.term.cmp(&right.term));

        let mut term_index = BTreeMap::new();
        for (index, record) in terms.iter().enumerate() {
            if let Some(previous) = term_index.insert(record.term.clone(), index) {
                let other: &TermRecord = &terms[previous];
                return Err(ReferenceError::new(format!(
                    "term {:?} is explained by both {} and {}",
                    record.term, other.primary, record.primary
                )));
            }
        }

        let legacy = parse_legacy_redirects(legacy_toml)?;
        let mut legacy_index = BTreeMap::new();
        for (index, redirect) in legacy.iter().enumerate() {
            if legacy_index.insert(redirect.term.clone(), index).is_some() {
                return Err(ReferenceError::new(format!(
                    "duplicate legacy redirect {:?}",
                    redirect.term
                )));
            }
            if !term_index.contains_key(&redirect.canonical) {
                return Err(ReferenceError::new(format!(
                    "legacy redirect {:?} points to missing canonical term {:?}",
                    redirect.term, redirect.canonical
                )));
            }
        }

        Ok(Self {
            terms,
            term_index,
            sources,
            legacy,
            legacy_index,
        })
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

    /// Raw embedded source of a corpus exemplum, by its primary path.
    ///
    /// Only files whose frontmatter carries a non-empty `explains` list are
    /// retained, so this returns `None` for any other path.
    #[cfg(test)]
    pub(crate) fn source(&self, primary_path: &str) -> Option<&str> {
        self.sources.get(primary_path).map(String::as_str)
    }

    pub fn legacy_redirect(&self, name: &str) -> Option<&LegacyRedirect> {
        self.legacy_index
            .get(name)
            .map(|index| &self.legacy[*index])
    }

    /// Resolve a canonical term or legacy spelling.
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

    /// Parse exempla entries and build an explain [`Registry`].
    ///
    /// # Errors
    ///
    /// Returns an error if an exempla entry or a legacy redirect entry fails to
    /// build, or if the assembled registry fails validation.
    pub fn build_registry(&self) -> Result<Registry, ExplainError> {
        let mut entries = Vec::new();

        for record in &self.terms {
            let source = &self.sources[&record.primary];
            entries.push(entry_from_exempla(&record.primary, source, &record.term)?);
        }

        let canonical_by_term: BTreeMap<String, Entry> = entries
            .iter()
            .map(|entry| (entry.term.clone(), entry.clone()))
            .collect();

        for redirect in &self.legacy {
            let canonical = &canonical_by_term[&redirect.canonical];
            entries.push(legacy_entry_from_redirect(
                &redirect.term,
                &redirect.canonical,
                &redirect.message,
                canonical,
            )?);
        }

        Registry::from_entries(entries)
    }
}

fn explains_of(path: &str, source: &str) -> Result<Vec<String>, ReferenceError> {
    let split = split_frontmatter(source)
        .map_err(|error| ReferenceError::new(format!("{path}: {error}")))?;
    let Some(frontmatter_text) = split.frontmatter_text else {
        return Ok(Vec::new());
    };
    let frontmatter: ExplainsFrontmatter = toml::from_str(frontmatter_text)
        .map_err(|error| ReferenceError::new(format!("{path}: {error}")))?;
    Ok(frontmatter.explains)
}

fn parse_legacy_redirects(text: &str) -> Result<Vec<LegacyRedirect>, ReferenceError> {
    let file: LegacyRedirectsFile = toml::from_str(text)
        .map_err(|err| ReferenceError::new(format!("invalid legacy-redirects.toml: {err}")))?;

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
