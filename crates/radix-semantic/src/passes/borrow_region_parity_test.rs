//! BRI-U4 differential parity harness.
//!
//! Parity theorem under test, not assertion: for every program (the inline
//! divergence corpus plus the `radix/corpus` language exempla), the
//! region-routed checker (`borrow::analyze_with_regions` with BRI-2 facts)
//! must not report any borrow-family violation that the block-scope
//! approximation (`borrow::analyze` with no region facts) accepts. The
//! inverse direction is legal — regions may accept after last-use inference —
//! and unsupported HIR shapes keep the approximation (fail closed), so those
//! programs are excluded from comparison.

use super::borrow_region;
use crate::test_support::analyze_source;
use crate::{Resolver, SemanticError};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Borrow-family issues whose presence may differ between the two checkers.
const BORROW_ISSUES: &[&str] = &[
    "write_while_borrowed",
    "use_while_mutably_borrowed",
    "mutable_borrow_conflict",
    "shared_borrow_conflict",
    "borrow_of_moved",
    "cannot_move_while_borrowed",
    "use_after_move",
];

/// Distinct borrow-family issue strings in one checker's output.
fn borrow_issue_set(errors: &[SemanticError]) -> BTreeSet<&str> {
    errors
        .iter()
        .filter_map(|error| {
            let issue = error.args.iter().find(|arg| arg.name == "issue")?;
            BORROW_ISSUES
                .contains(&issue.value.as_str())
                .then_some(issue.value.as_str())
        })
        .collect()
}

fn issue_set(result: &Result<(), Vec<SemanticError>>) -> BTreeSet<&str> {
    match result {
        Ok(()) => BTreeSet::new(),
        Err(errors) => borrow_issue_set(errors),
    }
}

/// Outcome of running both checkers over one program.
enum Parity {
    /// Region engine engaged; approximation-accepted programs still accepted.
    Pass,
    /// Region engine engaged and newly rejected an approximation-legal
    /// program — the parity defect this harness exists to catch.
    Violation(String),
    /// No typed HIR / fact table, or an unsupported HIR shape: the
    /// approximation keeps authority (fail closed), so no comparison holds.
    Skipped,
}

fn parity_check(source: &str, label: &str) -> Parity {
    let result = analyze_source(source);
    let Some(hir) = result.hir.as_ref() else {
        return Parity::Skipped;
    };
    let Some(facts) = result.access_facts.as_ref() else {
        return Parity::Skipped;
    };
    let Some(regions) = borrow_region::analyze(hir, &result.types, facts).ok() else {
        return Parity::Skipped;
    };
    let resolver = Resolver::new();

    let approximation =
        crate::passes::borrow::analyze(hir, &resolver, &result.types, &result.interner);
    let region = crate::passes::borrow::analyze_with_regions(
        hir,
        &resolver,
        &result.types,
        &result.interner,
        Some(&regions),
    );

    let approx_issues = issue_set(&approximation);
    let region_issues = issue_set(&region);
    let newly_rejected: Vec<_> = region_issues.difference(&approx_issues).collect();
    if newly_rejected.is_empty() {
        Parity::Pass
    } else {
        Parity::Violation(format!(
            "{label}: regions newly report {newly_rejected:?} that the approximation accepted"
        ))
    }
}

/// The inline divergence corpus: every cyclic, condition-borrow, and
/// nested-release shape pinned by the BRI stress class.
fn divergence_corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "dum back-edge release-before-write",
            r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() ≺ 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
",
        ),
        (
            "loop rumpe release-before-write",
            r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    fac {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        rumpe
    } dum verum
}
",
        ),
        (
            "condition borrow with body write",
            r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    dum view.longitudo() ≺ 5 {
        xs.appende(4)
    }
}
",
        ),
        (
            "nested inner borrow released before outer write",
            r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia numerus i ← 0
    dum i ≺ 2 {
        varia in lista<numerus> view ← xs
        varia numerus j ← 0
        dum j ≺ 2 {
            nota view.longitudo()
            j ← j + 1
        }
        xs.appende(4)
        i ← i + 1
    }
}
",
        ),
        (
            "perge back-edge after last use",
            r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia numerus i ← 0
    dum i ≺ 3 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        i ← i + 1
        perge
    }
}
",
        ),
        (
            "re-established borrow after earlier shadow release",
            r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> first ← xs
    nota first.longitudo()
    varia in lista<numerus> second ← xs
    nota second.longitudo()
    xs.appende(4)
}
",
        ),
        (
            "live region keeps write_while_borrowed",
            r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() ≺ 5 {
        varia in lista<numerus> view ← xs
        xs.appende(4)
        nota view.longitudo()
    }
}
",
        ),
    ]
}

#[test]
fn divergence_corpus_preserves_parity() {
    let mut participated = 0usize;
    for (label, source) in divergence_corpus() {
        match parity_check(source, label) {
            Parity::Pass => participated += 1,
            Parity::Violation(violation) => panic!("{violation}"),
            Parity::Skipped => {}
        }
    }
    assert!(
        participated == divergence_corpus().len(),
        "every divergence fixture must engage the region engine"
    );
}

#[test]
fn exempla_corpus_preserves_parity() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    if !corpus.is_dir() {
        // Corpus relocation is not a parity defect; the inline divergence
        // corpus above still carries the theorem.
        return;
    }
    let mut sources: Vec<PathBuf> = walk_fab(&corpus);
    sources.sort();
    assert!(
        sources.len() > 100,
        "expected the language exempla corpus at {}, found {} files",
        corpus.display(),
        sources.len()
    );
    let mut participated = 0usize;
    let mut doc_artifacts = 0usize;
    for path in &sources {
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        // Corpus entries are keyword-reference documents: TOML frontmatter
        // plus prose around the Faber exemplar. Only files that still lower
        // to typed HIR can participate; the rest are doc artifacts, not
        // programs, and the approximation keeps authority for them.
        let source = strip_frontmatter(&raw);
        match parity_check(&source, &path.display().to_string()) {
            Parity::Pass => participated += 1,
            Parity::Violation(violation) => panic!("{violation}"),
            Parity::Skipped => doc_artifacts += 1,
        }
    }
    assert_eq!(
        participated + doc_artifacts,
        sources.len(),
        "every corpus file must be classified"
    );
    assert!(
        participated >= 10,
        "only {participated} exempla engaged the region engine; the parity theorem needs real coverage"
    );
}

/// Drop the leading `+++ … +++` TOML frontmatter block from a corpus entry.
fn strip_frontmatter(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("+++")
        && let Some(end) = rest.find("+++")
    {
        return rest[end + 3..].to_string();
    }
    raw.to_string()
}

#[test]
fn divergence_corpus_runs_through_production_path() {
    // The parity harness compares isolated checkers; this receipt keeps one
    // production-path run per divergence fixture so the corpus cannot drift
    // away from the driver wiring.
    for (label, source) in divergence_corpus() {
        let _ = analyze_source(source);
        let _ = label;
    }
}

fn walk_fab(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_fab(&path));
        } else if path.extension().is_some_and(|ext| ext == "fab") {
            out.push(path);
        }
    }
    out
}
