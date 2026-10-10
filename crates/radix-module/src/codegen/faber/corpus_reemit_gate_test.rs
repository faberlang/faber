//! Corpus-wide re-emit gate (fmtsw1).
//!
//! The 2026-08-27 format sweep re-emitted the whole exempla corpus through the
//! HIR-backed Faber emitter before it was lossless, and dropped authored
//! constructs in dozens of files without any guard noticing. This gate runs the
//! same pipeline over every corpus exemplum and checks two things:
//!
//! 1. the post-emit meaning re-validation (`hir_meaning_diverged`) holds;
//! 2. the multiset of string literals, comment lines, and annotation heads
//!    survives — a construct the HIR cannot carry (a hook body, a named hole,
//!    top-level statements lowering drops) is invisible to the meaning
//!    fingerprint, so its strings and comments are the witness.
//!
//! Files that cannot analyze (the negative exempla) are skipped: `format`
//! refuses error documents by design. Known gaps are listed with their cause
//! and owner; a listed file that starts passing fails the gate so the list
//! stays honest.

use super::reemit_faber;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Meaning divergences that exist today. Each needs a fix outside the emitter
/// leaf (or an exemplum repair) and is tracked here so the gate stays green
/// only while the list is exact.
const KNOWN_MEANING_GAPS: &[(&str, &str)] = &[
    // Temporary exact-list entries (2026-10-10): unit sb170 made these two files
    // analyzable (first-match admits an array literal source) and the re-emit
    // validation then reports a first-match over a literal array as `nihil`.
    // Owner: unit sb182; it removes both rows when fixed.
    (
        "primus-quem/primus-quem-air.fab",
        "re-emit of primus_quem over an array literal re-validates as nihil (sb182)",
    ),
    (
        "primus-quem/primus-quem.fab",
        "re-emit of primus_quem over an array literal re-validates as nihil (sb182)",
    ),
];

/// Files the structure check refuses (`CODEGEN006` / `ast_construct_dropped`): HIR keeps no
/// representation for the construct, so a re-emit would silently delete it.
const EXPECTED_STRUCTURE_REFUSALS: &[(&str, &str)] = &[
    (
        "postpara/postpara.fab",
        "`teardown`/`setup` hook blocks are not represented in HIR (lowering keeps only `omnia` hooks, spliced into test bodies)",
    ),
    ("postparabit/postparabit.fab", "async hook blocks: see postpara/postpara.fab"),
    ("praepara/praepara.fab", "hook blocks: see postpara/postpara.fab"),
    ("praeparabit/praeparabit.fab", "async hook blocks: see postpara/postpara.fab"),
];

/// Witness losses (strings, comments, annotation heads) the meaning fingerprint cannot see.
const KNOWN_STRING_LOSS: &[(&str, &str)] = &[(
    "literalia/named-holes.fab",
    "named template holes (`§{name}`) lower to positional holes; the names are not retained",
)];

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn collect_fab(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_fab(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "fab") {
            out.push(path);
        }
    }
}

/// Reader locale from the leading `+++` frontmatter block; product default `en`.
fn frontmatter_locale(source: &str) -> String {
    let Some(rest) = source.strip_prefix("+++\n") else {
        return "en".to_owned();
    };
    let header = rest.split("\n+++").next().unwrap_or("");
    header
        .lines()
        .find_map(|line| {
            let value = line
                .strip_prefix("locale")?
                .trim_start()
                .strip_prefix('=')?
                .trim();
            Some(value.trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| "en".to_owned())
}

fn strip_frontmatter(source: &str) -> &str {
    source
        .strip_prefix("+++\n")
        .and_then(|rest| rest.split_once("\n+++\n").map(|(_, body)| body))
        .unwrap_or(source)
}

/// Multiset of witnesses the meaning fingerprint cannot see: `"…"` literals,
/// `#` comment lines (trimmed), and `@ name` annotation heads.
fn string_literals(source: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let chars: Vec<char> = strip_frontmatter(source).chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '#' => {
                let start = i;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                let comment: String = chars[start..i].iter().collect();
                *counts
                    .entry(format!("comment:{}", comment.trim_end()))
                    .or_insert(0) += 1;
            }
            '@' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                let start = j;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let head: String = chars[start..j].iter().collect();
                *counts.entry(format!("annotation:{head}")).or_insert(0) += 1;
                i = j;
            }
            '"' => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                let literal: String = chars[start..(i + 1).min(chars.len())].iter().collect();
                *counts.entry(literal).or_insert(0) += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    counts
}

#[derive(Default)]
struct Survey {
    audited: usize,
    skipped_unanalyzable: usize,
    meaning_diverged: Vec<String>,
    structure_refused: Vec<String>,
    string_lost: Vec<String>,
}

fn survey_corpus() -> Survey {
    let root = corpus_root();
    let mut files = Vec::new();
    collect_fab(&root, &mut files);
    let mut survey = Survey::default();
    for path in files {
        let source = fs::read_to_string(&path).expect("read corpus file");
        let locale = frontmatter_locale(&source);
        let pack = LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(&locale))
            .unwrap_or_else(|err| panic!("{locale} pack should load: {err:?}"));
        let session = Session::new(
            Config::default()
                .with_target(Target::HirFaber)
                .with_dev_locale_root(),
        );
        let result = reemit_faber(&session, &pack, path.to_str().expect("utf-8 path"), &source);
        let key = path
            .strip_prefix(&root)
            .expect("corpus-relative path")
            .to_string_lossy()
            .replace('\\', "/");
        if !result.success() {
            if result
                .diagnostics
                .iter()
                .any(|d| d.issue() == Some("hir_meaning_diverged"))
            {
                survey.meaning_diverged.push(key);
            } else if result
                .diagnostics
                .iter()
                .any(|d| d.issue() == Some("ast_construct_dropped"))
            {
                survey.structure_refused.push(key);
            } else {
                survey.skipped_unanalyzable += 1;
            }
            continue;
        }
        survey.audited += 1;
        let emitted = result
            .output
            .expect("successful re-emit carries output")
            .code;
        if string_literals(&source) != string_literals(&emitted) {
            survey.string_lost.push(key);
        }
    }
    survey
}

fn assert_exact(label: &str, found: &[String], known: &[(&str, &str)]) {
    let unexpected: Vec<&String> = found
        .iter()
        .filter(|key| !known.iter().any(|(known_key, _)| known_key == key))
        .collect();
    assert!(
        unexpected.is_empty(),
        "re-emit {label} in corpus files not listed as known gaps: {unexpected:?}"
    );
    let stale: Vec<&str> = known
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| !found.iter().any(|f| f == key))
        .collect();
    assert!(
        stale.is_empty(),
        "known {label} gaps that no longer occur (remove from the list): {stale:?}"
    );
}

#[test]
fn corpus_reemit_keeps_meaning_and_authored_witnesses() {
    let survey = survey_corpus();
    assert!(
        survey.audited > 300,
        "the gate must audit the corpus, not skip it ({} audited, {} unanalyzable)",
        survey.audited,
        survey.skipped_unanalyzable
    );
    assert_exact("meaning divergence", &survey.meaning_diverged, KNOWN_MEANING_GAPS);
    assert_exact("structure refusal", &survey.structure_refused, EXPECTED_STRUCTURE_REFUSALS);
    assert_exact("witness loss", &survey.string_lost, KNOWN_STRING_LOSS);
}
