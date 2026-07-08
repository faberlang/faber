//! Disk-backed reference registry for `faber explain`.
//!
//! This module indexes exempla reference-pack entries loaded from disk
//! (`examples/corpus/` in development, `share/faber/reference/` when installed)
//! and exposes rendering-friendly lookup/search results. `.fab` parsing lives in
//! [`crate::reference_parse`]; this module owns registry indexing and validation.
//!
//! INVARIANTS
//! ==========
//! - Corpus errors are programmer errors: malformed entries fail during
//!   registry construction instead of producing partial help output.
//! - Canonical entries and legacy spellings stay distinct so the CLI can teach
//!   current Faber syntax while still explaining historical or familiar forms.
//! - Ordered maps are used deliberately to keep list/search output stable for
//!   tests, docs, and terminal users.
//! - Non-canonical entries must point at a canonical replacement before they can
//!   be rendered or returned from search.

use crate::io_buf::writeln_buf;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Parsed explain entry consumed by lookup, search, rendering, and JSON output.
///
/// `canonical` and `canonical_term` form the central invariant:
/// canonical entries describe the current syntax, while non-canonical entries
/// must point at the current term they should redirect users toward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// Primary spelling or glyph shown to users.
    pub term: String,

    /// Broad reference kind used for grouping and display.
    pub kind: Kind,

    /// Human-readable topic bucket, such as `comparison` or `control-flow`.
    pub category: String,

    /// Whether this entry describes current canonical Faber syntax.
    pub canonical: bool,

    /// One-sentence summary used in lists and short renderings.
    pub summary: String,

    /// Compact syntax contract shown before the long description.
    pub syntax: String,

    /// Example snippets declared in front matter for structured consumers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,

    /// Alternate query terms that resolve to this entry without legacy status.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,

    /// Historical spellings or forms associated with this entry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub legacy: Vec<String>,

    /// Canonical replacement term for non-canonical entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_term: Option<String>,

    /// Related terms, aliases, or legacy spellings for cross-reference output.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<String>,

    /// Markdown body after front matter.
    pub body: String,
}

/// Reference taxonomy used for grouping, display, and JSON consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Reserved word or structural language form.
    Keyword,

    /// Symbolic or word operator.
    Operator,

    /// Metadata marker that modifies declarations or target behavior.
    Annotation,

    /// Literal value syntax.
    Literal,

    /// Built-in or documented type name.
    Type,

    /// Declaration or behavior modifier.
    Modifier,

    /// Non-canonical historical syntax kept for migration guidance.
    Legacy,

    /// Reference topic that explains a language idea rather than one token.
    Concept,

    /// Explicit value-conversion form (`↦ target`).
    Conversio,
}

/// Result of resolving a user query against the registry.
///
/// The variants preserve resolution provenance so renderers can distinguish a
/// direct canonical lookup from an alias hit or a legacy spelling that should
/// show both old and replacement terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<'a> {
    /// Query matched the canonical term exactly.
    Exact(&'a Entry),

    /// Query matched an accepted alternate name for a canonical entry.
    Alias {
        /// Original user query that matched the alias index.
        query: String,

        /// Canonical entry reached through the alias.
        entry: &'a Entry,
    },

    /// Query matched non-canonical syntax and carries the replacement entry.
    Legacy {
        /// Original user query that matched a legacy term or alias.
        query: String,

        /// Legacy entry explaining the old syntax.
        entry: &'a Entry,

        /// Canonical entry users should write instead.
        canonical: &'a Entry,
    },
}

/// Ranked search result.
///
/// Lower scores are better. The numeric value is intentionally small and local
/// to this module; callers should sort through [`Registry::search`] rather than
/// depending on score constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchHit<'a> {
    /// Entry to display for this hit, usually canonicalized from legacy matches.
    pub entry: &'a Entry,

    /// Relative match quality; lower values sort first.
    pub score: u8,
}

/// Parsed and indexed explain corpus.
///
/// The three maps split exact terms, aliases, and legacy spellings because each
/// route has different user-facing behavior. Map values are indexes into
/// `entries`, which keeps lookup results borrowing from one owned corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    entries: Vec<Entry>,
    terms: BTreeMap<String, usize>,
    aliases: BTreeMap<String, usize>,
    legacy: BTreeMap<String, usize>,
    pub(crate) reference_root: Option<std::path::PathBuf>,
    pub(crate) reference_version: Option<String>,
    pub(crate) version_warning: Option<String>,
}

/// Error type for corpus parsing, validation, and rendering failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainError {
    /// Human-readable diagnostic with source filename when available.
    pub message: String,
}

impl ExplainError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExplainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExplainError {}

impl Registry {
    /// Load the exempla reference pack from disk and build the explain registry.
    pub fn load() -> Result<Self, ExplainError> {
        Self::load_from_disk()
    }

    /// Load the disk-backed exempla reference pack and build the explain registry.
    pub fn load_from_disk() -> Result<Self, ExplainError> {
        let pack = crate::reference::ReferencePack::load()?;
        let version_warning = pack.version_warning().map(str::to_owned);
        let mut registry = pack.build_registry()?;
        registry.version_warning = version_warning;
        Ok(registry)
    }

    /// Non-fatal pack/binary version skew detected at load time.
    pub fn version_warning(&self) -> Option<&str> {
        self.version_warning.as_deref()
    }

    /// Build a registry from parsed entries and optional pack provenance.
    pub(crate) fn from_entries(
        entries: Vec<Entry>,
        reference_root: Option<std::path::PathBuf>,
        reference_version: Option<String>,
    ) -> Result<Self, ExplainError> {
        let mut registry = Self {
            entries,
            terms: BTreeMap::new(),
            aliases: BTreeMap::new(),
            legacy: BTreeMap::new(),
            reference_root,
            reference_version,
            version_warning: None,
        };
        registry.index()?;
        registry.validate_references()?;
        Ok(registry)
    }

    /// Return the loaded reference pack root when this registry came from disk.
    pub fn reference_root(&self) -> Option<&std::path::Path> {
        self.reference_root.as_deref()
    }

    /// Return the pack version recorded in `PACK.toml` when available.
    pub fn reference_version(&self) -> Option<&str> {
        self.reference_version.as_deref()
    }

    /// Return parsed entries in registry order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Return all declared categories in deterministic order.
    pub fn categories(&self) -> BTreeSet<&str> {
        self.entries
            .iter()
            .map(|entry| entry.category.as_str())
            .collect()
    }

    /// Return entries whose category exactly matches `category`.
    pub fn by_category(&self, category: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.category == category)
            .collect()
    }

    /// Resolve an exact term, alias, or legacy spelling.
    ///
    /// This function intentionally does not normalize `query`; CLI callers can
    /// choose whether exact explain lookup should be case-sensitive, while
    /// [`Registry::search`] handles fuzzy discovery.
    pub fn lookup(&self, query: &str) -> Option<Lookup<'_>> {
        // Exact terms win over aliases and legacy spellings. A non-canonical
        // exact term still resolves as legacy so the caller can display the
        // replacement contract instead of treating old syntax as current.
        if let Some(index) = self.terms.get(query) {
            let entry = &self.entries[*index];
            if entry.canonical {
                return Some(Lookup::Exact(entry));
            }

            let canonical_term = entry.canonical_term.as_deref()?;
            let canonical = self
                .terms
                .get(canonical_term)
                .map(|index| &self.entries[*index])?;
            return Some(Lookup::Legacy {
                query: query.to_owned(),
                entry,
                canonical,
            });
        }

        if let Some(index) = self.aliases.get(query) {
            return Some(Lookup::Alias {
                query: query.to_owned(),
                entry: &self.entries[*index],
            });
        }

        let legacy_index = self.legacy.get(query)?;
        let entry = &self.entries[*legacy_index];
        let canonical_term = entry.canonical_term.as_deref()?;
        let canonical = self
            .terms
            .get(canonical_term)
            .map(|index| &self.entries[*index])?;
        Some(Lookup::Legacy {
            query: query.to_owned(),
            entry,
            canonical,
        })
    }

    /// Search terms, aliases, references, summaries, syntax strings, and body text.
    ///
    /// Results are normalized to canonical display entries when a legacy entry
    /// matches, then sorted by match quality and term for deterministic output.
    pub fn search(&self, query: &str) -> Vec<SearchHit<'_>> {
        let query = normalize_query(query);
        let mut hits: BTreeMap<String, SearchHit<'_>> = BTreeMap::new();

        for entry in &self.entries {
            let Some(score) = search_score(entry, &query) else {
                continue;
            };

            let Some(display_entry) = self.display_search_entry(entry) else {
                continue;
            };

            // Multiple old spellings may point to the same canonical entry.
            // Keep one display row per canonical term, preserving the strongest
            // match score that led the user there.
            let key = display_entry.term.clone();
            let should_replace = match hits.get(&key) {
                Some(existing) => {
                    score < existing.score
                        || (score == existing.score
                            && display_entry.canonical
                            && !existing.entry.canonical)
                }
                None => true,
            };

            if should_replace {
                hits.insert(
                    key,
                    SearchHit {
                        entry: display_entry,
                        score,
                    },
                );
            }
        }

        let mut results: Vec<SearchHit<'_>> = hits.into_values().collect();
        results.sort_by(|left, right| {
            left.score
                .cmp(&right.score)
                .then_with(|| right.entry.canonical.cmp(&left.entry.canonical))
                .then_with(|| left.entry.term.cmp(&right.entry.term))
        });
        results
    }

    pub(crate) fn index(&mut self) -> Result<(), ExplainError> {
        // Terms are globally unique, including non-canonical entries. That lets
        // `lookup()` detect exact old spellings before checking aliases.
        for (index, entry) in self.entries.iter().enumerate() {
            insert_unique(&mut self.terms, &entry.term, index, "term")?;
        }

        // Aliases are for alternate names of the same current concept. Legacy
        // spellings are indexed separately because they need different output.
        for (index, entry) in self.entries.iter().enumerate() {
            for alias in &entry.aliases {
                insert_unique(&mut self.aliases, alias, index, "alias")?;
            }
        }

        for (index, entry) in self.entries.iter().enumerate() {
            if entry.canonical {
                continue;
            }

            insert_unique(&mut self.legacy, &entry.term, index, "legacy term")?;
        }

        for (index, entry) in self.entries.iter().enumerate() {
            for legacy in &entry.legacy {
                // If a legacy spelling has its own non-canonical entry, prefer
                // that full entry over a bare legacy alias.
                if let Some(explicit_index) = self.terms.get(legacy) {
                    if !self.entries[*explicit_index].canonical {
                        continue;
                    }
                }

                insert_unique(&mut self.legacy, legacy, index, "legacy alias")?;
            }
        }

        Ok(())
    }

    pub(crate) fn validate_references(&self) -> Result<(), ExplainError> {
        // Reference validation runs after indexing so related terms may point
        // at canonical terms, aliases, or legacy spellings with identical rules
        // to interactive lookup.
        for entry in &self.entries {
            if !entry.canonical && entry.canonical_term.is_none() {
                return Err(ExplainError::new(format!(
                    "{} is non-canonical but has no canonical_term",
                    entry.term
                )));
            }

            if let Some(canonical_term) = &entry.canonical_term {
                let Some(index) = self.terms.get(canonical_term) else {
                    return Err(ExplainError::new(format!(
                        "{} points to missing canonical_term {canonical_term}",
                        entry.term
                    )));
                };
                if !self.entries[*index].canonical {
                    return Err(ExplainError::new(format!(
                        "{} points to non-canonical canonical_term {canonical_term}",
                        entry.term
                    )));
                }
            }

            for related in &entry.related {
                if !self.terms.contains_key(related)
                    && !self.aliases.contains_key(related)
                    && !self.legacy.contains_key(related)
                {
                    return Err(ExplainError::new(format!(
                        "{} has unknown related term {related}",
                        entry.term
                    )));
                }
            }
        }

        Ok(())
    }

    fn display_search_entry<'a>(&'a self, entry: &'a Entry) -> Option<&'a Entry> {
        if entry.canonical {
            return Some(entry);
        }

        let canonical_term = entry.canonical_term.as_deref()?;
        let index = self.terms.get(canonical_term)?;
        Some(&self.entries[*index])
    }
}

/// Render one lookup result in the terminal-oriented explain format.
pub fn render_plain(lookup: &Lookup<'_>) -> String {
    crate::explain_render::render_lookup_plain(lookup)
}

/// Render ranked search results for terminal output.
pub fn render_search(query: &str, hits: &[SearchHit<'_>]) -> String {
    crate::explain_render::render_search(query, hits)
}

/// Render one lookup result as JSON while preserving resolution provenance.
pub fn render_json(lookup: &Lookup<'_>) -> Result<String, ExplainError> {
    match lookup {
        Lookup::Exact(entry) => serde_json::to_string_pretty(entry),
        Lookup::Alias { query, entry } => serde_json::to_string_pretty(&JsonLookup {
            query,
            resolved_via: "alias",
            entry,
            canonical: None,
        }),
        Lookup::Legacy {
            query,
            entry,
            canonical,
        } => serde_json::to_string_pretty(&JsonLookup {
            query,
            resolved_via: "legacy",
            entry,
            canonical: Some(canonical),
        }),
    }
    .map_err(|err| ExplainError::new(format!("failed to render JSON: {err}")))
}

/// Render a grouped list of canonical explain terms.
///
/// Legacy entries are intentionally omitted from the normal list so the command
/// presents the current language surface first.
pub fn render_list(registry: &Registry) -> String {
    let mut out = String::new();
    if let Some(root) = registry.reference_root() {
        writeln_buf(&mut out, format_args!("reference: {}", root.display()));
        if let Some(version) = registry.reference_version() {
            writeln_buf(&mut out, format_args!("pack version: {version}"));
        }
        out.push('\n');
    }
    let entries = registry
        .entries()
        .iter()
        .filter(|entry| entry.canonical)
        .collect::<Vec<_>>();
    let term_width = entries
        .iter()
        .map(|entry| display_width(&entry.term))
        .max()
        .unwrap_or(0);
    let category_width = entries
        .iter()
        .map(|entry| display_width(&entry.category))
        .max()
        .unwrap_or(0);

    for kind in [
        Kind::Keyword,
        Kind::Operator,
        Kind::Annotation,
        Kind::Literal,
        Kind::Modifier,
        Kind::Type,
        Kind::Concept,
        Kind::Conversio,
    ] {
        let group = entries
            .iter()
            .copied()
            .filter(|entry| entry.kind == kind)
            .collect::<Vec<_>>();
        if group.is_empty() {
            continue;
        }

        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(kind.list_title());
        out.push('\n');

        for entry in group {
            out.push_str("  ");
            push_padded(&mut out, &entry.term, term_width);
            out.push_str("  ");
            push_padded(&mut out, &entry.category, category_width);
            out.push_str("  ");
            out.push_str(&entry.summary);
            out.push('\n');
        }
    }
    out
}

/// Render all entries in one category.
///
/// Unlike [`render_list`], category output includes legacy entries. This makes
/// category views useful for auditing a whole documentation area.
pub fn render_category(registry: &Registry, category: &str) -> Option<String> {
    let entries = registry.by_category(category);
    if entries.is_empty() {
        return None;
    }

    let mut out = String::new();
    let term_width = entries
        .iter()
        .map(|entry| display_width(&entry.term))
        .max()
        .unwrap_or(0);
    for entry in entries {
        push_padded(&mut out, &entry.term, term_width);
        out.push_str("  ");
        out.push_str(&entry.summary);
        out.push('\n');
    }
    Some(out)
}

fn push_padded(out: &mut String, value: &str, width: usize) {
    out.push_str(value);
    let padding = width.saturating_sub(display_width(value));
    out.push_str(&" ".repeat(padding));
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

#[derive(Serialize)]
struct JsonLookup<'a> {
    query: &'a str,
    resolved_via: &'static str,
    entry: &'a Entry,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical: Option<&'a Entry>,
}

fn insert_unique(
    map: &mut BTreeMap<String, usize>,
    key: &str,
    index: usize,
    label: &str,
) -> Result<(), ExplainError> {
    if let Some(previous) = map.insert(key.to_owned(), index) {
        return Err(ExplainError::new(format!(
            "duplicate {label} {key} in entries {previous} and {index}"
        )));
    }
    Ok(())
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Keyword => "keyword",
            Kind::Operator => "operator",
            Kind::Annotation => "annotation",
            Kind::Literal => "literal",
            Kind::Type => "type",
            Kind::Modifier => "modifier",
            Kind::Legacy => "legacy",
            Kind::Concept => "concept",
            Kind::Conversio => "conversio",
        }
    }

    fn list_title(self) -> &'static str {
        match self {
            Kind::Keyword => "KEYWORDS",
            Kind::Operator => "OPERATORS",
            Kind::Annotation => "ANNOTATIONS",
            Kind::Literal => "LITERALS",
            Kind::Type => "TYPES",
            Kind::Modifier => "MODIFIERS",
            Kind::Legacy => "LEGACY",
            Kind::Concept => "CONCEPTS",
            Kind::Conversio => "CONVERSIONS",
        }
    }
}

fn normalize_query(query: &str) -> String {
    query.trim().to_lowercase()
}

fn search_score(entry: &Entry, query: &str) -> Option<u8> {
    if query.is_empty() {
        return None;
    }

    // Lower is better. Bias keeps exact/current terminology above prose hits
    // while still allowing body text to make an entry discoverable.
    let mut best: Option<u8> = None;
    score_term(&mut best, &entry.term, query, 0);

    for alias in &entry.aliases {
        score_term(&mut best, alias, query, 1);
    }

    for legacy in &entry.legacy {
        score_term(&mut best, legacy, query, 1);
    }

    if let Some(canonical_term) = &entry.canonical_term {
        score_term(&mut best, canonical_term, query, 1);
    }

    score_field(&mut best, &entry.summary, query, 3);
    score_field(&mut best, &entry.syntax, query, 3);
    for related in &entry.related {
        score_field(&mut best, related, query, 3);
    }
    score_field(&mut best, &entry.body, query, 4);

    best
}

fn score_term(best: &mut Option<u8>, candidate: &str, query: &str, bias: u8) {
    score_field(best, candidate, query, bias);
}

fn score_field(best: &mut Option<u8>, candidate: &str, query: &str, bias: u8) {
    let candidate = candidate.to_lowercase();
    let score = if candidate == query {
        Some(bias)
    } else if candidate.starts_with(query) {
        Some(bias + 1)
    } else if candidate.contains(query) {
        Some(bias + 2)
    } else {
        None
    };

    if let Some(score) = score {
        match best {
            Some(current) if *current <= score => {}
            _ => *best = Some(score),
        }
    }
}
