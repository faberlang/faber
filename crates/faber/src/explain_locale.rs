//! Reader-locale projection of the explain reference.
//!
//! The reference is authored against canonical identities (the Latin registry
//! keys that every locale pack maps to its own spelling). A reader of locale
//! `en` asks for `fn`, not `functio`, so the registry is projected through the
//! selected [`LocalePack`] before it is indexed: terms, cross references,
//! summaries, syntax lines and body prose all take the pack's spelling.
//!
//! The pack is the only source of spellings. There is no table in this module:
//! a pack whose rows are identities (the `la` pack) projects to the identity
//! registry, and a pack that renames a row renames it everywhere.
//!
//! INVARIANTS
//! ==========
//! - Display terms are unique. When two identities share one spelling in a pack,
//!   the first by (identity-equals-spelling, category, identity) keeps the bare
//!   spelling and each other entry is qualified as `spelling (category)`, so
//!   every entry stays reachable and no entry is dropped.
//! - Legacy-redirect entries keep their term: they are historical spellings,
//!   not identities.
//! - Prose substitution rewrites only words inside backtick spans; bare prose
//!   words are left as written (several identities are ordinary English words,
//!   so a bare word cannot be told from an identifier). Syntax lines rewrite
//!   every word.

use crate::explain::{Entry, Kind};
use radix::module::locale::LocalePack;
use std::collections::BTreeMap;

/// Identity-to-spelling projection for one reader pack.
pub(crate) struct Projection {
    /// Final display term for each projected entry identity.
    terms: BTreeMap<String, String>,
    /// Identity word to spelling, for rewriting running text.
    words: BTreeMap<String, String>,
}

impl Projection {
    /// Build the projection for `entries` under `pack`.
    pub(crate) fn new(entries: &[Entry], pack: &LocalePack) -> Self {
        Self {
            terms: project_terms(entries, pack),
            words: pack_words(pack),
        }
    }

    /// The spelling of every projected identity, for caller-side hints.
    pub(crate) fn terms(&self) -> &BTreeMap<String, String> {
        &self.terms
    }

    /// Project every entry in place.
    pub(crate) fn apply(&self, entries: &mut [Entry]) {
        for entry in entries {
            if entry.kind != Kind::Legacy {
                if let Some(term) = self.terms.get(&entry.term) {
                    entry.term.clone_from(term);
                }
            }
            entry.aliases = entry
                .aliases
                .iter()
                .filter(|alias| **alias != entry.term)
                .cloned()
                .collect();
            for related in &mut entry.related {
                *related = self.term_of(related);
            }
            if let Some(canonical) = &mut entry.canonical_term {
                *canonical = self.term_of(canonical);
            }
            entry.summary = rewrite_prose(&entry.summary, &self.words);
            entry.syntax = rewrite_words(&entry.syntax, &self.words, 0);
            entry.body = self.rewrite_body(&entry.body);
        }
    }

    fn term_of(&self, identity: &str) -> String {
        self.terms
            .get(identity)
            .cloned()
            .unwrap_or_else(|| identity.to_owned())
    }

    /// Rewrite the prose that precedes the first `fab` example. The example is
    /// source in its own locale and is never touched.
    fn rewrite_body(&self, body: &str) -> String {
        match body.find("```fab") {
            Some(split) => {
                let (prose, rest) = body.split_at(split);
                format!("{}{rest}", rewrite_prose(prose, &self.words))
            }
            None => rewrite_prose(body, &self.words),
        }
    }
}

/// The pack's spelling for one entry identity, if the pack renames it.
///
/// A type entry consults the type rows first, every other entry the keyword
/// rows first; intrinsic rows are the last resort.
fn spelling<'p>(pack: &'p LocalePack, entry: &Entry) -> Option<&'p str> {
    let keyword = pack
        .keywords
        .get(&entry.term)
        .map(|row| row.canonical.as_str());
    let ty = pack
        .types
        .get(&entry.term)
        .map(|row| row.canonical.as_str());
    let intrinsic = pack
        .intrinsics
        .get(&entry.term)
        .map(|row| row.canonical.as_str());
    if entry.kind == Kind::Type {
        ty.or(keyword).or(intrinsic)
    } else {
        keyword.or(ty).or(intrinsic)
    }
}

fn project_terms(entries: &[Entry], pack: &LocalePack) -> BTreeMap<String, String> {
    // Group the entries that project to one spelling.
    let mut groups: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    for entry in entries.iter().filter(|entry| entry.kind != Kind::Legacy) {
        let display = spelling(pack, entry).unwrap_or(&entry.term).to_owned();
        groups.entry(display).or_default().push(entry);
    }

    let mut terms = BTreeMap::new();
    for (display, mut group) in groups {
        group.sort_by(|left, right| {
            (left.term != display)
                .cmp(&(right.term != display))
                .then_with(|| left.category.cmp(&right.category))
                .then_with(|| left.term.cmp(&right.term))
        });
        for (rank, entry) in group.into_iter().enumerate() {
            let term = if rank == 0 {
                display.clone()
            } else {
                format!("{display} ({})", entry.category)
            };
            terms.insert(entry.term.clone(), term);
        }
    }
    terms
}

/// Identity word to spelling, for every keyword and type row the pack renames.
fn pack_words(pack: &LocalePack) -> BTreeMap<String, String> {
    let keywords = pack
        .keywords
        .iter()
        .map(|(key, row)| (key, row.canonical.as_str()));
    let types = pack
        .types
        .iter()
        .map(|(key, row)| (key, row.canonical.as_str()));
    keywords
        .chain(types)
        .filter(|(key, spelling)| key.as_str() != *spelling && is_word(key))
        .map(|(key, spelling)| (key.clone(), spelling.to_owned()))
        .collect()
}

fn is_word(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Rewrite prose: only the words inside backtick spans.
///
/// Bare prose words are left exactly as written: several identities are
/// ordinary English words, so a bare word cannot be told from an identifier.
/// A span's leading article (an `itera`) is made to agree with its new
/// spelling.
pub(crate) fn rewrite_prose(text: &str, words: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut segments = text.split('`');
    if let Some(first) = segments.next() {
        out.push_str(first);
    }
    // After the first split segment, segments alternate span, prose, span, ...
    // An unterminated final span is treated as prose, as it renders.
    let rest: Vec<&str> = segments.collect();
    for (index, segment) in rest.iter().enumerate() {
        let is_span = index % 2 == 0 && index + 1 < rest.len();
        if is_span {
            let rewritten = rewrite_words(segment, words, 0);
            if rewritten != *segment {
                agree_article(&mut out, &rewritten);
            }
            out.push('`');
            out.push_str(&rewritten);
            out.push('`');
        } else if index % 2 == 0 {
            out.push('`');
            out.push_str(segment);
        } else {
            out.push_str(segment);
        }
    }
    out
}

/// Rewrite whole identifier words of at least `min_len` letters.
pub(crate) fn rewrite_words(
    text: &str,
    words: &BTreeMap<String, String>,
    min_len: usize,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        match words.get(word.as_str()) {
            Some(spelling) if word.chars().count() >= min_len => {
                agree_article(out, spelling);
                out.push_str(spelling);
            }
            _ => out.push_str(word),
        }
        word.clear();
    };
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
            out.push(ch);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Make a trailing indefinite article in `out` agree with the word about to be
/// appended (`an itera` becomes `a for` once `itera` is spelled `for`).
fn agree_article(out: &mut String, next: &str) {
    let wants_an = next
        .chars()
        .next()
        .is_some_and(|ch| "aeiouAEIOU".contains(ch));
    for (article, other) in [("an ", "a "), ("a ", "an "), ("An ", "A "), ("A ", "An ")] {
        let Some(head) = out.strip_suffix(article) else {
            continue;
        };
        let at_boundary = head
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
        let is_an = article.trim_end().eq_ignore_ascii_case("an");
        if at_boundary && is_an != wants_an {
            let head_len = head.len();
            out.truncate(head_len);
            out.push_str(other);
        }
        return;
    }
}
