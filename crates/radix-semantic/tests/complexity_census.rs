//! CC-U4 complexity census: per-function cyclomatic scores over the two
//! declared Faber trees — `radix/corpus/` and in-tree `lib/norma/src/` —
//! using CC-U1's pure scoring function.
//!
//! `#[ignore]`d by default: this is a measurement harness, not a regression
//! test. Run with
//! `cargo test -p radix-semantic --test complexity_census -- --ignored
//! --nocapture` to print the tables live; the harness also writes the full
//! markdown block to `<target-dir>/tmp/complexity-census.md` (the
//! cargo-managed temp root, so `cargo clean` sweeps it). Output is
//! deterministic for a fixed tree state: files are walked in sorted order and
//! rows keep declaration order, so the same trees reprint the same tables.
//!
//! Files that fail to analyze are listed with their diagnostic codes, never
//! silently skipped — the failure list is part of the census artifact.
//!
//! Postures (delivery.md CC-U4 read_scope):
//! - `radix/corpus/`: driver single-unit analysis per file.
//! - `lib/norma/src/`: package-analysis posture — each module is resolved
//!   through the production `LibraryResolver` (in-tree `lib/` fallback) and
//!   analyzed in-process through the radix-program library cache (the S8.5
//!   Norma-graph analysis path), so intra-tree `norma:*` imports resolve
//!   exactly as a consuming package resolves them. This harness only reads
//!   the tree.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use radix::module::{AnalyzeRequest, Session, analyze_source};
use radix::program::LibraryImportBinding;
use radix::program::library::{
    LibraryInterfaceCache, LibraryResolver, with_library_cached_analysis_mut,
};
use radix::{Config, Target};
use radix_diagnostics::Span;
use radix_hir::{HirFunction, HirItemKind, HirModule};
use radix_lexer::Interner;
use radix_semantic::passes::complexity::function_complexity;
use radix_syntax::Visibility;

/// Candidate baked-default thresholds the census reports over-budget counts
/// for (delivery.md CC-U4 done_when).
const THRESHOLDS: [u32; 4] = [8, 10, 12, 15];

/// One scored declared function (bodyless functions are never scored).
#[derive(Debug, Clone)]
struct FunctionRow {
    /// Tree-relative file path, forward slashes.
    file: String,
    /// Top-level name or `Genus.method`.
    name: String,
    kernel: bool,
    score: u32,
}

/// One file the census could not analyze, with its leading diagnostic facts.
#[derive(Debug, Clone)]
struct FailureRow {
    file: String,
    reasons: String,
}

/// Census state for one tree.
#[derive(Debug)]
struct TreeCensus {
    name: &'static str,
    files_attempted: usize,
    files_scored: usize,
    rows: Vec<FunctionRow>,
    failures: Vec<FailureRow>,
}

impl TreeCensus {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            files_attempted: 0,
            files_scored: 0,
            rows: Vec::new(),
            failures: Vec::new(),
        }
    }

    fn scores(&self) -> Vec<u32> {
        let mut scores: Vec<u32> = self.rows.iter().map(|row| row.score).collect();
        scores.sort_unstable();
        scores
    }

    fn kernel_scores(&self) -> Vec<u32> {
        let mut scores: Vec<u32> = self
            .rows
            .iter()
            .filter(|row| row.kernel)
            .map(|row| row.score)
            .collect();
        scores.sort_unstable();
        scores
    }

    fn over_budget(&self, threshold: u32) -> usize {
        self.rows.iter().filter(|row| row.score > threshold).count()
    }

    fn kernel_over_budget(&self, threshold: u32) -> usize {
        self.rows
            .iter()
            .filter(|row| row.kernel && row.score > threshold)
            .count()
    }
}

/// Nearest-rank percentile: the ceil(p/100 · n)-th smallest value (1-indexed),
/// clamped into range. The median is the upper median under this rule.
fn nearest_rank(sorted: &[u32], pct: u32) -> Option<u32> {
    let count = sorted.len();
    if count == 0 {
        return None;
    }
    let rank = ((count as u64) * u64::from(pct))
        .div_ceil(100)
        .clamp(1, count as u64) as usize;
    Some(sorted[rank - 1])
}

fn fmt_stat(sorted: &[u32], pct: u32) -> String {
    nearest_rank(sorted, pct).map_or_else(|| "—".to_owned(), |value| value.to_string())
}

/// Score every declared function with a body in one analyzed unit.
///
/// Mirrors the pass's own population exactly (`passes::complexity::complexity`):
/// top-level functions with `def_id < 1_000_000` (compiler-synthesized lifted
/// closures excluded) and genus methods; bodyless interface methods return
/// `None` from the pure scoring function and are skipped here too.
fn score_hir_functions(hir: &HirModule, interner: &Interner, file: &str) -> Vec<FunctionRow> {
    let mut rows = Vec::new();
    let mut consider = |prefix: &str, func: &HirFunction| {
        if let Some(score) = function_complexity(func) {
            rows.push(FunctionRow {
                file: file.to_owned(),
                name: format!("{prefix}{}", interner.resolve(func.name)),
                kernel: func.is_kernel,
                score,
            });
        }
    };
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) if item.def_id.0 < 1_000_000 => consider("", func),
            HirItemKind::Struct(strukt) => {
                let prefix = format!("{}.", interner.resolve(strukt.name));
                for method in &strukt.methods {
                    consider(&prefix, &method.func);
                }
            }
            _ => {}
        }
    }
    rows
}

/// Compact, sorted, deduped diagnostic facts for the honest failure list.
fn summarize_diagnostics(diagnostics: &[radix::diagnostics::Diagnostic]) -> String {
    let mut facts: Vec<String> = diagnostics
        .iter()
        .map(|diag| match (diag.code, diag.issue()) {
            (Some(code), Some(issue)) => format!("{code}:{issue}"),
            (Some(code), None) => code.to_owned(),
            (None, Some(issue)) => issue.to_owned(),
            (None, None) => "uncoded".to_owned(),
        })
        .collect();
    facts.sort();
    facts.dedup();
    let shown: Vec<&str> = facts.iter().take(3).map(String::as_str).collect();
    if facts.len() > shown.len() {
        format!("{} (+{} more)", shown.join(", "), facts.len() - shown.len())
    } else {
        shown.join(", ")
    }
}

/// Every `.fab` under `root`, sorted by root-relative path (deterministic).
fn fab_files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut entries = Vec::new();
    collect_fab_files(root, root, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn collect_fab_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fab_files(root, &path, out);
        } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("fab") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, path));
        }
    }
}

/// The radix checkout's corpus root (this packet's own tree).
fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Corpus tree: driver single-unit analysis per file. Reject/negative exempla that fail analysis are honest failure
/// rows, not skips.
fn census_corpus() -> TreeCensus {
    let mut census = TreeCensus::new("radix/corpus");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_dev_locale_root(),
    );
    for (rel, file) in fab_files(&corpus_root()) {
        census.files_attempted += 1;
        let Ok(source) = fs::read_to_string(&file) else {
            census.failures.push(FailureRow {
                file: rel,
                reasons: "unreadable".to_owned(),
            });
            continue;
        };
        let name = file.display().to_string();
        match analyze_source(&session, &name, &source, AnalyzeRequest::default()) {
            Ok(unit) => {
                census
                    .rows
                    .extend(score_hir_functions(&unit.hir, &unit.interner, &rel));
                census.files_scored += 1;
            }
            Err(diagnostics) => census.failures.push(FailureRow {
                file: rel,
                reasons: summarize_diagnostics(&diagnostics),
            }),
        }
    }
    census
}

/// Dev-tree reader locale pack for the library cache seam (mirrors
/// `radix-program` `test_locale_pack`: every norma module declares its
/// locale in frontmatter, and the loader loads it from `locale/`).
fn dev_locale_pack(
    _input: &Path,
    locale: Option<&str>,
) -> Result<Option<radix::module::locale::LocalePack>, Box<radix::diagnostics::Diagnostic>> {
    let locale = locale.unwrap_or("en");
    let path = radix_semantic::locale::workspace_locale_pack_path(locale);
    let pack = radix::module::locale::LocalePack::from_toml_path(path).map_err(|err| {
        Box::new(radix::diagnostics::Diagnostic::error(format!(
            "failed to load reader locale '{locale}' pack: {err}"
        )))
    })?;
    Ok(Some(pack))
}

/// The in-tree Norma package (`lib/norma/src`) through the production
/// resolver + radix-program library cache: per-module full analysis with
/// intra-tree `norma:*` imports resolved as a consuming package would.
fn census_norma_tree() -> TreeCensus {
    const PROVIDER: &str = "norma";
    let mut census = TreeCensus::new("lib/norma/src");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source_root = repo_root.join("lib").join(PROVIDER).join("src");
    assert!(
        source_root.is_dir(),
        "census: {PROVIDER} source tree missing at {}",
        source_root.display()
    );
    // No explicit home: the resolver falls back to the in-tree `<repo>/lib`.
    let resolver = LibraryResolver::with_optional_home(None);
    let mut cache =
        LibraryInterfaceCache::with_config(&Config::default().with_target(Target::HirRust))
            .with_locale_loader(dev_locale_pack);

    for (rel, _file) in fab_files(&source_root) {
        census.files_attempted += 1;
        let specifier = format!("{}:{}", PROVIDER, rel.trim_end_matches(".fab"));
        let module = match resolver.resolve(&specifier) {
            Ok(Some(module)) => module,
            Ok(None) => {
                census.failures.push(FailureRow {
                    file: rel,
                    reasons: format!("resolver did not claim {specifier:?}"),
                });
                continue;
            }
            Err(err) => {
                census.failures.push(FailureRow {
                    file: rel,
                    reasons: format!("resolve {specifier}: {err:?}"),
                });
                continue;
            }
        };
        let binding = LibraryImportBinding {
            binding: module
                .module_name()
                .unwrap_or(module.package.as_str())
                .to_owned(),
            visibility: Visibility::Privata,
            import_span: Span::default(),
            module,
        };
        let mut rows = Vec::new();
        let analyzed =
            with_library_cached_analysis_mut(&binding, &resolver, &mut cache, |analysis, _| {
                rows = score_hir_functions(&analysis.hir, &analysis.interner, &rel);
                Ok(())
            });
        match analyzed {
            Ok(()) => {
                census.rows.extend(rows);
                census.files_scored += 1;
            }
            Err(diagnostic) => census.failures.push(FailureRow {
                file: rel,
                reasons: format!(
                    "{} {}",
                    diagnostic.code.unwrap_or("<none>"),
                    diagnostic.issue().unwrap_or("analysis failed")
                ),
            }),
        }
    }
    census
}

fn distribution_row(
    label: &str,
    files: usize,
    scored: usize,
    failed: usize,
    sorted: &[u32],
) -> String {
    format!(
        "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
        label,
        files,
        scored,
        failed,
        sorted.len(),
        fmt_stat(sorted, 0),
        fmt_stat(sorted, 50),
        fmt_stat(sorted, 90),
        fmt_stat(sorted, 99),
        fmt_stat(sorted, 100),
    )
}

/// The whole measured block, rendered as standalone markdown sections.
fn render_census(trees: &[TreeCensus]) -> String {
    let mut out = String::new();

    out.push_str("## Distribution per tree\n\n");
    out.push_str("| Tree | .fab files | files scored | files failed | functions scored | min | median | p90 | p99 | max |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for tree in trees {
        let scores = tree.scores();
        let row = distribution_row(
            tree.name,
            tree.files_attempted,
            tree.files_scored,
            tree.failures.len(),
            &scores,
        );
        out.push_str(&row);
        out.push('\n');
    }
    let pooled: Vec<u32> = {
        let mut all: Vec<u32> = trees.iter().flat_map(TreeCensus::scores).collect();
        all.sort_unstable();
        all
    };
    let pooled_row = distribution_row(
        "all trees pooled",
        trees.iter().map(|tree| tree.files_attempted).sum(),
        trees.iter().map(|tree| tree.files_scored).sum(),
        trees.iter().map(|tree| tree.failures.len()).sum(),
        &pooled,
    );
    out.push_str(&pooled_row);
    out.push('\n');

    out.push_str("\n## Kernel subset (`HirFunction.is_kernel`)\n\n");
    out.push_str("| Scope | kernel functions | min | median | p90 | p99 | max |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for tree in trees {
        let kernel = tree.kernel_scores();
        let row = if kernel.is_empty() {
            format!("| {} (kernel) | 0 | — | — | — | — | — |", tree.name)
        } else {
            format!(
                "| {} (kernel) | {} | {} | {} | {} | {} | {} |",
                tree.name,
                kernel.len(),
                fmt_stat(&kernel, 0),
                fmt_stat(&kernel, 50),
                fmt_stat(&kernel, 90),
                fmt_stat(&kernel, 99),
                fmt_stat(&kernel, 100),
            )
        };
        out.push_str(&row);
        out.push('\n');
    }
    let pooled_kernel: Vec<u32> = {
        let mut all: Vec<u32> = trees.iter().flat_map(TreeCensus::kernel_scores).collect();
        all.sort_unstable();
        all
    };
    let pooled_kernel_row = if pooled_kernel.is_empty() {
        "| all trees pooled (kernel) | 0 | — | — | — | — | — |".to_owned()
    } else {
        format!(
            "| all trees pooled (kernel) | {} | {} | {} | {} | {} | {} |",
            pooled_kernel.len(),
            fmt_stat(&pooled_kernel, 0),
            fmt_stat(&pooled_kernel, 50),
            fmt_stat(&pooled_kernel, 90),
            fmt_stat(&pooled_kernel, 99),
            fmt_stat(&pooled_kernel, 100),
        )
    };
    out.push_str(&pooled_kernel_row);
    out.push('\n');

    out.push_str("\n## Over-budget function counts (score > threshold)\n\n");
    let header: Vec<String> = THRESHOLDS
        .iter()
        .map(|threshold| format!("> {threshold}"))
        .collect();
    let _ = writeln!(out, "| Scope | {} |", header.join(" | "));
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for tree in trees {
        let counts: Vec<String> = THRESHOLDS
            .iter()
            .map(|threshold| tree.over_budget(*threshold).to_string())
            .collect();
        let _ = writeln!(out, "| {} | {} |", tree.name, counts.join(" | "));
    }
    let kernel_counts: Vec<String> = THRESHOLDS
        .iter()
        .map(|threshold| {
            trees
                .iter()
                .map(|tree| tree.kernel_over_budget(*threshold))
                .sum::<usize>()
                .to_string()
        })
        .collect();
    let _ = writeln!(
        out,
        "| kernel subset (pooled) | {} |",
        kernel_counts.join(" | ")
    );

    out.push_str("\n## Files not scored (honest failure list)\n\n");
    let any_failure = trees.iter().any(|tree| !tree.failures.is_empty());
    if any_failure {
        out.push_str("| Tree | File | Leading diagnostics |\n");
        out.push_str("| --- | --- | --- |\n");
        for tree in trees {
            for failure in &tree.failures {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} |",
                    tree.name, failure.file, failure.reasons
                );
            }
        }
    } else {
        out.push_str("None — every attempted file analyzed.\n");
    }

    out.push_str("\n## Per-function rows\n\n");
    out.push_str("| Tree | File | Function | Kernel | Score |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for tree in trees {
        for row in &tree.rows {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                tree.name,
                row.file,
                row.name,
                if row.kernel { "yes" } else { "no" },
                row.score
            );
        }
    }
    out
}

#[test]
#[ignore = "CC-U4 census harness: run with `-- --ignored --nocapture` to print the tables"]
fn complexity_census_over_corpus_norma() {
    let trees = vec![census_corpus(), census_norma_tree()];

    // Structural self-checks: the census must have measured real populations
    // and its bookkeeping must close (scored + failed == attempted per tree).
    for tree in &trees {
        assert!(
            tree.files_attempted > 0,
            "census tree {} enumerated no files",
            tree.name
        );
        assert_eq!(
            tree.files_scored + tree.failures.len(),
            tree.files_attempted,
            "census tree {} bookkeeping must close",
            tree.name
        );
        assert!(
            !tree.rows.is_empty(),
            "census tree {} scored zero functions (artifact would be vacuous)",
            tree.name
        );
    }
    assert!(
        trees[0].files_attempted >= 460,
        "corpus baseline floor is 460 files, got {}",
        trees[0].files_attempted
    );

    let markdown = render_census(&trees);
    println!(
        "\n===== COMPLEXITY CENSUS (CC-U4) =====\n{markdown}===== END COMPLEXITY CENSUS ====="
    );

    // Deterministic extraction copy under the cargo-managed temp root.
    let target_tmp = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.ancestors().nth(2).map(|target| target.join("tmp")))
        })
        .expect("census: cargo-managed temp root unreachable");
    fs::create_dir_all(&target_tmp).expect("create census temp root");
    let artifact = target_tmp.join("complexity-census.md");
    fs::write(&artifact, &markdown)
        .unwrap_or_else(|err| panic!("write {}: {err}", artifact.display()));
    println!("census tables written to {}", artifact.display());
}
